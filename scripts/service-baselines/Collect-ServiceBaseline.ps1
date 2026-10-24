[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Environment,
    [Parameter(Mandatory=$true)][string]$OutputPath,
    [string]$ExpectedBuild,
    [string]$ExpectedEditionId
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Get-RegistryTree([Microsoft.Win32.RegistryKey]$Key) {
    $result = [ordered]@{ values = [ordered]@{}; subkeys = [ordered]@{} }
    foreach ($valueName in $Key.GetValueNames()) {
        $displayName = if ([string]::IsNullOrEmpty($valueName)) { '(Default)' } else { $valueName }
        $value = $Key.GetValue($valueName, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $kind = $Key.GetValueKind($valueName).ToString()
        $result.values[$displayName] = [ordered]@{ kind = $kind; data = $value }
    }
    foreach ($subName in $Key.GetSubKeyNames()) {
        $sub = $Key.OpenSubKey($subName)
        if ($null -ne $sub) {
            try { $result.subkeys[$subName] = Get-RegistryTree $sub }
            finally { $sub.Dispose() }
        }
    }
    return $result
}

$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$build = [string]$cv.CurrentBuildNumber
$editionId = [string]$cv.EditionID
$ubr = [string]$cv.UBR
$version = [string]$cv.DisplayVersion
if ($ExpectedBuild -and $build -ne $ExpectedBuild) { throw "Build observada $build difere da esperada $ExpectedBuild; coleta abortada." }
if ($ExpectedEditionId -and $editionId -ne $ExpectedEditionId) { throw "EditionID observado '$editionId' difere do esperado '$ExpectedEditionId'; coleta abortada." }

$root = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SYSTEM\CurrentControlSet\Services')
if ($null -eq $root) { throw 'Não foi possível abrir HKLM\SYSTEM\CurrentControlSet\Services.' }
$services = [System.Collections.Generic.List[object]]::new()
try {
    foreach ($name in $root.GetSubKeyNames()) {
        # Instâncias por usuário/sessão têm sufixo hexadecimal; conservar o template sem sufixo.
        if ($name -match '_[0-9a-fA-F]{4,}$') { continue }
        $key = $root.OpenSubKey($name)
        if ($null -eq $key) { continue }
        try {
            $start = $key.GetValue('Start', $null)
            if ($null -eq $start) { continue }
            $delayed = $key.GetValue('DelayedAutostart', 0)
            $type = $key.GetValue('Type', $null)
            $triggerKey = $key.OpenSubKey('TriggerInfo')
            $triggerInfo = $null
            if ($null -ne $triggerKey) {
                try { $triggerInfo = Get-RegistryTree $triggerKey }
                finally { $triggerKey.Dispose() }
            }
            $deps = $key.GetValue('DependOnService', @(), [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            $label = switch ([int]$start) {
                0 { 'Boot' }
                1 { 'System' }
                2 { if ([int]$delayed -eq 1) { 'Automatic (Delayed)' } else { 'Automatic' } }
                3 { 'Manual' }
                4 { 'Disabled' }
                default { 'Unknown' }
            }
            $services.Add([ordered]@{
                name = $name
                start = [int]$start
                delayed_autostart = [int]$delayed
                type = if ($null -eq $type) { $null } else { [int]$type }
                start_label = $label
                trigger_present = ($null -ne $triggerInfo)
                trigger_info = $triggerInfo
                depend_on_service = @($deps)
                display_name = [string]$key.GetValue('DisplayName', '')
                image_path = [string]$key.GetValue('ImagePath', '')
                source = 'measured'
            })
        } finally { $key.Dispose() }
    }
} finally { $root.Dispose() }

$payload = [ordered]@{
    schema_version = 1
    source = 'measured'
    environment = $Environment
    collected_at_utc = [DateTime]::UtcNow.ToString('o')
    computer_name = $env:COMPUTERNAME
    os = [ordered]@{
        product_name = [string]$cv.ProductName
        display_version = $version
        build = $build
        ubr = $ubr
        edition_id = $editionId
        installation_type = [string]$cv.InstallationType
    }
    services = @($services | Sort-Object name)
}
$parent = Split-Path -Parent $OutputPath
if ($parent -and -not (Test-Path $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
$payload | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $OutputPath -Encoding utf8
Write-Output "Coleta salva: $OutputPath ($($services.Count) serviços; build $build; EditionID $editionId)."
