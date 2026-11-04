#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capability_inventory;
mod cleanup_engine;
mod cleanup_findings;
mod complete_diagnostic;
mod cpu_diagnostic;
mod diagnostics;
mod diagnostics_store;
mod gpu_diagnostic;
mod memory_inventory;
mod memory_test;
mod registry_executor;
mod safety;
mod service_manager;
mod service_profiles;
mod storage_diagnostic;
mod system_compat;
mod task_registry;
mod tweak_profiles;

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::io::{BufReader, Read};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime};

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

// DISM é a etapa mais demorada: recebe 55% do progresso em limpeza média
// (5 etapas) e 65% em limpeza pesada (6+ etapas). O avanço do DISM é
// limitado a 94% de seu peso até o processo encerrar, contemplando a pausa
// frequente após exibir 100%.
fn cleanup_progress_percent(current: u32, total: u32, is_dism: bool, dism_percent: f64) -> u32 {
    if total == 0 || current == 0 || current > total {
        return 0;
    }

    let dism_weight = if total >= 6 { 0.65 } else { 0.55 };
    let other_weight = (1.0 - dism_weight) / (total - 1).max(1) as f64;

    let progress = if is_dism {
        let stage_progress = dism_percent.clamp(0.0, 100.0) / 100.0 * 0.94;
        (current - 1) as f64 * other_weight + stage_progress * dism_weight
    } else {
        let completed_before = if current > 1 { current - 1 } else { 0 };
        completed_before as f64 * other_weight
    };

    (progress.clamp(0.0, 1.0) * 100.0).floor() as u32
}

#[cfg(target_os = "windows")]
static STORAGE_SCRIPT_COUNTER: AtomicU64 = AtomicU64::new(0);

static SAFETY_STATE: std::sync::OnceLock<Arc<Mutex<SafetyManager>>> = std::sync::OnceLock::new();

static OPERATION_START_GATE: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();

fn operation_start_gate() -> &'static Mutex<()> {
    OPERATION_START_GATE.get_or_init(|| Mutex::new(()))
}

static RESTORE_POINT_RESPONSES: std::sync::OnceLock<
    Mutex<std::collections::HashMap<String, mpsc::Sender<bool>>>,
> = std::sync::OnceLock::new();

fn restore_point_responses() -> &'static Mutex<std::collections::HashMap<String, mpsc::Sender<bool>>>
{
    RESTORE_POINT_RESPONSES.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

static RESET_BASE_RESPONSES: std::sync::OnceLock<
    Mutex<std::collections::HashMap<String, mpsc::Sender<bool>>>,
> = std::sync::OnceLock::new();

fn reset_base_responses() -> &'static Mutex<std::collections::HashMap<String, mpsc::Sender<bool>>> {
    RESET_BASE_RESPONSES.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
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
    let (key, details) = message.split_once(' ').unwrap_or((message, ""));
    let label = match key {
        "task-started" => "Tarefa iniciada",
        "snapshot-started" => "Captura de segurança iniciada",
        "snapshot-failed" => "Falha na captura de segurança",
        "restore-point-phase-started" => "Criação do ponto de restauração iniciada",
        "restore-point-phase-completed" => "Ponto de restauração criado",
        "restore-point-phase-failed" => "Falha ao criar o ponto de restauração",
        "task-cancelled-by-user-after-restore-point-failure" => "Tarefa cancelada após falha no ponto de restauração",
        "user-approved-proceeding-without-restore-point" => "Usuário autorizou continuar sem ponto de restauração",
        "task-executor-completed" => "Execução concluída",
        "task-executor-failed" => "Falha no executor da tarefa",
        "registry-operation-failed" => "Falha na operação do Registro",
        "restore-point" => "Operação de ponto de restauração",
        "started" => "Comando iniciado",
        "finished" => "Comando finalizado",
        "failed" => "Falha no comando",
        "journal" => "Persistência do histórico de alterações",
        "confirmation-timeout" => "Tempo limite da confirmação atingido",
        _ => return message.to_string(),
    };
    let details = details
        .replace("task_id=", "tarefa=")
        .replace("pid=", "processo=")
        .replace("command=", "comando=")
        .replace("args=", "argumentos=")
        .replace("status=", "resultado=")
        .replace("output_bytes=", "bytes_de_saida=")
        .replace("output=", "saída=")
        .replace("error=", "detalhe=")
        .replace("reason=", "motivo=")
        .replace("operation=", "operação=")
        .replace("persist_error=", "erro_de_persistência=");
    if key == "restore-point" {
        return if details.starts_with("started") {
            format!("Criação do ponto de restauração iniciada: {}", details.trim_start_matches("started").trim())
        } else if details.starts_with("completed") {
            "Ponto de restauração criado com sucesso.".to_string()
        } else if details.starts_with("failed") {
            format!("Falha ao criar o ponto de restauração: {}", details.trim_start_matches("failed").trim())
        } else {
            format!("Ponto de restauração: {details}")
        };
    }
    if key == "journal" && details.starts_with("persist failed") {
        return format!(
            "Falha ao salvar o histórico de alterações: {}",
            details.trim_start_matches("persist failed").trim()
        );
    }
    if key == "journal" && details.starts_with("update failed after task error:") {
        return format!(
            "Falha ao atualizar o histórico após um erro na tarefa: {}",
            details.trim_start_matches("update failed after task error:").trim()
        );
    }
    if details.is_empty() {
        label.to_string()
    } else {
        format!("{label}: {details}")
    }
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
                "[Compatibilidade] Não foi possível classificar o ambiente; tratando como restritivo e preservando a reversibilidade. Detalhe: {error}"
            ));
            true
        }
    }
}

fn cleanup_standard(
    task_id: &str,
    _execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
    apply_registry: &mut dyn FnMut(&str) -> Result<u64, String>,
) -> Result<String, String> {
    let output = cleanup_engine::cleanup_standard(
        on_output,
        is_cancelled,
        BROWSER_CLEANUP_ENABLED.load(Ordering::Relaxed),
    )?;
    let registry_keys_removed = apply_registry(task_id)?;
    let summary = "Faxina básica concluída: temporários, caches, histórico recente e lixeira.";
    let registry_metrics = format!("QBOA_METRICS files=0 bytes=0 registry={registry_keys_removed}");
    on_output(summary);
    on_output(&registry_metrics);
    Ok(format!("{output}\n{summary}\n{registry_metrics}"))
}

fn cleanup_heavy_temp(
    _execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    cleanup_engine::cleanup_heavy_temp(on_output, is_cancelled)
}

fn cleanup_windows_update(
    _execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    cleanup_engine::cleanup_windows_update(on_output, is_cancelled)
}

fn disable_telemetry_components(
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    run_powershell(
        r#"
$ErrorActionPreference = 'Stop'
$failures = New-Object System.Collections.Generic.List[string]
$changed = 0
$skipped = 0

foreach ($serviceName in @('DiagTrack', 'dmwappushservice')) {
    try {
        $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
        if (-not $service) { $skipped++; continue }
        if ($service.Status -ne 'Stopped') {
            Stop-Service -Name $serviceName -Force -ErrorAction Stop
        }
        Set-Service -Name $serviceName -StartupType Disabled -ErrorAction Stop
        $verified = Get-CimInstance Win32_Service -Filter "Name='$serviceName'" -ErrorAction Stop
        if (-not $verified -or $verified.StartMode -ne 'Disabled' -or $verified.State -ne 'Stopped') {
            throw 'O Windows não confirmou o serviço parado e desativado.'
        }
        $changed++
        Write-Output "$($serviceName): parado e desativado."
    } catch {
        $failures.Add("Serviço $($serviceName): $($_.Exception.Message)")
    }
}

$scheduledTasks = @(
    [pscustomobject]@{ TaskPath = '\Microsoft\Windows\Application Experience\'; TaskName = 'Microsoft Compatibility Appraiser' },
    [pscustomobject]@{ TaskPath = '\Microsoft\Windows\Application Experience\'; TaskName = 'ProgramDataUpdater' },
    [pscustomobject]@{ TaskPath = '\Microsoft\Windows\Autochk\'; TaskName = 'Proxy' },
    [pscustomobject]@{ TaskPath = '\Microsoft\Windows\Customer Experience Improvement Program\'; TaskName = 'Consolidator' },
    [pscustomobject]@{ TaskPath = '\Microsoft\Windows\Customer Experience Improvement Program\'; TaskName = 'UsbCeip' },
    [pscustomobject]@{ TaskPath = '\Microsoft\Windows\Customer Experience Improvement Program\'; TaskName = 'KernelCeipTask' },
    [pscustomobject]@{ TaskPath = '\Microsoft\Windows\DiskDiagnostic\'; TaskName = 'Microsoft-Windows-DiskDiagnosticDataCollector' },
    [pscustomobject]@{ TaskPath = '\Microsoft\Windows\Feedback\Siuf\'; TaskName = 'DmClient' },
    [pscustomobject]@{ TaskPath = '\Microsoft\Windows\Feedback\Siuf\'; TaskName = 'DmClientOnScenarioDownload' }
)
foreach ($entry in $scheduledTasks) {
    $taskPath = $entry.TaskPath
    $taskName = $entry.TaskName
    try {
        $scheduled = Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction SilentlyContinue
        if (-not $scheduled) { $skipped++; continue }
        if ($scheduled.State -ne 'Disabled') {
            Disable-ScheduledTask -InputObject $scheduled -ErrorAction Stop | Out-Null
        }
        $verified = Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction Stop
        if ($verified.State -ne 'Disabled') {
            throw 'O Windows não confirmou a tarefa como desativada.'
        }
        $changed++
        Write-Output "$($taskPath)$($taskName): desativada."
    } catch {
        $failures.Add("Tarefa agendada $($taskPath)$($taskName): $($_.Exception.Message)")
    }
}

Write-Output ("Telemetria: {0} componente(s) confirmado(s), {1} não encontrado(s), {2} falha(s)." -f $changed, $skipped, $failures.Count)
foreach ($failure in $failures) { Write-Output $failure }
if ($failures.Count -gt 0) { exit 2 }
Write-Output 'As configurações de telemetria disponíveis foram aplicadas e verificadas.'
"#,
        execution_id,
        on_output,
        is_cancelled,
    )
}

fn disable_advanced_privacy(
    execution_id: &str,
    on_output: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    run_powershell(
        r#"
$ErrorActionPreference = 'Stop'
$failures = New-Object System.Collections.Generic.List[string]
$changed = 0
$skipped = 0
$policies = @(
    @{ Hive = 'LocalMachine'; Path = 'SOFTWARE\Policies\Microsoft\Windows\WindowsAI'; Name = 'DisableAIDataAnalysis'; Value = 1 },
    @{ Hive = 'CurrentUser'; Path = 'SOFTWARE\Policies\Microsoft\Windows\WindowsAI'; Name = 'DisableAIDataAnalysis'; Value = 1 },
    @{ Hive = 'LocalMachine'; Path = 'SOFTWARE\Policies\Microsoft\Windows\WindowsAI'; Name = 'AllowRecallEnablement'; Value = 0 },
    @{ Hive = 'CurrentUser'; Path = 'SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot'; Name = 'TurnOffWindowsCopilot'; Value = 1 }
)
foreach ($policy in $policies) {
    try {
        $hive = [System.Enum]::Parse([Microsoft.Win32.RegistryHive], [string]$policy.Hive)
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, [Microsoft.Win32.RegistryView]::Default)
        $key = $base.CreateSubKey($policy.Path)
        if (-not $key) { throw 'Não foi possível abrir/criar a chave de política.' }
        $key.SetValue($policy.Name, [int]$policy.Value, [Microsoft.Win32.RegistryValueKind]::DWord)
        $key.Dispose()
        $verifyKey = $base.OpenSubKey($policy.Path)
        $actual = $verifyKey.GetValue($policy.Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $verifyKey.Dispose()
        if ($null -eq $actual -or [int]$actual -ne [int]$policy.Value) {
            throw 'A leitura de confirmação não corresponde ao valor solicitado.'
        }
        $changed++
        Write-Output "$($policy.Hive)\$($policy.Path)\$($policy.Name): aplicado e verificado."
        $base.Dispose()
    } catch {
        $failures.Add("$($policy.Hive)\$($policy.Path)\$($policy.Name): $($_.Exception.Message)")
    }
}

try {
    $feature = Get-WindowsOptionalFeature -Online -FeatureName 'Recall' -ErrorAction SilentlyContinue
    if (-not $feature) {
        $skipped++
        Write-Output 'Recall: recurso não disponível nesta edição/versão do Windows.'
    } elseif ($feature.State -eq 'Enabled') {
        Disable-WindowsOptionalFeature -Online -FeatureName 'Recall' -NoRestart -ErrorAction Stop | Out-Null
        $verifiedFeature = Get-WindowsOptionalFeature -Online -FeatureName 'Recall' -ErrorAction Stop
        if ($verifiedFeature.State -eq 'Enabled') {
            throw 'O Windows ainda informa que o recurso Recall está habilitado.'
        }
        $changed++
        Write-Output "Recall: desativado (estado final: $($verifiedFeature.State))."
    } else {
        $skipped++
        Write-Output "Recall: já estava desativado (estado: $($feature.State))."
    }
} catch {
    $failures.Add("Recurso Recall: $($_.Exception.Message)")
}

Write-Output ("Privacidade avançada: {0} configuração(ões) aplicadas, {1} item(ns) não aplicável(is), {2} falha(s)." -f $changed, $skipped, $failures.Count)
foreach ($failure in $failures) { Write-Output $failure }
if ($failures.Count -gt 0) { exit 2 }
Write-Output 'As políticas de privacidade avançada foram aplicadas e verificadas. Algumas podem exigir sair da sessão ou reiniciar o Windows.'
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
    // A cópia permite que cancel_task atualize o sinal de cancelamento enquanto
    // o executor do Registro aguarda o PowerShell. O journal é persistido pelo
    // executor e o estado em memória recebe o resultado ao final.
    let mut safety = safety_state().lock().unwrap().clone();
    let result = registry_executor::execute_registry_task(
        task_id,
        &catalog,
        execution_id,
        snapshot_location,
        &mut safety,
        &mut backend,
    );
    if result.is_ok() {
        safety_state().lock().unwrap().changes = safety.changes.clone();
    }
    result
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
            let is_progress_bar = line.starts_with('[')
                && line.ends_with(']')
                && line.contains('%')
                && line.chars().all(|character| {
                    character.is_ascii_digit() || "[]=%>.- , ".contains(character)
                });
            !is_progress_bar
                && !line.contains("QBOA_METRICS")
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
    confirm_reset_base: &mut dyn FnMut(&str) -> bool,
) -> Result<String, String> {
    if is_cancelled() {
        return Err("Tudo bem, a tarefa foi interrompida a seu pedido.".to_string());
    }
    match task.id.as_str() {
        "limpeza_leve" => cleanup_standard(
            &task.id,
            execution_id,
            on_output,
            is_cancelled,
            apply_registry,
        ),
        "limpeza_media" => {
            on_output("[Etapa 1/4] Executando a limpeza segura de temporários, caches básicos e lixeira...");
            let part_1 = cleanup_standard(
                &task.id,
                execution_id,
                on_output,
                is_cancelled,
                apply_registry,
            )?;
            on_output("[Etapa 2/4] Limpando somente o cache de downloads do Windows Update...");
            let part_2 = cleanup_windows_update(execution_id, on_output, is_cancelled)?;
            on_output(
                "[Etapa 3/4] Limpando caches regeneráveis conhecidos dos aplicativos Adobe...",
            );
            let part_3 = cleanup_engine::cleanup_adobe_caches(on_output, is_cancelled)?;
            on_output("[Etapa 4/4] Removendo somente atalhos quebrados e associações inválidas confirmadas...");
            let findings = apply_cleanup_findings("complete", on_output);
            Ok(format!(
                "{part_1}\n{part_2}\n{part_3}\n{}",
                findings_summary(&findings)
            ))
        }
        "limpeza_pesada" => {
            let conservative_repair = report_system_compatibility(on_output);
            if conservative_repair {
                on_output("[Compatibilidade] Ambiente modificado/restritivo detectado. O DISM /ResetBase será omitido para preservar a possibilidade de reverter atualizações.");
            }
            on_output("[Etapa 1/9] Limpando temporários, caches básicos e lixeira...");
            let part_1 = cleanup_standard(
                &task.id,
                execution_id,
                on_output,
                is_cancelled,
                apply_registry,
            )?;
            on_output("[Etapa 2/9] Limpando caches Adobe regeneráveis conhecidos...");
            let part_adobe = cleanup_engine::cleanup_adobe_caches(on_output, is_cancelled)?;
            on_output(
                "[Etapa 3/9] Limpando Prefetch, temporários avançados e relatórios de falha...",
            );
            let part_2 = cleanup_heavy_temp(execution_id, on_output, is_cancelled)?;
            on_output("[Etapa 4/9] Processando o cache de downloads do Windows Update...");
            let part_3 = cleanup_windows_update(execution_id, on_output, is_cancelled)?;
            on_output("[Etapa 5/9] Removendo atalhos quebrados, associações inválidas e entradas órfãs confirmadas...");
            let findings = apply_cleanup_findings("deep", on_output);
            on_output("[Etapa 6/9] Verificando a integridade da imagem do Windows...");
            let part_4 = run_system_command(
                "dism",
                &["/Online", "/Cleanup-Image", "/ScanHealth"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            on_output("[Etapa 7/9] Reparando a imagem do Windows com DISM RestoreHealth...");
            let part_6 = run_system_command(
                "dism",
                &["/Online", "/Cleanup-Image", "/RestoreHealth"],
                execution_id,
                on_output,
                is_cancelled,
            )?;
            let part_5 = if conservative_repair {
                on_output("[Etapa 8/9] /ResetBase omitido: o ambiente foi classificado como modificado/restritivo.");
                "DISM /ResetBase omitido por compatibilidade; as atualizações continuam reversíveis.".to_string()
            } else if confirm_reset_base(
                "A próxima etapa executará DISM /StartComponentCleanup /ResetBase. Depois dela, não será possível desinstalar as atualizações do Windows que já estão instaladas. Se você não autorizar, a limpeza pesada será cancelada antes desta operação. Deseja executar o /ResetBase?"
            ) {
                on_output("[Etapa 8/9] Executando DISM StartComponentCleanup /ResetBase após confirmação explícita...");
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
            } else {
                on_output("[Etapa 8/9] Usuário não autorizou o /ResetBase; cancelando a limpeza pesada antes da operação irreversível.");
                return Err("Limpeza pesada cancelada: o DISM /ResetBase exige autorização explícita.".to_string());
            };
            on_output("[Etapa 9/9] Verificando arquivos protegidos com SFC...");
            let part_7 =
                run_system_command("sfc", &["/scannow"], execution_id, on_output, is_cancelled)?;
            Ok(format!(
                "{part_1}\n{part_adobe}\n{part_2}\n{part_3}\n{}\n{part_4}\n{part_5}\n{part_6}\n{part_7}",
                findings_summary(&findings)
            ))
        }
        "desengordurar_telemetria" => {
            apply_registry(&task.id)?;
            if is_cancelled() {
                return Err("Tarefa cancelada pelo usuário.".to_string());
            }
            disable_telemetry_components(execution_id, on_output, is_cancelled)
        }
        "desengordurar_telemetria_avancada" => {
            disable_advanced_privacy(execution_id, on_output, is_cancelled)
        }
        "desengordurar_tarefas" => disable_diagnostic_tasks(execution_id, on_output, is_cancelled),
        "limpeza_windows_update" => cleanup_windows_update(execution_id, on_output, is_cancelled),
        "limpeza_historico_navegadores" => cleanup_engine::cleanup_browser_history(
            on_output,
            is_cancelled,
            BROWSER_CLEANUP_ENABLED.load(Ordering::Relaxed),
        ),
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
async fn respond_reset_base_confirmation(
    execution_id: String,
    proceed: bool,
) -> Result<(), String> {
    let sender = reset_base_responses()
        .lock()
        .unwrap()
        .remove(&execution_id)
        .ok_or_else(|| {
            "A confirmação do DISM /ResetBase expirou ou não está mais pendente.".to_string()
        })?;
    sender
        .send(proceed)
        .map_err(|_| "Não foi possível enviar a resposta da confirmação do /ResetBase.".to_string())
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
    let execution = start_task(app, task.clone(), shutdown_on_complete)?;
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

fn start_task(
    app: AppHandle,
    task: Task,
    shutdown_on_complete: bool,
) -> Result<safety::ExecutionHandle, String> {
    let _start_guard = operation_start_gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if memory_test::session::is_active() {
        return Err(
            "O teste de integridade da RAM está em andamento. Aguarde ou cancele a sessão antes de iniciar outra tarefa."
                .to_string(),
        );
    }
    if cpu_diagnostic::is_active() {
        return Err(
            "O diagnóstico de CPU está em andamento. Aguarde ou cancele o teste antes de iniciar uma tarefa de limpeza/manutenção."
                .to_string(),
        );
    }
    let task_id_for_spawn = task.id.clone();
    let app_for_spawn = app.clone();
    let task_name = task.name.clone();
    let task_risk = task_risk_label(&task).to_string();
    let task_for_spawn = task.clone();
    let task_risk_for_spawn = task_risk.clone();

    let execution = safety_state()
        .lock()
        .unwrap()
        .try_register_execution(&task_id_for_spawn)?;
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
            let snapshot_manager = safety_state().lock().unwrap().clone();
            match snapshot_manager.capture_snapshot(&task_for_spawn.id) {
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
            let mut restore_point_manager = safety_state().lock().unwrap().clone();
            match restore_point_manager.create_restore_point(&format!(
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
                            "Não consegui criar um ponto de restauração: {err}\nQuer continuar mesmo assim?"
                        ),
                        false,
                        None,
                        &task_risk_for_spawn,
                    );
                    let proceed = match receiver.recv_timeout(Duration::from_secs(300)) {
                        Ok(response) => response,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            write_execution_log(
                                &execution_id_for_spawn,
                                "confirmation-timeout tipo=ponto-de-restauracao; decisão=cancelar",
                            );
                            false
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => false,
                    };
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
                            "Tudo bem, cancelei. O ponto de restauração não foi criado.",
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
            "Preparando tudo com cuidado e verificando possíveis riscos.",
            true,
            Some("30%"),
            &task_risk_for_spawn,
        );

        let app_for_output = app_for_spawn.clone();
        let task_id_for_output = task_id_for_spawn.clone();
        let execution_id_for_output = execution_id_for_spawn.clone();
        let task_risk_for_output = task_risk_for_spawn.clone();
        let mut streamed_output = String::new();
        let mut cleanup_step: Option<(u32, u32)> = None;
        let mut dism_percent = 0u32;
        let mut dism_active = false;
        let mut dism_stage_label = String::new();
        let mut on_output = |line: &str| {
            streamed_output.push_str(line);
            streamed_output.push('\n');

            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("[Etapa ") {
                if let Some((position, description)) = rest.split_once("] ") {
                    if let Some((current, total)) = position.split_once('/') {
                        if let (Ok(current), Ok(total)) =
                            (current.parse::<u32>(), total.parse::<u32>())
                        {
                            if total > 0 && current > 0 && current <= total {
                                cleanup_step = Some((current, total));
                                dism_percent = 0;
                                dism_active = description.to_ascii_lowercase().contains("dism")
                                    || description.to_ascii_lowercase().contains("imagem")
                                    || description
                                        .to_ascii_lowercase()
                                        .contains("componentes substituídos");
                                dism_stage_label = description.trim_end_matches("...").to_string();
                                let percent =
                                    cleanup_progress_percent(current, total, dism_active, 0.0);
                                let friendly = description.trim_end_matches("...");
                                emit_event(
                                    &app_for_output,
                                    "task-progress",
                                    &task_id_for_output,
                                    &execution_id_for_output,
                                    &format!("Passo {current}/{total} · {friendly}"),
                                    true,
                                    Some(&format!("{percent}%")),
                                    &task_risk_for_output,
                                );
                            }
                        }
                    }
                }
            } else if dism_active {
                if let Some(percent_pos) = trimmed.find('%') {
                    let prefix = &trimmed[..percent_pos];
                    let number: String = prefix
                        .chars()
                        .rev()
                        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                    if let Ok(value) = number.replace(',', ".").parse::<f64>() {
                        let value = value.clamp(0.0, 100.0);
                        let rounded = value.round() as u32;
                        if rounded >= dism_percent {
                            dism_percent = rounded;
                            if let Some((current, total)) = cleanup_step {
                                let overall = cleanup_progress_percent(current, total, true, value);
                                emit_event(
                                    &app_for_output,
                                    "task-progress",
                                    &task_id_for_output,
                                    &execution_id_for_output,
                                    &format!(
                                        "Passo {current}/{total} · {} ({rounded}%)",
                                        dism_stage_label.trim_end_matches('.')
                                    ),
                                    true,
                                    Some(&format!("{overall}%")),
                                    &task_risk_for_output,
                                );
                            }
                        }
                    }
                }
            }

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

        let mut confirm_reset_base = |message: &str| {
            let (sender, receiver) = mpsc::channel();
            reset_base_responses()
                .lock()
                .unwrap()
                .insert(execution_id_for_spawn.clone(), sender);
            emit_event(
                &app_for_spawn,
                "reset-base-confirmation-required",
                &task_id_for_spawn,
                &execution_id_for_spawn,
                message,
                false,
                None,
                &task_risk_for_spawn,
            );
            let proceed = match receiver.recv_timeout(Duration::from_secs(300)) {
                Ok(response) => response,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    write_execution_log(
                        &execution_id_for_spawn,
                        "confirmation-timeout tipo=reset-base; decisão=cancelar",
                    );
                    false
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => false,
            };
            reset_base_responses()
                .lock()
                .unwrap()
                .remove(&execution_id_for_spawn);
            proceed
        };

        let execution_started = Instant::now();
        let execution = execute_task(
            &task_for_spawn,
            &execution_id_for_spawn,
            &mut on_output,
            &is_cancelled,
            &mut apply_registry,
            &mut confirm_reset_base,
        );
        drop(confirm_reset_base);
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
                    &format!("Prontinho! Tarefa concluída: {}", task_name),
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
                    "A tarefa parou antes do fim — {files_removed} arquivos removidos, {registry_entries_removed} chaves/entradas de Registro excluídas, {:.3} GB liberados (estimativa pelos arquivos removidos), tempo até a interrupção/falha: {elapsed_label}.", 
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
                    let mut cancellation_message = err.clone();
                    let rollback_result = if let Some(change) = registry_change.take() {
                        Some(safety_state().lock().unwrap().rollback_task(&change.id))
                    } else if task_for_spawn.reversible && task_for_spawn.rollback_on_failure {
                        let recorded = safety_state().lock().unwrap().record_change(
                            &execution_id_for_spawn,
                            &task_id_for_spawn,
                            &task_name,
                            true,
                            snapshot_location.as_deref(),
                            "cancelled_pending_rollback",
                        );
                        Some(match recorded {
                            Ok(change) => safety_state().lock().unwrap().rollback_task(&change.id),
                            Err(record_error) => safety_state()
                                .lock()
                                .unwrap()
                                .rollback_unrecorded_change(&record_error.change),
                        })
                    } else {
                        None
                    };
                    if let Some(result) = rollback_result {
                        match result {
                            Ok(message) => cancellation_message.push_str(&format!(
                                "\nAs alterações parciais foram revertidas: {message}"
                            )),
                            Err(rollback_error) => cancellation_message.push_str(&format!(
                                "\nAtenção: não foi possível confirmar a reversão de todas as alterações: {rollback_error}"
                            )),
                        }
                    }
                    safety_state()
                        .lock()
                        .unwrap()
                        .finish_execution(&execution_id_for_spawn, "cancelled");
                    emit_event(
                        &app_for_spawn,
                        "task-cancelled",
                        &task_id_for_spawn,
                        &execution_id_for_spawn,
                        &cancellation_message,
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
    Ok(execution)
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
            format!("Identificador da execução obrigatório ou ambíguo para a tarefa: {task_id}")
        })?;
    let execution = safety
        .execution_state
        .get(&execution_id)
        .ok_or_else(|| "A execução informada não existe mais.".to_string())?;
    if execution.task_id != task_id {
        return Err("A execução informada não pertence à tarefa solicitada.".to_string());
    }
    if safety.cancel_execution(&execution_id) {
        Ok(format!(
            "Tarefa marcada para cancelamento: {task_id} ({execution_id})"
        ))
    } else {
        Err(format!("A tarefa não está mais em execução: {task_id}"))
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
    if safety_state().lock().unwrap().is_task_running() {
        return Err(
            "Já existe uma operação em andamento. Aguarde a conclusão antes de iniciar um perfil."
                .to_string(),
        );
    }

    tauri::async_runtime::spawn(async move {
        for task in tasks {
            if let Err(error) = start_task(app.clone(), task.clone(), false) {
                emit_event(
                    &app,
                    "task-error",
                    &task.id,
                    "",
                    &error,
                    false,
                    None,
                    task_risk_label(&task),
                );
                break;
            }
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
    app: AppHandle,
    mode: diagnostics::DiagnosticMode,
) -> Result<diagnostics_store::StoredDiagnosticReport, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    let mut stored = diagnostics_store::StoredDiagnosticReport::new(mode);
    diagnostics_store::save(&app_data_dir, &stored)?;
    let started = Instant::now();
    let mut backend = diagnostics::WindowsDiagnosticBackend::default();
    let execution = diagnostics::run_with_checkpoint(&mut backend, mode, |snapshot| {
        stored.checkpoint(snapshot.clone());
        diagnostics_store::save(&app_data_dir, &stored)
    });
    match execution {
        Ok(report) => stored.apply_result(report, started.elapsed().as_millis() as u64),
        Err(error) => stored.fail(error, started.elapsed().as_millis() as u64),
    }
    diagnostics_store::save(&app_data_dir, &stored)?;
    Ok(stored)
}

#[tauri::command]
async fn get_diagnostic_reports(
    app: AppHandle,
) -> Result<Vec<diagnostics_store::StoredDiagnosticReport>, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    diagnostics_store::load_all(&app_data_dir)
}

#[cfg(target_os = "windows")]
fn execute_storage_script(mode: &str, disk_id: Option<&str>) -> Result<serde_json::Value, String> {
    if !matches!(mode, "list" | "test") {
        return Err("Modo de teste de armazenamento inválido.".into());
    }
    if let Some(id) = disk_id {
        if !id.split_once(':').is_some_and(|(number, letter)| {
            !number.is_empty()
                && number.bytes().all(|byte| byte.is_ascii_digit())
                && letter.len() == 1
                && letter.as_bytes()[0].is_ascii_uppercase()
        }) {
            return Err("Identificador de disco inválido.".into());
        }
    }

    let disk_id = disk_id.unwrap_or("");
    let counter = STORAGE_SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let script_path = std::env::temp_dir().join(format!(
        "qboa-storage-{}-{counter:016x}.ps1",
        std::process::id()
    ));
    let mut created_script = false;
    let execution = (|| -> Result<std::process::Output, String> {
        let mut script_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&script_path)
            .map_err(|error| {
                format!("Não foi possível criar o script temporário de armazenamento: {error}")
            })?;
        created_script = true;
        script_file
            .write_all(include_str!("../scripts/storage_diagnostic.ps1").as_bytes())
            .map_err(|error| {
                format!("Não foi possível gravar o script temporário de armazenamento: {error}")
            })?;
        drop(script_file);

        let mut command = Command::new("powershell.exe");
        configure_hidden_console(&mut command);
        command
            .env("QBOA_STORAGE_MODE", mode)
            .env("QBOA_STORAGE_DISK_ID", disk_id)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script_path)
            .output()
            .map_err(|error| {
                format!("Não foi possível iniciar o coletor de armazenamento: {error}")
            })
    })();
    if created_script {
        std::fs::remove_file(&script_path).map_err(|error| {
            format!("Não foi possível remover o script temporário de armazenamento: {error}")
        })?;
    }
    let output = execution?;
    if !output.status.success() {
        let stderr = decode_process_output(&output.stderr);
        let stdout = decode_process_output(&output.stdout);
        let details = if !stderr.trim().is_empty() {
            stderr
        } else {
            stdout
        };
        return Err(format!(
            "O coletor de armazenamento falhou: {}",
            details.trim()
        ));
    }
    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .map_err(|error| format!("O coletor de armazenamento retornou JSON inválido: {error}"))
}

#[cfg(not(target_os = "windows"))]
fn execute_storage_script(mode: &str, disk_id: Option<&str>) -> Result<serde_json::Value, String> {
    use serde_json::json;

    if mode == "list" {
        return Ok(json!([{
            "id": "0:C",
            "disk_number": 0,
            "model": "Disco fictício para testes de interface",
            "capacity_bytes": 256000000000u64,
            "volume": "C:\\",
            "free_bytes": 128000000000u64,
            "class": "nvme",
            "bus_type": "NVMe",
            "backend": "fake"
        }]));
    }
    if mode != "test" || disk_id != Some("0:C") {
        return Err("Identificador de disco inválido para o backend fictício.".into());
    }
    Ok(json!({
        "disk_id": "0:C",
        "disk_name": "Disco fictício para testes de interface",
        "volume": "C:\\",
        "capacity_bytes": 256000000000u64,
        "class": "nvme",
        "bus_type": "NVMe",
        "backend": "fake",
        "measurements": [
            {"kind":"seq_small_write","value":123.0,"unit":"MB/s","bytes_transferred":10485760,"duration_ms":100.0,"queue_depth":1,"block_size_bytes":131072,"reason":null},
            {"kind":"seq_small_read","value":130.0,"unit":"MB/s","bytes_transferred":10485760,"duration_ms":100.0,"queue_depth":1,"block_size_bytes":131072,"reason":null},
            {"kind":"seq_large_write","value":1700.0,"unit":"MB/s","bytes_transferred":134217728,"duration_ms":100.0,"queue_depth":8,"block_size_bytes":1048576,"reason":null},
            {"kind":"seq_large_read","value":2100.0,"unit":"MB/s","bytes_transferred":134217728,"duration_ms":100.0,"queue_depth":8,"block_size_bytes":1048576,"reason":null},
            {"kind":"random_small_write","value":12000.0,"unit":"IOPS","bytes_transferred":10485760,"duration_ms":100.0,"queue_depth":1,"block_size_bytes":4096,"reason":null},
            {"kind":"random_small_read","value":16000.0,"unit":"IOPS","bytes_transferred":10485760,"duration_ms":100.0,"queue_depth":1,"block_size_bytes":4096,"reason":null},
            {"kind":"random_large_write","value":80000.0,"unit":"IOPS","bytes_transferred":134217728,"duration_ms":100.0,"queue_depth":32,"block_size_bytes":4096,"reason":null},
            {"kind":"random_large_read","value":95000.0,"unit":"IOPS","bytes_transferred":134217728,"duration_ms":100.0,"queue_depth":32,"block_size_bytes":4096,"reason":null}
        ],
        "smart": {
            "usable": true,
            "source": "backend fictício (sem hardware real)",
            "predicted_failure": false,
            "reallocated": 0,
            "pending": 0,
            "uncorrectable": 0,
            "interface_crc_before": 0,
            "interface_crc_after": 0,
            "wear_percent": 4.0,
            "wear_limit_percent": 100.0,
            "temperature_c": 36.0,
            "pio_mode": false,
            "slow_sata_link": false,
            "usb_slow": false,
            "attributes": [],
            "reliability_metrics": []
        },
        "recommendation": "inconclusivo",
        "reasons": [],
        "execution_status": "simulated",
        "space_available": true,
        "issues": ["Dados simulados para desenvolvimento; nenhuma leitura ou medição de hardware foi executada."]
    }))
}

#[tauri::command]
fn list_storage_disks() -> Result<serde_json::Value, String> {
    execute_storage_script("list", None)
}

#[tauri::command]
fn run_storage_diagnostic(app: AppHandle, disk_id: String) -> Result<serde_json::Value, String> {
    let _start_guard = operation_start_gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if gpu_diagnostic::is_active() {
        return Err(
            "O teste de GPU está em andamento. Aguarde ou cancele antes de testar o armazenamento."
                .into(),
        );
    }
    if cpu_diagnostic::is_active() {
        return Err(
            "O diagnóstico de CPU está em andamento. Aguarde ou cancele o teste antes de testar o armazenamento."
                .to_string(),
        );
    }
    if memory_test::session::is_active() {
        return Err(
            "O teste de RAM está em andamento. Aguarde ou cancele a sessão antes de testar o armazenamento."
                .to_string(),
        );
    }
    let started_at = storage_diagnostic::timestamp_now();
    let mut result = execute_storage_script("test", Some(&disk_id))?;
    let mut report: storage_diagnostic::StorageReport = serde_json::from_value(result.clone())
        .map_err(|error| format!("Relatório de armazenamento incompleto: {error}"))?;
    storage_diagnostic::evaluate(&mut report);
    let object = result
        .as_object_mut()
        .ok_or_else(|| "Relatório de armazenamento não é um objeto JSON.".to_string())?;
    object.insert(
        "recommendation".into(),
        serde_json::to_value(report.recommendation)
            .map_err(|error| format!("Falha ao serializar estado interno: {error}"))?,
    );
    object.insert(
        "reasons".into(),
        serde_json::to_value(report.reasons)
            .map_err(|error| format!("Falha ao serializar evidências: {error}"))?,
    );
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    storage_diagnostic::persist_report(&app_data_dir, &mut result, started_at)?;
    Ok(result)
}

#[tauri::command]
fn get_storage_diagnostic_reports(app: AppHandle) -> Result<serde_json::Value, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    storage_diagnostic::load_stored_reports(&app_data_dir)
}

#[tauri::command]
async fn start_memory_test(
    app: AppHandle,
    profile: memory_test::runner::MemoryTestProfile,
    memory_budget_bytes: Option<u64>,
    preferred_block_bytes: Option<usize>,
    seed: Option<u64>,
) -> Result<memory_test::session::MemoryTestSessionSnapshot, String> {
    let _start_guard = operation_start_gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if safety_state().lock().unwrap().is_task_running() {
        return Err("Já existe uma tarefa do QBoa em andamento. Aguarde sua conclusão antes de testar a RAM.".to_string());
    }
    if gpu_diagnostic::is_active() {
        return Err(
            "O teste de GPU está em andamento. Aguarde ou cancele antes de iniciar o teste de RAM."
                .into(),
        );
    }
    if cpu_diagnostic::is_active() {
        return Err(
            "O teste de CPU está em andamento. Aguarde ou cancele antes de iniciar o teste de RAM."
                .to_string(),
        );
    }
    let mut config = memory_test::runner::MemoryTestConfig::for_profile(profile);
    if let Some(value) = memory_budget_bytes {
        config.memory_budget_bytes = value;
    }
    if let Some(value) = preferred_block_bytes {
        config.preferred_block_bytes = value;
    }
    if let Some(value) = seed {
        config.seed = value;
    }
    if profile == memory_test::runner::MemoryTestProfile::Custom {
        let total_physical_bytes = memory_inventory::collect()?.total_physical_bytes;
        if config.memory_budget_bytes > total_physical_bytes {
            return Err(
                "O orçamento personalizado não pode superar o total de RAM física detectada."
                    .to_string(),
            );
        }
    }
    memory_test::session::start(app, config)
}

#[tauri::command]
fn get_memory_test_session() -> Option<memory_test::session::MemoryTestSessionSnapshot> {
    memory_test::session::get_session()
}

#[tauri::command]
fn cancel_memory_test(
    app: AppHandle,
    session_id: Option<String>,
) -> Result<memory_test::session::MemoryTestSessionSnapshot, String> {
    memory_test::session::cancel(&app, session_id.as_deref())
}

#[tauri::command]
async fn get_memory_inventory() -> Result<memory_inventory::MemoryInventory, String> {
    memory_inventory::collect()
}

#[tauri::command]
async fn get_cpu_inventory(app: AppHandle) -> Result<cpu_diagnostic::CpuInventory, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    cpu_diagnostic::collect_inventory(&app_data_dir)
}

#[tauri::command]
fn install_pawnio(app: AppHandle) -> Result<String, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    cpu_diagnostic::install_pawnio(&app_data_dir)
}

#[tauri::command]
fn start_cpu_test(
    app: AppHandle,
    profile: cpu_diagnostic::CpuTestProfile,
) -> Result<cpu_diagnostic::CpuTestSessionSnapshot, String> {
    let _start_guard = operation_start_gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if safety_state().lock().unwrap().is_task_running() {
        return Err(
            "Há uma tarefa de limpeza/manutenção em andamento. Aguarde sua conclusão antes de iniciar um teste de CPU."
                .to_string(),
        );
    }
    if gpu_diagnostic::is_active() {
        return Err(
            "O teste de GPU está em andamento. Aguarde ou cancele antes de iniciar o teste de CPU."
                .into(),
        );
    }
    if memory_test::session::is_active() {
        return Err(
            "O teste de RAM está em andamento. Aguarde ou cancele antes de iniciar o teste de CPU."
                .to_string(),
        );
    }
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    let inventory = cpu_diagnostic::collect_inventory(&app_data_dir)?;
    if !inventory.sensor_available {
        return Err(inventory.sensor_note);
    }
    cpu_diagnostic::start(app, app_data_dir, profile, inventory)
}

#[tauri::command]
fn get_cpu_test_session() -> Option<cpu_diagnostic::CpuTestSessionSnapshot> {
    cpu_diagnostic::get_session()
}

#[tauri::command]
fn cancel_cpu_test(
    app: AppHandle,
    session_id: Option<String>,
) -> Result<cpu_diagnostic::CpuTestSessionSnapshot, String> {
    cpu_diagnostic::cancel(&app, session_id.as_deref())
}

#[tauri::command]
fn get_cpu_diagnostic_reports(
    app: AppHandle,
) -> Result<Vec<cpu_diagnostic::CpuTestReport>, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    cpu_diagnostic::load_reports(&app_data_dir)
}

#[tauri::command]
fn get_gpu_inventory(app: AppHandle) -> Result<gpu_diagnostic::GpuInventory, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    gpu_diagnostic::collect_inventory(&app_data_dir)
}

#[tauri::command]
fn start_gpu_test(
    app: AppHandle,
    profile: gpu_diagnostic::GpuTestProfile,
) -> Result<gpu_diagnostic::GpuTestSessionSnapshot, String> {
    let _start_guard = operation_start_gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if safety_state().lock().unwrap().is_task_running() {
        return Err("Há uma tarefa de limpeza/manutenção em andamento. Aguarde sua conclusão antes de iniciar o teste de GPU.".into());
    }
    if memory_test::session::is_active() || cpu_diagnostic::is_active() {
        return Err("Outro teste de hardware está em andamento. Aguarde ou cancele a sessão atual antes de iniciar o teste de GPU.".into());
    }
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    let inventory = gpu_diagnostic::collect_inventory(&app_data_dir)?;
    if !inventory.sensor_available {
        return Err(inventory.sensor_note);
    }
    gpu_diagnostic::start(app, app_data_dir, profile, inventory)
}

#[tauri::command]
fn get_gpu_test_session() -> Option<gpu_diagnostic::GpuTestSessionSnapshot> {
    gpu_diagnostic::get_session()
}

#[tauri::command]
fn cancel_gpu_test(
    app: AppHandle,
    session_id: Option<String>,
) -> Result<gpu_diagnostic::GpuTestSessionSnapshot, String> {
    gpu_diagnostic::cancel(&app, session_id.as_deref())
}

#[tauri::command]
fn get_gpu_diagnostic_reports(
    app: AppHandle,
) -> Result<Vec<gpu_diagnostic::GpuTestReport>, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Não foi possível localizar a pasta de dados do QBoa: {error}"))?;
    gpu_diagnostic::load_reports(&app_data_dir)
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
async fn get_service_profile_state() -> Result<ServiceProfileState, String> {
    let mut backend = service_profiles::WindowsServiceProfileBackend::default();
    let system = backend.detect_system()?;
    service_profiles::capture_initial_baseline(&mut backend, &system)?;
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
    changes: Vec<service_profiles::ServiceProfileOverride>,
) -> Result<(), String> {
    let mut backend = service_profiles::WindowsServiceProfileBackend::default();
    let system = backend.detect_system()?;
    let service_profile = service_profiles::system_profile(&system)?;
    service_profiles::apply_profile_with_overrides(&mut backend, &service_profile, profile, changes)
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

#[tauri::command]
fn finish_startup(app: AppHandle) -> Result<(), String> {
    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| "A janela principal do QBoa não foi encontrada.".to_string())?;
    if let Some(splash) = app.get_webview_window("splashscreen") {
        let _ = splash.close();
    }
    main_window
        .show()
        .map_err(|error| format!("Não foi possível exibir a janela principal: {error}"))?;
    main_window
        .set_focus()
        .map_err(|error| format!("Não foi possível ativar a janela principal: {error}"))?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            finish_startup,
            get_tasks,
            run_task,
            respond_restore_point_failure,
            respond_reset_base_confirmation,
            get_running_browsers,
            prepare_browser_cleanup,
            cancel_task,
            run_preset,
            get_debloat_profile_tasks,
            create_restore_point,
            rollback_session,
            get_session_history,
            run_diagnostic_check,
            complete_diagnostic::update_complete_diagnostic_report,
            get_diagnostic_reports,
            list_storage_disks,
            run_storage_diagnostic,
            get_storage_diagnostic_reports,
            start_memory_test,
            get_memory_test_session,
            cancel_memory_test,
            get_memory_inventory,
            get_cpu_inventory,
            install_pawnio,
            start_cpu_test,
            get_cpu_test_session,
            cancel_cpu_test,
            get_cpu_diagnostic_reports,
            get_gpu_inventory,
            start_gpu_test,
            get_gpu_test_session,
            cancel_gpu_test,
            get_gpu_diagnostic_reports,
            get_capability_inventory,
            list_windows_services,
            manage_windows_service,
            get_service_profile_state,
            preview_service_profile,
            apply_service_profile,
            restore_windows_service_defaults,
            undo_service_profile_changes
        ])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o Qboa Digital");
}
