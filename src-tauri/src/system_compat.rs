use serde::Deserialize;
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompatibilityAssessment {
    /// True when a known modified distribution or a potentially restrictive
    /// Windows Update configuration is detected. This is deliberately conservative.
    pub(crate) use_conservative_repair: bool,
    pub(crate) summary: String,
    pub(crate) indicators: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Probe {
    product_name: Option<String>,
    image_version: Option<String>,
    edition_id: Option<String>,
    build_lab_ex: Option<String>,
    markers: Vec<String>,
    disabled_update_services: Vec<String>,
}

pub(crate) fn assess(
    product_name: Option<&str>,
    image_version: Option<&str>,
    edition_id: Option<&str>,
    build_lab_ex: Option<&str>,
    markers: &[String],
    disabled_services: &[String],
) -> CompatibilityAssessment {
    let identity = [
        product_name.unwrap_or_default(),
        image_version.unwrap_or_default(),
        edition_id.unwrap_or_default(),
        build_lab_ex.unwrap_or_default(),
    ]
    .join(" ")
    .to_lowercase();

    let mut indicators = Vec::new();
    if ["revios", "atlasos", "tiny11", "ghost spectre", "ghostspectre"]
        .iter()
        .any(|marker| identity.contains(marker))
    {
        indicators.push(format!(
            "Identificação do Windows contém um nome de distribuição modificada: {}",
            product_name.unwrap_or("nome não informado")
        ));
    }

    for marker in markers {
        if !marker.trim().is_empty() {
            indicators.push(format!("Indicador de distribuição modificada: {marker}"));
        }
    }

    for service in disabled_services {
        if !service.trim().is_empty() {
            indicators.push(format!(
                "Serviço relacionado a atualização/reparo desativado: {service}"
            ));
        }
    }

    let conservative = !indicators.is_empty();
    let summary = if conservative {
        "Ambiente modificado ou configuração restritiva possível; o QBoa usará uma rotina conservadora.".to_string()
    } else {
        "Nenhum indicador conhecido de Windows modificado foi encontrado; isso não garante que o sistema seja íntegro.".to_string()
    };

    CompatibilityAssessment {
        use_conservative_repair: conservative,
        summary,
        indicators,
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn detect() -> Result<CompatibilityAssessment, String> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$markers = @()
foreach ($key in @('HKLM:\SOFTWARE\ReviOS', 'HKLM:\SOFTWARE\AtlasOS')) {
  if (Test-Path -LiteralPath $key) { $markers += ($key -replace '^HKLM:\\', 'Registro: ') }
}
foreach ($path in @((Join-Path $env:ProgramData 'ReviOS'), (Join-Path $env:WINDIR 'ReviOS'))) {
  if (Test-Path -LiteralPath $path) { $markers += $path }
}
$disabled = @()
foreach ($name in @('wuauserv', 'BITS', 'cryptsvc', 'TrustedInstaller')) {
  $service = Get-CimInstance Win32_Service -Filter ("Name='" + $name + "'")
  if ($service -and ($service.StartMode -eq 'Disabled' -or $service.State -eq 'Stopped' -and $name -in @('wuauserv','BITS'))) {
    $disabled += ($name + ' (' + $service.StartMode + ', ' + $service.State + ')')
  }
}
[pscustomobject]@{
  product_name = [string]$cv.ProductName
  image_version = [string]$cv.ImageVersion
  edition_id = [string]$cv.EditionID
  build_lab_ex = [string]$cv.BuildLabEx
  markers = @($markers)
  disabled_update_services = @($disabled)
} | ConvertTo-Json -Depth 4 -Compress
"#;

    let mut command = Command::new("powershell");
    command.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().map_err(|error| {
        format!("Não foi possível verificar a compatibilidade do Windows: {error}")
    })?;
    if !output.status.success() {
        return Err(format!(
            "A verificação de compatibilidade terminou com código {:?}.",
            output.status.code()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let probe: Probe = serde_json::from_str(stdout.trim_start_matches('\u{feff}').trim())
        .map_err(|error| format!("Resposta da verificação de compatibilidade inválida: {error}"))?;

    Ok(assess(
        probe.product_name.as_deref(),
        probe.image_version.as_deref(),
        probe.edition_id.as_deref(),
        probe.build_lab_ex.as_deref(),
        &probe.markers,
        &probe.disabled_update_services,
    ))
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn detect() -> Result<CompatibilityAssessment, String> {
    Ok(assess(None, None, None, None, &[], &[]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_revi_os_in_windows_identity() {
        let result = assess(Some("ReviOS 10"), None, Some("Professional"), None, &[], &[]);
        assert!(result.use_conservative_repair);
        assert!(result.summary.contains("conservadora"));
    }

    #[test]
    fn recognizes_registry_markers_and_disabled_services() {
        let marker = vec!["Registro: HKLM:\\SOFTWARE\\ReviOS".to_string()];
        let services = vec!["wuauserv (Disabled, Stopped)".to_string()];
        let result = assess(Some("Windows 10 Pro"), None, None, None, &marker, &services);
        assert!(result.use_conservative_repair);
        assert_eq!(result.indicators.len(), 2);
    }

    #[test]
    fn standard_windows_identity_without_markers_is_not_flagged() {
        let result = assess(
            Some("Windows 10 Pro"),
            Some("22H2"),
            Some("Professional"),
            Some("19045.1.amd64fre"),
            &[],
            &[],
        );
        assert!(!result.use_conservative_repair);
        assert!(result.indicators.is_empty());
    }
}
