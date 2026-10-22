#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capability_inventory;
mod cleanup_findings;
mod diagnostics;
mod registry_executor;
mod safety;
mod service_manager;
mod service_profiles;
mod system_compat;
mod task_registry;
mod tweak_profiles;

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::io::{BufRead, BufReader, Read};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Runtime};

use safety::{ChangeRecord, SafetyManager};
use service_profiles::ServiceProfileBackend;
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

static BROWSER_CLEANUP_ENABLED: AtomicBool = AtomicBool::new(true);

static SAFETY_STATE: std::sync::OnceLock<Arc<Mutex<SafetyManager>>> = std::sync::OnceLock::new();

static RESTORE_POINT_RESPONSES: std::sync::OnceLock<
    Mutex<std::collections::HashMap<String, mpsc::Sender<bool>>>,
> = std::sync::OnceLock::new();

fn restore_point_responses() -> &'static Mutex<std::collections::HashMap<String, mpsc::Sender<bool>>>
{
    RESTORE_POINT_RESPONSES.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

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

fn decode_process_output(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    #[cfg(target_os = "windows")]
    {
        // DISM e ferramentas antigas podem emitir a página OEM do Windows, não UTF-8.
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MultiByteToWideChar(
                code_page: u32,
                flags: u32,
                input: *const i8,
                input_len: i32,
                output: *mut u16,
                output_len: i32,
            ) -> i32;
        }
        if !bytes.is_empty() && bytes.len() <= i32::MAX as usize {
            let required = unsafe {
                MultiByteToWideChar(
                    1,
                    0,
                    bytes.as_ptr().cast(),
                    bytes.len() as i32,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if required > 0 {
                let mut wide = vec![0u16; required as usize];
                let written = unsafe {
                    MultiByteToWideChar(
                        1,
                        0,
                        bytes.as_ptr().cast(),
                        bytes.len() as i32,
                        wide.as_mut_ptr(),
                        required,
                    )
                };
                if written > 0 {
                    return String::from_utf16_lossy(&wide[..written as usize]);
                }
            }
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}
fn forward_output<R: Read + Send + 'static>(
    stream: R,
    sender: mpsc::Sender<String>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // DISM atualiza a barra com carriage return (\\r), sem newline.
        // Lemos por blocos para emitir cada atualização como uma substituição da linha atual.
        let mut reader = BufReader::new(stream);
        let mut chunk = [0u8; 4096];
        let mut line = Vec::new();
        let mut previous_was_cr = false;
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    if !line.is_empty() {
                        let text = decode_process_output(&line);
                        let _ = sender.send(text);
                    }
                    break;
                }
                Ok(count) => {
                    for byte in &chunk[..count] {
                        if *byte == b'\r' || *byte == b'\n' {
                            if *byte == b'\n' && previous_was_cr {
                                previous_was_cr = false;
                                continue;
                            }
                            let text = decode_process_output(&line);
                            line.clear();
                            if sender.send(text).is_err() {
                                return;
                            }
                            previous_was_cr = *byte == b'\r';
                        } else {
                            line.push(*byte);
                            previous_was_cr = false;
                        }
                    }
                }
                Err(error) => {
                    let _ = sender.send(format!("Falha ao ler saída do processo: {error}"));
                    break;
                }
            }
        }
    })
}
fn configure_hidden_console(command: &mut Command) {
    #[cfg(target_os = "windows")]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    #[cfg(not(target_os = "windows"))]
    let _ = command;
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

#[cfg(target_os = "windows")]
#[repr(C)]
struct LocalSystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

#[cfg(target_os = "windows")]
fn local_timestamp() -> String {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(time: *mut LocalSystemTime);
    }
    let mut now = LocalSystemTime {
        year: 0,
        month: 0,
        day_of_week: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        milliseconds: 0,
    };
    unsafe {
        GetLocalTime(&mut now);
    }
    format!(
        "{:02}:{:02} {:02}-{:02}-{:04}",
        now.hour, now.minute, now.day, now.month, now.year
    )
}

#[cfg(not(target_os = "windows"))]
fn local_timestamp() -> String {
    "hora local indisponível".to_string()
}

fn humanize_execution_log(message: &str) -> String {
    if let Some(details) = message.strip_prefix("task-executor-failed error=") {
        return format!("Executor de tarefas falhou: {details}");
    }
    if let Some(details) = message.strip_prefix("registry-operation-failed error=") {
        return format!("Falha na operação de Registro: {details}");
    }
    message.to_string()
}

fn write_execution_log(execution_id: &str, message: &str) {
    let path = execution_log_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let mut lines = message.lines();
        if let Some(first) = lines.next() {
            let humanized = humanize_execution_log(first);
            let _ = writeln!(
                file,
                "[{execution_id}] [{}] {}",
                local_timestamp(),
                humanized
            );
            for line in lines {
                let _ = writeln!(file, "    {line}");
            }
        } else {
            let _ = writeln!(file, "[{execution_id}] [{}]", local_timestamp());
        }
    }
}

fn run_command(
    cmd: &str,
    args: &[&str],
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    let mut command = Command::new(cmd);
    configure_hidden_console(&mut command);
    let mut child = command
        .env(
            "QBOA_CLEAN_BROWSER_CACHE",
            if BROWSER_CLEANUP_ENABLED.load(Ordering::Relaxed) {
                "1"
            } else {
                "0"
            },
        )
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

    let stdout_thread = forward_output(stdout, sender.clone());
    let stderr_thread = forward_output(stderr, sender.clone());
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
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            while let Ok(line) = receiver.try_recv() {
                on_output(&line);
                output.push_str(&line);
                output.push('\n');
            }
            write_execution_log(
                execution_id,
                &format!(
                    "finished pid={} status={status} output_bytes={}",
                    child.id(),
                    output.len()
                ),
            );
            if status.success() {
                return Ok(output);
            }
            write_execution_log(
                execution_id,
                &format!("failed command={cmd} output={output}"),
            );
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
    let utf8_script = format!(
        "[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false); $OutputEncoding = New-Object System.Text.UTF8Encoding($false); {script}"
    );
    run_command(
        "powershell",
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &utf8_script,
        ],
        execution_id,
        on_output,
        is_cancelled,
    )
}

fn report_system_compatibility(on_output: &mut dyn FnMut(&str)) -> bool {
    match system_compat::detect() {
        Ok(assessment) => {
            on_output(&format!("[Compatibilidade] {}", assessment.summary));
            for indicator in &assessment.indicators {
                on_output(&format!("[Compatibilidade] {indicator}"));
            }
            assessment.use_conservative_repair
        }
        Err(error) => {
            on_output(&format!(
                "[Compatibilidade] Não foi possível classificar o ambiente; mantendo o fluxo padrão. Detalhe: {error}"
            ));
            false
        }
    }
}

fn cleanup_standard(
    task_id: &str,
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
    apply_registry: &mut dyn FnMut(&str) -> Result<u64, String>,
) -> Result<String, String> {
    let output = run_powershell(
        include_str!("../scripts/cleanup_standard.ps1"),
        execution_id,
        on_output,
        is_cancelled,
    )?;
    let registry_keys_removed = apply_registry(task_id)?;
    let summary = "Limpeza padrão concluída: temporários, caches, MRUs e lixeira.";
    let registry_metrics = format!("QBOA_METRICS files=0 bytes=0 registry={registry_keys_removed}");
    on_output(summary);
    on_output(&registry_metrics);
    Ok(format!("{output}\n{summary}\n{registry_metrics}"))
}

fn cleanup_heavy_temp(
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    run_powershell(
        r#"
$paths = @("$env:TEMP", "$env:WINDIR\Temp", "$env:WINDIR\Minidump", "$env:WINDIR\LiveKernelReports");
$removed = 0; $removedFiles = 0; [long]$freedBytes = 0; $failed = 0;
foreach ($p in $paths) {
  if (Test-Path -LiteralPath $p) {
    foreach ($item in @(Get-ChildItem -LiteralPath $p -Force -ErrorAction SilentlyContinue)) {
      try {
        if ($item.PSIsContainer) { $files = @(Get-ChildItem -LiteralPath $item.FullName -Recurse -Force -File -ErrorAction SilentlyContinue) }
        else { $files = @($item) }
        $size = ($files | Measure-Object -Property Length -Sum).Sum
        Remove-Item -LiteralPath $item.FullName -Recurse -Force -ErrorAction Stop
        $removedFiles += $files.Count
        if ($size) { $freedBytes += [long]$size }
        $removed++
      } catch { $failed++ ; Write-Output ("Não foi possível remover: " + $item.FullName + " — " + $_.Exception.Message) }
    }
  }
}
try { Clear-RecycleBin -Force -ErrorAction SilentlyContinue } catch {}
Write-Output ("Limpeza pesada: {0} item(ns)/pasta(s) removido(s), {1} falha(s)." -f $removed, $failed)
Write-Output ("QBOA_METRICS files={0} bytes={1} registry=0" -f $removedFiles, $freedBytes)
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
$ErrorActionPreference = 'Stop';
$path = Join-Path $env:WINDIR 'SoftwareDistribution\Download';
if (-not (Test-Path -LiteralPath $path)) {
  Write-Output 'Cache de download do Windows Update não existe; nada a limpar.';
  Write-Output 'QBOA_METRICS files=0 bytes=0 registry=0';
  exit 0;
}
$items = @(Get-ChildItem -LiteralPath $path -Force -ErrorAction Stop);
$removed = 0; $removedFiles = 0; [long]$freedBytes = 0; $failed = 0;
foreach ($item in $items) {
  try {
    $tree = @($item) + @(Get-ChildItem -LiteralPath $item.FullName -Recurse -Force -File -ErrorAction SilentlyContinue);
    $files = @($tree | Where-Object { -not $_.PSIsContainer });
    $size = ($files | Measure-Object -Property Length -Sum).Sum;
    Remove-Item -LiteralPath $item.FullName -Recurse -Force -ErrorAction Stop;
    $removedFiles += $files.Count;
    if ($size) { $freedBytes += [long]$size; }
    $removed++;
  } catch {
    $failed++;
    Write-Output ("Não foi possível remover: " + $item.FullName + " — " + $_.Exception.Message);
  }
}
Write-Output ("Windows Update: {0} item(ns) removido(s), {1} falha(s)." -f $removed, $failed);
Write-Output ("QBOA_METRICS files={0} bytes={1} registry=0" -f $removedFiles, $freedBytes);
if ($failed -gt 0) { exit 2 }
"#,
        execution_id,
        on_output,
        is_cancelled,
    )
}

fn disable_telemetry_components(
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    run_powershell(
        r#"
$ErrorActionPreference = 'SilentlyContinue';
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

fn resolve_task(task_id: &str) -> Result<Option<Task>, String> {
    let registry = TaskRegistry::try_new()?;
    Ok(registry.resolve(task_id).cloned())
}

#[cfg(target_os = "windows")]
fn apply_registry_task(
    task_id: &str,
    execution_id: &str,
    snapshot_location: &str,
) -> Result<ChangeRecord, String> {
    let catalog = TaskRegistry::try_new()?;
    let mut backend = registry_executor::PowerShellRegistryBackend;
    registry_executor::execute_registry_task(
        task_id,
        &catalog,
        execution_id,
        snapshot_location,
        &mut safety_state().lock().unwrap(),
        &mut backend,
    )
}

#[cfg(not(target_os = "windows"))]
fn apply_registry_task(
    task_id: &str,
    _execution_id: &str,
    _snapshot_location: &str,
) -> Result<ChangeRecord, String> {
    Err(format!(
        "Executor de Registro só está disponível no Windows: {task_id}"
    ))
}

#[cfg(target_os = "windows")]
fn apply_cleanup_findings(
    profile: &str,
    on_output: &mut dyn FnMut(&str),
) -> cleanup_findings::CleanupExecutionReport {
    let mut backend = cleanup_findings::WindowsCleanupFindingBackend;
    match cleanup_findings::cleanup_profile(&mut backend, profile) {
        Ok(report) => {
            on_output(&format!(
                "Varredura integrada ({profile}): {} arquivos removidos, {} entradas de Registro excluídas, {} falha(s).",
                report.files_removed, report.registry_entries_removed, report.failures
            ));
            for failure in &report.failure_details {
                on_output(&format!("Falha ao corrigir achado: {failure}"));
            }
            on_output(&format!(
                "QBOA_METRICS files={} bytes={} registry={}",
                report.files_removed, report.bytes_freed, report.registry_entries_removed
            ));
            report
        }
        Err(error) => {
            on_output(&format!(
                "A varredura integrada não pôde ser concluída: {error}. A limpeza principal continuará."
            ));
            cleanup_findings::CleanupExecutionReport {
                failures: 1,
                ..Default::default()
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_cleanup_findings(
    _profile: &str,
    _on_output: &mut dyn FnMut(&str),
) -> cleanup_findings::CleanupExecutionReport {
    cleanup_findings::CleanupExecutionReport::default()
}

fn findings_summary(report: &cleanup_findings::CleanupExecutionReport) -> String {
    format!(
        "Varredura integrada: {} arquivos removidos, {} entradas de Registro excluídas, {} falha(s).\nQBOA_METRICS files={} bytes={} registry={}",
        report.files_removed,
        report.registry_entries_removed,
        report.failures,
        report.files_removed,
        report.bytes_freed,
        report.registry_entries_removed
    )
}

fn count_mru_registry_keys(snapshot_location: &str) -> u64 {
    let Ok(contents) = std::fs::read(snapshot_location) else {
        return 0;
    };
    let Ok(snapshot) = serde_json::from_slice::<serde_json::Value>(&contents) else {
        return 0;
    };
    let Some(entries) = snapshot
        .get("registry")
        .and_then(serde_json::Value::as_array)
    else {
        return 0;
    };
    let roots = [
        r"software\microsoft\windows\currentversion\explorer\recentdocs",
        r"software\microsoft\windows\currentversion\explorer\runmru",
        r"software\microsoft\windows\currentversion\explorer\typedpaths",
    ];
    let mut paths = std::collections::HashSet::new();
    for entry in entries {
        let hive = entry
            .get("hive")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let path = entry
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let normalized = path.to_ascii_lowercase();
        if hive.eq_ignore_ascii_case("CurrentUser")
            && roots
                .iter()
                .any(|root| normalized == *root || normalized.starts_with(&format!("{root}\\")))
        {
            paths.insert(normalized);
        }
    }
    paths.len() as u64
}

fn parse_cleanup_metrics(output: &str) -> (u64, u64, u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut registry = 0u64;
    let mut failures = 0u64;
    let mut seen_failure_lines = std::collections::HashSet::new();
    for line in output.lines() {
        if line.contains("QBOA_METRICS") {
            for field in line.split_whitespace() {
                if let Some(value) = field.strip_prefix("files=") {
                    files = files.saturating_add(value.parse().unwrap_or(0));
                } else if let Some(value) = field.strip_prefix("bytes=") {
                    bytes = bytes.saturating_add(value.parse().unwrap_or(0));
                } else if let Some(value) = field.strip_prefix("registry=") {
                    registry = registry.saturating_add(value.parse().unwrap_or(0));
                }
            }
        }
        if line.contains("falha(s)") && seen_failure_lines.insert(line.trim().to_string()) {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            let count = parts
                .iter()
                .position(|part| part.contains("falha(s)"))
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| {
                    parts[index]
                        .trim_matches(|ch: char| !ch.is_ascii_digit())
                        .parse::<u64>()
                        .ok()
                })
                .unwrap_or(0);
            failures = failures.saturating_add(count);
        }
    }
    (files, bytes, registry, failures)
}

fn format_cleanup_summary(
    files: u64,
    bytes: u64,
    registry: u64,
    failures: u64,
    elapsed_label: &str,
) -> String {
    let registry_label = if registry == 1 {
        "chave/entrada de Registro excluída"
    } else {
        "chaves/entradas de Registro excluídas"
    };
    format!(
        "\n\n=================\nRESUMO DA EXECUÇÃO\n=================\n\n- {files} arquivos removidos\n- {registry} {registry_label}\n- {:.1} MB liberados (estimativa)\n- Tempo de execução: {elapsed_label}\n\n- Total de erros durante execução: {failures}",
        bytes as f64 / 1_000_000.0
    )
}

fn clean_final_cleanup_output(output: &str) -> String {
    output
        .lines()
        .filter(|line| {
            let line = line.trim();
            !line.contains("QBOA_METRICS")
                && !line.starts_with("Limpeza padrão:")
                && !line.starts_with("Limpeza leve:")
                && !line.starts_with("Limpeza média:")
                && !line.starts_with("Limpeza pesada:")
                && !line.starts_with("Lixeira não esvaziada:")
                && !line.starts_with("Varredura integrada:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn execute_task(
    task: &Task,
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
    apply_registry: &mut dyn FnMut(&str) -> Result<u64, String>,
) -> Result<String, String> {
    match task.id.as_str() {
        "limpeza_leve" => cleanup_standard(
            &task.id,
            execution_id,
            on_output,
            is_cancelled,
            apply_registry,
        ),
        "limpeza_media" => {
            let conservative_repair = report_system_compatibility(on_output);
            if conservative_repair {
                on_output("[Compatibilidade] Ambiente modificado/restritivo detectado. O QBoa manterá o DISM padrão, sem inventar uma origem de reparo.");
            }
            on_output("[Etapa 1/5] Limpando temporários, caches e lixeira...");
            let part_1 = cleanup_standard(
                &task.id,
                execution_id,
                on_output,
                is_cancelled,
                apply_registry,
            )?;
            on_output("[Etapa 2/5] Executando a Limpeza de Disco do Windows (cleanmgr)...");
            let part_2 = run_system_command(
                "cleanmgr",
                &["/verylowdisk"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            on_output("[Etapa 3/5] Processando o cache temporário do Windows Update...");
            let part_3 = cleanup_windows_update(execution_id, on_output, is_cancelled)?;
            on_output(
                "[Etapa 4/5] Removendo atalhos quebrados e associações inválidas confirmadas...",
            );
            let findings = apply_cleanup_findings("complete", on_output);
            on_output(
                "[Etapa 5/5] Verificando e reparando a imagem do Windows com DISM RestoreHealth...",
            );
            let part_4 = run_system_command(
                "dism",
                &["/Online", "/Cleanup-Image", "/RestoreHealth"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            Ok(format!(
                "{part_1}\n{part_2}\n{part_3}\n{}\n{part_4}",
                findings_summary(&findings)
            ))
        }
        "limpeza_pesada" => {
            let conservative_repair = report_system_compatibility(on_output);
            if conservative_repair {
                on_output("[Compatibilidade] Ambiente modificado/restritivo detectado. O QBoa preservará a base de componentes e não executará /ResetBase, pois isso impede a desinstalação de atualizações instaladas.");
            }
            on_output("[Etapa 1/8] Limpando temporários, caches e lixeira...");
            let part_1 = cleanup_standard(
                &task.id,
                execution_id,
                on_output,
                is_cancelled,
                apply_registry,
            )?;
            on_output(
                "[Etapa 2/8] Limpando diretórios temporários avançados e relatórios de falha...",
            );
            let part_2 = cleanup_heavy_temp(execution_id, on_output, is_cancelled)?;
            on_output("[Etapa 3/8] Processando o cache temporário do Windows Update...");
            let part_3 = cleanup_windows_update(execution_id, on_output, is_cancelled)?;
            on_output("[Etapa 4/8] Removendo atalhos quebrados, associações inválidas e entradas órfãs confirmadas...");
            let findings = apply_cleanup_findings("deep", on_output);
            on_output("[Etapa 5/8] Verificando a integridade da imagem do Windows...");
            let part_4 = run_system_command(
                "dism",
                &["/Online", "/Cleanup-Image", "/ScanHealth"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            let part_5 = if conservative_repair {
                on_output("[Etapa 6/8] Pulando /ResetBase por segurança no ambiente modificado/restritivo.");
                "DISM /ResetBase ignorado por política conservadora.".to_string()
            } else {
                on_output("[Etapa 6/8] Limpando componentes substituídos e redefinindo a base (ResetBase). Atenção: atualizações instaladas deixarão de poder ser desinstaladas...");
                run_system_command(
                    "dism",
                    &[
                        "/Online",
                        "/Cleanup-Image",
                        "/StartComponentCleanup",
                        "/ResetBase",
                    ],
                    execution_id,
                    on_output,
                    is_cancelled,
                )?
            };
            on_output("[Etapa 7/8] Reparando a imagem do Windows com DISM RestoreHealth...");
            let part_6 = run_system_command(
                "dism",
                &["/Online", "/Cleanup-Image", "/RestoreHealth"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            on_output("[Etapa 8/8] Verificando arquivos protegidos com SFC...");
            let part_7 =
                run_system_command("sfc", &["/scannow"], execution_id, on_output, is_cancelled)?;
            Ok(format!(
                "{part_1}\n{part_2}\n{part_3}\n{}\n{part_4}\n{part_5}\n{part_6}\n{part_7}",
                findings_summary(&findings)
            ))
        }
        "desengordurar_telemetria" => {
            apply_registry(&task.id)?;
            disable_telemetry_components(execution_id, on_output, is_cancelled)
        }
        "desengordurar_tarefas" => disable_diagnostic_tasks(execution_id, on_output, is_cancelled),
        "limpeza_windows_update" => cleanup_windows_update(execution_id, on_output, is_cancelled),
        "diagnostico_dism_scan" => run_system_command(
            "dism",
            &["/Online", "/Cleanup-Image", "/ScanHealth"],
            execution_id,
            on_output,
            is_cancelled,
        ),
        "diagnostico_dism_restore" => {
            report_system_compatibility(on_output);
            run_system_command(
                "dism",
                &["/Online", "/Cleanup-Image", "/RestoreHealth"],
                execution_id,
                on_output,
                is_cancelled,
            )
        }
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
    Ok(TaskRegistry::try_new()?.list())
}

#[tauri::command]
async fn respond_restore_point_failure(execution_id: String, proceed: bool) -> Result<(), String> {
    let sender = restore_point_responses()
        .lock()
        .unwrap()
        .remove(&execution_id)
        .ok_or_else(|| {
            "A confirmação do ponto de restauração expirou ou não está mais pendente.".to_string()
        })?;
    sender
        .send(proceed)
        .map_err(|_| "Não foi possível enviar a resposta para a tarefa.".to_string())
}

#[tauri::command]
async fn run_task(
    app: AppHandle,
    task_id: String,
    shutdown_on_complete: bool,
) -> Result<TaskResult, String> {
    let registry = TaskRegistry::try_new()?;
    let task = registry
        .resolve(&task_id)
        .cloned()
        .ok_or_else(|| format!("Tarefa desconhecida: {task_id}"))?;
    let execution = start_task(app, task.clone(), shutdown_on_complete);
    let result = task_result(&task, &execution.execution_id);
    Ok(result)
}

fn task_result(task: &Task, execution_id: &str) -> TaskResult {
    let task_risk = task_risk_label(task).to_string();
    TaskResult {
        ok: true,
        output: format!("Tarefa iniciada: {}", task.name),
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
        write_execution_log(
            &execution_id_for_spawn,
            &format!("task-started task_id={task_id_for_spawn}"),
        );
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

        let snapshot_location = if task_for_spawn.reversible
            || task_for_spawn.executor == task_registry::ExecutorType::Registry
        {
            write_execution_log(&execution_id_for_spawn, "snapshot-started");
            match safety_state()
                .lock()
                .unwrap()
                .capture_snapshot(&task_for_spawn.id)
            {
                Ok(location) => Some(location),
                Err(error) => {
                    write_execution_log(
                        &execution_id_for_spawn,
                        &format!("snapshot-failed error={error}"),
                    );
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
                    write_execution_log(
                        &execution_id_for_spawn,
                        &format!("restore-point-phase-failed error={err}"),
                    );
                    let (sender, receiver) = mpsc::channel();
                    restore_point_responses()
                        .lock()
                        .unwrap()
                        .insert(execution_id_for_spawn.clone(), sender);
                    emit_event(
                        &app_for_spawn,
                        "restore-point-confirmation-required",
                        &task_id_for_spawn,
                        &execution_id_for_spawn,
                        &format!(
                            "Não foi possível criar um ponto de restauração: {err}\nDeseja prosseguir sem ponto de restauração?"
                        ),
                        false,
                        None,
                        &task_risk_for_spawn,
                    );
                    let proceed = receiver.recv().unwrap_or(false);
                    restore_point_responses()
                        .lock()
                        .unwrap()
                        .remove(&execution_id_for_spawn);
                    if !proceed {
                        write_execution_log(
                            &execution_id_for_spawn,
                            "task-cancelled-by-user-after-restore-point-failure",
                        );
                        safety_state()
                            .lock()
                            .unwrap()
                            .finish_execution(&execution_id_for_spawn, "cancelled");
                        emit_event(
                            &app_for_spawn,
                            "task-cancelled",
                            &task_id_for_spawn,
                            &execution_id_for_spawn,
                            "Tarefa cancelada. O ponto de restauração não foi criado.",
                            false,
                            None,
                            &task_risk_for_spawn,
                        );
                        return;
                    }
                    write_execution_log(
                        &execution_id_for_spawn,
                        "user-approved-proceeding-without-restore-point",
                    );
                    emit_event(
                        &app_for_spawn,
                        "task-output",
                        &task_id_for_spawn,
                        &execution_id_for_spawn,
                        "Aviso: prosseguindo sem ponto de restauração; não será possível usar esse ponto para reverter alterações.",
                        true,
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
        let mut streamed_output = String::new();
        let mut on_output = |line: &str| {
            streamed_output.push_str(line);
            streamed_output.push('\n');
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
        let mut registry_change = None;
        let mut apply_registry = |registry_task_id: &str| {
            if task_for_spawn.executor != task_registry::ExecutorType::Registry {
                return Ok(0);
            }
            match apply_registry_task(
                registry_task_id,
                &execution_id_for_spawn,
                snapshot_location.as_deref().unwrap_or_default(),
            ) {
                Ok(change) => {
                    registry_change = Some(change);
                    let count = if matches!(
                        registry_task_id,
                        "limpeza_leve" | "limpeza_media" | "limpeza_pesada"
                    ) {
                        count_mru_registry_keys(snapshot_location.as_deref().unwrap_or_default())
                    } else {
                        0
                    };
                    Ok(count)
                }
                Err(error) => {
                    write_execution_log(
                        &execution_id_for_spawn,
                        &format!("registry-operation-failed error={error}"),
                    );
                    Err(error)
                }
            }
        };

        let execution_started = Instant::now();
        let execution = execute_task(
            &task_for_spawn,
            &execution_id_for_spawn,
            &mut on_output,
            &is_cancelled,
            &mut apply_registry,
        );
        drop(apply_registry);
        drop(on_output);
        match execution {
            Ok(output) => {
                let (files_removed, bytes_freed, registry_entries_removed, failures) =
                    parse_cleanup_metrics(&output);
                let elapsed = execution_started.elapsed();
                let elapsed_seconds = elapsed.as_secs();
                let elapsed_label = if elapsed_seconds >= 60 {
                    format!("{} min {} s", elapsed_seconds / 60, elapsed_seconds % 60)
                } else {
                    format!("{:.1} s", elapsed.as_secs_f64())
                };
                let summary = format_cleanup_summary(
                    files_removed,
                    bytes_freed,
                    registry_entries_removed,
                    failures,
                    &elapsed_label,
                );
                let cleaned_output = clean_final_cleanup_output(&output);
                let output = if cleaned_output.trim().is_empty() {
                    summary.clone()
                } else {
                    format!("{cleaned_output}\n\n{summary}")
                };
                write_execution_log(
                    &execution_id_for_spawn,
                    &format!("task-executor-completed {summary}"),
                );
                let journal_result = if let Some(change) = registry_change.take() {
                    safety_state()
                        .lock()
                        .unwrap()
                        .update_change_result(&change.id, "completed")
                        .map_err(|error| error)
                } else {
                    safety_state()
                        .lock()
                        .unwrap()
                        .record_change(
                            &execution_id_for_spawn,
                            &task_id_for_spawn,
                            &task_name,
                            task.reversible,
                            snapshot_location.as_deref(),
                            "completed",
                        )
                        .map(|_| ())
                        .map_err(|error| error.error)
                };
                if let Err(error) = journal_result {
                    let message = format!(
                        "Tarefa executada, mas não foi possível registrar a alteração no journal: {}",
                        error
                    );
                    write_execution_log(&execution_id_for_spawn, &message);
                    safety_state()
                        .lock()
                        .unwrap()
                        .finish_execution(&execution_id_for_spawn, "journal_failed");
                    emit_event(
                        &app_for_spawn,
                        "task-error",
                        &task_id_for_spawn,
                        &execution_id_for_spawn,
                        &message,
                        false,
                        None,
                        &task_risk_for_spawn,
                    );
                    return;
                }

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
            }
            Err(err) => {
                let (files_removed, bytes_freed, registry_entries_removed, _failures) =
                    parse_cleanup_metrics(&streamed_output);
                let elapsed = execution_started.elapsed();
                let elapsed_seconds = elapsed.as_secs();
                let elapsed_label = if elapsed_seconds >= 60 {
                    format!("{} min {} s", elapsed_seconds / 60, elapsed_seconds % 60)
                } else {
                    format!("{:.1} s", elapsed.as_secs_f64())
                };
                let summary = format!(
                    "Resumo parcial da execução — {files_removed} arquivos removidos, {registry_entries_removed} chaves/entradas de Registro excluídas, {:.3} GB liberados (estimativa pelos arquivos removidos), tempo até a interrupção/falha: {elapsed_label}.",
                    bytes_freed as f64 / 1_000_000_000.0
                );
                emit_event(
                    &app_for_spawn,
                    "task-output",
                    &task_id_for_spawn,
                    &execution_id_for_spawn,
                    &summary,
                    true,
                    None,
                    &task_risk_for_spawn,
                );
                write_execution_log(
                    &execution_id_for_spawn,
                    &format!("task-executor-failed error={err}; {summary}"),
                );
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
                    let failed_change_result = if let Some(change) = registry_change.take() {
                        Ok(change)
                    } else {
                        safety_state().lock().unwrap().record_change(
                            &execution_id_for_spawn,
                            &task_id_for_spawn,
                            &task_name,
                            task.reversible,
                            snapshot_location.as_deref(),
                            "failed",
                        )
                    };
                    let failed_change = match failed_change_result {
                        Ok(change) => change,
                        Err(record_error) => {
                            let rollback_result = if task.rollback_on_failure && task.reversible {
                                Some(
                                    safety_state()
                                        .lock()
                                        .unwrap()
                                        .rollback_unrecorded_change(&record_error.change),
                                )
                            } else {
                                None
                            };
                            let final_status = match rollback_result.as_ref() {
                                Some(Ok(_)) => "failed_journal_error_and_rolled_back",
                                Some(Err(_)) => "failed_journal_error_rollback_failed",
                                None => "failed_journal_error",
                            };
                            let mut message = format!(
                                "{err}; não foi possível registrar a alteração no journal: {}",
                                record_error.error
                            );
                            if let Some(Err(rollback_error)) = rollback_result.as_ref() {
                                message.push_str(&format!(
                                    "; rollback também falhou: {rollback_error}"
                                ));
                            }
                            write_execution_log(&execution_id_for_spawn, &message);
                            safety_state()
                                .lock()
                                .unwrap()
                                .finish_execution(&execution_id_for_spawn, final_status);
                            emit_event(
                                &app_for_spawn,
                                "task-error",
                                &task_id_for_spawn,
                                &execution_id_for_spawn,
                                &message,
                                false,
                                None,
                                &task_risk_for_spawn,
                            );
                            return;
                        }
                    };
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
                    if let Err(persist_error) = safety_state()
                        .lock()
                        .unwrap()
                        .update_change_result(&failed_change.id, final_status)
                    {
                        write_execution_log(
                            &execution_id_for_spawn,
                            &format!("journal update failed after task error: {persist_error}"),
                        );
                    }
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
async fn get_running_browsers() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("powershell");
        configure_hidden_console(&mut command);
        let output = command
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "$names=@('chrome','msedge','brave','vivaldi','opera','firefox'); $found=@(Get-Process -Name $names -ErrorAction SilentlyContinue | Select-Object -ExpandProperty ProcessName -Unique); ConvertTo-Json -InputObject @($found) -Compress"])
            .output()
            .map_err(|error| format!("Não foi possível verificar os navegadores abertos: {error}"))?;
        if !output.status.success() {
            return Err("Não foi possível verificar os navegadores abertos.".to_string());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let value: serde_json::Value = serde_json::from_str(text.trim())
            .map_err(|error| format!("Resposta inválida ao verificar navegadores: {error}"))?;
        let values = value.as_array().cloned().unwrap_or_else(|| vec![value]);
        return Ok(values
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect());
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(Vec::new())
    }
}

#[tauri::command]
async fn prepare_browser_cleanup(close_running: bool) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        if close_running {
            let mut command = Command::new("powershell");
            configure_hidden_console(&mut command);
            let output = command
                .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command",
                    "$names=@('chrome','msedge','brave','vivaldi','opera','firefox'); Get-Process -Name $names -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue; 'Navegadores encerrados quando estavam em execução.'"])
                .output()
                .map_err(|error| format!("Não foi possível fechar os navegadores: {error}"))?;
            if !output.status.success() {
                return Err("Não foi possível fechar os navegadores. Feche-os manualmente ou escolha ignorar a limpeza dos navegadores.".to_string());
            }
            BROWSER_CLEANUP_ENABLED.store(true, Ordering::Relaxed);
            return Ok("A limpeza dos caches dos navegadores está habilitada.".to_string());
        }
        BROWSER_CLEANUP_ENABLED.store(false, Ordering::Relaxed);
        return Ok("A limpeza dos caches dos navegadores será ignorada.".to_string());
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = close_running;
        Ok("A limpeza de navegadores só está disponível no Windows.".to_string())
    }
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
    let registry = TaskRegistry::try_new()?;
    let tasks: Vec<Task> = registry
        .preset_tasks(&preset_id)
        .into_iter()
        .cloned()
        .collect();
    let ids: Vec<String> = tasks.iter().map(|task| task.id.clone()).collect();
    if tasks.is_empty() {
        return Err(format!("Perfil desconhecido ou vazio: {preset_id}"));
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
async fn get_debloat_profile_tasks(
    profile: task_registry::TweakProfile,
    selected_ids: Vec<String>,
) -> Result<Vec<String>, String> {
    let registry = TaskRegistry::try_new()?;
    tweak_profiles::resolve_task_ids(&registry, profile, &selected_ids)
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

#[tauri::command]
async fn run_diagnostic_check(
    mode: diagnostics::DiagnosticMode,
) -> Result<diagnostics::DiagnosticReport, String> {
    let mut backend = diagnostics::WindowsDiagnosticBackend::default();
    diagnostics::run(&mut backend, mode)
}

#[tauri::command]
async fn get_capability_inventory() -> Result<capability_inventory::CapabilityInventory, String> {
    capability_inventory::collect()
}

#[derive(Clone, Serialize)]
struct ServiceProfileState {
    system: service_profiles::DetectedSystem,
    profile: service_profiles::ServiceProfileFile,
    products: Vec<service_profiles::ProductProfile>,
}

#[tauri::command]
async fn list_windows_services() -> Result<Vec<service_manager::ServiceEntry>, String> {
    service_manager::list_services()
}

#[tauri::command]
async fn manage_windows_service(
    name: String,
    action: String,
    start_type: Option<String>,
) -> Result<String, String> {
    service_manager::manage_service(&name, &action, start_type.as_deref())
}

#[tauri::command]
async fn audit_unsigned_services(
) -> Result<Vec<service_manager::ServiceSignatureFinding>, String> {
    service_manager::audit_unsigned_services()
}

#[tauri::command]
async fn get_service_profile_state() -> Result<ServiceProfileState, String> {
    let mut backend = service_profiles::WindowsServiceProfileBackend::default();
    let system = backend.detect_system()?;
    let profile = service_profiles::system_profile(&system)?;
    Ok(ServiceProfileState {
        system,
        profile,
        products: service_profiles::product_profiles(),
    })
}

#[tauri::command]
async fn preview_service_profile(
    profile: service_profiles::ProductProfile,
) -> Result<service_profiles::ServicePreview, String> {
    let mut backend = service_profiles::WindowsServiceProfileBackend::default();
    let system = backend.detect_system()?;
    let service_profile = service_profiles::system_profile(&system)?;
    Ok(service_profiles::preview(
        &system,
        &service_profile,
        profile,
    ))
}

#[tauri::command]
async fn apply_service_profile(
    profile: service_profiles::ProductProfile,
    selected_to_disable: Vec<String>,
) -> Result<(), String> {
    if !selected_to_disable.is_empty() {
        let audit = service_manager::audit_unsigned_services()?;
        for name in &selected_to_disable {
            let finding = audit
                .iter()
                .find(|finding| finding.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| {
                    format!(
                        "O serviço {name} não consta mais na auditoria de assinaturas; nenhuma alteração foi aplicada."
                    )
                })?;
            if finding.protected {
                return Err(format!(
                    "O serviço {name} é protegido pelo QBoa e não pode ser desativado."
                ));
            }
        }
    }

    let mut backend = service_profiles::WindowsServiceProfileBackend::default();
    let system = backend.detect_system()?;
    let service_profile = service_profiles::system_profile(&system)?;
    service_profiles::apply_profile_with_disabled_services(
        &mut backend,
        &service_profile,
        profile,
        &selected_to_disable,
    )
}

#[tauri::command]
async fn restore_windows_service_defaults() -> Result<(), String> {
    let mut backend = service_profiles::WindowsServiceProfileBackend::default();
    let system = backend.detect_system()?;
    let profile = service_profiles::system_profile(&system)?;
    service_profiles::restore_windows_defaults(&mut backend, &profile)
}

#[tauri::command]
async fn undo_service_profile_changes() -> Result<(), String> {
    let mut backend = service_profiles::WindowsServiceProfileBackend::default();
    let system = backend.detect_system()?;
    let profile = service_profiles::system_profile(&system)?;
    service_profiles::undo_last_application(&mut backend, &profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tasks_are_resolved() {
        assert!(resolve_task("limpeza_leve").unwrap().is_some());
        assert!(resolve_task("desengordurar_telemetria").unwrap().is_some());
        assert!(resolve_task("diagnostico_sfc").unwrap().is_some());
    }

    #[test]
    fn unknown_tasks_are_rejected() {
        let registry = TaskRegistry::try_new().unwrap();
        assert!(registry.resolve("task_inexistente").is_none());
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_tasks,
            run_task,
            respond_restore_point_failure,
            get_running_browsers,
            prepare_browser_cleanup,
            cancel_task,
            run_preset,
            get_debloat_profile_tasks,
            create_restore_point,
            rollback_session,
            get_session_history,
            run_diagnostic_check,
            get_capability_inventory,
            list_windows_services,
            manage_windows_service,
            get_service_profile_state,
            preview_service_profile,
            audit_unsigned_services,
            apply_service_profile,
            restore_windows_service_defaults,
            undo_service_profile_changes
        ])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o Qboa Digital");
}
