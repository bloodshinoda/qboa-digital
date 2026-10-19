use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryExistence {
    Absent,
    Present,
}

impl Serialize for RegistryExistence {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bool(matches!(self, Self::Present))
    }
}

impl<'de> Deserialize<'de> for RegistryExistence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum LegacyExistence {
            Bool(bool),
            Name(String),
        }

        match LegacyExistence::deserialize(deserializer)? {
            LegacyExistence::Bool(true) => Ok(Self::Present),
            LegacyExistence::Bool(false) => Ok(Self::Absent),
            LegacyExistence::Name(value) if value.eq_ignore_ascii_case("present") => {
                Ok(Self::Present)
            }
            LegacyExistence::Name(value) if value.eq_ignore_ascii_case("absent") => {
                Ok(Self::Absent)
            }
            LegacyExistence::Name(value) => Err(serde::de::Error::custom(format!(
                "Estado de existência do Registro inválido: {value}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RollbackStatus {
    Pending,
    Applied,
    RollbackAvailable,
    RollingBack,
    RolledBack,
    RollbackFailed,
    RollbackPartial,
    Unknown,
    Irreversible,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistrySnapshot {
    pub hive: String,
    pub path: String,
    pub value_name: String,
    #[serde(rename = "exists", alias = "existence")]
    pub existence: RegistryExistence,
    pub value_type: Option<String>,
    pub value: Option<serde_json::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceSnapshot {
    pub name: String,
    pub exists: bool,
    pub startup_type: Option<String>,
    pub state: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledTaskSnapshot {
    pub task_path: String,
    pub task_name: String,
    pub exists: bool,
    pub enabled: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeatureSnapshot {
    pub name: String,
    pub exists: bool,
    pub state: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SystemSnapshot {
    pub strategy: String,
    pub registry: Vec<RegistrySnapshot>,
    pub services: Vec<ServiceSnapshot>,
    pub scheduled_tasks: Vec<ScheduledTaskSnapshot>,
    pub features: Vec<FeatureSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeRecord {
    pub id: String,
    pub execution_id: String,
    pub timestamp: String,
    pub task_id: String,
    pub description: String,
    pub backup_location: Option<String>,
    pub reversible: bool,
    pub rollback_status: RollbackStatus,
    pub result: String,
}

#[derive(Debug)]
pub struct RecordChangeError {
    pub change: ChangeRecord,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct SafetyManager {
    pub backup_root: PathBuf,
    pub changes: Vec<ChangeRecord>,
    pub execution_state: HashMap<String, ExecutionState>,
    journal_path: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionState {
    pub execution_id: String,
    pub task_id: String,
    pub status: String,
    pub started_at: String,
    pub cancelled: bool,
    pub process_id: Option<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionHandle {
    pub execution_id: String,
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
        // Snapshots ficam em LocalAppData, fora de %TEMP%: a própria limpeza
        // de temporários não pode apagar o backup necessário para validar/compensar o Registro.
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Qboa Digital")
            .join("snapshots");
        let journal_path = default_journal_path();
        let _ = fs::create_dir_all(&root);

        let mut manager = Self {
            backup_root: root,
            changes: Vec::new(),
            execution_state: HashMap::new(),
            journal_path,
        };
        manager.load_journal();
        manager
    }
}

impl SafetyManager {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(crate) fn with_journal_path(path: PathBuf) -> Self {
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Qboa Digital")
            .join("snapshots");
        let _ = fs::create_dir_all(&root);
        let mut manager = Self {
            backup_root: root,
            changes: Vec::new(),
            execution_state: HashMap::new(),
            journal_path: path,
        };
        manager.load_journal();
        manager
    }

    fn load_journal(&mut self) {
        if !self.journal_path.exists() {
            return;
        }

        let data = match fs::read_to_string(&self.journal_path) {
            Ok(data) => data,
            Err(error) => {
                write_safety_log(&format!(
                    "journal load failed path={} error={error}",
                    self.journal_path.display()
                ));
                return;
            }
        };

        match serde_json::from_str::<Vec<ChangeRecord>>(&data) {
            Ok(changes) => {
                let mut unique = HashMap::new();
                for change in changes {
                    unique.insert(change.id.clone(), change);
                }
                self.changes = unique.into_values().collect();
                self.changes
                    .sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then_with(|| a.id.cmp(&b.id)));
            }
            Err(error) => {
                let corrupt_path = self
                    .journal_path
                    .with_extension(format!("corrupt-{}", unique_id()));
                let renamed = fs::rename(&self.journal_path, &corrupt_path).is_ok();
                write_safety_log(&format!(
                    "journal invalid path={} error={error} recovery_file={} renamed={renamed}",
                    self.journal_path.display(),
                    corrupt_path.display()
                ));
                self.changes.clear();
            }
        }
    }

    fn persist_journal(&self) -> Result<(), String> {
        self.persist_journal_with(replace_file_atomically)
    }

    fn persist_journal_with<F>(&self, replace: F) -> Result<(), String>
    where
        F: FnOnce(&Path, &Path) -> Result<(), String>,
    {
        let parent = self
            .journal_path
            .parent()
            .ok_or_else(|| "Caminho do journal sem diretório pai.".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("Falha ao criar diretório do journal: {e}"))?;

        let data = serde_json::to_vec_pretty(&self.changes)
            .map_err(|e| format!("Falha ao serializar journal: {e}"))?;

        let temp_path = self
            .journal_path
            .with_extension(format!("tmp-{}", unique_id()));
        {
            let mut file = fs::File::create(&temp_path)
                .map_err(|e| format!("Falha ao criar journal temporário: {e}"))?;
            file.write_all(&data)
                .map_err(|e| format!("Falha ao gravar journal temporário: {e}"))?;
            file.sync_all()
                .map_err(|e| format!("Falha ao sincronizar journal temporário: {e}"))?;
        }

        if let Err(error) = replace(&temp_path, &self.journal_path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }

        Ok(())
    }

    fn persist_after_change(&self, operation: &str) -> Result<(), String> {
        self.persist_journal().map_err(|error| {
            write_safety_log(&format!(
                "journal persist failed operation={operation} error={error}"
            ));
            error
        })
    }
    pub fn create_restore_point(&mut self, reason: &str) -> Result<String, String> {
        #[cfg(target_os = "windows")]
        {
            let script = format!("Checkpoint-Computer -Description '{}' -RestorePointType 'MODIFY_SETTINGS' -ErrorAction Stop", reason.replace("'", "''"));
            write_safety_log(&format!("restore-point started reason={reason}"));
            let output = Command::new("powershell")
                .creation_flags(0x08000000)
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    &script,
                ])
                .output()
                .map_err(|e| format!("Falha ao criar ponto de restauração: {e}"))?;
            if output.status.success() {
                write_safety_log("restore-point completed");
                return Ok(format!("Windows Restore Point: {reason}"));
            }
            let error = format!(
                "Falha ao criar ponto de restauração: {}",
                output_text(&output.stdout, &output.stderr)
            );
            write_safety_log(&format!("restore-point failed error={error}"));
            return Err(error);
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = reason;
            Err("Pontos de restauração só estão disponíveis no Windows.".to_string())
        }
    }
    pub fn capture_snapshot(&self, task_id: &str) -> Result<String, String> {
        let strategy = known_strategy(task_id)
            .ok_or_else(|| format!("Sem snapshot seguro para a tarefa: {task_id}"))?;
        #[cfg(target_os = "windows")]
        {
            let output = run_snapshot_script(strategy)?;
            let snapshot: SystemSnapshot =
                serde_json::from_str(&output).map_err(|e| format!("Snapshot inválido: {e}"))?;
            if snapshot.strategy != strategy {
                return Err("Snapshot retornou estratégia inesperada.".to_string());
            }
            let location = self
                .backup_root
                .join(format!("snapshot-{task_id}-{}.json", unique_id()));
            fs::write(
                &location,
                serde_json::to_vec_pretty(&snapshot).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Falha ao salvar snapshot: {e}"))?;
            return Ok(location.to_string_lossy().to_string());
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = strategy;
            Err("Snapshots do Windows só estão disponíveis no Windows.".to_string())
        }
    }

    pub fn record_change(
        &mut self,
        execution_id: &str,
        task_id: &str,
        description: &str,
        reversible: bool,
        snapshot: Option<&str>,
        result: &str,
    ) -> Result<ChangeRecord, RecordChangeError> {
        let change = ChangeRecord {
            id: format!("change-{}", unique_id()),
            execution_id: execution_id.to_string(),
            timestamp: iso_timestamp(),
            task_id: task_id.to_string(),
            description: description.to_string(),
            backup_location: snapshot.map(str::to_string),
            reversible,
            rollback_status: if reversible {
                RollbackStatus::RollbackAvailable
            } else {
                RollbackStatus::Irreversible
            },
            result: result.to_string(),
        };
        self.changes.push(change.clone());
        if let Err(error) = self.persist_after_change("record_change") {
            self.changes.retain(|record| record.id != change.id);
            return Err(RecordChangeError { change, error });
        }
        Ok(change)
    }
    pub fn rollback_task(&mut self, change_id: &str) -> Result<String, String> {
        self.rollback_task_with(change_id, &restore_snapshot)
    }

    pub(crate) fn rollback_task_with<F>(
        &mut self,
        change_id: &str,
        restore: &F,
    ) -> Result<String, String>
    where
        F: Fn(&str, &str) -> Result<String, String>,
    {
        let index = self
            .changes
            .iter()
            .rposition(|c| c.id == change_id)
            .ok_or_else(|| format!("Alteração não encontrada: {change_id}"))?;
        let change = self.changes[index].clone();
        if !change.reversible {
            return Err(format!("Alteração irreversível: {}", change.description));
        }
        if change.rollback_status == RollbackStatus::RolledBack {
            return Err("Rollback já concluído.".to_string());
        }
        let previous_status = self.changes[index].rollback_status.clone();
        self.changes[index].rollback_status = RollbackStatus::RollingBack;
        if let Err(error) = self.persist_after_change("rollback_started") {
            self.changes[index].rollback_status = previous_status;
            return Err(format!(
                "Não foi possível registrar o início do rollback: {error}"
            ));
        }

        let location = match change.backup_location.clone() {
            Some(location) => location,
            None => {
                self.changes[index].rollback_status = RollbackStatus::RollbackFailed;
                if let Err(error) = self.persist_after_change("rollback_failed_without_snapshot") {
                    self.changes[index].rollback_status = RollbackStatus::Unknown;
                    return Err(format!(
                        "Rollback sem snapshot; não foi possível registrar a falha: {error}"
                    ));
                }
                return Err("Rollback sem snapshot.".to_string());
            }
        };
        match restore(&location, &change.task_id) {
            Ok(message) => {
                self.changes[index].rollback_status = RollbackStatus::RolledBack;
                if let Err(error) = self.persist_after_change("rollback_completed") {
                    self.changes[index].rollback_status = RollbackStatus::Unknown;
                    return Err(format!(
                        "Rollback aplicado, mas o journal não confirmou o resultado: {error}"
                    ));
                }
                Ok(message)
            }
            Err(error) => {
                self.changes[index].rollback_status = if error.contains("parcial") {
                    RollbackStatus::RollbackPartial
                } else {
                    RollbackStatus::RollbackFailed
                };
                if let Err(persist_error) = self.persist_after_change("rollback_failed") {
                    self.changes[index].rollback_status = RollbackStatus::Unknown;
                    return Err(format!(
                        "{error}; falha ao atualizar o journal: {persist_error}"
                    ));
                }
                Err(error)
            }
        }
    }
    pub fn rollback_session(&mut self, execution_id: Option<&str>) -> Result<Vec<String>, String> {
        self.rollback_session_with(execution_id, &restore_snapshot)
    }

    fn rollback_session_with<F>(
        &mut self,
        execution_id: Option<&str>,
        restore: &F,
    ) -> Result<Vec<String>, String>
    where
        F: Fn(&str, &str) -> Result<String, String>,
    {
        let ids: Vec<String> = self
            .changes
            .iter()
            .rev()
            .filter(|c| execution_id.map(|id| id == c.execution_id).unwrap_or(true))
            .map(|c| c.id.clone())
            .collect();

        let mut result = Vec::new();
        let mut failures = Vec::new();

        for id in ids {
            let Some(change) = self.changes.iter().find(|change| change.id == id).cloned() else {
                failures.push(format!("{id}: alteração não encontrada"));
                continue;
            };

            if !change.reversible {
                result.push(format!(
                    "Ignorada alteração irreversível: {}",
                    change.description
                ));
                continue;
            }

            match self.rollback_task_with(&id, restore) {
                Ok(message) => result.push(message),
                Err(error) => failures.push(format!("{}: {error}", change.description)),
            }
        }

        if failures.is_empty() {
            Ok(result)
        } else {
            Err(format!(
                "Rollback da sessão concluído com falhas ({}): {}",
                failures.len(),
                failures.join(" | ")
            ))
        }
    }
    pub fn journal(&self) -> Vec<ChangeRecord> {
        self.changes.clone()
    }
    pub fn rollback_status(&self, change_id: &str) -> Option<RollbackStatus> {
        self.changes
            .iter()
            .find(|change| change.id == change_id)
            .map(|change| change.rollback_status.clone())
    }
    pub fn update_change_result(&mut self, change_id: &str, result: &str) -> Result<(), String> {
        if let Some(index) = self
            .changes
            .iter()
            .position(|change| change.id == change_id)
        {
            let previous = std::mem::replace(&mut self.changes[index].result, result.to_string());
            if let Err(error) = self.persist_after_change("update_change_result") {
                self.changes[index].result = previous;
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn rollback_unrecorded_change(&self, change: &ChangeRecord) -> Result<String, String> {
        if !change.reversible {
            return Err(format!("Alteração irreversível: {}", change.description));
        }
        let location = change
            .backup_location
            .as_deref()
            .ok_or_else(|| "Rollback sem snapshot.".to_string())?;
        restore_snapshot(location, &change.task_id)
    }
    pub fn register_execution(&mut self, task_id: &str) -> ExecutionHandle {
        let execution_id = format!("execution-{}", unique_id());
        self.execution_state.insert(
            execution_id.clone(),
            ExecutionState {
                execution_id: execution_id.clone(),
                task_id: task_id.to_string(),
                status: "running".to_string(),
                started_at: iso_timestamp(),
                cancelled: false,
                process_id: None,
            },
        );
        ExecutionHandle {
            execution_id,
            task_id: task_id.to_string(),
            status: "running".to_string(),
            cancelled: false,
        }
    }
    pub fn cancel_execution(&mut self, execution_id: &str) -> bool {
        let Some(state) = self.execution_state.get_mut(execution_id) else {
            return false;
        };
        state.cancelled = true;
        state.status = "cancelling".to_string();
        true
    }
    pub fn is_cancelled(&self, execution_id: &str) -> bool {
        self.execution_state
            .get(execution_id)
            .is_some_and(|s| s.cancelled)
    }
    pub fn execution_for_task(&self, task_id: &str) -> Option<String> {
        let matches: Vec<&ExecutionState> = self
            .execution_state
            .values()
            .filter(|state| {
                state.task_id == task_id
                    && (state.status == "running" || state.status == "cancelling")
            })
            .collect();
        (matches.len() == 1).then(|| matches[0].execution_id.clone())
    }
    pub fn set_process_id(&mut self, execution_id: &str, pid: u32) {
        if let Some(s) = self.execution_state.get_mut(execution_id) {
            s.process_id = Some(pid)
        }
    }
    pub fn process_id(&self, execution_id: &str) -> Option<u32> {
        self.execution_state
            .get(execution_id)
            .and_then(|s| s.process_id)
    }
    pub fn is_task_running(&self) -> bool {
        self.execution_state
            .values()
            .any(|s| s.status == "running" || s.status == "cancelling")
    }
    pub fn finish_execution(&mut self, execution_id: &str, status: &str) -> Option<ExecutionState> {
        let state = self.execution_state.get_mut(execution_id)?;
        state.status = status.to_string();
        Some(state.clone())
    }
}

fn write_safety_log(message: &str) {
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Qboa Digital")
        .join("logs");
    let _ = fs::create_dir_all(&root);
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("execution.log"))
    {
        use std::io::Write;
        let _ = writeln!(file, "[safety] {message}");
    }
}

fn known_strategy(task_id: &str) -> Option<&'static str> {
    match task_id {
        "desengordurar_telemetria" => Some("telemetry"),
        "desengordurar_tarefas" => Some("diagnostic_tasks"),
        "limpeza_leve" | "limpeza_media" | "limpeza_pesada" => Some("mru_cleanup"),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn run_snapshot_script(strategy: &str) -> Result<String, String> {
    let script = if strategy == "telemetry" {
        snapshot_script(strategy).replace(
            "$sv=@();",
            "$specs+=@(@('LocalMachine','SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\DataCollection','AllowTelemetry'),@('LocalMachine','SOFTWARE\\Wow6432Node\\Microsoft\\Windows\\CurrentVersion\\Policies\\DataCollection','AllowTelemetry'),@('LocalMachine','SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection','DisableEnterpriseAuthProxy'),@('LocalMachine','SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsAI','DisableAIDataAnalysis'));$sv=@();",
        ).replace("$t-eq'Binary'", "($t-eq'Binary' -or $t-eq'None')")
    } else {
        snapshot_script(strategy).to_string()
    };
    let utf8_script = format!(
        "$utf8=[System.Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=$utf8;{script}"
    );
    let output = Command::new("powershell")
        .creation_flags(0x08000000)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &utf8_script,
        ])
        .output()
        .map_err(|e| format!("Falha ao capturar snapshot: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "Falha ao capturar snapshot: {}",
            output_text(&output.stdout, &output.stderr)
        ))
    }
}

#[cfg(target_os = "windows")]
fn snapshot_script(strategy: &str) -> &'static str {
    match strategy {
        "telemetry" => {
            r#"$ErrorActionPreference='Stop';$r=@();$specs=@(@('LocalMachine','SOFTWARE\Policies\Microsoft\Windows\DataCollection','AllowTelemetry'),@('LocalMachine','SOFTWARE\Policies\Microsoft\Windows\DataCollection','AllowDeviceNameInTelemetry'),@('LocalMachine','SOFTWARE\Policies\Microsoft\Windows\DataCollection','DoNotShowFeedbackNotifications'),@('LocalMachine','SOFTWARE\Policies\Microsoft\Windows\DataCollection','DisableTelemetryOptInChangeNotification'),@('LocalMachine','SOFTWARE\Policies\Microsoft\Windows\WindowsAI','TurnOffWindowsCopilot'),@('CurrentUser','SOFTWARE\Policies\Microsoft\Windows\WindowsAI','DisableAIDataAnalysis'),@('LocalMachine','SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced','EnableRecallOnDevice'),@('CurrentUser','SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced','EnableRecallOnDevice'));foreach($s in $specs){$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$($s[0]),[Microsoft.Win32.RegistryView]::Default);$k=$b.OpenSubKey($s[1]);$e=$k -and ($k.GetValueNames()-contains $s[2]);$v=$null;$t=$null;if($e){$t=$k.GetValueKind($s[2]).ToString();$v=$k.GetValue($s[2],$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);if($t-eq'Binary'){$v=[Convert]::ToBase64String($v)}};$r+=[pscustomobject]@{hive=$s[0];path=$s[1];value_name=$s[2];exists=$e;value_type=$t;value=$v}};$sv=@();foreach($n in @('DiagTrack','dmwappushservice')){$x=Get-CimInstance Win32_Service -Filter "Name='$n'";$sv+=[pscustomobject]@{name=$n;exists=[bool]$x;startup_type=if($x){$x.StartMode}else{$null};state=if($x){$x.State}else{$null}}};$tasks=@();foreach($n in @('\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser','\Microsoft\Windows\Application Experience\ProgramDataUpdater','\Microsoft\Windows\Autochk\Proxy','\Microsoft\Windows\Customer Experience Improvement Program\Consolidator','\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip','\Microsoft\Windows\Customer Experience Improvement Program\KernelCeipTask','\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector','\Microsoft\Windows\Feedback\Siuf\DmClient','\Microsoft\Windows\Feedback\Siuf\DmClientOnScenarioDownload')){$p=[System.IO.Path]::GetDirectoryName($n)+'\';$q=[System.IO.Path]::GetFileName($n);$x=Get-ScheduledTask -TaskPath $p -TaskName $q -ErrorAction SilentlyContinue;$tasks+=[pscustomobject]@{task_path=$p;task_name=$q;exists=[bool]$x;enabled=if($x){$x.State-ne'Disabled'}else{$null}}};$f=Get-WindowsOptionalFeature -Online -FeatureName Recall -ErrorAction SilentlyContinue;[pscustomobject]@{strategy='telemetry';registry=$r;services=$sv;scheduled_tasks=$tasks;features=@([pscustomobject]@{name='Recall';exists=[bool]$f;state=if($f){$f.State}else{$null}})}|ConvertTo-Json -Depth 8 -Compress"#
        }
        "diagnostic_tasks" => {
            r#"$ErrorActionPreference='Stop';$tasks=@();foreach($n in @('\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser','\Microsoft\Windows\Application Experience\ProgramDataUpdater','\Microsoft\Windows\Autochk\Proxy')){$p=[System.IO.Path]::GetDirectoryName($n)+'\';$q=[System.IO.Path]::GetFileName($n);$x=Get-ScheduledTask -TaskPath $p -TaskName $q -ErrorAction SilentlyContinue;$tasks+=[pscustomobject]@{task_path=$p;task_name=$q;exists=[bool]$x;enabled=if($x){$x.State-ne'Disabled'}else{$null}}};[pscustomobject]@{strategy='diagnostic_tasks';registry=@();services=@();scheduled_tasks=$tasks;features=@()}|ConvertTo-Json -Depth 8 -Compress"#
        }
        "mru_cleanup" => {
            r#"$ErrorActionPreference='Stop';$roots=@(@('CurrentUser','Software\Microsoft\Windows\CurrentVersion\Explorer\RecentDocs'),@('CurrentUser','Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU'),@('CurrentUser','Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths'));$r=@();foreach($root in $roots){$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$($root[0]),[Microsoft.Win32.RegistryView]::Default);$queue=New-Object System.Collections.Queue;$queue.Enqueue($root[1]);while($queue.Count -gt 0){$p=[string]$queue.Dequeue();$k=$b.OpenSubKey($p);if($k){foreach($n in $k.GetValueNames()){$t=$k.GetValueKind($n).ToString();$v=$k.GetValue($n,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);if($t-eq'Binary' -or $t-eq'None'){$v=[Convert]::ToBase64String($v)};$r+=[pscustomobject]@{hive=$root[0];path=$p;value_name=$n;exists=$true;value_type=$t;value=$v}};foreach($n in $k.GetSubKeyNames()){$queue.Enqueue("$p\$n")};$k.Dispose()}}};[pscustomobject]@{strategy='mru_cleanup';registry=@($r);services=@();scheduled_tasks=@();features=@()}|ConvertTo-Json -Depth 8 -Compress"#
        }
        _ => "",
    }
}

#[cfg(target_os = "windows")]
fn restore_snapshot(location: &str, task_id: &str) -> Result<String, String> {
    let strategy = known_strategy(task_id)
        .ok_or_else(|| "Estratégia de rollback não permitida.".to_string())?;
    let snapshot: SystemSnapshot = serde_json::from_str(
        &fs::read_to_string(location).map_err(|e| format!("Snapshot indisponível: {e}"))?,
    )
    .map_err(|e| format!("Snapshot inválido: {e}"))?;
    if snapshot.strategy != strategy {
        return Err("Snapshot e estratégia não correspondem.".to_string());
    }
    let encoded = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;
    let restore = restore_script().replace(
        "$v=$x.value;if($x.value_type-eq'Binary'){$v=[Convert]::FromBase64String($v)};",
        "$v=$x.value;switch($x.value_type){'Binary'{$v=[Convert]::FromBase64String([string]$v)}'None'{$v=[Convert]::FromBase64String([string]$v)}'DWord'{$v=[int]$v}'QWord'{$v=[long]$v}'MultiString'{$v=[string[]]$v}};",
    );
    let script = format!(
        "$s=ConvertFrom-Json -InputObject '{}';{}",
        encoded.replace('\'', "''"),
        restore
    );
    let output = Command::new("powershell")
        .creation_flags(0x08000000)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()
        .map_err(|e| format!("Falha ao iniciar rollback: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Rollback falhou: {}",
            output_text(&output.stdout, &output.stderr)
        ));
    }
    let after: SystemSnapshot = serde_json::from_str(&run_snapshot_script(strategy)?)
        .map_err(|e| format!("Validação inválida: {e}"))?;
    if !snapshot_matches_captured_state(&after, &snapshot) {
        return Err("Rollback parcial: validação não corresponde ao snapshot.".to_string());
    }
    Ok("Rollback aplicado e validado.".to_string())
}

#[cfg(target_os = "windows")]
fn snapshot_matches_captured_state(actual: &SystemSnapshot, expected: &SystemSnapshot) -> bool {
    actual.strategy == expected.strategy
        && expected
            .registry
            .iter()
            .all(|entry| actual.registry.contains(entry))
        && expected
            .services
            .iter()
            .all(|entry| actual.services.contains(entry))
        && expected
            .scheduled_tasks
            .iter()
            .all(|entry| actual.scheduled_tasks.contains(entry))
        && expected
            .features
            .iter()
            .all(|entry| actual.features.contains(entry))
}

#[cfg(not(target_os = "windows"))]
fn restore_snapshot(_location: &str, _task_id: &str) -> Result<String, String> {
    Err("Rollback granular só pode ser validado no Windows.".to_string())
}
#[cfg(target_os = "windows")]
fn restore_script() -> &'static str {
    r#"foreach($x in $s.registry){$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$($x.hive),[Microsoft.Win32.RegistryView]::Default);$k=$b.OpenSubKey($x.path,$true);if(!$x.exists){if($k){$k.DeleteValue($x.value_name,$false)}}else{if(!$k){$k=$b.CreateSubKey($x.path)};$v=$x.value;if($x.value_type-eq'Binary'){$v=[Convert]::FromBase64String($v)};$k.SetValue($x.value_name,$v,[Microsoft.Win32.RegistryValueKind]::$($x.value_type))}};foreach($x in $s.services){if($x.exists){$startup=$x.startup_type;if($startup-eq'Auto'){$startup='Automatic'};Set-Service -Name $x.name -StartupType $startup;if($x.state-eq'Running'){Start-Service -Name $x.name}else{Stop-Service -Name $x.name -Force}}};foreach($x in $s.scheduled_tasks){if($x.exists){if($x.enabled){Enable-ScheduledTask -TaskPath $x.task_path -TaskName $x.task_name}else{Disable-ScheduledTask -TaskPath $x.task_path -TaskName $x.task_name}}};foreach($x in $s.features){if($x.exists -and $x.state-eq'Enabled'){Enable-WindowsOptionalFeature -Online -FeatureName $x.name -NoRestart}else{Disable-WindowsOptionalFeature -Online -FeatureName $x.name -NoRestart}}"#
}

#[cfg(target_os = "windows")]
fn output_text(stdout: &[u8], stderr: &[u8]) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    )
    .trim()
    .to_string()
}
fn default_journal_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Qboa Digital")
            .join("journal")
            .join("changes.json")
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::temp_dir()
            .join("qboa-digital")
            .join("journal")
            .join("changes.json")
    }
}

fn replace_file_atomically(temp_path: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if destination.exists() {
            return replace_existing_file_windows(temp_path, destination);
        }

        return move_new_file_windows(temp_path, destination);
    }

    #[cfg(not(target_os = "windows"))]
    fs::rename(temp_path, destination)
        .map_err(|e| format!("Falha ao finalizar journal atômico: {e}"))
}

#[cfg(target_os = "windows")]
fn replace_existing_file_windows(temp_path: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    let backup_path = destination.with_extension(format!("backup-{}", unique_id()));
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let temp_wide: Vec<u16> = temp_path.as_os_str().encode_wide().chain(Some(0)).collect();
    let backup_wide: Vec<u16> = backup_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let replaced = unsafe {
        ReplaceFileW(
            destination_wide.as_ptr(),
            temp_wide.as_ptr(),
            backup_wide.as_ptr(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if replaced == 0 {
        let error = std::io::Error::last_os_error();
        if backup_path.exists() {
            let restored = unsafe {
                MoveFileExW(
                    backup_wide.as_ptr(),
                    destination_wide.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if restored == 0 {
                return Err(format!(
                    "Falha ao substituir journal existente: {error}; o journal anterior está preservado em {} e não pôde ser restaurado: {}",
                    backup_path.display(),
                    std::io::Error::last_os_error()
                ));
            }
        }
        return Err(format!("Falha ao substituir journal existente: {error}"));
    }
    if let Err(error) = fs::remove_file(&backup_path) {
        write_safety_log(&format!(
            "journal replacement succeeded but backup cleanup failed path={} error={error}",
            backup_path.display()
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn move_new_file_windows(temp_path: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    let temp_wide: Vec<u16> = temp_path.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let moved = unsafe {
        MoveFileExW(
            temp_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(format!(
            "Falha ao finalizar journal: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
#[link(name = "Kernel32")]
extern "system" {
    fn ReplaceFileW(
        replaced_file_name: *const u16,
        replacement_file_name: *const u16,
        backup_file_name: *const u16,
        replace_flags: u32,
        exclude: *mut std::ffi::c_void,
        reserved: *mut std::ffi::c_void,
    ) -> i32;
    fn MoveFileExW(existing_file_name: *const u16, new_file_name: *const u16, flags: u32) -> i32;
}

fn unique_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!(
        "{}-{n}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    )
}
fn iso_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let d = doy - (153 * mp + 2).div_euclid(5) + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FakeRegistry {
        exists: bool,
        kind: String,
        value: String,
    }
    impl FakeRegistry {
        fn restore(&mut self, before: &Self) {
            *self = before.clone();
        }
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FakeService {
        startup: String,
        running: bool,
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FakeTask {
        exists: bool,
        enabled: bool,
    }
    #[test]
    fn registry_existing_value_and_type() {
        let before = FakeRegistry {
            exists: true,
            kind: "DWord".into(),
            value: "7".into(),
        };
        let mut now = FakeRegistry {
            exists: true,
            kind: "String".into(),
            value: "changed".into(),
        };
        now.restore(&before);
        assert_eq!(now, before)
    }
    #[test]
    fn registry_missing_value_stays_missing() {
        let before = FakeRegistry {
            exists: false,
            kind: String::new(),
            value: String::new(),
        };
        let mut now = FakeRegistry {
            exists: true,
            kind: "DWord".into(),
            value: "1".into(),
        };
        now.restore(&before);
        assert!(!now.exists)
    }

    #[test]
    fn registry_existence_is_independent_from_value_and_type() {
        let snapshots = vec![
            RegistrySnapshot {
                hive: "LocalMachine".into(),
                path: "Software\\Qboa".into(),
                value_name: "Absent".into(),
                existence: RegistryExistence::Absent,
                value_type: None,
                value: None,
            },
            RegistrySnapshot {
                hive: "LocalMachine".into(),
                path: "Software\\Qboa".into(),
                value_name: "Zero".into(),
                existence: RegistryExistence::Present,
                value_type: Some("DWord".into()),
                value: Some(serde_json::json!(0)),
            },
            RegistrySnapshot {
                hive: "LocalMachine".into(),
                path: "Software\\Qboa".into(),
                value_name: "Empty".into(),
                existence: RegistryExistence::Present,
                value_type: Some("String".into()),
                value: Some(serde_json::json!("")),
            },
            RegistrySnapshot {
                hive: "LocalMachine".into(),
                path: "Software\\Qboa".into(),
                value_name: "None".into(),
                existence: RegistryExistence::Present,
                value_type: Some("None".into()),
                value: None,
            },
        ];

        let encoded = serde_json::to_value(&snapshots).unwrap();
        assert_eq!(encoded[0]["exists"], false);
        assert_eq!(encoded[1]["exists"], true);
        assert_eq!(encoded[2]["exists"], true);
        assert_eq!(encoded[3]["exists"], true);
        assert_eq!(encoded[1]["value"], 0);
        assert_eq!(encoded[2]["value"], "");
        assert_eq!(encoded[3]["value_type"], "None");

        let decoded: Vec<RegistrySnapshot> = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, snapshots);
    }

    #[test]
    fn legacy_registry_snapshot_journal_shape_remains_readable() {
        let legacy = serde_json::json!({
            "strategy": "telemetry",
            "registry": [{
                "hive": "LocalMachine",
                "path": "SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection",
                "value_name": "AllowTelemetry",
                "exists": true,
                "value_type": "DWord",
                "value": 0
            }],
            "services": [],
            "scheduled_tasks": [],
            "features": []
        });

        let snapshot: SystemSnapshot = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(snapshot.strategy, "telemetry");
        assert_eq!(snapshot.registry[0].existence, RegistryExistence::Present);
        assert_eq!(snapshot.registry[0].value, Some(serde_json::json!(0)));
        assert_eq!(
            serde_json::to_value(snapshot).unwrap()["registry"][0]["exists"],
            true
        );
    }
    #[test]
    fn services_restore_all_states() {
        for (startup, running) in [
            ("Automatic", true),
            ("Automatic", false),
            ("Manual", true),
            ("Manual", false),
            ("Disabled", false),
        ] {
            let before = FakeService {
                startup: startup.into(),
                running,
            };
            let now = before.clone();
            assert_eq!(now, before)
        }
    }
    #[test]
    fn scheduled_tasks_restore_enabled_disabled_missing() {
        for before in [
            FakeTask {
                exists: true,
                enabled: true,
            },
            FakeTask {
                exists: true,
                enabled: false,
            },
            FakeTask {
                exists: false,
                enabled: false,
            },
        ] {
            let now = before.clone();
            assert_eq!(now, before)
        }
    }
    #[test]
    fn rollback_without_snapshot_fails() {
        let path = std::env::temp_dir().join(format!("qboa-rollback-missing-{}.json", unique_id()));
        let mut manager = SafetyManager::with_journal_path(path.clone());
        let change = manager
            .record_change(
                "exec-1",
                "desengordurar_telemetria",
                "x",
                true,
                None,
                "applied",
            )
            .unwrap();
        assert!(manager.rollback_task(&change.id).is_err());
        assert_eq!(
            manager.journal()[0].rollback_status,
            RollbackStatus::RollbackFailed
        );
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn repeated_rollback_is_rejected() {
        let mut manager = SafetyManager::new();
        let mut change = manager
            .record_change("exec-1", "x", "x", true, Some("missing"), "applied")
            .unwrap();
        change.rollback_status = RollbackStatus::RolledBack;
        manager.changes[0] = change;
        let change_id = manager.changes[0].id.clone();
        assert!(manager.rollback_task(&change_id).is_err())
    }
    #[test]
    fn executions_same_task_are_independent() {
        let mut manager = SafetyManager::new();
        let first = manager.register_execution("same");
        let second = manager.register_execution("same");
        assert_ne!(first.execution_id, second.execution_id);
        assert!(manager.cancel_execution(&first.execution_id));
        assert!(manager.is_cancelled(&first.execution_id));
        assert!(!manager.is_cancelled(&second.execution_id))
    }
    #[test]
    fn timestamp_is_rfc3339_utc() {
        let timestamp = iso_timestamp();
        assert!(timestamp.ends_with('Z'));
        assert_eq!(timestamp.len(), 20);
        assert_eq!(&timestamp[4..5], "-");
        assert_eq!(&timestamp[7..8], "-");
        assert_eq!(&timestamp[10..11], "T");
        assert_eq!(&timestamp[13..14], ":");
        assert_eq!(&timestamp[16..17], ":");
    }

    #[test]
    fn journal_round_trip_uses_persistent_file() {
        let path = std::env::temp_dir().join(format!("qboa-journal-test-{}.json", unique_id()));
        let mut manager = SafetyManager::with_journal_path(path.clone());
        let change = manager
            .record_change(
                "exec-roundtrip",
                "desengordurar_telemetria",
                "roundtrip",
                true,
                None,
                "applied",
            )
            .unwrap();

        let restored = SafetyManager::with_journal_path(path.clone());
        assert_eq!(restored.changes, vec![change]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn failed_journal_persistence_does_not_register_change() {
        let path = std::env::temp_dir().join(format!("qboa-journal-failure-{}", unique_id()));
        fs::create_dir_all(&path).unwrap();
        let mut manager = SafetyManager::with_journal_path(path.clone());

        let error = manager
            .record_change(
                "exec-persist-failure",
                "desengordurar_telemetria",
                "unpersisted",
                true,
                None,
                "completed",
            )
            .unwrap_err();

        assert!(error.error.contains("journal"));
        assert_eq!(error.change.result, "completed");
        assert!(manager.journal().is_empty());

        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn journal_replacement_updates_existing_file() {
        let root = std::env::temp_dir().join(format!("qboa-journal-replace-{}", unique_id()));
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("changes.json");
        let temporary = root.join("changes.tmp");
        fs::write(&destination, b"old journal").unwrap();
        fs::write(&temporary, b"new journal").unwrap();

        replace_file_atomically(&temporary, &destination).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"new journal");
        assert!(!temporary.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_journal_replacement_preserves_previous_file() {
        let root = std::env::temp_dir().join(format!("qboa-journal-replace-fail-{}", unique_id()));
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("changes.json");
        let missing_temporary = root.join("missing.tmp");
        fs::write(&destination, b"previous journal").unwrap();

        assert!(replace_file_atomically(&missing_temporary, &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"previous journal");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_journal_persistence_keeps_previous_journal_contents() {
        let path = std::env::temp_dir().join(format!("qboa-journal-preserve-{}", unique_id()));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"[]").unwrap();
        let mut manager = SafetyManager::with_journal_path(path.clone());
        manager.changes.push(ChangeRecord {
            id: "change-unpersisted".to_string(),
            execution_id: "exec-unpersisted".to_string(),
            timestamp: iso_timestamp(),
            task_id: "test".to_string(),
            description: "unpersisted".to_string(),
            backup_location: None,
            reversible: false,
            rollback_status: RollbackStatus::Irreversible,
            result: "completed".to_string(),
        });

        let error = manager
            .persist_journal_with(|_, _| Err("replacement intentionally failed".to_string()))
            .unwrap_err();

        assert!(error.contains("replacement intentionally failed"));
        assert_eq!(fs::read(&path).unwrap(), b"[]");
        assert_eq!(manager.journal().len(), 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn journal_persists_result_and_rollback_status() {
        let path = std::env::temp_dir().join(format!("qboa-journal-status-{}.json", unique_id()));
        let mut manager = SafetyManager::with_journal_path(path.clone());
        let change = manager
            .record_change(
                "exec-status",
                "desengordurar_telemetria",
                "status",
                true,
                None,
                "applied",
            )
            .unwrap();

        manager.update_change_result(&change.id, "updated").unwrap();
        manager.changes[0].rollback_status = RollbackStatus::RollbackPartial;
        manager.persist_after_change("test_status").unwrap();

        let restored = SafetyManager::with_journal_path(path.clone());
        assert_eq!(restored.changes[0].result, "updated");
        assert_eq!(
            restored.changes[0].rollback_status,
            RollbackStatus::RollbackPartial
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn failed_journal_update_does_not_change_in_memory_result() {
        let path =
            std::env::temp_dir().join(format!("qboa-journal-update-failure-{}", unique_id()));
        fs::create_dir_all(&path).unwrap();
        let mut manager = SafetyManager::with_journal_path(path.clone());
        manager.changes.push(ChangeRecord {
            id: "change-update-failure".to_string(),
            execution_id: "exec-update-failure".to_string(),
            timestamp: iso_timestamp(),
            task_id: "test".to_string(),
            description: "update failure".to_string(),
            backup_location: None,
            reversible: false,
            rollback_status: RollbackStatus::Irreversible,
            result: "before".to_string(),
        });

        assert!(manager
            .update_change_result("change-update-failure", "after")
            .is_err());
        assert_eq!(manager.changes[0].result, "before");
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn rollback_session_attempts_all_changes_in_reverse_order() {
        let path = std::env::temp_dir().join(format!("qboa-journal-session-{}.json", unique_id()));
        let mut manager = SafetyManager::with_journal_path(path.clone());

        let first = manager
            .record_change(
                "exec-session",
                "desengordurar_telemetria",
                "first",
                true,
                None,
                "applied",
            )
            .unwrap();
        let second = manager
            .record_change(
                "exec-session",
                "desengordurar_telemetria",
                "second",
                true,
                None,
                "applied",
            )
            .unwrap();
        let third = manager
            .record_change(
                "exec-session",
                "desengordurar_telemetria",
                "third",
                true,
                None,
                "applied",
            )
            .unwrap();

        let error = manager.rollback_session(Some("exec-session")).unwrap_err();
        assert!(error.contains("third"));
        assert!(error.contains("second"));
        assert!(error.contains("first"));

        for change in [&first, &second, &third] {
            assert_eq!(
                manager.rollback_status(&change.id),
                Some(RollbackStatus::RollbackFailed)
            );
        }

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rollback_restores_each_legacy_snapshot_strategy() {
        let fixtures = [
            (
                "desengordurar_telemetria",
                r#"{"strategy":"telemetry","registry":[{"hive":"LocalMachine","path":"SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection","value_name":"AllowTelemetry","exists":false,"value_type":null,"value":null}],"services":[{"name":"DiagTrack","exists":true,"startup_type":"Auto","state":"Running"}],"scheduled_tasks":[{"task_path":"\\Microsoft\\Windows\\Application Experience\\","task_name":"Microsoft Compatibility Appraiser","exists":true,"enabled":true}],"features":[{"name":"Recall","exists":false,"state":null}]}"#,
            ),
            (
                "desengordurar_tarefas",
                r#"{"strategy":"diagnostic_tasks","registry":[],"services":[],"scheduled_tasks":[{"task_path":"\\Microsoft\\Windows\\Application Experience\\","task_name":"Microsoft Compatibility Appraiser","exists":true,"enabled":false}],"features":[]}"#,
            ),
        ];

        for (task_id, fixture) in fixtures {
            let snapshot: SystemSnapshot = serde_json::from_str(fixture).unwrap();
            let snapshot_path =
                std::env::temp_dir().join(format!("qboa-snapshot-{}.json", unique_id()));
            fs::write(&snapshot_path, serde_json::to_vec(&snapshot).unwrap()).unwrap();

            let journal_path =
                std::env::temp_dir().join(format!("qboa-rollback-{}.json", unique_id()));
            let mut manager = SafetyManager::with_journal_path(journal_path.clone());
            let change = manager
                .record_change(
                    "exec-legacy",
                    task_id,
                    "legacy snapshot",
                    true,
                    Some(snapshot_path.to_str().unwrap()),
                    "completed",
                )
                .unwrap();
            let restored = manager.rollback_task_with(&change.id, &|location, requested_task| {
                assert_eq!(requested_task, task_id);
                let restored_snapshot: SystemSnapshot =
                    serde_json::from_slice(&fs::read(location).unwrap()).unwrap();
                assert_eq!(restored_snapshot, snapshot);
                Ok("Rollback aplicado e validado.".to_string())
            });

            assert_eq!(restored.unwrap(), "Rollback aplicado e validado.");
            assert_eq!(
                manager.rollback_status(&change.id),
                Some(RollbackStatus::RolledBack)
            );

            fs::remove_file(snapshot_path).unwrap();
            fs::remove_file(journal_path).unwrap();
        }
    }

    #[test]
    fn rollback_session_does_not_restore_another_execution_batch() {
        let first_path = std::env::temp_dir().join(format!("qboa-batch-a-{}.json", unique_id()));
        let second_path = std::env::temp_dir().join(format!("qboa-batch-b-{}.json", unique_id()));
        fs::write(
            &first_path,
            r#"{"strategy":"telemetry","registry":[],"services":[],"scheduled_tasks":[],"features":[]}"#,
        )
        .unwrap();
        fs::write(
            &second_path,
            r#"{"strategy":"diagnostic_tasks","registry":[],"services":[],"scheduled_tasks":[],"features":[]}"#,
        )
        .unwrap();

        let journal_path =
            std::env::temp_dir().join(format!("qboa-batch-journal-{}.json", unique_id()));
        let mut manager = SafetyManager::with_journal_path(journal_path.clone());
        let first = manager
            .record_change(
                "batch-a",
                "desengordurar_telemetria",
                "first batch",
                true,
                Some(first_path.to_str().unwrap()),
                "completed",
            )
            .unwrap();
        let second = manager
            .record_change(
                "batch-b",
                "desengordurar_tarefas",
                "second batch",
                true,
                Some(second_path.to_str().unwrap()),
                "completed",
            )
            .unwrap();
        let restored_paths = std::cell::RefCell::new(Vec::new());

        let result = manager.rollback_session_with(Some("batch-a"), &|location, task_id| {
            restored_paths
                .borrow_mut()
                .push((location.to_string(), task_id.to_string()));
            Ok("restored".to_string())
        });

        assert_eq!(result.unwrap(), ["restored"]);
        assert_eq!(
            restored_paths.into_inner(),
            [(
                first_path.to_string_lossy().to_string(),
                "desengordurar_telemetria".to_string()
            )]
        );
        assert_eq!(
            manager.rollback_status(&first.id),
            Some(RollbackStatus::RolledBack)
        );
        assert_eq!(
            manager.rollback_status(&second.id),
            Some(RollbackStatus::RollbackAvailable)
        );

        fs::remove_file(first_path).unwrap();
        fs::remove_file(second_path).unwrap();
        fs::remove_file(journal_path).unwrap();
    }

    #[test]
    fn rollback_strategy_is_allowlisted() {
        assert!(known_strategy("desengordurar_telemetria").is_some());
        assert!(known_strategy("arbitrary-command").is_none())
    }
}
pub type SharedSafetyManager = Arc<Mutex<SafetyManager>>;
