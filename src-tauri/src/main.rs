#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod safety;
mod task_registry;

use serde::Serialize;
use std::io::{BufRead, BufReader, Read};
use std::fs::OpenOptions;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};

use safety::SafetyManager;
use task_registry::{Task, TaskRegistry};

#[derive(Clone, Serialize, Debug)]
struct TaskResult {
    ok: bool,
    output: String,
    task_id: String,
    execution_id: String,
    risk: String,
    restore_point_created: bool,
    change_id: Option<String>,
}

#[derive(Clone, Serialize, Debug)]
struct EventPayload {
    event: String,
    task_id: String,
    execution_id: String,
    message: String,
    ok: bool,
    progress: Option<String>,
    risk: String,
}

static SAFETY_STATE: std::sync::OnceLock<Arc<Mutex<SafetyManager>>> = std::sync::OnceLock::new();

fn safety_state() -> &'static Arc<Mutex<SafetyManager>> {
    SAFETY_STATE.get_or_init(|| Arc::new(Mutex::new(SafetyManager::new())))
}

fn emit_event<R: Runtime>(
    app: &AppHandle<R>,
    event: &str,
    task_id: &str,
    execution_id: &str,
    message: &str,
    ok: bool,
    progress: Option<&str>,
    risk: &str,
) {
    let payload = EventPayload {
        event: event.to_string(),
        task_id: task_id.to_string(),
        execution_id: execution_id.to_string(),
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

fn execution_log_path() -> std::path::PathBuf {
    let root = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Qboa Digital")
        .join("logs");
    let _ = std::fs::create_dir_all(&root);
    root.join("execution.log")
}

fn write_execution_log(execution_id: &str, message: &str) {
    let path = execution_log_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{execution_id}] {message}");
    }
}

fn run_command(
    cmd: &str,
    args: &[&str],
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Falha ao executar '{cmd}': {e}"))?;
    write_execution_log(
        execution_id,
        &format!("started pid={} command={} args={args:?}", child.id(), cmd),
    );
    safety_state()
        .lock()
        .unwrap()
        .set_process_id(execution_id, child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("'{cmd}' não forneceu stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("'{cmd}' não forneceu stderr"))?;
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
            match terminate_process_tree(&mut child) {
                Ok(()) => return Err(format!("Comando cancelado: {cmd}")),
                Err(error) => {
                    return Err(format!("Cancelamento não confirmado para {cmd}: {error}"))
                }
            }
        }

        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("Falha ao aguardar '{cmd}': {e}"))?
        {
            while let Ok(line) = receiver.recv_timeout(Duration::from_millis(100)) {
                on_output(&line);
                output.push_str(&line);
                output.push('\n');
            }
            write_execution_log(
                execution_id,
                &format!("finished pid={} status={status} output_bytes={}", child.id(), output.len()),
            );
            if status.success() {
                return Ok(output);
            }
            write_execution_log(execution_id, &format!("failed command={cmd} output={output}"));
            return Err(format!("Comando falhou ({cmd}):\n{output}"));
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn terminate_process_tree(child: &mut std::process::Child) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .status()
            .map_err(|error| format!("Falha ao encerrar a árvore de processos: {error}"))?;
        if !status.success() {
            return Err(
                "Não foi possível confirmar o encerramento da árvore de processos.".to_string(),
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        child
            .kill()
            .map_err(|error| format!("Falha ao cancelar processo: {error}"))?;
    }
    child
        .wait()
        .map_err(|error| format!("Falha ao aguardar processo cancelado: {error}"))?;
    Ok(())
}

fn run_powershell(
    script: &str,
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    run_command(
        "powershell",
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
        execution_id,
        on_output,
        is_cancelled,
    )
}

fn cleanup_standard(
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
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
foreach ($key in @(
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\RecentDocs',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\OpenSavePidlMRU',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\LastVisitedPidlMRU',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Map Network Drive MRU',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery'
)) { Remove-Item -Path $key -Recurse -Force -ErrorAction SilentlyContinue }
Remove-Item -Path "$env:APPDATA\Microsoft\Windows\Recent\*" -Recurse -Force -Confirm:$false -ErrorAction SilentlyContinue;
Write-Output 'Limpeza padrão concluída: temporários, caches, MRUs e lixeira.';
"#,
        execution_id,
        on_output,
        is_cancelled,
    )
}

fn cleanup_heavy_temp(
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    run_powershell(
        r#"
$paths = @("$env:TEMP", "$env:WINDIR\Temp", "$env:WINDIR\Minidump", "$env:WINDIR\LiveKernelReports");
foreach ($p in $paths) { if (Test-Path $p) { Get-ChildItem $p -Recurse -Force -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue; } }
Clear-RecycleBin -Force -ErrorAction SilentlyContinue;
Write-Output 'Limpeza pesada concluída.';
"#,
        execution_id,
        on_output,
        is_cancelled,
    )
}

fn cleanup_windows_update(
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    run_powershell(
        r#"
$ErrorActionPreference = 'SilentlyContinue';
$path = "$env:WINDIR\SoftwareDistribution\Download";
if (Test-Path $path) { Remove-Item -Path "$path\*.tmp" -Force -ErrorAction SilentlyContinue }
Write-Output 'Cache temporário do Windows Update processado.';
"#,
        execution_id,
        on_output,
        is_cancelled,
    )
}

fn disable_telemetry(
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
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
    @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'DisableTelemetryOptInChangeNotification', 1),
    @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'DisableEnterpriseAuthProxy', 1),
    @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI', 'DisableAIDataAnalysis', 1),
    @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI', 'TurnOffWindowsCopilot', 1),
    @('HKCU:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI', 'DisableAIDataAnalysis', 1),
    @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'EnableRecallOnDevice', 0),
    @('HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'EnableRecallOnDevice', 0)
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
    '\Microsoft\Windows\Customer Experience Improvement Program\KernelCeipTask',
  '\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector',
  '\Microsoft\Windows\Feedback\Siuf\DmClient',
  '\Microsoft\Windows\Feedback\Siuf\DmClientOnScenarioDownload'
)
foreach ($task in $tasks) { Disable-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue | Out-Null }
 $feature = Get-WindowsOptionalFeature -Online -FeatureName 'Recall' -ErrorAction SilentlyContinue;
if ($feature -and $feature.State -eq 'Enabled') { Disable-WindowsOptionalFeature -Online -FeatureName 'Recall' -NoRestart -ErrorAction SilentlyContinue | Out-Null }
Write-Output 'Telemetria e diagnósticos ajustados.';
"#,
        execution_id,
        on_output,
        is_cancelled,
    )
}

fn disable_diagnostic_tasks(
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    run_powershell(
        r#"foreach ($task in @('\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser','\Microsoft\Windows\Application Experience\ProgramDataUpdater','\Microsoft\Windows\Autochk\Proxy')) { $path = [System.IO.Path]::GetDirectoryName($task) + '\'; $name = [System.IO.Path]::GetFileName($task); Disable-ScheduledTask -TaskPath $path -TaskName $name -ErrorAction Stop | Out-Null }; Write-Output 'Tarefas de diagnóstico desativadas.';"#,
        execution_id,
        on_output,
        is_cancelled,
    )
}

fn run_system_command(
    command_name: &str,
    args: &[&str],
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    run_command(command_name, args, execution_id, on_output, is_cancelled)
}

#[cfg(target_os = "windows")]
fn schedule_shutdown() -> Result<(), String> {
    Command::new("shutdown")
        .args(["/s", "/t", "30", "/c", "Qboa Digital concluiu a tarefa."])
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Não foi possível agendar o desligamento: {error}"))
}

#[cfg(not(target_os = "windows"))]
fn schedule_shutdown() -> Result<(), String> {
    Err("Desligamento automático só está disponível no Windows.".to_string())
}

fn resolve_task(task_id: &str) -> Option<Task> {
    let registry = TaskRegistry::new();
    registry.resolve(task_id).cloned()
}

fn execute_task(
    task: &Task,
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    match task.id.as_str() {
        "limpeza_leve" => cleanup_standard(execution_id, on_output, is_cancelled),
        "limpeza_media" => {
            let part_1 = cleanup_standard(execution_id, on_output, is_cancelled)?;
            let part_2 = run_system_command(
                "cleanmgr",
                &["/sagerun:1"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            let part_3 = cleanup_windows_update(execution_id, on_output, is_cancelled)?;
            let part_4 = run_system_command(
                "dism",
                &["/Online", "/Cleanup-Image", "/ScanHealth"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            Ok(format!("{part_1}\n{part_2}\n{part_3}\n{part_4}"))
        }
        "limpeza_pesada" => {
            let part_1 = cleanup_standard(execution_id, on_output, is_cancelled)?;
            let part_2 = cleanup_heavy_temp(execution_id, on_output, is_cancelled)?;
            let part_3 = cleanup_windows_update(execution_id, on_output, is_cancelled)?;
            let part_4 = run_system_command(
                "dism",
                &["/Online", "/Cleanup-Image", "/ScanHealth"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            let part_5 = run_system_command(
                "dism",
                &["/Online", "/Cleanup-Image", "/StartComponentCleanup"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            let part_6 = run_system_command(
                "dism",
                &["/Online", "/Cleanup-Image", "/RestoreHealth"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            let part_7 =
                run_system_command("sfc", &["/scannow"], execution_id, on_output, is_cancelled)?;
            Ok(format!(
                "{part_1}\n{part_2}\n{part_3}\n{part_4}\n{part_5}\n{part_6}\n{part_7}"
            ))
        }
        "desengordurar_telemetria" => disable_telemetry(execution_id, on_output, is_cancelled),
        "desengordurar_tarefas" => disable_diagnostic_tasks(execution_id, on_output, is_cancelled),
        "limpeza_windows_update" => cleanup_windows_update(execution_id, on_output, is_cancelled),
        "diagnostico_dism_scan" => run_system_command(
            "dism",
            &["/Online", "/Cleanup-Image", "/ScanHealth"],
            execution_id,
            on_output,
            is_cancelled,
        ),
        "diagnostico_dism_restore" => run_system_command(
            "dism",
            &["/Online", "/Cleanup-Image", "/RestoreHealth"],
            execution_id,
            on_output,
            is_cancelled,
        ),
        "diagnostico_sfc" => {
            run_system_command("sfc", &["/scannow"], execution_id, on_output, is_cancelled)
        }
        "diagnostico_informacoes" => {
            run_system_command("systeminfo", &[], execution_id, on_output, is_cancelled)
        }
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
async fn run_task(
    app: AppHandle,
    task_id: String,
    shutdown_on_complete: bool,
) -> Result<TaskResult, String> {
    let registry = TaskRegistry::new();
    let task = registry
        .resolve(&task_id)
        .cloned()
        .ok_or_else(|| format!("Task desconhecida: {task_id}"))?;
    let execution = start_task(app, task.clone(), shutdown_on_complete);
    let result = task_result(&task, &execution.execution_id);
    Ok(result)
}

fn task_result(task: &Task, execution_id: &str) -> TaskResult {
    let task_risk = task_risk_label(task).to_string();
    TaskResult {
        ok: true,
        output: format!("Task iniciada: {}", task.name),
        task_id: task.id.clone(),
        execution_id: execution_id.to_string(),
        risk: task_risk,
        restore_point_created: task.creates_restore_point || task.risk.requires_restore_point(),
        change_id: None,
    }
}

fn start_task(app: AppHandle, task: Task, shutdown_on_complete: bool) -> safety::ExecutionHandle {
    let task_id_for_spawn = task.id.clone();
    let app_for_spawn = app.clone();
    let task_name = task.name.clone();
    let task_risk = task_risk_label(&task).to_string();
    let task_for_spawn = task.clone();
    let task_risk_for_spawn = task_risk.clone();

    let execution = safety_state()
        .lock()
        .unwrap()
        .register_execution(&task_id_for_spawn);
    let execution_id = execution.execution_id.clone();
    let execution_id_for_spawn = execution_id.clone();

    tauri::async_runtime::spawn(async move {
        write_execution_log(&execution_id_for_spawn, &format!("task-started task_id={task_id_for_spawn}"));
        emit_event(
            &app_for_spawn,
            "task-started",
            &task_id_for_spawn,
            &execution_id_for_spawn,
            &task_name,
            true,
            None,
            &task_risk_for_spawn,
        );

        let snapshot_location = if task_for_spawn.reversible {
            write_execution_log(&execution_id_for_spawn, "snapshot-started");
            match safety_state()
                .lock()
                .unwrap()
                .capture_snapshot(&task_for_spawn.id)
            {
                Ok(location) => Some(location),
                Err(error) => {
                    write_execution_log(&execution_id_for_spawn, &format!("snapshot-failed error={error}"));
                    safety_state()
                        .lock()
                        .unwrap()
                        .finish_execution(&execution_id_for_spawn, "failed");
                    emit_event(
                        &app_for_spawn,
                        "task-error",
                        &task_id_for_spawn,
                        &execution_id_for_spawn,
                        &error,
                        false,
                        None,
                        &task_risk_for_spawn,
                    );
                    return;
                }
            }
        } else {
            None
        };

        if task_for_spawn.creates_restore_point || task_for_spawn.risk.requires_restore_point() {
            write_execution_log(&execution_id_for_spawn, "restore-point-phase-started");
            match safety_state()
                .lock()
                .unwrap()
                .create_restore_point(&format!(
                    "Qboa Digital — Antes da tarefa: {}",
                    task_for_spawn.name
                )) {
                Ok(point) => {
                    write_execution_log(&execution_id_for_spawn, "restore-point-phase-completed");
                    emit_event(
                        &app_for_spawn,
                        "restore-point-created",
                        &task_id_for_spawn,
                        &execution_id_for_spawn,
                        &point,
                        true,
                        Some("100%"),
                        &task_risk_for_spawn,
                    );
                    Some(point)
                }
                Err(err) => {
                    write_execution_log(&execution_id_for_spawn, &format!("restore-point-phase-failed error={err}"));
                    emit_event(
                        &app_for_spawn,
                        "restore-point-error",
                        &task_id_for_spawn,
                        &execution_id_for_spawn,
                        &err,
                        false,
                        None,
                        &task_risk_for_spawn,
                    );
                    None
                }
            }
        } else {
            None
        };

        emit_event(
            &app_for_spawn,
            "task-progress",
            &task_id_for_spawn,
            &execution_id_for_spawn,
            "Preparando execução e monitorando riscos.",
            true,
            Some("30%"),
            &task_risk_for_spawn,
        );

        let app_for_output = app_for_spawn.clone();
        let task_id_for_output = task_id_for_spawn.clone();
        let execution_id_for_output = execution_id_for_spawn.clone();
        let task_risk_for_output = task_risk_for_spawn.clone();
        let mut on_output = move |line: &str| {
            emit_event(
                &app_for_output,
                "task-output",
                &task_id_for_output,
                &execution_id_for_output,
                line,
                true,
                Some("running"),
                &task_risk_for_output,
            );
        };
        let execution_id_for_cancel = execution_id_for_spawn.clone();
        let is_cancelled = move || {
            safety_state()
                .lock()
                .unwrap()
                .is_cancelled(&execution_id_for_cancel)
        };
        let execution = execute_task(
            &task_for_spawn,
            &execution_id_for_spawn,
            &mut on_output,
            &is_cancelled,
        );
        match execution {
            Ok(output) => {
                write_execution_log(&execution_id_for_spawn, "task-executor-completed");
                let change = safety_state().lock().unwrap().record_change(
                    &execution_id_for_spawn,
                    &task_id_for_spawn,
                    &task_name,
                    task.reversible,
                    snapshot_location.as_deref(),
                    "completed",
                );

                safety_state()
                    .lock()
                    .unwrap()
                    .finish_execution(&execution_id_for_spawn, "completed");
                emit_event(
                    &app_for_spawn,
                    "task-output",
                    &task_id_for_spawn,
                    &execution_id_for_spawn,
                    &output,
                    true,
                    Some("100%"),
                    &task_risk_for_spawn,
                );
                emit_event(
                    &app_for_spawn,
                    "task-completed",
                    &task_id_for_spawn,
                    &execution_id_for_spawn,
                    &format!("Tarefa concluída: {}", task_name),
                    true,
                    Some("100%"),
                    &task_risk_for_spawn,
                );

                if shutdown_on_complete {
                    match schedule_shutdown() {
                        Ok(()) => emit_event(
                            &app_for_spawn,
                            "shutdown-scheduled",
                            &task_id_for_spawn,
                            &execution_id_for_spawn,
                            "Desligamento agendado para daqui a 30 segundos.",
                            true,
                            Some("100%"),
                            &task_risk_for_spawn,
                        ),
                        Err(error) => emit_event(
                            &app_for_spawn,
                            "shutdown-error",
                            &task_id_for_spawn,
                            &execution_id_for_spawn,
                            &error,
                            false,
                            None,
                            &task_risk_for_spawn,
                        ),
                    }
                }

                let _ = change;
            }
            Err(err) => {
                write_execution_log(&execution_id_for_spawn, &format!("task-executor-failed error={err}"));
                let cancelled = safety_state()
                    .lock()
                    .unwrap()
                    .is_cancelled(&execution_id_for_spawn);
                if cancelled && !err.starts_with("Cancelamento não confirmado") {
                    safety_state()
                        .lock()
                        .unwrap()
                        .finish_execution(&execution_id_for_spawn, "cancelled");
                    emit_event(
                        &app_for_spawn,
                        "task-cancelled",
                        &task_id_for_spawn,
                        &execution_id_for_spawn,
                        &err,
                        false,
                        None,
                        &task_risk_for_spawn,
                    );
                } else {
                    let failed_change = safety_state().lock().unwrap().record_change(
                        &execution_id_for_spawn,
                        &task_id_for_spawn,
                        &task_name,
                        task.reversible,
                        snapshot_location.as_deref(),
                        "failed",
                    );
                    let final_status = if task.rollback_on_failure && task.reversible {
                        let rollback_result = safety_state()
                            .lock()
                            .unwrap()
                            .rollback_task(&failed_change.id);
                        match rollback_result {
                            Ok(_) => "failed_and_rolled_back",
                            Err(_) => match safety_state()
                                .lock()
                                .unwrap()
                                .rollback_status(&failed_change.id)
                            {
                                Some(safety::RollbackStatus::RollbackPartial) => {
                                    "failed_rollback_partial"
                                }
                                _ => "failed_rollback_error",
                            },
                        }
                    } else {
                        "failed"
                    };
                    safety_state()
                        .lock()
                        .unwrap()
                        .update_change_result(&failed_change.id, final_status);
                    safety_state()
                        .lock()
                        .unwrap()
                        .finish_execution(&execution_id_for_spawn, final_status);
                    emit_event(
                        &app_for_spawn,
                        "task-error",
                        &task_id_for_spawn,
                        &execution_id_for_spawn,
                        &format!("{err} ({final_status})"),
                        false,
                        None,
                        &task_risk_for_spawn,
                    );
                }
            }
        }
    });
    execution
}

#[tauri::command]
async fn cancel_task(task_id: String, execution_id: Option<String>) -> Result<String, String> {
    let mut safety = safety_state().lock().unwrap();
    let execution_id = execution_id
        .or_else(|| safety.execution_for_task(&task_id))
        .ok_or_else(|| {
            format!("Execution ID obrigatório quando há mais de uma execução: {task_id}")
        })?;
    if safety.cancel_execution(&execution_id) {
        Ok(format!(
            "Tarefa marcada como cancelando: {task_id} ({execution_id})"
        ))
    } else {
        Err(format!(
            "Tarefa não encontrada para cancelamento: {task_id}"
        ))
    }
}

#[tauri::command]
async fn run_preset(app: AppHandle, preset_id: String) -> Result<Vec<String>, String> {
    let registry = TaskRegistry::new();
    let tasks: Vec<Task> = registry
        .preset_tasks(&preset_id)
        .into_iter()
        .cloned()
        .collect();
    let ids: Vec<String> = tasks.iter().map(|task| task.id.clone()).collect();
    if tasks.is_empty() {
        return Err(format!("Preset desconhecido ou vazio: {preset_id}"));
    }

    tauri::async_runtime::spawn(async move {
        for task in tasks {
            start_task(app.clone(), task, false);
            while safety_state().lock().unwrap().is_task_running() {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
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
    safety.rollback_session(None)
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
