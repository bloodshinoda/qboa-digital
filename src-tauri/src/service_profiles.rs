use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
#[cfg(target_os = "windows")]
use std::path::PathBuf;

const PROFILE_JSON: [&str; 10] = [
    include_str!("../service-profiles/windows-10/home.json"),
    include_str!("../service-profiles/windows-10/pro.json"),
    include_str!("../service-profiles/windows-10/pro-for-workstations.json"),
    include_str!("../service-profiles/windows-10/enterprise.json"),
    include_str!("../service-profiles/windows-10/education.json"),
    include_str!("../service-profiles/windows-11/home.json"),
    include_str!("../service-profiles/windows-11/pro.json"),
    include_str!("../service-profiles/windows-11/pro-for-workstations.json"),
    include_str!("../service-profiles/windows-11/enterprise.json"),
    include_str!("../service-profiles/windows-11/education.json"),
];
const PROTECTED_SERVICES: &[&str] = &["wuauserv", "WinDefend", "Dhcp", "RpcSs", "PlugPlay"];
const APPLYABLE_PROFILES: &[ProductProfile] = &[
    ProductProfile::Recommended,
    ProductProfile::Safe,
    ProductProfile::Refined,
    ProductProfile::Aggressive,
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WindowsVersion {
    Windows10,
    Windows11,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WindowsEdition {
    Home,
    Pro,
    ProForWorkstations,
    Enterprise,
    Education,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FormFactor {
    Desktop,
    Notebook,
    Any,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductProfile {
    Recommended,
    Safe,
    Refined,
    Aggressive,
    Custom,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ServiceRule {
    pub name: String,
    pub start_type: String,
    pub windows_default_start_type: Option<String>,
    pub profiles: Vec<ProductProfile>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ServiceProfileFile {
    pub os: WindowsVersion,
    pub edition: WindowsEdition,
    pub form_factor: FormFactor,
    pub validated: bool,
    pub services: Vec<ServiceRule>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct InstalledService {
    pub name: String,
    pub start_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct DetectedSystem {
    pub os: WindowsVersion,
    pub version: String,
    pub build: String,
    pub edition: WindowsEdition,
    pub edition_id: String,
    pub supported: bool,
    pub form_factor: Option<FormFactor>,
    pub services: Vec<InstalledService>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ServicePreviewItem {
    pub service: String,
    pub current: String,
    pub proposed: Option<String>,
    pub incompatible: Option<String>,
    pub ignored: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ServicePreview {
    pub profile: ProductProfile,
    pub applicable: bool,
    pub validated: bool,
    pub message: String,
    pub services: Vec<ServicePreviewItem>,
    pub ignored: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ServiceSnapshot {
    os: WindowsVersion,
    build: String,
    edition: WindowsEdition,
    services: Vec<InstalledService>,
}

pub(crate) trait ServiceProfileBackend {
    fn detect_system(&mut self) -> Result<DetectedSystem, String>;
    fn start_types(&mut self, names: &[String]) -> Result<Vec<InstalledService>, String>;
    fn set_start_type(&mut self, name: &str, start_type: &str) -> Result<(), String>;
    fn load_snapshot(&mut self) -> Result<Option<ServiceSnapshot>, String>;
    fn save_snapshot(&mut self, snapshot: &ServiceSnapshot) -> Result<(), String>;
}

pub(crate) fn product_profiles() -> Vec<ProductProfile> {
    vec![
        ProductProfile::Recommended,
        ProductProfile::Safe,
        ProductProfile::Refined,
        ProductProfile::Aggressive,
        ProductProfile::Custom,
    ]
}

pub(crate) fn profile_files() -> Result<Vec<ServiceProfileFile>, String> {
    PROFILE_JSON
        .into_iter()
        .map(|json| {
            serde_json::from_str(json)
                .map_err(|error| format!("Arquivo de perfil de serviços inválido: {error}"))
        })
        .collect()
}

pub(crate) fn system_profile(system: &DetectedSystem) -> Result<ServiceProfileFile, String> {
    let profile = profile_files()?
        .into_iter()
        .find(|profile| profile.os == system.os && profile.edition == system.edition);
    match profile {
        Some(profile) => Ok(profile),
        None if !system.supported => Ok(ServiceProfileFile {
            os: system.os.clone(),
            edition: WindowsEdition::Unknown,
            form_factor: FormFactor::Any,
            validated: false,
            services: Vec::new(),
        }),
        None => Err("Não há perfil para este Windows.".to_string()),
    }
}

fn detected_system_edition(edition_id: &str) -> (WindowsEdition, bool) {
    match parse_edition(edition_id) {
        Some(edition) => (edition, true),
        None => (WindowsEdition::Unknown, false),
    }
}

fn powershell_output_result(
    succeeded: bool,
    stdout: &[u8],
    _stderr: &[u8],
) -> Result<String, String> {
    if succeeded {
        Ok(String::from_utf8_lossy(stdout).trim().to_string())
    } else {
        Err("Não foi possível consultar os serviços do Windows.".to_string())
    }
}

pub(crate) fn preview(
    system: &DetectedSystem,
    profile: &ServiceProfileFile,
    selected: ProductProfile,
) -> ServicePreview {
    if !system.supported || !profile.validated || profile.services.is_empty() {
        let services = system
            .services
            .iter()
            .map(|service| ServicePreviewItem {
                service: service.name.clone(),
                current: service.start_type.clone(),
                proposed: None,
                incompatible: None,
                ignored: false,
            })
            .collect();
        let message = if !system.supported {
            format!(
                "Edição detectada: {}. Não há perfil para ela.",
                system.edition_id
            )
        } else {
            format!(
                "Edição {} detectada ({}). Perfil ainda não validado; nada será alterado.",
                edition_label(&system.edition),
                system.edition_id
            )
        };
        return ServicePreview {
            profile: selected,
            applicable: false,
            validated: false,
            message,
            services,
            ignored: Vec::new(),
        };
    }

    let mut reasons = profile_incompatibilities(system, profile);
    let rules: HashMap<String, &ServiceRule> = if selected == ProductProfile::Custom {
        HashMap::new()
    } else {
        profile
            .services
            .iter()
            .filter(|rule| rule.profiles.contains(&selected))
            .map(|rule| (rule.name.to_ascii_lowercase(), rule))
            .collect()
    };
    let installed: HashSet<String> = system
        .services
        .iter()
        .map(|service| service.name.to_ascii_lowercase())
        .collect();
    let mut items = Vec::new();
    let mut ignored = Vec::new();

    for service in &system.services {
        let Some(rule) = rules.get(&service.name.to_ascii_lowercase()) else {
            ignored.push(service.name.clone());
            items.push(ServicePreviewItem {
                service: service.name.clone(),
                current: service.start_type.clone(),
                proposed: None,
                incompatible: None,
                ignored: true,
            });
            continue;
        };
        let incompatibility = if protected_disable(&rule.name, &rule.start_type) {
            Some("Serviço protegido não pode ser desativado.")
        } else if !valid_start_type(&service.start_type) {
            Some("Tipo de inicialização atual não pode ser alterado com segurança.")
        } else if !valid_start_type(&rule.start_type) {
            Some("Tipo de inicialização inválido.")
        } else {
            None
        };
        if let Some(reason) = incompatibility {
            reasons.push(reason);
        }
        items.push(ServicePreviewItem {
            service: service.name.clone(),
            current: service.start_type.clone(),
            proposed: Some(rule.start_type.clone()),
            incompatible: incompatibility.map(str::to_string),
            ignored: false,
        });
    }
    for rule in rules.values() {
        if !installed.contains(&rule.name.to_ascii_lowercase()) {
            let reason = "Serviço não instalado.";
            reasons.push(reason);
            items.push(ServicePreviewItem {
                service: rule.name.clone(),
                current: "não instalado".to_string(),
                proposed: Some(rule.start_type.clone()),
                incompatible: Some(reason.to_string()),
                ignored: false,
            });
        }
    }
    items.sort_by(|a, b| a.service.cmp(&b.service));
    ignored.sort();
    let applicable = reasons.is_empty()
        && selected != ProductProfile::Custom
        && APPLYABLE_PROFILES.contains(&selected);
    let message = if reasons.is_empty() {
        if applicable {
            "Prévia pronta.".to_string()
        } else {
            "Perfil personalizado não aplica lista.".to_string()
        }
    } else {
        reasons.sort_unstable();
        reasons.dedup();
        reasons.join(" ")
    };
    ServicePreview {
        profile: selected,
        applicable,
        validated: profile.validated,
        message,
        services: items,
        ignored,
    }
}

pub(crate) fn apply_profile<B: ServiceProfileBackend>(
    backend: &mut B,
    profile: &ServiceProfileFile,
    selected: ProductProfile,
) -> Result<(), String> {
    ensure_profile_has_validated_services(profile)?;
    if selected != ProductProfile::Custom
        && profile.services.iter().any(|rule| {
            rule.profiles.contains(&selected) && protected_disable(&rule.name, &rule.start_type)
        })
    {
        return Err("Perfil solicita desativar um serviço protegido.".to_string());
    }
    let system = backend.detect_system()?;
    let preview = preview(&system, profile, selected);
    if !preview.applicable {
        return Err(format!("Perfil não aplicável: {}", preview.message));
    }

    let names: Vec<String> = preview
        .services
        .iter()
        .filter(|item| item.proposed.is_some() && item.incompatible.is_none())
        .map(|item| item.service.clone())
        .collect();
    let snapshot_services = backend.start_types(&names)?;
    ensure_snapshot_covers(&names, &snapshot_services)?;
    backend.save_snapshot(&ServiceSnapshot {
        os: system.os,
        build: system.build,
        edition: system.edition,
        services: snapshot_services,
    })?;
    for item in preview
        .services
        .iter()
        .filter(|item| item.proposed.is_some() && item.incompatible.is_none())
    {
        backend.set_start_type(&item.service, item.proposed.as_deref().unwrap_or_default())?;
    }
    Ok(())
}

pub(crate) fn restore_windows_defaults<B: ServiceProfileBackend>(
    backend: &mut B,
    profile: &ServiceProfileFile,
) -> Result<(), String> {
    ensure_profile_has_validated_services(profile)?;
    let system = backend.detect_system()?;
    if !profile_incompatibilities(&system, profile).is_empty() {
        return Err("Perfil não corresponde ao Windows detectado.".to_string());
    }
    let mut defaults = Vec::new();
    for rule in &profile.services {
        let start_type = rule
            .windows_default_start_type
            .as_deref()
            .ok_or_else(|| format!("Baseline ausente para {}.", rule.name))?;
        if !valid_start_type(start_type) || protected_disable(&rule.name, start_type) {
            return Err(format!("Baseline inválido para {}.", rule.name));
        }
        defaults.push((rule.name.clone(), start_type.to_string()));
    }
    let names: Vec<String> = defaults.iter().map(|(name, _)| name.clone()).collect();
    let current = backend.start_types(&names)?;
    ensure_snapshot_covers(&names, &current)?;
    backend.save_snapshot(&ServiceSnapshot {
        os: system.os,
        build: system.build,
        edition: system.edition,
        services: current,
    })?;
    for (name, start_type) in defaults {
        backend.set_start_type(&name, &start_type)?;
    }
    Ok(())
}

pub(crate) fn undo_last_application<B: ServiceProfileBackend>(
    backend: &mut B,
    profile: &ServiceProfileFile,
) -> Result<(), String> {
    ensure_profile_has_validated_services(profile)?;
    let system = backend.detect_system()?;
    if !profile_incompatibilities(&system, profile).is_empty() {
        return Err("Perfil não corresponde ao Windows detectado.".to_string());
    }
    let snapshot = backend
        .load_snapshot()?
        .ok_or_else(|| "Snapshot de serviços ausente.".to_string())?;
    if snapshot.os != system.os
        || snapshot.build != system.build
        || snapshot.edition != system.edition
    {
        return Err("Snapshot não corresponde ao Windows detectado.".to_string());
    }
    let allowed_names: HashSet<String> = profile
        .services
        .iter()
        .map(|rule| rule.name.to_ascii_lowercase())
        .collect();
    for service in &snapshot.services {
        if !valid_start_type(&service.start_type)
            || protected_disable(&service.name, &service.start_type)
            || !allowed_names.contains(&service.name.to_ascii_lowercase())
        {
            return Err(format!("Snapshot inválido para {}.", service.name));
        }
    }
    for service in &snapshot.services {
        backend.set_start_type(&service.name, &service.start_type)?;
    }
    Ok(())
}

fn ensure_profile_has_validated_services(profile: &ServiceProfileFile) -> Result<(), String> {
    if !profile.validated || profile.services.is_empty() {
        return Err("Perfil ainda não validado.".to_string());
    }
    Ok(())
}

fn edition_label(edition: &WindowsEdition) -> &'static str {
    match edition {
        WindowsEdition::Home => "Home",
        WindowsEdition::Pro => "Pro",
        WindowsEdition::ProForWorkstations => "Pro for Workstations",
        WindowsEdition::Enterprise => "Enterprise",
        WindowsEdition::Education => "Education",
        WindowsEdition::Unknown => "desconhecida",
    }
}

fn ensure_snapshot_covers(names: &[String], services: &[InstalledService]) -> Result<(), String> {
    let captured: HashSet<String> = services
        .iter()
        .map(|service| service.name.to_ascii_lowercase())
        .collect();
    if names
        .iter()
        .any(|name| !captured.contains(&name.to_ascii_lowercase()))
    {
        return Err("Não foi possível capturar todos os serviços.".to_string());
    }
    Ok(())
}

fn profile_incompatibilities(
    system: &DetectedSystem,
    profile: &ServiceProfileFile,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if !system.supported {
        reasons.push("Edição não suportada.");
    }
    if system.os != profile.os {
        reasons.push("Versão do Windows incompatível.");
    }
    if system.edition != profile.edition {
        reasons.push("Edição do Windows incompatível.");
    }
    if !profile.validated {
        reasons.push("Perfil ainda não validado.");
    }
    if profile.form_factor != FormFactor::Any {
        match &system.form_factor {
            Some(detected) if detected == &profile.form_factor => {}
            Some(_) => reasons.push("Formato do dispositivo incompatível."),
            None => reasons.push("Formato do dispositivo não detectado."),
        }
    }
    reasons
}

fn valid_start_type(start_type: &str) -> bool {
    ["automatic", "manual", "disabled"].contains(&start_type.to_ascii_lowercase().as_str())
}

fn protected_disable(service: &str, start_type: &str) -> bool {
    start_type.eq_ignore_ascii_case("disabled")
        && PROTECTED_SERVICES
            .iter()
            .any(|protected| service.eq_ignore_ascii_case(protected))
}

#[cfg(target_os = "windows")]
pub(crate) struct WindowsServiceProfileBackend {
    snapshot_path: PathBuf,
}

#[cfg(target_os = "windows")]
impl Default for WindowsServiceProfileBackend {
    fn default() -> Self {
        Self {
            snapshot_path: std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("Qboa Digital")
                .join("service-profile")
                .join("last-application.json"),
        }
    }
}

#[cfg(target_os = "windows")]
impl ServiceProfileBackend for WindowsServiceProfileBackend {
    fn detect_system(&mut self) -> Result<DetectedSystem, String> {
        let script = r#"
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
$key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion')
if (-not $key) { throw 'Registro do Windows indisponível.' }
$editionId = [string]$key.GetValue('EditionID', '')
$build = [string]$key.GetValue('CurrentBuildNumber', '')
$buildNumber = 0
[void][int]::TryParse($build, [ref]$buildNumber)
$version = if ($buildNumber -ge 22000) { 'Windows11' } else { 'Windows10' }
$displayVersion = [string]$key.GetValue('DisplayVersion', '')
if (-not $displayVersion) { $displayVersion = [string]$key.GetValue('ReleaseId', '') }
$services = [System.ServiceProcess.ServiceController]::GetServices() | ForEach-Object {
    $serviceKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("SYSTEM\CurrentControlSet\Services\$($_.ServiceName)")
    if ($serviceKey) {
        $start = switch ([int]$serviceKey.GetValue('Start', -1)) {
            2 { 'Automatic' }
            3 { 'Manual' }
            4 { 'Disabled' }
            default { [string]$serviceKey.GetValue('Start', 'Unknown') }
        }
        [pscustomobject]@{ name=$_.ServiceName; start_type=$start }
        $serviceKey.Dispose()
    }
}
[pscustomobject]@{ version=$version; display_version=$displayVersion; build=$build; edition_id=$editionId; services=@($services) } | ConvertTo-Json -Depth 5 -Compress
"#;
        let data: WindowsSystemData = run_powershell_json(script)?;
        let os = match data.version.as_str() {
            "Windows10" => WindowsVersion::Windows10,
            "Windows11" => WindowsVersion::Windows11,
            _ => return Err("Versão do Windows não suportada.".to_string()),
        };
        let (edition, supported) = detected_system_edition(&data.edition_id);
        Ok(DetectedSystem {
            os,
            version: data.display_version,
            build: data.build,
            edition,
            edition_id: data.edition_id,
            supported,
            form_factor: None,
            services: data.services,
        })
    }

    fn start_types(&mut self, names: &[String]) -> Result<Vec<InstalledService>, String> {
        let encoded = base64_encode(&serde_json::to_vec(names).map_err(|error| error.to_string())?);
        let script = format!(
            r#"$ErrorActionPreference='Stop';$names=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'))|ConvertFrom-Json;$all=Get-CimInstance Win32_Service -ErrorAction Stop;$r=@();foreach($name in $names){{$matches=@($all|Where-Object{{$_.Name -ieq $name}});if($matches.Count-ne 1){{throw "Serviço não encontrado ou ambíguo: $name"}};$s=$matches[0];$start=switch($s.StartMode){{'Auto'{{'Automatic'}}'Manual'{{'Manual'}}'Disabled'{{'Disabled'}}default{{$s.StartMode}}}};$r+=[pscustomobject]@{{name=$s.Name;start_type=$start}}}};ConvertTo-Json -InputObject @($r) -Depth 4 -Compress"#
        );
        run_powershell_json(&script)
    }

    fn set_start_type(&mut self, name: &str, start_type: &str) -> Result<(), String> {
        if !valid_start_type(start_type) || protected_disable(name, start_type) {
            return Err(format!("Alteração bloqueada para o serviço {name}."));
        }
        let encoded = base64_encode(name.as_bytes());
        let start_type = match start_type.to_ascii_lowercase().as_str() {
            "automatic" => "Automatic",
            "manual" => "Manual",
            _ => "Disabled",
        };
        let script = format!(
            r#"$ErrorActionPreference='Stop';$name=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'));$services=@(Get-Service|Where-Object{{$_.Name -ieq $name}});if($services.Count-ne 1){{throw "Serviço não encontrado ou ambíguo: $name"}};Set-Service -InputObject $services[0] -StartupType {start_type}"#
        );
        run_powershell(&script).map(|_| ())
    }

    fn load_snapshot(&mut self) -> Result<Option<ServiceSnapshot>, String> {
        if !self.snapshot_path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(&self.snapshot_path)
            .map_err(|error| format!("Não foi possível ler snapshot de serviços: {error}"))?;
        serde_json::from_str(&data)
            .map(Some)
            .map_err(|error| format!("Snapshot de serviços inválido: {error}"))
    }

    fn save_snapshot(&mut self, snapshot: &ServiceSnapshot) -> Result<(), String> {
        let parent = self
            .snapshot_path
            .parent()
            .ok_or_else(|| "Caminho do snapshot inválido.".to_string())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Não foi possível criar pasta de snapshot: {error}"))?;
        let temporary = self.snapshot_path.with_extension("tmp");
        let data = serde_json::to_vec_pretty(snapshot)
            .map_err(|error| format!("Não foi possível serializar snapshot: {error}"))?;
        let mut file = std::fs::File::create(&temporary)
            .map_err(|error| format!("Não foi possível criar snapshot temporário: {error}"))?;
        std::io::Write::write_all(&mut file, &data)
            .map_err(|error| format!("Não foi possível gravar snapshot: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Não foi possível sincronizar snapshot: {error}"))?;
        std::fs::rename(&temporary, &self.snapshot_path)
            .map_err(|error| format!("Não foi possível salvar snapshot: {error}"))
    }
}

#[derive(Deserialize)]
struct WindowsSystemData {
    version: String,
    display_version: String,
    build: String,
    edition_id: String,
    services: Vec<InstalledService>,
}

fn parse_edition(value: &str) -> Option<WindowsEdition> {
    match value {
        "Core" | "CoreN" | "CoreSingleLanguage" | "CoreCountrySpecific" => {
            Some(WindowsEdition::Home)
        }
        "Professional" | "ProfessionalN" => Some(WindowsEdition::Pro),
        "ProfessionalWorkstation" | "ProfessionalWorkstationN" => {
            Some(WindowsEdition::ProForWorkstations)
        }
        "Enterprise" | "EnterpriseN" | "EnterpriseG" | "EnterpriseS" | "IoTEnterprise" => {
            Some(WindowsEdition::Enterprise)
        }
        "Education" | "EducationN" | "ProfessionalEducation" => Some(WindowsEdition::Education),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn run_powershell_json<T: for<'de> Deserialize<'de>>(script: &str) -> Result<T, String> {
    let output = run_powershell(script)?;
    serde_json::from_str(output.trim_start_matches('\u{feff}'))
        .map_err(|_| "Não foi possível interpretar os dados dos serviços do Windows.".to_string())
}

#[cfg(target_os = "windows")]
fn run_powershell(script: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let encoded = base64_encode(
        &format!(
            "$utf8=[System.Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=$utf8;{script}"
        )
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>(),
    );
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|_| "Não foi possível consultar os serviços do Windows.".to_string())?;
    powershell_output_result(output.status.success(), &output.stdout, &output.stderr)
}

#[cfg(target_os = "windows")]
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

#[cfg(not(target_os = "windows"))]
pub(crate) struct WindowsServiceProfileBackend;

#[cfg(not(target_os = "windows"))]
impl Default for WindowsServiceProfileBackend {
    fn default() -> Self {
        Self
    }
}

#[cfg(not(target_os = "windows"))]
impl ServiceProfileBackend for WindowsServiceProfileBackend {
    fn detect_system(&mut self) -> Result<DetectedSystem, String> {
        Err("Detecção de serviços só está disponível no Windows.".to_string())
    }
    fn start_types(&mut self, _names: &[String]) -> Result<Vec<InstalledService>, String> {
        Err("Operação de serviços só está disponível no Windows.".to_string())
    }
    fn set_start_type(&mut self, _name: &str, _start_type: &str) -> Result<(), String> {
        Err("Operação de serviços só está disponível no Windows.".to_string())
    }
    fn load_snapshot(&mut self) -> Result<Option<ServiceSnapshot>, String> {
        Err("Operação de serviços só está disponível no Windows.".to_string())
    }
    fn save_snapshot(&mut self, _snapshot: &ServiceSnapshot) -> Result<(), String> {
        Err("Operação de serviços só está disponível no Windows.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeBackend {
        system: DetectedSystem,
        snapshot: Option<ServiceSnapshot>,
        saved: Vec<ServiceSnapshot>,
        changes: Vec<(String, String)>,
    }

    impl ServiceProfileBackend for FakeBackend {
        fn detect_system(&mut self) -> Result<DetectedSystem, String> {
            Ok(self.system.clone())
        }
        fn start_types(&mut self, names: &[String]) -> Result<Vec<InstalledService>, String> {
            Ok(self
                .system
                .services
                .iter()
                .filter(|service| names.contains(&service.name))
                .cloned()
                .collect())
        }
        fn set_start_type(&mut self, name: &str, start_type: &str) -> Result<(), String> {
            self.changes
                .push((name.to_string(), start_type.to_string()));
            if let Some(service) = self
                .system
                .services
                .iter_mut()
                .find(|item| item.name == name)
            {
                service.start_type = start_type.to_string();
            }
            Ok(())
        }
        fn load_snapshot(&mut self) -> Result<Option<ServiceSnapshot>, String> {
            Ok(self.snapshot.clone())
        }
        fn save_snapshot(&mut self, snapshot: &ServiceSnapshot) -> Result<(), String> {
            self.saved.push(snapshot.clone());
            self.snapshot = Some(snapshot.clone());
            Ok(())
        }
    }

    fn profile(os: WindowsVersion, edition: WindowsEdition) -> ServiceProfileFile {
        ServiceProfileFile {
            os,
            edition,
            form_factor: FormFactor::Any,
            validated: false,
            services: Vec::new(),
        }
    }

    fn system(os: WindowsVersion, edition: WindowsEdition) -> DetectedSystem {
        DetectedSystem {
            os,
            version: "22H2".to_string(),
            build: "19045".to_string(),
            edition_id: match &edition {
                WindowsEdition::Home => "Core",
                WindowsEdition::Pro => "Professional",
                WindowsEdition::ProForWorkstations => "ProfessionalWorkstation",
                WindowsEdition::Enterprise => "Enterprise",
                WindowsEdition::Education => "Education",
                WindowsEdition::Unknown => "UnknownEdition",
            }
            .to_string(),
            supported: edition != WindowsEdition::Unknown,
            edition,
            form_factor: Some(FormFactor::Desktop),
            services: vec![
                InstalledService {
                    name: "ExampleSvc".to_string(),
                    start_type: "Automatic".to_string(),
                },
                InstalledService {
                    name: "Dhcp".to_string(),
                    start_type: "Automatic".to_string(),
                },
                InstalledService {
                    name: "MysterySvc".to_string(),
                    start_type: "Manual".to_string(),
                },
            ],
        }
    }

    fn fake_backend(os: WindowsVersion, edition: WindowsEdition) -> FakeBackend {
        FakeBackend {
            system: system(os, edition),
            snapshot: None,
            saved: Vec::new(),
            changes: Vec::new(),
        }
    }

    #[test]
    fn profile_for_another_windows_version_or_edition_is_inapplicable() {
        let detected = system(WindowsVersion::Windows11, WindowsEdition::Pro);
        let mut wrong_version = profile(WindowsVersion::Windows10, WindowsEdition::Pro);
        wrong_version.validated = true;
        assert!(!preview(&detected, &wrong_version, ProductProfile::Safe,).applicable);
        let mut wrong_edition = profile(WindowsVersion::Windows11, WindowsEdition::Home);
        wrong_edition.validated = true;
        assert!(!preview(&detected, &wrong_edition, ProductProfile::Safe,).applicable);
    }

    #[test]
    fn unknown_edition_is_retained_and_never_applicable() {
        let (edition, supported) = detected_system_edition("FutureEditionID");
        assert_eq!(edition, WindowsEdition::Unknown);
        assert!(!supported);

        let mut backend = fake_backend(WindowsVersion::Windows11, edition);
        backend.system.edition_id = "FutureEditionID".to_string();
        let detected = backend.detect_system().unwrap();
        let profile = system_profile(&detected).unwrap();
        assert!(!profile.validated);
        assert!(profile.services.is_empty());
        assert!(detected
            .services
            .iter()
            .any(|service| service.name == "MysterySvc"));

        let preview = preview(&detected, &profile, ProductProfile::Safe);
        assert!(!preview.applicable);
        assert_eq!(
            preview.message,
            "Edição detectada: FutureEditionID. Não há perfil para ela."
        );
        assert!(preview.ignored.is_empty());
        assert!(preview
            .services
            .iter()
            .any(|service| service.service == "MysterySvc"
                && service.proposed.is_none()
                && !service.ignored));
    }

    #[test]
    fn empty_unvalidated_profile_lists_detected_services_without_changes() {
        let backend = fake_backend(
            WindowsVersion::Windows11,
            WindowsEdition::ProForWorkstations,
        );
        let profile = profile(
            WindowsVersion::Windows11,
            WindowsEdition::ProForWorkstations,
        );
        let preview = preview(&backend.system, &profile, ProductProfile::Safe);

        assert!(!preview.applicable);
        assert_eq!(
            preview.message,
            "Edição Pro for Workstations detectada (ProfessionalWorkstation). Perfil ainda não validado; nada será alterado."
        );
        assert_eq!(preview.services.len(), backend.system.services.len());
        assert!(preview.services.iter().all(|service| {
            service.proposed.is_none()
                && service.incompatible.is_none()
                && !service.ignored
                && service.current
                    == backend
                        .system
                        .services
                        .iter()
                        .find(|installed| installed.name == service.service)
                        .unwrap()
                        .start_type
        }));
        assert!(preview.ignored.is_empty());
    }

    #[test]
    fn empty_profile_cannot_apply_restore_or_undo() {
        let mut profile = profile(WindowsVersion::Windows10, WindowsEdition::Home);
        profile.validated = true;
        let mut backend = fake_backend(WindowsVersion::Windows10, WindowsEdition::Home);

        assert!(apply_profile(&mut backend, &profile, ProductProfile::Safe).is_err());
        assert!(restore_windows_defaults(&mut backend, &profile).is_err());
        assert!(undo_last_application(&mut backend, &profile).is_err());
        assert!(backend.changes.is_empty());
        assert!(backend.saved.is_empty());
    }

    #[test]
    fn edition_ids_map_to_the_requested_windows_families() {
        let cases = [
            ("Core", WindowsEdition::Home),
            ("CoreN", WindowsEdition::Home),
            ("CoreSingleLanguage", WindowsEdition::Home),
            ("CoreCountrySpecific", WindowsEdition::Home),
            ("Professional", WindowsEdition::Pro),
            ("ProfessionalN", WindowsEdition::Pro),
            (
                "ProfessionalWorkstation",
                WindowsEdition::ProForWorkstations,
            ),
            (
                "ProfessionalWorkstationN",
                WindowsEdition::ProForWorkstations,
            ),
            ("Enterprise", WindowsEdition::Enterprise),
            ("EnterpriseN", WindowsEdition::Enterprise),
            ("EnterpriseG", WindowsEdition::Enterprise),
            ("EnterpriseS", WindowsEdition::Enterprise),
            ("IoTEnterprise", WindowsEdition::Enterprise),
            ("Education", WindowsEdition::Education),
            ("EducationN", WindowsEdition::Education),
            ("ProfessionalEducation", WindowsEdition::Education),
        ];
        for (edition_id, expected) in cases {
            assert_eq!(detected_system_edition(edition_id), (expected, true));
        }
    }

    #[test]
    fn powershell_clixml_stderr_is_never_returned_to_the_interface() {
        let clixml = b"#< CLIXML\r\n<Objs><Obj S='Error'>Preparando m\xC3\xB3dulos para primeiro uso</Obj></Objs>";
        let error = powershell_output_result(false, b"", clixml).unwrap_err();
        assert_eq!(error, "Não foi possível consultar os serviços do Windows.");
        assert!(!error.contains("CLIXML"));
        assert!(!error.contains("Preparando módulos"));

        let stdout = b"{\"services\":[]}";
        assert_eq!(
            powershell_output_result(true, stdout, clixml).unwrap(),
            "{\"services\":[]}"
        );
    }

    #[test]
    fn unknown_service_is_ignored_and_left_unchanged() {
        let mut profile = profile(WindowsVersion::Windows10, WindowsEdition::Home);
        profile.validated = true;
        profile.services.push(ServiceRule {
            name: "ExampleSvc".to_string(),
            start_type: "Manual".to_string(),
            windows_default_start_type: None,
            profiles: vec![ProductProfile::Safe],
        });
        let mut backend = fake_backend(WindowsVersion::Windows10, WindowsEdition::Home);
        let result = preview(&backend.system, &profile, ProductProfile::Safe);
        assert!(result.ignored.contains(&"MysterySvc".to_string()));
        assert!(apply_profile(&mut backend, &profile, ProductProfile::Safe).is_ok());
        assert_eq!(
            backend
                .system
                .services
                .iter()
                .find(|service| service.name == "MysterySvc")
                .unwrap()
                .start_type,
            "Manual"
        );
        assert!(backend.changes.iter().all(|(name, _)| name != "MysterySvc"));
    }

    #[test]
    fn protected_services_cannot_be_disabled() {
        for name in ["wuauserv", "WinDefend", "Dhcp", "RpcSs", "PlugPlay"] {
            let mut profile = profile(WindowsVersion::Windows10, WindowsEdition::Home);
            profile.validated = true;
            profile.services.push(ServiceRule {
                name: name.to_string(),
                start_type: "Disabled".to_string(),
                windows_default_start_type: None,
                profiles: vec![ProductProfile::Safe],
            });
            let mut backend = fake_backend(WindowsVersion::Windows10, WindowsEdition::Home);
            assert!(apply_profile(&mut backend, &profile, ProductProfile::Safe).is_err());
            assert!(backend.changes.is_empty());
            assert!(backend.saved.is_empty());
        }
    }

    #[test]
    fn unvalidated_profile_cannot_apply_or_change_start_type() {
        let profile = profile(WindowsVersion::Windows10, WindowsEdition::Home);
        let mut backend = fake_backend(WindowsVersion::Windows10, WindowsEdition::Home);
        assert!(apply_profile(&mut backend, &profile, ProductProfile::Safe).is_err());
        assert!(backend.changes.is_empty());
        assert!(backend.saved.is_empty());
        assert_eq!(backend.system.services[0].start_type, "Automatic");
    }

    #[test]
    fn tweak_profile_metadata_does_not_validate_service_profiles() {
        let profiles = profile_files().unwrap();
        assert!(profiles.iter().all(|profile| !profile.validated));
        let profile = profiles
            .into_iter()
            .find(|profile| {
                profile.os == WindowsVersion::Windows10 && profile.edition == WindowsEdition::Home
            })
            .unwrap();
        let mut backend = fake_backend(WindowsVersion::Windows10, WindowsEdition::Home);
        assert!(apply_profile(&mut backend, &profile, ProductProfile::Safe).is_err());
        assert!(backend.changes.is_empty());
        assert!(backend.saved.is_empty());
    }

    #[test]
    fn undo_without_snapshot_fails_without_changing_service() {
        let mut profile = profile(WindowsVersion::Windows10, WindowsEdition::Home);
        profile.validated = true;
        let mut backend = fake_backend(WindowsVersion::Windows10, WindowsEdition::Home);
        assert!(undo_last_application(&mut backend, &profile).is_err());
        assert!(backend.changes.is_empty());
        assert_eq!(backend.system.services[0].start_type, "Automatic");
    }

    #[test]
    fn undo_restores_the_saved_start_type_for_profile_services() {
        let mut profile = profile(WindowsVersion::Windows10, WindowsEdition::Home);
        profile.validated = true;
        profile.services.push(ServiceRule {
            name: "ExampleSvc".to_string(),
            start_type: "Manual".to_string(),
            windows_default_start_type: Some("Automatic".to_string()),
            profiles: vec![ProductProfile::Safe],
        });
        let mut backend = fake_backend(WindowsVersion::Windows10, WindowsEdition::Home);
        backend.snapshot = Some(ServiceSnapshot {
            os: WindowsVersion::Windows10,
            build: "19045".to_string(),
            edition: WindowsEdition::Home,
            services: vec![InstalledService {
                name: "ExampleSvc".to_string(),
                start_type: "Manual".to_string(),
            }],
        });
        assert!(undo_last_application(&mut backend, &profile).is_ok());
        assert_eq!(
            backend.changes,
            vec![("ExampleSvc".to_string(), "Manual".to_string())]
        );
    }

    #[test]
    fn undo_rejects_protected_disabled_snapshot_before_changing_services() {
        let mut profile = profile(WindowsVersion::Windows10, WindowsEdition::Home);
        profile.validated = true;
        profile.services = ["ExampleSvc", "Dhcp"]
            .into_iter()
            .map(|name| ServiceRule {
                name: name.to_string(),
                start_type: "Manual".to_string(),
                windows_default_start_type: None,
                profiles: vec![ProductProfile::Safe],
            })
            .collect();
        let mut backend = fake_backend(WindowsVersion::Windows10, WindowsEdition::Home);
        backend.snapshot = Some(ServiceSnapshot {
            os: WindowsVersion::Windows10,
            build: "19045".to_string(),
            edition: WindowsEdition::Home,
            services: vec![
                InstalledService {
                    name: "ExampleSvc".to_string(),
                    start_type: "Manual".to_string(),
                },
                InstalledService {
                    name: "Dhcp".to_string(),
                    start_type: "Disabled".to_string(),
                },
            ],
        });
        assert!(undo_last_application(&mut backend, &profile).is_err());
        assert!(backend.changes.is_empty());
    }

    #[test]
    fn unvalidated_profile_cannot_restore_windows_defaults() {
        let profile = profile(WindowsVersion::Windows10, WindowsEdition::Home);
        let mut backend = fake_backend(WindowsVersion::Windows10, WindowsEdition::Home);
        assert!(restore_windows_defaults(&mut backend, &profile).is_err());
        assert!(backend.changes.is_empty());
        assert!(backend.saved.is_empty());
        assert_eq!(backend.system.services[0].start_type, "Automatic");
    }

    #[test]
    fn restore_windows_defaults_snapshots_before_restoring() {
        let mut profile = profile(WindowsVersion::Windows10, WindowsEdition::Home);
        profile.validated = true;
        profile.services.push(ServiceRule {
            name: "ExampleSvc".to_string(),
            start_type: "Manual".to_string(),
            windows_default_start_type: Some("Automatic".to_string()),
            profiles: vec![ProductProfile::Safe],
        });
        let mut backend = fake_backend(WindowsVersion::Windows10, WindowsEdition::Home);
        backend.system.services[0].start_type = "Manual".to_string();

        assert!(restore_windows_defaults(&mut backend, &profile).is_ok());
        assert_eq!(backend.saved[0].services[0].start_type, "Manual");
        assert_eq!(
            backend.changes,
            vec![("ExampleSvc".to_string(), "Automatic".to_string())]
        );
    }

    #[test]
    fn profile_templates_have_empty_unvalidated_matrices() {
        let profiles = profile_files().unwrap();
        assert_eq!(profiles.len(), 10);
        assert!(profiles
            .iter()
            .all(|profile| !profile.validated && profile.services.is_empty()));
        for os in [WindowsVersion::Windows10, WindowsVersion::Windows11] {
            for edition in [
                WindowsEdition::Home,
                WindowsEdition::Pro,
                WindowsEdition::ProForWorkstations,
                WindowsEdition::Enterprise,
                WindowsEdition::Education,
            ] {
                assert_eq!(
                    profiles
                        .iter()
                        .filter(|profile| profile.os == os && profile.edition == edition)
                        .count(),
                    1
                );
            }
        }
    }

    #[test]
    fn custom_profile_never_builds_an_applyable_preview() {
        let mut profile = profile(WindowsVersion::Windows10, WindowsEdition::Home);
        profile.validated = true;
        profile.services.push(ServiceRule {
            name: "ExampleSvc".to_string(),
            start_type: "Disabled".to_string(),
            windows_default_start_type: None,
            profiles: vec![ProductProfile::Custom],
        });
        let result = preview(
            &system(WindowsVersion::Windows10, WindowsEdition::Home),
            &profile,
            ProductProfile::Custom,
        );
        assert!(!result.applicable);
        assert!(result
            .services
            .iter()
            .all(|service| service.proposed.is_none()));
    }
}
