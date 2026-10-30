using System.Globalization;
using System.Management;
using System.Text.Json;
using Microsoft.Win32;
using LibreHardwareMonitor.Hardware;

namespace QBoa.HardwareMonitor;

internal static class Program
{
    private const string Provider = "LibreHardwareMonitorLib 0.9.6 (integrado)";
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = false
    };

    private sealed record SensorInfo(string Name, string Identifier, double Value);
    private sealed record Discovery(bool Available, string Provider, IReadOnlyList<SensorInfo> Sensors, string Note, bool PawnioInstalled, string? PawnioVersion);
    private sealed record Telemetry(string TimestampUtc, double? TemperatureC, string? TemperatureSensor,
        double? CpuUtilizationPercent, double? FrequencyMhz);

    public static int Main(string[] args)
    {
        var mode = args.FirstOrDefault()?.Trim().ToLowerInvariant();
        if (mode is not ("discover" or "stream"))
        {
            Console.Error.WriteLine("Modo inválido. Use discover ou stream.");
            return 2;
        }

        try
        {
            using var monitor = new HardwareSession();
            monitor.Open();
            if (mode == "discover")
            {
                var sensors = monitor.ReadCpuTemperatures();
                var pawnio = ReadPawnIoInstallation();
                var available = pawnio.Installed && sensors.Count > 0;
                var note = !pawnio.Installed
                    ? "O driver PawnIO não está instalado. Ele é necessário para que o LibreHardwareMonitor acesse determinados sensores térmicos da CPU. Instale a versão oficial em https://github.com/namazso/PawnIO.Setup/releases/latest e atualize esta tela. O teste não será iniciado."
                    : sensors.Count > 0
                        ? $"Sensores térmicos de CPU detectados pelo coletor integrado. PawnIO {pawnio.Version ?? "instalado"}."
                        : $"PawnIO {pawnio.Version ?? "está instalado"}, mas o coletor não encontrou leituras térmicas válidas de CPU neste hardware. O teste não será iniciado.";
                Console.WriteLine(JsonSerializer.Serialize(new Discovery(
                    available, Provider, sensors, note, pawnio.Installed, pawnio.Version
                ), JsonOptions));
                return 0;
            }

            while (true)
            {
                var sensors = monitor.ReadCpuTemperatures();
                var selected = sensors.OrderByDescending(sensor => sensor.Value).FirstOrDefault();
                var sample = new Telemetry(DateTimeOffset.UtcNow.ToString("O", CultureInfo.InvariantCulture),
                    selected?.Value, selected?.Name, ReadCpuUtilization(), ReadCpuFrequency());
                Console.WriteLine(JsonSerializer.Serialize(sample, JsonOptions));
                Console.Out.Flush();
                Thread.Sleep(TimeSpan.FromSeconds(1));
            }
        }
        catch (Exception exception)
        {
            Console.Error.WriteLine(exception.ToString());
            return 1;
        }
    }

    private static (bool Installed, string? Version) ReadPawnIoInstallation()
    {
        const string uninstallKey = @"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO";
        try
        {
            using var key = Registry.LocalMachine.OpenSubKey(uninstallKey);
            if (key is null)
                return (false, null);

            var version = key.GetValue("DisplayVersion") as string;
            return (true, string.IsNullOrWhiteSpace(version) ? null : version.Trim());
        }
        catch (UnauthorizedAccessException)
        {
            // A leitura de HKLM normalmente é permitida; falha de acesso não deve
            // ser interpretada como ausência comprovada do driver.
            return (false, null);
        }
    }

    private sealed class HardwareSession : IDisposable
    {
        private readonly Computer _computer = new() { IsCpuEnabled = true };
        public void Open() => _computer.Open();

        public IReadOnlyList<SensorInfo> ReadCpuTemperatures()
        {
            foreach (var hardware in _computer.Hardware.Where(item => item.HardwareType == HardwareType.Cpu))
                UpdateRecursively(hardware);
            return _computer.Hardware
                .Where(item => item.HardwareType == HardwareType.Cpu)
                .SelectMany(EnumerateHardware)
                .SelectMany(item => item.Sensors)
                .Where(sensor => sensor.SensorType == SensorType.Temperature && sensor.Value.HasValue)
                .Select(sensor => new SensorInfo(sensor.Name, sensor.Identifier.ToString(), sensor.Value!.Value))
                .Where(sensor => double.IsFinite(sensor.Value) && sensor.Value is > 0 and <= 150)
                .ToArray();
        }

        private static IEnumerable<IHardware> EnumerateHardware(IHardware hardware)
        {
            yield return hardware;
            foreach (var child in hardware.SubHardware)
                foreach (var nested in EnumerateHardware(child))
                    yield return nested;
        }

        private static void UpdateRecursively(IHardware hardware)
        {
            hardware.Update();
            foreach (var child in hardware.SubHardware)
                UpdateRecursively(child);
        }

        public void Dispose() => _computer.Close();
    }

    private static double? ReadCpuUtilization()
    {
        try
        {
            using var searcher = new ManagementObjectSearcher(
                "SELECT PercentProcessorTime FROM Win32_PerfFormattedData_PerfOS_Processor WHERE Name='_Total'");
            using var results = searcher.Get();
            foreach (ManagementObject item in results)
            {
                using (item) return Convert.ToDouble(item["PercentProcessorTime"], CultureInfo.InvariantCulture);
            }
        }
        catch { }
        return null;
    }

    private static double? ReadCpuFrequency()
    {
        try
        {
            using var searcher = new ManagementObjectSearcher(
                "SELECT ProcessorFrequency FROM Win32_PerfFormattedData_Counters_ProcessorInformation WHERE Name='_Total'");
            using var results = searcher.Get();
            foreach (ManagementObject item in results)
            {
                using (item) return Convert.ToDouble(item["ProcessorFrequency"], CultureInfo.InvariantCulture);
            }
        }
        catch { }
        return null;
    }
}
