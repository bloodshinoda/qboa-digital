//! Inventário de módulos físicos de memória reportados pelo Windows.
//!
//! O Windows/WMI não expõe de forma portável os timings CAS nem os perfis
//! XMP/EXPO gravados no SPD. Esses campos são retornados como indisponíveis,
//! em vez de inferidos a partir da frequência nominal.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct MemoryModule {
    pub device_locator: Option<String>,
    pub bank_label: Option<String>,
    pub capacity_bytes: Option<u64>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub memory_type: Option<String>,
    pub configured_speed_mhz: Option<u32>,
    pub rated_speed_mhz: Option<u32>,
    pub form_factor: Option<String>,
    pub data_width_bits: Option<u32>,
    pub total_width_bits: Option<u32>,
    pub configured_voltage_mv: Option<u32>,
    pub attributes: Option<u32>,
    pub xmp_expo_max_speed_mhz: Option<u32>,
    pub cas_latency: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct MemoryInventory {
    pub total_physical_bytes: u64,
    pub available_physical_bytes: u64,
    pub slot_count: Option<u32>,
    pub modules: Vec<MemoryModule>,
    pub warnings: Vec<String>,
    pub timing_limitation: String,
}

#[cfg(target_os = "windows")]
pub(crate) fn collect() -> Result<MemoryInventory, String> {
    use std::{os::windows::process::CommandExt, process::Command};

    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$os = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
$cs = Get-CimInstance Win32_ComputerSystem -ErrorAction Stop
$dimm = @(Get-CimInstance Win32_PhysicalMemory -ErrorAction Stop | Sort-Object DeviceLocator, BankLabel)
$arrays = @(Get-CimInstance Win32_PhysicalMemoryArray -ErrorAction SilentlyContinue)
$slots = $null
if ($arrays.Count -gt 0) {
  $slotSum = ($arrays | Measure-Object -Property MemoryDevices -Sum).Sum
  if ($null -ne $slotSum -and [int]$slotSum -gt 0) { $slots = [int]$slotSum }
}
function Clean-Text($value) {
  if ($null -eq $value) { return $null }
  $text = ([string]$value).Trim()
  if (!$text -or $text -match '(?i)^(unknown|not specified|to be filled by o\.e\.m\.|to be filled|n/?a)$') { return $null }
  return $text
}
function Memory-TypeName($code) {
  switch ([int]$code) {
    20 { 'DDR' }
    21 { 'DDR2' }
    24 { 'DDR3' }
    26 { 'DDR4' }
    30 { 'LPDDR3' }
    34 { 'DDR5' }
    35 { 'LPDDR5' }
    0 { $null }
    default { if ([int]$code -gt 0) { "SMBIOS $code" } else { $null } }
  }
}
function Positive-UInt($value) {
  if ($null -ne $value -and [uint64]$value -gt 0) { return [uint64]$value }
  return $null
}
$modules = @($dimm | ForEach-Object {
  $configured = Positive-UInt $_.ConfiguredClockSpeed
  $rated = Positive-UInt $_.Speed
  $form = switch ([int]$_.FormFactor) {
    8 { 'DIMM' }
    12 { 'SO-DIMM' }
    13 { 'Micro-DIMM' }
    14 { 'Mini-DIMM' }
    0 { $null }
    default { if ([int]$_.FormFactor -gt 0) { "Código $($_.FormFactor)" } else { $null } }
  }
  [pscustomobject]@{
    device_locator = Clean-Text $_.DeviceLocator
    bank_label = Clean-Text $_.BankLabel
    capacity_bytes = Positive-UInt $_.Capacity
    manufacturer = Clean-Text $_.Manufacturer
    part_number = Clean-Text $_.PartNumber
    memory_type = Memory-TypeName $_.SMBIOSMemoryType
    configured_speed_mhz = $configured
    rated_speed_mhz = $rated
    form_factor = $form
    data_width_bits = Positive-UInt $_.DataWidth
    total_width_bits = Positive-UInt $_.TotalWidth
    configured_voltage_mv = Positive-UInt $_.ConfiguredVoltage
    attributes = Positive-UInt $_.Attributes
    # Win32_PhysicalMemory não fornece CAS latency nem XMP/EXPO.
    xmp_expo_max_speed_mhz = $null
    cas_latency = $null
  }
})
$timingNote = 'Latência CAS e frequência máxima de perfis XMP/EXPO não são expostas pela API CIM/WMI padrão; não foram inferidas.'
[pscustomobject]@{
  total_physical_bytes = [uint64]$cs.TotalPhysicalMemory
  available_physical_bytes = [uint64]$os.FreePhysicalMemory * 1KB
  slot_count = $slots
  modules = @($modules)
  warnings = @()
  timing_limitation = $timingNote
} | ConvertTo-Json -Depth 6 -Compress
"#;

    let encoded = base64_encode(
        &format!(
            "$utf8=[System.Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=$utf8;{SCRIPT}"
        )
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>(),
    );
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ])
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|error| format!("Não foi possível consultar os módulos de RAM: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Consulta dos módulos de RAM falhou: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let json = String::from_utf8_lossy(&output.stdout);
    let inventory: MemoryInventory = serde_json::from_str(json.trim_start_matches('\u{feff}'))
        .map_err(|error| format!("Resposta do inventário de RAM inválida: {error}"))?;
    if inventory.total_physical_bytes == 0 {
        return Err("O Windows não retornou o total de memória física.".to_string());
    }
    Ok(inventory)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn collect() -> Result<MemoryInventory, String> {
    Err("O inventário detalhado de memória está disponível apenas no Windows.".to_string())
}

#[cfg(target_os = "windows")]
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_inventory_serializes_limitation_without_inventing_timings() {
        let inventory = MemoryInventory {
            total_physical_bytes: 16 * 1024 * 1024 * 1024,
            available_physical_bytes: 8 * 1024 * 1024 * 1024,
            slot_count: Some(4),
            modules: vec![MemoryModule {
                device_locator: Some("DIMM_A1".to_string()),
                bank_label: None,
                capacity_bytes: Some(8 * 1024 * 1024 * 1024),
                manufacturer: Some("Example".to_string()),
                part_number: Some("Example-8G".to_string()),
                memory_type: Some("DDR4".to_string()),
                configured_speed_mhz: Some(3200),
                rated_speed_mhz: Some(3200),
                form_factor: Some("DIMM".to_string()),
                data_width_bits: Some(64),
                total_width_bits: Some(72),
                configured_voltage_mv: Some(1200),
                attributes: None,
                xmp_expo_max_speed_mhz: None,
                cas_latency: None,
            }],
            warnings: vec![],
            timing_limitation: "Não exposto pela API padrão.".to_string(),
        };
        let json = serde_json::to_value(inventory).unwrap();
        assert_eq!(json["modules"][0]["configured_speed_mhz"], 3200);
        assert!(json["modules"][0]["xmp_expo_max_speed_mhz"].is_null());
        assert!(json["modules"][0]["cas_latency"].is_null());
        assert_eq!(json["slot_count"], 4);
    }
}
