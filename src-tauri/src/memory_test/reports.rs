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
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub(crate) fn should_persist_report(report: &super::runner::MemoryTestReport) -> bool {
    report.memory_tested_bytes > 0
}

pub(crate) fn persist_report(
    app_data_dir: &Path,
    report: &super::runner::MemoryTestReport,
) -> Result<(), String> {
    let mut value = serde_json::to_value(report)
        .map_err(|error| format!("Não foi possível serializar o laudo de memória: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "O laudo de memória precisa ser um objeto JSON.".to_string())?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = NEXT_REPORT_ID.fetch_add(1, Ordering::Relaxed);
    let report_id = format!(
        "memory-{nanos:032x}-{:08x}-{counter:08x}",
        std::process::id()
    );
    if !safe_report_id(&report_id) {
        return Err("Não foi possível gerar o identificador do laudo de memória.".into());
    }
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    object.insert("report_id".into(), serde_json::json!(report_id));
    let report_id = object
        .get("report_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "O identificador do laudo de memória ficou inválido.".to_string())?;
    let directory = reports_dir(app_data_dir);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Não foi possível criar o histórico de memória: {error}"))?;
    let destination = directory.join(format!("{report_id}.json"));
    let temporary = directory.join(format!(".{report_id}.tmp"));
    let bytes = serde_json::to_vec_pretty(&value)
        .map_err(|error| format!("Não foi possível serializar o laudo de memória: {error}"))?;
    let result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                format!("Não foi possível criar o arquivo temporário do laudo: {error}")
            })?;
        file.write_all(&bytes)
            .map_err(|error| format!("Falha ao gravar laudo de memória: {error}"))?;
        file.flush()
            .map_err(|error| format!("Falha ao descarregar laudo de memória: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Falha ao sincronizar laudo de memória: {error}"))?;
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("Não foi possível finalizar o laudo de memória: {error}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn load_reports(app_data_dir: &Path) -> Result<Vec<Value>, String> {
    let directory = reports_dir(app_data_dir);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(&directory)
        .map_err(|error| format!("Não foi possível listar o histórico de memória: {error}"))?;
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
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let report: Value = match serde_json::from_slice(&bytes) {
            Ok(report) => report,
            Err(_) => continue,
        };
        let id = report
            .get("report_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let schema = report.get("schema_version").and_then(Value::as_u64);
        let tested_bytes = report
            .get("memory_tested_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if !safe_report_id(id) || schema != Some(SCHEMA_VERSION as u64) || tested_bytes == 0 {
            continue;
        }
        reports.push(report);
    }
    reports.sort_by(|left, right| {
        right
            .get("finished_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(
                left.get("finished_at")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
    });
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_and_loads_memory_reports() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("qboa-memory-reports-{unique}"));
        let report = super::super::runner::MemoryTestReport {
            schema_version: 1,
            engine_version: "test".to_string(),
            profile: super::super::runner::MemoryTestProfile::Fast,
            status: super::super::runner::MemoryTestStatus::Passed,
            stop_reason: super::super::runner::StopReason::Completed,
            started_at: "2026-11-24T10:00:00Z".to_string(),
            finished_at: "2026-11-24T10:00:01Z".to_string(),
            duration_ms: 1000,
            memory_budget_bytes: 256,
            memory_tested_bytes: 256,
            block_size_preferred_bytes: 128,
            largest_block_allocated_bytes: 128,
            blocks_completed: 2,
            passes_planned: 1,
            passes_completed: 1,
            algorithms_planned: vec!["A01".to_string()],
            algorithms_executed: vec!["A01".to_string()],
            a04_seeds: Vec::new(),
            mismatches_total: 0,
            mismatches: Vec::new(),
            coverage_description: "Cobertura de teste".to_string(),
            warnings: Vec::new(),
        };

        persist_report(&directory, &report).unwrap();
        let stored = load_reports(&directory).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0]["profile"], "fast");
        assert_eq!(stored[0]["status"], "passed");
        assert!(stored[0]["report_id"]
            .as_str()
            .unwrap()
            .starts_with("memory-"));

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn does_not_persist_reports_without_tested_bytes() {
        let mut report = super::super::runner::MemoryTestReport {
            schema_version: 1,
            engine_version: "test".to_string(),
            profile: super::super::runner::MemoryTestProfile::Fast,
            status: super::super::runner::MemoryTestStatus::Inconclusive,
            stop_reason: super::super::runner::StopReason::MemoryPressure,
            started_at: "start".to_string(),
            finished_at: "end".to_string(),
            duration_ms: 0,
            memory_budget_bytes: 1024,
            memory_tested_bytes: 0,
            block_size_preferred_bytes: 128,
            largest_block_allocated_bytes: 0,
            blocks_completed: 0,
            passes_planned: 1,
            passes_completed: 0,
            algorithms_planned: vec!["A01".to_string()],
            algorithms_executed: Vec::new(),
            a04_seeds: Vec::new(),
            mismatches_total: 0,
            mismatches: Vec::new(),
            coverage_description: "Nenhuma memória foi testada".to_string(),
            warnings: Vec::new(),
        };
        assert!(!should_persist_report(&report));
        report.memory_tested_bytes = 1;
        assert!(should_persist_report(&report));
    }

    #[test]
    fn report_ids_reject_path_traversal() {
        assert!(safe_report_id("memory-1234_abcd"));
        assert!(!safe_report_id("../memory-report"));
        assert!(!safe_report_id(""));
    }
}
