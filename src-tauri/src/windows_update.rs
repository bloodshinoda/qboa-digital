use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub(crate) fn set_enabled(enabled: bool) -> Result<String, String> {
    #[cfg(not(windows))]
    {
        let _ = enabled;
        return Err("O reparo do Windows Update só está disponível no Windows.".to_string());
    }

    #[cfg(windows)]
    {
        let script = if enabled {
            ENABLE_SCRIPT
        } else {
            DISABLE_SCRIPT
        };
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ]);
        command.creation_flags(0x08000000);
        let output = command.output().map_err(|error| {
            format!("Não foi possível iniciar o reparo do Windows Update: {error}")
        })?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if output.status.success() {
            Ok(if stdout.is_empty() {
                "Operação do Windows Update concluída.".to_string()
            } else {
                stdout
            })
        } else {
            let detail = if !stderr.is_empty() { stderr } else { stdout };
            Err(if detail.is_empty() {
                "A operação do Windows Update falhou.".to_string()
            } else {
                detail
            })
        }
    }
}

#[cfg(windows)]
const ENABLE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw 'Execute o QBoa Digital como administrador para restaurar o Windows Update.'
}

$messages = [System.Collections.Generic.List[string]]::new()
$policyPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate'
if (Test-Path -LiteralPath $policyPath) {
  Remove-Item -LiteralPath $policyPath -Recurse -Force
  $messages.Add('Políticas locais de bloqueio do Windows Update removidas.')
} else {
  $messages.Add('Nenhuma política local de bloqueio do Windows Update encontrada.')
}

$services = @(
  @{ Name = 'wuauserv'; Start = 'demand' },
  @{ Name = 'BITS'; Start = 'demand' },
  @{ Name = 'CryptSvc'; Start = 'auto' },
  @{ Name = 'UsoSvc'; Start = 'demand' },
  @{ Name = 'DoSvc'; Start = 'demand' },
  @{ Name = 'WaaSMedicSvc'; Start = 'demand' },
  @{ Name = 'InstallService'; Start = 'demand' }
)
foreach ($item in $services) {
  $service = Get-Service -Name $item.Name -ErrorAction SilentlyContinue
  if (-not $service) { continue }
  & sc.exe config $item.Name 'start=' $item.Start 2>&1 | Out-Null
  if ($LASTEXITCODE -eq 0) {
    $messages.Add("Inicialização de $($item.Name) reconfigurada.")
  } else {
    $messages.Add("O Windows protegeu a configuração de $($item.Name); o serviço não pôde ser reconfigurado.")
  }
}
foreach ($name in @('CryptSvc','BITS','wuauserv','UsoSvc','DoSvc')) {
  try {
    $service = Get-Service -Name $name -ErrorAction Stop
    if ($service.Status -ne 'Running') {
      Start-Service -Name $name -ErrorAction Stop
    }
    $messages.Add("Serviço $name em execução.")
  } catch {
    $messages.Add("Não foi possível iniciar $name agora: $($_.Exception.Message)")
  }
}
foreach ($taskPath in @('\\Microsoft\\Windows\\WindowsUpdate\\', '\\Microsoft\\Windows\\UpdateOrchestrator\\')) {
  try {
    $tasks = @(Get-ScheduledTask -TaskPath $taskPath -ErrorAction SilentlyContinue)
    foreach ($task in $tasks) {
      try { Enable-ScheduledTask -InputObject $task -ErrorAction Stop | Out-Null } catch {}
    }
    if ($tasks.Count -gt 0) { $messages.Add("Tarefas agendadas de $taskPath habilitadas.") }
  } catch {}
}
$messages.Add('O Windows Update foi colocado novamente em condição de funcionamento. Abra Configurações > Windows Update e procure atualizações.')
$messages | ForEach-Object { Write-Output $_ }
"#;

#[cfg(windows)]
const DISABLE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw 'Execute o QBoa Digital como administrador para alterar o Windows Update.'
}

$policyPath = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate'
$auPath = Join-Path $policyPath 'AU'
New-Item -Path $auPath -Force | Out-Null
New-ItemProperty -Path $auPath -Name NoAutoUpdate -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty -Path $policyPath -Name DisableWindowsUpdateAccess -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty -Path $policyPath -Name SetDisableUXWUAccess -PropertyType DWord -Value 1 -Force | Out-Null

$messages = [System.Collections.Generic.List[string]]::new()
foreach ($name in @('wuauserv','UsoSvc')) {
  if (Get-Service -Name $name -ErrorAction SilentlyContinue) {
    & sc.exe config $name 'start=' 'disabled' 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) {
      $messages.Add("Inicialização de $name desabilitada.")
    } else {
      $messages.Add("O Windows protegeu a configuração de $name; o bloqueio por política permanece aplicado.")
    }
    try { Stop-Service -Name $name -Force -ErrorAction Stop } catch {}
  }
}
foreach ($taskPath in @('\\Microsoft\\Windows\\WindowsUpdate\\', '\\Microsoft\\Windows\\UpdateOrchestrator\\')) {
  try {
    Get-ScheduledTask -TaskPath $taskPath -ErrorAction SilentlyContinue |
      Disable-ScheduledTask -ErrorAction SilentlyContinue | Out-Null
  } catch {}
}
$messages.Add('Atualizações automáticas bloqueadas por política local. Para voltar ao normal, escolha “Ser gente” no QBoa.')
$messages | ForEach-Object { Write-Output $_ }
"#;
