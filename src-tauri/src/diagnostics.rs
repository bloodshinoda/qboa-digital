use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticMode {
    Quick,
    Complete,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticCategory {
    Sistema,
    Armazenamento,
    MemoriaRam,
    PlacaDeVideo,
    Processador,
    Temperatura,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticStatus {
    Ok,
    Atencao,
    Falha,
    Indisponivel,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct DiagnosticResult {
    pub category: DiagnosticCategory,
    pub status: DiagnosticStatus,
    pub evidence: Vec<String>,
    pub source: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct DiagnosticReport {
    pub mode: DiagnosticMode,
    pub results: Vec<DiagnosticResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct DiskData {
    pub name: Option<String>,
    pub size_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Default)]
#[serde(default)]
pub(crate) struct DiagnosticData {
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub os_build: Option<String>,
    pub os_edition: Option<String>,
    pub disks: Vec<DiskData>,
    pub ram_total_bytes: Option<u64>,
    pub ram_available_bytes: Option<u64>,
    pub gpu_names: Vec<String>,
    pub gpu_sensor_temperature_c: Option<f64>,
    pub gpu_sensor_source: Option<String>,
    pub processor_name: Option<String>,
    pub processor_logical_count: Option<u32>,
    pub thermal_zone_temperature_c: Option<f64>,
    pub thermal_zone_source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtendedCheck {
    pub evidence: String,
    pub succeeded: bool,
}

pub(crate) trait DiagnosticBackend {
    fn collect(&mut self) -> Result<DiagnosticData, String>;
    fn run_extended_checks(&mut self) -> Result<Vec<ExtendedCheck>, String>;
}

pub(crate) fn run<B: DiagnosticBackend>(
    backend: &mut B,
    mode: DiagnosticMode,
) -> Result<DiagnosticReport, String> {
    let data = backend.collect()?;
    let mut results = build_results(&data);
    if mode == DiagnosticMode::Complete {
        let checks = backend.run_extended_checks()?;
        let system = results
            .iter_mut()
            .find(|result| result.category == DiagnosticCategory::Sistema)
            .expect("system result is always created");
        if checks.iter().any(|check| !check.succeeded) {
            system.status = DiagnosticStatus::Falha;
        }
        for check in checks {
            system.evidence.push(check.evidence);
            system.source.push("SFC / DISM".to_string());
        }
        system.source.sort();
        system.source.dedup();
    }
    Ok(DiagnosticReport { mode, results })
}

fn build_results(data: &DiagnosticData) -> Vec<DiagnosticResult> {
    let mut system_evidence = Vec::new();
    if let Some(name) = data.os_name.as_deref() {
        system_evidence.push(name.to_string());
    }
    if let Some(version) = data.os_version.as_deref() {
        system_evidence.push(format!("Versão {version}"));
    }
    if let Some(build) = data.os_build.as_deref() {
        system_evidence.push(format!("Build {build}"));
    }
    if let Some(edition) = data.os_edition.as_deref() {
        system_evidence.push(format!("Edição {edition}"));
    }
    let system_sources = if system_evidence.is_empty() {
        Vec::new()
    } else {
        vec!["Win32_OperatingSystem e registro do Windows".to_string()]
    };

    let valid_disks: Vec<(&str, u64, u64)> = data
        .disks
        .iter()
        .filter_map(|disk| {
            let (Some(name), Some(size), Some(free)) =
                (disk.name.as_deref(), disk.size_bytes, disk.free_bytes)
            else {
                return None;
            };
            if name.trim().is_empty() || size == 0 || free > size {
                return None;
            }
            Some((name, size, free))
        })
        .collect();
    let storage_status = if valid_disks.is_empty() {
        DiagnosticStatus::Indisponivel
    } else if valid_disks
        .iter()
        .any(|(_, size, free)| *free as f64 / (*size as f64) < 0.10)
    {
        DiagnosticStatus::Atencao
    } else {
        DiagnosticStatus::Ok
    };
    let storage_evidence = valid_disks
        .iter()
        .map(|(name, size, free)| {
            format!(
                "{name}: {} livres de {} ({:.1}%)",
                format_bytes(*free),
                format_bytes(*size),
                *free as f64 * 100.0 / *size as f64
            )
        })
        .collect();
    let storage_sources = if valid_disks.is_empty() {
        Vec::new()
    } else {
        vec!["Win32_LogicalDisk".to_string()]
    };

    let ram_evidence = match (data.ram_total_bytes, data.ram_available_bytes) {
        (Some(total), Some(available)) if total > 0 && available <= total => vec![format!(
            "{} totais; {} disponíveis",
            format_bytes(total),
            format_bytes(available)
        )],
        _ => Vec::new(),
    };
    let ram_sources = if ram_evidence.is_empty() {
        Vec::new()
    } else {
        vec!["Win32_OperatingSystem".to_string()]
    };

    let gpu_temperature = data
        .gpu_sensor_temperature_c
        .filter(|temperature| temperature.is_finite() && (0.0..=150.0).contains(temperature));
    let mut gpu_evidence: Vec<String> = data
        .gpu_names
        .iter()
        .filter(|name| !name.trim().is_empty())
        .cloned()
        .collect();
    if let (Some(temperature), Some(_)) = (
        gpu_temperature,
        data.gpu_sensor_source
            .as_deref()
            .filter(|source| !source.trim().is_empty()),
    ) {
        gpu_evidence.push(format!("Sensor GPU: {temperature:.1} °C"));
    }
    let gpu_sources = if gpu_evidence.is_empty() {
        Vec::new()
    } else {
        let mut sources = Vec::new();
        if !data.gpu_names.is_empty() {
            sources.push("Win32_VideoController".to_string());
        }
        if gpu_temperature.is_some() {
            sources.extend(
                data.gpu_sensor_source
                    .as_deref()
                    .filter(|source| !source.trim().is_empty())
                    .map(str::to_string),
            );
        }
        sources
    };

    let mut processor_evidence = Vec::new();
    if let Some(name) = data
        .processor_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        processor_evidence.push(name.to_string());
    }
    if let Some(count) = data.processor_logical_count.filter(|count| *count > 0) {
        processor_evidence.push(format!("{count} processadores lógicos"));
    }
    let processor_sources = if processor_evidence.is_empty() {
        Vec::new()
    } else {
        vec!["Win32_Processor".to_string()]
    };

    let mut temperature_evidence = Vec::new();
    let mut temperature_sources = Vec::new();
    if let Some(temperature) = data
        .thermal_zone_temperature_c
        .filter(|temperature| temperature.is_finite() && (0.0..=150.0).contains(temperature))
    {
        if let Some(source) = data
            .thermal_zone_source
            .as_deref()
            .filter(|source| !source.trim().is_empty())
        {
            temperature_evidence.push(format!("Zona térmica: {temperature:.1} °C"));
            temperature_sources.push(source.to_string());
        }
    }
    if let (Some(temperature), Some(source)) = (
        gpu_temperature,
        data.gpu_sensor_source
            .as_deref()
            .filter(|source| !source.trim().is_empty()),
    ) {
        temperature_evidence.push(format!("GPU: {temperature:.1} °C"));
        temperature_sources.push(source.to_string());
    }

    vec![
        result(
            DiagnosticCategory::Sistema,
            if system_evidence.is_empty() {
                DiagnosticStatus::Indisponivel
            } else {
                DiagnosticStatus::Ok
            },
            system_evidence,
            system_sources,
        ),
        result(
            DiagnosticCategory::Armazenamento,
            storage_status,
            storage_evidence,
            storage_sources,
        ),
        result(
            DiagnosticCategory::MemoriaRam,
            if ram_evidence.is_empty() {
                DiagnosticStatus::Indisponivel
            } else {
                DiagnosticStatus::Ok
            },
            ram_evidence,
            ram_sources,
        ),
        result(
            DiagnosticCategory::PlacaDeVideo,
            if gpu_evidence.is_empty() {
                DiagnosticStatus::Indisponivel
            } else {
                DiagnosticStatus::Ok
            },
            gpu_evidence,
            gpu_sources,
        ),
        result(
            DiagnosticCategory::Processador,
            if processor_evidence.is_empty() {
                DiagnosticStatus::Indisponivel
            } else {
                DiagnosticStatus::Ok
            },
            processor_evidence,
            processor_sources,
        ),
        result(
            DiagnosticCategory::Temperatura,
            if temperature_evidence.is_empty() {
                DiagnosticStatus::Indisponivel
            } else {
                DiagnosticStatus::Ok
            },
            temperature_evidence,
            temperature_sources,
        ),
    ]
}

fn result(
    category: DiagnosticCategory,
    status: DiagnosticStatus,
    evidence: Vec<String>,
    source: Vec<String>,
) -> DiagnosticResult {
    DiagnosticResult {
        category,
        status,
        evidence: if evidence.is_empty() {
            vec!["Dado confiável não disponível.".to_string()]
        } else {
            evidence
        },
        source: if source.is_empty() {
            vec!["Provedor não disponível.".to_string()]
        } else {
            source
        },
    }
}

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    format!("{:.1} GiB", bytes as f64 / GIB)
}

#[cfg(target_os = "windows")]
pub(crate) struct WindowsDiagnosticBackend;

#[cfg(target_os = "windows")]
impl Default for WindowsDiagnosticBackend {
    fn default() -> Self {
        Self
    }
}

#[cfg(target_os = "windows")]
impl DiagnosticBackend for WindowsDiagnosticBackend {
    fn collect(&mut self) -> Result<DiagnosticData, String> {
        use std::process::Command;

        let script = r#"
$ErrorActionPreference = 'Stop'
$os = $null
$current = $null
$disks = @()
$gpus = @()
$processors = @()
try { $os = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop } catch {}
try { $current = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction Stop } catch {}
try {
  $disks = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' -ErrorAction Stop | ForEach-Object {
    [pscustomobject]@{ name=$_.DeviceID; size_bytes=$_.Size; free_bytes=$_.FreeSpace }
  })
} catch {}
try {
  $gpus = @(Get-CimInstance Win32_VideoController -ErrorAction Stop | ForEach-Object { [string]$_.Name } | Where-Object { $_ })
} catch {}
try { $processors = @(Get-CimInstance Win32_Processor -ErrorAction Stop) } catch {}
$thermal = @()
try {
  $thermal = @(Get-CimInstance -Namespace 'root/wmi' -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction Stop | ForEach-Object {
    $c = ($_.CurrentTemperature / 10) - 273.15
    if ($c -ge 0 -and $c -le 150) { [double]$c }
  } | Where-Object { $_ -ne $null })
} catch {}
$gpuTemperature = $null
$gpuTemperatureSource = $null
foreach ($namespace in @('root/LibreHardwareMonitor', 'root/OpenHardwareMonitor')) {
  try {
    $gpuSensors = @(Get-CimInstance -Namespace $namespace -ClassName Sensor -ErrorAction Stop | Where-Object {
      $_.SensorType -eq 'Temperature' -and $_.HardwareType -like 'Gpu*' -and $_.Value -ge 0 -and $_.Value -le 150
    })
    if ($gpuSensors.Count -gt 0) {
      $gpuTemperature = [double]$gpuSensors[0].Value
      $gpuTemperatureSource = "$namespace Sensor"
      break
    }
  } catch {}
}
[pscustomobject]@{
  os_name=if ($current) { [string]$current.ProductName } else { $null }
  os_version=if ($current) { [string]$current.DisplayVersion } else { $null }
  os_build=if ($os) { [string]$os.BuildNumber } elseif ($current) { [string]$current.CurrentBuildNumber } else { $null }
  os_edition=if ($current) { [string]$current.EditionID } else { $null }
  disks=$disks
  ram_total_bytes=if ($os) { ([uint64]$os.TotalVisibleMemorySize * 1KB) } else { $null }
  ram_available_bytes=if ($os) { ([uint64]$os.FreePhysicalMemory * 1KB) } else { $null }
  gpu_names=$gpus
  gpu_sensor_temperature_c=$gpuTemperature
  gpu_sensor_source=$gpuTemperatureSource
  processor_name=if ($processors.Count -gt 0) { [string]$processors[0].Name } else { $null }
  processor_logical_count=if ($processors.Count -gt 0) { [uint32]$processors[0].NumberOfLogicalProcessors } else { $null }
  thermal_zone_temperature_c=if ($thermal.Count -gt 0) { [double]$thermal[0] } else { $null }
  thermal_zone_source=if ($thermal.Count -gt 0) { 'MSAcpi_ThermalZoneTemperature' } else { $null }
} | ConvertTo-Json -Depth 5 -Compress
"#;
        let output = powershell_output(script)?;
        serde_json::from_str(output.trim_start_matches('\u{feff}'))
            .map_err(|error| format!("Resposta do diagnóstico inválida: {error}"))
    }

    fn run_extended_checks(&mut self) -> Result<Vec<ExtendedCheck>, String> {
        use std::process::Command;

        let mut results = Vec::new();
        for (program, args) in [
            ("sfc", vec!["/verifyonly"]),
            ("dism", vec!["/Online", "/Cleanup-Image", "/ScanHealth"]),
        ] {
            match Command::new(program).args(&args).output() {
                Ok(output) => {
                    let succeeded = output.status.success();
                    let text = if output.stdout.is_empty() {
                        String::from_utf8_lossy(&output.stderr)
                    } else {
                        String::from_utf8_lossy(&output.stdout)
                    };
                    results.push(ExtendedCheck {
                        evidence: format!(
                            "{program} {}: código {}; {}",
                            args.join(" "),
                            output.status.code().unwrap_or(-1),
                            text.lines().last().unwrap_or("Sem resumo de saída.")
                        ),
                        succeeded,
                    });
                }
                Err(error) => results.push(ExtendedCheck {
                    evidence: format!("Não foi possível executar {program}: {error}"),
                    succeeded: false,
                }),
            }
        }
        Ok(results)
    }
}

#[cfg(not(target_os = "windows"))]
#[derive(Default)]
pub(crate) struct WindowsDiagnosticBackend;

#[cfg(not(target_os = "windows"))]
impl DiagnosticBackend for WindowsDiagnosticBackend {
    fn collect(&mut self) -> Result<DiagnosticData, String> {
        Err("Diagnóstico de hardware disponível apenas no Windows.".to_string())
    }

    fn run_extended_checks(&mut self) -> Result<Vec<ExtendedCheck>, String> {
        Err("Verificação completa disponível apenas no Windows.".to_string())
    }
}

#[cfg(target_os = "windows")]
fn powershell_output(script: &str) -> Result<String, String> {
    use std::process::Command;

    let encoded = base64_encode(
        &format!(
            "$utf8=[System.Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=$utf8;{script}"
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
        .output()
        .map_err(|error| format!("Não foi possível consultar o sistema: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Consulta do diagnóstico falhou: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "windows")]
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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

    #[derive(Default)]
    struct FakeBackend {
        data: DiagnosticData,
        extended_calls: usize,
        extended_failure: bool,
    }

    impl DiagnosticBackend for FakeBackend {
        fn collect(&mut self) -> Result<DiagnosticData, String> {
            Ok(self.data.clone())
        }

        fn run_extended_checks(&mut self) -> Result<Vec<ExtendedCheck>, String> {
            self.extended_calls += 1;
            Ok(vec![
                ExtendedCheck {
                    evidence: "sfc /verifyonly: código 0".to_string(),
                    succeeded: !self.extended_failure,
                },
                ExtendedCheck {
                    evidence: "dism /Online /Cleanup-Image /ScanHealth: código 0".to_string(),
                    succeeded: true,
                },
            ])
        }
    }

    #[test]
    fn quick_check_does_not_run_sfc_or_dism() {
        let mut backend = FakeBackend::default();
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(backend.extended_calls, 0);
        assert_eq!(report.results.len(), 6);
    }

    #[test]
    fn complete_check_runs_extended_checks() {
        let mut backend = FakeBackend::default();
        let report = run(&mut backend, DiagnosticMode::Complete).unwrap();
        assert_eq!(backend.extended_calls, 1);
        let system = report
            .results
            .iter()
            .find(|result| result.category == DiagnosticCategory::Sistema)
            .unwrap();
        assert!(system
            .evidence
            .iter()
            .any(|item| item.contains("sfc /verifyonly")));
        assert!(system.evidence.iter().any(|item| item.contains("dism")));
    }

    #[test]
    fn failed_extended_check_is_reported_as_failure() {
        let mut backend = FakeBackend {
            extended_failure: true,
            ..FakeBackend::default()
        };
        let report = run(&mut backend, DiagnosticMode::Complete).unwrap();
        let system = report
            .results
            .iter()
            .find(|result| result.category == DiagnosticCategory::Sistema)
            .unwrap();
        assert_eq!(system.status, DiagnosticStatus::Falha);
    }

    #[test]
    fn absent_sensors_are_unavailable_without_zero_values() {
        let mut backend = FakeBackend::default();
        backend.data.processor_name = Some("Example CPU".to_string());
        backend.data.processor_logical_count = Some(8);
        backend.data.gpu_names = vec!["Example GPU".to_string()];
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let temperature = report
            .results
            .iter()
            .find(|result| result.category == DiagnosticCategory::Temperatura)
            .unwrap();
        let processor = report
            .results
            .iter()
            .find(|result| result.category == DiagnosticCategory::Processador)
            .unwrap();
        let gpu = report
            .results
            .iter()
            .find(|result| result.category == DiagnosticCategory::PlacaDeVideo)
            .unwrap();
        assert_eq!(temperature.status, DiagnosticStatus::Indisponivel);
        assert!(temperature
            .evidence
            .iter()
            .all(|item| !item.contains("0 °C")));
        assert_eq!(processor.status, DiagnosticStatus::Ok);
        assert_eq!(gpu.status, DiagnosticStatus::Ok);
    }

    #[test]
    fn absent_gpu_sensor_does_not_hide_thermal_zone_temperature() {
        let mut backend = FakeBackend::default();
        backend.data.thermal_zone_temperature_c = Some(47.0);
        backend.data.thermal_zone_source = Some("Fake thermal provider".to_string());
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let temperature = report
            .results
            .iter()
            .find(|result| result.category == DiagnosticCategory::Temperatura)
            .unwrap();
        assert_eq!(temperature.status, DiagnosticStatus::Ok);
        assert_eq!(temperature.evidence, ["Zona térmica: 47.0 °C"]);
    }

    #[test]
    fn gpu_sensor_is_reported_only_when_provider_supplies_a_value_and_source() {
        let mut backend = FakeBackend::default();
        backend.data.gpu_names = vec!["Example GPU".to_string()];
        backend.data.gpu_sensor_temperature_c = Some(51.5);
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let gpu = report
            .results
            .iter()
            .find(|result| result.category == DiagnosticCategory::PlacaDeVideo)
            .unwrap();
        assert_eq!(gpu.status, DiagnosticStatus::Ok);
        assert!(!gpu.evidence.iter().any(|item| item.contains("Sensor GPU")));

        backend.data.gpu_sensor_source = Some("Optional fake sensor".to_string());
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let gpu = report
            .results
            .iter()
            .find(|result| result.category == DiagnosticCategory::PlacaDeVideo)
            .unwrap();
        assert!(gpu.evidence.iter().any(|item| item.contains("51.5 °C")));
        assert!(gpu.source.iter().any(|item| item == "Optional fake sensor"));
    }

    #[test]
    fn storage_provider_failure_does_not_change_other_categories() {
        let mut backend = FakeBackend::default();
        backend.data.ram_total_bytes = Some(8 * 1024 * 1024 * 1024);
        backend.data.ram_available_bytes = Some(4 * 1024 * 1024 * 1024);
        backend.data.disks = vec![DiskData {
            name: Some("C:".to_string()),
            size_bytes: Some(0),
            free_bytes: None,
        }];
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let storage = report
            .results
            .iter()
            .find(|result| result.category == DiagnosticCategory::Armazenamento)
            .unwrap();
        let ram = report
            .results
            .iter()
            .find(|result| result.category == DiagnosticCategory::MemoriaRam)
            .unwrap();
        assert_eq!(storage.status, DiagnosticStatus::Indisponivel);
        assert_eq!(ram.status, DiagnosticStatus::Ok);
        assert!(storage.evidence.iter().all(|item| !item.contains("0 GiB")));
    }

    #[test]
    fn low_storage_is_attention_and_all_categories_have_evidence_and_source() {
        let mut backend = FakeBackend::default();
        backend.data.os_name = Some("Windows test".to_string());
        backend.data.disks = vec![DiskData {
            name: Some("C:".to_string()),
            size_bytes: Some(1000),
            free_bytes: Some(50),
        }];
        backend.data.ram_total_bytes = Some(16_000);
        backend.data.ram_available_bytes = Some(8_000);
        backend.data.gpu_names = vec!["Example GPU".to_string()];
        backend.data.processor_name = Some("Example CPU".to_string());
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(report.results.len(), 6);
        assert!(report
            .results
            .iter()
            .all(|result| !result.evidence.is_empty() && !result.source.is_empty()));
        assert_eq!(
            report
                .results
                .iter()
                .find(|result| result.category == DiagnosticCategory::Armazenamento)
                .unwrap()
                .status,
            DiagnosticStatus::Atencao
        );
    }
}
