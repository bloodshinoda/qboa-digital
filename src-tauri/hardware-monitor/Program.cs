using System.Globalization;
using System.Management;
using System.Runtime.InteropServices;
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
        double? CpuUtilizationPercent, double? FrequencyMhz, string? FrequencySource);
    private sealed record FrequencyReading(double? Value, string? Source);

    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessorPowerInformation
    {
        public uint Number;
        public uint MaxMhz;
        public uint CurrentMhz;
        public uint MhzLimit;
        public uint MaxIdleState;
        public uint CurrentIdleState;
    }

    private const int ProcessorInformation = 11;

    [DllImport("powrprof.dll")]
    private static extern uint CallNtPowerInformation(
        int informationLevel,
        IntPtr inputBuffer,
        uint inputBufferLength,
        IntPtr outputBuffer,
        uint outputBufferLength);
    private sealed record GpuSensorInfo(string Name, string Identifier, double Value);
    private sealed record GpuDiscovery(bool Available, string Provider, IReadOnlyList<GpuSensorInfo> Sensors, IReadOnlyList<GpuSensorInfo> CpuTemperatureSensors, string Note);
    private sealed record GpuTelemetry(string TimestampUtc, double? TemperatureC, string? TemperatureSensor,
        string? TemperatureSource, double? UtilizationPercent, double? FrequencyMhz);

    public static int Main(string[] args)
    {
        var mode = args.FirstOrDefault()?.Trim().ToLowerInvariant();
        if (mode is not ("discover" or "stream" or "gpu-discover" or "gpu-stream" or "gpu-stream-cpu-fallback"))
        {
            Console.Error.WriteLine("Modo inválido. Use discover, stream, gpu-discover ou gpu-stream.");
            return 2;
        }

        try
        {
            if (mode is "gpu-discover" or "gpu-stream" or "gpu-stream-cpu-fallback")
            {
                using var gpuMonitor = new GpuHardwareSession();
                gpuMonitor.Open();
                if (mode == "gpu-discover")
                {
                    var sensors = gpuMonitor.ReadGpuTemperatures();
                    var cpuSensors = gpuMonitor.ReadCpuTemperatures();
                    var available = sensors.Count > 0;
                    var note = available
                        ? "Sensores térmicos próprios da GPU detectados pelo coletor integrado."
                        : cpuSensors.Count > 0
                            ? "Nenhum sensor próprio da GPU foi encontrado; sensores térmicos da CPU estão disponíveis como referência alternativa."
                            : "Não foram encontrados sensores térmicos válidos na GPU nem na CPU.";
                    Console.WriteLine(JsonSerializer.Serialize(new GpuDiscovery(available, Provider, sensors, cpuSensors, note), JsonOptions));
                    return 0;
                }

                while (true)
                {
                    Console.WriteLine(JsonSerializer.Serialize(gpuMonitor.ReadTelemetry(mode == "gpu-stream-cpu-fallback"), JsonOptions));
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
                var frequency = ReadCpuFrequency(monitor);
                var sample = new Telemetry(DateTimeOffset.UtcNow.ToString("O", CultureInfo.InvariantCulture),
                    selected?.Value, selected?.Name, ReadCpuUtilization(), frequency.Value, frequency.Source);
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
        private readonly Computer _computer = new() { IsGpuEnabled = true, IsCpuEnabled = true };
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

        public IReadOnlyList<GpuSensorInfo> ReadCpuTemperatures()
        {
            foreach (var hardware in _computer.Hardware.Where(item => item.HardwareType == HardwareType.Cpu))
                Update(hardware);
            return _computer.Hardware
                .Where(item => item.HardwareType == HardwareType.Cpu)
                .SelectMany(Enumerate)
                .SelectMany(item => item.Sensors)
                .Where(sensor => sensor.SensorType == SensorType.Temperature && sensor.Value.HasValue)
                .Select(sensor => new GpuSensorInfo(sensor.Name, sensor.Identifier.ToString(), sensor.Value!.Value))
                .Where(sensor => double.IsFinite(sensor.Value) && sensor.Value is > 0 and <= 150)
                .ToArray();
        }

        public GpuTelemetry ReadTelemetry(bool allowCpuFallback)
        {
            var gpuSensors = UpdatedGpuHardware().SelectMany(item => item.Sensors)
                .Where(sensor => sensor.Value.HasValue && double.IsFinite(sensor.Value.Value))
                .ToArray();
            var gpuTemperature = allowCpuFallback
                ? null
                : gpuSensors.Where(sensor => sensor.SensorType == SensorType.Temperature &&
                        sensor.Value is > 0 and <= 150)
                    .OrderByDescending(sensor => sensor.Value).FirstOrDefault();
            var cpuTemperature = allowCpuFallback
                ? ReadCpuTemperatures().OrderByDescending(sensor => sensor.Value).FirstOrDefault()
                : null;
            var temperature = gpuTemperature?.Value ?? cpuTemperature?.Value;
            var temperatureSensor = gpuTemperature?.Name ?? cpuTemperature?.Name;
            var temperatureSource = gpuTemperature is not null ? "gpu" : cpuTemperature is not null ? "cpu" : null;
            var utilization = gpuSensors.Where(sensor => sensor.SensorType == SensorType.Load &&
                    sensor.Value is >= 0 and <= 100)
                .OrderByDescending(sensor => sensor.Name.Contains("Core", StringComparison.OrdinalIgnoreCase) ? 0 : 1)
                .FirstOrDefault();
            var frequency = gpuSensors.Where(sensor => sensor.SensorType == SensorType.Clock &&
                    sensor.Value is > 0 and < 100000)
                .OrderByDescending(sensor => sensor.Value).FirstOrDefault();
            return new GpuTelemetry(DateTimeOffset.UtcNow.ToString("O", CultureInfo.InvariantCulture),
                temperature, temperatureSensor, temperatureSource, utilization?.Value, frequency?.Value);
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

        public IReadOnlyList<SensorInfo> ReadCpuClocks()
        {
            foreach (var hardware in _computer.Hardware.Where(item => item.HardwareType == HardwareType.Cpu))
                UpdateRecursively(hardware);
            return _computer.Hardware
                .Where(item => item.HardwareType == HardwareType.Cpu)
                .SelectMany(EnumerateHardware)
                .SelectMany(item => item.Sensors)
                .Where(sensor => sensor.SensorType == SensorType.Clock &&
                    sensor.Value.HasValue &&
                    sensor.Name.Contains("Core", StringComparison.OrdinalIgnoreCase))
                .Select(sensor => new SensorInfo(sensor.Name, sensor.Identifier.ToString(), sensor.Value!.Value))
                .Where(sensor => double.IsFinite(sensor.Value) && sensor.Value is > 0 and < 10000)
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

    private static FrequencyReading ReadCpuFrequency(HardwareSession monitor)
    {
        var coreClocks = monitor.ReadCpuClocks();
        if (coreClocks.Count > 0)
        {
            return new FrequencyReading(
                coreClocks.Average(sensor => sensor.Value),
                "LibreHardwareMonitor · média dos clocks dos núcleos");
        }

        var processorCount = Environment.ProcessorCount;
        var itemSize = Marshal.SizeOf<ProcessorPowerInformation>();
        var bufferSize = checked((uint)(processorCount * itemSize));
        var buffer = Marshal.AllocHGlobal((int)bufferSize);
        try
        {
            var status = CallNtPowerInformation(ProcessorInformation, IntPtr.Zero, 0, buffer, bufferSize);
            if (status == 0)
            {
                var readings = Enumerable.Range(0, processorCount)
                    .Select(index => Marshal.PtrToStructure<ProcessorPowerInformation>(IntPtr.Add(buffer, index * itemSize)))
                    .Select(item => item.CurrentMhz)
                    .Where(value => value > 0)
                    .Select(value => (double)value)
                    .ToArray();
                if (readings.Length > 0)
                {
                    return new FrequencyReading(
                        readings.Average(),
                        "Windows CallNtPowerInformation · média dos processadores lógicos");
                }
            }
        }
        catch { }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }

        // Fallback legado: mantém a telemetria disponível em sistemas nos quais
        // CallNtPowerInformation não retorna CurrentMhz.
        try
        {
            using var searcher = new ManagementObjectSearcher(
                "SELECT ProcessorFrequency FROM Win32_PerfFormattedData_Counters_ProcessorInformation WHERE Name='_Total'");
            using var results = searcher.Get();
            foreach (ManagementObject item in results)
            {
                using (item)
                {
                    var value = Convert.ToDouble(item["ProcessorFrequency"], CultureInfo.InvariantCulture);
                    if (value > 0)
                        return new FrequencyReading(value, "WMI ProcessorInformation · fallback");
                }
            }
        }
        catch { }
        return new FrequencyReading(null, null);
    }
}
