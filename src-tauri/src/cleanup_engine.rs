//! Motor nativo de limpeza do QBoa. Não executa PowerShell nem cleanmgr.
//! As métricas mantêm a semântica histórica: arquivos regulares e bytes medidos
//! antes da exclusão, e quantidade de entradas de primeiro nível removidas.

use std::path::{Path, PathBuf};

#[derive(Default, Debug, Clone, Copy)]
struct Metrics {
    entries: u64,
    files: u64,
    bytes: u64,
    failed: u64,
}

#[cfg(windows)]
mod platform {
    use super::{Metrics, Path, PathBuf};
    use std::ffi::{c_char, c_int, c_void, OsStr};
    use std::fs;
    use std::os::windows::ffi::OsStrExt;
    use std::process::Command;
    use std::os::windows::process::CommandExt;

    const SHERB_NOCONFIRMATION: u32 = 0x00000001;
    const SHERB_NOPROGRESSUI: u32 = 0x00000002;
    const SHERB_NOSOUND: u32 = 0x00000004;

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHEmptyRecycleBinW(hwnd: *mut c_void, root: *const u16, flags: u32) -> i32;
    }

    #[link(name = "winsqlite3")]
    unsafe extern "system" {
        fn sqlite3_open16(filename: *const c_void, db: *mut *mut c_void) -> c_int;
        fn sqlite3_exec(db: *mut c_void, sql: *const c_char, callback: *mut c_void, arg: *mut c_void, error: *mut *mut c_char) -> c_int;
        fn sqlite3_errmsg(db: *mut c_void) -> *const c_char;
        fn sqlite3_close(db: *mut c_void) -> c_int;
        fn sqlite3_free(value: *mut c_void);
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn cancelled(check: &dyn Fn() -> bool) -> Result<(), String> {
        if check() { Err("Limpeza cancelada pelo usuário.".into()) } else { Ok(()) }
    }

    fn tree_stats(path: &Path, check: &dyn Fn() -> bool) -> Result<(u64, u64), String> {
        cancelled(check)?;
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() { return Ok((0, 0)); }
        if meta.is_file() { return Ok((1, meta.len())); }
        if !meta.is_dir() { return Ok((0, 0)); }
        let mut files = 0u64;
        let mut bytes = 0u64;
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            cancelled(check)?;
            let entry = entry.map_err(|e| e.to_string())?;
            let child = entry.path();
            let meta = fs::symlink_metadata(&child).map_err(|e| e.to_string())?;
            if meta.file_type().is_symlink() { continue; }
            if meta.is_file() { files += 1; bytes = bytes.saturating_add(meta.len()); }
            else if meta.is_dir() {
                let (count, size) = tree_stats(&child, check)?;
                files += count;
                bytes = bytes.saturating_add(size);
            }
        }
        Ok((files, bytes))
    }

    fn remove_entry(path: &Path, metrics: &mut Metrics, check: &dyn Fn() -> bool) -> Result<(), String> {
        cancelled(check)?;
        let (files, bytes) = tree_stats(path, check)?;
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if meta.is_dir() && !meta.file_type().is_symlink() {
            fs::remove_dir_all(path).map_err(|e| e.to_string())?;
        } else {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        metrics.entries += 1;
        metrics.files += files;
        metrics.bytes = metrics.bytes.saturating_add(bytes);
        Ok(())
    }

    fn clear_recycle_bin() -> Result<(), String> {
        let result = unsafe {
            SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND)
        };
        if result >= 0 { Ok(()) } else { Err(format!("SHEmptyRecycleBinW retornou HRESULT 0x{:08X}", result as u32)) }
    }

    fn clean_roots(roots: &[PathBuf], label: &str, check: &dyn Fn() -> bool, ignore_root_errors: bool, on_output: &mut dyn FnMut(&str)) -> Result<(Metrics, String), String> {
        let mut metrics = Metrics::default();
        let mut output = Vec::new();
        for root in roots {
            cancelled(check)?;
            let root_meta = match fs::symlink_metadata(root) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => meta,
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) if ignore_root_errors => {
                    metrics.failed += 1;
                    let message = format!("Não foi possível acessar {}: {error}", root.display());
                    on_output(&message);
                    output.push(message);
                    continue;
                }
                Err(error) => return Err(format!("Não foi possível acessar {}: {error}", root.display())),
            };
            let _ = root_meta;
            let progress = format!("Limpando temporários: {}", root.display());
            on_output(&progress);
            output.push(progress);
            let entries = match fs::read_dir(root) {
                Ok(entries) => entries,
                Err(error) if ignore_root_errors => {
                    metrics.failed += 1;
                    output.push(format!("Não foi possível acessar {}: {error}", root.display()));
                    continue;
                }
                Err(error) => return Err(format!("Não foi possível enumerar {}: {error}", root.display())),
            };
            for entry in entries {
                cancelled(check)?;
                match entry {
                    Ok(entry) => {
                        let path = entry.path();
                        match remove_entry(&path, &mut metrics, check) {
                            Ok(()) => {}
                            Err(error) if error == "Limpeza cancelada pelo usuário." => return Err(error),
                            Err(error) => {
                                metrics.failed += 1;
                                let message = format!("Não foi possível remover: {} — {error}", path.display());
                                on_output(&message);
                                output.push(message);
                            }
                        }
                    }
                    Err(error) => { metrics.failed += 1; let message = format!("Falha ao enumerar item: {error}"); on_output(&message); output.push(message); }
                }
            }
        }
        output.push(format!("{label}: {} item(ns)/pasta(s) removido(s), {} falha(s).", metrics.entries, metrics.failed));
        Ok((metrics, output.join("\n")))
    }

    fn browser_roots() -> Vec<PathBuf> {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
        let roaming = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
        [
            local.join("Google/Chrome/User Data"),
            local.join("Microsoft/Edge/User Data"),
            local.join("BraveSoftware/Brave-Browser/User Data"),
            local.join("Vivaldi/User Data"),
            roaming.join("Opera Software/Opera Stable"),
            roaming.join("Opera Software/Opera GX Stable"),
            local.join("Yandex/YandexBrowser/User Data"),
            local.join("Chromium/User Data"),
            local.join("CocCoc/Browser/User Data"),
            local.join("Arc/User Data"),
            roaming.join("Mozilla/Firefox/Profiles"),
        ].into_iter().filter(|p| p.exists()).collect()
    }

    fn collect_named_dirs(root: &Path, names: &[&str], found: &mut Vec<PathBuf>, depth: usize, check: &dyn Fn() -> bool) -> Result<(), String> {
        cancelled(check)?;
        if depth > 16 { return Ok(()); }
        let root_meta = match fs::symlink_metadata(root) { Ok(v) => v, Err(_) => return Ok(()) };
        if root_meta.file_type().is_symlink() || !root_meta.is_dir() { return Ok(()); }
        let entries = match fs::read_dir(root) { Ok(v) => v, Err(_) => return Ok(()) };
        for entry in entries.flatten() {
            cancelled(check)?;
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) { Ok(v) => v, Err(_) => continue };
            if meta.file_type().is_symlink() || !meta.is_dir() { continue; }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if names.iter().any(|n| name == n.to_ascii_lowercase()) || path.to_string_lossy().to_ascii_lowercase().ends_with("\\service worker\\cachestorage") {
                found.push(path.clone());
            }
            collect_named_dirs(&path, names, found, depth + 1, check)?;
        }
        Ok(())
    }

    fn clean_browser_caches(metrics: &mut Metrics, output: &mut Vec<String>, check: &dyn Fn() -> bool, on_output: &mut dyn FnMut(&str)) -> Result<(), String> {
        let names = ["Cache", "Cache2", "Code Cache", "GPUCache", "StartupCache", "shader-cache", "CacheStorage"];
        for root in browser_roots() {
            cancelled(check)?;
            let progress = format!("Procurando caches do navegador: {}", root.display());
            on_output(&progress);
            output.push(progress);
            let mut dirs = Vec::new();
            collect_named_dirs(&root, &names, &mut dirs, 0, check)?;
            dirs.sort_by_key(|p| std::cmp::Reverse(p.to_string_lossy().len()));
            for dir in dirs {
                if !dir.exists() { continue; }
                match remove_entry(&dir, metrics, check) {
                    Ok(()) => {}
                    Err(error) if error == "Limpeza cancelada pelo usuário." => return Err(error),
                    Err(error) => { metrics.failed += 1; let message = format!("Não foi possível remover cache: {} — {error}", dir.display()); on_output(&message); output.push(message); }
                }
            }
        }
        Ok(())
    }

    pub(super) fn cleanup_standard(
        on_output: &mut dyn FnMut(&str),
        check: &dyn Fn() -> bool,
        clean_browser_cache: bool,
    ) -> Result<String, String> {
        let temp = std::env::var_os("TEMP").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let windir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let roots = [temp, windir.join("Temp")];
        let (mut metrics, initial) = clean_roots(&roots, "Limpeza padrão", check, true, on_output)?;
        let mut output = initial.lines()
            .filter(|line| !line.starts_with("Limpeza padrão:"))
            .map(str::to_string)
            .collect::<Vec<_>>();
        if clean_browser_cache {
            clean_browser_caches(&mut metrics, &mut output, check, on_output)?;
        } else {
            output.push("Caches dos navegadores ignorados conforme sua escolha de manter os navegadores abertos.".into());
        }
        match clear_recycle_bin() {
            Ok(()) => output.push("Lixeira esvaziada.".into()),
            Err(error) => output.push(format!("Lixeira não esvaziada: {error}")),
        }
        let summary = format!("Limpeza padrão: {} item(ns)/pasta(s) removido(s), {} falha(s).", metrics.entries, metrics.failed);
        let metrics_line = format!("QBOA_METRICS files={} bytes={} registry=0", metrics.files, metrics.bytes);
        output.push(summary.clone());
        output.push(metrics_line.clone());
        let result = output.join("\n");
        on_output(&summary);
        on_output(&metrics_line);
        Ok(result)
    }

    pub(super) fn cleanup_heavy_temp(on_output: &mut dyn FnMut(&str), check: &dyn Fn() -> bool) -> Result<String, String> {
        let temp = std::env::var_os("TEMP").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let windir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let roots = [temp, windir.join("Temp"), windir.join("Minidump"), windir.join("LiveKernelReports")];
        let (metrics, result) = clean_roots(&roots, "Limpeza pesada", check, true, on_output)?;
        let mut output = vec![result];
        let _ = clear_recycle_bin();
        let metrics_line = format!("QBOA_METRICS files={} bytes={} registry=0", metrics.files, metrics.bytes);
        output.push(metrics_line.clone());
        let result = output.join("\n");
        on_output(&format!("Limpeza pesada: {} item(ns)/pasta(s) removido(s), {} falha(s).", metrics.entries, metrics.failed));
        on_output(&metrics_line);
        Ok(result)
    }

    pub(super) fn cleanup_windows_update(on_output: &mut dyn FnMut(&str), check: &dyn Fn() -> bool) -> Result<String, String> {
        let windir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let root = windir.join("SoftwareDistribution/Download");
        if !root.exists() {
            let result = "Cache de download do Windows Update não existe; nada a limpar.\nQBOA_METRICS files=0 bytes=0 registry=0".to_string();
            for line in result.lines() { on_output(line); }
            return Ok(result);
        }
        let (metrics, output) = clean_roots(&[root], "Windows Update", check, false, on_output)?;
        let result = format!("{output}\nQBOA_METRICS files={} bytes={} registry=0", metrics.files, metrics.bytes);
        on_output(&format!("Windows Update: {} item(ns) removido(s), {} falha(s).", metrics.entries, metrics.failed));
        on_output(&format!("QBOA_METRICS files={} bytes={} registry=0", metrics.files, metrics.bytes));
        if metrics.failed > 0 { Err(result) } else { Ok(result) }
    }

    fn run_ie_history_cleanup() -> Result<(), String> {
        let windir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let rundll = windir.join("System32/rundll32.exe");
        let status = Command::new(rundll).args(["InetCpl.cpl,ClearMyTracksByProcess", "1"]).creation_flags(0x08000000).status().map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err(format!("rundll32 retornou {status}")) }
    }

    fn sqlite_exec(path: &Path, sql: &str) -> Result<(), String> {
        let filename = wide(path.as_os_str());
        let mut db: *mut c_void = std::ptr::null_mut();
        let open = unsafe { sqlite3_open16(filename.as_ptr().cast(), &mut db) };
        if open != 0 { return Err(format!("SQLite open retornou {open}")); }
        let sql_c = std::ffi::CString::new(sql).map_err(|e| e.to_string())?;
        let mut error: *mut c_char = std::ptr::null_mut();
        let code = unsafe { sqlite3_exec(db, sql_c.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), &mut error) };
        let message = if code == 0 { String::new() } else if !error.is_null() {
            let value = unsafe { std::ffi::CStr::from_ptr(error).to_string_lossy().into_owned() };
            unsafe { sqlite3_free(error.cast()); }
            value
        } else {
            let ptr = unsafe { sqlite3_errmsg(db) };
            if ptr.is_null() { format!("SQLite retornou {code}") } else { unsafe { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() } }
        };
        unsafe { sqlite3_close(db); }
        if code == 0 { Ok(()) } else { Err(message) }
    }

    fn add_file(path: &Path, metrics: &mut Metrics) {
        if let Ok(meta) = fs::symlink_metadata(path) {
            if meta.is_file() && !meta.file_type().is_symlink() {
                let size = meta.len();
                if fs::remove_file(path).is_ok() { metrics.files += 1; metrics.bytes = metrics.bytes.saturating_add(size); }
                else { metrics.failed += 1; }
            }
        }
    }

    pub(super) fn cleanup_browser_history(on_output: &mut dyn FnMut(&str), check: &dyn Fn() -> bool, clean_browser_cache: bool) -> Result<String, String> {
        let mut metrics = Metrics::default();
        let mut profiles = 0u64;
        let mut output = Vec::new();
        if !clean_browser_cache {
            let message = "Histórico dos navegadores preservado conforme sua escolha de manter os navegadores abertos.";
            output.push(message.into());
            on_output(message);
        } else {
            output.push("[Etapa 1/1] Localizando perfis e limpando históricos...".into());
            on_output("[Etapa 1/1] Localizando perfis e limpando históricos...");
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
            let roaming = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
            let roots = [
                local.join("Google/Chrome/User Data"), local.join("Microsoft/Edge/User Data"),
                local.join("BraveSoftware/Brave-Browser/User Data"), local.join("Vivaldi/User Data"),
                local.join("Yandex/YandexBrowser/User Data"), local.join("Chromium/User Data"),
                local.join("CocCoc/Browser/User Data"), local.join("Arc/User Data"),
                roaming.join("Opera Software/Opera Stable"), roaming.join("Opera Software/Opera GX Stable"),
            ];
            for root in roots.iter().filter(|p| p.exists()) {
                cancelled(check)?;
                let mut histories = Vec::new();
                collect_files_named(root, "History", &mut histories, 0, check)?;
                for history in histories {
                    cancelled(check)?;
                    let progress = format!("Limpando histórico Chromium: {}", history.parent().unwrap_or(root).display());
                    on_output(&progress);
                    output.push(progress);
                    add_file(&history, &mut metrics);
                    for suffix in ["-journal", "-wal", "-shm"] {
                        add_file(&PathBuf::from(format!("{}{}", history.display(), suffix)), &mut metrics);
                    }
                    profiles += 1;
                }
            }
            let ie_root = windir_path().join("System32");
            if ie_root.exists() {
                match run_ie_history_cleanup() {
                    Ok(()) => { profiles += 1; output.push("Solicitada a limpeza do histórico do Internet Explorer / Internet Options.".into()); }
                    Err(error) => output.push(format!("Internet Explorer indisponível nesta instalação: {error}")),
                }
            }
            let firefox_roots = [roaming.join("Mozilla/Firefox/Profiles"), local.join("Mozilla/Firefox/Profiles")];
            for root in firefox_roots.iter().filter(|p| p.exists()) {
                let mut databases = Vec::new();
                collect_files_named(root, "places.sqlite", &mut databases, 0, check)?;
                for database in databases {
                    cancelled(check)?;
                    let before = fs::metadata(&database).map(|m| m.len()).unwrap_or(0);
                    let sql = "PRAGMA foreign_keys=ON; BEGIN IMMEDIATE; DELETE FROM moz_historyvisits; DELETE FROM moz_inputhistory; DELETE FROM moz_places WHERE id NOT IN (SELECT fk FROM moz_bookmarks WHERE fk IS NOT NULL); COMMIT; VACUUM;";
                    match sqlite_exec(&database, sql) {
                        Ok(()) => {
                            let after = fs::metadata(&database).map(|m| m.len()).unwrap_or(before);
                            metrics.bytes = metrics.bytes.saturating_add(before.saturating_sub(after));
                            for suffix in ["-journal", "-wal", "-shm"] { add_file(&PathBuf::from(format!("{}{}", database.display(), suffix)), &mut metrics); }
                            profiles += 1;
                            let message = format!("Histórico Firefox limpo; favoritos preservados: {}", database.parent().unwrap_or(root).display());
                            on_output(&message);
                            output.push(message);
                        }
                        Err(error) => { metrics.failed += 1; let message = format!("Não foi possível limpar o histórico Firefox em {}: {error}", database.parent().unwrap_or(root).display()); on_output(&message); output.push(message); }
                    }
                }
            }
            if profiles == 0 { output.push("Nenhum perfil compatível de navegador foi encontrado.".into()); }
        }
        output.push(format!("Histórico dos navegadores: {profiles} perfil(is) processado(s), {} arquivo(s) removido(s), {} falha(s).", metrics.files, metrics.failed));
        output.push(format!("QBOA_METRICS files={} bytes={} registry=0", metrics.files, metrics.bytes));
        let result = output.join("\n");
        on_output(&format!("Histórico dos navegadores: {profiles} perfil(is) processado(s), {} arquivo(s) removido(s), {} falha(s).", metrics.files, metrics.failed));
        on_output(&format!("QBOA_METRICS files={} bytes={} registry=0", metrics.files, metrics.bytes));
        Ok(result)
    }

    fn windir_path() -> PathBuf {
        std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
    }

    fn collect_files_named(root: &Path, target: &str, found: &mut Vec<PathBuf>, depth: usize, check: &dyn Fn() -> bool) -> Result<(), String> {
        cancelled(check)?;
        if depth > 16 { return Ok(()); }
        let root_meta = match fs::symlink_metadata(root) { Ok(v) => v, Err(_) => return Ok(()) };
        if root_meta.file_type().is_symlink() || !root_meta.is_dir() { return Ok(()); }
        let entries = match fs::read_dir(root) { Ok(v) => v, Err(_) => return Ok(()) };
        for entry in entries.flatten() {
            cancelled(check)?;
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) { Ok(v) => v, Err(_) => continue };
            if meta.file_type().is_symlink() { continue; }
            if meta.is_file() && entry.file_name().to_string_lossy().eq_ignore_ascii_case(target) { found.push(path); }
            else if meta.is_dir() { collect_files_named(&path, target, found, depth + 1, check)?; }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub(super) fn cleanup_standard(_: &mut dyn FnMut(&str), _: &dyn Fn() -> bool, _: bool) -> Result<String, String> { Err("A limpeza nativa está disponível somente no Windows.".into()) }
    pub(super) fn cleanup_heavy_temp(_: &mut dyn FnMut(&str), _: &dyn Fn() -> bool) -> Result<String, String> { Err("A limpeza nativa está disponível somente no Windows.".into()) }
    pub(super) fn cleanup_windows_update(_: &mut dyn FnMut(&str), _: &dyn Fn() -> bool) -> Result<String, String> { Err("A limpeza nativa está disponível somente no Windows.".into()) }
    pub(super) fn cleanup_browser_history(_: &mut dyn FnMut(&str), _: &dyn Fn() -> bool, _: bool) -> Result<String, String> { Err("A limpeza nativa está disponível somente no Windows.".into()) }
}

pub fn cleanup_standard(on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool, clean_browser_cache: bool) -> Result<String, String> {
    platform::cleanup_standard(on_output, is_cancelled, clean_browser_cache)
}
pub fn cleanup_heavy_temp(on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    platform::cleanup_heavy_temp(on_output, is_cancelled)
}
pub fn cleanup_windows_update(on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    platform::cleanup_windows_update(on_output, is_cancelled)
}
pub fn cleanup_browser_history(on_output: &mut dyn FnMut(&str), is_cancelled: &dyn Fn() -> bool, clean_browser_cache: bool) -> Result<String, String> {
    platform::cleanup_browser_history(on_output, is_cancelled, clean_browser_cache)
}
