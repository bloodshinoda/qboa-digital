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
    private sealed record GpuSensorInfo(string Name, string Identifier, double Value);
    private sealed record GpuDiscovery(bool Available, string Provider, IReadOnlyList<GpuSensorInfo> Sensors, string Note);
    private sealed record GpuTelemetry(string TimestampUtc, double? TemperatureC, string? TemperatureSensor,
        double? UtilizationPercent, double? FrequencyMhz);

    public static int Main(string[] args)
    {
        var mode = args.FirstOrDefault()?.Trim().ToLowerInvariant();
        if (mode is not ("discover" or "stream" or "gpu-discover" or "gpu-stream"))
        {
            Console.Error.WriteLine("Modo inválido. Use discover, stream, gpu-discover ou gpu-stream.");
            return 2;
        }

        try
        {
            if (mode is "gpu-discover" or "gpu-stream")
            {
                using var gpuMonitor = new GpuHardwareSession();
                gpuMonitor.Open();
                if (mode == "gpu-discover")
                {
                    var sensors = gpuMonitor.ReadGpuTemperatures();
                    var available = sensors.Count > 0;
                    var note = available
                        ? "Sensores térmicos de GPU detectados pelo coletor integrado."
                        : "O coletor integrado não encontrou sensores térmicos válidos de GPU neste hardware. O teste de estabilidade não será iniciado.";
                    Console.WriteLine(JsonSerializer.Serialize(new GpuDiscovery(available, Provider, sensors, note), JsonOptions));
                    return 0;
                }

                while (true)
                {
                    Console.WriteLine(JsonSerializer.Serialize(gpuMonitor.ReadTelemetry(), JsonOptions));
                    Console.Out.Flush();
                    Thread.Sleep(TimeSpan.FromSeconds(1));
                }
            }

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
        const string uninstallPath = @"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
        try
        {
            using var machine = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64);
            using var uninstall = machine.OpenSubKey(uninstallPath);
            if (uninstall is null)
                return (false, null);

            foreach (var subKeyName in uninstall.GetSubKeyNames())
            {
                using var appKey = uninstall.OpenSubKey(subKeyName);
                var displayName = appKey?.GetValue("DisplayName") as string;
                if (!(displayName?.Trim().StartsWith("PawnIO", StringComparison.OrdinalIgnoreCase) ?? false))
                    continue;

                var version = appKey?.GetValue("DisplayVersion") as string;
                return (true, string.IsNullOrWhiteSpace(version) ? null : version.Trim());
            }
            return (false, null);
        }
        catch (UnauthorizedAccessException)
        {
            // Não interpretar uma falha de acesso como ausência comprovada do driver.
            return (false, null);
        }
    }


    private sealed class GpuHardwareSession : IDisposable
    {
        private readonly Computer _computer = new() { IsGpuEnabled = true };
        public void Open() => _computer.Open();

        private static bool IsGpu(IHardware hardware) =>
            hardware.HardwareType is HardwareType.GpuNvidia or HardwareType.GpuAmd or HardwareType.GpuIntel;

        private static IEnumerable<IHardware> Enumerate(IHardware hardware)
        {
            yield return hardware;
            foreach (var child in hardware.SubHardware)
                foreach (var nested in Enumerate(child))
                    yield return nested;
        }

        private static void Update(IHardware hardware)
        {
            hardware.Update();
            foreach (var child in hardware.SubHardware) Update(child);
        }

        private IEnumerable<IHardware> UpdatedGpuHardware()
        {
            var hardware = _computer.Hardware.Where(IsGpu).ToArray();
            foreach (var item in hardware) Update(item);
            return hardware.SelectMany(Enumerate).ToArray();
        }

        public IReadOnlyList<GpuSensorInfo> ReadGpuTemperatures() =>
            UpdatedGpuHardware().SelectMany(item => item.Sensors)
                .Where(sensor => sensor.SensorType == SensorType.Temperature && sensor.Value.HasValue)
                .Select(sensor => new GpuSensorInfo(sensor.Name, sensor.Identifier.ToString(), sensor.Value!.Value))
                .Where(sensor => double.IsFinite(sensor.Value) && sensor.Value is > 0 and <= 150)
                .ToArray();

        public GpuTelemetry ReadTelemetry()
        {
            var sensors = UpdatedGpuHardware().SelectMany(item => item.Sensors)
                .Where(sensor => sensor.Value.HasValue && double.IsFinite(sensor.Value.Value))
                .ToArray();
            var temperature = sensors.Where(sensor => sensor.SensorType == SensorType.Temperature &&
                    sensor.Value is > 0 and <= 150)
                .OrderByDescending(sensor => sensor.Value).FirstOrDefault();
            var utilization = sensors.Where(sensor => sensor.SensorType == SensorType.Load &&
                    sensor.Value is >= 0 and <= 100)
                .OrderByDescending(sensor => sensor.Name.Contains("Core", StringComparison.OrdinalIgnoreCase) ? 0 : 1)
                .FirstOrDefault();
            var frequency = sensors.Where(sensor => sensor.SensorType == SensorType.Clock &&
                    sensor.Value is > 0 and < 100000)
                .OrderByDescending(sensor => sensor.Value).FirstOrDefault();
            return new GpuTelemetry(DateTimeOffset.UtcNow.ToString("O", CultureInfo.InvariantCulture),
                temperature?.Value, temperature?.Name, utilization?.Value, frequency?.Value);
        }

        public void Dispose() => _computer.Close();
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
