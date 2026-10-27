use crate::diagnostics::{DiagnosticCheck, DiagnosticMode, DiagnosticReport, DiagnosticResult};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u32 = 1;
const MAX_REPORT_BYTES: u64 = 4 * 1024 * 1024;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionStatus {
    Running,
    Completed,
    Partial,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DiagnosticIssue {
    pub severity: String,
    pub component: String,
    pub code: String,
    pub message: String,
    pub occurred_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DiagnosticSystem {
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub os_build: Option<String>,
    pub os_edition: Option<String>,
    pub architecture: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StoredDiagnosticReport {
    pub schema_version: u32,
    pub report_id: String,
    pub app_version: String,
    pub mode: DiagnosticMode,
    pub execution_status: ExecutionStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<u64>,
    pub system: DiagnosticSystem,
    pub results: Vec<DiagnosticResult>,
    pub checks: Vec<DiagnosticCheck>,
    pub issues: Vec<DiagnosticIssue>,
}

impl StoredDiagnosticReport {
    pub(crate) fn new(mode: DiagnosticMode) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            report_id: new_report_id(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            mode,
            execution_status: ExecutionStatus::Running,
            started_at: timestamp_now(),
            finished_at: None,
            duration_ms: None,
            system: DiagnosticSystem {
                os_name: None,
                os_version: None,
                os_build: None,
                os_edition: None,
                architecture: Some(std::env::consts::ARCH.to_string()),
            },
            results: Vec::new(),
            checks: Vec::new(),
            issues: Vec::new(),
        }
    }

    pub(crate) fn apply_result(&mut self, report: DiagnosticReport, elapsed_ms: u64) {
        self.results = report.results;
        self.checks = report.checks;
        self.execution_status = ExecutionStatus::Completed;
        self.finished_at = Some(timestamp_now());
        self.duration_ms = Some(elapsed_ms);
        self.system.os_name = self.results.iter()
            .find(|item| matches!(item.category, crate::diagnostics::DiagnosticCategory::Sistema))
            .and_then(|item| item.evidence.iter().find_map(|line| line.strip_prefix("Sistema: ").map(str::to_string)));
        self.system.os_version = self.results.iter()
            .find(|item| matches!(item.category, crate::diagnostics::DiagnosticCategory::Sistema))
            .and_then(|item| item.evidence.iter().find_map(|line| line.strip_prefix("Versão ").map(str::to_string)));
        self.system.os_build = self.results.iter()
            .find(|item| matches!(item.category, crate::diagnostics::DiagnosticCategory::Sistema))
            .and_then(|item| item.evidence.iter().find_map(|line| line.strip_prefix("Build ").map(str::to_string)));
        self.system.os_edition = self.results.iter()
            .find(|item| matches!(item.category, crate::diagnostics::DiagnosticCategory::Sistema))
            .and_then(|item| item.evidence.iter().find_map(|line| line.strip_prefix("Edição ").map(str::to_string)));
    }

    pub(crate) fn fail(&mut self, message: String, elapsed_ms: u64) {
        self.execution_status = ExecutionStatus::Failed;
        self.finished_at = Some(timestamp_now());
        self.duration_ms = Some(elapsed_ms);
        self.issues.push(DiagnosticIssue {
            severity: "error".to_string(),
            component: "diagnostic_runner".to_string(),
            code: "diagnostic_execution_failed".to_string(),
            message: message.chars().take(1000).collect(),
            occurred_at: Some(timestamp_now()),
        });
    }
}

fn new_report_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().as_nanos();
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{:016x}{:08x}{:08x}", nanos as u64, std::process::id(), counter as u32)
}

fn timestamp_now() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().as_secs();
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
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        day_seconds / 3600, (day_seconds % 3600) / 60, day_seconds % 60)
}

fn reports_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("diagnostics").join("reports")
}

fn safe_report_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.bytes().all(|b|
        b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub(crate) fn save(app_data_dir: &Path, report: &StoredDiagnosticReport) -> Result<(), String> {
    if !safe_report_id(&report.report_id) {
        return Err("Identificador de laudo inválido.".to_string());
    }
    let dir = reports_dir(app_data_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("Não foi possível criar a pasta dos laudos: {e}"))?;
    let destination = dir.join(format!("{}.json", report.report_id));
    let temporary = dir.join(format!(".{}.{}.tmp", report.report_id, NEXT_ID.fetch_add(1, Ordering::Relaxed)));
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|e| format!("Não foi possível serializar o laudo: {e}"))?;
    let write_result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)
            .map_err(|e| format!("Não foi possível criar arquivo temporário do laudo: {e}"))?;
        file.write_all(&bytes).map_err(|e| format!("Falha ao gravar laudo: {e}"))?;
        file.flush().map_err(|e| format!("Falha ao descarregar laudo: {e}"))?;
        file.sync_all().map_err(|e| format!("Falha ao sincronizar laudo: {e}"))?;
        replace_file(&temporary, &destination)
    })();
    if write_result.is_err() { let _ = fs::remove_file(&temporary); }
    write_result
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|e| format!("Não foi possível finalizar a gravação do laudo: {e}"))
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }
    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe { MoveFileExW(source_wide.as_ptr(), destination_wide.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
    if result == 0 { Err(format!("Não foi possível finalizar a gravação do laudo: {}", std::io::Error::last_os_error())) } else { Ok(()) }
}

pub(crate) fn load_all(app_data_dir: &Path) -> Result<Vec<StoredDiagnosticReport>, String> {
    let dir = reports_dir(app_data_dir);
    if !dir.exists() { return Ok(Vec::new()); }
    let entries = fs::read_dir(&dir).map_err(|e| format!("Não foi possível listar os laudos: {e}"))?;
    let mut reports = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") { continue; }
        let metadata = match fs::metadata(&path) { Ok(m) => m, Err(_) => continue };
        if metadata.len() > MAX_REPORT_BYTES { continue; }
        let content = match fs::read(&path) { Ok(c) => c, Err(_) => continue };
        let mut report: StoredDiagnosticReport = match serde_json::from_slice(&content) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !safe_report_id(&report.report_id) || report.schema_version != SCHEMA_VERSION { continue; }
        if matches!(&report.execution_status, ExecutionStatus::Running) {
            report.execution_status = ExecutionStatus::Partial;
            report.finished_at = Some(timestamp_now());
            report.issues.push(DiagnosticIssue {
                severity: "warning".to_string(),
                component: "diagnostic_runner".to_string(),
                code: "execution_interrupted".to_string(),
                message: "A execução foi interrompida antes de registrar sua conclusão.".to_string(),
                occurred_at: Some(timestamp_now()),
            });
            let _ = save(app_data_dir, &report);
        }
        reports.push(report);
    }
    reports.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_id_is_safe_and_unique_for_sequential_calls() {
        let first = new_report_id();
        let second = new_report_id();
        assert_ne!(first, second);
        assert!(safe_report_id(&first));
        assert!(!safe_report_id("../report"));
    }

    #[test]
    fn timestamp_is_rfc3339_utc() {
        let value = timestamp_now();
        assert_eq!(value.len(), 20);
        assert!(value.ends_with('Z'));
        assert_eq!(&value[4..5], "-");
        assert_eq!(&value[10..11], "T");
    }

    #[test]
    fn report_round_trips_as_json() {
        let report = StoredDiagnosticReport::new(DiagnosticMode::Quick);
        let json = serde_json::to_string(&report).unwrap();
        let restored: StoredDiagnosticReport = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.report_id, report.report_id);
        assert_eq!(restored.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn app_data_path_is_not_a_fixed_user_path() {
        assert!(reports_dir(Path::new("app-data")).ends_with("diagnostics/reports"));
    }
}
