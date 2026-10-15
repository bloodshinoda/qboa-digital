use crate::task_registry::{TaskModule, TaskRegistry, TweakProfile};
use std::collections::HashSet;

const DEBLOAT_TASK_ORDER: &[&str] = &[
    "desengordurar_telemetria",
    "desengordurar_tarefas",
    "desengordurar_telemetria_avancada",
];

pub(crate) fn resolve_task_ids(
    registry: &TaskRegistry,
    profile: TweakProfile,
    selected_ids: &[String],
) -> Result<Vec<String>, String> {
    if profile == TweakProfile::Personalizado {
        if selected_ids.is_empty() {
            return Err("Marque ao menos uma tarefa para o perfil Personalizado.".to_string());
        }
        let mut seen = HashSet::new();
        for id in selected_ids {
            if !seen.insert(id.as_str()) {
                return Err(format!("Tarefa duplicada no perfil Personalizado: {id}."));
            }
            let task = registry
                .resolve(id)
                .ok_or_else(|| format!("Tarefa desconhecida no perfil Personalizado: {id}."))?;
            if task.module != TaskModule::Debloat
                || !task.tweak_profiles.contains(&TweakProfile::Personalizado)
            {
                return Err(format!("Tarefa fora do módulo Desengordurar: {id}."));
            }
        }
        return Ok(selected_ids.to_vec());
    }

    if !selected_ids.is_empty() {
        return Err("Seleção manual só é válida no perfil Personalizado.".to_string());
    }
    let ids: Vec<String> = DEBLOAT_TASK_ORDER
        .iter()
        .filter_map(|id| {
            let task = registry.resolve(id)?;
            task.tweak_profiles
                .contains(&profile)
                .then(|| (*id).to_string())
        })
        .collect();
    if ids.is_empty() {
        return Err("Perfil sem tarefas disponíveis.".to_string());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_registry::ExecutorType;

    #[test]
    fn debloat_tasks_keep_their_existing_executors() {
        let registry = TaskRegistry::try_new().unwrap();
        let expected = [
            ("desengordurar_telemetria", ExecutorType::Registry),
            ("desengordurar_tarefas", ExecutorType::PowerShell),
            (
                "desengordurar_telemetria_avancada",
                ExecutorType::PowerShell,
            ),
        ];
        for (id, executor) in expected {
            assert_eq!(registry.resolve(id).unwrap().executor, executor, "{id}");
        }
    }

    #[test]
    fn custom_profile_returns_only_the_selected_task() {
        let registry = TaskRegistry::try_new().unwrap();
        let selected = vec!["desengordurar_tarefas".to_string()];
        assert_eq!(
            resolve_task_ids(&registry, TweakProfile::Personalizado, &selected).unwrap(),
            selected
        );
    }

    #[test]
    fn fixed_profiles_resolve_only_existing_debloat_tasks() {
        let registry = TaskRegistry::try_new().unwrap();
        assert_eq!(
            resolve_task_ids(&registry, TweakProfile::Conservador, &[]).unwrap(),
            ["desengordurar_telemetria"]
        );
        assert_eq!(
            resolve_task_ids(&registry, TweakProfile::Equilibrado, &[]).unwrap(),
            ["desengordurar_telemetria", "desengordurar_tarefas"]
        );
        assert_eq!(
            resolve_task_ids(&registry, TweakProfile::Agressivo, &[]).unwrap(),
            [
                "desengordurar_telemetria",
                "desengordurar_tarefas",
                "desengordurar_telemetria_avancada"
            ]
        );
        for id in resolve_task_ids(&registry, TweakProfile::Agressivo, &[]).unwrap() {
            let task = registry.resolve(&id).unwrap();
            assert!(
                task.reversible || task.risk.requires_restore_point(),
                "Agressivo inclui tarefa irreversível sem confirmação: {id}"
            );
        }
    }
}
