//! Persistência local dos laudos do teste de memória RAM.
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u32 = 1;
const MAX_REPORT_BYTES: u64 = 4 * 1024 * 1024;
static NEXT_REPORT_ID: AtomicU64 = AtomicU64::new(0);

fn reports_dir(app_data_dir: &Path) -> std::path::PathBuf {
    app_data_dir.join("diagnostics").join("memory-reports")
}

fn safe_report_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 &&
        id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub(crate) fn persist_report(app_data_dir: &Path, report: &super::runner::MemoryTestReport) -> Result<(), String> {
    let mut value = serde_json::to_value(report)
        .map_err(|error| format!("Não foi possível serializar o laudo de memória: {error}"))?;
    let object = value.as_object_mut()
        .ok_or_else(|| "O laudo de memória precisa ser um objeto JSON.".to_string())?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let counter = NEXT_REPORT_ID.fetch_add(1, Ordering::Relaxed);
    let report_id = format!("memory-{nanos:032x}-{:08x}-{counter:08x}", std::process::id());
    if !safe_report_id(&report_id) {
        return Err("Não foi possível gerar o identificador do laudo de memória.".into());
    }
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    object.insert("report_id".into(), serde_json::json!(report_id));
    let directory = reports_dir(app_data_dir);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Não foi possível criar o histórico de memória: {error}"))?;
    let destination = directory.join(format!("{}.json", object["report_id"].as_str().unwrap()));
    let temporary = directory.join(format!(".{}.tmp", object["report_id"].as_str().unwrap()));
    let bytes = serde_json::to_vec_pretty(&value)
        .map_err(|error| format!("Não foi possível serializar o laudo de memória: {error}"))?;
    let result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)
            .map_err(|error| format!("Não foi possível criar o arquivo temporário do laudo: {error}"))?;
        file.write_all(&bytes).map_err(|error| format!("Falha ao gravar laudo de memória: {error}"))?;
        file.flush().map_err(|error| format!("Falha ao descarregar laudo de memória: {error}"))?;
        file.sync_all().map_err(|error| format!("Falha ao sincronizar laudo de memória: {error}"))?;
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("Não foi possível finalizar o laudo de memória: {error}"))?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temporary); }
    result
}

pub(crate) fn load_reports(app_data_dir: &Path) -> Result<Vec<Value>, String> {
    let directory = reports_dir(app_data_dir);
    if !directory.exists() { return Ok(Vec::new()); }
    let entries = fs::read_dir(&directory)
        .map_err(|error| format!("Não foi possível listar o histórico de memória: {error}"))?;
    let mut reports = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") { continue; }
        let metadata = match fs::metadata(&path) {
            Ok(metadata) if metadata.len() <= MAX_REPORT_BYTES => metadata,
            _ => continue,
        };
        let _ = metadata;
        let bytes = match fs::read(&path) { Ok(bytes) => bytes, Err(_) => continue };
        let report: Value = match serde_json::from_slice(&bytes) { Ok(report) => report, Err(_) => continue };
        let id = report.get("report_id").and_then(Value::as_str).unwrap_or("");
        let schema = report.get("schema_version").and_then(Value::as_u64);
        if !safe_report_id(id) || schema != Some(SCHEMA_VERSION as u64) { continue; }
        reports.push(report);
    }
    reports.sort_by(|left, right| {
        right.get("finished_at").and_then(Value::as_str).unwrap_or("")
            .cmp(left.get("finished_at").and_then(Value::as_str).unwrap_or(""))
    });
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_ids_reject_path_traversal() {
        assert!(safe_report_id("memory-1234_abcd"));
        assert!(!safe_report_id("../memory-report"));
        assert!(!safe_report_id(""));
    }
}
