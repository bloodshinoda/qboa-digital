use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub(crate) fn repair() -> Result<String, String> {
    #[cfg(not(windows))]
    {
        return Err("O reparo do Windows Update só está disponível no Windows.".to_string());
    }

    #[cfg(windows)]
    {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            ENABLE_SCRIPT,
        ]);
        command.creation_flags(0x08000000);
        let output = command.output().map_err(|error| {
            format!("Não foi possível iniciar a recuperação do Windows Update: {error}")
        })?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if output.status.success() {
            Ok(if stdout.is_empty() {
                "Recuperação do Windows Update concluída.".to_string()
            } else {
                stdout
            })
        } else {
            let detail = if !stderr.is_empty() { stderr } else { stdout };
            Err(if detail.is_empty() {
                "A recuperação do Windows Update falhou.".to_string()
            } else {
                detail
            })
        }
    }
}

#[cfg(any(windows, test))]
const ENABLE_SCRIPT: &str = r#"
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw 'Execute o QBoa Digital como administrador para recuperar o Windows Update.'
}

$messages = [System.Collections.Generic.List[string]]::new()
$warnings = [System.Collections.Generic.List[string]]::new()

# Remove apenas valores locais conhecidos por bloquear a experiência do Windows Update.
# Mantém políticas de WSUS, MDM e demais configurações que não são bloqueios explícitos.
$policyTargets = @(
  @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate'; Names = @('DisableWindowsUpdateAccess', 'SetDisableUXWUAccess') },
  @{ Path = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU'; Names = @('NoAutoUpdate') }
)
foreach ($target in $policyTargets) {
  if (-not (Test-Path -LiteralPath $target.Path)) { continue }
  foreach ($valueName in $target.Names) {
    $value = Get-ItemProperty -LiteralPath $target.Path -Name $valueName -ErrorAction SilentlyContinue
    if ($null -ne $value -and $null -ne $value.$valueName) {
      Remove-ItemProperty -LiteralPath $target.Path -Name $valueName -ErrorAction Stop
      $messages.Add("Bloqueio local removido: $valueName.")
    }
  }
}

$services = @(
  @{ Name = 'wuauserv'; Start = 'demand'; Required = $true },
  @{ Name = 'BITS'; Start = 'demand'; Required = $true },
  @{ Name = 'CryptSvc'; Start = 'auto'; Required = $true },
  @{ Name = 'UsoSvc'; Start = 'demand'; Required = $false },
  @{ Name = 'DoSvc'; Start = 'demand'; Required = $false },
  @{ Name = 'WaaSMedicSvc'; Start = 'demand'; Required = $false },
  @{ Name = 'InstallService'; Start = 'demand'; Required = $false }
)
foreach ($item in $services) {
  $service = Get-Service -Name $item.Name -ErrorAction SilentlyContinue
  if (-not $service) {
    if ($item.Required) { throw "Serviço essencial ausente: $($item.Name)." }
    $warnings.Add("Serviço opcional não encontrado: $($item.Name).")
    continue
  }
  & sc.exe config $item.Name 'start=' $item.Start | Out-Null
  if ($LASTEXITCODE -ne 0) {
    if ($item.Required) { throw "Não foi possível reconfigurar o serviço essencial $($item.Name)." }
    $warnings.Add("O Windows protegeu a configuração de $($item.Name); mantida a configuração atual.")
  } else {
    $messages.Add("Inicialização de $($item.Name) configurada.")
  }
}

foreach ($name in @('CryptSvc', 'BITS', 'wuauserv')) {
  try {
    $service = Get-Service -Name $name -ErrorAction Stop
    if ($service.Status -ne 'Running') {
      Start-Service -Name $name -ErrorAction Stop
    }
    $messages.Add("Serviço essencial disponível: $name.")
  } catch {
    throw "Não foi possível iniciar o serviço essencial ${name}: $($_.Exception.Message)"
  }
}

foreach ($taskPath in @('\Microsoft\Windows\WindowsUpdate\', '\Microsoft\Windows\UpdateOrchestrator\')) {
  try {
    $tasks = @(Get-ScheduledTask -TaskPath $taskPath -ErrorAction SilentlyContinue)
    foreach ($task in $tasks) {
      try {
        Enable-ScheduledTask -InputObject $task -ErrorAction Stop | Out-Null
      } catch {
        $warnings.Add("Não foi possível reativar a tarefa $($task.TaskName): $($_.Exception.Message)")
      }
    }
    if ($tasks.Count -gt 0) {
      $messages.Add("Tarefas de atualização verificadas em $taskPath.")
    }
  } catch {
    $warnings.Add("Não foi possível consultar as tarefas em ${taskPath}: $($_.Exception.Message)")
  }
}

$messages | ForEach-Object { Write-Output $_ }
$warnings | ForEach-Object { Write-Output "Aviso: $_" }
Write-Output 'Recuperação local concluída. Políticas corporativas ou de gerenciamento podem ser reaplicadas pelo Windows.'
"#;

#[cfg(test)]
mod tests {
    use super::ENABLE_SCRIPT;

    #[test]
    fn repair_removes_only_known_local_blocking_policy_values() {
        assert!(ENABLE_SCRIPT.contains("'DisableWindowsUpdateAccess'"));
        assert!(ENABLE_SCRIPT.contains("'SetDisableUXWUAccess'"));
        assert!(ENABLE_SCRIPT.contains("'NoAutoUpdate'"));
        assert!(!ENABLE_SCRIPT.contains("Remove-Item -LiteralPath $policyPath -Recurse"));
    }

    #[test]
    fn repair_reconfigures_and_starts_required_services() {
        for service in ["wuauserv", "BITS", "CryptSvc"] {
            assert!(ENABLE_SCRIPT.contains(&format!("Name = '{service}'")));
        }
        assert!(ENABLE_SCRIPT.contains("foreach ($name in @('CryptSvc', 'BITS', 'wuauserv'))"));
        assert!(ENABLE_SCRIPT.contains("Start-Service -Name $name"));
    }

    #[test]
    fn repair_delimits_interpolated_powershell_variables() {
        assert!(ENABLE_SCRIPT.contains("${name}:"));
        assert!(ENABLE_SCRIPT.contains("${taskPath}:"));
        assert!(!ENABLE_SCRIPT.contains("$name:"));
        assert!(!ENABLE_SCRIPT.contains("$taskPath:"));
    }

    #[cfg(windows)]
    #[test]
    fn embedded_powershell_script_parses_without_execution() {
        let parser = "$tokens = $null; $errors = $null; [System.Management.Automation.Language.Parser]::ParseInput($env:QBOA_TEST_SCRIPT, [ref]$tokens, [ref]$errors) | Out-Null; if ($errors.Count -gt 0) { $errors | ForEach-Object { Write-Error $_ }; exit 1 }";
        let output = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                parser,
            ])
            .env("QBOA_TEST_SCRIPT", ENABLE_SCRIPT)
            .output()
            .expect("PowerShell deve estar disponível no runner Windows.");
        assert!(
            output.status.success(),
            "O script de recuperação contém erro de sintaxe: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repair_sets_utf8_output_encoding() {
        assert!(ENABLE_SCRIPT.contains("[Console]::OutputEncoding"));
        assert!(ENABLE_SCRIPT.contains("$OutputEncoding"));
    }

    fn repair_reports_policy_management_limitations() {
        assert!(ENABLE_SCRIPT.contains("Políticas corporativas ou de gerenciamento"));
    }
}
