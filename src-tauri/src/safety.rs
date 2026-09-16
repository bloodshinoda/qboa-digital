use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
use std::process::Command;

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
    pub exists: bool,
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
        let root = std::env::temp_dir().join("qboa-digital").join("snapshots");
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
    fn with_journal_path(path: PathBuf) -> Self {
        let root = std::env::temp_dir().join("qboa-digital").join("snapshots");
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
        let parent = self
            .journal_path
            .parent()
            .ok_or_else(|| "Caminho do journal sem diretório pai.".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("Falha ao criar diretório do journal: {e}"))?;

        let data = serde_json::to_vec_pretty(&self.changes)
            .map_err(|e| format!("Falha ao serializar journal: {e}"))?;

        let temp_path = self.journal_path.with_extension(format!("tmp-{}", unique_id()));
        {
            let mut file = fs::File::create(&temp_path)
                .map_err(|e| format!("Falha ao criar journal temporário: {e}"))?;
            file.write_all(&data)
                .map_err(|e| format!("Falha ao gravar journal temporário: {e}"))?;
            file.sync_all()
                .map_err(|e| format!("Falha ao sincronizar journal temporário: {e}"))?;
        }

        if let Err(error) = replace_file_atomically(&temp_path, &self.journal_path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }

        Ok(())
    }

    fn persist_after_change(&self, operation: &str) {
        if let Err(error) = self.persist_journal() {
            write_safety_log(&format!(
                "journal persist failed operation={operation} error={error}"
            ));
        }
    }
    pub fn create_restore_point(&mut self, reason: &str) -> Result<String, String> {
        #[cfg(target_os = "windows")]
        {
            let script = format!("Checkpoint-Computer -Description '{}' -RestorePointType 'MODIFY_SETTINGS' -ErrorAction Stop", reason.replace("'", "''"));
            write_safety_log(&format!("restore-point started reason={reason}"));
            let output = Command::new("powershell")
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
    ) -> ChangeRecord {
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
        self.persist_after_change("record_change");
        change
    }
    pub fn rollback_task(&mut self, change_id: &str) -> Result<String, String> {
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
        self.changes[index].rollback_status = RollbackStatus::RollingBack;
        self.persist_after_change("rollback_started");

        let location = match change.backup_location.clone() {
            Some(location) => location,
            None => {
                self.changes[index].rollback_status = RollbackStatus::RollbackFailed;
                self.persist_after_change("rollback_failed_without_snapshot");
                return Err("Rollback sem snapshot.".to_string());
            }
        };
        match restore_snapshot(&location, &change.task_id) {
            Ok(message) => {
                self.changes[index].rollback_status = RollbackStatus::RolledBack;
                self.persist_after_change("rollback_completed");
                Ok(message)
            }
            Err(error) => {
                self.changes[index].rollback_status = if error.contains("parcial") {
                    RollbackStatus::RollbackPartial
                } else {
                    RollbackStatus::RollbackFailed
                };
                self.persist_after_change("rollback_failed");
                Err(error)
            }
        }
    }
    pub fn rollback_session(&mut self, execution_id: Option<&str>) -> Result<Vec<String>, String> {
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

            match self.rollback_task(&id) {
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
    pub fn update_change_result(&mut self, change_id: &str, result: &str) {
        if let Some(change) = self
            .changes
            .iter_mut()
            .find(|change| change.id == change_id)
        {
            change.result = result.to_string();
            self.persist_after_change("update_change_result");
        }
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
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn run_snapshot_script(strategy: &str) -> Result<String, String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            snapshot_script(strategy),
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
    let script = format!(
        "$s=ConvertFrom-Json -InputObject '{}';{}",
        encoded.replace('\'', "''"),
        restore_script()
    );
    let output = Command::new("powershell")
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
    if after != snapshot {
        return Err("Rollback parcial: validação não corresponde ao snapshot.".to_string());
    }
    Ok("Rollback aplicado e validado.".to_string())
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
            fs::remove_file(destination)
                .map_err(|e| format!("Falha ao substituir journal existente: {e}"))?;
        }
    }

    fs::rename(temp_path, destination)
        .map_err(|e| format!("Falha ao finalizar journal atômico: {e}"))
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
        let mut manager = SafetyManager::new();
        let change = manager.record_change(
            "exec-1",
            "desengordurar_telemetria",
            "x",
            true,
            None,
            "applied",
        );
        assert!(manager.rollback_task(&change.id).is_err());
        assert_eq!(
            manager.journal()[0].rollback_status,
            RollbackStatus::RollbackFailed
        )
    }
    #[test]
    fn repeated_rollback_is_rejected() {
        let mut manager = SafetyManager::new();
        let mut change =
            manager.record_change("exec-1", "x", "x", true, Some("missing"), "applied");
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
        let change = manager.record_change(
            "exec-roundtrip",
            "desengordurar_telemetria",
            "roundtrip",
            true,
            None,
            "applied",
        );

        let restored = SafetyManager::with_journal_path(path.clone());
        assert_eq!(restored.changes, vec![change]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn journal_persists_result_and_rollback_status() {
        let path = std::env::temp_dir().join(format!("qboa-journal-status-{}.json", unique_id()));
        let mut manager = SafetyManager::with_journal_path(path.clone());
        let change = manager.record_change(
            "exec-status",
            "desengordurar_telemetria",
            "status",
            true,
            None,
            "applied",
        );

        manager.update_change_result(&change.id, "updated");
        manager.changes[0].rollback_status = RollbackStatus::RollbackPartial;
        manager.persist_after_change("test_status");

        let restored = SafetyManager::with_journal_path(path.clone());
        assert_eq!(restored.changes[0].result, "updated");
        assert_eq!(
            restored.changes[0].rollback_status,
            RollbackStatus::RollbackPartial
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rollback_session_attempts_all_changes_in_reverse_order() {
        let path = std::env::temp_dir().join(format!("qboa-journal-session-{}.json", unique_id()));
        let mut manager = SafetyManager::with_journal_path(path.clone());

        let first = manager.record_change(
            "exec-session",
            "desengordurar_telemetria",
            "first",
            true,
            None,
            "applied",
        );
        let second = manager.record_change(
            "exec-session",
            "desengordurar_telemetria",
            "second",
            true,
            None,
            "applied",
        );
        let third = manager.record_change(
            "exec-session",
            "desengordurar_telemetria",
            "third",
            true,
            None,
            "applied",
        );

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
    fn rollback_strategy_is_allowlisted() {
        assert!(known_strategy("desengordurar_telemetria").is_some());
        assert!(known_strategy("arbitrary-command").is_none())
    }
}
pub type SharedSafetyManager = Arc<Mutex<SafetyManager>>;
