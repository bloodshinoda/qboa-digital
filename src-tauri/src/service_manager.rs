use serde::{Deserialize, Serialize};
use std::process::Command;

const PROTECTED_SERVICES: &[&str] = &[
    "RpcSs",
    "DcomLaunch",
    "PlugPlay",
    "EventLog",
    "SamSs",
    "LSM",
    "WinDefend",
    "wuauserv",
    "Dhcp",
    "BITS",
    "CryptSvc",
    "TrustedInstaller",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ServiceEntry {
    pub name: String,
    pub display_name: String,
    pub status: String,
    pub start_type: String,
    pub is_microsoft: bool,
    pub protected: bool,
    pub protection_reason: Option<String>,
}

#[derive(Deserialize)]
struct RawService {
    name: String,
    display_name: String,
    status: String,
    start_type: String,
    is_microsoft: bool,
}

pub(crate) fn list_services() -> Result<Vec<ServiceEntry>, String> {
    #[cfg(not(target_os = "windows"))]
    {
        return Err("A gestão de serviços só está disponível no Windows.".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let script = r#"
$ErrorActionPreference = 'Stop'
$services = @(Get-CimInstance Win32_Service -ErrorAction Stop | ForEach-Object {
  $start = switch ([string]$_.StartMode) {
    'Auto' { 'automatic' }
    'Manual' { 'manual' }
    'Disabled' { 'disabled' }
    default { 'unknown' }
  }
  $binaryPath = $null
  if ([string]$_.PathName -match '^"([^"]+\\.exe)"') {
    $binaryPath = $Matches[1]
  } elseif ([string]$_.PathName -match '^(.+?\\.exe)(?:\\s|$)') {
    $binaryPath = $Matches[1].Trim('"')
  }
  if ($binaryPath -and [IO.Path]::GetFileName($binaryPath) -ieq 'svchost.exe') {
    $serviceDll = (Get-ItemProperty -LiteralPath "HKLM:\\SYSTEM\\CurrentControlSet\\Services\\$($_.Name)\\Parameters" -Name ServiceDll -ErrorAction SilentlyContinue).ServiceDll
    if ($serviceDll) { $binaryPath = [Environment]::ExpandEnvironmentVariables([string]$serviceDll) }
  }
  $isMicrosoft = $false
  if ($binaryPath -and (Test-Path -LiteralPath $binaryPath -PathType Leaf)) {
    try {
      $signature = Get-AuthenticodeSignature -LiteralPath $binaryPath -ErrorAction Stop
      $isMicrosoft = $signature.Status -eq 'Valid' -and [string]$signature.SignerCertificate.Subject -match '(?i)(Microsoft Corporation|Microsoft Windows)'
    } catch { $isMicrosoft = $false }
  }
  [pscustomobject]@{
    name = [string]$_.Name
    display_name = [string]$_.DisplayName
    status = ([string]$_.State).ToLowerInvariant()
    start_type = $start
    is_microsoft = [bool]$isMicrosoft
  }
} | Sort-Object display_name)
ConvertTo-Json -InputObject $services -Depth 4 -Compress
"#;
        let output = run_powershell(script)?;
        let raw: Vec<RawService> = serde_json::from_str(
            output.trim_start_matches('\u{feff}').trim(),
        )
        .map_err(|error| format!("Não foi possível interpretar a lista de serviços: {error}"))?;
        Ok(raw
            .into_iter()
            .map(|service| {
                let protected = is_protected(&service.name);
                ServiceEntry {
                    name: service.name,
                    display_name: service.display_name,
                    status: service.status,
                    start_type: service.start_type,
                    is_microsoft: service.is_microsoft,
                    protected,
                    protection_reason: protected
                        .then(|| "Serviço crítico protegido pelo QBoa.".to_string()),
                }
            })
            .collect())
    }
}

pub(crate) fn manage_service(
    name: &str,
    action: &str,
    start_type: Option<&str>,
) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err("Nome de serviço inválido.".to_string());
    }

    match action {
        "start" | "stop" | "restart" => {
            if start_type.is_some() {
                return Err(
                    "Tipo de inicialização não deve ser informado para esta ação.".to_string(),
                );
            }
            if matches!(action, "stop" | "restart") && is_protected(name) {
                return Err(format!(
                    "Ação bloqueada: o serviço {name} é protegido pelo QBoa."
                ));
            }
        }
        "set_start_type" => {
            let requested =
                start_type.ok_or_else(|| "Informe o tipo de inicialização.".to_string())?;
            if !valid_start_type(requested) {
                return Err("Tipo de inicialização inválido.".to_string());
            }
            if requested.eq_ignore_ascii_case("disabled") && is_protected(name) {
                return Err(format!(
                    "Não é permitido desativar o serviço protegido {name}."
                ));
            }
        }
        _ => return Err("Ação de serviço desconhecida.".to_string()),
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (name, action, start_type);
        Err("A gestão de serviços só está disponível no Windows.".to_string())
    }

    #[cfg(target_os = "windows")]
    {
        let encoded_name = base64_encode(name.as_bytes());
        let script = match action {
            "start" => format!(
                r#"
$ErrorActionPreference='Stop'
$name=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_name}'))
$svc=Get-Service -Name $name -ErrorAction Stop
Start-Service -InputObject $svc -ErrorAction Stop
$svc.Refresh()
Write-Output ("Serviço {{0}}: {{1}}" -f $svc.Name, $svc.Status)
"#
            ),
            "stop" => format!(
                r#"
$ErrorActionPreference='Stop'
$name=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_name}'))
$svc=Get-Service -Name $name -ErrorAction Stop
Stop-Service -InputObject $svc -ErrorAction Stop
$svc.Refresh()
Write-Output ("Serviço {{0}}: {{1}}" -f $svc.Name, $svc.Status)
"#
            ),
            "restart" => format!(
                r#"
$ErrorActionPreference='Stop'
$name=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_name}'))
$svc=Get-Service -Name $name -ErrorAction Stop
Restart-Service -InputObject $svc -ErrorAction Stop
$svc.Refresh()
Write-Output ("Serviço {{0}}: {{1}}" -f $svc.Name, $svc.Status)
"#
            ),
            "set_start_type" => {
                let start = match start_type.unwrap_or_default().to_ascii_lowercase().as_str() {
                    "automatic" => "Automatic",
                    "manual" => "Manual",
                    "disabled" => "Disabled",
                    _ => return Err("Tipo de inicialização inválido.".to_string()),
                };
                format!(
                    r#"
$ErrorActionPreference='Stop'
$name=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_name}'))
$svc=Get-Service -Name $name -ErrorAction Stop
Set-Service -InputObject $svc -StartupType {start} -ErrorAction Stop
Write-Output ("Inicialização de {{0}} alterada para {{1}}" -f $svc.Name, '{start}')
"#
                )
            }
            _ => unreachable!("A ação foi validada antes da execução"),
        };
        run_powershell(&script)
    }
}

fn valid_start_type(value: &str) -> bool {
    ["automatic", "manual", "disabled"].contains(&value.to_ascii_lowercase().as_str())
}

fn is_protected(name: &str) -> bool {
    PROTECTED_SERVICES
        .iter()
        .any(|service| service.eq_ignore_ascii_case(name))
}

#[cfg(target_os = "windows")]
fn run_powershell(script: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
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
        .map_err(|error| format!("Não foi possível executar a operação de serviço: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            "A operação de serviço falhou. Verifique permissões e o estado do serviço.".to_string()
        } else {
            format!("A operação de serviço falhou: {detail}")
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_services_are_protected_case_insensitively() {
        assert!(is_protected("RpcSs"));
        assert!(is_protected("WUauserv"));
        assert!(!is_protected("Spooler"));
    }

    #[test]
    fn invalid_actions_and_start_types_are_rejected() {
        assert!(manage_service("Spooler", "delete", None).is_err());
        assert!(manage_service("Spooler", "set_start_type", Some("boot")).is_err());
        assert!(manage_service("RpcSs", "set_start_type", Some("disabled")).is_err());
        assert!(manage_service("RpcSs", "stop", None).is_err());
    }
}
