use std::env;
use std::path::PathBuf;
use std::process::Command;

fn build_hardware_monitor(manifest_dir: &PathBuf) {
    let project = manifest_dir.join("hardware-monitor").join("QBoa.HardwareMonitor.csproj");
    let output_dir = manifest_dir.join("binaries");
    std::fs::create_dir_all(&output_dir).expect("não foi possível criar src-tauri/binaries");
    println!("cargo:rerun-if-changed=hardware-monitor/Program.cs");
    println!("cargo:rerun-if-changed=hardware-monitor/QBoa.HardwareMonitor.csproj");

    let status = Command::new("dotnet")
        .arg("publish").arg(&project)
        .args(["--configuration", "Release", "--runtime", "win-x64", "--self-contained", "true",
            "-p:PublishSingleFile=true", "-p:IncludeNativeLibrariesForSelfExtract=true",
            "-p:EnableCompressionInSingleFile=true", "-p:DebugType=None", "-p:DebugSymbols=false", "--output"])
        .arg(&output_dir).status()
        .expect("não foi possível executar dotnet. Instale o .NET 10 SDK para compilar o coletor térmico do QBoa.");
    if !status.success() { panic!("falha ao compilar o coletor térmico LibreHardwareMonitor integrado"); }
    if !output_dir.join("qboa-hardware-monitor.exe").is_file() {
        panic!("dotnet publish não gerou binaries/qboa-hardware-monitor.exe");
    }
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR ausente"));
    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        build_hardware_monitor(&manifest_dir);
        let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("qboa.manifest"));
        let attrs = tauri_build::Attributes::new().windows_attributes(windows);
        tauri_build::try_build(attrs).expect("falha ao embutir o manifest do Windows");
    } else {
        tauri_build::build();
    }
}
