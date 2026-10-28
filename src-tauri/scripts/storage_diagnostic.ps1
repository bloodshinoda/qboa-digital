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
    private static extern bool FlushFileBuffers(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr handle);

    public static QboaBenchResult Run(string path, long fileBytes, int blockBytes, int queueDepth,
        bool write, bool randomAccess, long maxBytes, int maxMilliseconds) {
        if (blockBytes <= 0 || blockBytes % 4096 != 0 || fileBytes < blockBytes ||
            fileBytes % 4096 != 0 || queueDepth < 1 || queueDepth > 32 ||
            maxBytes < blockBytes || maxMilliseconds < 1)
            throw new ArgumentException("Parâmetros de benchmark inválidos.");

        long reservedBytes = 0, completedBytes = 0, operations = 0, sequentialBlock = -1;
        int stop = 0;
        var failures = new Exception[queueDepth];
        var stopwatch = Stopwatch.StartNew();
        var workers = new Thread[queueDepth];

        for (int workerId = 0; workerId < queueDepth; workerId++) {
            int localId = workerId;
            workers[workerId] = new Thread(() => {
                IntPtr allocation = IntPtr.Zero;
                IntPtr aligned = IntPtr.Zero;
                IntPtr handle = InvalidHandle;
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
                        if (!SetFilePointerEx(handle, offset, out newPosition, 0))
                            throw new Win32Exception(Marshal.GetLastWin32Error(), "Falha ao posicionar a E/S direta.");
                        uint transferred;
                        bool ok = write
                            ? WriteFile(handle, aligned, (uint)blockBytes, out transferred, IntPtr.Zero)
                            : ReadFile(handle, aligned, (uint)blockBytes, out transferred, IntPtr.Zero);
                        if (!ok || transferred != (uint)blockBytes)
                            throw new Win32Exception(Marshal.GetLastWin32Error(), "Falha na leitura/escrita direta do arquivo temporário.");

                        Interlocked.Add(ref completedBytes, transferred);
                        Interlocked.Increment(ref operations);
                    }

                    if (write && handle != InvalidHandle && !FlushFileBuffers(handle))
                        throw new Win32Exception(Marshal.GetLastWin32Error(), "Falha ao descarregar gravações do teste.");
                } catch (Exception error) {
                    failures[localId] = error;
                    Interlocked.Exchange(ref stop, 1);
                } finally {
                    if (handle != InvalidHandle) CloseHandle(handle);
                    if (allocation != IntPtr.Zero) Marshal.FreeHGlobal(allocation);
                }
            });
            workers[workerId].IsBackground = true;
            workers[workerId].Start();
        }

        foreach (Thread worker in workers) worker.Join();
        stopwatch.Stop();
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

function Get-ClassForDisk($disk, $diskInfo, $physicalDisk) {
    $bus = ''
    if ($null -ne $diskInfo -and $null -ne $diskInfo.BusType) { $bus = [string]$diskInfo.BusType }
    elseif ($null -ne $physicalDisk -and $null -ne $physicalDisk.BusType) { $bus = [string]$physicalDisk.BusType }
    elseif ($null -ne $disk.InterfaceType) { $bus = [string]$disk.InterfaceType }

    if ($bus -match 'USB') { return 'usb' }
    if ($bus -match '^NVMe$') { return 'nvme' }

    if ($bus -match '^SATA$') {
        $media = ''
        if ($null -ne $physicalDisk -and $null -ne $physicalDisk.MediaType) { $media = [string]$physicalDisk.MediaType }
        if ($media -eq 'SSD') { return 'sata_ssd' }
        if ($media -eq 'HDD') {
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
        $pd = $physical | Where-Object { "$($_.DeviceId)" -eq "$number" } | Select-Object -First 1
        $partitions = @(Get-Partition -DiskNumber $number -ErrorAction SilentlyContinue |
            Where-Object { $_.DriveLetter -and $_.Type -notin @('Reserved','System') })
        foreach ($partition in $partitions) {
            $volume = Get-Volume -Partition $partition -ErrorAction SilentlyContinue
            if ($null -eq $volume -or -not $volume.DriveLetter -or -not $volume.FileSystem -or $volume.FileSystem -eq 'CDFS') { continue }
            $letter = ([string]$volume.DriveLetter).ToUpperInvariant()
            if ($letter -notmatch '^[A-Z]$') { continue }
            $capacity = $null
            if ($null -ne $disk.Size -and [long]$disk.Size -gt 0) { $capacity = [long]$disk.Size }
            $free = $null
            if ($null -ne $volume.SizeRemaining) { $free = [long]$volume.SizeRemaining }
            $entries += [pscustomobject]@{
                id = ([string]$number) + ':' + $letter
                backend = 'windows'
                disk_number = $number
                model = ([string]$disk.Model).Trim()
                capacity_bytes = $capacity
                volume = $letter + ':\'
                free_bytes = $free
                class = Get-ClassForDisk $disk $info $pd
                bus_type = if ($null -ne $info -and $null -ne $info.BusType) { [string]$info.BusType } elseif ($null -ne $pd) { [string]$pd.BusType } else { [string]$disk.InterfaceType }
            }
        }
    }
    return ,$entries
}

function Get-SmartSnapshot([int]$diskNumber) {
    $disk = Get-CimInstance Win32_DiskDrive -Filter ("Index = " + $diskNumber) -ErrorAction Stop
    $pd = Get-PhysicalDisk -ErrorAction SilentlyContinue | Where-Object { "$($_.DeviceId)" -eq "$diskNumber" } | Select-Object -First 1
    $counter = $null
    try { if ($null -ne $pd) { $counter = $pd | Get-StorageReliabilityCounter -ErrorAction SilentlyContinue } } catch {}

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

    $pnpParts = ([string]$disk.PNPDeviceID).Split('\')
    $prefix = ''
    if ($pnpParts.Length -ge 2) { $prefix = ($pnpParts[0..1] -join '\') }

    try {
        $smartData = @(Get-CimInstance -Namespace root/wmi -ClassName MSStorageDriver_FailurePredictData -ErrorAction SilentlyContinue |
            Where-Object { $prefix -and ([string]$_.InstanceName).StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) })
        $smartThresholds = @(Get-CimInstance -Namespace root/wmi -ClassName MSStorageDriver_FailurePredictThresholds -ErrorAction SilentlyContinue |
            Where-Object { $prefix -and ([string]$_.InstanceName).StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) })
        $smartStatus = @(Get-CimInstance -Namespace root/wmi -ClassName MSStorageDriver_FailurePredictStatus -ErrorAction SilentlyContinue |
            Where-Object { $prefix -and ([string]$_.InstanceName).StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) })
        if ($smartData.Count -gt 0) {
            $raw = $smartData[0].VendorSpecific
            $thresholdMap = @{}
            if ($smartThresholds.Count -gt 0) {
                $thresholdBytes = $smartThresholds[0].VendorSpecific
                for ($offset = 2; $offset + 11 -lt $thresholdBytes.Length; $offset += 12) {
                    $id = [int]$thresholdBytes[$offset]
                    if ($id -ne 0) { $thresholdMap[$id] = [int]$thresholdBytes[$offset + 3] }
                }
            }
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
            for ($offset = 2; $offset + 11 -lt $raw.Length; $offset += 12) {
                $id = [int]$raw[$offset]
                if ($id -eq 0) { continue }
                [uint64]$rawValue = 0
                [uint64]$multiplier = 1
                for ($j = 0; $j -lt 6; $j++) {
                    $rawValue += ([uint64]$raw[$offset + 5 + $j] * $multiplier)
                    $multiplier *= 256
                }
                $current = [int]$raw[$offset + 3]
                $worst = [int]$raw[$offset + 4]
                $threshold = $null
                if ($thresholdMap.ContainsKey($id)) { $threshold = [int]$thresholdMap[$id] }
                $attributes += [pscustomobject]@{
                    id = $id
                    name = if ($names.ContainsKey($id)) { $names[$id] } else { 'SMART ' + $id }
                    current = $current
                    worst = $worst
                    threshold = $threshold
                    raw_value = $rawValue
                }
            }
            $sources += 'MSStorageDriver_FailurePredictData'
        }
        if ($smartStatus.Count -gt 0) { $predictedFailure = [bool]$smartStatus[0].PredictFailure; $sources += 'MSStorageDriver_FailurePredictStatus' }
    } catch {}

    $attr5 = $attributes | Where-Object { $_.id -eq 5 } | Select-Object -First 1
    $attr197 = $attributes | Where-Object { $_.id -eq 197 } | Select-Object -First 1
    $attr198 = $attributes | Where-Object { $_.id -eq 198 } | Select-Object -First 1
    $attr199 = $attributes | Where-Object { $_.id -eq 199 } | Select-Object -First 1
    $attr194 = $attributes | Where-Object { $_.id -in @(190,194) } | Select-Object -First 1
    $wearAttr = $attributes | Where-Object { $_.id -in @(202,231,233) -and $null -ne $_.threshold -and $_.current -gt 0 } | Select-Object -First 1

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
$testFileBytes = 134217728
$maxBytesPerMeasurement = 134217728
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
                $result = [QboaStorageBench]::Run($testPath, $testFileBytes, [int]$spec.block,
                    [int]$spec.queue, [bool]$spec.write, [bool]$spec.random,
                    $maxBytesPerMeasurement, $timeLimitMs)
                $seconds = [double]$result.DurationMs / 1000.0
                if ($seconds -le 0) { throw 'Duração da medição inválida.' }
                if ($spec.unit -eq 'MB/s') { $value = [Math]::Round(([double]$result.Bytes / 1048576.0 / $seconds), 2) }
                else { $value = [Math]::Round(([double]$result.Operations / $seconds), 2) }
                $measurements += [pscustomobject]@{
                    kind = $spec.kind
                    value = $value
                    unit = $spec.unit
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
$smart = if ($null -ne $smartAfter) { $smartAfter } else { $smartBefore }
if ($null -eq $smart) {
    $smart = [pscustomobject]@{
        usable = $false; source = ''; predicted_failure = $null; reallocated = $null; pending = $null;
        uncorrectable = $null; interface_crc = $null; wear_percent = $null; wear_limit_percent = $null;
        temperature_c = $null; attributes = @(); reliability_metrics = @()
    }
}
$crcBefore = if ($null -ne $smartBefore) { $smartBefore.interface_crc } else { $null }
$crcAfter = if ($null -ne $smartAfter) { $smartAfter.interface_crc } else { $null }
$usbSlow = $false
if ($entry.class -eq 'usb') {
    $readMeasure = $measurements | Where-Object { $_.kind -eq 'seq_large_read' -and $null -ne $_.value } | Select-Object -First 1
    if ($null -ne $readMeasure -and [double]$readMeasure.value -lt 20) { $usbSlow = $true }
}
$pio = $false
# A ausência de uma fonte confiável para o modo de transferência não é reportada como falha de disco.
$report = [pscustomobject]@{
    disk_id = $diskId
    backend = 'windows'
    disk_name = $entry.model
    volume = $entry.volume
    capacity_bytes = $entry.capacity_bytes
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
        slow_sata_link = $false
        usb_slow = $usbSlow
        attributes = @($smart.attributes)
        reliability_metrics = @($smart.reliability_metrics)
    }
    recommendation = 'inconclusivo'
    reasons = @()
    execution_status = $executionStatus
    space_available = [bool]$spaceAvailable
    issues = @($issues)
}
ConvertTo-Json -InputObject $report -Depth 12 -Compress
