$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$OutputEncoding = New-Object System.Text.UTF8Encoding($false)

$mode = [string]$env:QBOA_STORAGE_MODE
$diskId = [string]$env:QBOA_STORAGE_DISK_ID

$benchSource = @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

public sealed class QboaBenchResult {
    public long Bytes { get; set; }
    public long Operations { get; set; }
    public double DurationMs { get; set; }
}

public static class QboaStorageBench {
    private const uint GENERIC_READ = 0x80000000;
    private const uint GENERIC_WRITE = 0x40000000;
    private const uint FILE_SHARE_READ = 0x00000001;
    private const uint FILE_SHARE_WRITE = 0x00000002;
    private const uint OPEN_EXISTING = 3;
    private const uint FILE_FLAG_NO_BUFFERING = 0x20000000;
    private const uint FILE_FLAG_WRITE_THROUGH = 0x80000000;
    private static readonly IntPtr InvalidHandle = new IntPtr(-1);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetFilePointerEx(IntPtr handle, long distance, out long newPosition, uint method);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool ReadFile(IntPtr handle, IntPtr buffer, uint bytes, out uint transferred, IntPtr overlapped);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool WriteFile(IntPtr handle, IntPtr buffer, uint bytes, out uint transferred, IntPtr overlapped);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr handle);

    public static QboaBenchResult Run(string path, long fileBytes, int blockBytes, int queueDepth,
        bool write, bool randomAccess, long maxBytes, int maxMilliseconds) {
        return Run(path, fileBytes, blockBytes, queueDepth, write, randomAccess, maxBytes, maxMilliseconds, null);
    }

    public static QboaBenchResult Run(string path, long fileBytes, int blockBytes, int queueDepth,
        bool write, bool randomAccess, long maxBytes, int maxMilliseconds,
        Action<long, long, long, double, double> progress) {
        if (blockBytes <= 0 || blockBytes % 4096 != 0 || fileBytes < blockBytes ||
            fileBytes % 4096 != 0 || queueDepth < 1 || queueDepth > 32 ||
            maxBytes < blockBytes || maxMilliseconds < 1)
            throw new ArgumentException("Parâmetros de benchmark inválidos.");

        long reservedBytes = 0, completedBytes = 0, operations = 0, sequentialBlock = 0;
        int stop = 0;
        var failures = new Exception[queueDepth];
        var stopwatch = new Stopwatch();
        var workers = new Thread[queueDepth];
        var ready = new CountdownEvent(queueDepth);
        var startGate = new ManualResetEventSlim(false);

        for (int workerId = 0; workerId < queueDepth; workerId++) {
            int localId = workerId;
            workers[workerId] = new Thread(() => {
                IntPtr allocation = IntPtr.Zero;
                IntPtr aligned = IntPtr.Zero;
                IntPtr handle = InvalidHandle;
                bool announcedReady = false;
                try {
                    allocation = Marshal.AllocHGlobal(blockBytes + 4095);
                    long alignedAddress = (allocation.ToInt64() + 4095L) & ~4095L;
                    aligned = new IntPtr(alignedAddress);
                    for (int i = 0; i < blockBytes; i++)
                        Marshal.WriteByte(aligned, i, (byte)((i * 31 + localId * 17 + 0x5A) & 0xFF));

                    uint access = write ? GENERIC_WRITE : GENERIC_READ;
                    uint flags = FILE_FLAG_NO_BUFFERING | (write ? FILE_FLAG_WRITE_THROUGH : 0);
                    handle = CreateFileW(path, access, FILE_SHARE_READ | FILE_SHARE_WRITE,
                        IntPtr.Zero, OPEN_EXISTING, flags, IntPtr.Zero);
                    if (handle == InvalidHandle)
                        throw new Win32Exception(Marshal.GetLastWin32Error(), "Não foi possível abrir o arquivo de teste para E/S direta.");

                    var random = new Random(unchecked(Environment.TickCount ^ (localId * 7919)));
                    long maxBlocks = fileBytes / 4096;
                    ready.Signal();
                    announcedReady = true;
                    startGate.Wait();
                    while (Volatile.Read(ref stop) == 0 && stopwatch.ElapsedMilliseconds < maxMilliseconds) {
                        long current, reservation;
                        while (true) {
                            current = Interlocked.Read(ref reservedBytes);
                            reservation = current + blockBytes;
                            if (reservation > maxBytes) break;
                            if (Interlocked.CompareExchange(ref reservedBytes, reservation, current) == current) break;
                        }
                        if (reservation > maxBytes) break;

                        long offset;
                        if (randomAccess)
                            offset = (long)random.Next(0, (int)maxBlocks) * 4096L;
                        else {
                            long blockNumber = Interlocked.Increment(ref sequentialBlock) - 1;
                            offset = (blockNumber * blockBytes) % fileBytes;
                        }

                        long newPosition;
                        if (!SetFilePointerEx(handle, offset, out newPosition, 0)) {
                            int errorCode = Marshal.GetLastWin32Error();
                            throw new Win32Exception(errorCode,
                                "Falha ao posicionar a E/S direta (Win32 " + errorCode + "): " +
                                new Win32Exception(errorCode).Message);
                        }
                        uint transferred;
                        bool ok = write
                            ? WriteFile(handle, aligned, (uint)blockBytes, out transferred, IntPtr.Zero)
                            : ReadFile(handle, aligned, (uint)blockBytes, out transferred, IntPtr.Zero);
                        if (!ok || transferred != (uint)blockBytes)
                            throw new Win32Exception(Marshal.GetLastWin32Error(), "Falha na leitura/escrita direta do arquivo temporário.");

                        Interlocked.Add(ref completedBytes, transferred);
                        Interlocked.Increment(ref operations);
                    }

                } catch (Exception error) {
                    failures[localId] = error;
                    Interlocked.Exchange(ref stop, 1);
                } finally {
                    if (!announcedReady) ready.Signal();
                    if (handle != InvalidHandle) CloseHandle(handle);
                    if (allocation != IntPtr.Zero) Marshal.FreeHGlobal(allocation);
                }
            });
            workers[workerId].IsBackground = true;
            workers[workerId].Start();
        }

        ready.Wait();
        stopwatch.Start();
        startGate.Set();
        long previousBytes = 0;
        long previousOperations = 0;
        long previousTicks = Stopwatch.GetTimestamp();
        while (true) {
            bool anyAlive = false;
            foreach (Thread worker in workers) {
                if (worker.IsAlive) { anyAlive = true; break; }
            }
            if (!anyAlive) break;
            Thread.Sleep(150);
            long currentBytes = Interlocked.Read(ref completedBytes);
            long currentOperations = Interlocked.Read(ref operations);
            long currentTicks = Stopwatch.GetTimestamp();
            double intervalSeconds = (currentTicks - previousTicks) / (double)Stopwatch.Frequency;
            if (progress != null && intervalSeconds > 0 &&
                (currentBytes > previousBytes || currentOperations > previousOperations)) {
                try {
                    progress(stopwatch.ElapsedMilliseconds, currentBytes, currentOperations,
                        (currentBytes - previousBytes) / 1048576.0 / intervalSeconds,
                        (currentOperations - previousOperations) / intervalSeconds);
                } catch { progress = null; }
            }
            previousBytes = currentBytes;
            previousOperations = currentOperations;
            previousTicks = currentTicks;
        }
        foreach (Thread worker in workers) worker.Join();
        long finalBytes = Interlocked.Read(ref completedBytes);
        long finalOperations = Interlocked.Read(ref operations);
        long finalTicks = Stopwatch.GetTimestamp();
        double finalIntervalSeconds = (finalTicks - previousTicks) / (double)Stopwatch.Frequency;
        if (progress != null && finalIntervalSeconds > 0 &&
            (finalBytes > previousBytes || finalOperations > previousOperations)) {
            try {
                progress(stopwatch.ElapsedMilliseconds, finalBytes, finalOperations,
                    (finalBytes - previousBytes) / 1048576.0 / finalIntervalSeconds,
                    (finalOperations - previousOperations) / finalIntervalSeconds);
            } catch { }
        }
        stopwatch.Stop();
        ready.Dispose();
        startGate.Dispose();
        foreach (Exception failure in failures)
            if (failure != null) throw new InvalidOperationException(failure.Message, failure);
        if (completedBytes <= 0 || operations <= 0 || stopwatch.Elapsed.TotalSeconds <= 0)
            throw new InvalidOperationException("Nenhuma operação de E/S foi concluída.");
        return new QboaBenchResult {
            Bytes = completedBytes,
            Operations = operations,
            DurationMs = stopwatch.Elapsed.TotalMilliseconds
        };
    }
}
'@

function Get-DiskBusType($disk, $diskInfo, $physicalDisk) {
    $infoBus = ''
    $physicalBus = ''
    if ($null -ne $diskInfo -and $null -ne $diskInfo.BusType) { $infoBus = ([string]$diskInfo.BusType).Trim() }
    if ($null -ne $physicalDisk -and $null -ne $physicalDisk.BusType) { $physicalBus = ([string]$physicalDisk.BusType).Trim() }

    foreach ($candidate in @($infoBus, $physicalBus)) {
        if ($candidate -match '^NVMe$') { return 'NVMe' }
        if ($candidate -match '^USB$') { return 'USB' }
        if ($candidate -match '^SATA$') { return 'SATA' }
    }

    # O Windows pode expor NVMe como SCSI no Win32_DiskDrive e como Unknown no Get-Disk.
    $pnp = [string]$disk.PNPDeviceID
    if ($pnp -match '(?i)NVME') { return 'NVMe' }
    if ($pnp -match '(?i)(USBSTOR|USB\\VID_)') { return 'USB' }

    foreach ($candidate in @($infoBus, $physicalBus)) {
        if (-not [string]::IsNullOrWhiteSpace($candidate) -and $candidate -notmatch '^(Unknown|Unspecified|0)$') {
            return $candidate
        }
    }

    $interface = [string]$disk.InterfaceType
    if (-not [string]::IsNullOrWhiteSpace($interface) -and $interface -notmatch '^(Unknown|Unspecified)$') {
        return $interface
    }
    return 'Unknown'
}

function Find-PhysicalDisk($disk, $diskInfo, $physicalDisks) {
    $diskUniqueId = ''
    $diskSerial = ([string]$disk.SerialNumber).Trim()
    if ($null -ne $diskInfo -and $null -ne $diskInfo.UniqueId) { $diskUniqueId = ([string]$diskInfo.UniqueId).Trim() }

    if ($diskUniqueId) {
        $matches = @($physicalDisks | Where-Object {
            $null -ne $_.UniqueId -and ([string]$_.UniqueId).Trim().Equals($diskUniqueId, [StringComparison]::OrdinalIgnoreCase)
        })
        if ($matches.Count -eq 1) { return $matches[0] }
    }
    if ($diskSerial) {
        $matches = @($physicalDisks | Where-Object {
            $serial = ([string]$_.SerialNumber).Trim()
            $serial -and $serial.Equals($diskSerial, [StringComparison]::OrdinalIgnoreCase)
        })
        if ($matches.Count -eq 1) { return $matches[0] }
    }

    $number = [int]$disk.Index
    $byDeviceId = @($physicalDisks | Where-Object { "$($_.DeviceId)" -eq "$number" })
    if ($byDeviceId.Count -eq 1) { return $byDeviceId[0] }
    return $null
}

function Get-ClassForDisk($disk, $diskInfo, $physicalDisk) {
    $bus = Get-DiskBusType $disk $diskInfo $physicalDisk
    if ($bus -match '^USB$') { return 'usb' }
    if ($bus -match '^NVMe$') { return 'nvme' }

    if ($bus -match '^SATA$') {
        $media = ''
        if ($null -ne $physicalDisk -and $null -ne $physicalDisk.MediaType) {
            $media = ([string]$physicalDisk.MediaType).Trim()
        }
        if ($media -match '^SSD$') { return 'sata_ssd' }
        if ($media -match '^HDD$') {
            $computer = Get-CimInstance Win32_ComputerSystem -ErrorAction SilentlyContinue
            if ($null -ne $computer -and [int]$computer.PCSystemType -eq 2) { return 'notebook_hdd' }
            if ($null -ne $computer -and [int]$computer.PCSystemType -in @(1,3,4,5,6,7,8)) { return 'desktop_hdd' }
        }
    }
    return 'unknown'
}


function Get-DiskEntries {
    $disks = @(Get-CimInstance Win32_DiskDrive -ErrorAction Stop)
    $diskInfo = @(Get-Disk -ErrorAction SilentlyContinue)
    $physical = @(Get-PhysicalDisk -ErrorAction SilentlyContinue)
    $entries = @()

    foreach ($disk in $disks) {
        $number = [int]$disk.Index
        $info = $diskInfo | Where-Object { [int]$_.Number -eq $number } | Select-Object -First 1
        $pd = Find-PhysicalDisk $disk $info $physical
        $partitions = @(Get-Partition -DiskNumber $number -ErrorAction SilentlyContinue |
            Where-Object { $_.DriveLetter -and $_.Type -notin @('Reserved','System') })
        foreach ($partition in $partitions) {
            $volume = Get-Volume -Partition $partition -ErrorAction SilentlyContinue
            if ($null -eq $volume -or -not $volume.DriveLetter -or -not $volume.FileSystem -or $volume.FileSystem -eq 'CDFS') { continue }
            $letter = ([string]$volume.DriveLetter).ToUpperInvariant()
            if ($letter -notmatch '^[A-Z]$') { continue }
            $capacity = $null
            if ($null -ne $disk.Size -and [long]$disk.Size -gt 0) { $capacity = [long]$disk.Size }
            $volumeCapacity = $null
            if ($null -ne $volume.Size -and [long]$volume.Size -gt 0) { $volumeCapacity = [long]$volume.Size }
            $free = $null
            if ($null -ne $volume.SizeRemaining) { $free = [long]$volume.SizeRemaining }
            $entries += [pscustomobject]@{
                id = ([string]$number) + ':' + $letter
                backend = 'windows'
                disk_number = $number
                model = ([string]$disk.Model).Trim()
                capacity_bytes = $capacity
                volume_capacity_bytes = $volumeCapacity
                volume = $letter + ':\'
                free_bytes = $free
                class = Get-ClassForDisk $disk $info $pd
                bus_type = Get-DiskBusType $disk $info $pd
            }
        }
    }
    return $entries
}

function ConvertFrom-SmartVendorSpecific([byte[]]$Raw, [hashtable]$ThresholdMap) {
    if ($null -eq $Raw -or $Raw.Length -lt 14) {
        throw 'Payload VendorSpecific ausente ou menor que o cabeçalho SMART esperado.'
    }
    if ($null -eq $ThresholdMap) { $ThresholdMap = @{} }
    $names = @{
        1='Read Error Rate'; 3='Spin-Up Time'; 4='Start/Stop Count'; 5='Reallocated Sectors';
        7='Seek Error Rate'; 9='Power-On Hours'; 10='Spin Retry Count'; 12='Power Cycle Count';
        170='Available Reserved Space'; 171='Program Fail Count'; 172='Erase Fail Count';
        173='Wear Leveling Count'; 177='Wear Leveling Count'; 179='Used Reserved Block Count';
        181='Program Fail Count'; 182='Erase Fail Count'; 183='SATA Downshift'; 184='End-to-End Error';
        187='Reported Uncorrectable Errors'; 188='Command Timeout'; 190='Airflow Temperature';
        194='Temperature'; 195='ECC Error Rate'; 196='Reallocation Event Count';
        197='Current Pending Sector Count'; 198='Offline Uncorrectable'; 199='UDMA CRC Error Count';
        202='Percent Lifetime Remaining'; 231='SSD Life Left'; 232='Available Reserved Space';
        233='Media Wearout Indicator'; 241='Total LBAs Written'; 242='Total LBAs Read'
    }
    $attributes = @()
    for ($offset = 2; $offset + 11 -lt $Raw.Length; $offset += 12) {
        $id = [int]$Raw[$offset]
        if ($id -eq 0) { continue }
        [uint64]$rawValue = 0
        [uint64]$multiplier = 1
        for ($j = 0; $j -lt 6; $j++) {
            $rawValue += ([uint64]$Raw[$offset + 5 + $j] * $multiplier)
            $multiplier *= 256
        }
        $threshold = $null
        if ($ThresholdMap.ContainsKey($id)) { $threshold = [int]$ThresholdMap[$id] }
        $attributes += [pscustomobject]@{
            id = $id
            name = if ($names.ContainsKey($id)) { $names[$id] } else { 'SMART ' + $id }
            current = [int]$Raw[$offset + 3]
            worst = [int]$Raw[$offset + 4]
            threshold = $threshold
            raw_value = $rawValue
        }
    }
    return $attributes
}

function Get-SmartSectorAttributeStatus($Attribute, [string]$DataStatus, [string]$ParseStatus) {
    if ($null -ne $Attribute) { return 'available' }
    if ($ParseStatus -eq 'parse_error') { return 'parse_error' }
    if ($DataStatus -eq 'collection_error') { return 'collection_error' }
    if ($DataStatus -eq 'not_exposed') { return 'provider_unavailable' }
    return 'attribute_not_reported'
}

function Get-SmartSnapshot([int]$diskNumber) {
    $disk = Get-CimInstance Win32_DiskDrive -Filter ("Index = " + $diskNumber) -ErrorAction Stop
    $diskInfo = Get-Disk -Number $diskNumber -ErrorAction SilentlyContinue
    $physical = @(Get-PhysicalDisk -ErrorAction SilentlyContinue)
    $pd = Find-PhysicalDisk $disk $diskInfo $physical
    $counter = $null
    $errors = @()
    $counterStatus = 'not_exposed'
    try {
        if ($null -ne $diskInfo) {
            $counter = Get-StorageReliabilityCounter -Disk $diskInfo -ErrorAction Stop
        }
    } catch {
        $counterStatus = 'collection_error'
        $errors += ('Get-StorageReliabilityCounter (Disk): ' + $_.Exception.Message)
    }
    if ($null -eq $counter -and $null -ne $pd) {
        try {
            $counter = Get-StorageReliabilityCounter -PhysicalDisk $pd -ErrorAction Stop
        } catch {
            $counterStatus = 'collection_error'
            $errors += ('Get-StorageReliabilityCounter (PhysicalDisk): ' + $_.Exception.Message)
        }
    }
    if ($null -ne $counter) { $counterStatus = 'available' }

    $attributes = @()
    $reallocated = $null
    $pending = $null
    $uncorrectable = $null
    $crc = $null
    $temperature = $null
    $wear = $null
    $wearLimit = $null
    $predictedFailure = $null
    $sources = @()
    $reliability = @()
    $dataStatus = 'not_checked'
    $thresholdStatus = 'not_checked'
    $predictStatus = 'not_checked'
    $instanceStatus = 'not_checked'
    $parseStatus = 'not_checked'

    $pnpParts = ([string]$disk.PNPDeviceID).Split('\')
    $prefix = ''
    if ($pnpParts.Length -ge 2) { $prefix = ($pnpParts[0..1] -join '\') }

    $smartData = @()
    $smartThresholds = @()
    $smartStatus = @()
    try {
        $smartData = @(Get-CimInstance -Namespace root/wmi -ClassName MSStorageDriver_FailurePredictData -ErrorAction Stop |
            Where-Object { $prefix -and ([string]$_.InstanceName).StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) })
        $dataStatus = if ($smartData.Count -gt 0) { 'available' } else { 'not_exposed' }
        $instanceStatus = if ($smartData.Count -gt 0) { 'matched' } else { 'no_matching_instance' }
    } catch {
        $dataStatus = 'collection_error'
        $instanceStatus = 'query_error'
        $errors += ('MSStorageDriver_FailurePredictData: ' + $_.Exception.Message)
    }
    try {
        $smartThresholds = @(Get-CimInstance -Namespace root/wmi -ClassName MSStorageDriver_FailurePredictThresholds -ErrorAction Stop |
            Where-Object { $prefix -and ([string]$_.InstanceName).StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) })
        $thresholdStatus = if ($smartThresholds.Count -gt 0) { 'available' } else { 'not_exposed' }
    } catch {
        $thresholdStatus = 'collection_error'
        $errors += ('MSStorageDriver_FailurePredictThresholds: ' + $_.Exception.Message)
    }
    try {
        $smartStatus = @(Get-CimInstance -Namespace root/wmi -ClassName MSStorageDriver_FailurePredictStatus -ErrorAction Stop |
            Where-Object { $prefix -and ([string]$_.InstanceName).StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) })
        $predictStatus = if ($smartStatus.Count -gt 0) { 'available' } else { 'not_exposed' }
        if ($smartStatus.Count -gt 0) {
            $predictedFailure = [bool]$smartStatus[0].PredictFailure
            $sources += 'MSStorageDriver_FailurePredictStatus'
        }
    } catch {
        $predictStatus = 'collection_error'
        $errors += ('MSStorageDriver_FailurePredictStatus: ' + $_.Exception.Message)
    }

    if ($smartData.Count -gt 0) {
        try {
            $raw = $smartData[0].VendorSpecific
            $thresholdMap = @{}
            if ($smartThresholds.Count -gt 0) {
                $thresholdBytes = $smartThresholds[0].VendorSpecific
                if ($null -ne $thresholdBytes) {
                    for ($offset = 2; $offset + 11 -lt $thresholdBytes.Length; $offset += 12) {
                        $id = [int]$thresholdBytes[$offset]
                        if ($id -ne 0) { $thresholdMap[$id] = [int]$thresholdBytes[$offset + 3] }
                    }
                }
            }
            $attributes = @(ConvertFrom-SmartVendorSpecific -Raw $raw -ThresholdMap $thresholdMap)
            if ($attributes.Count -eq 0) {
                throw 'Payload SMART recebido, mas nenhum atributo válido foi interpretado.'
            }
            $parseStatus = 'parsed'
            $sources += 'MSStorageDriver_FailurePredictData'
        } catch {
            $parseStatus = 'parse_error'
            $errors += ('Interpretação dos atributos SMART: ' + $_.Exception.Message)
        }
    } elseif ($dataStatus -eq 'collection_error') {
        $parseStatus = 'not_attempted'
    } else {
        $parseStatus = 'not_exposed'
    }

    $attr5 = $attributes | Where-Object { $_.id -eq 5 } | Select-Object -First 1
    $attr197 = $attributes | Where-Object { $_.id -eq 197 } | Select-Object -First 1
    $attr198 = $attributes | Where-Object { $_.id -eq 198 } | Select-Object -First 1
    $attr199 = $attributes | Where-Object { $_.id -eq 199 } | Select-Object -First 1
    $attr194 = $attributes | Where-Object { $_.id -in @(190,194) } | Select-Object -First 1
    $wearAttr = $attributes | Where-Object { $_.id -in @(202,231,233) -and $null -ne $_.threshold -and $_.current -ge 0 } | Select-Object -First 1

    if ($null -ne $attr5) { $reallocated = [uint64]$attr5.raw_value }
    if ($null -ne $attr197) { $pending = [uint64]$attr197.raw_value }
    if ($null -ne $attr198) { $uncorrectable = [uint64]$attr198.raw_value }
    if ($null -ne $attr199) { $crc = [uint64]$attr199.raw_value }
    if ($null -ne $attr194) {
        $tempByte = [int]($attr194.raw_value -band 255)
        if ($tempByte -ge 1 -and $tempByte -le 120) { $temperature = $tempByte }
    }

    if ($null -ne $counter) {
        $sources += 'Get-StorageReliabilityCounter'
        foreach ($property in @('Temperature','TemperatureMax','Wear','ReadErrorsTotal','ReadErrorsUncorrected','WriteErrorsTotal','WriteErrorsUncorrected','PowerOnHours','StartStopCycleCount','LoadUnloadCycleCount')) {
            $propertyInfo = $counter.PSObject.Properties[$property]
            if ($null -ne $propertyInfo -and $null -ne $propertyInfo.Value) {
                $reliability += [pscustomobject]@{ name = $property; value = [string]$propertyInfo.Value }
            }
        }
        if ($null -ne $counter.Temperature -and [double]$counter.Temperature -gt 0 -and [double]$counter.Temperature -lt 150) {
            $temperature = [double]$counter.Temperature
        }
        if ($null -ne $counter.Wear -and [double]$counter.Wear -ge 0 -and [double]$counter.Wear -le 100) {
            $wear = [double]$counter.Wear
            $wearLimit = 100.0
        }
    }
    if ($null -eq $wear -and $null -ne $wearAttr) {
        $wear = [double](100 - [int]$wearAttr.current)
        $wearLimit = [double](100 - [int]$wearAttr.threshold)
    }

    $sectorStatus = @{
        reallocated = Get-SmartSectorAttributeStatus -Attribute $attr5 -DataStatus $dataStatus -ParseStatus $parseStatus
        pending = Get-SmartSectorAttributeStatus -Attribute $attr197 -DataStatus $dataStatus -ParseStatus $parseStatus
        uncorrectable = Get-SmartSectorAttributeStatus -Attribute $attr198 -DataStatus $dataStatus -ParseStatus $parseStatus
    }
    $usable = ($attributes.Count -gt 0 -or $reliability.Count -gt 0 -or $null -ne $predictedFailure)
    return [pscustomobject]@{
        usable = [bool]$usable
        source = (($sources | Select-Object -Unique) -join ', ')
        predicted_failure = $predictedFailure
        reallocated = $reallocated
        pending = $pending
        uncorrectable = $uncorrectable
        interface_crc = $crc
        wear_percent = $wear
        wear_limit_percent = $wearLimit
        temperature_c = $temperature
        attributes = @($attributes)
        reliability_metrics = @($reliability)
        collection_status = [pscustomobject]@{
            failure_predict_data = $dataStatus
            failure_predict_thresholds = $thresholdStatus
            failure_predict_status = $predictStatus
            storage_reliability_counter = $counterStatus
            instance_match = $instanceStatus
            attributes_parse = $parseStatus
            sector_attributes = [pscustomobject]$sectorStatus
            errors = @($errors)
        }
    }
}

function New-Measurement([string]$kind, [string]$unit, [int]$blockSize, [int]$queueDepth, [string]$reason) {
    return [pscustomobject]@{
        kind = $kind
        value = $null
        unit = $unit
        bytes_transferred = $null
        duration_ms = $null
        queue_depth = $queueDepth
        block_size_bytes = $blockSize
        reason = $reason
    }
}

function Get-MeasurementSpec {
    return @(
        [pscustomobject]@{ kind='seq_small_write'; unit='MB/s'; block=131072; queue=1; write=$true; random=$false },
        [pscustomobject]@{ kind='seq_small_read'; unit='MB/s'; block=131072; queue=1; write=$false; random=$false },
        [pscustomobject]@{ kind='seq_large_write'; unit='MB/s'; block=1048576; queue=8; write=$true; random=$false },
        [pscustomobject]@{ kind='seq_large_read'; unit='MB/s'; block=1048576; queue=8; write=$false; random=$false },
        [pscustomobject]@{ kind='random_small_write'; unit='IOPS'; block=4096; queue=1; write=$true; random=$true },
        [pscustomobject]@{ kind='random_small_read'; unit='IOPS'; block=4096; queue=1; write=$false; random=$true },
        [pscustomobject]@{ kind='random_large_write'; unit='IOPS'; block=4096; queue=32; write=$true; random=$true },
        [pscustomobject]@{ kind='random_large_read'; unit='IOPS'; block=4096; queue=32; write=$false; random=$true }
    )
}

if ($mode -eq 'selftest') {
    $smartFixture = New-Object byte[] 14
    $smartFixture[2] = 5
    $smartFixture[3] = 100
    $smartFixture[4] = 95
    $smartFixture[5] = 7
    $smartParsed = @(ConvertFrom-SmartVendorSpecific -Raw $smartFixture -ThresholdMap @{ 5 = 10 })
    if ($smartParsed.Count -ne 1 -or $smartParsed[0].id -ne 5 -or $smartParsed[0].raw_value -ne 7 -or $smartParsed[0].threshold -ne 10) {
        throw 'Autoteste SMART falhou ao interpretar o atributo de setores realocados.'
    }
    $invalidSmartRejected = $false
    try {
        [void](ConvertFrom-SmartVendorSpecific -Raw ([byte[]](1,2,3,4)) -ThresholdMap @{})
    } catch {
        $invalidSmartRejected = $true
    }
    if (-not $invalidSmartRejected) {
        throw 'Autoteste SMART falhou: payload inválido deveria ser rejeitado.'
    }
    $statusCases = @(
        @{ attribute = $smartParsed[0]; data = 'available'; parse = 'parsed'; expected = 'available' },
        @{ attribute = $null; data = 'not_exposed'; parse = 'not_exposed'; expected = 'provider_unavailable' },
        @{ attribute = $null; data = 'collection_error'; parse = 'not_attempted'; expected = 'collection_error' },
        @{ attribute = $null; data = 'available'; parse = 'parse_error'; expected = 'parse_error' },
        @{ attribute = $null; data = 'available'; parse = 'parsed'; expected = 'attribute_not_reported' }
    )
    foreach ($case in $statusCases) {
        $actualStatus = Get-SmartSectorAttributeStatus -Attribute $case.attribute -DataStatus $case.data -ParseStatus $case.parse
        if ($actualStatus -ne $case.expected) {
            throw ('Autoteste SMART falhou: esperado ' + $case.expected + ', recebido ' + $actualStatus + '.')
        }
    }
    Add-Type -TypeDefinition $benchSource -Language CSharp -ErrorAction Stop
    $selfTestFileBytes = 16777216
    $selfTestMaxBytes = 16777216
    $selfTestPath = Join-Path $env:TEMP ('.qboa-storage-selftest-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $selfTestResults = @()
    try {
        $seed = New-Object System.IO.FileStream($selfTestPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::ReadWrite)
        try {
            $seedBuffer = New-Object byte[] (1048576)
            for ($i = 0; $i -lt $seedBuffer.Length; $i++) {
                $seedBuffer[$i] = [byte](($i * 17 + 91) -band 255)
            }
            $remaining = $selfTestFileBytes
            while ($remaining -gt 0) {
                $count = [int][Math]::Min($seedBuffer.Length, $remaining)
                $seed.Write($seedBuffer, 0, $count)
                $remaining -= $count
            }
            $seed.Flush($true)
        } finally {
            $seed.Dispose()
        }

        foreach ($spec in (Get-MeasurementSpec)) {
            $samples = New-Object 'System.Collections.Generic.List[object]'
            $progressScript = {
                param($elapsedMs, $totalBytes, $totalOperations, $mbPerSecond, $iops)
                $samples.Add([pscustomobject]@{
                    elapsed_ms = [long]$elapsedMs
                    bytes_transferred = [long]$totalBytes
                    operations = [long]$totalOperations
                    mb_per_second = [Math]::Round([double]$mbPerSecond, 2)
                    iops = [Math]::Round([double]$iops, 2)
                })
            }.GetNewClosure()
            $progressCallback = [System.Action[long,long,long,double,double]]$progressScript
            $result = [QboaStorageBench]::Run(
                $selfTestPath,
                $selfTestFileBytes,
                [int]$spec.block,
                [int]$spec.queue,
                [bool]$spec.write,
                [bool]$spec.random,
                $selfTestMaxBytes,
                5000,
                $progressCallback
            )
            $seconds = [double]$result.DurationMs / 1000.0
            if ($seconds -le 0 -or $result.Bytes -le 0 -or $result.Operations -le 0) {
                throw ('A medição de autoteste não produziu I/O: ' + $spec.kind)
            }
            if ($samples.Count -eq 0) {
                throw ('O callback não produziu amostras de desempenho: ' + $spec.kind)
            }
            $mbPerSecond = [Math]::Round(([double]$result.Bytes / 1048576.0 / $seconds), 2)
            $iops = [Math]::Round(([double]$result.Operations / $seconds), 2)
            if ($spec.unit -eq 'MB/s') {
                $measuredValue = $mbPerSecond
                $secondaryValue = $iops
                $secondaryUnit = 'IOPS'
            } else {
                $measuredValue = $iops
                $secondaryValue = $mbPerSecond
                $secondaryUnit = 'MB/s'
            }
            $selfTestResults += [pscustomobject]@{
                kind = $spec.kind
                unit = $spec.unit
                block = [int]$spec.block
                queue = [int]$spec.queue
                write = [bool]$spec.write
                random = [bool]$spec.random
                value = $measuredValue
                secondary_value = $secondaryValue
                secondary_unit = $secondaryUnit
                mb_per_second = $mbPerSecond
                iops = $iops
                sample_count = $samples.Count
                bytes_transferred = [long]$result.Bytes
                duration_ms = [Math]::Round($result.DurationMs, 2)
                operations = [long]$result.Operations
                queue_depth = [int]$spec.queue
                block_size_bytes = [int]$spec.block
                reason = $null
            }
        }
    } finally {
        if (Test-Path -LiteralPath $selfTestPath) {
            Remove-Item -LiteralPath $selfTestPath -Force -ErrorAction Stop
        }
        if (Test-Path -LiteralPath $selfTestPath) {
            throw 'O arquivo temporário do autoteste de armazenamento não foi removido.'
        }
    }
    ConvertTo-Json -InputObject $selfTestResults -Depth 4 -Compress
    exit 0
}

if ($mode -eq 'specs') {
    Add-Type -TypeDefinition $benchSource -Language CSharp -ErrorAction Stop
    $specs = @(Get-MeasurementSpec)
    if ($specs.Count -ne 8) { throw 'O contrato do benchmark deve conter exatamente oito medições.' }
    ConvertTo-Json -InputObject $specs -Depth 4 -Compress
    exit 0
}

if ($mode -eq 'list') {
    $entries = @(Get-DiskEntries)
    ConvertTo-Json -InputObject $entries -Depth 8 -Compress
    exit 0
}

if ($mode -ne 'test' -or $diskId -notmatch '^([0-9]+):([A-Z])$') {
    throw 'Modo ou identificador de disco inválido.'
}
$diskNumber = [int]$Matches[1]
$driveLetter = $Matches[2]
$entry = @(Get-DiskEntries | Where-Object { $_.id -eq $diskId }) | Select-Object -First 1
if ($null -eq $entry) { throw 'O disco/volume selecionado não está mais disponível.' }

$freeBytes = $null
try { $freeBytes = [long](Get-Volume -DriveLetter $driveLetter -ErrorAction Stop).SizeRemaining } catch {}
$testFileBytes = 268435456
$maxBytesPerMeasurement = 1073741824
$timeLimitMs = 3000
$spaceAvailable = ($null -ne $freeBytes -and $freeBytes -ge ($testFileBytes + 67108864))
$smartBefore = $null
$smartAfter = $null
$measurements = @()
$issues = @()
$executionStatus = 'completed'
$testPath = $null

try { $smartBefore = Get-SmartSnapshot $diskNumber } catch { $issues += ('Falha na leitura inicial do SMART: ' + $_.Exception.Message) }
foreach ($spec in (Get-MeasurementSpec)) {
    $measurements += New-Measurement $spec.kind $spec.unit $spec.block $spec.queue $(if ($spaceAvailable) { 'Aguardando execução.' } else { 'Espaço livre insuficiente; teste não iniciado.' })
}
if ($spaceAvailable) {
    try {
        Add-Type -TypeDefinition $benchSource -Language CSharp -ErrorAction Stop
        $testPath = Join-Path ($driveLetter + ':\') ('.qboa-storage-test-' + [Guid]::NewGuid().ToString('N') + '.tmp')
        $seed = New-Object System.IO.FileStream($testPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::ReadWrite)
        try {
            $seedBuffer = New-Object byte[] (1048576)
            for ($i = 0; $i -lt $seedBuffer.Length; $i++) { $seedBuffer[$i] = [byte](($i * 17 + 91) -band 255) }
            $remaining = $testFileBytes
            while ($remaining -gt 0) {
                $count = [int][Math]::Min($seedBuffer.Length, $remaining)
                $seed.Write($seedBuffer, 0, $count)
                $remaining -= $count
            }
            $seed.Flush($true)
        } finally { $seed.Dispose() }

        $completedCount = 0
        $measurements = @()
        foreach ($spec in (Get-MeasurementSpec)) {
            try {
                $samples = New-Object 'System.Collections.Generic.List[object]'
                $kind = [string]$spec.kind
                $progressScript = {
                    param($elapsedMs, $totalBytes, $totalOperations, $mbPerSecond, $iops)
                    $samples.Add([pscustomobject]@{
                        elapsed_ms = [long]$elapsedMs
                        bytes_transferred = [long]$totalBytes
                        operations = [long]$totalOperations
                        mb_per_second = [Math]::Round([double]$mbPerSecond, 2)
                        iops = [Math]::Round([double]$iops, 2)
                    })
                    $line = [string]::Format([Globalization.CultureInfo]::InvariantCulture,
                        'QBOA_PROGRESS|{0}|{1}|{2}|{3}|{4:R}|{5:R}',
                        $kind, $elapsedMs, $totalBytes, $totalOperations, $mbPerSecond, $iops)
                    [Console]::Out.WriteLine($line)
                    [Console]::Out.Flush()
                }.GetNewClosure()
                $progressCallback = [System.Action[long,long,long,double,double]]$progressScript
                $result = [QboaStorageBench]::Run($testPath, $testFileBytes, [int]$spec.block,
                    [int]$spec.queue, [bool]$spec.write, [bool]$spec.random,
                    $maxBytesPerMeasurement, $timeLimitMs, $progressCallback)
                $seconds = [double]$result.DurationMs / 1000.0
                if ($seconds -le 0) { throw 'Duração da medição inválida.' }
                $mbPerSecond = [Math]::Round(([double]$result.Bytes / 1048576.0 / $seconds), 2)
                $iops = [Math]::Round(([double]$result.Operations / $seconds), 2)
                if ($spec.unit -eq 'MB/s') {
                    $value = $mbPerSecond
                    $secondaryValue = $iops
                    $secondaryUnit = 'IOPS'
                } else {
                    $value = $iops
                    $secondaryValue = $mbPerSecond
                    $secondaryUnit = 'MB/s'
                }
                $measurements += [pscustomobject]@{
                    kind = $spec.kind
                    value = $value
                    unit = $spec.unit
                    secondary_value = $secondaryValue
                    secondary_unit = $secondaryUnit
                    mb_per_second = $mbPerSecond
                    iops = $iops
                    samples = $samples.ToArray()
                    bytes_transferred = [long]$result.Bytes
                    duration_ms = [Math]::Round($result.DurationMs, 2)
                    queue_depth = [int]$spec.queue
                    block_size_bytes = [int]$spec.block
                    reason = $null
                }
                $completedCount++
            } catch {
                $measurements += New-Measurement $spec.kind $spec.unit $spec.block $spec.queue ('Falha nesta medição: ' + $_.Exception.Message)
                $issues += ('Medição ' + $spec.kind + ' falhou: ' + $_.Exception.Message)
            }
        }
        if ($completedCount -ne 8) { $executionStatus = 'partial' }
    } catch {
        $executionStatus = 'partial'
        $issues += ('Falha na preparação do teste: ' + $_.Exception.Message)
    } finally {
        if ($null -ne $testPath) {
            try { Remove-Item -LiteralPath $testPath -Force -ErrorAction Stop } catch { $issues += 'Não foi possível remover automaticamente o arquivo temporário do teste.' }
        }
    }
} else {
    $executionStatus = 'partial'
    $issues += 'Espaço livre insuficiente: nenhum benchmark de leitura/escrita foi iniciado.'
}

try { $smartAfter = Get-SmartSnapshot $diskNumber } catch { $issues += ('Falha na leitura final do SMART: ' + $_.Exception.Message) }
if ($null -ne $smartAfter -and [bool]$smartAfter.usable) {
    $smart = $smartAfter
} elseif ($null -ne $smartBefore -and [bool]$smartBefore.usable) {
    # Preserva a coleta inicial se o provedor parar de entregar dados válidos ao final.
    $smart = $smartBefore
} elseif ($null -ne $smartAfter) {
    $smart = $smartAfter
} else {
    $smart = $smartBefore
}
if ($null -eq $smart) {
    $smart = [pscustomobject]@{
        usable = $false; source = ''; predicted_failure = $null; reallocated = $null; pending = $null;
        uncorrectable = $null; interface_crc = $null; wear_percent = $null; wear_limit_percent = $null;
        temperature_c = $null; attributes = @(); reliability_metrics = @()
        collection_status = [pscustomobject]@{
            failure_predict_data = 'collection_error'
            failure_predict_thresholds = 'not_attempted'
            failure_predict_status = 'not_attempted'
            storage_reliability_counter = 'collection_error'
            instance_match = 'query_error'
            attributes_parse = 'not_attempted'
            sector_attributes = [pscustomobject]@{
                reallocated = 'collection_error'
                pending = 'collection_error'
                uncorrectable = 'collection_error'
            }
            errors = @('A coleta SMART não retornou um snapshot válido; consulte os avisos da coleta.')
        }
    }
}
$crcBefore = if ($null -ne $smartBefore) { $smartBefore.interface_crc } else { $null }
$crcAfter = if ($null -ne $smartAfter) { $smartAfter.interface_crc } else { $null }
$usbSlow = $false
if ($entry.class -eq 'usb') {
    $readMeasure = $measurements | Where-Object { $_.kind -eq 'seq_large_read' -and $null -ne $_.value } | Select-Object -First 1
    if ($null -ne $readMeasure -and [double]$readMeasure.value -lt 20) { $usbSlow = $true }
}
# Os cmdlets padrão do Windows não expõem um estado PIO/negociação SATA confiável para todos os controladores.
# Não gravamos false quando essa informação não pode ser detectada.
$pio = $null
$slowSataLink = $null
$report = [pscustomobject]@{
    disk_id = $diskId
    backend = 'windows'
    disk_name = $entry.model
    volume = $entry.volume
    capacity_bytes = $entry.capacity_bytes
    volume_capacity_bytes = $entry.volume_capacity_bytes
    free_bytes = $freeBytes
    class = $entry.class
    bus_type = $entry.bus_type
    measurements = @($measurements)
    smart = [pscustomobject]@{
        usable = [bool]$smart.usable
        source = [string]$smart.source
        predicted_failure = $smart.predicted_failure
        reallocated = $smart.reallocated
        pending = $smart.pending
        uncorrectable = $smart.uncorrectable
        interface_crc_before = $crcBefore
        interface_crc_after = $crcAfter
        wear_percent = $smart.wear_percent
        wear_limit_percent = $smart.wear_limit_percent
        temperature_c = $smart.temperature_c
        pio_mode = $pio
        slow_sata_link = $slowSataLink
        usb_slow = $usbSlow
        attributes = @($smart.attributes)
        reliability_metrics = @($smart.reliability_metrics)
        collection_status = $smart.collection_status
    }
    recommendation = 'inconclusivo'
    reasons = @()
    execution_status = $executionStatus
    space_available = [bool]$spaceAvailable
    issues = @($issues)
}
ConvertTo-Json -InputObject $report -Depth 12 -Compress

