use serde::{Deserialize, Serialize};
use std::fs;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeRecord {
    pub id: String,
    pub timestamp: String,
    pub task_id: String,
    pub description: String,
    pub backup_location: Option<String>,
    pub reversible: bool,
    pub rollback_status: String,
    pub result: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupRecord {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub created_at: String,
    pub payload: String,
}

#[derive(Debug, Clone)]
pub struct SafetyManager {
    pub backup_root: PathBuf,
    pub changes: Vec<ChangeRecord>,
    pub backups: Vec<BackupRecord>,
    pub execution_state: HashMap<String, ExecutionState>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionState {
    pub task_id: String,
    pub status: String,
    pub started_at: String,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionHandle {
    pub task_id: String,
    pub status: String,
    pub cancelled: bool,
}

impl ExecutionHandle {
    pub fn is_running(&self) -> bool {
        self.status == "running" && !self.cancelled
    }
}

impl Default for SafetyManager {
    fn default() -> Self {
        let root = std::env::temp_dir().join("qboa-digital").join("backups");
        if !root.exists() {
            let _ = fs::create_dir_all(&root);
        }

        Self {
            backup_root: root,
            changes: Vec::new(),
            backups: Vec::new(),
            execution_state: HashMap::new(),
        }
    }
}

impl SafetyManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_restore_point(&mut self, reason: &str) -> Result<String, String> {
        let _ = reason;

        #[cfg(target_os = "windows")]
        {
            let script = format!(
                "Checkpoint-Computer -Description '{}' -RestorePointType 'MODIFY_SETTINGS' -ErrorAction Stop; Write-Output 'restore-point-created'",
                reason.replace("'", "''")
            );

            let output = Command::new("powershell")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
                .output()
                .map_err(|e| format!("Não foi possível iniciar o PowerShell para criar o ponto de restauração: {e}"))?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let combined = format!("{stdout}{stderr}").trim().to_string();

            if output.status.success() {
                let location = format!("Windows Restore Point: {reason}");
                return Ok(location);
            }

            Err(format!(
                "Proteção do Sistema não está disponível neste computador.\nDetalhe: {combined}"
            ))
        }

        #[cfg(not(target_os = "windows"))]
        {
            Err("Pontos de restauração do Windows só estão disponíveis em sistemas Windows.".to_string())
        }
    }

    pub fn create_backup(&mut self, kind: &str, path: &str, payload: &str) -> Result<String, String> {
        let session_id = current_timestamp_millis();
        let file_name = format!("{kind}-{session_id}.json");
        let target = self.backup_root.join(file_name);

        let backup = BackupRecord {
            id: format!("backup-{session_id}"),
            kind: kind.to_string(),
            path: path.to_string(),
            created_at: iso_timestamp(),
            payload: payload.to_string(),
        };

        let serialized = serde_json::to_string_pretty(&backup)
            .map_err(|e| format!("Falha ao serializar backup: {e}"))?;

        fs::write(&target, serialized)
            .map_err(|e| format!("Falha ao gravar backup em {}: {e}", target.display()))?;

        self.backups.push(backup);
        Ok(target.to_string_lossy().to_string())
    }

    pub fn record_change(
        &mut self,
        task_id: &str,
        description: &str,
        reversible: bool,
        backup_location: Option<&str>,
        result: &str,
    ) -> ChangeRecord {
        let id = format!("change-{}", current_timestamp_millis());
        let change = ChangeRecord {
            id: id.clone(),
            timestamp: iso_timestamp(),
            task_id: task_id.to_string(),
            description: description.to_string(),
            backup_location: backup_location.map(|value| value.to_string()),
            reversible,
            rollback_status: if reversible { "pending".to_string() } else { "irreversible".to_string() },
            result: result.to_string(),
        };

        self.changes.push(change.clone());
        change
    }

    pub fn rollback_task(&mut self, change_id: &str) -> Result<String, String> {
        let index = self
            .changes
            .iter()
            .rposition(|change| change.id == change_id)
            .ok_or_else(|| format!("Alteração não encontrada: {change_id}"))?;

        let change = self.changes[index].clone();
        if !change.reversible {
            return Ok(format!("Irreversível: {}", change.description));
        }

        if let Some(output) = rollback_windows_change(&change.task_id)? {
            self.changes[index].rollback_status = "rolled_back".to_string();
            return Ok(output);
        }

        let message = format!("Rollback solicitado para: {}", change.description);
        self.changes[index].rollback_status = "rolled_back".to_string();
        Ok(message)
    }

    pub fn rollback_session(&mut self) -> Result<Vec<String>, String> {
        let mut messages = Vec::new();
        let mut changes = self.changes.clone();
        changes.reverse();

        for change in changes {
            if !change.reversible {
                messages.push(format!("Irreversível: {}", change.description));
                continue;
            }

            let rollback_message = self.rollback_task(&change.id)?;
            messages.push(rollback_message);
        }

        Ok(messages)
    }

    pub fn journal(&self) -> Vec<ChangeRecord> {
        self.changes.clone()
    }

    pub fn register_execution(&mut self, task_id: &str) -> ExecutionHandle {
        let state = ExecutionState {
            task_id: task_id.to_string(),
            status: "running".to_string(),
            started_at: iso_timestamp(),
            cancelled: false,
        };

        self.execution_state.insert(task_id.to_string(), state.clone());
        ExecutionHandle {
            task_id: state.task_id.clone(),
            status: state.status.clone(),
            cancelled: state.cancelled,
        }
    }

    pub fn cancel_execution(&mut self, task_id: &str) -> bool {
        let Some(entry) = self.execution_state.get_mut(task_id) else {
            return false;
        };

        entry.status = "cancelled".to_string();
        entry.cancelled = true;
        true
    }

    pub fn is_cancelled(&self, task_id: &str) -> bool {
        matches!(
            self.execution_state.get(task_id),
            Some(entry) if entry.cancelled || entry.status == "cancelled"
        )
    }

    pub fn finish_execution(&mut self, task_id: &str, status: &str) -> Option<ExecutionState> {
        let state = self.execution_state.get_mut(task_id)?;
        state.status = status.to_string();
        Some(state.clone())
    }
}

#[cfg(target_os = "windows")]
fn rollback_windows_change(task_id: &str) -> Result<Option<String>, String> {
    let script = match task_id {
        "desengordurar_telemetria" | "desengordurar_telemetria_avancada" => r#"
$ErrorActionPreference = 'Stop'
$keys = @(
  @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'AllowTelemetry'),
  @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'AllowDeviceNameInTelemetry'),
  @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection', 'AllowTelemetry'),
  @('HKLM:\SOFTWARE\Wow6432Node\Microsoft\Windows\CurrentVersion\Policies\DataCollection', 'AllowTelemetry'),
  @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'DoNotShowFeedbackNotifications'),
  @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'DisableTelemetryOptInChangeNotification')
)
foreach ($key in $keys) { Remove-ItemProperty -Path $key[0] -Name $key[1] -ErrorAction SilentlyContinue }
foreach ($serviceName in @('DiagTrack', 'dmwappushservice')) {
  Set-Service -Name $serviceName -StartupType Manual -ErrorAction SilentlyContinue
  Start-Service -Name $serviceName -ErrorAction SilentlyContinue
}
Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskPath -match 'Application Experience|Customer Experience Improvement Program|Autochk\Proxy|DiskDiagnostic|Feedback' } | Enable-ScheduledTask -ErrorAction SilentlyContinue
'Políticas, serviços e tarefas de telemetria restaurados.'
"#,
        "desengordurar_tarefas" => r#"
$ErrorActionPreference = 'Stop'
Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskPath -match 'Application Experience|Customer Experience Improvement Program|Autochk\Proxy' } | Enable-ScheduledTask -ErrorAction SilentlyContinue
'Tarefas de diagnóstico restauradas.'
"#,
        _ => return Ok(None),
    };

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .output()
        .map_err(|error| format!("Falha ao iniciar rollback do Windows: {error}"))?;
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string();

    if output.status.success() {
        Ok(Some(message))
    } else {
        Err(format!("Rollback falhou para {task_id}: {message}"))
    }
}

#[cfg(not(target_os = "windows"))]
fn rollback_windows_change(task_id: &str) -> Result<Option<String>, String> {
    if matches!(task_id, "desengordurar_telemetria" | "desengordurar_telemetria_avancada" | "desengordurar_tarefas") {
        return Err("Rollback granular só pode ser executado em um sistema Windows.".to_string());
    }

    Ok(None)
}

fn current_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn iso_timestamp() -> String {
    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();
    let nanos = duration.subsec_nanos();
    format!("{secs}.{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_change_and_rollback_task() {
        let mut safety = SafetyManager::new();
        let change = safety.record_change(
            "task-1",
            "Telemetria desativada",
            true,
            Some("backup.json"),
            "ok",
        );

        assert_eq!(change.reversible, true);
        assert!(safety.rollback_task(&change.id).is_ok());
    }

    #[test]
    fn rollback_session_reports_irreversible_items() {
        let mut safety = SafetyManager::new();
        safety.record_change("task-1", "Cache limpo", false, None, "ok");
        let result = safety.rollback_session();
        assert!(result.is_ok());
    }

    #[test]
    fn execution_tracker_registers_and_cancels_tasks() {
        let mut state = SafetyManager::new();

        let task_id = "diagnostico_sfc";
        let handle = state.register_execution(task_id);
        assert!(handle.is_running());

        state.cancel_execution(task_id);
        assert!(state.is_cancelled(task_id));
    }
}

pub type SharedSafetyManager = Arc<Mutex<SafetyManager>>;
