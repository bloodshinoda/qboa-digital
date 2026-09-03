use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: TaskCategory,
    pub risk: TaskRisk,
    pub requires_admin: bool,
    pub estimated_duration: u32,
    pub reversible: bool,
    pub creates_restore_point: bool,
    pub rollback_strategy: String,
    pub rollback_on_failure: bool,
    pub dependencies: Vec<String>,
    pub executor: ExecutorType,
    pub presets: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct TaskRegistry {
    tasks: HashMap<String, Task>,
    presets: HashMap<String, Vec<String>>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        let mut registry = Self::default();

        let tasks = vec![
            Task {
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
                executor: ExecutorType::PowerShell,
                presets: vec!["express".to_string(), "normal".to_string(), "turbo".to_string()],
            },
            Task {
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
                executor: ExecutorType::Sequence,
                presets: vec!["normal".to_string(), "turbo".to_string()],
            },
            Task {
                id: "limpeza_pesada".to_string(),
                name: "Limpeza pesada".to_string(),
                description: "Executa limpeza profunda com remoção extra de arquivos temporários.".to_string(),
                category: TaskCategory::Cleanup,
                risk: TaskRisk::Advanced,
                requires_admin: true,
                estimated_duration: 18,
                reversible: false,
                creates_restore_point: true,
                rollback_strategy: "RestorePoint".to_string(),
                rollback_on_failure: true,
                dependencies: vec!["limpeza_media".to_string()],
                executor: ExecutorType::Sequence,
                presets: vec!["turbo".to_string()],
            },
            Task {
                id: "desengordurar_telemetria".to_string(),
                name: "Desativar telemetria".to_string(),
                description: "Reduz coleta de diagnósticos e políticas de privacidade do Windows.".to_string(),
                category: TaskCategory::Debloat,
                risk: TaskRisk::Moderate,
                requires_admin: true,
                estimated_duration: 7,
                reversible: true,
                creates_restore_point: true,
                rollback_strategy: "RestoreRegistry".to_string(),
                rollback_on_failure: true,
                dependencies: vec![],
                executor: ExecutorType::PowerShell,
                presets: vec!["normal".to_string(), "turbo".to_string()],
            },
            Task {
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
            Task {
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
            Task {
                id: "diagnostico_dism_restore".to_string(),
                name: "DISM Restore".to_string(),
                description: "Tenta reparar componentes do Windows usando a imagem do sistema.".to_string(),
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
            Task {
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
            Task {
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
                presets: vec!["express".to_string(), "normal".to_string(), "turbo".to_string()],
            },
            Task {
                id: "desengordurar_telemetria_avancada".to_string(),
                name: "Privacidade avançada".to_string(),
                description: "Desativa serviços, políticas e recursos do Windows AI/Recall.".to_string(),
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
            Task {
                id: "limpeza_windows_update".to_string(),
                name: "Windows Update cache".to_string(),
                description: "Remove arquivos temporários de atualização do Windows com cuidado.".to_string(),
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
        ];

        for task in tasks {
            registry.tasks.insert(task.id.clone(), task.clone());
            for preset in &task.presets {
                registry.presets.entry(preset.clone()).or_default().push(task.id.clone());
            }
        }

        registry
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
        let mut tasks: Vec<_> = ids.iter().filter_map(|id| self.tasks.get(id)).collect();
        tasks.sort_by(|a, b| a.name.cmp(&b.name));
        tasks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_known_tasks() {
        let registry = TaskRegistry::new();
        assert!(registry.resolve("limpeza_leve").is_some());
        assert!(registry.resolve("desengordurar_telemetria").is_some());
        assert!(registry.resolve("diagnostico_sfc").is_some());
    }

    #[test]
    fn unknown_tasks_are_missing() {
        let registry = TaskRegistry::new();
        assert!(registry.resolve("task_inexistente").is_none());
    }

    #[test]
    fn presets_are_built() {
        let registry = TaskRegistry::new();
        assert!(!registry.preset_tasks("express").is_empty());
        assert!(!registry.preset_tasks("normal").is_empty());
        assert!(!registry.preset_tasks("turbo").is_empty());
    }
}
