use crate::safety::{
    ChangeRecord, RegistryExistence, RegistrySnapshot, SafetyManager, SystemSnapshot,
};
#[cfg(test)]
use crate::safety::{FeatureSnapshot, ScheduledTaskSnapshot, ServiceSnapshot};
use crate::task_registry::{ExecutorType, TaskRegistry};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RegistryKey {
    pub hive: String,
    pub path: String,
    pub value_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RegistryTree {
    pub hive: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RegistryValue {
    pub value_type: String,
    pub value: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RegistryMutation {
    pub key: RegistryKey,
    pub value: RegistryValue,
}

pub(crate) trait RegistryBackend {
    fn read_values(&mut self, keys: &[RegistryKey]) -> Result<Vec<Option<RegistryValue>>, String>;
    fn write_values(&mut self, mutations: &[RegistryMutation]) -> Result<(), String>;
    fn delete_values(&mut self, keys: &[RegistryKey]) -> Result<(), String>;
    fn read_tree_values(&mut self, trees: &[RegistryTree])
        -> Result<Vec<RegistrySnapshot>, String>;
    fn key_trees_exist(&mut self, trees: &[RegistryTree]) -> Result<Vec<bool>, String>;
    fn delete_key_trees(&mut self, trees: &[RegistryTree]) -> Result<(), String>;
}

pub(crate) fn execute_registry_task<B: RegistryBackend>(
    task_id: &str,
    catalog: &TaskRegistry,
    execution_id: &str,
    snapshot_location: &str,
    safety: &mut SafetyManager,
    backend: &mut B,
) -> Result<ChangeRecord, String> {
    let task = catalog
        .resolve(task_id)
        .ok_or_else(|| format!("Tarefa desconhecida para executor de Registro: {task_id}"))?;
    if task.executor != ExecutorType::Registry {
        return Err(format!("Tarefa {task_id} não usa o executor de Registro."));
    }
    let mutations = registry_mutations(task_id);
    let trees = registry_trees(task_id);
    if mutations.is_none() && trees.is_none() {
        return Err(format!(
            "Tarefa {task_id} não tem operação de Registro permitida."
        ));
    }
    let snapshot: SystemSnapshot = serde_json::from_slice(
        &fs::read(snapshot_location)
            .map_err(|error| format!("Snapshot indisponível para {task_id}: {error}"))?,
    )
    .map_err(|error| format!("Snapshot inválido para {task_id}: {error}"))?;
    let expected_strategy = if mutations.is_some() {
        if !task.reversible || task.rollback_strategy != "RestoreRegistry" {
            return Err(format!(
                "Tarefa {task_id} não tem snapshot de Registro compatível."
            ));
        }
        "telemetry"
    } else {
        "mru_cleanup"
    };
    if snapshot.strategy != expected_strategy {
        return Err(format!(
            "Tarefa {task_id}: estratégia de snapshot incompatível."
        ));
    }
    if let Some(mutations) = mutations {
        let keys: Vec<RegistryKey> = mutations.iter().map(|item| item.key.clone()).collect();
        let before = values_from_snapshot(task_id, &snapshot, &keys)?;
        if backend.read_values(&keys)? != before {
            return Err(format!(
                "Tarefa {task_id}: o Registro mudou após a captura do snapshot."
            ));
        }

        if let Err(error) = backend.write_values(&mutations) {
            return Err(compensate_after_failure(task_id, &snapshot, backend, error));
        }

        let after = match backend.read_values(&keys) {
            Ok(after) => after,
            Err(error) => {
                return Err(compensate_after_failure(task_id, &snapshot, backend, error));
            }
        };
        let expected: Vec<Option<RegistryValue>> = mutations
            .iter()
            .map(|mutation| Some(mutation.value.clone()))
            .collect();
        if after != expected {
            return Err(compensate_after_failure(
                task_id,
                &snapshot,
                backend,
                "A leitura de volta do Registro divergiu do valor proposto.".to_string(),
            ));
        }
    } else if let Some(trees) = trees {
        let mut before = backend.read_tree_values(&trees)?;
        let mut captured = snapshot.registry.clone();
        sort_registry_values(&mut before);
        sort_registry_values(&mut captured);
        if before != captured {
            return Err(format!(
                "Tarefa {task_id}: o Registro mudou após a captura do snapshot."
            ));
        }

        if let Err(error) = backend.delete_key_trees(&trees) {
            return Err(compensate_after_failure(task_id, &snapshot, backend, error));
        }
        match backend.key_trees_exist(&trees) {
            Ok(exists) if exists.iter().all(|exists| !exists) => {}
            Ok(_) => {
                return Err(compensate_after_failure(
                    task_id,
                    &snapshot,
                    backend,
                    "A leitura de volta confirmou que uma chave de limpeza ainda existe."
                        .to_string(),
                ));
            }
            Err(error) => {
                return Err(compensate_after_failure(task_id, &snapshot, backend, error));
            }
        }
    }

    match safety.record_change(
        execution_id,
        task_id,
        &task.name,
        task.reversible,
        Some(snapshot_location),
        "registry_applied",
    ) {
        Ok(change) => Ok(change),
        Err(error) => Err(compensate_after_failure(
            task_id,
            &snapshot,
            backend,
            format!(
                "Não foi possível persistir a alteração no journal: {}",
                error.error
            ),
        )),
    }
}

fn values_from_snapshot(
    task_id: &str,
    snapshot: &SystemSnapshot,
    keys: &[RegistryKey],
) -> Result<Vec<Option<RegistryValue>>, String> {
    keys.iter()
        .map(|key| {
            let entry = snapshot
                .registry
                .iter()
                .find(|entry| {
                    entry.hive == key.hive
                        && entry.path == key.path
                        && entry.value_name == key.value_name
                })
                .ok_or_else(|| {
                    format!(
                        "Tarefa {task_id}: snapshot não contém {}\\{}\\{}.",
                        key.hive, key.path, key.value_name
                    )
                })?;
            match entry.existence {
                RegistryExistence::Absent => Ok(None),
                RegistryExistence::Present => Ok(Some(RegistryValue {
                    value_type: entry.value_type.clone().ok_or_else(|| {
                        format!(
                            "Tarefa {task_id}: snapshot sem tipo para {}.",
                            key.value_name
                        )
                    })?,
                    value: entry.value.clone().ok_or_else(|| {
                        format!(
                            "Tarefa {task_id}: snapshot sem valor bruto para {}.",
                            key.value_name
                        )
                    })?,
                })),
            }
        })
        .collect()
}

fn sort_registry_values(values: &mut [RegistrySnapshot]) {
    values.sort_by(|left, right| {
        (&left.hive, &left.path, &left.value_name).cmp(&(
            &right.hive,
            &right.path,
            &right.value_name,
        ))
    });
}

fn compensate_after_failure<B: RegistryBackend>(
    task_id: &str,
    snapshot: &SystemSnapshot,
    backend: &mut B,
    error: String,
) -> String {
    let restore = if snapshot.strategy == "mru_cleanup" {
        match registry_trees(task_id) {
            Some(trees) => backend
                .delete_key_trees(&trees)
                .and_then(|()| restore_registry_snapshot(snapshot, backend))
                .and_then(|()| {
                    let mut actual = backend.read_tree_values(&trees)?;
                    let mut expected = snapshot.registry.clone();
                    sort_registry_values(&mut actual);
                    sort_registry_values(&mut expected);
                    if actual == expected {
                        Ok(())
                    } else {
                        Err("Os valores das chaves MRU não foram restaurados.".to_string())
                    }
                }),
            None => Err(format!("Tarefa {task_id} sem chaves MRU para compensação.")),
        }
    } else {
        restore_registry_snapshot(snapshot, backend)
    };
    match restore {
        Ok(()) => format!("Tarefa {task_id}: {error}; valores anteriores restaurados."),
        Err(restore_error) => {
            format!("Tarefa {task_id}: {error}; falha ao restaurar snapshot: {restore_error}")
        }
    }
}

fn restore_registry_snapshot<B: RegistryBackend>(
    snapshot: &SystemSnapshot,
    backend: &mut B,
) -> Result<(), String> {
    let mut present = Vec::new();
    let mut absent = Vec::new();
    for entry in &snapshot.registry {
        let key = RegistryKey {
            hive: entry.hive.clone(),
            path: entry.path.clone(),
            value_name: entry.value_name.clone(),
        };
        match entry.existence {
            RegistryExistence::Absent => absent.push(key),
            RegistryExistence::Present => {
                let value_type = entry
                    .value_type
                    .clone()
                    .ok_or_else(|| format!("Snapshot sem tipo: {}", entry.value_name))?;
                let value = entry.value.clone().ok_or_else(|| {
                    format!(
                        "Snapshot sem valor bruto para valor presente: {}",
                        entry.value_name
                    )
                })?;
                present.push(RegistryMutation {
                    key,
                    value: RegistryValue { value_type, value },
                });
            }
        }
    }
    if !present.is_empty() {
        backend.write_values(&present)?;
    }
    if !absent.is_empty() {
        backend.delete_values(&absent)?;
    }

    let keys: Vec<RegistryKey> = snapshot
        .registry
        .iter()
        .map(|entry| RegistryKey {
            hive: entry.hive.clone(),
            path: entry.path.clone(),
            value_name: entry.value_name.clone(),
        })
        .collect();
    let actual = backend.read_values(&keys)?;
    let expected: Vec<Option<RegistryValue>> = snapshot
        .registry
        .iter()
        .map(|entry| match entry.existence {
            RegistryExistence::Absent => Some(None),
            RegistryExistence::Present => Some(Some(RegistryValue {
                value_type: entry.value_type.clone()?,
                value: entry.value.clone()?,
            })),
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "Snapshot de Registro incompleto.".to_string())?;
    if actual != expected {
        return Err("A validação do rollback de Registro divergiu do snapshot.".to_string());
    }
    Ok(())
}

#[cfg(test)]
fn registry_system_snapshot(
    task_id: &str,
    keys: &[RegistryKey],
    values: Vec<Option<RegistryValue>>,
) -> Result<SystemSnapshot, String> {
    if task_id != "desengordurar_telemetria" {
        return Err(format!(
            "Tarefa {task_id} sem estratégia de snapshot de Registro."
        ));
    }
    let registry = keys
        .iter()
        .zip(values)
        .map(|(key, value)| match value {
            Some(value) => RegistrySnapshot {
                hive: key.hive.clone(),
                path: key.path.clone(),
                value_name: key.value_name.clone(),
                existence: RegistryExistence::Present,
                value_type: Some(value.value_type),
                value: Some(value.value),
            },
            None => RegistrySnapshot {
                hive: key.hive.clone(),
                path: key.path.clone(),
                value_name: key.value_name.clone(),
                existence: RegistryExistence::Absent,
                value_type: None,
                value: None,
            },
        })
        .collect();

    Ok(SystemSnapshot {
        strategy: "telemetry".to_string(),
        registry,
        services: Vec::<ServiceSnapshot>::new(),
        scheduled_tasks: Vec::<ScheduledTaskSnapshot>::new(),
        features: Vec::<FeatureSnapshot>::new(),
    })
}

fn registry_mutations(task_id: &str) -> Option<Vec<RegistryMutation>> {
    if task_id != "desengordurar_telemetria" {
        return None;
    }
    let values = [
        (
            "LocalMachine",
            r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "AllowTelemetry",
            0,
        ),
        (
            "LocalMachine",
            r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "AllowDeviceNameInTelemetry",
            0,
        ),
        (
            "LocalMachine",
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection",
            "AllowTelemetry",
            0,
        ),
        (
            "LocalMachine",
            r"SOFTWARE\Wow6432Node\Microsoft\Windows\CurrentVersion\Policies\DataCollection",
            "AllowTelemetry",
            0,
        ),
        (
            "LocalMachine",
            r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "DoNotShowFeedbackNotifications",
            1,
        ),
        (
            "LocalMachine",
            r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "DisableTelemetryOptInChangeNotification",
            1,
        ),
        (
            "LocalMachine",
            r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "DisableEnterpriseAuthProxy",
            1,
        ),
        (
            "LocalMachine",
            r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI",
            "DisableAIDataAnalysis",
            1,
        ),
        (
            "LocalMachine",
            r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI",
            "TurnOffWindowsCopilot",
            1,
        ),
        (
            "CurrentUser",
            r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI",
            "DisableAIDataAnalysis",
            1,
        ),
        (
            "LocalMachine",
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
            "EnableRecallOnDevice",
            0,
        ),
        (
            "CurrentUser",
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
            "EnableRecallOnDevice",
            0,
        ),
    ];
    Some(
        values
            .into_iter()
            .map(|(hive, path, value_name, value)| RegistryMutation {
                key: RegistryKey {
                    hive: hive.to_string(),
                    path: path.to_string(),
                    value_name: value_name.to_string(),
                },
                value: RegistryValue {
                    value_type: "DWord".to_string(),
                    value: serde_json::json!(value),
                },
            })
            .collect(),
    )
}

fn registry_trees(task_id: &str) -> Option<Vec<RegistryTree>> {
    if !matches!(task_id, "limpeza_leve" | "limpeza_media" | "limpeza_pesada") {
        return None;
    }
    Some(
        [
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\RecentDocs",
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU",
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths",
        ]
        .into_iter()
        .map(|path| RegistryTree {
            hive: "CurrentUser".to_string(),
            path: path.to_string(),
        })
        .collect(),
    )
}

#[cfg(target_os = "windows")]
pub(crate) struct PowerShellRegistryBackend;

#[cfg(target_os = "windows")]
impl RegistryBackend for PowerShellRegistryBackend {
    fn read_values(&mut self, keys: &[RegistryKey]) -> Result<Vec<Option<RegistryValue>>, String> {
        #[derive(Deserialize)]
        struct ReadResult {
            exists: bool,
            value_type: Option<String>,
            value: Option<serde_json::Value>,
        }

        let encoded = serde_json::to_string(keys).map_err(|error| error.to_string())?;
        let script = format!(
            "$ErrorActionPreference='Stop';$specs=ConvertFrom-Json -InputObject '{}';$r=@();foreach($s in $specs){{$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$($s.hive),[Microsoft.Win32.RegistryView]::Default);$k=$b.OpenSubKey($s.path);$e=$k -and ($k.GetValueNames()-contains $s.value_name);$v=$null;$t=$null;if($e){{$t=$k.GetValueKind($s.value_name).ToString();$v=$k.GetValue($s.value_name,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);if($t-eq'Binary' -or $t-eq'None'){{$v=[Convert]::ToBase64String($v)}}}};$r+=@{{exists=$e;value_type=$t;value=$v}}}};ConvertTo-Json -InputObject @($r) -Depth 8 -Compress",
            powershell_literal(&encoded)
        );
        let output = run_powershell_script(&script)?;
        let result: Vec<ReadResult> =
            serde_json::from_str(output.trim_start_matches('\u{feff}'))
                .map_err(|error| format!("Resposta de leitura do Registro inválida: {error}"))?;
        result
            .into_iter()
            .map(|item| {
                if item.exists {
                    Ok(Some(RegistryValue {
                        value_type: item
                            .value_type
                            .ok_or_else(|| "Valor presente do Registro sem tipo.".to_string())?,
                        value: item.value.ok_or_else(|| {
                            "Valor presente do Registro sem valor bruto.".to_string()
                        })?,
                    }))
                } else {
                    Ok(None)
                }
            })
            .collect()
    }

    fn write_values(&mut self, mutations: &[RegistryMutation]) -> Result<(), String> {
        let supported_types = [
            "Binary",
            "DWord",
            "ExpandString",
            "MultiString",
            "None",
            "QWord",
            "String",
        ];
        for mutation in mutations {
            if !supported_types.contains(&mutation.value.value_type.as_str()) {
                return Err(format!(
                    "Tipo de Registro não suportado no snapshot: {}.",
                    mutation.value.value_type
                ));
            }
        }
        let encoded = serde_json::to_string(mutations).map_err(|error| error.to_string())?;
        let script = format!(
            "$ErrorActionPreference='Stop';$items=ConvertFrom-Json -InputObject '{}';foreach($x in $items){{$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$($x.key.hive),[Microsoft.Win32.RegistryView]::Default);$k=$b.CreateSubKey($x.key.path);$kind=[Microsoft.Win32.RegistryValueKind]::$($x.value.value_type);$v=$x.value.value;switch($x.value.value_type){{'Binary'{{$v=[Convert]::FromBase64String([string]$v)}}'None'{{$v=[Convert]::FromBase64String([string]$v)}}'DWord'{{$v=[int]$v}}'QWord'{{$v=[long]$v}}'MultiString'{{$v=[string[]]$v}}default{{$v=[string]$v}}}};$k.SetValue($x.key.value_name,$v,$kind)}};",
            powershell_literal(&encoded)
        );
        run_powershell_script(&script).map(|_| ())
    }

    fn delete_values(&mut self, keys: &[RegistryKey]) -> Result<(), String> {
        let encoded = serde_json::to_string(keys).map_err(|error| error.to_string())?;
        let script = format!(
            "$ErrorActionPreference='Stop';$items=ConvertFrom-Json -InputObject '{}';foreach($x in $items){{$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$($x.hive),[Microsoft.Win32.RegistryView]::Default);$k=$b.OpenSubKey($x.path,$true);if($k){{$k.DeleteValue($x.value_name,$false)}}}};",
            powershell_literal(&encoded)
        );
        run_powershell_script(&script).map(|_| ())
    }

    fn read_tree_values(
        &mut self,
        trees: &[RegistryTree],
    ) -> Result<Vec<RegistrySnapshot>, String> {
        let encoded = serde_json::to_string(trees).map_err(|error| error.to_string())?;
        let script = format!(
            "$ErrorActionPreference='Stop';$roots=ConvertFrom-Json -InputObject '{}';$r=@();foreach($root in $roots){{$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$($root.hive),[Microsoft.Win32.RegistryView]::Default);$queue=New-Object System.Collections.Queue;$queue.Enqueue($root.path);while($queue.Count -gt 0){{$p=[string]$queue.Dequeue();$k=$b.OpenSubKey($p);if($k){{foreach($n in $k.GetValueNames()){{$t=$k.GetValueKind($n).ToString();$v=$k.GetValue($n,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);if($t-eq'Binary' -or $t-eq'None'){{$v=[Convert]::ToBase64String($v)}};$r+=[pscustomobject]@{{hive=$root.hive;path=$p;value_name=$n;exists=$true;value_type=$t;value=$v}}}};foreach($n in $k.GetSubKeyNames()){{$queue.Enqueue(\"$p\\$n\")}};$k.Dispose()}}}};ConvertTo-Json -InputObject @($r) -Depth 8 -Compress",
            powershell_literal(&encoded)
        );
        let output = run_powershell_script(&script)?;
        let mut result: Vec<RegistrySnapshot> =
            serde_json::from_str(output.trim_start_matches('\u{feff}'))
                .map_err(|error| format!("Resposta de leitura da árvore inválida: {error}"))?;
        sort_registry_values(&mut result);
        Ok(result)
    }

    fn key_trees_exist(&mut self, trees: &[RegistryTree]) -> Result<Vec<bool>, String> {
        let encoded = serde_json::to_string(trees).map_err(|error| error.to_string())?;
        let script = format!(
            "$ErrorActionPreference='Stop';$roots=ConvertFrom-Json -InputObject '{}';$r=@();foreach($root in $roots){{$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$($root.hive),[Microsoft.Win32.RegistryView]::Default);$k=$b.OpenSubKey($root.path);$r+=([bool]$k);if($k){{$k.Dispose()}}}};ConvertTo-Json -InputObject @($r) -Compress",
            powershell_literal(&encoded)
        );
        let output = run_powershell_script(&script)?;
        serde_json::from_str(output.trim_start_matches('\u{feff}'))
            .map_err(|error| format!("Resposta de validação de chaves inválida: {error}"))
    }

    fn delete_key_trees(&mut self, trees: &[RegistryTree]) -> Result<(), String> {
        let encoded = serde_json::to_string(trees).map_err(|error| error.to_string())?;
        let script = format!(
            "$ErrorActionPreference='Stop';$roots=ConvertFrom-Json -InputObject '{}';foreach($root in $roots){{$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$($root.hive),[Microsoft.Win32.RegistryView]::Default);$k=$b.OpenSubKey($root.path);if($k){{$k.Dispose();$b.DeleteSubKeyTree($root.path,$false)}}}}",
            powershell_literal(&encoded)
        );
        run_powershell_script(&script).map(|_| ())
    }
}

#[cfg(target_os = "windows")]
fn powershell_literal(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(target_os = "windows")]
fn run_powershell_script(script: &str) -> Result<String, String> {
    use std::process::Command;

    let utf8_script = format!(
        "$utf8=[System.Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=$utf8;{script}"
    );
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &utf8_script,
        ])
        .output()
        .map_err(|error| format!("Falha ao iniciar executor de Registro: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "Operação de Registro falhou: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::RollbackStatus;
    use crate::task_registry::TaskRegistry;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    #[derive(Default)]
    struct FakeRegistry {
        values: HashMap<(String, String, String), RegistryValue>,
    }

    impl FakeRegistry {
        fn key(key: &RegistryKey) -> (String, String, String) {
            (key.hive.clone(), key.path.clone(), key.value_name.clone())
        }
    }

    impl RegistryBackend for FakeRegistry {
        fn read_values(
            &mut self,
            keys: &[RegistryKey],
        ) -> Result<Vec<Option<RegistryValue>>, String> {
            Ok(keys
                .iter()
                .map(|key| self.values.get(&Self::key(key)).cloned())
                .collect())
        }

        fn write_values(&mut self, mutations: &[RegistryMutation]) -> Result<(), String> {
            for mutation in mutations {
                self.values
                    .insert(Self::key(&mutation.key), mutation.value.clone());
            }
            Ok(())
        }

        fn delete_values(&mut self, keys: &[RegistryKey]) -> Result<(), String> {
            for key in keys {
                self.values.remove(&Self::key(key));
            }
            Ok(())
        }

        fn read_tree_values(
            &mut self,
            trees: &[RegistryTree],
        ) -> Result<Vec<RegistrySnapshot>, String> {
            let mut result = Vec::new();
            for ((hive, path, value_name), value) in &self.values {
                if trees.iter().any(|tree| {
                    tree.hive == *hive
                        && (path == &tree.path || path.starts_with(&format!("{}\\", tree.path)))
                }) {
                    result.push(RegistrySnapshot {
                        hive: hive.clone(),
                        path: path.clone(),
                        value_name: value_name.clone(),
                        existence: RegistryExistence::Present,
                        value_type: Some(value.value_type.clone()),
                        value: Some(value.value.clone()),
                    });
                }
            }
            sort_registry_values(&mut result);
            Ok(result)
        }

        fn key_trees_exist(&mut self, trees: &[RegistryTree]) -> Result<Vec<bool>, String> {
            Ok(trees
                .iter()
                .map(|tree| {
                    self.values.keys().any(|(hive, path, _)| {
                        hive == &tree.hive
                            && (path == &tree.path || path.starts_with(&format!("{}\\", tree.path)))
                    })
                })
                .collect())
        }

        fn delete_key_trees(&mut self, trees: &[RegistryTree]) -> Result<(), String> {
            self.values.retain(|(hive, path, _), _| {
                !trees.iter().any(|tree| {
                    hive == &tree.hive
                        && (path == &tree.path || path.starts_with(&format!("{}\\", tree.path)))
                })
            });
            Ok(())
        }
    }

    fn manager() -> (SafetyManager, PathBuf) {
        let path = std::env::temp_dir().join(format!("qboa-registry-test-{}.json", test_id()));
        (SafetyManager::with_journal_path(path.clone()), path)
    }

    fn registry_snapshot_path(registry: &mut FakeRegistry) -> PathBuf {
        let mutations = registry_mutations("desengordurar_telemetria").unwrap();
        let keys: Vec<_> = mutations.iter().map(|item| item.key.clone()).collect();
        let values = registry.read_values(&keys).unwrap();
        let snapshot = registry_system_snapshot("desengordurar_telemetria", &keys, values).unwrap();
        let path = std::env::temp_dir().join(format!("qboa-registry-snapshot-{}.json", test_id()));
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        path
    }

    fn cleanup_snapshot_path(registry: &mut FakeRegistry) -> PathBuf {
        let trees = registry_trees("limpeza_leve").unwrap();
        let snapshot = SystemSnapshot {
            strategy: "mru_cleanup".to_string(),
            registry: registry.read_tree_values(&trees).unwrap(),
            ..SystemSnapshot::default()
        };
        let path = std::env::temp_dir().join(format!("qboa-cleanup-snapshot-{}.json", test_id()));
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        path
    }

    fn test_id() -> String {
        format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    fn restore_change_with_fake_registry(
        manager: &mut SafetyManager,
        change: &ChangeRecord,
        registry: &mut FakeRegistry,
    ) -> Result<String, String> {
        let registry = std::cell::RefCell::new(registry);
        manager.rollback_task_with(&change.id, &|location, task_id| {
            let snapshot: SystemSnapshot =
                serde_json::from_slice(&fs::read(location).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            if task_id != "desengordurar_telemetria" {
                return Err(format!("Tarefa inesperada no snapshot: {task_id}"));
            }
            restore_registry_snapshot(&snapshot, &mut **registry.borrow_mut())?;
            Ok("Rollback de Registro aplicado e validado.".to_string())
        })
    }

    #[test]
    fn registry_task_apply_and_rollback_restore_absent_and_existing_values() {
        let catalog = TaskRegistry::try_new().unwrap();
        let (mut safety, journal) = manager();
        let mut registry = FakeRegistry::default();
        let mutations = registry_mutations("desengordurar_telemetria").unwrap();
        let existing_key = FakeRegistry::key(&mutations[0].key);
        registry.values.insert(
            existing_key.clone(),
            RegistryValue {
                value_type: "String".to_string(),
                value: serde_json::json!("previous"),
            },
        );
        let snapshot_path = registry_snapshot_path(&mut registry);

        let change = execute_registry_task(
            "desengordurar_telemetria",
            &catalog,
            "execution-a",
            snapshot_path.to_str().unwrap(),
            &mut safety,
            &mut registry,
        )
        .unwrap();
        assert_eq!(
            registry.values.get(&existing_key),
            Some(&mutations[0].value)
        );
        assert!(registry
            .values
            .contains_key(&FakeRegistry::key(&mutations[1].key)));

        restore_change_with_fake_registry(&mut safety, &change, &mut registry).unwrap();
        assert_eq!(
            registry.values.get(&existing_key),
            Some(&RegistryValue {
                value_type: "String".to_string(),
                value: serde_json::json!("previous"),
            })
        );
        assert!(!registry
            .values
            .contains_key(&FakeRegistry::key(&mutations[1].key)));
        assert_eq!(
            safety.rollback_status(&change.id),
            Some(RollbackStatus::RolledBack)
        );
        fs::remove_file(snapshot_path).unwrap();
        fs::remove_file(journal).unwrap();
    }

    #[test]
    fn rolling_back_task_b_restores_its_snapshot_not_task_a_state() {
        let catalog = TaskRegistry::try_new().unwrap();
        let (mut safety, journal) = manager();
        let mut registry = FakeRegistry::default();
        let mutation = &registry_mutations("desengordurar_telemetria").unwrap()[0];
        let key = FakeRegistry::key(&mutation.key);
        registry.values.insert(
            key.clone(),
            RegistryValue {
                value_type: "DWord".to_string(),
                value: serde_json::json!(2),
            },
        );
        let snapshot_path_a = registry_snapshot_path(&mut registry);
        let change_a = execute_registry_task(
            "desengordurar_telemetria",
            &catalog,
            "execution-a",
            snapshot_path_a.to_str().unwrap(),
            &mut safety,
            &mut registry,
        )
        .unwrap();
        assert_eq!(registry.values.get(&key), Some(&mutation.value));

        registry.values.insert(
            key.clone(),
            RegistryValue {
                value_type: "DWord".to_string(),
                value: serde_json::json!(3),
            },
        );
        let snapshot_path_b = registry_snapshot_path(&mut registry);
        let change_b = execute_registry_task(
            "desengordurar_telemetria",
            &catalog,
            "execution-b",
            snapshot_path_b.to_str().unwrap(),
            &mut safety,
            &mut registry,
        )
        .unwrap();
        restore_change_with_fake_registry(&mut safety, &change_b, &mut registry).unwrap();
        assert_eq!(
            registry.values.get(&key),
            Some(&RegistryValue {
                value_type: "DWord".to_string(),
                value: serde_json::json!(3),
            })
        );
        assert_ne!(change_a.backup_location, change_b.backup_location);
        fs::remove_file(snapshot_path_a).unwrap();
        fs::remove_file(snapshot_path_b).unwrap();
        fs::remove_file(journal).unwrap();
    }

    #[test]
    fn unknown_id_and_non_registry_executor_are_rejected() {
        let catalog = TaskRegistry::try_new().unwrap();
        let (mut safety, journal) = manager();
        let mut registry = FakeRegistry::default();

        let unknown = execute_registry_task(
            "unknown-task",
            &catalog,
            "execution-unknown",
            "unused",
            &mut safety,
            &mut registry,
        )
        .unwrap_err();
        assert!(unknown.contains("unknown-task"));

        let non_registry = execute_registry_task(
            "desengordurar_tarefas",
            &catalog,
            "execution-non-registry",
            "unused",
            &mut safety,
            &mut registry,
        )
        .unwrap_err();
        assert!(non_registry.contains("desengordurar_tarefas"));
        assert!(registry.values.is_empty());
        assert!(safety.journal().is_empty());
        let _ = fs::remove_file(journal);
    }

    #[test]
    fn cleanup_registry_trees_are_removed_via_shared_executor() {
        let catalog = TaskRegistry::try_new().unwrap();
        let (mut safety, journal) = manager();
        let mut registry = FakeRegistry::default();
        let tree = &registry_trees("limpeza_leve").unwrap()[0];
        let root_key = RegistryKey {
            hive: tree.hive.clone(),
            path: tree.path.clone(),
            value_name: "0".to_string(),
        };
        let child_key = RegistryKey {
            hive: tree.hive.clone(),
            path: format!("{}\\SubKey", tree.path),
            value_name: "Value".to_string(),
        };
        for key in [&root_key, &child_key] {
            registry.values.insert(
                FakeRegistry::key(key),
                RegistryValue {
                    value_type: "String".to_string(),
                    value: serde_json::json!("captured"),
                },
            );
        }
        let unrelated_key = RegistryKey {
            hive: "CurrentUser".to_string(),
            path: r"Software\Qboa\Unrelated".to_string(),
            value_name: "Keep".to_string(),
        };
        let unrelated_value = RegistryValue {
            value_type: "DWord".to_string(),
            value: serde_json::json!(9),
        };
        registry
            .values
            .insert(FakeRegistry::key(&unrelated_key), unrelated_value.clone());
        let snapshot_path = cleanup_snapshot_path(&mut registry);

        let change = execute_registry_task(
            "limpeza_leve",
            &catalog,
            "execution-cleanup",
            snapshot_path.to_str().unwrap(),
            &mut safety,
            &mut registry,
        )
        .unwrap();

        assert_eq!(
            registry.values.get(&FakeRegistry::key(&unrelated_key)),
            Some(&unrelated_value)
        );
        assert_eq!(registry.values.len(), 1);
        assert!(!change.reversible);
        assert_eq!(safety.journal().len(), 1);
        fs::remove_file(snapshot_path).unwrap();
        fs::remove_file(journal).unwrap();
    }

    #[test]
    fn cleanup_registry_journal_failure_restores_mru_values() {
        let catalog = TaskRegistry::try_new().unwrap();
        let journal = std::env::temp_dir().join(format!("qboa-mru-journal-dir-{}", test_id()));
        fs::create_dir(&journal).unwrap();
        let mut safety = SafetyManager::with_journal_path(journal.clone());
        let mut registry = FakeRegistry::default();
        let mutation_key = RegistryKey {
            hive: "CurrentUser".to_string(),
            path: registry_trees("limpeza_leve").unwrap()[0].path.clone(),
            value_name: "0".to_string(),
        };
        let prior = RegistryValue {
            value_type: "String".to_string(),
            value: serde_json::json!("prior"),
        };
        registry
            .values
            .insert(FakeRegistry::key(&mutation_key), prior.clone());
        let snapshot_path = cleanup_snapshot_path(&mut registry);

        let error = execute_registry_task(
            "limpeza_leve",
            &catalog,
            "execution-mru-journal-failure",
            snapshot_path.to_str().unwrap(),
            &mut safety,
            &mut registry,
        )
        .unwrap_err();

        assert!(error.contains("journal"));
        assert_eq!(
            registry.values.get(&FakeRegistry::key(&mutation_key)),
            Some(&prior)
        );
        assert!(safety.journal().is_empty());
        fs::remove_file(snapshot_path).unwrap();
        fs::remove_dir(journal).unwrap();
    }

    #[test]
    fn journal_failure_compensates_registry_changes() {
        let catalog = TaskRegistry::try_new().unwrap();
        let journal = std::env::temp_dir().join(format!("qboa-journal-dir-{}", test_id()));
        fs::create_dir(&journal).unwrap();
        let mut safety = SafetyManager::with_journal_path(journal.clone());
        let mut registry = FakeRegistry::default();
        let mutation = &registry_mutations("desengordurar_telemetria").unwrap()[0];
        let key = FakeRegistry::key(&mutation.key);
        registry.values.insert(
            key.clone(),
            RegistryValue {
                value_type: "DWord".to_string(),
                value: serde_json::json!(7),
            },
        );
        let snapshot_path = registry_snapshot_path(&mut registry);

        let error = execute_registry_task(
            "desengordurar_telemetria",
            &catalog,
            "execution-journal-failure",
            snapshot_path.to_str().unwrap(),
            &mut safety,
            &mut registry,
        )
        .unwrap_err();

        assert!(error.contains("journal"));
        assert_eq!(
            registry.values.get(&key),
            Some(&RegistryValue {
                value_type: "DWord".to_string(),
                value: serde_json::json!(7),
            })
        );
        assert!(safety.journal().is_empty());
        fs::remove_file(snapshot_path).unwrap();
        fs::remove_dir(journal).unwrap();
    }
}
