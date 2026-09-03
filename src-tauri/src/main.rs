#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod safety;
mod task_registry;

use serde::Serialize;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};

use safety::SafetyManager;
use task_registry::{Task, TaskRegistry};

#[derive(Clone, Serialize, Debug)]
struct TaskResult {
    ok: bool,
    output: String,
    task_id: String,
    risk: String,
    restore_point_created: bool,
    change_id: Option<String>,
}

#[derive(Clone, Serialize, Debug)]
struct EventPayload {
    event: String,
    task_id: String,
    message: String,
    ok: bool,
    progress: Option<String>,
    risk: String,
}

static SESSION_STATE: std::sync::OnceLock<Mutex<Vec<String>>> = std::sync::OnceLock::new();
static SAFETY_STATE: std::sync::OnceLock<Arc<Mutex<SafetyManager>>> = std::sync::OnceLock::new();

fn session_state() -> &'static Mutex<Vec<String>> {
    SESSION_STATE.get_or_init(|| Mutex::new(Vec::new()))
}

fn safety_state() -> &'static Arc<Mutex<SafetyManager>> {
    SAFETY_STATE.get_or_init(|| Arc::new(Mutex::new(SafetyManager::new())))
}

fn emit_event<R: Runtime>(app: &AppHandle<R>, event: &str, task_id: &str, message: &str, ok: bool, progress: Option<&str>, risk: &str) {
    let payload = EventPayload {
        event: event.to_string(),
        task_id: task_id.to_string(),
        message: message.to_string(),
        ok,
        progress: progress.map(str::to_string),
        risk: risk.to_string(),
    };

    let _ = app.emit("qboa-event", payload);
}

fn forward_output<R: Read + Send + 'static>(stream: R, sender: mpsc::Sender<String>) {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            if let Ok(line) = line {
                let _ = sender.send(line);
            }
        }
    });
}

fn run_command(
    cmd: &str,
    args: &[&str],
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Falha ao executar '{cmd}': {e}"))?;
    let stdout = child.stdout.take().ok_or_else(|| format!("'{cmd}' não forneceu stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| format!("'{cmd}' não forneceu stderr"))?;
    let (sender, receiver) = mpsc::channel();

    forward_output(stdout, sender.clone());
    forward_output(stderr, sender.clone());
    drop(sender);

    let mut output = String::new();
    loop {
        while let Ok(line) = receiver.try_recv() {
            on_output(&line);
            output.push_str(&line);
            output.push('\n');
        }

        if is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Comando cancelado: {cmd}"));
        }

        if let Some(status) = child.try_wait().map_err(|e| format!("Falha ao aguardar '{cmd}': {e}"))? {
            while let Ok(line) = receiver.try_recv() {
                on_output(&line);
                output.push_str(&line);
                output.push('\n');
            }
            if status.success() {
                return Ok(output);
            }
            return Err(format!("Comando falhou ({cmd}):\n{output}"));
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn run_powershell(script: &str, on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    run_command(
        "powershell",
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
        on_output,
        is_cancelled,
    )
}

fn cleanup_standard(on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    run_powershell(
        r#"
$ErrorActionPreference = 'SilentlyContinue';
foreach ($path in @($env:TEMP, "$env:WINDIR\Temp")) {
    if (Test-Path $path) { Remove-Item -Path "$path\*" -Recurse -Force -ErrorAction SilentlyContinue }
}
$browserRoots = @(
    "$env:LOCALAPPDATA\Google\Chrome\User Data",
    "$env:LOCALAPPDATA\Microsoft\Edge\User Data",
    "$env:LOCALAPPDATA\BraveSoftware\Brave-Browser\User Data",
    "$env:LOCALAPPDATA\Vivaldi\User Data",
    "$env:APPDATA\Opera Software\Opera Stable",
    "$env:APPDATA\Mozilla\Firefox\Profiles"
)
$cacheNames = @('Cache', 'Cache2', 'Code Cache', 'GPUCache', 'StartupCache', 'shader-cache', 'Service Worker\CacheStorage')
foreach ($root in $browserRoots) {
    if (Test-Path $root) {
        Get-ChildItem -LiteralPath $root -Directory -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object {
            if ($cacheNames -contains $_.Name -or $_.FullName -match '\\Service Worker\\CacheStorage$') { Remove-Item -LiteralPath $_.FullName -Recurse -Force -ErrorAction SilentlyContinue }
        }
    }
}
Clear-RecycleBin -Force -ErrorAction SilentlyContinue;
Write-Output 'Limpeza padrão concluída: temporários, caches e lixeira.';
"#,
        on_output,
        is_cancelled,
    )
}

fn cleanup_heavy_temp(on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    run_powershell(
        r#"
$paths = @("$env:TEMP", "$env:WINDIR\Temp", "$env:WINDIR\Minidump", "$env:WINDIR\LiveKernelReports");
foreach ($p in $paths) { if (Test-Path $p) { Get-ChildItem $p -Recurse -Force -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue; } }
Clear-RecycleBin -Force -ErrorAction SilentlyContinue;
Write-Output 'Limpeza pesada concluída.';
"#,
        on_output,
        is_cancelled,
    )
}

fn disable_telemetry(on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    run_powershell(
        r#"
$ErrorActionPreference = 'SilentlyContinue';
function Set-Dword { param([string]$Path, [string]$Name, [int]$Value) New-Item -Path $Path -Force -ErrorAction SilentlyContinue | Out-Null; New-ItemProperty -Path $Path -Name $Name -PropertyType DWord -Value $Value -Force -ErrorAction SilentlyContinue | Out-Null }
$telemetryKeys = @(
  @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'AllowTelemetry', 0),
  @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'AllowDeviceNameInTelemetry', 0),
  @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection', 'AllowTelemetry', 0),
  @('HKLM:\SOFTWARE\Wow6432Node\Microsoft\Windows\CurrentVersion\Policies\DataCollection', 'AllowTelemetry', 0),
  @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'DoNotShowFeedbackNotifications', 1),
  @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'DisableTelemetryOptInChangeNotification', 1)
)
foreach ($key in $telemetryKeys) { Set-Dword -Path $key[0] -Name $key[1] -Value $key[2] }
foreach ($serviceName in @('DiagTrack', 'dmwappushservice')) {
  $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
  if ($service) { Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue; Set-Service -Name $serviceName -StartupType Disabled -ErrorAction SilentlyContinue }
}
$tasks = @(
  '\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser',
  '\Microsoft\Windows\Application Experience\ProgramDataUpdater',
  '\Microsoft\Windows\Autochk\Proxy',
  '\Microsoft\Windows\Customer Experience Improvement Program\Consolidator',
  '\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip',
  '\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector',
  '\Microsoft\Windows\Feedback\Siuf\DmClient',
  '\Microsoft\Windows\Feedback\Siuf\DmClientOnScenarioDownload'
)
foreach ($task in $tasks) { Disable-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue | Out-Null }
Write-Output 'Telemetria e diagnósticos ajustados.';
"#,
        on_output,
        is_cancelled,
    )
}

fn disable_diagnostic_tasks(on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    run_powershell(
        r#"Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskPath -match 'Application Experience|Customer Experience Improvement Program|Autochk\\Proxy' } | Disable-ScheduledTask -ErrorAction SilentlyContinue; Write-Output 'Tarefas de diagnóstico desativadas.';"#,
        on_output,
        is_cancelled,
    )
}

fn run_system_command(command_name: &str, args: &[&str], on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    run_command(command_name, args, on_output, is_cancelled)
}

fn resolve_task(task_id: &str) -> Option<Task> {
    let registry = TaskRegistry::new();
    registry.resolve(task_id).cloned()
}

fn execute_task(task: &Task, on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    match task.id.as_str() {
        "limpeza_leve" => cleanup_standard(on_output, is_cancelled),
        "limpeza_media" => {
            let part_1 = cleanup_standard(on_output, is_cancelled)?;
            let part_2 = run_system_command("cleanmgr", &["/sagerun:1"], on_output, is_cancelled)?;
            Ok(format!("{part_1}\n{part_2}"))
        }
        "limpeza_pesada" => {
            let part_1 = cleanup_standard(on_output, is_cancelled)?;
            let part_2 = cleanup_heavy_temp(on_output, is_cancelled)?;
            Ok(format!("{part_1}\n{part_2}"))
        }
        "desengordurar_telemetria" => disable_telemetry(on_output, is_cancelled),
        "desengordurar_tarefas" => disable_diagnostic_tasks(on_output, is_cancelled),
        "diagnostico_dism_scan" => run_system_command("dism", &["/Online", "/Cleanup-Image", "/ScanHealth"], on_output, is_cancelled),
        "diagnostico_dism_restore" => run_system_command("dism", &["/Online", "/Cleanup-Image", "/RestoreHealth"], on_output, is_cancelled),
        "diagnostico_sfc" => run_system_command("sfc", &["/scannow"], on_output, is_cancelled),
        "diagnostico_informacoes" => run_system_command("systeminfo", &[], on_output, is_cancelled),
        _ => Err(format!("Task não implementada: {}", task.id)),
    }
}

fn task_risk_label(task: &Task) -> &'static str {
    task.risk.as_str()
}

#[tauri::command]
async fn get_tasks() -> Result<Vec<Task>, String> {
    Ok(TaskRegistry::new().list())
}

#[tauri::command]
async fn run_task(app: AppHandle, task_id: String) -> Result<TaskResult, String> {
    let registry = TaskRegistry::new();
    let task = registry.resolve(&task_id).cloned().ok_or_else(|| format!("Task desconhecida: {task_id}"))?;
    let task_id_for_spawn = task.id.clone();
    let app_for_spawn = app.clone();
    let task_name = task.name.clone();
    let task_risk = task_risk_label(&task).to_string();
    let task_for_spawn = task.clone();
    let task_risk_for_spawn = task_risk.clone();

    let execution = safety_state().lock().unwrap().register_execution(&task_id_for_spawn);
    let execution_cancelled = execution.cancelled;

    tauri::async_runtime::spawn(async move {
        emit_event(&app_for_spawn, "task-started", &task_id_for_spawn, &task_name, true, None, &task_risk_for_spawn);

        if execution_cancelled {
            emit_event(&app_for_spawn, "task-cancelled", &task_id_for_spawn, "Tarefa cancelada antes da execução.", false, Some("cancelled"), &task_risk_for_spawn);
            return;
        }

        let restore_point = if task_for_spawn.creates_restore_point || task_for_spawn.risk.requires_restore_point() {
            match safety_state().lock().unwrap().create_restore_point(&format!("Qboa Digital — Antes da tarefa: {}", task_for_spawn.name)) {
                Ok(point) => {
                    emit_event(&app_for_spawn, "restore-point-created", &task_id_for_spawn, &point, true, Some("100%"), &task_risk_for_spawn);
                    Some(point)
                }
                Err(err) => {
                    emit_event(&app_for_spawn, "restore-point-error", &task_id_for_spawn, &err, false, None, &task_risk_for_spawn);
                    None
                }
            }
        } else {
            None
        };

        emit_event(&app_for_spawn, "task-progress", &task_id_for_spawn, "Preparando execução e monitorando riscos.", true, Some("30%"), &task_risk_for_spawn);

        let app_for_output = app_for_spawn.clone();
        let task_id_for_output = task_id_for_spawn.clone();
        let task_risk_for_output = task_risk_for_spawn.clone();
        let mut on_output = move |line: &str| {
            emit_event(&app_for_output, "task-output", &task_id_for_output, line, true, Some("running"), &task_risk_for_output);
        };
        let task_id_for_cancel = task_id_for_spawn.clone();
        let is_cancelled = move || safety_state().lock().unwrap().is_cancelled(&task_id_for_cancel);
        let execution = execute_task(&task_for_spawn, &mut on_output, &is_cancelled);
        match execution {
            Ok(output) => {
                let serialized = serde_json::to_string(&output).unwrap_or_else(|_| output.clone());
                let change = safety_state().lock().unwrap().record_change(
                    &task_id_for_spawn,
                    &task_name,
                    task.reversible,
                    restore_point.as_deref(),
                    "aplicada",
                );

                session_state().lock().unwrap().push(change.id.clone());
                safety_state().lock().unwrap().finish_execution(&task_id_for_spawn, "completed");
                emit_event(&app_for_spawn, "task-output", &task_id_for_spawn, &output, true, Some("100%"), &task_risk_for_spawn);
                emit_event(&app_for_spawn, "task-completed", &task_id_for_spawn, &format!("Tarefa concluída: {}", task_name), true, Some("100%"), &task_risk_for_spawn);

                if task.reversible {
                    let _ = app_for_spawn.emit("qboa-backup-created", serde_json::json!({"task_id": task_id_for_spawn, "backup": serialized, "change_id": change.id }));
                }
            }
            Err(err) => {
                let cancelled = safety_state().lock().unwrap().is_cancelled(&task_id_for_spawn);
                safety_state().lock().unwrap().finish_execution(&task_id_for_spawn, if cancelled { "cancelled" } else { "failed" });
                emit_event(&app_for_spawn, if cancelled { "task-cancelled" } else { "task-error" }, &task_id_for_spawn, &err, false, None, &task_risk_for_spawn);
            }
        }
    });

    Ok(TaskResult {
        ok: true,
        output: format!("Task iniciada: {}", task.name),
        task_id: task.id,
        risk: task_risk,
        restore_point_created: task.creates_restore_point || task.risk.requires_restore_point(),
        change_id: None,
    })
}

#[tauri::command]
async fn cancel_task(task_id: String) -> Result<String, String> {
    let mut safety = safety_state().lock().unwrap();
    if safety.cancel_execution(&task_id) {
        Ok(format!("Tarefa marcada como cancelada: {task_id}"))
    } else {
        Err(format!("Tarefa não encontrada para cancelamento: {task_id}"))
    }
}

#[tauri::command]
async fn run_preset(app: AppHandle, preset_id: String) -> Result<Vec<String>, String> {
    let registry = TaskRegistry::new();
    let tasks = registry.preset_tasks(&preset_id);
    let ids: Vec<String> = tasks.iter().map(|task| task.id.clone()).collect();
    for task in tasks {
        let _ = run_task(app.clone(), task.id.clone()).await;
    }
    Ok(ids)
}

#[tauri::command]
async fn create_restore_point() -> Result<String, String> {
    let mut safety = safety_state().lock().unwrap();
    safety.create_restore_point("Qboa Digital — Proteção do sistema")
}

#[tauri::command]
async fn rollback_session() -> Result<Vec<String>, String> {
    let mut safety = safety_state().lock().unwrap();
    safety.rollback_session()
}

#[tauri::command]
async fn get_session_history() -> Result<Vec<safety::ChangeRecord>, String> {
    Ok(safety_state().lock().unwrap().journal())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tasks_are_resolved() {
        assert!(resolve_task("limpeza_leve").is_some());
        assert!(resolve_task("desengordurar_telemetria").is_some());
        assert!(resolve_task("diagnostico_sfc").is_some());
    }

    #[test]
    fn unknown_tasks_are_rejected() {
        let registry = TaskRegistry::new();
        assert!(registry.resolve("task_inexistente").is_none());
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_tasks,
            run_task,
            cancel_task,
            run_preset,
            create_restore_point,
            rollback_session,
            get_session_history
        ])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o Qboa Digital");
}
