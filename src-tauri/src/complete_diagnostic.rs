use crate::diagnostics::{DiagnosticMode, DiagnosticResult};
use crate::diagnostics_store::{
    self, DiagnosticIssue, DiagnosticSystem, ExecutionStatus, StoredDiagnosticReport,
};
use std::fs;
use tauri::{AppHandle, Manager};

fn report_path(app: &AppHandle, report_id: &str) -> Result<std::path::PathBuf, String> {
    if report_id.is_empty()
        || report_id.len() > 80
        || !report_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("Identificador de laudo inválido.".into());
    }
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    Ok(app_data_dir
        .join("diagnostics")
        .join("reports")
        .join(format!("{report_id}.json")))
}

#[tauri::command]
pub(crate) fn update_complete_diagnostic_report(
    app: AppHandle,
    report_id: String,
    results: Vec<DiagnosticResult>,
    issues: Vec<DiagnosticIssue>,
    system: Option<DiagnosticSystem>,
    execution_status: String,
    duration_ms: Option<u64>,
) -> Result<StoredDiagnosticReport, String> {
    let path = report_path(&app, &report_id)?;
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("Não foi possível consultar o laudo completo: {error}"))?;
    if metadata.len() > 4 * 1024 * 1024 {
        return Err("O laudo completo excede o tamanho máximo permitido.".into());
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("Não foi possível carregar o laudo completo: {error}"))?;
    let mut report: StoredDiagnosticReport = serde_json::from_slice(&bytes)
        .map_err(|error| format!("O laudo completo está inválido: {error}"))?;
    if report.mode != DiagnosticMode::Complete {
        return Err("O laudo informado não é de um diagnóstico completo.".into());
    }

    report.results = results;
    report.issues = issues;
    if let Some(system) = system {
        report.system = system;
    }
    report.execution_status = match execution_status.as_str() {
        "running" => ExecutionStatus::Running,
        "completed" => ExecutionStatus::Completed,
        "partial" => ExecutionStatus::Partial,
        "cancelled" => ExecutionStatus::Cancelled,
        "failed" => ExecutionStatus::Failed,
        _ => return Err("Estado de execução do laudo inválido.".into()),
    };
    if matches!(&report.execution_status, ExecutionStatus::Running) {
        report.finished_at = None;
        report.duration_ms = None;
    } else {
        report.finished_at = Some(crate::storage_diagnostic::timestamp_now());
        report.duration_ms = duration_ms;
    }
    diagnostics_store::save(
        &app.path().app_data_dir().map_err(|error| {
            format!("Não foi possível localizar a pasta de dados do QBoa: {error}")
        })?,
        &report,
    )?;
    Ok(report)
}
