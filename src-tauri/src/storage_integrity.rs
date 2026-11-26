//! Contrato de dados para diagnóstico de integridade lógica de volumes.
//!
//! Este módulo define o contrato de dados, a elegibilidade e os tipos compartilhados
//! pelo executor. A execução não inclui parâmetros nem operações de reparo.

use serde::{Deserialize, Serialize};

pub(crate) const STORAGE_INTEGRITY_SCHEMA_VERSION: u32 = 2;

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

/// Método de diagnóstico exclusivamente não reparador.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StorageIntegrityMethod {
    ChkdskNoRepair,
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
#[serde(deny_unknown_fields)]
pub(crate) struct StorageIntegrityVolume {
    pub volume_id: String,
    pub drive_letter: Option<String>,
    pub file_system: StorageFileSystem,
}

/// Laudo de uma tentativa de diagnóstico, sem qualquer operação de reparo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StorageIntegrityReport {
    pub schema_version: u32,
    pub check_id: String,
    pub volume: StorageIntegrityVolume,
    pub method: StorageIntegrityMethod,
    pub status: StorageIntegrityExecutionStatus,
    pub finding: StorageIntegrityFinding,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    /// Código de saída do processo, quando ele chegou a ser iniciado e finalizado.
    pub process_exit_code: Option<i32>,
    /// Tempo decorrido medido por relógio monotônico, independente do relógio civil.
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
            || !is_non_empty_text(&self.check_id)
            || !is_valid_volume_id(&self.volume.volume_id)
            || !is_non_empty_text(&self.message)
            || self.repair_performed
            || self.started_at_unix_ms == 0
            || self.finished_at_unix_ms < self.started_at_unix_ms
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
            StorageIntegrityExecutionStatus::Completed => {
                self.process_exit_code.is_some()
                    && matches!(
                        self.finding,
                        StorageIntegrityFinding::Clean
                            | StorageIntegrityFinding::CorruptionDetected
                            | StorageIntegrityFinding::Inconclusive
                    )
            }
            StorageIntegrityExecutionStatus::Unsupported
            | StorageIntegrityExecutionStatus::VolumeUnavailable => {
                self.process_exit_code.is_none()
                    && self.finding == StorageIntegrityFinding::NotApplicable
            }
            StorageIntegrityExecutionStatus::Cancelled
            | StorageIntegrityExecutionStatus::Failed => matches!(
                self.finding,
                StorageIntegrityFinding::Inconclusive | StorageIntegrityFinding::NotApplicable
            ),
        }
    }
}

fn is_non_empty_text(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}

/// Exige o caminho GUID canônico retornado pelas APIs de enumeração de volumes do Windows.
fn is_valid_volume_id(value: &str) -> bool {
    let Some(guid) = value
        .strip_prefix(r"\\?\Volume{")
        .and_then(|value| value.strip_suffix(r"}\"))
    else {
        return false;
    };

    let groups: Vec<&str> = guid.split('-').collect();
    let expected_lengths = [8, 4, 4, 4, 12];

    groups.len() == expected_lengths.len()
        && groups
            .iter()
            .zip(expected_lengths)
            .all(|(group, expected)| {
                group.len() == expected && group.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
}

use crate::technical_inventory::{StorageBusType, StorageCategory, StorageDeviceKind};
/// Resultado da avaliação prévia de um volume; não inicia nenhuma verificação.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StorageIntegrityEligibility {
    Eligible,
    Unsupported,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct StorageIntegrityCandidate {
    pub volume: StorageIntegrityVolume,
    pub eligibility: StorageIntegrityEligibility,
    pub reason: String,
}

#[derive(Clone, Debug)]
struct DiskEligibilityContext {
    disk_id: String,
    device_kind: Option<StorageDeviceKind>,
    bus_type: Option<StorageBusType>,
    is_removable: Option<bool>,
}

#[derive(Clone, Debug)]
struct VolumeEligibilityInput {
    volume_id: String,
    drive_letter: Option<String>,
    file_system: Option<String>,
    associated_disk_ids: Vec<String>,
}

fn classify_volume_candidate(
    volume: VolumeEligibilityInput,
    disks: &[DiskEligibilityContext],
) -> StorageIntegrityCandidate {
    let file_system = match volume
        .file_system
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("ntfs") => StorageFileSystem::Ntfs,
        Some("refs") => StorageFileSystem::Refs,
        Some("fat32") => StorageFileSystem::Fat32,
        Some("exfat") => StorageFileSystem::ExFat,
        Some(_) => StorageFileSystem::Other,
        None => StorageFileSystem::Unknown,
    };

    let base = StorageIntegrityVolume {
        volume_id: volume.volume_id,
        drive_letter: volume.drive_letter,
        file_system,
    };

    let result = |eligibility, reason: &str| StorageIntegrityCandidate {
        volume: base.clone(),
        eligibility,
        reason: reason.into(),
    };

    if !is_valid_volume_id(&base.volume_id) {
        return result(
            StorageIntegrityEligibility::Unavailable,
            "Identidade GUID do volume inválida ou indisponível.",
        );
    }
    if base.file_system == StorageFileSystem::Unknown {
        return result(
            StorageIntegrityEligibility::Unavailable,
            "Sistema de arquivos não identificado.",
        );
    }
    if base.file_system != StorageFileSystem::Ntfs {
        return result(
            StorageIntegrityEligibility::Unsupported,
            "A primeira implementação de verificação online será limitada a NTFS.",
        );
    }
    let Some(letter) = base.drive_letter.as_deref() else {
        return result(
            StorageIntegrityEligibility::Unsupported,
            "Volume sem letra de unidade; ficará fora da primeira etapa por segurança.",
        );
    };
    let bytes = letter.as_bytes();
    if bytes.len() != 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return result(
            StorageIntegrityEligibility::Unavailable,
            "Letra de unidade inválida.",
        );
    }
    if volume.associated_disk_ids.is_empty() {
        return result(
            StorageIntegrityEligibility::Unavailable,
            "Não foi possível confirmar o dispositivo físico associado ao volume.",
        );
    }

    for disk_id in &volume.associated_disk_ids {
        let Some(disk) = disks.iter().find(|disk| disk.disk_id == *disk_id) else {
            return result(
                StorageIntegrityEligibility::Unavailable,
                "O dispositivo associado não consta na coleta atual.",
            );
        };
        if disk.is_removable != Some(false) {
            return result(
                if disk.is_removable == Some(true) {
                    StorageIntegrityEligibility::Unsupported
                } else {
                    StorageIntegrityEligibility::Unavailable
                },
                "Não foi possível confirmar que o dispositivo é fixo.",
            );
        }
        match disk.device_kind.as_ref() {
            Some(StorageDeviceKind::HardDisk | StorageDeviceKind::SolidStateDrive) => {}
            Some(_) => {
                return result(
                    StorageIntegrityEligibility::Unsupported,
                    "A primeira etapa aceita somente discos locais fixos identificados como HDD ou SSD.",
                );
            }
            None => {
                return result(
                    StorageIntegrityEligibility::Unavailable,
                    "Tipo do dispositivo associado não identificado.",
                );
            }
        }
        match disk.bus_type.as_ref() {
            None | Some(StorageBusType::Unknown) => {
                return result(
                    StorageIntegrityEligibility::Unavailable,
                    "Barramento do dispositivo associado não identificado.",
                );
            }
            Some(
                StorageBusType::Usb
                | StorageBusType::Sd
                | StorageBusType::Mmc
                | StorageBusType::Virtual
                | StorageBusType::FileBackedVirtual
                | StorageBusType::Spaces
                | StorageBusType::Iscsi
                | StorageBusType::NvmeOf,
            ) => {
                return result(
                    StorageIntegrityEligibility::Unsupported,
                    "Dispositivos removíveis, virtuais, Storage Spaces e armazenamento em rede ficam fora da primeira etapa.",
                );
            }
            Some(_) => {}
        }
    }

    result(
        StorageIntegrityEligibility::Eligible,
        "Volume NTFS local em disco fixo identificado; elegível para a etapa de verificação.",
    )
}

/// Classifica volumes a partir de uma coleta independente, sem alterar o inventário.
pub(crate) fn classify_storage_volumes(
    storage: &StorageCategory,
) -> Vec<StorageIntegrityCandidate> {
    let disks: Vec<DiskEligibilityContext> = storage
        .physical_disks
        .iter()
        .map(|disk| DiskEligibilityContext {
            disk_id: disk.disk_id.clone(),
            device_kind: disk.device_kind.value.clone(),
            bus_type: disk.bus_type.value.clone(),
            is_removable: disk.is_removable.value,
        })
        .collect();

    storage
        .volumes
        .iter()
        .filter_map(|volume| {
            let volume_id = volume.volume_id.value.clone()?;
            Some(classify_volume_candidate(
                VolumeEligibilityInput {
                    volume_id,
                    drive_letter: volume.drive_letter.value.clone(),
                    file_system: volume.file_system.value.clone(),
                    associated_disk_ids: volume.associated_disk_ids.clone(),
                },
                &disks,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_VOLUME_ID: &str = r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\";

    fn report(
        status: StorageIntegrityExecutionStatus,
        finding: StorageIntegrityFinding,
    ) -> StorageIntegrityReport {
        StorageIntegrityReport {
            schema_version: STORAGE_INTEGRITY_SCHEMA_VERSION,
            check_id: "integrity-test-001".into(),
            volume: StorageIntegrityVolume {
                volume_id: VALID_VOLUME_ID.into(),
                drive_letter: Some("C:".into()),
                file_system: StorageFileSystem::Ntfs,
            },
            method: StorageIntegrityMethod::ChkdskNoRepair,
            status,
            finding,
            started_at_unix_ms: 1_000,
            finished_at_unix_ms: 2_500,
            process_exit_code: matches!(status, StorageIntegrityExecutionStatus::Completed)
                .then_some(0),
            duration_ms: 1_500,
            repair_performed: false,
            message: "Verificação concluída.".into(),
        }
    }

    fn candidate_volume(
        file_system: Option<&str>,
        drive_letter: Option<&str>,
    ) -> VolumeEligibilityInput {
        VolumeEligibilityInput {
            volume_id: VALID_VOLUME_ID.into(),
            drive_letter: drive_letter.map(str::to_string),
            file_system: file_system.map(str::to_string),
            associated_disk_ids: vec!["disk:0".into()],
        }
    }

    fn fixed_ssd() -> DiskEligibilityContext {
        DiskEligibilityContext {
            disk_id: "disk:0".into(),
            device_kind: Some(StorageDeviceKind::SolidStateDrive),
            bus_type: Some(StorageBusType::Sata),
            is_removable: Some(false),
        }
    }

    #[test]
    fn ntfs_em_disco_fixo_e_elegivel_sem_executar_varredura() {
        let candidate =
            classify_volume_candidate(candidate_volume(Some("NTFS"), Some("C:")), &[fixed_ssd()]);
        assert_eq!(candidate.eligibility, StorageIntegrityEligibility::Eligible);
        assert_eq!(candidate.volume.file_system, StorageFileSystem::Ntfs);
    }

    #[test]
    fn sistema_de_arquivos_nao_ntfs_e_excluido() {
        for file_system in ["ReFS", "FAT32", "exFAT", "RAW"] {
            let candidate = classify_volume_candidate(
                candidate_volume(Some(file_system), Some("D:")),
                &[fixed_ssd()],
            );
            assert_eq!(
                candidate.eligibility,
                StorageIntegrityEligibility::Unsupported,
                "{file_system}"
            );
        }
    }

    #[test]
    fn volume_sem_letra_nao_e_elegivel_na_primeira_etapa() {
        let candidate =
            classify_volume_candidate(candidate_volume(Some("NTFS"), None), &[fixed_ssd()]);
        assert_eq!(
            candidate.eligibility,
            StorageIntegrityEligibility::Unsupported
        );
    }

    #[test]
    fn dispositivo_removivel_virtual_ou_desconhecido_nao_e_elegivel() {
        let mut removable = fixed_ssd();
        removable.is_removable = Some(true);
        assert_eq!(
            classify_volume_candidate(candidate_volume(Some("NTFS"), Some("E:")), &[removable])
                .eligibility,
            StorageIntegrityEligibility::Unsupported
        );

        let mut virtual_disk = fixed_ssd();
        virtual_disk.device_kind = Some(StorageDeviceKind::VirtualDisk);
        virtual_disk.bus_type = Some(StorageBusType::FileBackedVirtual);
        assert_eq!(
            classify_volume_candidate(candidate_volume(Some("NTFS"), Some("F:")), &[virtual_disk])
                .eligibility,
            StorageIntegrityEligibility::Unsupported
        );

        let mut unknown = fixed_ssd();
        unknown.is_removable = None;
        assert_eq!(
            classify_volume_candidate(candidate_volume(Some("NTFS"), Some("G:")), &[unknown])
                .eligibility,
            StorageIntegrityEligibility::Unavailable
        );
    }

    #[test]
    fn volume_sem_associacao_confiavel_nao_e_elegivel() {
        let mut volume = candidate_volume(Some("NTFS"), Some("C:"));
        volume.associated_disk_ids.clear();
        assert_eq!(
            classify_volume_candidate(volume, &[fixed_ssd()]).eligibility,
            StorageIntegrityEligibility::Unavailable
        );
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
        assert_eq!(value.process_exit_code, Some(0));
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
    fn aceita_combinacoes_validas_de_estado_e_resultado() {
        for (status, finding) in [
            (
                StorageIntegrityExecutionStatus::Completed,
                StorageIntegrityFinding::Clean,
            ),
            (
                StorageIntegrityExecutionStatus::Completed,
                StorageIntegrityFinding::CorruptionDetected,
            ),
            (
                StorageIntegrityExecutionStatus::Completed,
                StorageIntegrityFinding::Inconclusive,
            ),
            (
                StorageIntegrityExecutionStatus::Unsupported,
                StorageIntegrityFinding::NotApplicable,
            ),
            (
                StorageIntegrityExecutionStatus::VolumeUnavailable,
                StorageIntegrityFinding::NotApplicable,
            ),
            (
                StorageIntegrityExecutionStatus::Cancelled,
                StorageIntegrityFinding::Inconclusive,
            ),
            (
                StorageIntegrityExecutionStatus::Cancelled,
                StorageIntegrityFinding::NotApplicable,
            ),
            (
                StorageIntegrityExecutionStatus::Failed,
                StorageIntegrityFinding::Inconclusive,
            ),
            (
                StorageIntegrityExecutionStatus::Failed,
                StorageIntegrityFinding::NotApplicable,
            ),
        ] {
            assert!(
                report(status, finding).is_valid(),
                "{status:?} / {finding:?}"
            );
        }
    }

    #[test]
    fn rejeita_combinacoes_incoerentes_de_estado_e_resultado() {
        for (status, finding) in [
            (
                StorageIntegrityExecutionStatus::Unsupported,
                StorageIntegrityFinding::Clean,
            ),
            (
                StorageIntegrityExecutionStatus::VolumeUnavailable,
                StorageIntegrityFinding::CorruptionDetected,
            ),
            (
                StorageIntegrityExecutionStatus::Failed,
                StorageIntegrityFinding::CorruptionDetected,
            ),
            (
                StorageIntegrityExecutionStatus::Completed,
                StorageIntegrityFinding::NotApplicable,
            ),
        ] {
            assert!(!report(status, finding).is_valid());
        }
    }

    #[test]
    fn execucao_concluida_exige_codigo_de_saida_do_processo() {
        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Inconclusive,
        );
        value.process_exit_code = None;
        assert!(!value.is_valid());
    }

    #[test]
    fn estados_sem_execucao_nao_podem_informar_codigo_de_saida() {
        for status in [
            StorageIntegrityExecutionStatus::Unsupported,
            StorageIntegrityExecutionStatus::VolumeUnavailable,
        ] {
            let mut value = report(status, StorageIntegrityFinding::NotApplicable);
            value.process_exit_code = Some(0);
            assert!(!value.is_valid(), "{status:?}");
        }
    }

    #[test]
    fn resultado_integro_registra_codigo_de_saida_sem_inferir_resultado() {
        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.process_exit_code = Some(1);
        assert!(value.is_valid());
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
    fn rejeita_identidade_e_textos_invalidos() {
        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.volume.volume_id.clear();
        assert!(!value.is_valid());

        for invalid_id in [
            "",
            r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdeg}\",
            r"\\?\Volume{0123456789ab-cdef-0123-456789abcdef}\",
            r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}",
            r"Volume{01234567-89ab-cdef-0123-456789abcdef}\",
        ] {
            let mut value = report(
                StorageIntegrityExecutionStatus::Completed,
                StorageIntegrityFinding::Clean,
            );
            value.volume.volume_id = invalid_id.into();
            assert!(!value.is_valid(), "ID deveria ser rejeitado: {invalid_id}");
        }

        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.check_id = "  ".into();
        assert!(!value.is_valid());

        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.message = "\n".into();
        assert!(!value.is_valid());
    }

    #[test]
    fn rejeita_letras_de_unidade_invalidas() {
        for invalid_letter in ["C", "CC:", "1:", "C/", " C:", "C: "] {
            let mut value = report(
                StorageIntegrityExecutionStatus::Completed,
                StorageIntegrityFinding::Clean,
            );
            value.volume.drive_letter = Some(invalid_letter.into());
            assert!(
                !value.is_valid(),
                "Letra deveria ser rejeitada: {invalid_letter}"
            );
        }
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

    #[test]
    fn aceita_duracao_monotonica_independente_dos_timestamps_civis() {
        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.finished_at_unix_ms = value.started_at_unix_ms + 10_000;
        value.duration_ms = 0;
        assert!(value.is_valid());
    }

    #[test]
    fn rejeita_timestamps_civis_invertidos_ou_inicio_invalido() {
        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.finished_at_unix_ms = value.started_at_unix_ms - 1;
        assert!(!value.is_valid());

        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.started_at_unix_ms = 0;
        assert!(!value.is_valid());
    }

    #[test]
    fn serializa_enums_e_campos_com_nomes_estaveis() {
        let value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::CorruptionDetected,
        );
        let json = serde_json::to_value(value).unwrap();
        assert_eq!(json["method"], "chkdsk_no_repair");
        assert_eq!(json["status"], "completed");
        assert_eq!(json["finding"], "corruption_detected");
        assert_eq!(json["volume"]["file_system"], "ntfs");
        assert_eq!(json["repair_performed"], false);
        assert_eq!(json["schema_version"], 2);
        assert_eq!(json["process_exit_code"], 0);
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
    fn rejeita_campos_desconhecidos_no_laudo_e_no_volume() {
        let report_json = serde_json::json!({
            "schema_version": 2,
            "check_id": "integrity-test-001",
            "volume": {
                "volume_id": VALID_VOLUME_ID,
                "drive_letter": "C:",
                "file_system": "ntfs",
                "unexpected": true
            },
            "method": "chkdsk_no_repair",
            "status": "completed",
            "finding": "clean",
            "started_at_unix_ms": 1000,
            "finished_at_unix_ms": 2500,
            "process_exit_code": 0,
            "duration_ms": 1500,
            "repair_performed": false,
            "message": "Verificação concluída."
        });
        assert!(serde_json::from_value::<StorageIntegrityReport>(report_json).is_err());

        let mut report_json = serde_json::to_value(report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        ))
        .unwrap();
        report_json["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<StorageIntegrityReport>(report_json).is_err());
    }

    #[test]
    fn rejeita_versao_de_schema_incompativel() {
        let mut value = report(
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Clean,
        );
        value.schema_version += 1;
        assert!(!value.is_valid());
    }
}
