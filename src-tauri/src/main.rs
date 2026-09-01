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
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script],
    )
}

fn run_bleachbit_if_available(args: &[&str]) -> TaskResult {
    let candidates = ["bleachbit_console.exe", "bleachbit_console"];

    for candidate in candidates {
        match Command::new(candidate).args(args).output() {
            Ok(out) => return process_result(out),
            Err(_) => continue,
        }
    }

    TaskResult {
        ok: false,
        output: "BleachBit não encontrado; o app continuará usando os comandos nativos do Windows."
            .to_string(),
    }
}

fn cleanup_temp_and_recycle() -> TaskResult {
    run_powershell(
        r#"$tempDirs = @($env:TEMP, "$env:WINDIR\Temp"); foreach ($d in $tempDirs) { if (Test-Path $d) { Remove-Item -Recurse -Force $d\* -ErrorAction SilentlyContinue; } }; Clear-RecycleBin -Force -ErrorAction SilentlyContinue; Write-Output 'Limpeza leve concluída.'"#,
    )
}

fn cleanup_heavy_temp() -> TaskResult {
    run_powershell(
        r#"$paths = @("$env:TEMP", "$env:WINDIR\Temp", "$env:WINDIR\Minidump", "$env:WINDIR\LiveKernelReports"); foreach ($p in $paths) { if (Test-Path $p) { Get-ChildItem $p -Recurse -Force -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue; } }; Clear-RecycleBin -Force -ErrorAction SilentlyContinue; Write-Output 'Limpeza pesada concluída.'"#,
    )
}

fn disable_telemetry() -> TaskResult {
    run_powershell(
        r#"Get-Service DiagTrack,dmwappushservice -ErrorAction SilentlyContinue | Stop-Service -ErrorAction SilentlyContinue; sc.exe config DiagTrack start= disabled; sc.exe config dmwappushservice start= disabled; New-Item -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection' -Force | Out-Null; Set-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection' -Name AllowTelemetry -Type DWord -Value 0 -Force; Write-Output 'Telemetria desativada.'"#,
    )
}

fn disable_diagnostic_tasks() -> TaskResult {
    run_powershell(
        r#"Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskPath -match 'Application Experience|Customer Experience Improvement Program|Autochk\\Proxy' } | Disable-ScheduledTask -ErrorAction SilentlyContinue; Write-Output 'Tarefas de diagnóstico desativadas.'"#,
    )
}

fn conservative_profile() -> TaskResult {
    run_powershell(
        r#"Write-Output 'Perfil conservador: ajuste de performance selecionado. Se o WinUtil estiver disponível, o preset pode ser executado manualmente.'"#,
    )
}

fn resolve_task(task_id: &str) -> Option<fn() -> TaskResult> {
    match task_id {
        // ---------- LIMPEZA ----------
        "limpeza_leve" => Some(cleanup_temp_and_recycle),
        "limpeza_media" => Some(|| run_command("cleanmgr", &["/sagerun:1"])),
        "limpeza_pesada" => Some(cleanup_heavy_temp),
        "limpeza_bleachbit" => Some(|| run_bleachbit_if_available(&["--clean", "system.recycle_bin", "system.tmp", "windows_defender.temp"])),

        // ---------- DESENGORDURAR ----------
        "desengordurar_telemetria" => Some(disable_telemetry),
        "desengordurar_tarefas" => Some(disable_diagnostic_tasks),
        "desengordurar_winutil" => Some(conservative_profile),

        // ---------- DIAGNÓSTICO ----------
        "diagnostico_dism_scan" => Some(|| run_command("dism", &["/Online", "/Cleanup-Image", "/ScanHealth"])),
        "diagnostico_dism_restore" => Some(|| run_command("dism", &["/Online", "/Cleanup-Image", "/RestoreHealth"])),
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
