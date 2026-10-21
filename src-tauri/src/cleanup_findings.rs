use serde::{Deserialize, Serialize};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

const ASSOCIATION_EXTENSIONS: &[&str] = &[
    ".txt", ".pdf", ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".zip", ".7z", ".rar",
    ".mp3", ".wav", ".mp4", ".mkv", ".html", ".htm", ".docx", ".xlsx", ".pptx", ".json", ".csv",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FindingCategory {
    BrokenShortcut,
    InvalidAssociation,
    OrphanUninstall,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FindingRisk {
    Low,
    Moderate,
    High,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CleanupFinding {
    pub id: String,
    pub category: FindingCategory,
    pub path: String,
    pub reason: String,
    pub risk: FindingRisk,
    pub profiles: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) enum ShortcutState {
    Valid,
    BrokenTarget,
    InvalidLink,
    Uncertain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShortcutObservation {
    pub path: PathBuf,
    pub state: ShortcutState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) enum AssociationState {
    MissingClass,
    Executable(PathBuf),
    Uncertain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssociationObservation {
    pub extension: String,
    pub source: AssociationSource,
    pub prog_id: String,
    pub state: AssociationState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) enum AssociationSource {
    UserChoice,
    UserClasses,
    MachineClasses,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UninstallObservation {
    pub hive: String,
    pub key_path: String,
    pub display_name: String,
    pub evidence_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FindingAction {
    DeleteShortcut(PathBuf),
    DeleteAssociation {
        extension: String,
        source: AssociationSource,
        prog_id: String,
    },
    DeleteUninstallEntry {
        hive: String,
        key_path: String,
        display_name: String,
        evidence_paths: Vec<PathBuf>,
    },
}

pub(crate) trait CleanupFindingBackend {
    fn shortcuts(&mut self) -> Result<Vec<ShortcutObservation>, String>;
    fn associations(&mut self) -> Result<Vec<AssociationObservation>, String>;
    fn uninstall_entries(&mut self) -> Result<Vec<UninstallObservation>, String>;
    fn path_exists(&mut self, path: &Path) -> Result<bool, String>;
    fn delete(&mut self, action: &FindingAction) -> Result<(), String>;
}

#[derive(Clone, Debug)]
struct DetectedFinding {
    finding: CleanupFinding,
    action: FindingAction,
}

pub(crate) fn detect<B: CleanupFindingBackend>(
    backend: &mut B,
) -> Result<Vec<CleanupFinding>, String> {
    Ok(detect_actions(backend)?
        .into_iter()
        .map(|detected| detected.finding)
        .collect())
}

pub(crate) fn delete_by_id<B: CleanupFindingBackend>(
    backend: &mut B,
    finding_id: &str,
    confirmed: bool,
) -> Result<(), String> {
    if finding_id.trim().is_empty() {
        return Err("ID do achado obrigatório.".to_string());
    }
    if !confirmed {
        return Err("Confirme a exclusão do achado.".to_string());
    }

    let detected = detect_actions(backend)?
        .into_iter()
        .find(|detected| detected.finding.id == finding_id)
        .ok_or_else(|| format!("Achado não encontrado ou já corrigido: {finding_id}"))?;
    backend.delete(&detected.action)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CleanupExecutionReport {
    pub files_removed: u64,
    pub registry_entries_removed: u64,
    pub bytes_freed: u64,
    pub failures: u64,
    pub failure_details: Vec<String>,
}

pub(crate) fn cleanup_profile<B: CleanupFindingBackend>(
    backend: &mut B,
    profile: &str,
) -> Result<CleanupExecutionReport, String> {
    let detected = detect_actions(backend)?;
    let mut report = CleanupExecutionReport::default();
    for candidate in detected {
        if !candidate
            .finding
            .profiles
            .iter()
            .any(|value| value == profile)
        {
            continue;
        }
        let file_size = match &candidate.action {
            FindingAction::DeleteShortcut(path) => std::fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or(0),
            _ => 0,
        };
        match backend.delete(&candidate.action) {
            Ok(()) => match candidate.action {
                FindingAction::DeleteShortcut(_) => {
                    report.files_removed += 1;
                    report.bytes_freed = report.bytes_freed.saturating_add(file_size);
                }
                FindingAction::DeleteAssociation { .. }
                | FindingAction::DeleteUninstallEntry { .. } => {
                    report.registry_entries_removed += 1;
                }
            },
            Err(error) => {
                report.failures += 1;
                report
                    .failure_details
                    .push(format!("{}: {error}", candidate.finding.path));
            },
        }
    }
    Ok(report)
}

fn detect_actions<B: CleanupFindingBackend>(
    backend: &mut B,
) -> Result<Vec<DetectedFinding>, String> {
    let mut findings = Vec::new();

    for shortcut in backend.shortcuts()? {
        let (reason, risk) = match shortcut.state {
            ShortcutState::BrokenTarget => ("O destino do atalho não existe.", FindingRisk::Low),
            ShortcutState::InvalidLink => ("O arquivo de atalho é inválido.", FindingRisk::Low),
            ShortcutState::Valid | ShortcutState::Uncertain => continue,
        };
        let identity = format!("shortcut|{}", stable_path(&shortcut.path));
        findings.push(DetectedFinding {
            finding: make_finding(
                &identity,
                FindingCategory::BrokenShortcut,
                shortcut.path.display().to_string(),
                reason,
                risk,
                &["basic", "complete", "deep"],
            ),
            action: FindingAction::DeleteShortcut(shortcut.path),
        });
    }

    for association in backend.associations()? {
        if !ASSOCIATION_EXTENSIONS
            .iter()
            .any(|extension| extension.eq_ignore_ascii_case(&association.extension))
        {
            continue;
        }
        let (reason, executable, risk) = match association.state {
            AssociationState::MissingClass => (
                "A associação aponta para uma classe inexistente.",
                None,
                FindingRisk::Moderate,
            ),
            AssociationState::Executable(executable) => {
                if backend.path_exists(&executable)? {
                    continue;
                }
                (
                    "O executável associado não existe.",
                    Some(executable),
                    FindingRisk::Moderate,
                )
            }
            AssociationState::Uncertain => continue,
        };
        let identity = format!(
            "association|{}|{:?}|{}",
            association.extension.to_ascii_lowercase(),
            association.source,
            association.prog_id.to_ascii_lowercase()
        );
        let association_key = match &association.source {
            AssociationSource::UserChoice => format!(
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{}\UserChoice",
                association.extension
            ),
            AssociationSource::UserClasses => {
                format!(r"HKCU\Software\Classes\{}", association.extension)
            }
            AssociationSource::MachineClasses => {
                format!(r"HKLM\Software\Classes\{}", association.extension)
            }
        };
        let path = format!(
            "{} -> {}{}",
            association_key,
            association.prog_id,
            executable
                .as_ref()
                .map(|path| format!(" ({})", path.display()))
                .unwrap_or_default()
        );
        findings.push(DetectedFinding {
            finding: make_finding(
                &identity,
                FindingCategory::InvalidAssociation,
                path,
                reason,
                risk,
                &["complete", "deep"],
            ),
            action: FindingAction::DeleteAssociation {
                extension: association.extension,
                source: association.source,
                prog_id: association.prog_id,
            },
        });
    }

    for entry in backend.uninstall_entries()? {
        if entry.display_name.trim().is_empty() || entry.evidence_paths.is_empty() {
            continue;
        }
        let mut all_missing = true;
        for path in &entry.evidence_paths {
            if backend.path_exists(path)? {
                all_missing = false;
                break;
            }
        }
        if !all_missing {
            continue;
        }
        let identity = format!(
            "uninstall|{}|{}",
            entry.hive.to_ascii_lowercase(),
            entry.key_path.to_ascii_lowercase()
        );
        findings.push(DetectedFinding {
            finding: make_finding(
                &identity,
                FindingCategory::OrphanUninstall,
                format!("{}\\{}", entry.hive, entry.key_path),
                "O programa não foi encontrado nos caminhos registrados.",
                FindingRisk::High,
                &["deep"],
            ),
            action: FindingAction::DeleteUninstallEntry {
                hive: entry.hive,
                key_path: entry.key_path,
                display_name: entry.display_name,
                evidence_paths: entry.evidence_paths,
            },
        });
    }

    findings.sort_by(|left, right| left.finding.id.cmp(&right.finding.id));
    if findings
        .windows(2)
        .any(|pair| pair[0].finding.id == pair[1].finding.id)
    {
        return Err("IDs de achados duplicados na verificação.".to_string());
    }
    Ok(findings)
}

fn make_finding(
    identity: &str,
    category: FindingCategory,
    path: String,
    reason: &str,
    risk: FindingRisk,
    profiles: &[&str],
) -> CleanupFinding {
    CleanupFinding {
        id: stable_id(identity),
        category,
        path,
        reason: reason.to_string(),
        risk,
        profiles: profiles
            .iter()
            .map(|profile| (*profile).to_string())
            .collect(),
    }
}

fn stable_id(identity: &str) -> String {
    let hash = identity.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("finding-{hash:016x}")
}

fn stable_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

#[cfg(target_os = "windows")]
pub(crate) struct WindowsCleanupFindingBackend;

#[cfg(target_os = "windows")]
impl CleanupFindingBackend for WindowsCleanupFindingBackend {
    fn shortcuts(&mut self) -> Result<Vec<ShortcutObservation>, String> {
        let mut roots: Vec<PathBuf> = run_powershell_json(
            r#"$ErrorActionPreference='Stop';$r=@([Environment]::GetFolderPath([Environment+SpecialFolder]::DesktopDirectory),[Environment]::GetFolderPath([Environment+SpecialFolder]::StartMenu),[Environment]::GetFolderPath([Environment+SpecialFolder]::CommonDesktopDirectory),[Environment]::GetFolderPath([Environment+SpecialFolder]::CommonStartMenu));ConvertTo-Json -InputObject @($r|Where-Object{$_}) -Compress"#,
        )?;
        for (variable, suffix) in [
            ("USERPROFILE", "Desktop"),
            ("PUBLIC", "Desktop"),
            ("APPDATA", r"Microsoft\Windows\Start Menu\Programs"),
            ("PROGRAMDATA", r"Microsoft\Windows\Start Menu\Programs"),
        ] {
            if let Some(root) = std::env::var_os(variable).map(PathBuf::from) {
                roots.push(root.join(suffix));
            }
        }
        roots.sort();
        roots.dedup();

        let mut observations = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for root in roots {
            match std::fs::metadata(&root) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(format!(
                        "Não foi possível verificar o local {}: {error}",
                        root.display()
                    ))
                }
            }
            collect_shortcuts(&root, &mut seen, &mut observations)?;
        }
        Ok(observations)
    }

    fn associations(&mut self) -> Result<Vec<AssociationObservation>, String> {
        let script = r#"
$ErrorActionPreference = 'Stop'
$extensions = @('.txt','.pdf','.png','.jpg','.jpeg','.gif','.bmp','.webp','.zip','.7z','.rar','.mp3','.wav','.mp4','.mkv','.html','.htm','.docx','.xlsx','.pptx','.json','.csv')
$currentUser = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Default)
$localMachine = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine,[Microsoft.Win32.RegistryView]::Default)
$classes = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::ClassesRoot,[Microsoft.Win32.RegistryView]::Default)
$results = @()
foreach ($extension in $extensions) {
    $source = $null
    $progId = $null
    $userChoicePath = "Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\$extension\UserChoice"
    $key = $currentUser.OpenSubKey($userChoicePath)
    if ($key) {
        $progId = [string]$key.GetValue('ProgId')
        if ($progId) { $source = 'user_choice' }
        $key.Dispose()
    }
    if (!$progId) {
        $key = $currentUser.OpenSubKey("Software\Classes\$extension")
        if ($key) {
            $progId = [string]$key.GetValue('')
            if ($progId) { $source = 'user_classes' }
            $key.Dispose()
        }
    }
    if (!$progId) {
        $key = $localMachine.OpenSubKey("Software\Classes\$extension")
        if ($key) {
            $progId = [string]$key.GetValue('')
            if ($progId) { $source = 'machine_classes' }
            $key.Dispose()
        }
    }
    if (!$progId) { continue }

    $class = $classes.OpenSubKey($progId)
    if (!$class) {
        $results += [pscustomobject]@{ extension=$extension; source=$source; prog_id=$progId; state='missing_class'; executable=$null }
        continue
    }
    $class.Dispose()
    $commandKey = $classes.OpenSubKey("$progId\shell\open\command")
    if (!$commandKey) { continue }
    $command = [string]$commandKey.GetValue('')
    $commandKey.Dispose()
    if (!$command) { continue }

    $command = [Environment]::ExpandEnvironmentVariables($command)
    $executable = $null
    if ($command -match '^\s*"([^"]+\.exe)"') {
        $executable = $Matches[1]
    } elseif ($command -match '^\s*(.+?\.exe)(?:\s|,|$)') {
        $executable = $Matches[1].Trim()
    }
    if (!$executable -or ![IO.Path]::IsPathRooted($executable)) { continue }
    $results += [pscustomobject]@{ extension=$extension; source=$source; prog_id=$progId; state='candidate'; executable=$executable }
}
$currentUser.Dispose()
$localMachine.Dispose()
$classes.Dispose()
ConvertTo-Json -InputObject @($results) -Depth 5 -Compress
"#;
        let items: Vec<WindowsAssociation> = run_powershell_json(script)?;
        Ok(items
            .into_iter()
            .filter_map(WindowsAssociation::into_observation)
            .collect())
    }

    fn uninstall_entries(&mut self) -> Result<Vec<UninstallObservation>, String> {
        let script = r#"$ErrorActionPreference='Stop';$roots=@(@('LocalMachine','SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'),@('LocalMachine','SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'),@('CurrentUser','SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'));$r=@();foreach($root in $roots){$base=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$($root[0]),[Microsoft.Win32.RegistryView]::Default);$key=$base.OpenSubKey($root[1]);if(!$key){continue};foreach($name in $key.GetSubKeyNames()){$child=$key.OpenSubKey($name);if(!$child){continue};$display=[string]$child.GetValue('DisplayName');$install=[string]$child.GetValue('InstallLocation');$icon=[string]$child.GetValue('DisplayIcon');$evidence=@();if($install -and [IO.Path]::IsPathRooted($install)){$evidence+=@($install)};if($icon){$candidate=$icon.Trim().Trim('"');if($candidate -match '^(.+?\.exe)(?:,.*)?$'){$candidate=$Matches[1]};if([IO.Path]::IsPathRooted($candidate)){$evidence+=@($candidate)}};if($display -and $evidence.Count -gt 0){$r+=[pscustomobject]@{hive=$root[0];key_path=($root[1]+'\'+$name);display_name=$display;evidence_paths=@($evidence)}};$child.Dispose()};$key.Dispose();$base.Dispose()};ConvertTo-Json -InputObject @($r) -Depth 6 -Compress"#;
        let items: Vec<WindowsUninstallEntry> = run_powershell_json(script)?;
        Ok(items
            .into_iter()
            .map(|item| UninstallObservation {
                hive: item.hive,
                key_path: item.key_path,
                display_name: item.display_name,
                evidence_paths: item.evidence_paths.into_iter().map(PathBuf::from).collect(),
            })
            .collect())
    }

    fn path_exists(&mut self, path: &Path) -> Result<bool, String> {
        match std::fs::metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!(
                "Não foi possível verificar o caminho {}: {error}",
                path.display()
            )),
        }
    }

    fn delete(&mut self, action: &FindingAction) -> Result<(), String> {
        match action {
            FindingAction::DeleteShortcut(path) => {
                if !path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
                    || !shortcut_path_is_allowed(path)
                {
                    return Err("Caminho de atalho fora dos locais permitidos.".to_string());
                }
                match inspect_shortcut(path)? {
                    ShortcutState::BrokenTarget | ShortcutState::InvalidLink => {}
                    ShortcutState::Valid | ShortcutState::Uncertain => {
                        return Err("O atalho não está confirmado como inválido.".to_string())
                    }
                }
                std::fs::remove_file(path).map_err(|error| {
                    format!(
                        "Não foi possível excluir o atalho {}: {error}",
                        path.display()
                    )
                })
            }
            FindingAction::DeleteAssociation {
                extension,
                source,
                prog_id,
            } => delete_association(extension, source, prog_id),
            FindingAction::DeleteUninstallEntry {
                hive,
                key_path,
                display_name,
                evidence_paths,
            } => delete_uninstall_entry(hive, key_path, display_name, evidence_paths),
        }
    }
}

#[cfg(target_os = "windows")]
fn collect_shortcuts(
    directory: &Path,
    seen: &mut std::collections::HashSet<PathBuf>,
    observations: &mut Vec<ShortcutObservation>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("Não foi possível ler {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!("Não foi possível enumerar {}: {error}", directory.display())
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Não foi possível verificar {}: {error}", path.display()))?;
        if file_type.is_dir() {
            collect_shortcuts(&path, seen, observations)?;
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
            && seen.insert(path.clone())
        {
            observations.push(ShortcutObservation {
                state: inspect_shortcut(&path)?,
                path,
            });
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn inspect_shortcut(path: &Path) -> Result<ShortcutState, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ShortcutState::Uncertain)
        }
        Err(error) => return Err(format!("Não foi possível ler atalho: {error}")),
    };
    const HEADER: [u8; 20] = [
        0x4c, 0x00, 0x00, 0x00, 0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x46,
    ];
    if bytes.len() < HEADER.len() || bytes[..HEADER.len()] != HEADER {
        return Ok(ShortcutState::InvalidLink);
    }

    let encoded_path = base64_encode(path.to_string_lossy().as_bytes());
    let script = format!(
        "$ErrorActionPreference='Stop';$p=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_path}'));try{{$shell=New-Object -ComObject WScript.Shell;$link=$shell.CreateShortcut($p);$target=[Environment]::ExpandEnvironmentVariables([string]$link.TargetPath);$result=if(!$target -or !( [IO.Path]::IsPathRooted($target) )){{'uncertain'}}elseif(Test-Path -LiteralPath $target){{'valid'}}else{{'broken'}};ConvertTo-Json -InputObject @{{state=$result}} -Compress}}catch{{ConvertTo-Json -InputObject @{{state='uncertain'}} -Compress}}"
    );
    let response: ShortcutResponse = run_powershell_json(&script)?;
    Ok(match response.state.as_str() {
        "valid" => ShortcutState::Valid,
        "broken" => ShortcutState::BrokenTarget,
        _ => ShortcutState::Uncertain,
    })
}

#[cfg(target_os = "windows")]
fn shortcut_path_is_allowed(path: &Path) -> bool {
    let path = stable_path(path);
    let roots = [
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .map(|root| root.join("Desktop")),
        std::env::var_os("PUBLIC")
            .map(PathBuf::from)
            .map(|root| root.join("Desktop")),
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|root| root.join(r"Microsoft\Windows\Start Menu\Programs")),
        std::env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .map(|root| root.join(r"Microsoft\Windows\Start Menu\Programs")),
    ];
    roots.into_iter().flatten().any(|root| {
        let root = stable_path(&root);
        path.starts_with(&format!("{root}\\"))
    })
}

#[cfg(target_os = "windows")]
fn delete_association(
    extension: &str,
    source: &AssociationSource,
    prog_id: &str,
) -> Result<(), String> {
    if !ASSOCIATION_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()) {
        return Err("Extensão fora da lista verificada.".to_string());
    }
    let mut verifier = WindowsCleanupFindingBackend;
    let current = verifier
        .associations()?
        .into_iter()
        .find(|item| {
            item.extension.eq_ignore_ascii_case(extension)
                && &item.source == source
                && item.prog_id.eq_ignore_ascii_case(prog_id)
        })
        .ok_or_else(|| "A associação mudou desde a detecção.".to_string())?;
    match current.state {
        AssociationState::MissingClass => {}
        AssociationState::Executable(path) => {
            if verifier.path_exists(&path)? {
                return Err("A associação não está mais comprovadamente inválida.".to_string());
            }
        }
        AssociationState::Uncertain => {
            return Err("A associação não está mais comprovadamente inválida.".to_string());
        }
    }
    let encoded_extension = base64_encode(extension.as_bytes());
    let encoded_prog_id = base64_encode(prog_id.as_bytes());
    let source_name = match source {
        AssociationSource::UserChoice => "user_choice",
        AssociationSource::UserClasses => "user_classes",
        AssociationSource::MachineClasses => "machine_classes",
    };
    let script = format!(
        r#"$ErrorActionPreference='Stop';$ext=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_extension}'));$prog=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_prog_id}'));$source='{source_name}';$registryPath='Software\Classes\'+$ext;if($source-eq'user_choice'){{$registryPath='Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\'+$ext+'\UserChoice';$base=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Default);$key=$base.OpenSubKey($registryPath);if(!$key -or [string]$key.GetValue('ProgId')-ne$prog){{throw 'Associação alterada desde a detecção.'}};$key.Dispose();$base.DeleteSubKeyTree($registryPath,$true);$base.Dispose()}}else{{$registryHive=[Microsoft.Win32.RegistryHive]::CurrentUser;if($source-eq'machine_classes'){{$registryHive=[Microsoft.Win32.RegistryHive]::LocalMachine}};$base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($registryHive,[Microsoft.Win32.RegistryView]::Default);$key=$base.OpenSubKey($registryPath);if(!$key -or [string]$key.GetValue('')-ne$prog){{throw 'Associação alterada desde a detecção.'}};$key.Dispose();$writeKey=$base.OpenSubKey($registryPath,$true);if(!$writeKey){{throw 'Chave da associação indisponível.'}};$writeKey.DeleteValue('', $false);$writeKey.Dispose();$base.Dispose()}}"#
    );
    run_powershell(&script).map(|_| ())
}

#[cfg(target_os = "windows")]
fn delete_uninstall_entry(
    hive: &str,
    key_path: &str,
    display_name: &str,
    evidence_paths: &[PathBuf],
) -> Result<(), String> {
    let allowed_roots = [
        (
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            "LocalMachine",
        ),
        (
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
            "LocalMachine",
        ),
        (
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            "CurrentUser",
        ),
    ];
    if !allowed_roots.iter().any(|(root, allowed_hive)| {
        hive == *allowed_hive && (key_path == *root || key_path.starts_with(&format!("{root}\\")))
    }) {
        return Err("Entrada de desinstalação fora dos locais permitidos.".to_string());
    }
    let encoded_path = base64_encode(key_path.as_bytes());
    let encoded_name = base64_encode(display_name.as_bytes());
    let encoded_evidence =
        base64_encode(&serde_json::to_vec(evidence_paths).map_err(|error| error.to_string())?);
    let script = format!(
        r#"$ErrorActionPreference='Stop';$path=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_path}'));$name=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_name}'));$evidence=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_evidence}'))|ConvertFrom-Json;if(@($evidence|Where-Object{{Test-Path -LiteralPath $_}}).Count -gt 0){{throw 'O programa voltou a aparecer no disco.'}};$hive=[Microsoft.Win32.RegistryHive]::{hive};$base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($hive,[Microsoft.Win32.RegistryView]::Default);$key=$base.OpenSubKey($path);if(!$key -or [string]$key.GetValue('DisplayName')-ne$name){{throw 'Entrada alterada desde a detecção.'}};$key.Dispose();$base.DeleteSubKeyTree($path,$true);$base.Dispose()"#
    );
    run_powershell(&script).map(|_| ())
}

#[cfg(target_os = "windows")]
fn run_powershell_json<T: for<'de> Deserialize<'de>>(script: &str) -> Result<T, String> {
    let output = run_powershell(script)?;
    serde_json::from_str(output.trim_start_matches('\u{feff}'))
        .map_err(|error| format!("Resposta da verificação inválida: {error}"))
}

#[cfg(target_os = "windows")]
fn run_powershell(script: &str) -> Result<String, String> {
    use std::process::Command;

    let utf16: Vec<u16> = format!(
        "$utf8=[System.Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=$utf8;{script}"
    )
    .encode_utf16()
    .collect();
    let encoded_script = base64_encode(
        &utf16
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let mut command = Command::new("powershell");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
        &encoded_script,
    ]);
    // A verificação inspeciona atalhos e pode iniciar o PowerShell repetidamente.
    // Impede que cada processo abra uma janela visível sobre a interface do QBoa.
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = command
        .output()
        .map_err(|error| format!("Não foi possível iniciar a verificação: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Verificação do Windows falhou: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "windows")]
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        encoded.push(TABLE[(first >> 2) as usize] as char);
        encoded.push(TABLE[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            TABLE[(((second & 0x0f) << 2) | (third >> 6)) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            TABLE[(third & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

#[cfg(target_os = "windows")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutResponse {
    state: String,
}

#[cfg(target_os = "windows")]
#[derive(Deserialize)]
struct WindowsAssociation {
    extension: String,
    source: String,
    prog_id: String,
    state: String,
    executable: Option<PathBuf>,
}

#[cfg(target_os = "windows")]
impl WindowsAssociation {
    fn into_observation(self) -> Option<AssociationObservation> {
        let source = match self.source.as_str() {
            "user_choice" => AssociationSource::UserChoice,
            "user_classes" => AssociationSource::UserClasses,
            "machine_classes" => AssociationSource::MachineClasses,
            _ => return None,
        };
        let state = match self.state.as_str() {
            "missing_class" => AssociationState::MissingClass,
            "candidate" => AssociationState::Executable(self.executable?),
            _ => AssociationState::Uncertain,
        };
        Some(AssociationObservation {
            extension: self.extension,
            source,
            prog_id: self.prog_id,
            state,
        })
    }
}

#[cfg(target_os = "windows")]
#[derive(Deserialize)]
struct WindowsUninstallEntry {
    hive: String,
    key_path: String,
    display_name: String,
    evidence_paths: Vec<String>,
}

#[cfg(not(target_os = "windows"))]
pub(crate) struct WindowsCleanupFindingBackend;

#[cfg(not(target_os = "windows"))]
impl CleanupFindingBackend for WindowsCleanupFindingBackend {
    fn shortcuts(&mut self) -> Result<Vec<ShortcutObservation>, String> {
        Err("A detecção de limpeza só está disponível no Windows.".to_string())
    }

    fn associations(&mut self) -> Result<Vec<AssociationObservation>, String> {
        Err("A detecção de limpeza só está disponível no Windows.".to_string())
    }

    fn uninstall_entries(&mut self) -> Result<Vec<UninstallObservation>, String> {
        Err("A detecção de limpeza só está disponível no Windows.".to_string())
    }

    fn path_exists(&mut self, _path: &Path) -> Result<bool, String> {
        Err("A detecção de limpeza só está disponível no Windows.".to_string())
    }

    fn delete(&mut self, _action: &FindingAction) -> Result<(), String> {
        Err("A exclusão de achados só está disponível no Windows.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Default)]
    struct FakeBackend {
        shortcuts: Vec<ShortcutObservation>,
        associations: Vec<AssociationObservation>,
        uninstall_entries: Vec<UninstallObservation>,
        existing_paths: HashSet<PathBuf>,
        deleted: Vec<FindingAction>,
    }

    impl CleanupFindingBackend for FakeBackend {
        fn shortcuts(&mut self) -> Result<Vec<ShortcutObservation>, String> {
            Ok(self.shortcuts.clone())
        }

        fn associations(&mut self) -> Result<Vec<AssociationObservation>, String> {
            Ok(self.associations.clone())
        }

        fn uninstall_entries(&mut self) -> Result<Vec<UninstallObservation>, String> {
            Ok(self.uninstall_entries.clone())
        }

        fn path_exists(&mut self, path: &Path) -> Result<bool, String> {
            Ok(self.existing_paths.contains(path))
        }

        fn delete(&mut self, action: &FindingAction) -> Result<(), String> {
            self.deleted.push(action.clone());
            Ok(())
        }
    }

    #[test]
    fn valid_shortcut_is_not_reported() {
        let mut backend = FakeBackend {
            shortcuts: vec![ShortcutObservation {
                path: PathBuf::from(r"C:\Users\test\Desktop\ok.lnk"),
                state: ShortcutState::Valid,
            }],
            ..FakeBackend::default()
        };

        assert!(detect(&mut backend).unwrap().is_empty());
    }

    #[test]
    fn broken_shortcut_is_reported() {
        let mut backend = FakeBackend {
            shortcuts: vec![ShortcutObservation {
                path: PathBuf::from(r"C:\Users\test\Desktop\broken.lnk"),
                state: ShortcutState::BrokenTarget,
            }],
            ..FakeBackend::default()
        };

        let findings = detect(&mut backend).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::BrokenShortcut);
        assert!(findings[0].id.starts_with("finding-"));
        assert_eq!(findings[0].profiles, ["basic", "complete", "deep"]);
        assert!(backend.deleted.is_empty());
    }

    #[test]
    fn malformed_shortcut_is_reported_but_uncertain_shortcut_is_not() {
        let mut backend = FakeBackend {
            shortcuts: vec![
                ShortcutObservation {
                    path: PathBuf::from(r"C:\Users\test\Desktop\invalid.lnk"),
                    state: ShortcutState::InvalidLink,
                },
                ShortcutObservation {
                    path: PathBuf::from(r"C:\Users\test\Desktop\unknown.lnk"),
                    state: ShortcutState::Uncertain,
                },
            ],
            ..FakeBackend::default()
        };

        let findings = detect(&mut backend).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].path.ends_with("invalid.lnk"));
    }

    #[test]
    fn association_with_existing_executable_is_not_reported() {
        let mut backend = FakeBackend {
            associations: vec![AssociationObservation {
                extension: ".pdf".to_string(),
                source: AssociationSource::UserChoice,
                prog_id: "PdfReader.Document".to_string(),
                state: AssociationState::Executable(PathBuf::from(r"C:\Program Files\Pdf\pdf.exe")),
            }],
            existing_paths: HashSet::from([PathBuf::from(r"C:\Program Files\Pdf\pdf.exe")]),
            ..FakeBackend::default()
        };

        assert!(detect(&mut backend).unwrap().is_empty());
    }

    #[test]
    fn association_with_missing_executable_is_reported() {
        let missing = PathBuf::from(r"C:\Program Files\Pdf\missing.exe");
        let mut backend = FakeBackend {
            associations: vec![AssociationObservation {
                extension: ".pdf".to_string(),
                source: AssociationSource::MachineClasses,
                prog_id: "PdfReader.Document".to_string(),
                state: AssociationState::Executable(missing.clone()),
            }],
            ..FakeBackend::default()
        };

        let findings = detect(&mut backend).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::InvalidAssociation);
        assert!(findings[0].path.contains(&missing.display().to_string()));
        assert_eq!(findings[0].profiles, ["complete", "deep"]);
    }

    #[test]
    fn uninstall_entry_is_not_orphaned_if_program_path_exists() {
        let install_path = PathBuf::from(r"C:\Program Files\Example");
        let mut backend = FakeBackend {
            uninstall_entries: vec![UninstallObservation {
                hive: "LocalMachine".to_string(),
                key_path: r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Example"
                    .to_string(),
                display_name: "Example".to_string(),
                evidence_paths: vec![install_path.clone()],
            }],
            existing_paths: HashSet::from([install_path]),
            ..FakeBackend::default()
        };

        assert!(detect(&mut backend).unwrap().is_empty());
    }

    #[test]
    fn uninstall_entry_is_reported_only_when_all_evidence_paths_are_missing() {
        let mut backend = FakeBackend {
            uninstall_entries: vec![UninstallObservation {
                hive: "LocalMachine".to_string(),
                key_path: r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Example"
                    .to_string(),
                display_name: "Example".to_string(),
                evidence_paths: vec![PathBuf::from(r"C:\Program Files\Example")],
            }],
            ..FakeBackend::default()
        };

        let findings = detect(&mut backend).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::OrphanUninstall);
        assert_eq!(findings[0].profiles, ["deep"]);
    }

    #[test]
    fn detection_does_not_mutate_files_or_registry() {
        let mut backend = FakeBackend {
            shortcuts: vec![ShortcutObservation {
                path: PathBuf::from(r"C:\Users\test\Desktop\broken.lnk"),
                state: ShortcutState::InvalidLink,
            }],
            ..FakeBackend::default()
        };
        let before = backend.deleted.clone();

        assert_eq!(detect(&mut backend).unwrap().len(), 1);
        assert_eq!(backend.deleted, before);
    }

    #[test]
    fn deletion_requires_a_known_id_and_confirmation() {
        let mut backend = FakeBackend::default();

        assert!(delete_by_id(&mut backend, "unknown", true).is_err());
        assert!(delete_by_id(&mut backend, "", true).is_err());
        assert!(delete_by_id(&mut backend, "unknown", false).is_err());
        assert!(backend.deleted.is_empty());
    }

    #[test]
    fn deleting_a_detected_item_requires_confirmation_and_uses_its_id() {
        let mut backend = FakeBackend {
            shortcuts: vec![ShortcutObservation {
                path: PathBuf::from(r"C:\Users\test\Desktop\broken.lnk"),
                state: ShortcutState::BrokenTarget,
            }],
            ..FakeBackend::default()
        };
        let id = detect(&mut backend).unwrap()[0].id.clone();

        delete_by_id(&mut backend, &id, true).unwrap();
        assert_eq!(
            backend.deleted,
            [FindingAction::DeleteShortcut(PathBuf::from(
                r"C:\Users\test\Desktop\broken.lnk"
            ))]
        );
    }

    #[test]
    fn cleanup_profile_applies_only_findings_allowed_by_profile() {
        let mut backend = FakeBackend {
            shortcuts: vec![ShortcutObservation {
                path: PathBuf::from(r"C:\Users\test\Desktop\broken.lnk"),
                state: ShortcutState::BrokenTarget,
            }],
            associations: vec![AssociationObservation {
                extension: ".pdf".to_string(),
                source: AssociationSource::MachineClasses,
                prog_id: "Missing.Pdf".to_string(),
                state: AssociationState::MissingClass,
            }],
            uninstall_entries: vec![UninstallObservation {
                hive: "LocalMachine".to_string(),
                key_path: r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Example"
                    .to_string(),
                display_name: "Example".to_string(),
                evidence_paths: vec![PathBuf::from(r"C:\Program Files\Example\app.exe")],
            }],
            ..FakeBackend::default()
        };

        let report = cleanup_profile(&mut backend, "complete").unwrap();
        assert_eq!(report.files_removed, 1);
        assert_eq!(report.registry_entries_removed, 1);
        assert_eq!(report.failures, 0);
        assert_eq!(backend.deleted.len(), 2);
        assert!(!backend
            .deleted
            .iter()
            .any(|action| matches!(action, FindingAction::DeleteUninstallEntry { .. })));
    }

    #[test]
    fn uncommon_extensions_are_not_classified() {
        let mut backend = FakeBackend {
            associations: vec![AssociationObservation {
                extension: ".rare".to_string(),
                source: AssociationSource::UserClasses,
                prog_id: "Missing".to_string(),
                state: AssociationState::MissingClass,
            }],
            ..FakeBackend::default()
        };

        assert!(detect(&mut backend).unwrap().is_empty());
    }
}
