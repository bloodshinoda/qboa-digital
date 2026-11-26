//! Contrato de dados para diagnóstico de integridade lógica de volumes.
//!
//! Este módulo não executa comandos nem realiza reparos. Ele define apenas o formato
//! validado que será consumido pelo futuro executor de verificação.

use serde::{Deserialize, Serialize};

pub(crate) const STORAGE_INTEGRITY_SCHEMA_VERSION: u32 = 1;

/// Sistema de arquivos informado pelo Windows para o volume examinado.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StorageFileSystem {
    Ntfs,
    Refs,
    Fat32,
    ExFat,
    Other,
    Unknown,
}

/// Método de diagnóstico. A primeira implementação será exclusivamente não reparadora.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StorageIntegrityMethod {
    RepairVolumeScan,
}

/// Estado operacional da tentativa de diagnóstico.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StorageIntegrityExecutionStatus {
    Completed,
    Unsupported,
    VolumeUnavailable,
    Cancelled,
    Failed,
}

/// Conclusão técnica. Ausência de conclusão nunca equivale a volume íntegro.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StorageIntegrityFinding {
    Clean,
    CorruptionDetected,
    Inconclusive,
    NotApplicable,
}

/// Identidade observada do volume. O GUID é preferido; a letra é apenas complementar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StorageIntegrityVolume {
    pub volume_id: String,
    pub drive_letter: Option<String>,
    pub file_system: StorageFileSystem,
}

/// Laudo de uma tentativa de diagnóstico, sem qualquer operação de reparo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StorageIntegrityReport {
    pub schema_version: u32,
    pub check_id: String,
    pub volume: StorageIntegrityVolume,
    pub method: StorageIntegrityMethod,
    pub status: StorageIntegrityExecutionStatus,
    pub finding: StorageIntegrityFinding,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    pub duration_ms: u64,
    /// Campo explícito para impedir que um resultado de diagnóstico seja confundido
    /// com uma execução corretiva. Deve permanecer false neste contrato.
    pub repair_performed: bool,
    pub message: String,
}

impl StorageIntegrityReport {
    /// Verifica consistência estrutural e combinações permitidas entre estado e resultado.
    pub(crate) fn is_valid(&self) -> bool {
        if self.schema_version != STORAGE_INTEGRITY_SCHEMA_VERSION
            || self.check_id.trim().is_empty()
            || self.volume.volume_id.trim().is_empty()
            || self.started_at_unix_ms == 0
            || self.finished_at_unix_ms < self.started_at_unix_ms
            || self.duration_ms == 0
            || self.repair_performed
            || self.message.trim().is_empty()
        {
            return false;
        }

        if let Some(letter) = self.volume.drive_letter.as_deref() {
            let bytes = letter.as_bytes();
            if bytes.len() != 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
                return false;
            }
        }

        match self.status {
            StorageIntegrityExecutionStatus::Completed => matches!(
                self.finding,
                StorageIntegrityFinding::Clean
                    | StorageIntegrityFinding::CorruptionDetected
                    | StorageIntegrityFinding::Inconclusive
            ),
            StorageIntegrityExecutionStatus::Unsupported
            | StorageIntegrityExecutionStatus::VolumeUnavailable => {
                self.finding == StorageIntegrityFinding::NotApplicable
            }
            StorageIntegrityExecutionStatus::Cancelled
            | StorageIntegrityExecutionStatus::Failed => matches!(
                self.finding,
                StorageIntegrityFinding::Inconclusive | StorageIntegrityFinding::NotApplicable
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(
        status: StorageIntegrityExecutionStatus,
        finding: StorageIntegrityFinding,
    ) -> StorageIntegrityReport {
        StorageIntegrityReport {
            schema_version: STORAGE_INTEGRITY_SCHEMA_VERSION,
            check_id: "integrity-test-001".into(),
            volume: StorageIntegrityVolume {
                volume_id: r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\".into(),
                drive_letter: Some("C:".into()),
                file_system: StorageFileSystem::Ntfs,
            },
            method: StorageIntegrityMethod::RepairVolumeScan,
            status,
            finding,
            started_at_unix_ms: 1_000,
            finished_at_unix_ms: 2_500,
            duration_ms: 1_500,
            repair_performed: false,
            message: "Verificação concluída.".into(),
        }
    }

    #[test]
    fn aceita_volume_concluido_sem_corrupcao() {
        assert!(report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean
        )
        .is_valid());
    }

    #[test]
    fn aceita_corrupcao_detectada_sem_indicar_reparo() {
        let value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::CorruptionDetected,
        );
        assert!(value.is_valid());
        assert!(!value.repair_performed);
    }

    #[test]
    fn resultado_inconclusivo_nunca_e_classificado_como_integro() {
        let value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Inconclusive,
        );
        assert!(value.is_valid());
        assert_ne!(value.finding, StorageIntegrityFinding::Clean);
    }

    #[test]
    fn rejeita_combinacoes_incoerentes_de_estado_e_resultado() {
        assert!(!report(
            StorageIntegrityExecutionStatus::Unsupported,
            StorageIntegrityFinding::Clean
        )
        .is_valid());
        assert!(!report(
            StorageIntegrityExecutionStatus::Failed,
            StorageIntegrityFinding::CorruptionDetected
        )
        .is_valid());
        assert!(!report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::NotApplicable
        )
        .is_valid());
    }

    #[test]
    fn rejeita_qualquer_indicacao_de_reparo() {
        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.repair_performed = true;
        assert!(!value.is_valid());
    }

    #[test]
    fn rejeita_identidade_tempos_e_letra_de_unidade_invalidos() {
        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.volume.volume_id.clear();
        assert!(!value.is_valid());

        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.volume.drive_letter = Some("C".into());
        assert!(!value.is_valid());

        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.finished_at_unix_ms = 999;
        assert!(!value.is_valid());
    }

    #[test]
    fn serializa_enums_e_campos_com_nomes_estaveis() {
        let value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::CorruptionDetected,
        );
        let json = serde_json::to_value(value).unwrap();
        assert_eq!(json["method"], "repair_volume_scan");
        assert_eq!(json["status"], "completed");
        assert_eq!(json["finding"], "corruption_detected");
        assert_eq!(json["volume"]["file_system"], "ntfs");
        assert_eq!(json["repair_performed"], false);
        assert_eq!(json["schema_version"], 1);
    }

    #[test]
    fn desserializa_laudo_sem_perder_o_estado() {
        let original = report(
            StorageIntegrityExecutionStatus::Cancelled,
            StorageIntegrityFinding::Inconclusive,
        );
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: StorageIntegrityReport = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
        assert!(decoded.is_valid());
    }

    #[test]
    fn aceita_volume_sem_letra_de_unidade() {
        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.volume.drive_letter = None;
        assert!(value.is_valid());
    }
}
