// Qboa Digital — backend Tauri
//
// Cada item da UI (Limpeza / Desengordurar / Diagnóstico) tem um `task_id` que
// bate com o `id` definido em src/data/qboa-structure.json. Esse backend só
// precisa saber traduzir esse id pro comando real do Windows e devolver o resultado.
//
// ATENÇÃO: isso só compila/roda de fato no Windows (dism, sfc, cleanmgr,
// chkdsk, dxdiag e msinfo32 não existem em outro SO). O esqueleto foi escrito
// pra já vir com a lógica de mapeamento pronta — falta só testar/ajustar
// flags no seu Windows real.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Command;
use serde::Serialize;

#[derive(Serialize)]
struct TaskResult {
    ok: bool,
    output: String,
}

fn run(cmd: &str, args: &[&str]) -> TaskResult {
    match Command::new(cmd).args(args).output() {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).to_string();
            combined.push_str(&String::from_utf8_lossy(&out.stderr));
            TaskResult {
                ok: out.status.success(),
                output: combined,
            }
        }
        Err(e) => TaskResult {
            ok: false,
            output: format!("Falha ao executar '{cmd}': {e}"),
        },
    }
}

fn run_powershell(script: &str) -> TaskResult {
    run(
        "powershell",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script],
    )
}

fn run_bleachbit_if_available(args: &[&str]) -> TaskResult {
    match Command::new("bleachbit_console.exe").args(args).output() {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).to_string();
            combined.push_str(&String::from_utf8_lossy(&out.stderr));
            TaskResult {
                ok: out.status.success(),
                output: combined,
            }
        }
        Err(_) => TaskResult {
            ok: false,
            output: "BleachBit não encontrado; o app continuará usando comandos nativos do Windows."
                .to_string(),
        },
    }
}

#[tauri::command]
fn run_task(task_id: String) -> TaskResult {
    match task_id.as_str() {
        // ---------- LIMPEZA ----------
        "limpeza_leve" => run_powershell(
            r#"$tempDirs = @($env:TEMP, "$env:WINDIR\Temp"); foreach ($d in $tempDirs) { if (Test-Path $d) { Remove-Item -Recurse -Force $d\* -ErrorAction SilentlyContinue; } }; Clear-RecycleBin -Force -ErrorAction SilentlyContinue; Write-Output 'Limpeza leve concluída.'"#,
        ),
        "limpeza_media" => run("cleanmgr", &["/sagerun:1"]),
        "limpeza_pesada" => run_powershell(
            r#"$paths = @("$env:TEMP", "$env:WINDIR\Temp", "$env:WINDIR\Minidump", "$env:WINDIR\LiveKernelReports"); foreach ($p in $paths) { if (Test-Path $p) { Get-ChildItem $p -Recurse -Force -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue; } }; Clear-RecycleBin -Force -ErrorAction SilentlyContinue; Write-Output 'Limpeza pesada concluída.'"#,
        ),
        "limpeza_bleachbit" => run_bleachbit_if_available(&["--clean", "system.recycle_bin", "system.tmp", "windows_defender.temp"]),

        // ---------- DESENGORDURAR ----------
        "desengordurar_telemetria" => run_powershell(
            r#"Get-Service DiagTrack,dmwappushservice -ErrorAction SilentlyContinue | Stop-Service -ErrorAction SilentlyContinue; sc.exe config DiagTrack start= disabled; sc.exe config dmwappushservice start= disabled; New-Item -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection' -Force | Out-Null; Set-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection' -Name AllowTelemetry -Type DWord -Value 0 -Force; Write-Output 'Telemetria desativada.'"#,
        ),
        "desengordurar_tarefas" => run_powershell(
            r#"Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskPath -match 'Application Experience|Customer Experience Improvement Program|Autochk\\Proxy' } | Disable-ScheduledTask -ErrorAction SilentlyContinue; Write-Output 'Tarefas de diagnóstico desativadas.'"#,
        ),
        "desengordurar_winutil" => run_powershell(
            r#"Write-Output 'Perfil conservador: ajuste de performance selecionado. Se o WinUtil estiver disponível, o preset pode ser executado manualmente.'"#,
        ),

        // ---------- DIAGNÓSTICO ----------
        "diagnostico_dism_scan" => run("dism", &["/Online", "/Cleanup-Image", "/ScanHealth"]),
        "diagnostico_dism_restore" => run("dism", &["/Online", "/Cleanup-Image", "/RestoreHealth"]),
        "diagnostico_sfc" => run("sfc", &["/scannow"]),
        "diagnostico_informacoes" => run("systeminfo", &[]),

        // ---------- COMPATIBILIDADE COM A VERSÃO ANTIGA ----------
        "quitacao_geral" => run("cleanmgr", &["/sagerun:1"]),
        "debito_automatico" => run_powershell(r#"Write-Output 'Fallback: limpeza de temporários e lixeira do Windows.'; Clear-RecycleBin -Force -ErrorAction SilentlyContinue; Remove-Item -Recurse -Force "$env:TEMP\*" -ErrorAction SilentlyContinue;"#),
        "disable_telemetry" => run_powershell(r#"Get-Service DiagTrack -ErrorAction SilentlyContinue | Stop-Service -ErrorAction SilentlyContinue; New-Item -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection' -Force | Out-Null; Set-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection' -Name AllowTelemetry -Type DWord -Value 0 -Force; Write-Output 'Telemetria reduzida.'"#),
        "dism_scan" => run("dism", &["/Online", "/Cleanup-Image", "/ScanHealth"]),
        "sfc_scan" => run("sfc", &["/scannow"]),
        "systeminfo" => run("systeminfo", &[]),

        other => TaskResult {
            ok: false,
            output: format!("Task desconhecida: {other}"),
        },
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![run_task])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o Qboa Digital");
}
