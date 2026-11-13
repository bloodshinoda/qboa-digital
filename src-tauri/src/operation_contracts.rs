//! Contratos compartilhados exclusivamente pelas operações de limpeza e manutenção.
//!
//! O diagnóstico mantém seus próprios modelos e ciclos de vida. A infraestrutura poderá
//! ser compartilhada futuramente sem acoplar os domínios neste momento.

use serde::Serialize;

/// Identidade estável de uma execução: task_id identifica a ação; execution_id, sua ocorrência.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CleanupExecutionRef {
    pub task_id: String,
    pub execution_id: String,
}

/// Estados possíveis do ciclo de vida de uma operação de limpeza/manutenção.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CleanupExecutionState {
    Pending,
    Running,
    AwaitingConfirmation,
    Cancelling,
    Completed,
    PartiallyCompleted,
    Cancelled,
    Failed,
}

/// Uma execução com falhas registradas nunca pode ser classificada como sucesso integral.
pub(crate) fn cleanup_result_state(failures: u64) -> CleanupExecutionState {
    if failures == 0 {
        CleanupExecutionState::Completed
    } else {
        CleanupExecutionState::PartiallyCompleted
    }
}

impl CleanupExecutionState {
    pub(crate) fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::PartiallyCompleted | Self::Cancelled | Self::Failed
        )
    }

    /// Valida transições permitidas sem alterar o estado atual.
    pub(crate) fn can_transition_to(self, next: Self) -> bool {
        use CleanupExecutionState::*;

        matches!(
            (self, next),
            (Pending, Running | Cancelled | Failed)
                | (
                    Running,
                    AwaitingConfirmation
                        | Cancelling
                        | Completed
                        | PartiallyCompleted
                        | Cancelled
                        | Failed
                )
                | (
                    AwaitingConfirmation,
                    Running | Cancelling | Cancelled | PartiallyCompleted | Failed
                )
                | (Cancelling, Cancelled | Failed | PartiallyCompleted)
        )
    }
}

/// Progresso estruturado. current/total representam etapas, não percentual.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CleanupProgress {
    pub current: Option<u32>,
    pub total: Option<u32>,
    pub percent: Option<u8>,
    pub stage: String,
    pub message: String,
}

impl CleanupProgress {
    pub(crate) fn is_valid(&self) -> bool {
        let steps_valid = match (self.current, self.total) {
            (Some(current), Some(total)) => total > 0 && current <= total,
            (None, None) => true,
            _ => false,
        };

        steps_valid && self.percent.is_none_or(|percent| percent <= 100)
    }
}

/// Eventos públicos com nomes compatíveis com os consumidores atuais do qboa-event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CleanupEventKind {
    TaskStarted,
    TaskProgress,
    TaskOutput,
    TaskCompleted,
    TaskCancelled,
    TaskError,
    RestorePointConfirmationRequired,
    ResetBaseConfirmationRequired,
}

/// Tipo de confirmação solicitada durante uma operação.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CleanupConfirmationKind {
    RestorePointUnavailable,
    IrreversibleResetBase,
}

/// Solicitação vinculada à execução que aguarda uma decisão explícita.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CleanupConfirmationRequest {
    pub execution: CleanupExecutionRef,
    pub kind: CleanupConfirmationKind,
    pub message: String,
}

/// Envelope serializado do evento legado. Os campos e seus nomes JSON são mantidos
/// para que a interface atual continue funcionando durante a migração.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CleanupEventEnvelope {
    pub event: String,
    pub task_id: String,
    pub execution_id: String,
    pub message: String,
    pub ok: bool,
    pub progress: Option<String>,
    pub risk: String,
}

/// Resultado final. A aceitação do comando de início não equivale à conclusão.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CleanupOperationResult {
    pub execution: CleanupExecutionRef,
    pub state: CleanupExecutionState,
    pub message: String,
    pub completed_steps: Option<u32>,
    pub total_steps: Option<u32>,
    pub change_id: Option<String>,
}

impl CleanupOperationResult {
    pub(crate) fn is_consistent(&self) -> bool {
        self.state.is_terminal()
            && match (self.completed_steps, self.total_steps) {
                (Some(completed), Some(total)) => total > 0 && completed <= total,
                (None, None) => true,
                _ => false,
            }
    }
}

/// Ciclo de vida aplicado pelo executor real de limpeza/manutenção.
#[derive(Clone, Debug)]
pub(crate) struct CleanupExecutionLifecycle {
    execution: CleanupExecutionRef,
    state: CleanupExecutionState,
}

impl CleanupExecutionLifecycle {
    pub(crate) fn new(task_id: &str, execution_id: &str) -> Self {
        Self {
            execution: CleanupExecutionRef {
                task_id: task_id.to_string(),
                execution_id: execution_id.to_string(),
            },
            state: CleanupExecutionState::Pending,
        }
    }

    pub(crate) fn transition(&mut self, next: CleanupExecutionState) -> Result<(), String> {
        if !self.state.can_transition_to(next) {
            return Err(format!(
                "Transição de execução inválida: {:?} -> {:?}",
                self.state, next
            ));
        }
        self.state = next;
        Ok(())
    }

    pub(crate) fn record_progress(&mut self, progress: CleanupProgress) -> Result<(), String> {
        if self.state != CleanupExecutionState::Running {
            return Err("O progresso só pode ser atualizado durante a execução.".into());
        }
        if !progress.is_valid() {
            return Err(
                "O progresso informado não respeita o contrato de etapas/percentual.".into(),
            );
        }
        Ok(())
    }

    pub(crate) fn finish(
        &mut self,
        state: CleanupExecutionState,
        message: impl Into<String>,
        change_id: Option<String>,
    ) -> Result<CleanupOperationResult, String> {
        if !state.is_terminal() {
            return Err("O resultado exige um estado terminal.".into());
        }
        self.transition(state)?;
        let result = CleanupOperationResult {
            execution: self.execution.clone(),
            state: self.state,
            message: message.into(),
            completed_steps: None,
            total_steps: None,
            change_id,
        };
        if !result.is_consistent() {
            return Err("O resultado final não respeita o contrato de execução.".into());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciclo_real_exige_inicio_antes_de_progredir_e_finalizar() {
        let mut lifecycle = CleanupExecutionLifecycle::new("limpeza_leve", "exec-1");
        assert!(lifecycle
            .record_progress(CleanupProgress {
                current: None,
                total: None,
                percent: Some(10),
                stage: "Preparação".into(),
                message: "Preparando".into(),
            })
            .is_err());
        lifecycle
            .transition(CleanupExecutionState::Running)
            .unwrap();
        assert!(lifecycle
            .record_progress(CleanupProgress {
                current: Some(1),
                total: Some(2),
                percent: Some(25),
                stage: "Temporários".into(),
                message: "Limpando".into(),
            })
            .is_ok());
        let result = lifecycle
            .finish(CleanupExecutionState::Completed, "Concluída", None)
            .unwrap();
        assert_eq!(result.execution.task_id, "limpeza_leve");
        assert_eq!(result.state, CleanupExecutionState::Completed);
        assert!(result.is_consistent());
    }

    #[test]
    fn ciclo_real_rejeita_transicoes_fora_da_ordem() {
        let mut lifecycle = CleanupExecutionLifecycle::new("limpeza_leve", "exec-2");
        assert!(lifecycle
            .transition(CleanupExecutionState::Completed)
            .is_err());
        lifecycle
            .transition(CleanupExecutionState::Running)
            .unwrap();
        lifecycle
            .transition(CleanupExecutionState::AwaitingConfirmation)
            .unwrap();
        lifecycle
            .transition(CleanupExecutionState::Running)
            .unwrap();
        assert!(lifecycle
            .transition(CleanupExecutionState::Pending)
            .is_err());
    }

    #[test]
    fn confirmacao_pode_terminar_com_execucao_parcial() {
        let mut lifecycle = CleanupExecutionLifecycle::new("limpeza_media", "exec-3");
        lifecycle
            .transition(CleanupExecutionState::Running)
            .unwrap();
        lifecycle
            .transition(CleanupExecutionState::AwaitingConfirmation)
            .unwrap();
        let result = lifecycle
            .finish(
                CleanupExecutionState::PartiallyCompleted,
                "Etapas anteriores concluídas antes da recusa.",
                None,
            )
            .unwrap();
        assert_eq!(result.state, CleanupExecutionState::PartiallyCompleted);
    }

    #[test]
    fn falhas_na_execucao_classificam_resultado_como_parcial() {
        assert_eq!(cleanup_result_state(0), CleanupExecutionState::Completed);
        assert_eq!(
            cleanup_result_state(1),
            CleanupExecutionState::PartiallyCompleted
        );
        assert_eq!(
            cleanup_result_state(26),
            CleanupExecutionState::PartiallyCompleted
        );
    }

    #[test]
    fn estados_terminais_nao_podem_ser_reabertos() {
        for state in [
            CleanupExecutionState::Completed,
            CleanupExecutionState::PartiallyCompleted,
            CleanupExecutionState::Cancelled,
            CleanupExecutionState::Failed,
        ] {
            assert!(state.is_terminal());
            assert!(!state.can_transition_to(CleanupExecutionState::Running));
        }
    }

    #[test]
    fn confirmacao_pode_retornar_a_execucao_ou_cancelar() {
        assert!(CleanupExecutionState::Running
            .can_transition_to(CleanupExecutionState::AwaitingConfirmation));
        assert!(CleanupExecutionState::AwaitingConfirmation
            .can_transition_to(CleanupExecutionState::Running));
        assert!(CleanupExecutionState::AwaitingConfirmation
            .can_transition_to(CleanupExecutionState::Cancelled));
    }

    #[test]
    fn cancelamento_exige_confirmacao_do_estado_final() {
        assert!(CleanupExecutionState::Running.can_transition_to(CleanupExecutionState::Cancelling));
        assert!(
            CleanupExecutionState::Cancelling.can_transition_to(CleanupExecutionState::Cancelled)
        );
        assert!(
            !CleanupExecutionState::Cancelling.can_transition_to(CleanupExecutionState::Completed)
        );
    }

    #[test]
    fn progresso_aceita_medicao_por_etapas_ou_percentual() {
        let progress = CleanupProgress {
            current: Some(2),
            total: Some(4),
            percent: Some(50),
            stage: "Limpeza temporária".into(),
            message: "Removendo arquivos".into(),
        };
        assert!(progress.is_valid());
    }

    #[test]
    fn progresso_rejeita_totais_incoerentes_e_percentual_acima_de_cem() {
        let mut progress = CleanupProgress {
            current: Some(5),
            total: Some(4),
            percent: Some(50),
            stage: String::new(),
            message: String::new(),
        };
        assert!(!progress.is_valid());
        progress.current = Some(1);
        progress.percent = Some(101);
        assert!(!progress.is_valid());
    }

    #[test]
    fn eventos_preservam_nomes_consumidos_pelo_frontend() {
        assert_eq!(
            serde_json::to_string(&CleanupEventKind::TaskStarted).unwrap(),
            "\"task-started\""
        );
        assert_eq!(
            serde_json::to_string(&CleanupEventKind::RestorePointConfirmationRequired).unwrap(),
            "\"restore-point-confirmation-required\""
        );
    }

    #[test]
    fn envelope_de_evento_preserva_identidade_e_formato_consumido_pelo_frontend() {
        let envelope = CleanupEventEnvelope {
            event: serde_json::to_string(&CleanupEventKind::TaskProgress)
                .unwrap()
                .trim_matches('"')
                .to_string(),
            task_id: "limpeza_media".to_string(),
            execution_id: "exec-caracterizacao-01".to_string(),
            message: "Etapa em andamento".to_string(),
            ok: true,
            progress: Some(r#"{"current":2,"total":4,"percent":35}"#.to_string()),
            risk: "moderate".to_string(),
        };

        let json = serde_json::to_value(envelope).unwrap();
        assert_eq!(json["event"], "task-progress");
        assert_eq!(json["task_id"], "limpeza_media");
        assert_eq!(json["execution_id"], "exec-caracterizacao-01");
        assert_eq!(json["message"], "Etapa em andamento");
        assert_eq!(json["ok"], true);
        assert_eq!(json["progress"], r#"{"current":2,"total":4,"percent":35}"#);
        assert_eq!(json["risk"], "moderate");
        assert_eq!(json.as_object().unwrap().len(), 7);
    }

    #[test]
    fn resultado_final_exige_contagem_de_etapas_coerente() {
        let result = CleanupOperationResult {
            execution: CleanupExecutionRef {
                task_id: "limpeza_leve".into(),
                execution_id: "exec-1".into(),
            },
            state: CleanupExecutionState::PartiallyCompleted,
            message: "Uma etapa falhou".into(),
            completed_steps: Some(3),
            total_steps: Some(4),
            change_id: None,
        };
        assert!(result.is_consistent());
    }
}
