//! Diagnóstico de CPU com carga computacional controlada e telemetria térmica.
//!
//! A temperatura é lida de um provedor de sensores de hardware ativo (LibreHardwareMonitor
//! ou Open Hardware Monitor). O Windows não expõe de forma consistente a temperatura do
//! package por uma API WMI universal; zonas ACPI genéricas não são rotuladas como CPU aqui.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Runtime};

const REPORT_SCHEMA_VERSION: u32 = 1;
const MAX_REPORT_BYTES: u64 = 4 * 1024 * 1024;
const THERMAL_STOP_C: f64 = 85.0;
const SAMPLE_TIMEOUT_SECS: u64 = 8;
static NEXT_REPORT_ID: AtomicU64 = AtomicU64::new(0);
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(0);

const EVENT_PROGRESS: &str = "qboa-cpu-test-progress";
const EVENT_STATE: &str = "qboa-cpu-test-state";
const EVENT_FINISHED: &str = "qboa-cpu-test-finished";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CpuTestProfile {
    Quick,
    Standard,
    Extended,
}

impl CpuTestProfile {
    fn duration_seconds(self) -> u64 {
        match self {
            Self::Quick => 5 * 60,
            Self::Standard => 15 * 60,
            Self::Extended => 30 * 60,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Quick => "Rápido · 5 minutos",
            Self::Standard => "Padrão · 15 minutos",
            Self::Extended => "Estendido · 30 minutos",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CpuInventory {
    pub name: String,
    pub manufacturer: Option<String>,
    pub socket: Option<String>,
    pub physical_cores: u32,
    pub logical_processors: u32,
    pub max_clock_mhz: Option<u32>,
    pub sensor_available: bool,
    pub sensor_provider: Option<String>,
    pub temperature_sensors: Vec<String>,
    pub sensor_note: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CpuTelemetrySample {
    pub elapsed_ms: u64,
    pub timestamp_utc: String,
    pub temperature_c: Option<f64>,
    pub temperature_sensor: Option<String>,
    pub cpu_utilization_percent: Option<f64>,
    pub frequency_mhz: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CpuTestReport {
    pub schema_version: u32,
    pub report_id: String,
    pub processor_name: String,
    pub physical_cores: u32,
    pub logical_processors: u32,
    #[serde(default)]
    pub worker_threads: u32,
    pub sensor_provider: String,
    #[serde(default)]
    pub profile: Option<CpuTestProfile>,
    pub target_duration_seconds: u64,
    pub duration_ms: u64,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// running, completed, cancelled, thermal_limit_reached, sensor_lost,
    /// errors_detected ou partial.
    pub execution_status: String,
    pub samples: Vec<CpuTelemetrySample>,
    pub iterations: u64,
    pub calculation_errors: u64,
    pub temperature_min_c: Option<f64>,
    pub temperature_max_c: Option<f64>,
    pub temperature_average_c: Option<f64>,
    pub cpu_utilization_max_percent: Option<f64>,
    pub cpu_utilization_average_percent: Option<f64>,
    pub frequency_peak_mhz: Option<f64>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CpuTestSessionSnapshot {
    pub session_id: String,
    pub lifecycle: String,
    pub profile: CpuTestProfile,
    pub started_at: String,
    pub elapsed_ms: u64,
    pub target_duration_ms: u64,
    pub progress_percent: u8,
    pub latest_sample: Option<CpuTelemetrySample>,
    pub iterations: u64,
    pub calculation_errors: u64,
    pub report: Option<CpuTestReport>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct RawTelemetrySample {
    timestamp_utc: String,
    temperature_c: Option<f64>,
    temperature_sensor: Option<String>,
    cpu_utilization_percent: Option<f64>,
    frequency_mhz: Option<f64>,
}

struct SessionEntry {
    snapshot: CpuTestSessionSnapshot,
    cancel: Arc<AtomicBool>,
}

static SESSION_REGISTRY: OnceLock<Mutex<Option<SessionEntry>>> = OnceLock::new();

fn registry() -> &'static Mutex<Option<SessionEntry>> {
    SESSION_REGISTRY.get_or_init(|| Mutex::new(None))
}

fn lock_registry() -> std::sync::MutexGuard<'static, Option<SessionEntry>> {
    registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn get_session() -> Option<CpuTestSessionSnapshot> {
    lock_registry().as_ref().map(|entry| entry.snapshot.clone())
}

pub(crate) fn is_active() -> bool {
    lock_registry().as_ref().is_some_and(|entry| {
        entry.snapshot.lifecycle == "running" || entry.snapshot.lifecycle == "cancelling"
    })
}

pub(crate) fn collect_inventory(app_data_dir: &Path) -> Result<CpuInventory, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app_data_dir;
        return Err("O diagnóstico de CPU exige Windows 10/11.".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        let script = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$cpu = Get-CimInstance -ClassName Win32_Processor | Select-Object -First 1
if (-not $cpu) { throw 'O Windows não retornou Win32_Processor.' }
[pscustomobject]@{
  name = [string]$cpu.Name
  manufacturer = [string]$cpu.Manufacturer
  socket = [string]$cpu.SocketDesignation
  physical_cores = [uint32]$cpu.NumberOfCores
  logical_processors = [uint32]$cpu.NumberOfLogicalProcessors
  max_clock_mhz = if ($cpu.MaxClockSpeed) { [uint32]$cpu.MaxClockSpeed } else { $null }
  sensor_available = $false
  sensor_provider = $null
  temperature_sensors = @()
  sensor_note = 'Consultando o coletor térmico integrado...'
} | ConvertTo-Json -Compress -Depth 5
"#;
        let output = run_powershell_once(script)?;
        let mut inventory: CpuInventory = serde_json::from_str(&output)
            .map_err(|error| format!("Resposta de inventário da CPU inválida: {error}"))?;
        let discovery_output = run_hardware_monitor_once(app_data_dir, "discover")?;
        let discovery: serde_json::Value =
            serde_json::from_str(&discovery_output)
            .map_err(|error| {
                format!("O coletor térmico retornou uma resposta inválida: {error}")
            })?;
        let sensors = discovery
            .get("sensors")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("name").and_then(serde_json::Value::as_str))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        inventory.sensor_available = discovery
            .get("available")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            && !sensors.is_empty();
        inventory.temperature_sensors = sensors;
        if inventory.sensor_available {
            inventory.sensor_provider =
                Some("LibreHardwareMonitorLib 0.9.6 (integrado)".to_string());
            inventory.sensor_note = format!(
                "{} sensor(es) térmico(s) de CPU detectado(s) pelo coletor integrado.",
                inventory.temperature_sensors.len()
            );
        } else {
            inventory.sensor_provider = None;
            inventory.sensor_note = discovery.get("note")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Nenhum sensor térmico de CPU compatível foi encontrado. O teste de carga não será iniciado.")
                .to_string();
        }
        Ok(inventory)
    }
}

// O PowerShell 5.1 pode herdar a página de código OEM do Windows ao escrever
// em stdout redirecionado. Fixamos UTF-8 sem BOM antes de qualquer saída JSON.
fn powershell_utf8_preamble() -> &'static str {
    "$utf8NoBom = New-Object System.Text.UTF8Encoding($false); [Console]::OutputEncoding = $utf8NoBom; $OutputEncoding = $utf8NoBom"
}

#[cfg(target_os = "windows")]
fn run_powershell_once(script: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let script = format!("{}\n{}", powershell_utf8_preamble(), script);
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000);
    let output = command.output().map_err(|error| {
        format!("Não foi possível consultar sensores/CPU via PowerShell: {error}")
    })?;
    if !output.status.success() {
        return Err(format!(
            "A consulta de CPU falhou: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_string())
        .map_err(|error| format!("A resposta do PowerShell não está em UTF-8: {error}"))
}

#[cfg(target_os = "windows")]
fn extract_hardware_monitor(app_data_dir: &Path) -> Result<PathBuf, String> {
    const EMBEDDED_MONITOR: &[u8] = include_bytes!("../binaries/qboa-hardware-monitor.exe");
    let runtime_dir = app_data_dir.join("runtime");
    fs::create_dir_all(&runtime_dir)
        .map_err(|e| format!("Não foi possível preparar o coletor térmico: {e}"))?;
    let destination = runtime_dir.join("qboa-hardware-monitor.exe");
    let temporary = runtime_dir.join(format!("qboa-hardware-monitor-{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|e| format!("Não foi possível extrair o coletor térmico: {e}"))?;
    file.write_all(EMBEDDED_MONITOR)
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("Não foi possível gravar o coletor térmico: {e}"))?;
    drop(file);
    if destination.exists() {
        fs::remove_file(&destination)
            .map_err(|e| format!("Não foi possível atualizar o coletor térmico: {e}"))?;
    }
    fs::rename(&temporary, &destination)
        .map_err(|e| format!("Não foi possível preparar o coletor térmico: {e}"))?;
    Ok(destination)
}

#[cfg(target_os = "windows")]
fn run_hardware_monitor_once(app_data_dir: &Path, mode: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let executable = extract_hardware_monitor(app_data_dir)?;
    let output = Command::new(executable)
        .arg(mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("Não foi possível iniciar o coletor térmico integrado: {e}"))?;
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if details.is_empty() {
            format!("O coletor térmico encerrou com código {}.", output.status)
        } else {
            format!("O coletor térmico integrado falhou: {details}")
        });
    }
    String::from_utf8(output.stdout)
        .map(|v| v.trim().to_string())
        .map_err(|e| format!("A resposta do coletor térmico não está em UTF-8: {e}"))
}

#[cfg(not(target_os = "windows"))]
fn run_hardware_monitor_once(_app_data_dir: &Path, _mode: &str) -> Result<String, String> {
    Err("O coletor térmico integrado exige Windows.".to_string())
}

#[cfg(target_os = "windows")]
fn start_telemetry_process(app_data_dir: &Path) -> Result<Child, String> {
    use std::os::windows::process::CommandExt;
    let executable = extract_hardware_monitor(app_data_dir)?;
    Command::new(executable)
        .arg("stream")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| format!("Não foi possível iniciar o monitoramento térmico integrado: {e}"))
}

#[cfg(not(target_os = "windows"))]
fn start_telemetry_process(_app_data_dir: &Path) -> Result<Child, String> {
    Err("A telemetria térmica exige Windows.".to_string())
}

pub(crate) fn start<R: Runtime>(
    app: AppHandle<R>,
    app_data_dir: PathBuf,
    profile: CpuTestProfile,
    inventory: CpuInventory,
) -> Result<CpuTestSessionSnapshot, String> {
    let duration_seconds = profile.duration_seconds();
    if !(1..=1800).contains(&duration_seconds) {
        return Err("Duração de teste inválida.".to_string());
    }

    let session_id = new_session_id();
    let cancel = Arc::new(AtomicBool::new(false));
    let snapshot = CpuTestSessionSnapshot {
        session_id: session_id.clone(),
        lifecycle: "running".into(),
        profile,
        started_at: timestamp_now(),
        elapsed_ms: 0,
        target_duration_ms: duration_seconds * 1000,
        progress_percent: 0,
        latest_sample: None,
        iterations: 0,
        calculation_errors: 0,
        report: None,
        error: None,
    };

    {
        let mut current = lock_registry();
        if current.as_ref().is_some_and(|entry| {
            entry.snapshot.lifecycle == "running" || entry.snapshot.lifecycle == "cancelling"
        }) {
            return Err("Já existe um teste de CPU em andamento.".into());
        }
        *current = Some(SessionEntry {
            snapshot: snapshot.clone(),
            cancel: Arc::clone(&cancel),
        });
    }

    emit_state(&app, &snapshot);
    let worker_app = app.clone();
    let worker_session_id = session_id.clone();
    let spawn_result = thread::Builder::new()
        .name(format!("qboa-cpu-test-{session_id}"))
        .spawn(move || {
            let result = run_test(
                &worker_app,
                &worker_session_id,
                &app_data_dir,
                profile,
                inventory,
                &cancel,
            );
            match result {
                Ok(report) => {
                    let finished = {
                        let mut current = lock_registry();
                        match current.as_mut() {
                            Some(entry) if entry.snapshot.session_id == worker_session_id => {
                                entry.snapshot.lifecycle = "completed".into();
                                entry.snapshot.elapsed_ms = report.duration_ms;
                                entry.snapshot.progress_percent =
                                    if report.execution_status == "completed" {
                                        100
                                    } else {
                                        entry.snapshot.progress_percent
                                    };
                                entry.snapshot.iterations = report.iterations;
                                entry.snapshot.calculation_errors = report.calculation_errors;
                                entry.snapshot.report = Some(report);
                                Some(entry.snapshot.clone())
                            }
                            _ => None,
                        }
                    };
                    if let Some(session) = finished {
                        let _ = worker_app.emit(
                            EVENT_FINISHED,
                            CpuTestSessionEvent {
                                session_id: worker_session_id,
                                session,
                            },
                        );
                    }
                }
                Err(error) => {
                    let failed = {
                        let mut current = lock_registry();
                        match current.as_mut() {
                            Some(entry) if entry.snapshot.session_id == worker_session_id => {
                                entry.snapshot.lifecycle = "failed".into();
                                entry.snapshot.error = Some(error);
                                Some(entry.snapshot.clone())
                            }
                            _ => None,
                        }
                    };
                    if let Some(session) = failed {
                        let _ = worker_app.emit(
                            EVENT_FINISHED,
                            CpuTestSessionEvent {
                                session_id: worker_session_id,
                                session,
                            },
                        );
                    }
                }
            }
        });

    if let Err(error) = spawn_result {
        let message = format!("Não foi possível iniciar a thread do teste de CPU: {error}");
        let failed = {
            let mut current = lock_registry();
            match current.as_mut() {
                Some(entry) if entry.snapshot.session_id == session_id => {
                    entry.snapshot.lifecycle = "failed".into();
                    entry.snapshot.error = Some(message.clone());
                    Some(entry.snapshot.clone())
                }
                _ => None,
            }
        };
        if let Some(session) = failed {
            emit_state(&app, &session);
        }
        return Err(message);
    }

    Ok(snapshot)
}

#[derive(Clone, Serialize)]
struct CpuTestSessionEvent {
    session_id: String,
    session: CpuTestSessionSnapshot,
}

pub(crate) fn cancel<R: Runtime>(
    app: &AppHandle<R>,
    expected_session_id: Option<&str>,
) -> Result<CpuTestSessionSnapshot, String> {
    let snapshot = {
        let mut current = lock_registry();
        let entry = current
            .as_mut()
            .ok_or_else(|| "Não há teste de CPU em andamento.".to_string())?;
        if expected_session_id.is_some_and(|id| id != entry.snapshot.session_id) {
            return Err("O identificador não corresponde ao teste de CPU atual.".into());
        }
        match entry.snapshot.lifecycle.as_str() {
            "running" => {
                entry.cancel.store(true, Ordering::Release);
                entry.snapshot.lifecycle = "cancelling".into();
                entry.snapshot.clone()
            }
            "cancelling" => entry.snapshot.clone(),
            _ => return Err("O teste de CPU já terminou.".into()),
        }
    };
    emit_state(app, &snapshot);
    Ok(snapshot)
}

fn emit_state<R: Runtime>(app: &AppHandle<R>, snapshot: &CpuTestSessionSnapshot) {
    let _ = app.emit(
        EVENT_STATE,
        CpuTestSessionEvent {
            session_id: snapshot.session_id.clone(),
            session: snapshot.clone(),
        },
    );
}

fn run_test<R: Runtime>(
    app: &AppHandle<R>,
    session_id: &str,
    app_data_dir: &Path,
    profile: CpuTestProfile,
    inventory: CpuInventory,
    cancel: &AtomicBool,
) -> Result<CpuTestReport, String> {
    // Temperatura é pré-requisito: o teste não começa sem telemetria confiável.
    let child = start_telemetry_process(app_data_dir)?;
    let mut child = TelemetryProcess(child);
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| "O coletor de telemetria não abriu stdout.".to_string())?;
    let (sender, receiver) = mpsc::channel::<String>();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut last_preflight_sample = None;
    for sample_number in 1..=3 {
        let line = receiver.recv_timeout(Duration::from_secs(SAMPLE_TIMEOUT_SECS))
            .map_err(|_| "O coletor integrado não forneceu três leituras térmicas consecutivas. O teste não foi iniciado.".to_string())?;
        let raw: RawTelemetrySample = serde_json::from_str(&line)
            .map_err(|e| format!("A amostra térmica de pré-verificação é inválida: {e}"))?;
        let temperature = raw
            .temperature_c
            .filter(|v| v.is_finite() && (0.0..=150.0).contains(v))
            .ok_or_else(|| {
                format!(
                    "O sensor térmico não forneceu leitura válida na pré-verificação ({sample_number}/3). O teste não foi iniciado."
                )
            })?;
        raw.temperature_sensor
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| {
                "O coletor não identificou a origem da leitura térmica. O teste não foi iniciado."
                    .to_string()
            })?;
        if temperature >= THERMAL_STOP_C {
            return Err(format!("A CPU já está a {:.1} °C antes do teste. Aguarde o resfriamento abaixo de {:.0} °C.", temperature, THERMAL_STOP_C));
        }
        last_preflight_sample = Some(raw);
    }
    let first_raw = last_preflight_sample
        .ok_or_else(|| "Não foi possível validar o sensor térmico da CPU.".to_string())?;
    let first_temperature = first_raw
        .temperature_c
        .expect("pré-verificação validou temperatura");

    let started_clock = Instant::now();
    let started_at = timestamp_now();
    let duration_seconds = profile.duration_seconds();
    let worker_count = inventory.logical_processors.clamp(1, 64) as usize;
    let iterations = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let workers =
        CpuWorkerGroup::spawn(worker_count, Arc::clone(&iterations), Arc::clone(&errors))?;

    let mut samples = Vec::new();
    let first_sample = CpuTelemetrySample {
        elapsed_ms: 0,
        timestamp_utc: first_raw.timestamp_utc.clone(),
        temperature_c: Some(first_temperature),
        temperature_sensor: first_raw.temperature_sensor.clone(),
        cpu_utilization_percent: first_raw
            .cpu_utilization_percent
            .filter(|value| value.is_finite() && (0.0..=100.0).contains(value)),
        frequency_mhz: first_raw
            .frequency_mhz
            .filter(|value| value.is_finite() && *value > 0.0 && *value < 20000.0),
    };
    samples.push(first_sample.clone());
    publish_progress(
        app,
        session_id,
        0,
        duration_seconds * 1000,
        &first_sample,
        &iterations,
        &errors,
    );

    let report_id = new_report_id();
    let mut terminal_status = "completed".to_string();
    let mut last_checkpoint = Instant::now();
    let mut last_temperature_at = Instant::now();
    while started_clock.elapsed() < Duration::from_secs(duration_seconds) {
        if cancel.load(Ordering::Acquire) {
            terminal_status = "cancelled".into();
            break;
        }
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(line) => {
                let raw: RawTelemetrySample = match serde_json::from_str(&line) {
                    Ok(sample) => sample,
                    Err(_) => {
                        terminal_status = "sensor_lost".into();
                        break;
                    }
                };
                let elapsed_ms = started_clock.elapsed().as_millis().min(u64::MAX as u128) as u64;
                let temperature = raw
                    .temperature_c
                    .filter(|v| v.is_finite() && (0.0..=150.0).contains(v));
                let sensor_name = raw
                    .temperature_sensor
                    .as_deref()
                    .filter(|name| !name.trim().is_empty());
                if temperature.is_none() || sensor_name.is_none() {
                    let sample = CpuTelemetrySample {
                        elapsed_ms,
                        timestamp_utc: raw.timestamp_utc,
                        temperature_c: None,
                        temperature_sensor: raw.temperature_sensor,
                        cpu_utilization_percent: raw
                            .cpu_utilization_percent
                            .filter(|v| v.is_finite() && (0.0..=100.0).contains(v)),
                        frequency_mhz: raw
                            .frequency_mhz
                            .filter(|v| v.is_finite() && *v > 0.0 && *v < 20000.0),
                    };
                    samples.push(sample.clone());
                    publish_progress(
                        app,
                        session_id,
                        elapsed_ms,
                        duration_seconds * 1000,
                        &sample,
                        &iterations,
                        &errors,
                    );
                    terminal_status = "sensor_lost".into();
                    break;
                }
                last_temperature_at = Instant::now();
                let temperature = temperature.expect("temperatura validada");
                if temperature >= THERMAL_STOP_C {
                    terminal_status = "thermal_limit_reached".into();
                }
                let sample = CpuTelemetrySample {
                    elapsed_ms,
                    timestamp_utc: raw.timestamp_utc,
                    temperature_c: Some(temperature),
                    temperature_sensor: raw.temperature_sensor,
                    cpu_utilization_percent: raw
                        .cpu_utilization_percent
                        .filter(|v| v.is_finite() && (0.0..=100.0).contains(v)),
                    frequency_mhz: raw
                        .frequency_mhz
                        .filter(|v| v.is_finite() && *v > 0.0 && *v < 20000.0),
                };
                samples.push(sample.clone());
                publish_progress(
                    app,
                    session_id,
                    elapsed_ms,
                    duration_seconds * 1000,
                    &sample,
                    &iterations,
                    &errors,
                );
                if last_checkpoint.elapsed() >= Duration::from_secs(15) {
                    let checkpoint = build_report(
                        report_id.clone(),
                        &inventory,
                        profile,
                        started_at.clone(),
                        elapsed_ms,
                        samples.clone(),
                        iterations.load(Ordering::Relaxed),
                        errors.load(Ordering::Relaxed),
                        "running",
                    );
                    persist_report(app_data_dir, &checkpoint)?;
                    last_checkpoint = Instant::now();
                }
                if terminal_status != "completed" {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last_temperature_at.elapsed() >= Duration::from_secs(3) {
                    terminal_status = "sensor_lost".into();
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                terminal_status = "sensor_lost".into();
                break;
            }
        }
    }

    drop(workers);
    let _ = child.0.kill();
    let _ = child.0.wait();
    let _ = reader.join();

    let elapsed_ms = started_clock.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let mut report = build_report(
        report_id,
        &inventory,
        profile,
        started_at,
        elapsed_ms,
        samples,
        iterations.load(Ordering::Relaxed),
        errors.load(Ordering::Relaxed),
        &terminal_status,
    );
    let temperature_samples = report
        .samples
        .iter()
        .filter(|sample| sample.temperature_c.is_some())
        .count();
    if temperature_samples < report.samples.len() {
        report.warnings.push(
            "A leitura térmica foi perdida durante o teste; a carga foi encerrada imediatamente e as amostras parciais foram preservadas.".into(),
        );
    }
    if report.calculation_errors > 0 {
        report
            .warnings
            .push("Foram encontradas divergências em cálculos redundantes durante a carga.".into());
        if report.execution_status == "completed" {
            report.execution_status = "errors_detected".into();
        }
    }
    match terminal_status.as_str() {
        "thermal_limit_reached" => report.warnings.push(format!(
            "Teste interrompido ao atingir o limite térmico conservador definido pelo QBoa ({:.0} °C); isso não representa o TjMax oficial do fabricante.",
            THERMAL_STOP_C
        )),
        "sensor_lost" => report
            .warnings
            .push("A telemetria térmica deixou de responder; a carga foi encerrada.".into()),
        "cancelled" => report
            .warnings
            .push("Execução cancelada pelo usuário; os dados parciais foram preservados.".into()),
        _ => {}
    }
    report.warnings.push("O teste aplica carga sintética de inteiros e não certifica estabilidade universal nem substitui testes especializados de longo prazo.".into());
    persist_report(app_data_dir, &report)?;
    Ok(report)
}

fn publish_progress<R: Runtime>(
    app: &AppHandle<R>,
    session_id: &str,
    elapsed_ms: u64,
    target_ms: u64,
    sample: &CpuTelemetrySample,
    iterations: &AtomicU64,
    errors: &AtomicU64,
) {
    let updated = {
        let mut current = lock_registry();
        match current.as_mut() {
            Some(entry) if entry.snapshot.session_id == session_id => {
                entry.snapshot.elapsed_ms = elapsed_ms;
                entry.snapshot.progress_percent = if target_ms == 0 {
                    0
                } else {
                    ((elapsed_ms.saturating_mul(100) / target_ms).min(100)) as u8
                };
                entry.snapshot.latest_sample = Some(sample.clone());
                entry.snapshot.iterations = iterations.load(Ordering::Relaxed);
                entry.snapshot.calculation_errors = errors.load(Ordering::Relaxed);
                Some(entry.snapshot.clone())
            }
            _ => None,
        }
    };
    if let Some(session) = updated {
        let _ = app.emit(
            EVENT_PROGRESS,
            CpuTestSessionEvent {
                session_id: session_id.to_string(),
                session,
            },
        );
    }
}

struct CpuWorkerGroup {
    stop: Arc<AtomicBool>,
    handles: Vec<thread::JoinHandle<()>>,
}

impl CpuWorkerGroup {
    fn spawn(
        worker_count: usize,
        iterations: Arc<AtomicU64>,
        errors: Arc<AtomicU64>,
    ) -> Result<Self, String> {
        let mut group = Self {
            stop: Arc::new(AtomicBool::new(false)),
            handles: Vec::with_capacity(worker_count),
        };
        for worker_index in 0..worker_count {
            let stop = Arc::clone(&group.stop);
            let count = Arc::clone(&iterations);
            let failures = Arc::clone(&errors);
            let worker = thread::Builder::new()
                .name(format!("qboa-cpu-load-{worker_index}"))
                .spawn(move || cpu_load_worker(worker_index as u64, stop, count, failures))
                .map_err(|error| format!("Não foi possível criar trabalhador de carga: {error}"))?;
            group.handles.push(worker);
        }
        Ok(group)
    }
}

impl Drop for CpuWorkerGroup {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for worker in self.handles.drain(..) {
            let _ = worker.join();
        }
    }
}

fn cpu_load_worker(
    seed: u64,
    stop: Arc<AtomicBool>,
    iterations: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
) {
    set_low_thread_priority();
    let mut state = seed ^ 0xA5A5_5A5A_0123_4567;
    while !stop.load(Ordering::Acquire) {
        let mut completed = 0u64;
        for _ in 0..2048 {
            let input = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let expected = mix_reference(input);
            let actual = mix_independent(input);
            if expected != actual {
                errors.fetch_add(1, Ordering::Relaxed);
            }
            state = expected.rotate_left(9) ^ 0xD1B5_4A32_D192_ED03;
            completed += 1;
        }
        iterations.fetch_add(completed, Ordering::Relaxed);
        std::hint::black_box(state);
        thread::yield_now();
    }
}

fn mix_reference(input: u64) -> u64 {
    let mut value = input;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 30;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 27;
    value = value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    value ^ (value >> 31)
}

fn mix_independent(input: u64) -> u64 {
    let first = ((input as u128 * 0xBF58_476D_1CE4_E5B9u128) & u64::MAX as u128) as u64;
    let second = first ^ (first >> 30);
    let third = ((second as u128 * 0x94D0_49BB_1331_11EBu128) & u64::MAX as u128) as u64;
    let fourth = third ^ (third >> 27);
    let fifth = ((fourth as u128 * 0x9E37_79B9_7F4A_7C15u128) & u64::MAX as u128) as u64;
    fifth ^ (fifth >> 31)
}

#[cfg(target_os = "windows")]
fn set_low_thread_priority() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> isize;
        fn SetThreadPriority(thread: isize, priority: i32) -> i32;
    }
    const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(target_os = "windows"))]
fn set_low_thread_priority() {}

fn build_report(
    report_id: String,
    inventory: &CpuInventory,
    profile: CpuTestProfile,
    started_at: String,
    duration_ms: u64,
    samples: Vec<CpuTelemetrySample>,
    iterations: u64,
    calculation_errors: u64,
    status: &str,
) -> CpuTestReport {
    let temperatures = samples
        .iter()
        .filter_map(|sample| sample.temperature_c)
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    let utilization = samples
        .iter()
        .filter_map(|sample| sample.cpu_utilization_percent)
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    let frequencies = samples
        .iter()
        .filter_map(|sample| sample.frequency_mhz)
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    let average = |values: &[f64]| {
        if values.is_empty() {
            None
        } else {
            Some(values.iter().sum::<f64>() / values.len() as f64)
        }
    };
    CpuTestReport {
        schema_version: REPORT_SCHEMA_VERSION,
        report_id,
        processor_name: inventory.name.clone(),
        physical_cores: inventory.physical_cores,
        logical_processors: inventory.logical_processors,
        worker_threads: inventory.logical_processors.clamp(1, 64),
        sensor_provider: inventory
            .sensor_provider
            .clone()
            .unwrap_or_else(|| "indisponível".into()),
        profile: Some(profile),
        target_duration_seconds: profile.duration_seconds(),
        duration_ms,
        started_at,
        finished_at: if status == "running" {
            None
        } else {
            Some(timestamp_now())
        },
        execution_status: status.to_string(),
        samples,
        iterations,
        calculation_errors,
        temperature_min_c: temperatures.iter().copied().reduce(f64::min),
        temperature_max_c: temperatures.iter().copied().reduce(f64::max),
        temperature_average_c: average(&temperatures),
        cpu_utilization_max_percent: utilization.iter().copied().reduce(f64::max),
        cpu_utilization_average_percent: average(&utilization),
        frequency_peak_mhz: frequencies.iter().copied().reduce(f64::max),
        warnings: Vec::new(),
    }
}

struct TelemetryProcess(Child);

impl Drop for TelemetryProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn cpu_reports_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("diagnostics").join("cpu-reports")
}

fn safe_report_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn persist_report(app_data_dir: &Path, report: &CpuTestReport) -> Result<(), String> {
    if !safe_report_id(&report.report_id) {
        return Err("Identificador inválido no laudo de CPU.".into());
    }
    let directory = cpu_reports_dir(app_data_dir);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Não foi possível criar o histórico de CPU: {error}"))?;
    let destination = directory.join(format!("{}.json", report.report_id));
    let temporary = directory.join(format!(
        ".{}.{}.tmp",
        report.report_id,
        NEXT_REPORT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|error| format!("Não foi possível serializar o laudo de CPU: {error}"))?;
    let result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                format!("Não foi possível criar arquivo temporário do laudo de CPU: {error}")
            })?;
        file.write_all(&bytes)
            .map_err(|error| format!("Falha ao gravar laudo de CPU: {error}"))?;
        file.flush()
            .map_err(|error| format!("Falha ao descarregar laudo de CPU: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Falha ao sincronizar laudo de CPU: {error}"))?;
        replace_report_file(&temporary, &destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn replace_report_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }
    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(format!(
            "Não foi possível finalizar o laudo de CPU: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_report_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination)
        .map_err(|error| format!("Não foi possível finalizar o laudo de CPU: {error}"))
}

pub(crate) fn load_reports(app_data_dir: &Path) -> Result<Vec<CpuTestReport>, String> {
    let directory = cpu_reports_dir(app_data_dir);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(&directory)
        .map_err(|error| format!("Não foi possível listar laudos de CPU: {error}"))?;
    let mut reports = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let metadata = match fs::metadata(&path) {
            Ok(metadata) if metadata.len() <= MAX_REPORT_BYTES => metadata,
            _ => continue,
        };
        let _ = metadata;
        let mut report: CpuTestReport = match fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        {
            Some(report) => report,
            None => continue,
        };
        if !safe_report_id(&report.report_id) || report.schema_version != REPORT_SCHEMA_VERSION {
            continue;
        }
        if report.execution_status == "running" && !is_active() {
            report.execution_status = "partial".into();
            report.finished_at = Some(timestamp_now());
            report.warnings.push("O aplicativo foi encerrado antes da conclusão do teste; somente as amostras já registradas foram preservadas.".into());
            let _ = persist_report(app_data_dir, &report);
        }
        reports.push(report);
    }
    reports.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    Ok(reports)
}

fn new_report_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "cpu-{:032x}-{:08x}-{:08x}",
        nanos,
        std::process::id(),
        NEXT_REPORT_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn new_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "cpu-session-{:032x}-{:08x}",
        nanos,
        NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn timestamp_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        day_seconds / 3600,
        (day_seconds % 3600) / 60,
        day_seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_output_is_explicitly_configured_as_utf8_without_bom() {
        let preamble = powershell_utf8_preamble();
        assert!(preamble.contains("[Console]::OutputEncoding"));
        assert!(preamble.contains("$OutputEncoding"));
        assert!(preamble.contains("UTF8Encoding($false)"));
    }

    #[test]
    fn rest_sensor_provider_uses_loopback_api() {
        let script = telemetry_script("hardware_monitor_rest");
        assert!(script.contains("http://127.0.0.1:8085/data.json"));
        assert!(script.contains("RawValue"));
        assert!(script.contains("HardwareId"));
        assert!(script.contains("SensorId"));
        assert!(script.contains("RawValue"));
        assert!(!script.contains("root\\LibreHardwareMonitor"));
    }

    #[test]
    fn missing_temperature_provider_uses_native_metrics_collector() {
        let script = telemetry_script("none");
        assert!(script.contains("Win32_PerfFormattedData_PerfOS_Processor"));
        assert!(script.contains("Win32_PerfFormattedData_Counters_ProcessorInformation"));
        assert!(script.contains("temperature_c = $null"));
        assert!(!script.contains("8085"));
    }

    #[test]
    fn lost_rest_sensor_does_not_stop_native_telemetry() {
        let script = telemetry_script("hardware_monitor_rest");
        assert!(script.contains("$selected = $null"));
        assert!(script.contains("catch {"));
        assert!(script.contains("Win32_PerfFormattedData_PerfOS_Processor"));
        assert!(script.contains("Win32_PerfFormattedData_Counters_ProcessorInformation"));
    }

    #[test]
    fn test_profiles_have_expected_durations() {
        assert_eq!(CpuTestProfile::Quick.duration_seconds(), 300);
        assert_eq!(CpuTestProfile::Standard.duration_seconds(), 900);
        assert_eq!(CpuTestProfile::Extended.duration_seconds(), 1800);
    }

    #[test]
    fn independent_mixing_paths_match() {
        for value in [0, 1, 0x1234_5678_9abc_def0, u64::MAX, 0x8000_0000_0000_0000] {
            assert_eq!(mix_reference(value), mix_independent(value));
        }
    }

    #[test]
    fn report_aggregates_telemetry_and_worker_count() {
        let inventory = CpuInventory {
            name: "CPU de teste".into(),
            manufacturer: Some("Vendor".into()),
            socket: None,
            physical_cores: 4,
            logical_processors: 8,
            max_clock_mhz: Some(4000),
            sensor_available: true,
            sensor_provider: Some("root\\LibreHardwareMonitor".into()),
            temperature_sensors: vec!["CPU Package".into()],
            sensor_note: String::new(),
        };
        let samples = vec![
            CpuTelemetrySample {
                elapsed_ms: 0,
                timestamp_utc: "2026-01-01T00:00:00Z".into(),
                temperature_c: Some(40.0),
                temperature_sensor: Some("CPU Package".into()),
                cpu_utilization_percent: Some(10.0),
                frequency_mhz: Some(1200.0),
            },
            CpuTelemetrySample {
                elapsed_ms: 1000,
                timestamp_utc: "2026-01-01T00:00:01Z".into(),
                temperature_c: Some(80.0),
                temperature_sensor: Some("CPU Package".into()),
                cpu_utilization_percent: Some(90.0),
                frequency_mhz: Some(4200.0),
            },
        ];
        let report = build_report(
            "cpu-report-test".into(),
            &inventory,
            CpuTestProfile::Quick,
            "2026-01-01T00:00:00Z".into(),
            1000,
            samples,
            2048,
            0,
            "completed",
        );
        assert_eq!(report.profile, Some(CpuTestProfile::Quick));
        assert_eq!(report.worker_threads, 8);
        assert_eq!(report.temperature_min_c, Some(40.0));
        assert_eq!(report.temperature_max_c, Some(80.0));
        assert_eq!(report.temperature_average_c, Some(60.0));
        assert_eq!(report.cpu_utilization_average_percent, Some(50.0));
        assert_eq!(report.frequency_peak_mhz, Some(4200.0));

        let mut legacy_json = serde_json::to_value(&report).expect("report serialization");
        let legacy_fields = legacy_json.as_object_mut().expect("report object");
        legacy_fields.remove("profile");
        legacy_fields.remove("worker_threads");
        let legacy_report: CpuTestReport =
            serde_json::from_value(legacy_json).expect("legacy report compatibility");
        assert_eq!(legacy_report.profile, None);
        assert_eq!(legacy_report.worker_threads, 0);
    }

    #[test]
    fn report_directory_is_inside_app_data() {
        assert!(cpu_reports_dir(Path::new("app-data")).ends_with("diagnostics/cpu-reports"));
    }

    #[test]
    fn report_ids_are_safe() {
        let id = new_report_id();
        assert!(safe_report_id(&id));
        assert!(!safe_report_id("../invalid"));
    }
}
