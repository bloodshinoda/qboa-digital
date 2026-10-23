use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityStatus {
    Detected,
    Disabled,
    RegisteredNotPresent,
    NotDetected,
    Unknown,
    NotApplicable,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Capability {
    pub id: String,
    pub label: String,
    pub status: CapabilityStatus,
    pub confidence: String,
    pub summary: String,
    pub sources: Vec<String>,
    pub details: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SystemSummary {
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub os_build: Option<String>,
    pub architecture: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub motherboard: Option<String>,
    pub processor: Option<String>,
    pub memory: Option<String>,
    pub gpu: Option<String>,
    pub device_type: Option<String>,
    pub device_confidence: Option<String>,
    pub device_evidence: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct CapabilityInventory {
    pub system: SystemSummary,
    pub capabilities: Vec<Capability>,
    pub warnings: Vec<String>,
}

#[cfg(target_os = "windows")]
fn collect_windows() -> Result<CapabilityInventory, String> {
    use std::{os::windows::process::CommandExt, process::Command};
    const SCRIPT: &str = r#"
$ErrorActionPreference='Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$OutputEncoding = [Console]::OutputEncoding
$warnings=[System.Collections.Generic.List[string]]::new()
$caps=[System.Collections.Generic.List[object]]::new()
$all=@();$present=@();$pnpOk=$false
try {$all=@(Get-PnpDevice -ErrorAction Stop);$present=@(Get-PnpDevice -PresentOnly -ErrorAction Stop);$pnpOk=$true} catch {$warnings.Add('Enumeração Plug and Play falhou.')}
function Add-Pnp([string]$id,[string]$label,[string]$pattern,[string[]]$classes) {
 if (-not $pnpOk) {$caps.Add([pscustomobject]@{id=$id;label=$label;status='unknown';confidence='low';summary='Consulta Plug and Play indisponível.';sources=@('Get-PnpDevice');details=@()});return}
 $a=@($all|Where-Object {($classes -contains [string]$_.Class)-or([string]$_.FriendlyName -match $pattern)-or([string]$_.InstanceId -match $pattern)})
 $p=@($present|Where-Object {($classes -contains [string]$_.Class)-or([string]$_.FriendlyName -match $pattern)-or([string]$_.InstanceId -match $pattern)})
 $d=@($a|Select-Object -First 8|ForEach-Object {"$($_.FriendlyName) — classe $($_.Class), estado $($_.Status), problema $($_.Problem)"})
 if($p.Count -gt 0){if(@($p|Where-Object {$_.Problem -eq 22}).Count -eq $p.Count){$s='disabled';$m='Dispositivo marcado como desabilitado pelo Windows.'}else{$s='detected';$m="Dispositivo(s) presente(s): $($p.Count)."};$c='high'}
 elseif($a.Count -gt 0){$s='registered_not_present';$m='Dispositivo registrado, mas não presente nesta enumeração.';$c='medium'}
 else{$s='not_detected';$m='Nenhum dispositivo correspondente enumerado; isso não comprova ausência física.';$c='low'}
 $caps.Add([pscustomobject]@{id=$id;label=$label;status=$s;confidence=$c;summary=$m;sources=@('Get-PnpDevice');details=@($d)})
}
Add-Pnp 'bluetooth' 'Bluetooth' 'Bluetooth|BTHENUM|BTHLE' @('Bluetooth')
Add-Pnp 'wifi' 'Wi-Fi' 'Wi.?Fi|Wireless|802\.11|WLAN' @()
Add-Pnp 'touchscreen' 'Tela sensível ao toque' 'Touch Screen|Touchscreen|HID_TOUCH|Digitizer' @()
Add-Pnp 'pen' 'Caneta digitalizadora' 'Pen Device|Stylus|Digitizer Pen|WACOM|N-trig' @()
Add-Pnp 'camera' 'Câmera' 'Camera|Webcam|Integrated Camera|USB Video' @('Camera','Image')
Add-Pnp 'biometrics' 'Biometria' 'Fingerprint|Biometric|Windows Hello Face' @('Biometric')
Add-Pnp 'audio' 'Áudio' 'Audio|Sound|Realtek.*Audio|High Definition Audio' @('MEDIA')
Add-Pnp 'battery' 'Bateria' 'Battery' @('Battery')
Add-Pnp 'ethernet' 'Ethernet' 'Ethernet|PCIe GbE|Gigabit Network|LAN Controller' @()
$caps.Add([pscustomobject]@{id='tpm';label='TPM';status='unknown';confidence='low';summary='Consulta pendente.';sources=@('Get-Tpm');details=@()})
try{$t=Get-Tpm;$i=$caps[$caps.Count-1];if($t.TpmPresent){$i.status=if($t.TpmEnabled){'detected'}else{'disabled'};$i.confidence='high';$i.summary="TPM presente; pronto=$($t.TpmReady); habilitado=$($t.TpmEnabled); ativado=$($t.TpmActivated).";$i.details=@("Versão $($t.ManufacturerVersion)","Fabricante $($t.ManufacturerIdTxt)")}else{$i.status='not_detected';$i.confidence='medium';$i.summary='O Windows não confirmou TPM presente.'}}catch{$caps[$caps.Count-1].summary='Consulta do TPM indisponível.';$caps[$caps.Count-1].details=@($_.Exception.Message);$warnings.Add('Estado do TPM inconclusivo.')}
$caps.Add([pscustomobject]@{id='secure_boot';label='Secure Boot';status='unknown';confidence='low';summary='Consulta pendente.';sources=@('Confirm-SecureBootUEFI');details=@()})
try{$sb=Confirm-SecureBootUEFI;$i=$caps[$caps.Count-1];$i.status=if($sb){'detected'}else{'disabled'};$i.confidence='high';$i.summary=if($sb){'Secure Boot habilitado.'}else{'Secure Boot desabilitado.'}}catch{$caps[$caps.Count-1].summary='Não foi possível confirmar Secure Boot; UEFI pode não estar disponível.';$caps[$caps.Count-1].details=@($_.Exception.Message)}
$caps.Add([pscustomobject]@{id='virtualization_firmware';label='Virtualização no firmware';status='unknown';confidence='low';summary='Consulta pendente.';sources=@('Win32_Processor');details=@()})
try{$cpus=@(Get-CimInstance Win32_Processor -ErrorAction Stop);$known=@($cpus|Where-Object {$null -ne $_.VirtualizationFirmwareEnabled}).Count -gt 0;$enabled=@($cpus|Where-Object {$_.VirtualizationFirmwareEnabled -eq $true}).Count -gt 0;$i=$caps[$caps.Count-1];if(-not $known){$i.summary='O provedor não informou o estado da virtualização.'}elseif($enabled){$i.status='detected';$i.confidence='high';$i.summary='Virtualização no firmware reportada como habilitada.'}else{$i.status='disabled';$i.confidence='medium';$i.summary='Virtualização não reportada como habilitada; confirme no UEFI/BIOS.'};$i.details=@($cpus|ForEach-Object {"$($_.Name): firmware=$($_.VirtualizationFirmwareEnabled); VM monitor=$($_.VMMonitorModeExtensions); SLAT=$($_.SecondLevelAddressTranslationExtensions)"})}catch{$caps[$caps.Count-1].summary='Consulta de virtualização indisponível.';$caps[$caps.Count-1].details=@($_.Exception.Message)}
$caps.Add([pscustomobject]@{id='hypervisor_active';label='Hipervisor ativo';status='unknown';confidence='low';summary='Consulta pendente.';sources=@('Win32_ComputerSystem');details=@()})
try{$cs=Get-CimInstance Win32_ComputerSystem -ErrorAction Stop;$i=$caps[$caps.Count-1];if($null -ne $cs.HypervisorPresent){$i.status=if($cs.HypervisorPresent){'detected'}else{'not_detected'};$i.confidence='high';$i.summary=if($cs.HypervisorPresent){'O Windows informa hipervisor ativo.'}else{'O Windows não informa hipervisor ativo.'}}}catch{$caps[$caps.Count-1].summary='Consulta do hipervisor indisponível.'}
$features=@(@('hyper_v','Hyper-V','Microsoft-Hyper-V-All'),@('wsl','Subsistema do Windows para Linux (WSL)','Microsoft-Windows-Subsystem-Linux'),@('virtual_machine_platform','Plataforma de Máquina Virtual','VirtualMachinePlatform'),@('sandbox','Windows Sandbox','Containers-DisposableClientVM'))
foreach($f in $features){$caps.Add([pscustomobject]@{id=$f[0];label=$f[1];status='unknown';confidence='low';summary='Consulta pendente.';sources=@('Get-WindowsOptionalFeature -Online');details=@()});$i=$caps[$caps.Count-1];try{$x=Get-WindowsOptionalFeature -Online -FeatureName $f[2] -ErrorAction Stop;$i.status=if($x.State -eq 'Enabled'){'detected'}elseif($x.State -eq 'Disabled'){'disabled'}else{'not_applicable'};$i.confidence='high';$i.summary="Estado do recurso $($f[2]): $($x.State).";$i.details=@("FeatureName=$($f[2])","State=$($x.State)")}catch{$i.summary="Não foi possível consultar o recurso $($f[2]).";$i.details=@($_.Exception.Message)}}
$sys=[pscustomobject]@{os_name=$null;os_version=$null;os_build=$null;architecture=$env:PROCESSOR_ARCHITECTURE;manufacturer=$null;model=$null;motherboard=$null;processor=$null;memory=$null;gpu=$null;device_type='unknown';device_confidence='low';device_evidence=@()}
try{$os=Get-CimInstance Win32_OperatingSystem -ErrorAction Stop;$sys.os_name=$os.Caption;$sys.os_version=$os.Version;$sys.os_build=$os.BuildNumber}catch{$warnings.Add('Versão do Windows indisponível.')}
try{
 $cs=Get-CimInstance Win32_ComputerSystem -ErrorAction Stop
 $sys.manufacturer=$cs.Manufacturer;$sys.model=$cs.Model
 $evidence=[System.Collections.Generic.List[string]]::new()
 $pcType=[int]$cs.PCSystemType
 # Win32_ComputerSystem.PCSystemType: 2=desktop, 3=mobile, 4=workstation.
 if($pcType -eq 2){$sys.device_type='desktop';$sys.device_confidence='high';$evidence.Add("Win32_ComputerSystem.PCSystemType=$pcType (desktop)")}
 elseif($pcType -eq 3){$sys.device_type='mobile';$sys.device_confidence='high';$evidence.Add("Win32_ComputerSystem.PCSystemType=$pcType (mobile)")}
 elseif($pcType -eq 4){$sys.device_type='desktop';$sys.device_confidence='high';$evidence.Add("Win32_ComputerSystem.PCSystemType=$pcType (workstation)")}
 $modelText=("$($cs.Manufacturer) $($cs.Model)")
 if($modelText -match '(?i)VMware|VirtualBox|Virtual Machine|KVM|QEMU|Hyper-V|Virtual Platform'){
   $sys.device_type='virtual_machine';$sys.device_confidence='high';$evidence.Add("Fabricante/modelo indica máquina virtual: $modelText")
 } else {
   try{
     $chassis=@(Get-CimInstance Win32_SystemEnclosure -ErrorAction Stop | ForEach-Object {$_.ChassisTypes} | Where-Object {$_ -ne $null} | ForEach-Object {[int]$_})
     # ChassisTypes: 3=desktop, 4=low-profile, 5=pizza box, 6=mini tower, 7=tower, 13=all-in-one, 15=space-saving, 16=lunch box, 24=sealed-case PC.
     $desktopChassis=@(3,4,5,6,7,13,15,16,24)
     $mobileChassis=@(8,9,10,11,12,14,18,21,30,31,32)
     if(@($chassis | Where-Object {$desktopChassis -contains $_}).Count -gt 0){
       $sys.device_type='desktop';$sys.device_confidence='high';$evidence.Add("Win32_SystemEnclosure.ChassisTypes=$($chassis -join ',')")
     } elseif($sys.device_type -eq 'unknown' -and @($chassis | Where-Object {$mobileChassis -contains $_}).Count -gt 0){
       $sys.device_type='mobile';$sys.device_confidence='high';$evidence.Add("Win32_SystemEnclosure.ChassisTypes=$($chassis -join ',')")
     }
   }catch{$warnings.Add('Tipo de chassi indisponível.')}
   if($sys.device_type -eq 'unknown'){
     try{$battery=@(Get-CimInstance Win32_Battery -ErrorAction Stop);if($battery.Count -gt 0){$sys.device_type='mobile';$sys.device_confidence='medium';$evidence.Add('Bateria reportada por Win32_Battery; tipo de equipamento inferido como móvel.')}}catch{$warnings.Add('Consulta auxiliar de bateria indisponível para classificação do equipamento.')}
   }
 }
 $sys.device_evidence=@($evidence)
}catch{$warnings.Add('Fabricante/modelo e classificação do equipamento indisponíveis.')}
try {
 $board=Get-CimInstance Win32_BaseBoard -ErrorAction Stop | Select-Object -First 1
 $parts=@([string]$board.Manufacturer,[string]$board.Product) | Where-Object { $_ -and $_.Trim() -and $_ -notmatch '^(?i)to be filled|default string|system product name$' }
 if($parts.Count -gt 0){$sys.motherboard=$parts -join ' '}
} catch {$warnings.Add('Placa-mãe não identificada.')}
try {
 $cpu=Get-CimInstance Win32_Processor -ErrorAction Stop | Select-Object -First 1
 $cpuName=[string]$cpu.Name -replace '\s+\d+-Core Processor
 $sys.processor=('{0} - {1:N2}GHz ({2}/{3})' -f $cpuName,($cpu.MaxClockSpeed/1000),$cpu.NumberOfCores,$cpu.NumberOfLogicalProcessors).Replace(',','.')
} catch {$warnings.Add('Processador não identificado.')}
try {
 $mem=@(Get-CimInstance Win32_PhysicalMemory -ErrorAction Stop)
 if($mem.Count -gt 0){
  $total=[math]::Round((($mem | Measure-Object -Property Capacity -Sum).Sum/1GB),0)
  $kind=switch([int]$mem[0].SMBIOSMemoryType){24 {'DDR3'} 26 {'DDR4'} 34 {'DDR5'} default {'RAM'}}
  $speeds=@($mem | ForEach-Object {if($_.ConfiguredClockSpeed){[int]$_.ConfiguredClockSpeed}elseif($_.Speed){[int]$_.Speed}} | Where-Object {$_ -gt 0} | Sort-Object -Unique)
  $speed=if($speeds.Count -eq 1){"$($speeds[0])MHz"}elseif($speeds.Count -gt 1){"$($speeds -join '/')MHz"}else{'velocidade não informada'}
  $sizes=@($mem | ForEach-Object {[math]::Round($_.Capacity/1GB,0)} | Sort-Object -Unique)
  $moduleSize=if($sizes.Count -eq 1){"$($sizes[0])GB"}else{'capacidades mistas'}
  $brands=@($mem | ForEach-Object {[string]$_.Manufacturer} | Where-Object {$_ -and $_.Trim() -and $_ -notmatch '^(?i)unknown|not specified|to be filled'} | Sort-Object -Unique)
  $brandText=if($brands.Count -gt 0){'; marca: ' + ($brands -join ', ')}else{''}
  $sys.memory=('{0}GB {1} {2} ({3}x{4}{5})' -f $total,$kind,$speed,$mem.Count,$moduleSize,$brandText)
 }
} catch {$warnings.Add('Memória RAM não identificada.')}
try {
 $gpus=@(Get-CimInstance Win32_VideoController -ErrorAction Stop | Where-Object {$_.Name} | Select-Object -ExpandProperty Name -Unique)
 if(($gpus -contains 'AMD Radeon(TM) Graphics') -and ([string]$sys.processor -match '5600GT')){$gpus=@($gpus | ForEach-Object {if($_ -eq 'AMD Radeon(TM) Graphics'){'Radeon Vega 7'}else{$_}})}
 if($gpus.Count -gt 0){$sys.gpu=$gpus -join '; '}
} catch {$warnings.Add('GPU não identificada.')}
ConvertTo-Json -InputObject ([pscustomobject]@{system=$sys;capabilities=@($caps);warnings=@($warnings)}) -Depth 8 -Compress
"#;
    let mut command = Command::new("powershell.exe");
    command.creation_flags(0x08000000).args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        SCRIPT,
    ]);
    let output = command
        .output()
        .map_err(|e| format!("Não foi possível iniciar a coleta de capacidades do Windows: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "A coleta de capacidades do Windows falhou.".into()
        } else {
            format!("A coleta de capacidades do Windows falhou: {stderr}")
        });
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|e| format!("Resposta do inventário não é UTF-8 válido: {e}"))?;
    serde_json::from_str(stdout.trim())
        .map_err(|e| format!("Não foi possível interpretar o inventário: {e}"))
}

pub(crate) fn collect() -> Result<CapabilityInventory, String> {
    #[cfg(target_os = "windows")]
    {
        collect_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("O inventário de capacidades só está disponível no Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_serialization_is_stable() {
        assert_eq!(
            serde_json::to_string(&CapabilityStatus::RegisteredNotPresent).unwrap(),
            "\"registered_not_present\""
        );
        assert_eq!(
            serde_json::to_string(&CapabilityStatus::NotApplicable).unwrap(),
            "\"not_applicable\""
        );
    }
    #[test]
    fn inventory_shape_serializes_conservatively() {
        let report = CapabilityInventory {
            system: SystemSummary {
                os_name: Some("Windows".into()),
                os_version: Some("10.0".into()),
                os_build: Some("22631".into()),
                architecture: Some("AMD64".into()),
                manufacturer: None,
                model: None,
                motherboard: None,
                processor: None,
                memory: None,
                gpu: None,
                device_type: Some("unknown".into()),
                device_confidence: Some("low".into()),
                device_evidence: vec![],
            },
            capabilities: vec![Capability {
                id: "bluetooth".into(),
                label: "Bluetooth".into(),
                status: CapabilityStatus::NotDetected,
                confidence: "low".into(),
                summary: "Nenhum dispositivo enumerado.".into(),
                sources: vec!["Get-PnpDevice".into()],
                details: vec![],
            }],
            warnings: vec![],
        };
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["capabilities"][0]["status"], "not_detected");
        assert_eq!(json["capabilities"][0]["confidence"], "low");
    }
}
,'' -replace '\s+\d+-Core
 $sys.processor=('{0} - {1:N2}GHz ({2}/{3})' -f $cpuName,($cpu.MaxClockSpeed/1000),$cpu.NumberOfCores,$cpu.NumberOfLogicalProcessors)
} catch {$warnings.Add('Processador não identificado.')}
try {
 $mem=@(Get-CimInstance Win32_PhysicalMemory -ErrorAction Stop)
 if($mem.Count -gt 0){
  $total=[math]::Round((($mem | Measure-Object -Property Capacity -Sum).Sum/1GB),0)
  $kind=switch([int]$mem[0].SMBIOSMemoryType){24 {'DDR3'} 26 {'DDR4'} 34 {'DDR5'} default {'RAM'}}
  $speeds=@($mem | ForEach-Object {if($_.ConfiguredClockSpeed){[int]$_.ConfiguredClockSpeed}elseif($_.Speed){[int]$_.Speed}} | Where-Object {$_ -gt 0} | Sort-Object -Unique)
  $speed=if($speeds.Count -eq 1){"$($speeds[0])MHz"}elseif($speeds.Count -gt 1){"$($speeds -join '/')MHz"}else{'velocidade não informada'}
  $sizes=@($mem | ForEach-Object {[math]::Round($_.Capacity/1GB,0)} | Sort-Object -Unique)
  $moduleSize=if($sizes.Count -eq 1){"$($sizes[0])GB"}else{'capacidades mistas'}
  $brands=@($mem | ForEach-Object {[string]$_.Manufacturer} | Where-Object {$_ -and $_.Trim() -and $_ -notmatch '^(?i)unknown|not specified|to be filled'} | Sort-Object -Unique)
  $brandText=if($brands.Count -gt 0){'; marca: ' + ($brands -join ', ')}else{''}
  $sys.memory=('{0}GB {1} {2} ({3}x{4}{5})' -f $total,$kind,$speed,$mem.Count,$moduleSize,$brandText)
 }
} catch {$warnings.Add('Memória RAM não identificada.')}
try {
 $gpus=@(Get-CimInstance Win32_VideoController -ErrorAction Stop | Where-Object {$_.Name} | Select-Object -ExpandProperty Name -Unique)
 if(($gpus -contains 'AMD Radeon(TM) Graphics') -and ([string]$sys.processor -match '5600GT')){$gpus=@($gpus | ForEach-Object {if($_ -eq 'AMD Radeon(TM) Graphics'){'Radeon Vega 7 (AMD Radeon(TM) Graphics)'}else{$_}})}
 if($gpus.Count -gt 0){$sys.gpu=$gpus -join '; '}
} catch {$warnings.Add('GPU não identificada.')}
ConvertTo-Json -InputObject ([pscustomobject]@{system=$sys;capabilities=@($caps);warnings=@($warnings)}) -Depth 8 -Compress
"#;
    let mut command = Command::new("powershell.exe");
    command.creation_flags(0x08000000).args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        SCRIPT,
    ]);
    let output = command
        .output()
        .map_err(|e| format!("Não foi possível iniciar a coleta de capacidades do Windows: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "A coleta de capacidades do Windows falhou.".into()
        } else {
            format!("A coleta de capacidades do Windows falhou: {stderr}")
        });
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|e| format!("Resposta do inventário não é UTF-8 válido: {e}"))?;
    serde_json::from_str(stdout.trim())
        .map_err(|e| format!("Não foi possível interpretar o inventário: {e}"))
}

pub(crate) fn collect() -> Result<CapabilityInventory, String> {
    #[cfg(target_os = "windows")]
    {
        collect_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("O inventário de capacidades só está disponível no Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_serialization_is_stable() {
        assert_eq!(
            serde_json::to_string(&CapabilityStatus::RegisteredNotPresent).unwrap(),
            "\"registered_not_present\""
        );
        assert_eq!(
            serde_json::to_string(&CapabilityStatus::NotApplicable).unwrap(),
            "\"not_applicable\""
        );
    }
    #[test]
    fn inventory_shape_serializes_conservatively() {
        let report = CapabilityInventory {
            system: SystemSummary {
                os_name: Some("Windows".into()),
                os_version: Some("10.0".into()),
                os_build: Some("22631".into()),
                architecture: Some("AMD64".into()),
                manufacturer: None,
                model: None,
                motherboard: None,
                processor: None,
                memory: None,
                gpu: None,
                device_type: Some("unknown".into()),
                device_confidence: Some("low".into()),
                device_evidence: vec![],
            },
            capabilities: vec![Capability {
                id: "bluetooth".into(),
                label: "Bluetooth".into(),
                status: CapabilityStatus::NotDetected,
                confidence: "low".into(),
                summary: "Nenhum dispositivo enumerado.".into(),
                sources: vec!["Get-PnpDevice".into()],
                details: vec![],
            }],
            warnings: vec![],
        };
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["capabilities"][0]["status"], "not_detected");
        assert_eq!(json["capabilities"][0]["confidence"], "low");
    }
}
,'' -replace '\s+with Radeon Graphics
 $sys.processor=('{0} - {1:N2}GHz ({2}/{3})' -f $cpuName,($cpu.MaxClockSpeed/1000),$cpu.NumberOfCores,$cpu.NumberOfLogicalProcessors)
} catch {$warnings.Add('Processador não identificado.')}
try {
 $mem=@(Get-CimInstance Win32_PhysicalMemory -ErrorAction Stop)
 if($mem.Count -gt 0){
  $total=[math]::Round((($mem | Measure-Object -Property Capacity -Sum).Sum/1GB),0)
  $kind=switch([int]$mem[0].SMBIOSMemoryType){24 {'DDR3'} 26 {'DDR4'} 34 {'DDR5'} default {'RAM'}}
  $speeds=@($mem | ForEach-Object {if($_.ConfiguredClockSpeed){[int]$_.ConfiguredClockSpeed}elseif($_.Speed){[int]$_.Speed}} | Where-Object {$_ -gt 0} | Sort-Object -Unique)
  $speed=if($speeds.Count -eq 1){"$($speeds[0])MHz"}elseif($speeds.Count -gt 1){"$($speeds -join '/')MHz"}else{'velocidade não informada'}
  $sizes=@($mem | ForEach-Object {[math]::Round($_.Capacity/1GB,0)} | Sort-Object -Unique)
  $moduleSize=if($sizes.Count -eq 1){"$($sizes[0])GB"}else{'capacidades mistas'}
  $brands=@($mem | ForEach-Object {[string]$_.Manufacturer} | Where-Object {$_ -and $_.Trim() -and $_ -notmatch '^(?i)unknown|not specified|to be filled'} | Sort-Object -Unique)
  $brandText=if($brands.Count -gt 0){'; marca: ' + ($brands -join ', ')}else{''}
  $sys.memory=('{0}GB {1} {2} ({3}x{4}{5})' -f $total,$kind,$speed,$mem.Count,$moduleSize,$brandText)
 }
} catch {$warnings.Add('Memória RAM não identificada.')}
try {
 $gpus=@(Get-CimInstance Win32_VideoController -ErrorAction Stop | Where-Object {$_.Name} | Select-Object -ExpandProperty Name -Unique)
 if(($gpus -contains 'AMD Radeon(TM) Graphics') -and ([string]$sys.processor -match '5600GT')){$gpus=@($gpus | ForEach-Object {if($_ -eq 'AMD Radeon(TM) Graphics'){'Radeon Vega 7 (AMD Radeon(TM) Graphics)'}else{$_}})}
 if($gpus.Count -gt 0){$sys.gpu=$gpus -join '; '}
} catch {$warnings.Add('GPU não identificada.')}
ConvertTo-Json -InputObject ([pscustomobject]@{system=$sys;capabilities=@($caps);warnings=@($warnings)}) -Depth 8 -Compress
"#;
    let mut command = Command::new("powershell.exe");
    command.creation_flags(0x08000000).args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        SCRIPT,
    ]);
    let output = command
        .output()
        .map_err(|e| format!("Não foi possível iniciar a coleta de capacidades do Windows: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "A coleta de capacidades do Windows falhou.".into()
        } else {
            format!("A coleta de capacidades do Windows falhou: {stderr}")
        });
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|e| format!("Resposta do inventário não é UTF-8 válido: {e}"))?;
    serde_json::from_str(stdout.trim())
        .map_err(|e| format!("Não foi possível interpretar o inventário: {e}"))
}

pub(crate) fn collect() -> Result<CapabilityInventory, String> {
    #[cfg(target_os = "windows")]
    {
        collect_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("O inventário de capacidades só está disponível no Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_serialization_is_stable() {
        assert_eq!(
            serde_json::to_string(&CapabilityStatus::RegisteredNotPresent).unwrap(),
            "\"registered_not_present\""
        );
        assert_eq!(
            serde_json::to_string(&CapabilityStatus::NotApplicable).unwrap(),
            "\"not_applicable\""
        );
    }
    #[test]
    fn inventory_shape_serializes_conservatively() {
        let report = CapabilityInventory {
            system: SystemSummary {
                os_name: Some("Windows".into()),
                os_version: Some("10.0".into()),
                os_build: Some("22631".into()),
                architecture: Some("AMD64".into()),
                manufacturer: None,
                model: None,
                motherboard: None,
                processor: None,
                memory: None,
                gpu: None,
                device_type: Some("unknown".into()),
                device_confidence: Some("low".into()),
                device_evidence: vec![],
            },
            capabilities: vec![Capability {
                id: "bluetooth".into(),
                label: "Bluetooth".into(),
                status: CapabilityStatus::NotDetected,
                confidence: "low".into(),
                summary: "Nenhum dispositivo enumerado.".into(),
                sources: vec!["Get-PnpDevice".into()],
                details: vec![],
            }],
            warnings: vec![],
        };
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["capabilities"][0]["status"], "not_detected");
        assert_eq!(json["capabilities"][0]["confidence"], "low");
    }
}
,''
 $sys.processor=('{0} - {1:N2}GHz ({2}/{3})' -f $cpuName,($cpu.MaxClockSpeed/1000),$cpu.NumberOfCores,$cpu.NumberOfLogicalProcessors)
} catch {$warnings.Add('Processador não identificado.')}
try {
 $mem=@(Get-CimInstance Win32_PhysicalMemory -ErrorAction Stop)
 if($mem.Count -gt 0){
  $total=[math]::Round((($mem | Measure-Object -Property Capacity -Sum).Sum/1GB),0)
  $kind=switch([int]$mem[0].SMBIOSMemoryType){24 {'DDR3'} 26 {'DDR4'} 34 {'DDR5'} default {'RAM'}}
  $speeds=@($mem | ForEach-Object {if($_.ConfiguredClockSpeed){[int]$_.ConfiguredClockSpeed}elseif($_.Speed){[int]$_.Speed}} | Where-Object {$_ -gt 0} | Sort-Object -Unique)
  $speed=if($speeds.Count -eq 1){"$($speeds[0])MHz"}elseif($speeds.Count -gt 1){"$($speeds -join '/')MHz"}else{'velocidade não informada'}
  $sizes=@($mem | ForEach-Object {[math]::Round($_.Capacity/1GB,0)} | Sort-Object -Unique)
  $moduleSize=if($sizes.Count -eq 1){"$($sizes[0])GB"}else{'capacidades mistas'}
  $brands=@($mem | ForEach-Object {[string]$_.Manufacturer} | Where-Object {$_ -and $_.Trim() -and $_ -notmatch '^(?i)unknown|not specified|to be filled'} | Sort-Object -Unique)
  $brandText=if($brands.Count -gt 0){'; marca: ' + ($brands -join ', ')}else{''}
  $sys.memory=('{0}GB {1} {2} ({3}x{4}{5})' -f $total,$kind,$speed,$mem.Count,$moduleSize,$brandText)
 }
} catch {$warnings.Add('Memória RAM não identificada.')}
try {
 $gpus=@(Get-CimInstance Win32_VideoController -ErrorAction Stop | Where-Object {$_.Name} | Select-Object -ExpandProperty Name -Unique)
 if(($gpus -contains 'AMD Radeon(TM) Graphics') -and ([string]$sys.processor -match '5600GT')){$gpus=@($gpus | ForEach-Object {if($_ -eq 'AMD Radeon(TM) Graphics'){'Radeon Vega 7 (AMD Radeon(TM) Graphics)'}else{$_}})}
 if($gpus.Count -gt 0){$sys.gpu=$gpus -join '; '}
} catch {$warnings.Add('GPU não identificada.')}
ConvertTo-Json -InputObject ([pscustomobject]@{system=$sys;capabilities=@($caps);warnings=@($warnings)}) -Depth 8 -Compress
"#;
    let mut command = Command::new("powershell.exe");
    command.creation_flags(0x08000000).args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        SCRIPT,
    ]);
    let output = command
        .output()
        .map_err(|e| format!("Não foi possível iniciar a coleta de capacidades do Windows: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "A coleta de capacidades do Windows falhou.".into()
        } else {
            format!("A coleta de capacidades do Windows falhou: {stderr}")
        });
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|e| format!("Resposta do inventário não é UTF-8 válido: {e}"))?;
    serde_json::from_str(stdout.trim())
        .map_err(|e| format!("Não foi possível interpretar o inventário: {e}"))
}

pub(crate) fn collect() -> Result<CapabilityInventory, String> {
    #[cfg(target_os = "windows")]
    {
        collect_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("O inventário de capacidades só está disponível no Windows.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_serialization_is_stable() {
        assert_eq!(
            serde_json::to_string(&CapabilityStatus::RegisteredNotPresent).unwrap(),
            "\"registered_not_present\""
        );
        assert_eq!(
            serde_json::to_string(&CapabilityStatus::NotApplicable).unwrap(),
            "\"not_applicable\""
        );
    }
    #[test]
    fn inventory_shape_serializes_conservatively() {
        let report = CapabilityInventory {
            system: SystemSummary {
                os_name: Some("Windows".into()),
                os_version: Some("10.0".into()),
                os_build: Some("22631".into()),
                architecture: Some("AMD64".into()),
                manufacturer: None,
                model: None,
                motherboard: None,
                processor: None,
                memory: None,
                gpu: None,
                device_type: Some("unknown".into()),
                device_confidence: Some("low".into()),
                device_evidence: vec![],
            },
            capabilities: vec![Capability {
                id: "bluetooth".into(),
                label: "Bluetooth".into(),
                status: CapabilityStatus::NotDetected,
                confidence: "low".into(),
                summary: "Nenhum dispositivo enumerado.".into(),
                sources: vec!["Get-PnpDevice".into()],
                details: vec![],
            }],
            warnings: vec![],
        };
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["capabilities"][0]["status"], "not_detected");
        assert_eq!(json["capabilities"][0]["confidence"], "low");
    }
}
