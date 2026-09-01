fn main() {
    // Embute o manifest do Windows (qboa.manifest) no .exe final, forçando
    // elevação UAC automática — necessário pra rodar dism/sfc/cleanmgr/chkdsk.
    #[cfg(target_os = "windows")]
    {
        let windows =
            tauri_build::WindowsAttributes::new().app_manifest(include_str!("qboa.manifest"));
        let attrs = tauri_build::Attributes::new().windows_attributes(windows);
        tauri_build::try_build(attrs).expect("falha ao embutir o manifest do Windows");
    }

    #[cfg(not(target_os = "windows"))]
    {
        tauri_build::build();
    }
}
