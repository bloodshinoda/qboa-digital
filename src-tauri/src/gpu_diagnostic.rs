//! Teste de estabilidade da GPU com Direct3D 11 (compute shader).
//! Usa somente o adaptador de hardware selecionado pelo Windows; não há fallback WARP.

use serde::{Deserialize, Serialize};
use std::ffi::{c_char, c_void};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Runtime};

const EVENT_PROGRESS: &str = "qboa-gpu-test-progress";
const EVENT_STATE: &str = "qboa-gpu-test-state";
const EVENT_FINISHED: &str = "qboa-gpu-test-finished";
const THERMAL_STOP_C: f64 = 85.0;
const SAMPLE_TIMEOUT_SECS: u64 = 8;
const REPORT_LIMIT: u64 = 4 * 1024 * 1024;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GpuTestProfile { Quick, Standard, Extended }

impl GpuTestProfile {
    pub(crate) fn duration_seconds(self) -> u64 {
        match self { Self::Quick => 300, Self::Standard => 900, Self::Extended => 1800 }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct GpuAdapter {
    pub name: String,
    pub driver_version: Option<String>,
    pub memory_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct GpuInventory {
    pub adapters: Vec<GpuAdapter>,
    pub sensor_available: bool,
    pub sensor_provider: Option<String>,
    pub temperature_sensors: Vec<String>,
    pub sensor_note: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct GpuTelemetrySample {
    pub elapsed_ms: u64,
    pub timestamp_utc: String,
    pub temperature_c: Option<f64>,
    pub temperature_sensor: Option<String>,
    pub utilization_percent: Option<f64>,
    pub frequency_mhz: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct GpuTestReport {
    pub schema_version: u32,
    pub report_id: String,
    pub adapter_names: Vec<String>,
    pub driver_versions: Vec<String>,
    pub api: String,
    pub profile: GpuTestProfile,
    pub target_duration_seconds: u64,
    pub duration_ms: u64,
    pub started_at: String,
    pub finished_at: String,
    pub execution_status: String,
    pub dispatches: u64,
    pub validation_errors: u64,
    pub samples: Vec<GpuTelemetrySample>,
    pub temperature_min_c: Option<f64>,
    pub temperature_max_c: Option<f64>,
    pub temperature_average_c: Option<f64>,
    pub utilization_max_percent: Option<f64>,
    pub utilization_average_percent: Option<f64>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct GpuTestSessionSnapshot {
    pub session_id: String,
    pub lifecycle: String,
    pub profile: GpuTestProfile,
    pub started_at: String,
    pub elapsed_ms: u64,
    pub target_duration_ms: u64,
    pub progress_percent: u8,
    pub latest_sample: Option<GpuTelemetrySample>,
    pub dispatches: u64,
    pub validation_errors: u64,
    pub report: Option<GpuTestReport>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct RawTelemetry {
    timestamp_utc: String,
    temperature_c: Option<f64>,
    temperature_sensor: Option<String>,
    utilization_percent: Option<f64>,
    frequency_mhz: Option<f64>,
}

struct SessionEntry { snapshot: GpuTestSessionSnapshot, cancel: Arc<AtomicBool> }
static SESSION: OnceLock<Mutex<Option<SessionEntry>>> = OnceLock::new();
fn registry() -> &'static Mutex<Option<SessionEntry>> { SESSION.get_or_init(|| Mutex::new(None)) }
fn lock_registry() -> std::sync::MutexGuard<'static, Option<SessionEntry>> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}
pub(crate) fn get_session() -> Option<GpuTestSessionSnapshot> {
    lock_registry().as_ref().map(|entry| entry.snapshot.clone())
}
pub(crate) fn is_active() -> bool {
    lock_registry().as_ref().is_some_and(|entry| matches!(entry.snapshot.lifecycle.as_str(), "running" | "cancelling"))
}

pub(crate) fn collect_inventory(app_data_dir: &Path) -> Result<GpuInventory, String> {
    #[cfg(not(target_os = "windows"))]
    { let _ = app_data_dir; return Err("O diagnóstico de GPU exige Windows 10/11.".into()); }
    #[cfg(target_os = "windows")]
    {
        let script = r#"
$ErrorActionPreference='Stop'
$items=@(Get-CimInstance Win32_VideoController | ForEach-Object {
  [pscustomobject]@{
    name=[string]$_.Name
    driver_version=if ($_.DriverVersion) {[string]$_.DriverVersion} else {$null}
    memory_bytes=if ($_.AdapterRAM -and [uint64]$_.AdapterRAM -gt 0) {[uint64]$_.AdapterRAM} else {$null}
  }
})
if ($items.Count -eq 0) { throw 'O Windows não identificou adaptadores gráficos.' }
ConvertTo-Json -InputObject $items -Depth 4 -Compress
"#;
        let output = cpu_diagnostic::run_powershell_once(script)?;
        let adapters: Vec<GpuAdapter> = serde_json::from_str(&output)
            .or_else(|_| serde_json::from_str::<GpuAdapter>(&output).map(|item| vec![item]))
            .map_err(|e| format!("Inventário de GPU inválido: {e}"))?;
        let discovery = run_hardware_monitor_once(app_data_dir, "gpu-discover")?;
        let value: serde_json::Value = serde_json::from_str(&discovery)
            .map_err(|e| format!("Resposta de sensores de GPU inválida: {e}"))?;
        let sensors = value.get("sensors").and_then(|v| v.as_array()).map(|items| items.iter()
            .filter_map(|item| item.get("name").and_then(|v| v.as_str()).map(str::to_string))
            .collect::<Vec<_>>()).unwrap_or_default();
        let available = value.get("available").and_then(|v| v.as_bool()).unwrap_or(false) && !sensors.is_empty();
        let note = value.get("note").and_then(|v| v.as_str()).unwrap_or(
            if available { "Sensores térmicos de GPU detectados." } else { "Nenhum sensor térmico compatível de GPU foi encontrado. O teste não será iniciado." }
        ).to_string();
        Ok(GpuInventory {
            adapters,
            sensor_available: available,
            sensor_provider: available.then(|| "LibreHardwareMonitorLib 0.9.6 (integrado)".into()),
            temperature_sensors: sensors,
            sensor_note: note,
        })
    }
}

pub(crate) fn start<R: Runtime>(
    app: AppHandle<R>, app_data_dir: PathBuf, profile: GpuTestProfile, inventory: GpuInventory
) -> Result<GpuTestSessionSnapshot, String> {
    if !inventory.sensor_available { return Err(inventory.sensor_note); }
    let session_id = format!("gpu-session-{:032x}-{:08x}", now_nanos(), NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let cancel = Arc::new(AtomicBool::new(false));
    let snapshot = GpuTestSessionSnapshot {
        session_id: session_id.clone(), lifecycle: "running".into(), profile,
        started_at: timestamp_now(), elapsed_ms: 0, target_duration_ms: profile.duration_seconds()*1000,
        progress_percent: 0, latest_sample: None, dispatches: 0, validation_errors: 0,
        report: None, error: None,
    };
    {
        let mut current = lock_registry();
        if current.as_ref().is_some_and(|e| matches!(e.snapshot.lifecycle.as_str(), "running" | "cancelling")) {
            return Err("Já existe um teste de GPU em andamento.".into());
        }
        *current = Some(SessionEntry { snapshot: snapshot.clone(), cancel: Arc::clone(&cancel) });
    }
    emit_state(&app, &snapshot);
    let worker_app = app.clone();
    let worker_id = session_id.clone();
    let spawn = thread::Builder::new().name(format!("qboa-gpu-test-{session_id}")).spawn(move || {
        let result = run_test(&worker_app, &worker_id, &app_data_dir, profile, &inventory, &cancel);
        let finished = {
            let mut current = lock_registry();
            match current.as_mut() {
                Some(entry) if entry.snapshot.session_id == worker_id => {
                    match result {
                        Ok(report) => {
                            entry.snapshot.lifecycle = if report.execution_status == "completed" { "completed" } else { "finished" }.into();
                            entry.snapshot.elapsed_ms = report.duration_ms;
                            entry.snapshot.progress_percent = if report.execution_status == "completed" { 100 } else { entry.snapshot.progress_percent };
                            entry.snapshot.dispatches = report.dispatches;
                            entry.snapshot.validation_errors = report.validation_errors;
                            entry.snapshot.report = Some(report);
                        }
                        Err(error) => { entry.snapshot.lifecycle = "failed".into(); entry.snapshot.error = Some(error); }
                    }
                    Some(entry.snapshot.clone())
                }
                _ => None,
            }
        };
        if let Some(session) = finished {
            let _ = worker_app.emit(EVENT_FINISHED, GpuTestSessionEvent { session_id: worker_id, session });
        }
    });
    if let Err(error) = spawn {
        let message = format!("Não foi possível iniciar a thread do teste de GPU: {error}");
        let mut current = lock_registry();
        if let Some(entry) = current.as_mut().filter(|e| e.snapshot.session_id == session_id) {
            entry.snapshot.lifecycle = "failed".into();
            entry.snapshot.error = Some(message.clone());
        }
        return Err(message);
    }
    Ok(snapshot)
}

#[derive(Clone, Serialize)]
struct GpuTestSessionEvent { session_id: String, session: GpuTestSessionSnapshot }

pub(crate) fn cancel<R: Runtime>(app: &AppHandle<R>, expected: Option<&str>) -> Result<GpuTestSessionSnapshot, String> {
    let snapshot = {
        let mut current = lock_registry();
        let entry = current.as_mut().ok_or("Não há teste de GPU em andamento.")?;
        if expected.is_some_and(|id| id != entry.snapshot.session_id) { return Err("O identificador não corresponde ao teste de GPU atual.".into()); }
        if entry.snapshot.lifecycle == "running" {
            entry.cancel.store(true, Ordering::Release);
            entry.snapshot.lifecycle = "cancelling".into();
        } else if entry.snapshot.lifecycle != "cancelling" { return Err("O teste de GPU já terminou.".into()); }
        entry.snapshot.clone()
    };
    emit_state(app, &snapshot);
    Ok(snapshot)
}

fn emit_state<R: Runtime>(app: &AppHandle<R>, snapshot: &GpuTestSessionSnapshot) {
    let _ = app.emit(EVENT_STATE, GpuTestSessionEvent { session_id: snapshot.session_id.clone(), session: snapshot.clone() });
}

fn publish_progress<R: Runtime>(
    app: &AppHandle<R>, session_id: &str, elapsed_ms: u64, target_ms: u64,
    sample: &GpuTelemetrySample, dispatches: u64, errors: u64
) {
    let updated = {
        let mut current = lock_registry();
        match current.as_mut() {
            Some(entry) if entry.snapshot.session_id == session_id => {
                entry.snapshot.elapsed_ms = elapsed_ms;
                entry.snapshot.progress_percent = ((elapsed_ms.saturating_mul(100) / target_ms.max(1)).min(100)) as u8;
                entry.snapshot.latest_sample = Some(sample.clone());
                entry.snapshot.dispatches = dispatches;
                entry.snapshot.validation_errors = errors;
                Some(entry.snapshot.clone())
            }
            _ => None
        }
    };
    if let Some(session) = updated {
        let _ = app.emit(EVENT_PROGRESS, GpuTestSessionEvent { session_id: session_id.into(), session });
    }
}

fn run_test<R: Runtime>(
    app: &AppHandle<R>, session_id: &str, app_data_dir: &Path, profile: GpuTestProfile,
    inventory: &GpuInventory, cancel: &AtomicBool
) -> Result<GpuTestReport, String> {
    let child = start_telemetry_process(app_data_dir)?;
    let mut child = TelemetryProcess(child);
    let stdout = child.0.stdout.take().ok_or("O coletor térmico não abriu stdout.")?;
    let (sender, receiver) = mpsc::channel::<(String, Instant)>();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => if sender.send((line, Instant::now())).is_err() { break; },
                Err(_) => break,
            }
        }
    });
    let mut first = None;
    for n in 1..=3 {
        let (line, _) = receiver.recv_timeout(Duration::from_secs(SAMPLE_TIMEOUT_SECS))
            .map_err(|_| "O coletor integrado não forneceu três leituras térmicas consecutivas da GPU. O teste não foi iniciado.".to_string())?;
        let raw: RawTelemetry = serde_json::from_str(&line).map_err(|e| format!("Amostra térmica inválida: {e}"))?;
        let temp = raw.temperature_c.filter(|v| valid_temperature(*v))
            .ok_or_else(|| format!("Sensor térmico da GPU sem leitura válida ({n}/3). O teste não foi iniciado."))?;
        let sensor = raw.temperature_sensor.as_deref().filter(|v| !v.trim().is_empty())
            .ok_or("O coletor não identificou a origem da temperatura da GPU.")?;
        if temp >= THERMAL_STOP_C { return Err(format!("A GPU já está a {temp:.1} °C antes do teste. Aguarde resfriar abaixo de {THERMAL_STOP_C:.0} °C.")); }
        first = Some((raw, temp, sensor.to_string()));
    }
    let (first_raw, first_temp, first_sensor) = first.ok_or("Pré-verificação térmica da GPU incompleta.")?;

    let mut engine = D3d11Engine::new()?;
    let started = Instant::now();
    let started_at = timestamp_now();
    let target = profile.duration_seconds();
    let mut samples = vec![GpuTelemetrySample {
        elapsed_ms: 0, timestamp_utc: first_raw.timestamp_utc, temperature_c: Some(first_temp),
        temperature_sensor: Some(first_sensor),
        utilization_percent: first_raw.utilization_percent.filter(|v| valid_percent(*v)),
        frequency_mhz: first_raw.frequency_mhz.filter(|v| v.is_finite() && *v > 0.0),
    }];
    let mut dispatches = 0u64;
    let mut errors = 0u64;
    let mut status = "completed".to_string();
    let mut engine_error: Option<String> = None;
    let mut last_sensor = Instant::now();
    while started.elapsed() < Duration::from_secs(target) {
        if cancel.load(Ordering::Acquire) { status = "cancelled".into(); break; }
        let active_until = Instant::now() + Duration::from_millis(650);
        while Instant::now() < active_until && started.elapsed() < Duration::from_secs(target) {
            if cancel.load(Ordering::Acquire) { status = "cancelled".into(); break; }
            match engine.dispatch_and_verify() {
                Ok(found) => {
                    errors = errors.saturating_add(found);
                    dispatches = dispatches.saturating_add(1);
                }
                Err(error) => {
                    status = "device_error".into();
                    engine_error = Some(error);
                    break;
                }
            }
        }
        if status != "completed" { break; }
        thread::sleep(Duration::from_millis(350));
        match receiver.recv_timeout(Duration::from_millis(300)) {
            Ok((mut line, mut received_at)) => {
                // Se a GPU demorar a concluir um lote, consome a amostra mais nova
                // em vez de acumular leituras antigas e atrasar a proteção térmica.
                while let Ok((next_line, next_received_at)) = receiver.try_recv() {
                    line = next_line;
                    received_at = next_received_at;
                }
                if received_at.elapsed() >= Duration::from_secs(3) {
                    status = "sensor_lost".into();
                    break;
                }
                let raw: RawTelemetry = match serde_json::from_str(&line) {
                    Ok(value) => value, Err(_) => { status = "sensor_lost".into(); break; }
                };
                let elapsed = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                let temperature = raw.temperature_c.filter(|v| valid_temperature(*v));
                let sensor = raw.temperature_sensor.as_deref().filter(|v| !v.trim().is_empty());
                if temperature.is_none() || sensor.is_none() { status = "sensor_lost".into(); break; }
                let temperature = temperature.unwrap();
                last_sensor = Instant::now();
                let sample = GpuTelemetrySample {
                    elapsed_ms: elapsed, timestamp_utc: raw.timestamp_utc, temperature_c: Some(temperature),
                    temperature_sensor: raw.temperature_sensor,
                    utilization_percent: raw.utilization_percent.filter(|v| valid_percent(*v)),
                    frequency_mhz: raw.frequency_mhz.filter(|v| v.is_finite() && *v > 0.0 && *v < 100000.0),
                };
                samples.push(sample.clone());
                publish_progress(app, session_id, elapsed, target*1000, &sample, dispatches, errors);
                if temperature >= THERMAL_STOP_C { status = "thermal_limit_reached".into(); break; }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last_sensor.elapsed() >= Duration::from_secs(3) { status = "sensor_lost".into(); break; }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => { status = "sensor_lost".into(); break; }
        }
    }
    drop(engine);
    let _ = child.0.kill(); let _ = child.0.wait(); let _ = reader.join();
    let elapsed = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let mut report = build_report(inventory, profile, started_at, elapsed, samples, dispatches, errors, &status);
    if report.validation_errors > 0 {
        report.warnings.push("Divergências foram detectadas nos resultados calculados pela GPU.".into());
        if report.execution_status == "completed" { report.execution_status = "errors_detected".into(); }
    }
    match status.as_str() {
        "thermal_limit_reached" => report.warnings.push(format!("Teste interrompido ao atingir o limite térmico conservador do QBoa ({THERMAL_STOP_C:.0} °C); isso não representa o limite oficial do fabricante.")),
        "sensor_lost" => report.warnings.push("A telemetria térmica da GPU foi perdida; a carga foi encerrada e as amostras parciais foram preservadas.".into()),
        "cancelled" => report.warnings.push("Teste cancelado pelo usuário; os dados parciais foram preservados.".into()),
        "device_error" => report.warnings.push(engine_error.unwrap_or_else(|| "O dispositivo Direct3D apresentou erro.".into())),
        _ => {}
    }
    report.warnings.push("O motor executa carga sintética por compute shader Direct3D 11 no adaptador de hardware padrão selecionado pelo Windows. Em sistemas híbridos, o inventário lista todos os adaptadores, mas o laudo não associa com precisão o dispositivo D3D ao nome WMI. Não testa todos os recursos gráficos, toda a VRAM nem certifica estabilidade universal.".into());
    persist_report(app_data_dir, &report)?;
    Ok(report)
}

fn build_report(
    inventory: &GpuInventory, profile: GpuTestProfile, started_at: String, duration_ms: u64,
    samples: Vec<GpuTelemetrySample>, dispatches: u64, errors: u64, status: &str
) -> GpuTestReport {
    let temps = samples.iter().filter_map(|s| s.temperature_c).collect::<Vec<_>>();
    let loads = samples.iter().filter_map(|s| s.utilization_percent).collect::<Vec<_>>();
    let avg = |v: &[f64]| if v.is_empty() { None } else { Some(v.iter().sum::<f64>() / v.len() as f64) };
    GpuTestReport {
        schema_version: 1,
        report_id: format!("gpu-{:032x}-{:08x}", now_nanos(), NEXT_ID.fetch_add(1, Ordering::Relaxed)),
        adapter_names: inventory.adapters.iter().map(|a| a.name.clone()).collect(),
        driver_versions: inventory.adapters.iter().filter_map(|a| a.driver_version.clone()).collect(),
        api: "Direct3D 11 Compute Shader (hardware)".into(), profile,
        target_duration_seconds: profile.duration_seconds(), duration_ms, started_at,
        finished_at: timestamp_now(), execution_status: status.into(), dispatches,
        validation_errors: errors, samples,
        temperature_min_c: temps.iter().copied().reduce(f64::min),
        temperature_max_c: temps.iter().copied().reduce(f64::max),
        temperature_average_c: avg(&temps),
        utilization_max_percent: loads.iter().copied().reduce(f64::max),
        utilization_average_percent: avg(&loads),
        warnings: Vec::new(),
    }
}

fn valid_temperature(value: f64) -> bool { value.is_finite() && value > 0.0 && value <= 150.0 }
fn valid_percent(value: f64) -> bool { value.is_finite() && (0.0..=100.0).contains(&value) }

fn persist_report(app_data_dir: &Path, report: &GpuTestReport) -> Result<(), String> {
    let directory = app_data_dir.join("diagnostics").join("gpu-reports");
    fs::create_dir_all(&directory).map_err(|e| format!("Não foi possível criar histórico de GPU: {e}"))?;
    let bytes = serde_json::to_vec_pretty(report).map_err(|e| format!("Laudo de GPU inválido: {e}"))?;
    if bytes.len() as u64 > REPORT_LIMIT { return Err("Laudo de GPU excedeu o limite de tamanho.".into()); }
    let path = directory.join(format!("{}.json", report.report_id));
    fs::write(&path, bytes).map_err(|e| format!("Não foi possível salvar laudo de GPU: {e}"))
}

pub(crate) fn load_reports(app_data_dir: &Path) -> Result<Vec<GpuTestReport>, String> {
    let directory = app_data_dir.join("diagnostics").join("gpu-reports");
    if !directory.exists() { return Ok(Vec::new()); }
    let mut reports = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") { continue; }
        if fs::metadata(&path).map(|m| m.len() > REPORT_LIMIT).unwrap_or(true) { continue; }
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(report) = serde_json::from_slice::<GpuTestReport>(&bytes) { reports.push(report); }
        }
    }
    reports.sort_by(|a,b| b.started_at.cmp(&a.started_at));
    Ok(reports)
}

struct TelemetryProcess(Child);
impl Drop for TelemetryProcess { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }

#[cfg(target_os = "windows")]
fn start_telemetry_process(app_data_dir: &Path) -> Result<Child, String> {
    use std::os::windows::process::CommandExt;
    let executable = cpu_diagnostic::extract_hardware_monitor(app_data_dir)?;
    Command::new(executable).arg("gpu-stream").stdout(Stdio::piped()).stderr(Stdio::null())
        .creation_flags(0x08000000).spawn()
        .map_err(|e| format!("Não foi possível iniciar telemetria da GPU: {e}"))
}
#[cfg(not(target_os = "windows"))]
fn start_telemetry_process(_app_data_dir: &Path) -> Result<Child, String> {
    Err("A telemetria de GPU exige Windows.".into())
}

#[cfg(target_os = "windows")]
fn run_hardware_monitor_once(app_data_dir: &Path, mode: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let executable = cpu_diagnostic::extract_hardware_monitor(app_data_dir)?;
    let output = Command::new(executable).arg(mode).stdout(Stdio::piped()).stderr(Stdio::piped())
        .creation_flags(0x08000000).output()
        .map_err(|e| format!("Não foi possível consultar sensores de GPU: {e}"))?;
    if !output.status.success() {
        return Err(format!("Coletor de GPU falhou: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    String::from_utf8(output.stdout).map(|v| v.trim().to_string()).map_err(|e| e.to_string())
}

#[cfg(not(target_os = "windows"))]
fn run_hardware_monitor_once(_app_data_dir: &Path, _mode: &str) -> Result<String, String> {
    Err("O coletor de hardware exige Windows.".into())
}

fn now_nanos() -> u128 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() }
fn timestamp_now() -> String {
    let seconds = now_nanos() / 1_000_000_000;
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
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", day_seconds / 3600, (day_seconds % 3600) / 60, day_seconds % 60)
}

#[cfg(target_os = "windows")]
struct D3d11Engine {
    device: *mut c_void, context: *mut c_void, shader: *mut c_void,
    output: *mut c_void, staging: *mut c_void, uav: *mut c_void,
}
#[cfg(target_os = "windows")]
unsafe impl Send for D3d11Engine {}

#[cfg(target_os = "windows")]
impl D3d11Engine {
    fn new() -> Result<Self, String> {
        let mut device = std::ptr::null_mut();
        let mut context = std::ptr::null_mut();
        let mut level = 0u32;
        let levels = [0xb000u32]; // D3D_FEATURE_LEVEL_11_0: compute shader obrigatório.
        let hr = unsafe { D3D11CreateDevice(std::ptr::null_mut(), 1, std::ptr::null_mut(), 0,
            levels.as_ptr(), levels.len() as u32, 7, &mut device, &mut level, &mut context) };
        if hr < 0 || device.is_null() || context.is_null() {
            return Err(format!("Direct3D 11 não conseguiu criar um dispositivo de hardware com Feature Level 11_0 (HRESULT 0x{:08X}). Nenhum fallback por software foi usado.", hr as u32));
        }
        let mut engine = Self { device, context, shader: std::ptr::null_mut(), output: std::ptr::null_mut(), staging: std::ptr::null_mut(), uav: std::ptr::null_mut() };
        if let Err(error) = engine.initialize_resources() { return Err(error); }
        Ok(engine)
    }

    fn initialize_resources(&mut self) -> Result<(), String> {
        const THREADS: u32 = 1024;
        const BYTES: u32 = THREADS * 16;
        let source = b"
RWStructuredBuffer<uint4> Results : register(u0);
[numthreads(64,1,1)]
void main(uint3 tid : SV_DispatchThreadID) {
 uint id=tid.x;
 uint a=id ^ 0xA511E9B3;
 uint b=id + 0x63D83595;
 uint c=id ^ 0x9E3779B9;
 uint d=id + 0x7F4A7C15;
 [loop] for(uint i=0;i<2048;i++) {
  a=(a^(a>>16))*0x7feb352d; b=(b^(b>>15))*0x846ca68b;
  c=(c^(c>>16))*0x7feb352d; d=(d^(d>>15))*0x846ca68b;
  a+=b^i; b+=c^a; c+=d^b; d+=a^c;
 }
 Results[id]=uint4(a,b,c,d);
}";
        let mut bytecode = std::ptr::null_mut();
        let mut errors = std::ptr::null_mut();
        let hr = unsafe { D3DCompile(source.as_ptr() as *const c_void, source.len(), std::ptr::null(), std::ptr::null(), std::ptr::null_mut(),
            b"main\0".as_ptr() as *const c_char, b"cs_5_0\0".as_ptr() as *const c_char, 1 << 11, 0, &mut bytecode, &mut errors) };
        if hr < 0 {
            let detail = unsafe { blob_text(errors) };
            unsafe { release(errors); }
            return Err(format!("Não foi possível compilar o compute shader Direct3D 11: {detail} (HRESULT 0x{:08X}).", hr as u32));
        }
        unsafe { release(errors); }
        let code_ptr = unsafe { blob_pointer(bytecode) };
        let code_len = unsafe { blob_size(bytecode) };
        let create_shader: unsafe extern "system" fn(*mut c_void,*const c_void,usize,*mut c_void,*mut *mut c_void)->i32 = unsafe { method(self.device,18) };
        let hr = unsafe { create_shader(self.device, code_ptr, code_len, std::ptr::null_mut(), &mut self.shader) };
        unsafe { release(bytecode); }
        check_hr(hr, "criar compute shader")?;

        let output_desc = BufferDesc { byte_width: BYTES, usage: 0, bind_flags: 0x80, cpu_access_flags: 0, misc_flags: 0x40, structure_byte_stride: 16 };
        let staging_desc = BufferDesc { byte_width: BYTES, usage: 3, bind_flags: 0, cpu_access_flags: 0x20000, misc_flags: 0, structure_byte_stride: 0 };
        let create_buffer: unsafe extern "system" fn(*mut c_void,*const BufferDesc,*const c_void,*mut *mut c_void)->i32 = unsafe { method(self.device,3) };
        check_hr(unsafe { create_buffer(self.device,&output_desc,std::ptr::null(),&mut self.output) }, "criar buffer de saída")?;
        check_hr(unsafe { create_buffer(self.device,&staging_desc,std::ptr::null(),&mut self.staging) }, "criar buffer de leitura")?;
        let uav_desc = UavDesc { format: 0, dimension: 1, buffer: [0, THREADS, 0, 0] };
        let create_uav: unsafe extern "system" fn(*mut c_void,*mut c_void,*const UavDesc,*mut *mut c_void)->i32 = unsafe { method(self.device,8) };
        check_hr(unsafe { create_uav(self.device,self.output,&uav_desc,&mut self.uav) }, "criar view de acesso à saída")?;
        let set_shader: unsafe extern "system" fn(*mut c_void,*mut c_void,*const *mut c_void,u32) = unsafe { method(self.context,69) };
        let set_uav: unsafe extern "system" fn(*mut c_void,u32,u32,*const *mut c_void,*const u32) = unsafe { method(self.context,68) };
        unsafe { set_shader(self.context,self.shader,std::ptr::null(),0); set_uav(self.context,0,1,&self.uav,std::ptr::null()); }
        Ok(())
    }

    fn dispatch_and_verify(&mut self) -> Result<u64, String> {
        let dispatch: unsafe extern "system" fn(*mut c_void,u32,u32,u32) = unsafe { method(self.context,41) };
        let copy: unsafe extern "system" fn(*mut c_void,*mut c_void,*mut c_void) = unsafe { method(self.context,47) };
        let map: unsafe extern "system" fn(*mut c_void,*mut c_void,u32,u32,u32,*mut Mapped) -> i32 = unsafe { method(self.context,14) };
        let unmap: unsafe extern "system" fn(*mut c_void,*mut c_void,u32) = unsafe { method(self.context,15) };
        unsafe { dispatch(self.context,16,1,1); copy(self.context,self.staging,self.output); }
        let mut mapped = Mapped { data: std::ptr::null_mut(), row_pitch: 0, depth_pitch: 0 };
        check_hr(unsafe { map(self.context,self.staging,0,1,0,&mut mapped) }, "ler resultado da GPU")?;
        if mapped.data.is_null() { unsafe { unmap(self.context,self.staging,0); } return Err("A GPU retornou um buffer de leitura vazio.".into()); }
        let values = unsafe { std::slice::from_raw_parts(mapped.data as *const u32, 1024*4) };
        let mut errors = 0u64;
        for id in 0..1024u32 {
            let expected = reference(id);
            let offset = id as usize * 4;
            if values[offset..offset+4] != expected { errors += 1; }
        }
        unsafe { unmap(self.context,self.staging,0); }
        let removed: unsafe extern "system" fn(*mut c_void)->i32 = unsafe { method(self.device,39) };
        let reason = unsafe { removed(self.device) };
        if reason < 0 { return Err(format!("O dispositivo Direct3D foi removido ou reiniciado (HRESULT 0x{:08X}).", reason as u32)); }
        Ok(errors)
    }
}

#[cfg(target_os = "windows")]
impl Drop for D3d11Engine {
    fn drop(&mut self) { unsafe { release(self.uav); release(self.staging); release(self.output); release(self.shader); release(self.context); release(self.device); } }
}

#[cfg(target_os = "windows")]
#[repr(C)] struct BufferDesc { byte_width:u32, usage:u32, bind_flags:u32, cpu_access_flags:u32, misc_flags:u32, structure_byte_stride:u32 }
#[cfg(target_os = "windows")]
#[repr(C)] struct UavDesc { format:i32, dimension:u32, buffer:[u32;4] }
#[cfg(target_os = "windows")]
#[repr(C)] struct Mapped { data:*mut c_void, row_pitch:u32, depth_pitch:u32 }

#[cfg(target_os = "windows")]
fn reference(id:u32)->[u32;4] {
    let (mut a,mut b,mut c,mut d)=(id^0xA511E9B3,id.wrapping_add(0x63D83595),id^0x9E3779B9,id.wrapping_add(0x7F4A7C15));
    for i in 0..2048u32 {
        a=(a^(a>>16)).wrapping_mul(0x7feb352d); b=(b^(b>>15)).wrapping_mul(0x846ca68b);
        c=(c^(c>>16)).wrapping_mul(0x7feb352d); d=(d^(d>>15)).wrapping_mul(0x846ca68b);
        a=a.wrapping_add(b^i); b=b.wrapping_add(c^a); c=c.wrapping_add(d^b); d=d.wrapping_add(a^c);
    }
    [a,b,c,d]
}

#[cfg(target_os = "windows")]
unsafe fn method<T: Copy>(object:*mut c_void,index:usize)->T {
    let table=*(object as *mut *mut *mut c_void);
    std::mem::transmute_copy(&*table.add(index))
}
#[cfg(target_os = "windows")]
unsafe fn release(object:*mut c_void) {
    if !object.is_null() {
        let release: unsafe extern "system" fn(*mut c_void)->u32=method(object,2);
        let _=release(object);
    }
}
#[cfg(target_os = "windows")]
unsafe fn blob_pointer(blob:*mut c_void)->*const c_void {
    let get: unsafe extern "system" fn(*mut c_void)->*mut c_void=method(blob,3); get(blob)
}
#[cfg(target_os = "windows")]
unsafe fn blob_size(blob:*mut c_void)->usize {
    let get: unsafe extern "system" fn(*mut c_void)->usize=method(blob,4); get(blob)
}
#[cfg(target_os = "windows")]
unsafe fn blob_text(blob:*mut c_void)->String {
    if blob.is_null() { return String::new(); }
    let ptr=blob_pointer(blob); let len=blob_size(blob);
    if ptr.is_null() || len==0 { return String::new(); }
    String::from_utf8_lossy(std::slice::from_raw_parts(ptr as *const u8,len)).trim().to_string()
}
#[cfg(target_os = "windows")]
fn check_hr(hr:i32, action:&str)->Result<(),String> {
    if hr<0 { Err(format!("Falha ao {action} (HRESULT 0x{:08X}).",hr as u32)) } else { Ok(()) }
}

#[cfg(target_os = "windows")]
#[link(name="d3d11")]
unsafe extern "system" {
    fn D3D11CreateDevice(adapter:*mut c_void, driver_type:u32, software:*mut c_void, flags:u32,
        feature_levels:*const u32, feature_levels_count:u32, sdk_version:u32,
        device:*mut *mut c_void, feature_level:*mut u32, immediate_context:*mut *mut c_void)->i32;
}
#[cfg(target_os = "windows")]
#[link(name="d3dcompiler")]
unsafe extern "system" {
    fn D3DCompile(source:*const c_void, source_len:usize, source_name:*const c_char, defines:*const c_void,
        include:*mut c_void, entry:*const c_char, target:*const c_char, flags1:u32, flags2:u32,
        code:*mut *mut c_void, errors:*mut *mut c_void)->i32;
}

#[cfg(not(target_os = "windows"))]
struct D3d11Engine;
#[cfg(not(target_os = "windows"))]
impl D3d11Engine {
    fn new()->Result<Self,String>{Err("O motor Direct3D 11 exige Windows 10/11.".into())}
    fn dispatch_and_verify(&mut self)->Result<u64,String>{Err("Direct3D 11 indisponível.".into())}
}
#[cfg(not(target_os = "windows"))]
impl Drop for D3d11Engine { fn drop(&mut self){} }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn gpu_profiles_have_expected_durations() {
        assert_eq!(GpuTestProfile::Quick.duration_seconds(),300);
        assert_eq!(GpuTestProfile::Standard.duration_seconds(),900);
        assert_eq!(GpuTestProfile::Extended.duration_seconds(),1800);
    }
    #[test] fn telemetry_ranges_are_validated() {
        assert!(!valid_temperature(0.0)); assert!(!valid_temperature(f64::NAN));
        assert!(valid_temperature(45.0)); assert!(!valid_percent(101.0));
        assert!(valid_percent(99.0));
    }
}
