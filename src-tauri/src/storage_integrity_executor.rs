//! Executor da verificação de integridade de armazenamento sem reparos.
//!
//! A execução é limitada a CHKDSK sem parâmetros de correção. O resultado só é
//! conclusivo quando a saída contém uma mensagem conhecida; caso contrário,
//! permanece inconclusivo.

use crate::storage_integrity::{
    StorageIntegrityCandidate, StorageIntegrityEligibility, StorageIntegrityExecutionStatus,
    StorageIntegrityFinding, StorageIntegrityMethod, StorageIntegrityReport,
    StorageIntegrityVolume, STORAGE_INTEGRITY_SCHEMA_VERSION,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static CHECK_COUNTER: AtomicU64 = AtomicU64::new(1);

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn new_check_id(started_at: u64) -> String {
    let sequence = CHECK_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("storage-integrity-{started_at}-{sequence}")
}

fn compact_output(output: &str) -> String {
    let compact: String = output
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let compact = compact.split_whitespace().collect::<Vec<_>>().join(" ");
    let shortened: String = compact.chars().take(900).collect();
    if shortened.is_empty() {
        "O CHKDSK não retornou texto de diagnóstico.".into()
    } else {
        shortened
    }
}

/// Reconhece somente mensagens explícitas conhecidas em português e inglês.
/// Saídas diferentes, inclusive falhas de acesso ao volume, ficam inconclusivas.
fn classify_output(output: &str) -> StorageIntegrityFinding {
    let normalized = output.to_lowercase();

    let clean_markers = [
        "found no problems",
        "não encontrou problemas",
        "nao encontrou problemas",
        "nenhum problema foi encontrado",
    ];
    if clean_markers
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return StorageIntegrityFinding::Clean;
    }

    let corruption_markers = [
        "found problems",
        "encontrou problemas",
        "errors found",
        "erros encontrados",
        "erros no sistema de arquivos",
    ];
    if corruption_markers
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return StorageIntegrityFinding::CorruptionDetected;
    }

    StorageIntegrityFinding::Inconclusive
}

fn build_report(
    volume: StorageIntegrityVolume,
    status: StorageIntegrityExecutionStatus,
    finding: StorageIntegrityFinding,
    started_at: u64,
    finished_at: u64,
    process_exit_code: Option<i32>,
    duration_ms: u64,
    message: String,
) -> StorageIntegrityReport {
    StorageIntegrityReport {
        schema_version: STORAGE_INTEGRITY_SCHEMA_VERSION,
        check_id: new_check_id(started_at),
        volume,
        method: StorageIntegrityMethod::ChkdskNoRepair,
        status,
        finding,
        started_at_unix_ms: started_at,
        finished_at_unix_ms: finished_at.max(started_at),
        process_exit_code,
        duration_ms,
        repair_performed: false,
        message,
    }
}

pub(crate) fn execute(candidate: StorageIntegrityCandidate) -> StorageIntegrityReport {
    let started_at = unix_ms();
    let started = Instant::now();

    match candidate.eligibility {
        StorageIntegrityEligibility::Unsupported => {
            return build_report(
                candidate.volume,
                StorageIntegrityExecutionStatus::Unsupported,
                StorageIntegrityFinding::NotApplicable,
                started_at,
                unix_ms(),
                None,
                started.elapsed().as_millis() as u64,
                candidate.reason,
            );
        }
        StorageIntegrityEligibility::Unavailable => {
            return build_report(
                candidate.volume,
                StorageIntegrityExecutionStatus::VolumeUnavailable,
                StorageIntegrityFinding::NotApplicable,
                started_at,
                unix_ms(),
                None,
                started.elapsed().as_millis() as u64,
                candidate.reason,
            );
        }
        StorageIntegrityEligibility::Eligible => {}
    }

    let Some(letter) = candidate.volume.drive_letter.clone() else {
        return build_report(
            candidate.volume,
            StorageIntegrityExecutionStatus::VolumeUnavailable,
            StorageIntegrityFinding::NotApplicable,
            started_at,
            unix_ms(),
            None,
            started.elapsed().as_millis() as u64,
            "Letra de unidade indisponível; a verificação não foi iniciada.".into(),
        );
    };

    if candidate.volume.file_system != crate::storage_integrity::StorageFileSystem::Ntfs
        || letter.len() != 2
        || !letter.as_bytes()[0].is_ascii_alphabetic()
        || letter.as_bytes()[1] != b':'
    {
        return build_report(
            candidate.volume,
            StorageIntegrityExecutionStatus::Unsupported,
            StorageIntegrityFinding::NotApplicable,
            started_at,
            unix_ms(),
            None,
            started.elapsed().as_millis() as u64,
            "O candidato não atende aos requisitos de NTFS e letra de unidade; nada foi executado."
                .into(),
        );
    }

    run_chkdsk(candidate.volume, &letter, started_at, started)
}

#[cfg(target_os = "windows")]
fn run_chkdsk(
    volume: StorageIntegrityVolume,
    drive_letter: &str,
    started_at: u64,
    started: Instant,
) -> StorageIntegrityReport {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    // Intencionalmente há somente um argumento: a letra do volume.
    // Não adicionar /f, /r, /x, /spotfix ou qualquer parâmetro de reparo.
    let result = Command::new("chkdsk.exe")
        .arg(drive_letter)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output();

    let finished_at = unix_ms();
    let duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;

    match result {
        Ok(output) => {
            let exit_code = output.status.code();
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push('\n');
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            let finding = classify_output(&text);
            let message = format!(
                "{} (código de saída: {}).",
                compact_output(&text),
                exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "indisponível".into())
            );

            let status = if exit_code.is_some() {
                StorageIntegrityExecutionStatus::Completed
            } else {
                StorageIntegrityExecutionStatus::Failed
            };
            let finding = if status == StorageIntegrityExecutionStatus::Completed {
                finding
            } else {
                StorageIntegrityFinding::Inconclusive
            };

            build_report(
                volume,
                status,
                finding,
                started_at,
                finished_at,
                exit_code,
                duration_ms,
                message,
            )
        }
        Err(error) => build_report(
            volume,
            StorageIntegrityExecutionStatus::Failed,
            StorageIntegrityFinding::Inconclusive,
            started_at,
            finished_at,
            None,
            duration_ms,
            format!("Não foi possível iniciar o CHKDSK: {error}"),
        ),
    }
}

#[cfg(not(target_os = "windows"))]
fn run_chkdsk(
    volume: StorageIntegrityVolume,
    _drive_letter: &str,
    started_at: u64,
    started: Instant,
) -> StorageIntegrityReport {
    build_report(
        volume,
        StorageIntegrityExecutionStatus::Unsupported,
        StorageIntegrityFinding::NotApplicable,
        started_at,
        unix_ms(),
        None,
        started.elapsed().as_millis() as u64,
        "A verificação de armazenamento está disponível somente no Windows.".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_integrity::{StorageFileSystem, StorageIntegrityVolume};

    fn volume() -> StorageIntegrityVolume {
        StorageIntegrityVolume {
            volume_id: r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\".into(),
            drive_letter: Some("C:".into()),
            file_system: StorageFileSystem::Ntfs,
        }
    }

    #[test]
    fn classifica_mensagem_explicita_de_volume_sem_problemas() {
        assert_eq!(
            classify_output("Windows has scanned the file system and found no problems."),
            StorageIntegrityFinding::Clean
        );
        assert_eq!(
            classify_output("O Windows verificou o sistema de arquivos e não encontrou problemas."),
            StorageIntegrityFinding::Clean
        );
    }

    #[test]
    fn classifica_mensagem_explicita_de_problemas() {
        assert_eq!(
            classify_output("Windows found problems with the file system."),
            StorageIntegrityFinding::CorruptionDetected
        );
        assert_eq!(
            classify_output("O Windows encontrou problemas no sistema de arquivos."),
            StorageIntegrityFinding::CorruptionDetected
        );
    }

    #[test]
    fn saida_ambigua_ou_falha_de_acesso_permanece_inconclusiva() {
        for output in [
            "",
            "Cannot open volume for direct access.",
            "Acesso negado.",
            "The type of the file system is NTFS.",
        ] {
            assert_eq!(
                classify_output(output),
                StorageIntegrityFinding::Inconclusive,
                "{output}"
            );
        }
    }

    #[test]
    fn mensagem_de_volume_limpo_tem_precedencia_sobre_termo_problems() {
        assert_eq!(
            classify_output("The scan found no problems; no further action is required."),
            StorageIntegrityFinding::Clean
        );
    }

    #[test]
    fn compactacao_remove_controles_e_limita_tamanho() {
        let value = compact_output(&format!("linha\n{}\nfim", "x".repeat(2_000)));
        assert!(!value.chars().any(char::is_control));
        assert!(value.chars().count() <= 900);
    }

    #[test]
    fn relatorio_do_executor_preserva_semantica_de_nao_reparo() {
        let report = build_report(
            volume(),
            StorageIntegrityExecutionStatus::Completed,
            StorageIntegrityFinding::Inconclusive,
            1_000,
            1_500,
            Some(1),
            500,
            "Saída inconclusiva.".into(),
        );
        assert!(report.is_valid());
        assert!(!report.repair_performed);
        assert_eq!(report.process_exit_code, Some(1));
        assert_eq!(report.finding, StorageIntegrityFinding::Inconclusive);
    }
}
