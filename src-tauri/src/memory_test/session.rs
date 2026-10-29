//! Gerenciamento de sessão do teste de integridade da RAM.
//!
//! Mantém uma única sessão ativa, publica eventos Tauri e permite consultar ou
//! cancelar a execução sem bloquear a thread de interface.

use super::runner::{
    self, MemoryTestConfig, MemoryTestProgress, MemoryTestReport, MemoryTestStatus,
};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Runtime};

const EVENT_PROGRESS: &str = "qboa-memory-test-progress";
const EVENT_STATE: &str = "qboa-memory-test-state";
const EVENT_FINISHED: &str = "qboa-memory-test-finished";

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);
static SESSION_REGISTRY: OnceLock<Mutex<Option<SessionEntry>>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
pub struct MemoryTestSessionSnapshot {
    pub session_id: String,
    /// running, cancelling, completed ou failed.
    pub lifecycle: String,
    pub profile: runner::MemoryTestProfile,
    pub started_at: String,
    pub progress: Option<MemoryTestProgress>,
    pub report: Option<MemoryTestReport>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
struct MemoryTestSessionEvent {
    session_id: String,
    session: MemoryTestSessionSnapshot,
}

struct SessionEntry {
    snapshot: MemoryTestSessionSnapshot,
    cancel: Arc<AtomicBool>,
}

fn registry() -> &'static Mutex<Option<SessionEntry>> {
    SESSION_REGISTRY.get_or_init(|| Mutex::new(None))
}

fn lock_registry() -> std::sync::MutexGuard<'static, Option<SessionEntry>> {
    registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn get_session() -> Option<MemoryTestSessionSnapshot> {
    lock_registry().as_ref().map(|entry| entry.snapshot.clone())
}

pub fn is_active() -> bool {
    lock_registry().as_ref().is_some_and(|entry| {
        entry.snapshot.lifecycle == "running" || entry.snapshot.lifecycle == "cancelling"
    })
}

pub fn start<R: Runtime>(
    app: AppHandle<R>,
    config: MemoryTestConfig,
) -> Result<MemoryTestSessionSnapshot, String> {
    config.validate()?;

    let session_id = new_session_id();
    let cancel = Arc::new(AtomicBool::new(false));
    let snapshot = MemoryTestSessionSnapshot {
        session_id: session_id.clone(),
        lifecycle: "running".to_string(),
        profile: config.profile,
        started_at: timestamp_now(),
        progress: None,
        report: None,
        error: None,
    };

    {
        let mut current = lock_registry();
        if let Some(entry) = current.as_ref() {
            if entry.snapshot.lifecycle == "running" || entry.snapshot.lifecycle == "cancelling" {
                return Err(format!(
                    "Já existe um teste de memória em andamento (sessão {}). Aguarde ou cancele essa sessão antes de iniciar outra.",
                    entry.snapshot.session_id
                ));
            }
        }
        *current = Some(SessionEntry {
            snapshot: snapshot.clone(),
            cancel: Arc::clone(&cancel),
        });
    }

    emit_state(&app, &snapshot);

    let worker_app = app.clone();
    let worker_session_id = session_id.clone();
    let spawn_result = std::thread::Builder::new()
        .name(format!("qboa-memory-test-{session_id}"))
        .spawn(move || {
            let report = runner::run(config, &cancel, |progress| {
                let updated = {
                    let mut current = lock_registry();
                    match current.as_mut() {
                        Some(entry) if entry.snapshot.session_id == worker_session_id => {
                            entry.snapshot.progress = Some(progress.clone());
                            Some(entry.snapshot.clone())
                        }
                        _ => None,
                    }
                };
                if let Some(session) = updated {
                    let _ = worker_app.emit(
                        EVENT_PROGRESS,
                        MemoryTestSessionEvent {
                            session_id: worker_session_id.clone(),
                            session,
                        },
                    );
                }
            });

            let finished = {
                let mut current = lock_registry();
                match current.as_mut() {
                    Some(entry) if entry.snapshot.session_id == worker_session_id => {
                        entry.snapshot.lifecycle = "completed".to_string();
                        entry.snapshot.progress = Some(progress_from_report(&report));
                        entry.snapshot.report = Some(report);
                        Some(entry.snapshot.clone())
                    }
                    _ => None,
                }
            };

            if let Some(session) = finished {
                let _ = worker_app.emit(
                    EVENT_FINISHED,
                    MemoryTestSessionEvent {
                        session_id: worker_session_id,
                        session,
                    },
                );
            }
        });

    if let Err(error) = spawn_result {
        let message = format!("Não foi possível iniciar a thread do teste de RAM: {error}");
        let failed = {
            let mut current = lock_registry();
            match current.as_mut() {
                Some(entry) if entry.snapshot.session_id == session_id => {
                    entry.snapshot.lifecycle = "failed".to_string();
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

pub fn cancel<R: Runtime>(
    app: &AppHandle<R>,
    session_id: Option<&str>,
) -> Result<MemoryTestSessionSnapshot, String> {
    let snapshot = {
        let mut current = lock_registry();
        let entry = current
            .as_mut()
            .ok_or_else(|| "Não há sessão de teste de memória para cancelar.".to_string())?;

        if let Some(expected_id) = session_id {
            if expected_id != entry.snapshot.session_id {
                return Err("O identificador não corresponde à sessão atual de memória.".to_string());
            }
        }

        match entry.snapshot.lifecycle.as_str() {
            "running" => {
                entry.cancel.store(true, Ordering::Release);
                entry.snapshot.lifecycle = "cancelling".to_string();
                entry.snapshot.clone()
            }
            "cancelling" => entry.snapshot.clone(),
            _ => return Err("A sessão de teste de memória já terminou.".to_string()),
        }
    };

    emit_state(app, &snapshot);
    Ok(snapshot)
}

fn emit_state<R: Runtime>(app: &AppHandle<R>, snapshot: &MemoryTestSessionSnapshot) {
    let _ = app.emit(
        EVENT_STATE,
        MemoryTestSessionEvent {
            session_id: snapshot.session_id.clone(),
            session: snapshot.clone(),
        },
    );
}

fn progress_from_report(report: &MemoryTestReport) -> MemoryTestProgress {
    MemoryTestProgress {
        profile: report.profile,
        phase: match report.status {
            MemoryTestStatus::Passed => "concluido",
            MemoryTestStatus::ErrorsDetected => "divergencias_detectadas",
            MemoryTestStatus::Cancelled => "cancelado",
            MemoryTestStatus::Inconclusive => "inconclusivo",
            MemoryTestStatus::Failed => "falhou",
        }
        .to_string(),
        memory_budget_bytes: report.memory_budget_bytes,
        memory_tested_bytes: report.memory_tested_bytes,
        current_block_index: report.blocks_completed,
        blocks_completed: report.blocks_completed,
        current_block_bytes: 0,
        mismatches_total: report.mismatches_total,
        percent: if report.memory_budget_bytes == 0 {
            0
        } else {
            ((report.memory_tested_bytes.saturating_mul(100) / report.memory_budget_bytes).min(100))
                as u8
        },
    }
}

fn new_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "mem-{:016x}-{:08x}-{:08x}",
        nanos as u64,
        std::process::id(),
        counter as u32
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
    fn session_ids_are_safe_and_unique() {
        let first = new_session_id();
        let second = new_session_id();
        assert_ne!(first, second);
        assert!(first.starts_with("mem-"));
        assert!(first.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
        }));
    }

    #[test]
    fn final_progress_reflects_partial_completion_and_mismatch_count() {
        let report = MemoryTestReport {
            schema_version: 1,
            engine_version: "test".to_string(),
            profile: runner::MemoryTestProfile::Standard,
            status: MemoryTestStatus::Cancelled,
            stop_reason: runner::StopReason::UserCancelled,
            started_at: "start".to_string(),
            finished_at: "end".to_string(),
            duration_ms: 1,
            memory_budget_bytes: 100,
            memory_tested_bytes: 45,
            block_size_preferred_bytes: 16,
            largest_block_allocated_bytes: 16,
            blocks_completed: 3,
            passes_planned: 1,
            passes_completed: 0,
            algorithms_planned: Vec::new(),
            algorithms_executed: Vec::new(),
            a04_seeds: Vec::new(),
            mismatches_total: 7,
            mismatches: Vec::new(),
            coverage_description: "partial".to_string(),
            warnings: Vec::new(),
        };
        let progress = progress_from_report(&report);
        assert_eq!(progress.percent, 45);
        assert_eq!(progress.mismatches_total, 7);
        assert_eq!(progress.phase, "cancelado");
    }
}
