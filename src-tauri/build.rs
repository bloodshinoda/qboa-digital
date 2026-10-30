use std::env;
use std::path::PathBuf;
use std::process::Command;

const PAWNIO_URL: &str =
    "https://github.com/namazso/PawnIO.Setup/releases/download/2.2.0/PawnIO_setup.exe";
const PAWNIO_SHA256: &str = "1F519A22E47187F70A1379A48CA604981C4FCF694F4E65B734AAA74A9FBA3032";

fn ensure_pawnio_installer(output_dir: &PathBuf) {
    let installer = output_dir.join("PawnIO_setup.exe");
    if installer.is_file() {
        let hash = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!(
                "(Get-FileHash -LiteralPath '{}' -Algorithm SHA256).Hash",
                installer.display()
            ))
            .output()
            .expect("não foi possível validar o instalador PawnIO com PowerShell");
        let actual = String::from_utf8_lossy(&hash.stdout)
            .trim()
            .to_ascii_uppercase();
        if hash.status.success() && actual == PAWNIO_SHA256 {
            return;
        }
        std::fs::remove_file(&installer)
            .expect("não foi possível remover instalador PawnIO inválido");
    }

    let script = format!(
        "$ErrorActionPreference = 'Stop'; Invoke-WebRequest -Uri '{}' -OutFile '{}'; $hash = (Get-FileHash -LiteralPath '{}' -Algorithm SHA256).Hash; if ($hash -ne '{}') {{ Remove-Item -LiteralPath '{}' -Force; throw 'SHA-256 do instalador PawnIO não confere' }}",
        PAWNIO_URL, installer.display(), installer.display(), PAWNIO_SHA256, installer.display()
    );
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .status()
        .expect("não foi possível baixar o instalador oficial PawnIO. Verifique a conexão com a internet.");
    if !status.success() {
        panic!(
            "não foi possível preparar o instalador PawnIO 2.2.0 ou a verificação SHA-256 falhou"
        );
    }
}

fn build_hardware_monitor(manifest_dir: &PathBuf) {
    let project = manifest_dir
        .join("hardware-monitor")
        .join("QBoa.HardwareMonitor.csproj");
    let output_dir = manifest_dir.join("binaries");
    std::fs::create_dir_all(&output_dir).expect("não foi possível criar src-tauri/binaries");
    ensure_pawnio_installer(&output_dir);
    println!("cargo:rerun-if-changed=binaries/PawnIO_setup.exe");
    println!("cargo:rerun-if-changed=hardware-monitor/Program.cs");
    println!("cargo:rerun-if-changed=hardware-monitor/QBoa.HardwareMonitor.csproj");

    let status = Command::new("dotnet")
        .arg("publish")
        .arg(&project)
        .args(["--configuration", "Release", "--runtime", "win-x64", "--self-contained", "true",
            "-p:PublishSingleFile=true", "-p:IncludeNativeLibrariesForSelfExtract=true",
            "-p:EnableCompressionInSingleFile=true", "-p:DebugType=None", "-p:DebugSymbols=false", "--output"])
        .arg(&output_dir)
        .status()
        .expect("não foi possível executar dotnet. Instale o .NET 10 SDK para compilar o coletor térmico do QBoa.");
    if !status.success() {
        panic!("falha ao compilar o coletor térmico LibreHardwareMonitor integrado");
    }
    if !output_dir.join("PawnIO_setup.exe").is_file() {
        panic!("instalador PawnIO não foi preparado em binaries/PawnIO_setup.exe");
    }
    if !output_dir.join("qboa-hardware-monitor.exe").is_file() {
        panic!("dotnet publish não gerou binaries/qboa-hardware-monitor.exe");
    }
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR ausente"));
    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        build_hardware_monitor(&manifest_dir);
        let windows =
            tauri_build::WindowsAttributes::new().app_manifest(include_str!("qboa.manifest"));
        let attrs = tauri_build::Attributes::new().windows_attributes(windows);
        tauri_build::try_build(attrs).expect("falha ao embutir o manifest do Windows");
    } else {
        tauri_build::build();
    }
}
