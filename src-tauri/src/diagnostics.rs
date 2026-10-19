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
    Informativo,
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
    pub checks: Vec<DiagnosticCheck>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct DiagnosticCheck {
    pub name: String,
    pub status: DiagnosticStatus,
    pub evidence: String,
    pub source: String,
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
    pub cpu_sensor_temperature_c: Option<f64>,
    pub cpu_sensor_source: Option<String>,
    pub processor_name: Option<String>,
    pub processor_logical_count: Option<u32>,
    pub thermal_zone_temperature_c: Option<f64>,
    pub thermal_zone_source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtendedCheck {
    pub name: String,
    pub evidence: String,
    pub status: DiagnosticStatus,
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
    let results = build_results(&data);
    let checks = if mode == DiagnosticMode::Complete {
        match backend.run_extended_checks() {
            Ok(checks) if !checks.is_empty() => checks
                .into_iter()
                .map(|check| DiagnosticCheck {
                    name: check.name,
                    status: check.status,
                    evidence: check.evidence,
                    source: "Verificação do Windows".to_string(),
                })
                .collect(),
            Ok(_) => unavailable_checks("O provedor não retornou resultados."),
            Err(error) => unavailable_checks(&error),
        }
    } else {
        Vec::new()
    };
    Ok(DiagnosticReport {
        mode,
        results,
        checks,
    })
}

fn build_results(data: &DiagnosticData) -> Vec<DiagnosticResult> {
    let mut system_evidence = Vec::new();
    if let Some(name) = data.os_name.as_deref() {
        if !name.trim().is_empty() {
            system_evidence.push(format!("Sistema: {name}"));
        }
    }
    if let Some(version) = data.os_version.as_deref() {
        if !version.trim().is_empty() {
            system_evidence.push(format!("Versão {version}"));
        }
    }
    if let Some(build) = data.os_build.as_deref() {
        if !build.trim().is_empty() {
            system_evidence.push(format!("Build {build}"));
        }
    }
    if let Some(edition) = data.os_edition.as_deref() {
        if !edition.trim().is_empty() {
            system_evidence.push(format!("Edição {edition}"));
        }
    }
    let system_status = if system_evidence.is_empty() {
        system_evidence.push("Identificação do sistema não disponível.".to_string());
        DiagnosticStatus::Indisponivel
    } else {
        DiagnosticStatus::Informativo
    };
    let system_sources = if system_status == DiagnosticStatus::Indisponivel {
        Vec::new()
    } else {
        vec!["Win32_OperatingSystem e registro do Windows".to_string()]
    };

    let mut valid_disks = Vec::new();
    let mut invalid_disks = 0;
    for disk in &data.disks {
        match (disk.name.as_deref(), disk.size_bytes, disk.free_bytes) {
            (Some(name), Some(size), Some(free))
                if !name.trim().is_empty() && size > 0 && free <= size =>
            {
                valid_disks.push((name, size, free));
            }
            _ => invalid_disks += 1,
        }
    }
    let low_space = valid_disks
        .iter()
        .any(|(_, size, free)| *free as f64 / (*size as f64) < 0.10);
    let storage_status = if low_space {
        DiagnosticStatus::Atencao
    } else if valid_disks.is_empty() || invalid_disks > 0 {
        DiagnosticStatus::Indisponivel
    } else {
        // A free-space check is not a disk-health test.
        DiagnosticStatus::Informativo
    };
    let mut storage_evidence: Vec<String> = valid_disks
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
    if low_space {
        storage_evidence.push("Alerta: espaço livre abaixo de 10%.".to_string());
    } else if !valid_disks.is_empty() {
        storage_evidence.push(
            "Espaço livre acima do limite de alerta; isso não testa a saúde do disco.".to_string(),
        );
    }
    if invalid_disks > 0 {
        storage_evidence.push(format!(
            "{invalid_disks} unidade(s) com dados ausentes ou inválidos."
        ));
    }
    if data.disks.is_empty() {
        storage_evidence.push("Dados de armazenamento não disponíveis.".to_string());
    }
    let storage_sources = if valid_disks.is_empty() {
        Vec::new()
    } else {
        vec!["Win32_LogicalDisk".to_string()]
    };

    let (ram_status, ram_evidence) = match (data.ram_total_bytes, data.ram_available_bytes) {
        (Some(total), Some(available)) if total > 0 && available <= total => {
            let available_percent = available as f64 * 100.0 / total as f64;
            let status = if available_percent < 10.0 {
                DiagnosticStatus::Atencao
            } else {
                DiagnosticStatus::Informativo
            };
            let mut evidence = vec![format!(
                "{} totais; {} disponíveis ({available_percent:.1}%)",
                format_bytes(total),
                format_bytes(available)
            )];
            if status == DiagnosticStatus::Atencao {
                evidence.push("Alerta: memória disponível abaixo de 10%.".to_string());
            } else {
                evidence.push("A coleta não testa a integridade da RAM.".to_string());
            }
            (status, evidence)
        }
        _ => (
            DiagnosticStatus::Indisponivel,
            vec!["Dados de memória ausentes ou inválidos.".to_string()],
        ),
    };
    let ram_sources = if ram_status == DiagnosticStatus::Indisponivel {
        Vec::new()
    } else {
        vec!["Win32_OperatingSystem".to_string()]
    };

    let mut gpu_evidence: Vec<String> = data
        .gpu_names
        .iter()
        .filter(|name| !name.trim().is_empty())
        .cloned()
        .collect();
    let gpu_sources = if gpu_evidence.is_empty() {
        Vec::new()
    } else {
        vec!["Win32_VideoController".to_string()]
    };
    if gpu_evidence.is_empty() {
        gpu_evidence.push("Identificação da placa de vídeo não disponível.".to_string());
    }
    let gpu_status = if gpu_sources.is_empty() {
        DiagnosticStatus::Indisponivel
    } else {
        DiagnosticStatus::Informativo
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
    if processor_evidence.is_empty() {
        processor_evidence.push("Identificação do processador não disponível.".to_string());
    }
    let processor_status = if processor_sources.is_empty() {
        DiagnosticStatus::Indisponivel
    } else {
        DiagnosticStatus::Informativo
    };

    let mut temperature_evidence = Vec::new();
    let mut temperature_sources = Vec::new();
    let mut has_valid_temperature = append_temperature(
        &mut temperature_evidence,
        &mut temperature_sources,
        "Sensor CPU",
        data.cpu_sensor_temperature_c,
        data.cpu_sensor_source.as_deref(),
    );
    has_valid_temperature |= append_temperature(
        &mut temperature_evidence,
        &mut temperature_sources,
        "Zona térmica (componente não identificado)",
        data.thermal_zone_temperature_c,
        data.thermal_zone_source.as_deref(),
    );
    has_valid_temperature |= append_temperature(
        &mut temperature_evidence,
        &mut temperature_sources,
        "Sensor GPU",
        data.gpu_sensor_temperature_c,
        data.gpu_sensor_source.as_deref(),
    );
    let temperature_status = if has_valid_temperature {
        DiagnosticStatus::Informativo
    } else {
        DiagnosticStatus::Indisponivel
    };
    if temperature_evidence.is_empty() {
        temperature_evidence.push("Leitura de temperatura confiável não disponível.".to_string());
    }

    vec![
        result(
            DiagnosticCategory::Sistema,
            system_status,
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
            ram_status,
            ram_evidence,
            ram_sources,
        ),
        result(
            DiagnosticCategory::PlacaDeVideo,
            gpu_status,
            gpu_evidence,
            gpu_sources,
        ),
        result(
            DiagnosticCategory::Processador,
            processor_status,
            processor_evidence,
            processor_sources,
        ),
        result(
            DiagnosticCategory::Temperatura,
            temperature_status,
            temperature_evidence,
            temperature_sources,
        ),
    ]
}

fn append_temperature(
    evidence: &mut Vec<String>,
    sources: &mut Vec<String>,
    label: &str,
    temperature: Option<f64>,
    source: Option<&str>,
) -> bool {
    let source = source.filter(|source| !source.trim().is_empty());
    if let Some(source) = source.filter(|source| !temperature_source_is_recognized(source)) {
        evidence.push(format!(
            "{label}: leitura ignorada; origem não reconhecida."
        ));
        sources.push(source.to_string());
        return false;
    }
    match (temperature, source) {
        (Some(value), Some(source)) if value.is_finite() && (0.0..=150.0).contains(&value) => {
            evidence.push(format!(
                "{label}: {value:.1} °C (informativo; sem limite seguro validado)"
            ));
            sources.push(source.to_string());
            true
        }
        (Some(value), Some(source)) => {
            let reason = if value.is_finite() {
                "fora da faixa plausível de 0 a 150 °C"
            } else {
                "não finita"
            };
            evidence.push(format!("{label}: leitura ignorada ({reason})."));
            sources.push(source.to_string());
            false
        }
        (Some(_), None) => {
            evidence.push(format!(
                "{label}: leitura ignorada; origem confiável ausente."
            ));
            false
        }
        (None, Some(source)) => {
            evidence.push(format!(
                "{label}: sensor identificado, leitura indisponível."
            ));
            sources.push(source.to_string());
            false
        }
        (None, None) => false,
    }
}

fn temperature_source_is_recognized(source: &str) -> bool {
    matches!(
        source.trim(),
        "MSAcpi_ThermalZoneTemperature"
            | "root/LibreHardwareMonitor Sensor"
            | "root/OpenHardwareMonitor Sensor"
    )
}

fn unavailable_checks(reason: &str) -> Vec<DiagnosticCheck> {
    [
        ("SFC /verifyonly", "sfc"),
        ("DISM /Online /Cleanup-Image /ScanHealth", "dism"),
    ]
    .into_iter()
    .map(|(name, program)| DiagnosticCheck {
        name: name.to_string(),
        status: DiagnosticStatus::Indisponivel,
        evidence: format!("{program}: verificação não disponível: {reason}"),
        source: "Verificação do Windows".to_string(),
    })
    .collect()
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
    if ($_.CurrentTemperature -ne $null) { [double]$c }
  })
} catch {}
$cpuTemperature = $null
$cpuTemperatureSource = $null
$gpuTemperature = $null
$gpuTemperatureSource = $null
foreach ($namespace in @('root/LibreHardwareMonitor', 'root/OpenHardwareMonitor')) {
  try {
    $cpuSensors = @(Get-CimInstance -Namespace $namespace -ClassName Sensor -ErrorAction Stop | Where-Object {
      $_.SensorType -eq 'Temperature' -and $_.HardwareType -like 'Cpu*' -and $_.Value -ne $null
    })
    if ($cpuSensors.Count -gt 0 -and $null -eq $cpuTemperature) {
      $preferredCpuSensor = $cpuSensors | Sort-Object @{ Expression = {
        if ($_.Name -match '(?i)CPU Package|Package|Tctl|Tdie') { 0 }
        elseif ($_.Name -match '(?i)Core') { 1 }
        else { 2 }
      }}
      $cpuTemperature = [double]$preferredCpuSensor[0].Value
      $cpuTemperatureSource = "$namespace Sensor"
    }
  } catch {}
  try {
    $gpuSensors = @(Get-CimInstance -Namespace $namespace -ClassName Sensor -ErrorAction Stop | Where-Object {
      $_.SensorType -eq 'Temperature' -and $_.HardwareType -like 'Gpu*' -and $_.Value -ne $null
    })
    if ($gpuSensors.Count -gt 0 -and $null -eq $gpuTemperature) {
      $gpuTemperature = [double]$gpuSensors[0].Value
      $gpuTemperatureSource = "$namespace Sensor"
    }
  } catch {}
  if ($null -ne $cpuTemperature -and $null -ne $gpuTemperature) { break }
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
  cpu_sensor_temperature_c=$cpuTemperature
  cpu_sensor_source=$cpuTemperatureSource
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
        for (name, program, args) in [
            ("SFC /verifyonly", "sfc", vec!["/verifyonly"]),
            (
                "DISM /Online /Cleanup-Image /ScanHealth",
                "dism",
                vec!["/Online", "/Cleanup-Image", "/ScanHealth"],
            ),
        ] {
            match Command::new(program).args(args).output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let status =
                        classify_extended_status(program, output.status.code(), &stdout, &stderr);
                    let stdout_summary = summarize_command_output(&stdout);
                    let stderr_summary = summarize_command_output(&stderr);
                    let summary = match (stdout_summary, stderr_summary) {
                        (Some(stdout), Some(stderr)) => {
                            format!("saída: {stdout}; erro: {stderr}")
                        }
                        (Some(stdout), None) => format!("saída: {stdout}"),
                        (None, Some(stderr)) => format!("erro: {stderr}"),
                        (None, None) => "Sem saída textual.".to_string(),
                    };
                    let exit = output.status.code().map_or_else(
                        || "sem código (processo interrompido ou cancelado)".to_string(),
                        |code| format!("código {code}"),
                    );
                    results.push(ExtendedCheck {
                        name: name.to_string(),
                        status,
                        evidence: format!("{exit}; {summary}"),
                    });
                }
                Err(error) => results.push(ExtendedCheck {
                    name: name.to_string(),
                    status: DiagnosticStatus::Indisponivel,
                    evidence: format!("Não foi possível executar {program}: {error}"),
                }),
            }
        }
        Ok(results)
    }
}

#[cfg(target_os = "windows")]
fn summarize_command_output(output: &str) -> Option<String> {
    let lines = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    let mut summary = lines
        .iter()
        .rev()
        .take(8)
        .rev()
        .copied()
        .collect::<Vec<_>>()
        .join(" | ");
    if summary.chars().count() > 700 {
        summary = summary.chars().rev().take(700).collect::<String>();
        summary = summary.chars().rev().collect();
    }
    Some(summary)
}

#[cfg(any(target_os = "windows", test))]
fn classify_extended_status(
    program: &str,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> DiagnosticStatus {
    let Some(code) = exit_code else {
        return DiagnosticStatus::Indisponivel;
    };
    if code != 0 {
        return DiagnosticStatus::Falha;
    }

    let output = format!("{stdout}\n{stderr}").to_lowercase();
    let issue_output = output
        .replace("nenhuma corrupção", "")
        .replace("nenhuma corrupcao", "")
        .replace("no component store corruption", "");
    let detected_issue = match program {
        "sfc" => [
            "found corrupt files",
            "found integrity violations",
            "encontrou arquivos corrompidos",
            "encontrou arquivos danificados",
            "encontrou violações de integridade",
            "encontrou violacoes de integridade",
        ]
        .iter()
        .any(|phrase| issue_output.contains(phrase)),
        "dism" => [
            "the component store is repairable",
            "the component store is not repairable",
            "o armazenamento de componentes pode ser reparado",
            "o armazenamento de componentes não pode ser reparado",
            "o armazenamento de componentes nao pode ser reparado",
            "corrupção detectada no armazenamento de componentes",
            "corrupcao detectada no armazenamento de componentes",
            "corrupção foi detectada no armazenamento de componentes",
            "corrupcao foi detectada no armazenamento de componentes",
        ]
        .iter()
        .any(|phrase| issue_output.contains(phrase)),
        _ => false,
    };
    if detected_issue {
        return DiagnosticStatus::Atencao;
    }
    if !stderr.trim().is_empty() {
        return DiagnosticStatus::Informativo;
    }

    let known_clean = match program {
        "sfc" => [
            "did not find any integrity violations",
            "não encontrou nenhuma violação de integridade",
            "nao encontrou nenhuma violacao de integridade",
        ]
        .iter()
        .any(|phrase| output.contains(phrase)),
        "dism" => [
            "no component store corruption detected",
            "nenhuma corrupção detectada no armazenamento de componentes",
            "nenhuma corrupcao detectada no armazenamento de componentes",
            "nenhuma corrupção foi detectada no armazenamento de componentes",
            "nenhuma corrupcao foi detectada no armazenamento de componentes",
        ]
        .iter()
        .any(|phrase| output.contains(phrase)),
        _ => false,
    };
    if known_clean {
        DiagnosticStatus::Ok
    } else {
        DiagnosticStatus::Informativo
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

    struct FakeBackend {
        data: DiagnosticData,
        extended_calls: usize,
        checks: Vec<ExtendedCheck>,
        extended_error: Option<String>,
    }

    impl Default for FakeBackend {
        fn default() -> Self {
            Self {
                data: DiagnosticData::default(),
                extended_calls: 0,
                checks: vec![
                    ExtendedCheck {
                        name: "SFC /verifyonly".to_string(),
                        status: DiagnosticStatus::Ok,
                        evidence: "código 0; verificação concluída".to_string(),
                    },
                    ExtendedCheck {
                        name: "DISM /Online /Cleanup-Image /ScanHealth".to_string(),
                        status: DiagnosticStatus::Ok,
                        evidence: "código 0; verificação concluída".to_string(),
                    },
                ],
                extended_error: None,
            }
        }
    }

    impl DiagnosticBackend for FakeBackend {
        fn collect(&mut self) -> Result<DiagnosticData, String> {
            Ok(self.data.clone())
        }

        fn run_extended_checks(&mut self) -> Result<Vec<ExtendedCheck>, String> {
            self.extended_calls += 1;
            match &self.extended_error {
                Some(error) => Err(error.clone()),
                None => Ok(self.checks.clone()),
            }
        }
    }

    fn result_for(report: &DiagnosticReport, category: DiagnosticCategory) -> &DiagnosticResult {
        report
            .results
            .iter()
            .find(|result| result.category == category)
            .unwrap()
    }

    #[test]
    fn quick_check_does_not_run_sfc_or_dism() {
        let mut backend = FakeBackend::default();
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(backend.extended_calls, 0);
        assert_eq!(report.results.len(), 6);
        assert!(report.checks.is_empty());
    }

    #[test]
    fn system_identification_is_informational_and_missing_data_is_unavailable() {
        let mut backend = FakeBackend::default();
        backend.data.os_name = Some("Windows test".to_string());
        backend.data.os_version = Some("24H2".to_string());
        let report = run(&mut backend, DiagnosticMode::Complete).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::Sistema).status,
            DiagnosticStatus::Informativo
        );
        assert_eq!(report.checks.len(), 2);

        backend.data = DiagnosticData::default();
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::Sistema).status,
            DiagnosticStatus::Indisponivel
        );
    }

    #[test]
    fn storage_classifies_safe_boundary_and_low_space() {
        let mut backend = FakeBackend::default();
        backend.data.disks = vec![DiskData {
            name: Some("C:".to_string()),
            size_bytes: Some(1000),
            free_bytes: Some(100),
        }];
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::Armazenamento).status,
            DiagnosticStatus::Informativo
        );

        backend.data.disks[0].free_bytes = Some(99);
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let storage = result_for(&report, DiagnosticCategory::Armazenamento);
        assert_eq!(storage.status, DiagnosticStatus::Atencao);
        assert!(storage
            .evidence
            .iter()
            .any(|line| line.contains("abaixo de 10%")));
    }

    #[test]
    fn storage_missing_or_invalid_values_are_unavailable() {
        let mut backend = FakeBackend::default();
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::Armazenamento).status,
            DiagnosticStatus::Indisponivel
        );

        backend.data.disks = vec![DiskData {
            name: Some("C:".to_string()),
            size_bytes: Some(0),
            free_bytes: Some(5),
        }];
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let storage = result_for(&report, DiagnosticCategory::Armazenamento);
        assert_eq!(storage.status, DiagnosticStatus::Indisponivel);
        assert!(!storage.evidence.iter().any(|line| line.contains("0 GiB")));
    }

    #[test]
    fn storage_never_claims_disk_health_and_marks_partial_data_unavailable() {
        let mut backend = FakeBackend::default();
        backend.data.disks = vec![
            DiskData {
                name: Some("C:".to_string()),
                size_bytes: Some(1000),
                free_bytes: Some(500),
            },
            DiskData {
                name: Some("D:".to_string()),
                size_bytes: Some(1000),
                free_bytes: None,
            },
        ];

        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let storage = result_for(&report, DiagnosticCategory::Armazenamento);
        assert_eq!(storage.status, DiagnosticStatus::Indisponivel);
        assert!(storage.evidence.iter().any(|line| line.contains("C:")));
        assert!(storage
            .evidence
            .iter()
            .any(|line| line.contains("1 unidade(s) com dados ausentes ou inválidos")));
        assert!(!storage.evidence.iter().any(|line| {
            line.to_lowercase().contains("disco saudável")
                || line.to_lowercase().contains("saúde do disco confirmada")
        }));
    }

    #[test]
    fn storage_rejects_free_space_greater_than_capacity() {
        let mut backend = FakeBackend::default();
        backend.data.disks = vec![DiskData {
            name: Some("C:".to_string()),
            size_bytes: Some(100),
            free_bytes: Some(101),
        }];

        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let storage = result_for(&report, DiagnosticCategory::Armazenamento);
        assert_eq!(storage.status, DiagnosticStatus::Indisponivel);
        assert!(!storage.evidence.iter().any(|line| line.contains("101")));
    }

    #[test]
    fn ram_uses_documented_ten_percent_alert_boundary_and_rejects_invalid_values() {
        let mut backend = FakeBackend::default();
        backend.data.ram_total_bytes = Some(1000);
        backend.data.ram_available_bytes = Some(100);
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::MemoriaRam).status,
            DiagnosticStatus::Informativo
        );

        backend.data.ram_available_bytes = Some(99);
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::MemoriaRam).status,
            DiagnosticStatus::Atencao
        );

        backend.data.ram_available_bytes = None;
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::MemoriaRam).status,
            DiagnosticStatus::Indisponivel
        );

        backend.data.ram_available_bytes = Some(100);
        backend.data.ram_total_bytes = Some(0);
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::MemoriaRam).status,
            DiagnosticStatus::Indisponivel
        );

        backend.data.ram_total_bytes = Some(100);
        backend.data.ram_available_bytes = Some(101);
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::MemoriaRam).status,
            DiagnosticStatus::Indisponivel
        );
    }

    #[test]
    fn complete_check_status_requires_exit_code_and_recognized_result() {
        assert_eq!(
            classify_extended_status(
                "sfc",
                Some(0),
                "Windows Resource Protection did not find any integrity violations.",
                ""
            ),
            DiagnosticStatus::Ok
        );
        assert_eq!(
            classify_extended_status(
                "sfc",
                Some(0),
                "Windows Resource Protection found corrupt files.",
                ""
            ),
            DiagnosticStatus::Atencao
        );
        assert_eq!(
            classify_extended_status("dism", Some(0), "Unexpected successful output.", ""),
            DiagnosticStatus::Informativo
        );
        assert_eq!(
            classify_extended_status("sfc", Some(1), "Check output", ""),
            DiagnosticStatus::Falha
        );
        assert_eq!(
            classify_extended_status("dism", None, "", ""),
            DiagnosticStatus::Indisponivel
        );
        assert_eq!(
            classify_extended_status("sfc", Some(0), "Known clean output", "warning"),
            DiagnosticStatus::Informativo
        );
    }

    #[test]
    fn sfc_and_dism_classify_representative_english_and_portuguese_messages() {
        for (program, message) in [
            (
                "sfc",
                "Windows Resource Protection did not find any integrity violations.",
            ),
            (
                "sfc",
                "A Proteção de Recursos do Windows não encontrou nenhuma violação de integridade.",
            ),
            ("dism", "No component store corruption detected."),
            (
                "dism",
                "Nenhuma corrupção foi detectada no armazenamento de componentes.",
            ),
        ] {
            assert_eq!(
                classify_extended_status(program, Some(0), message, ""),
                DiagnosticStatus::Ok,
                "{program}: {message}"
            );
        }

        for (program, message) in [
            (
                "sfc",
                "Windows Resource Protection found corrupt files and successfully repaired them.",
            ),
            (
                "sfc",
                "A Proteção de Recursos do Windows encontrou arquivos corrompidos, mas não pôde corrigir alguns deles.",
            ),
            ("dism", "The component store is repairable."),
            (
                "dism",
                "O armazenamento de componentes pode ser reparado.",
            ),
            (
                "dism",
                "O armazenamento de componentes não pode ser reparado.",
            ),
        ] {
            assert_eq!(
                classify_extended_status(program, Some(0), message, ""),
                DiagnosticStatus::Atencao,
                "{program}: {message}"
            );
        }
    }

    #[test]
    fn detected_corruption_takes_priority_over_generic_clean_text() {
        assert_eq!(
            classify_extended_status(
                "dism",
                Some(0),
                "No component store corruption detected. The component store is repairable.",
                ""
            ),
            DiagnosticStatus::Atencao
        );
        assert_eq!(
            classify_extended_status(
                "sfc",
                Some(0),
                "Did not find any integrity violations, but found corrupt files.",
                ""
            ),
            DiagnosticStatus::Atencao
        );
    }

    #[test]
    fn gpu_and_processor_identification_are_not_health_approvals() {
        let mut backend = FakeBackend::default();
        backend.data.gpu_names = vec!["Example GPU".to_string()];
        backend.data.processor_name = Some("Example CPU".to_string());
        backend.data.processor_logical_count = Some(8);
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::PlacaDeVideo).status,
            DiagnosticStatus::Informativo
        );
        assert_eq!(
            result_for(&report, DiagnosticCategory::Processador).status,
            DiagnosticStatus::Informativo
        );
        assert_eq!(
            result_for(&report, DiagnosticCategory::Temperatura).status,
            DiagnosticStatus::Indisponivel
        );
    }

    #[test]
    fn temperatures_require_a_source_and_plausible_finite_values() {
        let mut backend = FakeBackend::default();
        backend.data.thermal_zone_temperature_c = Some(47.0);
        backend.data.thermal_zone_source = Some("MSAcpi_ThermalZoneTemperature".to_string());
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let temperature = result_for(&report, DiagnosticCategory::Temperatura);
        assert_eq!(temperature.status, DiagnosticStatus::Informativo);
        assert!(temperature.evidence[0].contains("componente não identificado"));
        assert!(temperature.evidence[0].contains("sem limite seguro validado"));
        assert!(!temperature.evidence[0].to_lowercase().contains("cpu"));

        backend.data.thermal_zone_temperature_c = Some(f64::NAN);
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let temperature = result_for(&report, DiagnosticCategory::Temperatura);
        assert_eq!(temperature.status, DiagnosticStatus::Indisponivel);
        assert!(temperature.evidence[0].contains("não finita"));

        backend.data.thermal_zone_temperature_c = Some(151.0);
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::Temperatura).status,
            DiagnosticStatus::Indisponivel
        );

        backend.data.thermal_zone_temperature_c = Some(40.0);
        backend.data.thermal_zone_source = Some("unknown provider".to_string());
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let temperature = result_for(&report, DiagnosticCategory::Temperatura);
        assert_eq!(temperature.status, DiagnosticStatus::Indisponivel);
        assert!(temperature.evidence[0].contains("origem não reconhecida"));

        backend.data.thermal_zone_source = Some("MSAcpi_ThermalZoneTemperature".to_string());
        for temperature_value in [0.0, 150.0] {
            backend.data.thermal_zone_temperature_c = Some(temperature_value);
            let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
            assert_eq!(
                result_for(&report, DiagnosticCategory::Temperatura).status,
                DiagnosticStatus::Informativo
            );
        }

        backend.data.thermal_zone_temperature_c = Some(42.0);
        backend.data.thermal_zone_source = None;
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let temperature = result_for(&report, DiagnosticCategory::Temperatura);
        assert_eq!(temperature.status, DiagnosticStatus::Indisponivel);
        assert!(temperature.evidence[0].contains("origem confiável ausente"));

        backend.data.thermal_zone_temperature_c = None;
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert_eq!(
            result_for(&report, DiagnosticCategory::Temperatura).status,
            DiagnosticStatus::Indisponivel
        );
    }

    #[test]
    fn gpu_identification_without_temperature_sensor_remains_informational() {
        let mut backend = FakeBackend::default();
        backend.data.gpu_names = vec!["Example GPU".to_string()];
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let gpu = result_for(&report, DiagnosticCategory::PlacaDeVideo);
        assert_eq!(gpu.status, DiagnosticStatus::Informativo);
        assert!(gpu.evidence.iter().any(|line| line == "Example GPU"));
        assert_eq!(
            result_for(&report, DiagnosticCategory::Temperatura).status,
            DiagnosticStatus::Indisponivel
        );
    }

    #[test]
    fn cpu_temperature_sensor_is_reported_separately_from_gpu_and_thermal_zone() {
        let mut backend = FakeBackend::default();
        backend.data.cpu_sensor_temperature_c = Some(62.5);
        backend.data.cpu_sensor_source =
            Some("root/LibreHardwareMonitor Sensor".to_string());
        backend.data.gpu_sensor_temperature_c = Some(71.0);
        backend.data.gpu_sensor_source =
            Some("root/OpenHardwareMonitor Sensor".to_string());
        backend.data.thermal_zone_temperature_c = Some(39.0);
        backend.data.thermal_zone_source =
            Some("MSAcpi_ThermalZoneTemperature".to_string());

        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        let temperature = result_for(&report, DiagnosticCategory::Temperatura);
        assert_eq!(temperature.status, DiagnosticStatus::Informativo);
        assert!(temperature
            .evidence
            .iter()
            .any(|line| line.contains("Sensor CPU: 62.5 °C")));
        assert!(temperature
            .evidence
            .iter()
            .any(|line| line.contains("Sensor GPU: 71.0 °C")));
        assert!(temperature.evidence.iter().any(|line| line.contains("Zona térmica")));
        assert_eq!(temperature.source.len(), 3);
    }

    #[test]
    fn temperature_accepts_exact_sources_emitted_by_the_powershell_collector() {
        let sources = [
            "MSAcpi_ThermalZoneTemperature",
            "root/LibreHardwareMonitor Sensor",
            "root/OpenHardwareMonitor Sensor",
        ];
        for source in sources {
            assert!(temperature_source_is_recognized(source), "{source}");
            let mut evidence = Vec::new();
            let mut reported_sources = Vec::new();
            assert!(append_temperature(
                &mut evidence,
                &mut reported_sources,
                "Sensor",
                Some(45.0),
                Some(source)
            ));
            assert_eq!(reported_sources, [source]);
            assert!(evidence[0].contains("45.0 °C"));
        }
        assert!(!temperature_source_is_recognized(
            "root/LibreHardwareMonitor"
        ));
        assert!(!temperature_source_is_recognized(
            "root/OpenHardwareMonitor"
        ));
    }

    #[test]
    fn complete_checks_separate_pass_failure_and_cancellation() {
        let mut backend = FakeBackend::default();
        let report = run(&mut backend, DiagnosticMode::Quick).unwrap();
        assert!(report.checks.is_empty());

        let report = run(&mut backend, DiagnosticMode::Complete).unwrap();
        assert_eq!(report.checks.len(), 2);
        assert!(report
            .checks
            .iter()
            .all(|check| check.status == DiagnosticStatus::Ok));

        backend.checks[0].status = DiagnosticStatus::Falha;
        backend.checks[0].evidence = "código 2; erro de verificação".to_string();
        let report = run(&mut backend, DiagnosticMode::Complete).unwrap();
        assert_eq!(report.checks[0].status, DiagnosticStatus::Falha);
        assert!(report.checks[0].evidence.contains("código 2"));

        backend.checks[0].status = DiagnosticStatus::Indisponivel;
        backend.checks[0].evidence = "processo cancelado".to_string();
        let report = run(&mut backend, DiagnosticMode::Complete).unwrap();
        assert_eq!(report.checks[0].status, DiagnosticStatus::Indisponivel);
        assert!(report.checks[0].evidence.contains("cancelado"));
    }

    #[test]
    fn failed_to_start_complete_checks_are_reported_unavailable() {
        let mut backend = FakeBackend {
            extended_error: Some("provedor não executável".to_string()),
            ..FakeBackend::default()
        };
        let report = run(&mut backend, DiagnosticMode::Complete).unwrap();
        assert_eq!(report.checks.len(), 2);
        assert!(report
            .checks
            .iter()
            .all(|check| check.status == DiagnosticStatus::Indisponivel));
        assert!(report.checks[0]
            .evidence
            .contains("provedor não executável"));
    }

    #[test]
    fn diagnostic_status_serializes_informational_state_explicitly() {
        assert_eq!(
            serde_json::to_string(&DiagnosticStatus::Informativo).unwrap(),
            "\"informativo\""
        );
        let report = DiagnosticReport {
            mode: DiagnosticMode::Quick,
            results: vec![DiagnosticResult {
                category: DiagnosticCategory::Sistema,
                status: DiagnosticStatus::Informativo,
                evidence: vec!["Windows test".to_string()],
                source: vec!["fake".to_string()],
            }],
            checks: Vec::new(),
        };
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["results"][0]["status"], "informativo");
        assert!(json["checks"].as_array().unwrap().is_empty());
    }

    #[test]
    fn all_categories_always_include_evidence_and_source() {
        let mut backend = FakeBackend::default();
        backend.data.os_name = Some("Windows test".to_string());
        backend.data.disks = vec![DiskData {
            name: Some("C:".to_string()),
            size_bytes: Some(1000),
            free_bytes: Some(500),
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
            result_for(&report, DiagnosticCategory::Armazenamento).status,
            DiagnosticStatus::Informativo
        );
    }
}
