use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskModule {
    Cleanup,
    Debloat,
    Diagnostic,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskCategory {
    Cleanup,
    Debloat,
    Diagnostic,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskRisk {
    Safe,
    Moderate,
    Advanced,
}

impl TaskRisk {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Moderate => "moderate",
            Self::Advanced => "advanced",
        }
    }

    pub fn requires_restore_point(&self) -> bool {
        !matches!(self, Self::Safe)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutorType {
    Command,
    PowerShell,
    Sequence,
    Registry,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ImpactArea {
    Space,
    Performance,
    Privacy,
    Appearance,
    Energy,
    Stability,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpectedImpact {
    pub area: ImpactArea,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRequirements {
    pub admin: bool,
    pub windows_versions: Vec<String>,
    pub hardware: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RebootRequirement {
    No,
    Recommended,
    Required,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub description: String,
    pub module: TaskModule,
    pub category: TaskCategory,
    // Kept as the existing runner description; dispatch remains in main.rs.
    pub risk: TaskRisk,
    pub requires_admin: bool,
    pub estimated_duration: u32,
    pub reversible: bool,
    pub creates_restore_point: bool,
    pub rollback_strategy: String,
    pub rollback_on_failure: bool,
    pub dependencies: Vec<String>,
    pub executor: ExecutorType,
    pub impact: Vec<ExpectedImpact>,
    pub requirements: TaskRequirements,
    pub verification: String,
    pub reboot: RebootRequirement,
    pub profiles: Vec<String>,
    pub presets: Vec<String>,
}

#[derive(Clone, Debug)]
struct LegacyTask {
    id: String,
    name: String,
    description: String,
    category: TaskCategory,
    risk: TaskRisk,
    requires_admin: bool,
    estimated_duration: u32,
    reversible: bool,
    creates_restore_point: bool,
    rollback_strategy: String,
    rollback_on_failure: bool,
    dependencies: Vec<String>,
    executor: ExecutorType,
    presets: Vec<String>,
}

impl From<LegacyTask> for Task {
    fn from(legacy: LegacyTask) -> Self {
        let module = match legacy.category {
            TaskCategory::Cleanup => TaskModule::Cleanup,
            TaskCategory::Debloat => TaskModule::Debloat,
            TaskCategory::Diagnostic => TaskModule::Diagnostic,
        };
        let impact = task_impact(&legacy.id);
        let verification = task_verification(&legacy.id).to_string();
        let reboot = task_reboot(&legacy.id);
        let profiles = legacy.presets.clone();

        Self {
            id: legacy.id,
            name: legacy.name,
            description: legacy.description,
            module,
            category: legacy.category,
            risk: legacy.risk,
            requires_admin: legacy.requires_admin,
            estimated_duration: legacy.estimated_duration,
            reversible: legacy.reversible,
            creates_restore_point: legacy.creates_restore_point,
            rollback_strategy: legacy.rollback_strategy,
            rollback_on_failure: legacy.rollback_on_failure,
            dependencies: legacy.dependencies,
            executor: legacy.executor,
            impact,
            requirements: TaskRequirements {
                admin: legacy.requires_admin,
                windows_versions: vec!["Windows 10".to_string(), "Windows 11".to_string()],
                hardware: Vec::new(),
            },
            verification,
            reboot,
            profiles,
            presets: legacy.presets,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TaskRegistry {
    tasks: HashMap<String, Task>,
    presets: HashMap<String, Vec<String>>,
}

impl TaskRegistry {
    pub fn try_new() -> Result<Self, String> {
        let tasks = vec![
            LegacyTask {
                id: "limpeza_leve".to_string(),
                name: "Limpeza leve".to_string(),
                description: "Remove temporários, caches descartáveis e lixeira.".to_string(),
                category: TaskCategory::Cleanup,
                risk: TaskRisk::Safe,
                requires_admin: false,
                estimated_duration: 6,
                reversible: false,
                creates_restore_point: false,
                rollback_strategy: "None".to_string(),
                rollback_on_failure: false,
                dependencies: vec![],
                executor: ExecutorType::Registry,
                presets: vec![
                    "express".to_string(),
                    "normal".to_string(),
                    "turbo".to_string(),
                ],
            },
            LegacyTask {
                id: "limpeza_media".to_string(),
                name: "Limpeza média".to_string(),
                description: "Executa a limpeza padrão e o Disk Cleanup do Windows.".to_string(),
                category: TaskCategory::Cleanup,
                risk: TaskRisk::Moderate,
                requires_admin: false,
                estimated_duration: 12,
                reversible: false,
                creates_restore_point: false,
                rollback_strategy: "None".to_string(),
                rollback_on_failure: false,
                dependencies: vec!["limpeza_leve".to_string()],
                executor: ExecutorType::Registry,
                presets: vec!["normal".to_string(), "turbo".to_string()],
            },
            LegacyTask {
                id: "limpeza_pesada".to_string(),
                name: "Limpeza pesada".to_string(),
                description: "Executa limpeza profunda com remoção extra de arquivos temporários."
                    .to_string(),
                category: TaskCategory::Cleanup,
                risk: TaskRisk::Advanced,
                requires_admin: true,
                estimated_duration: 18,
                reversible: false,
                creates_restore_point: true,
                rollback_strategy: "RestorePoint".to_string(),
                rollback_on_failure: true,
                dependencies: vec!["limpeza_media".to_string()],
                executor: ExecutorType::Registry,
                presets: vec!["turbo".to_string()],
            },
            LegacyTask {
                id: "desengordurar_telemetria".to_string(),
                name: "Desativar telemetria".to_string(),
                description: "Reduz coleta de diagnósticos e políticas de privacidade do Windows."
                    .to_string(),
                category: TaskCategory::Debloat,
                risk: TaskRisk::Moderate,
                requires_admin: true,
                estimated_duration: 7,
                reversible: true,
                creates_restore_point: true,
                rollback_strategy: "RestoreRegistry".to_string(),
                rollback_on_failure: true,
                dependencies: vec![],
                executor: ExecutorType::Registry,
                presets: vec!["normal".to_string(), "turbo".to_string()],
            },
            LegacyTask {
                id: "desengordurar_tarefas".to_string(),
                name: "Desabilitar tarefas de diagnóstico".to_string(),
                description: "Desativa tarefas agendadas de coleta e compatibilidade.".to_string(),
                category: TaskCategory::Debloat,
                risk: TaskRisk::Moderate,
                requires_admin: true,
                estimated_duration: 6,
                reversible: true,
                creates_restore_point: true,
                rollback_strategy: "RestoreScheduledTask".to_string(),
                rollback_on_failure: true,
                dependencies: vec![],
                executor: ExecutorType::PowerShell,
                presets: vec!["normal".to_string(), "turbo".to_string()],
            },
            LegacyTask {
                id: "diagnostico_dism_scan".to_string(),
                name: "DISM Scan".to_string(),
                description: "Verifica a integridade da imagem do sistema sem reparar.".to_string(),
                category: TaskCategory::Diagnostic,
                risk: TaskRisk::Safe,
                requires_admin: true,
                estimated_duration: 15,
                reversible: false,
                creates_restore_point: false,
                rollback_strategy: "None".to_string(),
                rollback_on_failure: false,
                dependencies: vec![],
                executor: ExecutorType::Command,
                presets: vec!["normal".to_string(), "turbo".to_string()],
            },
            LegacyTask {
                id: "diagnostico_dism_restore".to_string(),
                name: "DISM Restore".to_string(),
                description: "Tenta reparar componentes do Windows usando a imagem do sistema."
                    .to_string(),
                category: TaskCategory::Diagnostic,
                risk: TaskRisk::Advanced,
                requires_admin: true,
                estimated_duration: 30,
                reversible: false,
                creates_restore_point: true,
                rollback_strategy: "RestorePoint".to_string(),
                rollback_on_failure: true,
                dependencies: vec!["diagnostico_dism_scan".to_string()],
                executor: ExecutorType::Command,
                presets: vec!["turbo".to_string()],
            },
            LegacyTask {
                id: "diagnostico_sfc".to_string(),
                name: "SFC /scannow".to_string(),
                description: "Verifica a integridade dos arquivos do sistema.".to_string(),
                category: TaskCategory::Diagnostic,
                risk: TaskRisk::Moderate,
                requires_admin: true,
                estimated_duration: 20,
                reversible: false,
                creates_restore_point: true,
                rollback_strategy: "RestorePoint".to_string(),
                rollback_on_failure: true,
                dependencies: vec![],
                executor: ExecutorType::Command,
                presets: vec!["normal".to_string(), "turbo".to_string()],
            },
            LegacyTask {
                id: "diagnostico_informacoes".to_string(),
                name: "Informações do sistema".to_string(),
                description: "Resumem ambiente, hardware e sistema operacional.".to_string(),
                category: TaskCategory::Diagnostic,
                risk: TaskRisk::Safe,
                requires_admin: false,
                estimated_duration: 5,
                reversible: false,
                creates_restore_point: false,
                rollback_strategy: "None".to_string(),
                rollback_on_failure: false,
                dependencies: vec![],
                executor: ExecutorType::Command,
                presets: vec![
                    "express".to_string(),
                    "normal".to_string(),
                    "turbo".to_string(),
                ],
            },
            LegacyTask {
                id: "desengordurar_telemetria_avancada".to_string(),
                name: "Privacidade avançada".to_string(),
                description: "Desativa serviços, políticas e recursos do Windows AI/Recall."
                    .to_string(),
                category: TaskCategory::Debloat,
                risk: TaskRisk::Advanced,
                requires_admin: true,
                estimated_duration: 10,
                reversible: true,
                creates_restore_point: true,
                rollback_strategy: "RestoreRegistry".to_string(),
                rollback_on_failure: true,
                dependencies: vec!["desengordurar_telemetria".to_string()],
                executor: ExecutorType::PowerShell,
                presets: vec!["turbo".to_string()],
            },
            LegacyTask {
                id: "limpeza_windows_update".to_string(),
                name: "Windows Update cache".to_string(),
                description: "Remove arquivos temporários de atualização do Windows com cuidado."
                    .to_string(),
                category: TaskCategory::Cleanup,
                risk: TaskRisk::Moderate,
                requires_admin: true,
                estimated_duration: 9,
                reversible: false,
                creates_restore_point: false,
                rollback_strategy: "None".to_string(),
                rollback_on_failure: false,
                dependencies: vec![],
                executor: ExecutorType::PowerShell,
                presets: vec!["normal".to_string(), "turbo".to_string()],
            },
        ]
        .into_iter()
        .map(Task::from)
        .collect();

        Self::from_tasks(tasks, preset_definitions())
    }

    pub fn from_tasks(
        tasks: Vec<Task>,
        presets: HashMap<String, Vec<String>>,
    ) -> Result<Self, String> {
        validate_tasks(&tasks)?;
        validate_presets(&tasks, &presets)?;

        let mut registry = Self {
            tasks: HashMap::new(),
            presets,
        };
        for task in tasks {
            registry.tasks.insert(task.id.clone(), task);
        }
        Ok(registry)
    }

    pub fn resolve(&self, task_id: &str) -> Option<&Task> {
        self.tasks.get(task_id)
    }

    pub fn list(&self) -> Vec<Task> {
        let mut items: Vec<_> = self.tasks.values().cloned().collect();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        items
    }

    pub fn preset_tasks(&self, preset: &str) -> Vec<&Task> {
        let ids = self.presets.get(preset).cloned().unwrap_or_default();
        ids.iter().filter_map(|id| self.tasks.get(id)).collect()
    }

    pub fn from_catalog_json(input: &str) -> Result<Self, String> {
        let root: serde_json::Value =
            serde_json::from_str(input).map_err(|error| format!("Catálogo inválido: {error}"))?;
        let entries = root
            .get("tasks")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "Catálogo inválido: campo tasks ausente ou inválido.".to_string())?;
        let presets: HashMap<String, Vec<String>> =
            serde_json::from_value(root.get("presets").cloned().unwrap_or_default())
                .map_err(|error| format!("Catálogo inválido: presets: {error}"))?;

        let tasks = entries
            .iter()
            .map(|entry| {
                let id = entry
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("<id ausente>");
                serde_json::from_value::<Task>(entry.clone())
                    .map_err(|error| format!("Tarefa {id}: catálogo inválido: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_tasks(tasks, presets)
    }
}

fn preset_definitions() -> HashMap<String, Vec<String>> {
    HashMap::from([
        (
            "express".to_string(),
            vec![
                "limpeza_leve".to_string(),
                "diagnostico_informacoes".to_string(),
            ],
        ),
        (
            "normal".to_string(),
            vec![
                "desengordurar_telemetria".to_string(),
                "limpeza_media".to_string(),
                "diagnostico_informacoes".to_string(),
            ],
        ),
        (
            "turbo".to_string(),
            vec![
                "desengordurar_telemetria".to_string(),
                "limpeza_pesada".to_string(),
            ],
        ),
    ])
}

fn validate_tasks(tasks: &[Task]) -> Result<(), String> {
    let mut ids = HashSet::new();
    for task in tasks {
        if task.id.trim().is_empty() {
            return Err("Tarefa <id vazio>: id obrigatório.".to_string());
        }
        if !ids.insert(task.id.as_str()) {
            return Err(format!("Tarefa {}: id duplicado.", task.id));
        }
        if task.name.trim().is_empty() || task.description.trim().is_empty() {
            return Err(format!(
                "Tarefa {}: nome e descrição são obrigatórios.",
                task.id
            ));
        }
        if task.requires_admin != task.requirements.admin {
            return Err(format!(
                "Tarefa {}: requirements.admin diverge de requires_admin.",
                task.id
            ));
        }
        let expected_module = match task.category {
            TaskCategory::Cleanup => TaskModule::Cleanup,
            TaskCategory::Debloat => TaskModule::Debloat,
            TaskCategory::Diagnostic => TaskModule::Diagnostic,
        };
        if task.module != expected_module {
            return Err(format!(
                "Tarefa {}: módulo incompatível com a categoria.",
                task.id
            ));
        }
        if task.impact.is_empty() {
            return Err(format!("Tarefa {}: impacto esperado obrigatório.", task.id));
        }
        if matches!(task.executor, ExecutorType::Registry)
            && !matches!(
                task.id.as_str(),
                "desengordurar_telemetria" | "limpeza_leve" | "limpeza_media" | "limpeza_pesada"
            )
        {
            return Err(format!(
                "Tarefa {}: executor de Registro não permitido para este ID.",
                task.id
            ));
        }
        if task.profiles != task.presets {
            return Err(format!(
                "Tarefa {}: profiles deve preservar os presets existentes.",
                task.id
            ));
        }
        if task.reversible
            && !matches!(
                task.rollback_strategy.as_str(),
                "RestoreRegistry" | "RestoreScheduledTask"
            )
        {
            return Err(format!(
                "Tarefa {}: reversível sem estratégia de snapshot granular usada atualmente.",
                task.id
            ));
        }
        if task.reversible && !task.rollback_on_failure {
            return Err(format!(
                "Tarefa {}: operação reversível sem estratégia de falha/rollback.",
                task.id
            ));
        }
        if task.verification.trim().is_empty() {
            return Err(format!(
                "Tarefa {}: método de verificação obrigatório.",
                task.id
            ));
        }
    }

    for task in tasks {
        for dependency in &task.dependencies {
            if !ids.contains(dependency.as_str()) {
                return Err(format!(
                    "Tarefa {}: dependência inexistente {dependency}.",
                    task.id
                ));
            }
        }
    }

    let by_id: HashMap<&str, &Task> = tasks.iter().map(|task| (task.id.as_str(), task)).collect();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for task in tasks {
        visit_dependencies(task, &by_id, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit_dependencies<'a>(
    task: &'a Task,
    by_id: &HashMap<&str, &'a Task>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> Result<(), String> {
    if visited.contains(task.id.as_str()) {
        return Ok(());
    }
    if !visiting.insert(task.id.as_str()) {
        return Err(format!("Tarefa {}: dependência cíclica.", task.id));
    }
    for dependency in &task.dependencies {
        if let Some(dependency_task) = by_id.get(dependency.as_str()) {
            visit_dependencies(dependency_task, by_id, visiting, visited)?;
        }
    }
    visiting.remove(task.id.as_str());
    visited.insert(task.id.as_str());
    Ok(())
}

fn validate_presets(tasks: &[Task], presets: &HashMap<String, Vec<String>>) -> Result<(), String> {
    let ids: HashSet<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
    for (preset, task_ids) in presets {
        for task_id in task_ids {
            if !ids.contains(task_id.as_str()) {
                return Err(format!(
                    "Preset {preset}: tarefa referenciada inexistente {task_id}."
                ));
            }
        }
    }
    Ok(())
}

fn task_impact(id: &str) -> Vec<ExpectedImpact> {
    let (area, description) = match id {
        "limpeza_leve" | "limpeza_media" | "limpeza_pesada" | "limpeza_windows_update" => (
            ImpactArea::Space,
            "Pode liberar espaço removendo arquivos temporários definidos pela tarefa.",
        ),
        "desengordurar_telemetria"
        | "desengordurar_tarefas"
        | "desengordurar_telemetria_avancada" => (
            ImpactArea::Privacy,
            "Reduz coleta/diagnóstico conforme as configurações manipuladas pela tarefa.",
        ),
        "diagnostico_dism_restore" | "diagnostico_sfc" => (
            ImpactArea::Stability,
            "Pode reparar componentes ou arquivos do sistema conforme o resultado da ferramenta.",
        ),
        _ => (
            ImpactArea::Stability,
            "Leitura/diagnóstico sem alteração intencional das configurações do sistema.",
        ),
    };
    vec![ExpectedImpact {
        area,
        description: description.to_string(),
    }]
}

fn task_verification(id: &str) -> &'static str {
    match id {
        "diagnostico_dism_scan" | "diagnostico_dism_restore" | "diagnostico_sfc" => {
            "O backend verifica o código de saída e apresenta a saída do utilitário; não consulta o estado final do sistema."
        }
        "diagnostico_informacoes" => {
            "O backend verifica o código de saída de systeminfo e apresenta a saída."
        }
        _ => {
            "O backend verifica o código de saída do executor e apresenta sua saída; não verifica individualmente cada alteração."
        }
    }
}

fn task_reboot(id: &str) -> RebootRequirement {
    match id {
        "diagnostico_dism_restore" | "diagnostico_sfc" => RebootRequirement::Recommended,
        _ => RebootRequirement::No,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_known_tasks() {
        let registry = TaskRegistry::try_new().unwrap();
        assert!(registry.resolve("limpeza_leve").is_some());
        assert!(registry.resolve("desengordurar_telemetria").is_some());
        assert!(registry.resolve("diagnostico_sfc").is_some());
    }

    #[test]
    fn unknown_tasks_are_missing() {
        let registry = TaskRegistry::try_new().unwrap();
        assert!(registry.resolve("task_inexistente").is_none());
    }

    #[test]
    fn presets_are_built() {
        let registry = TaskRegistry::try_new().unwrap();
        assert!(!registry.preset_tasks("express").is_empty());
        assert!(!registry.preset_tasks("normal").is_empty());
        assert!(!registry.preset_tasks("turbo").is_empty());
    }

    #[test]
    fn presets_keep_operational_order() {
        let registry = TaskRegistry::try_new().unwrap();
        let normal: Vec<_> = registry
            .preset_tasks("normal")
            .iter()
            .map(|task| task.id.as_str())
            .collect();
        let turbo: Vec<_> = registry
            .preset_tasks("turbo")
            .iter()
            .map(|task| task.id.as_str())
            .collect();

        assert_eq!(
            normal,
            [
                "desengordurar_telemetria",
                "limpeza_media",
                "diagnostico_informacoes"
            ]
        );
        assert_eq!(turbo, ["desengordurar_telemetria", "limpeza_pesada"]);
    }

    #[test]
    fn current_catalog_loads_with_complete_metadata() {
        let registry = TaskRegistry::try_new().unwrap();
        let tasks = registry.list();
        assert_eq!(tasks.len(), 11);
        for task in tasks {
            assert!(!task.impact.is_empty());
            assert!(!task.verification.is_empty());
            assert_eq!(task.profiles, task.presets);
            assert_eq!(task.requirements.admin, task.requires_admin);
        }
    }

    #[test]
    fn current_tasks_resolve_to_declared_executors() {
        let registry = TaskRegistry::try_new().unwrap();
        let expected = [
            ("limpeza_leve", ExecutorType::Registry),
            ("limpeza_media", ExecutorType::Registry),
            ("limpeza_pesada", ExecutorType::Registry),
            ("desengordurar_telemetria", ExecutorType::Registry),
            ("desengordurar_tarefas", ExecutorType::PowerShell),
            ("diagnostico_dism_scan", ExecutorType::Command),
            ("diagnostico_dism_restore", ExecutorType::Command),
            ("diagnostico_sfc", ExecutorType::Command),
            ("diagnostico_informacoes", ExecutorType::Command),
            (
                "desengordurar_telemetria_avancada",
                ExecutorType::PowerShell,
            ),
            ("limpeza_windows_update", ExecutorType::PowerShell),
        ];

        for (id, executor) in expected {
            assert_eq!(registry.resolve(id).unwrap().executor, executor, "{id}");
        }
    }

    #[test]
    fn invalid_executor_is_rejected_with_task_id() {
        let registry = TaskRegistry::try_new().unwrap();
        let task = registry.resolve("limpeza_leve").unwrap();
        let mut task_json = serde_json::to_value(task).unwrap();
        task_json["executor"] = serde_json::json!("NotARealExecutor");
        let catalog = serde_json::json!({
            "tasks": [task_json],
            "presets": {}
        });

        let error = TaskRegistry::from_catalog_json(&catalog.to_string()).unwrap_err();
        assert!(error.contains("limpeza_leve"), "{error}");
        assert!(
            error.contains("executor") || error.contains("variant"),
            "{error}"
        );
    }

    #[test]
    fn missing_and_cyclic_dependencies_are_rejected_with_task_id() {
        let registry = TaskRegistry::try_new().unwrap();
        let mut tasks = registry.list();
        let task = tasks
            .iter_mut()
            .find(|task| task.id == "limpeza_leve")
            .unwrap();
        task.dependencies = vec!["nao_existe".to_string()];
        let error = TaskRegistry::from_tasks(tasks.clone(), HashMap::new()).unwrap_err();
        assert!(error.contains("limpeza_leve"), "{error}");
        assert!(error.contains("nao_existe"), "{error}");

        let mut tasks = registry.list();
        tasks
            .iter_mut()
            .find(|task| task.id == "limpeza_leve")
            .unwrap()
            .dependencies = vec!["limpeza_media".to_string()];
        let error = TaskRegistry::from_tasks(tasks, HashMap::new()).unwrap_err();
        assert!(error.contains("limpeza_"), "{error}");
        assert!(error.contains("cíclica"), "{error}");
    }
}
