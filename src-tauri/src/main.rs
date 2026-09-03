// Qboa Digital — backend Tauri
//
// A lógica de execução foi separada em duas camadas:
// 1) resolução do task_id -> ação concreta
// 2) execução do comando real, com helpers compartilhados
//
// Isso facilita manter a lista de tarefas num único lugar, reduzir duplicação e
// preparar o app para novos itens do `.bat`/Windows sem espalhar `match` por
// vários blocos de código.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::process::{Command, Output};

#[derive(Serialize, Debug, PartialEq, Eq)]
struct TaskResult {
    ok: bool,
    output: String,
}

fn process_result(out: Output) -> TaskResult {
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let combined = format!("{stdout}{stderr}");

    TaskResult {
        ok: out.status.success(),
        output: combined,
    }
}

fn run_command(cmd: &str, args: &[&str]) -> TaskResult {
    match Command::new(cmd).args(args).output() {
        Ok(out) => process_result(out),
        Err(e) => TaskResult {
            ok: false,
            output: format!("Falha ao executar '{cmd}': {e}"),
        },
    }
}

fn run_powershell(script: &str) -> TaskResult {
    run_command(
        "powershell",
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
    )
}

fn cleanup_standard() -> TaskResult {
    run_powershell(
        r#"
$ErrorActionPreference = 'SilentlyContinue'

# Temporarios do usuario e do Windows.
foreach ($path in @($env:TEMP, "$env:WINDIR\Temp")) {
    if (Test-Path $path) {
        Remove-Item -Path "$path\*" -Recurse -Force
    }
}

# Somente caches descartaveis; dados persistentes, cookies e sessao ficam intactos.
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
        Get-ChildItem -LiteralPath $root -Directory -Recurse -Force | ForEach-Object {
            if ($cacheNames -contains $_.Name -or $_.FullName -match '\\Service Worker\\CacheStorage$') {
                Remove-Item -LiteralPath $_.FullName -Recurse -Force
            }
        }
    }
}

# MRUs do Windows: remove apenas historicos de uso, nao arquivos pessoais.
$mruKeys = @(
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\RecentDocs',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\OpenSavePidlMRU',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\LastVisitedPidlMRU',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Map Network Drive MRU',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery'
)
foreach ($key in $mruKeys) {
    if (Test-Path $key) {
        Remove-ItemProperty -Path $key -Name '*' -ErrorAction SilentlyContinue
        Get-ChildItem -Path $key -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force
    }
}
Remove-Item -LiteralPath "$env:APPDATA\Microsoft\Windows\Recent\*" -Force -ErrorAction SilentlyContinue
Clear-RecycleBin -Force -ErrorAction SilentlyContinue
Write-Output 'Limpeza padrão concluída: temporários, caches, MRUs e lixeira.'
"#,
    )
}

fn cleanup_heavy_temp() -> TaskResult {
    run_powershell(
        r#"$paths = @("$env:TEMP", "$env:WINDIR\Temp", "$env:WINDIR\Minidump", "$env:WINDIR\LiveKernelReports"); foreach ($p in $paths) { if (Test-Path $p) { Get-ChildItem $p -Recurse -Force -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue; } }; Clear-RecycleBin -Force -ErrorAction SilentlyContinue; Write-Output 'Limpeza pesada concluída.'"#,
    )
}

fn disable_telemetry() -> TaskResult {
    run_powershell(
        r#"
$ErrorActionPreference = 'SilentlyContinue'

function Set-Dword {
    param([string]$Path, [string]$Name, [int]$Value)
    New-Item -Path $Path -Force | Out-Null
    New-ItemProperty -Path $Path -Name $Name -PropertyType DWord -Value $Value -Force | Out-Null
}

$telemetryKeys = @(
    @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'AllowTelemetry', 0),
    @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'AllowDeviceNameInTelemetry', 0),
    @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection', 'AllowTelemetry', 0),
    @('HKLM:\SOFTWARE\Wow6432Node\Microsoft\Windows\CurrentVersion\Policies\DataCollection', 'AllowTelemetry', 0),
    @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'DoNotShowFeedbackNotifications', 1),
    @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection', 'DisableTelemetryOptInChangeNotification', 1),
    @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI', 'DisableAIDataAnalysis', 1),
    @('HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI', 'TurnOffWindowsCopilot', 1),
    @('HKCU:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI', 'DisableAIDataAnalysis', 1),
    @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'EnableRecallOnDevice', 0),
    @('HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'EnableRecallOnDevice', 0)
)
foreach ($key in $telemetryKeys) {
    Set-Dword -Path $key[0] -Name $key[1] -Value $key[2]
}

foreach ($serviceName in @('DiagTrack', 'dmwappushservice')) {
    $service = Get-Service -Name $serviceName
    if ($service) {
        Stop-Service -Name $serviceName -Force
        Set-Service -Name $serviceName -StartupType Disabled
    }
}

$telemetryTasks = @(
    '\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser',
    '\Microsoft\Windows\Application Experience\ProgramDataUpdater',
    '\Microsoft\Windows\Autochk\Proxy',
    '\Microsoft\Windows\Customer Experience Improvement Program\Consolidator',
    '\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip',
    '\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector',
    '\Microsoft\Windows\Feedback\Siuf\DmClient',
    '\Microsoft\Windows\Feedback\Siuf\DmClientOnScenarioDownload'
)
foreach ($task in $telemetryTasks) {
    Disable-ScheduledTask -TaskName $task | Out-Null
}

$recallFeature = Get-WindowsOptionalFeature -Online -FeatureName 'Recall'
if ($recallFeature -and $recallFeature.State -eq 'Enabled') {
    Disable-WindowsOptionalFeature -Online -FeatureName 'Recall' -NoRestart | Out-Null
}

Write-Output 'Telemetria, Recall, serviços e tarefas de diagnóstico desativados.'
"#,
    )
}

fn disable_diagnostic_tasks() -> TaskResult {
    run_powershell(
        r#"Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskPath -match 'Application Experience|Customer Experience Improvement Program|Autochk\\Proxy' } | Disable-ScheduledTask -ErrorAction SilentlyContinue; Write-Output 'Tarefas de diagnóstico desativadas.'"#,
    )
}

fn resolve_task(task_id: &str) -> Option<fn() -> TaskResult> {
    match task_id {
        // ---------- LIMPEZA ----------
        "limpeza_leve" => Some(|| {
            let telemetry = disable_telemetry();
            if !telemetry.ok {
                return telemetry;
            }
            cleanup_standard()
        }),
        "limpeza_media" => Some(|| {
            let telemetry = disable_telemetry();
            if !telemetry.ok {
                return telemetry;
            }
            let standard = cleanup_standard();
            if !standard.ok {
                return standard;
            }
            run_command("cleanmgr", &["/sagerun:1"])
        }),
        "limpeza_pesada" => Some(|| {
            let telemetry = disable_telemetry();
            if !telemetry.ok {
                return telemetry;
            }
            let standard = cleanup_standard();
            if !standard.ok {
                return standard;
            }
            cleanup_heavy_temp()
        }),

        // ---------- DESENGORDURAR ----------
        "desengordurar_telemetria" => Some(disable_telemetry),
        "desengordurar_tarefas" => Some(disable_diagnostic_tasks),

        // ---------- DIAGNÓSTICO ----------
        "diagnostico_dism_scan" => {
            Some(|| run_command("dism", &["/Online", "/Cleanup-Image", "/ScanHealth"]))
        }
        "diagnostico_dism_restore" => {
            Some(|| run_command("dism", &["/Online", "/Cleanup-Image", "/RestoreHealth"]))
        }
        "diagnostico_sfc" => Some(|| run_command("sfc", &["/scannow"])),
        "diagnostico_informacoes" => Some(|| run_command("systeminfo", &[])),

        // ---------- COMPATIBILIDADE COM A VERSÃO ANTIGA ----------
        "quitacao_geral" => Some(|| run_command("cleanmgr", &["/sagerun:1"])),
        "debito_automatico" => Some(|| {
            run_powershell(
                r#"Write-Output 'Fallback: limpeza de temporários e lixeira do Windows.'; Clear-RecycleBin -Force -ErrorAction SilentlyContinue; Remove-Item -Recurse -Force "$env:TEMP\*" -ErrorAction SilentlyContinue;"#,
            )
        }),
        "disable_telemetry" => Some(disable_telemetry),
        "dism_scan" => Some(|| run_command("dism", &["/Online", "/Cleanup-Image", "/ScanHealth"])),
        "sfc_scan" => Some(|| run_command("sfc", &["/scannow"])),
        "systeminfo" => Some(|| run_command("systeminfo", &[])),
        _ => None,
    }
}

#[tauri::command]
fn run_task(task_id: String) -> TaskResult {
    match resolve_task(task_id.as_str()) {
        Some(task) => task(),
        None => TaskResult {
            ok: false,
            output: format!("Task desconhecida: {task_id}"),
        },
    }
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
        let result = run_task("task_inexistente".to_string());
        assert!(!result.ok);
        assert!(result.output.contains("Task desconhecida"));
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![run_task])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o Qboa Digital");
}
