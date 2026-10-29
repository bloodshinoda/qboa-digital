//! Runner controlado para o QBoa Memory Test.
//!
//! Executa somente sobre buffers alocados pelo próprio processo. O limite de
//! memória é um teto da sessão; o runner mantém apenas um bloco alocado por vez.

use super::algorithms::{
    self, AlgorithmId, MemoryMismatch, MAX_MISMATCH_DETAILS,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const MAX_SESSION_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const DEFAULT_BLOCK_BYTES: usize = 64 * 1024 * 1024;
pub const MIN_BLOCK_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_BLOCK_BYTES: usize = 256 * 1024 * 1024;
pub const MIN_AVAILABLE_RESERVE_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_SESSION_MISMATCH_DETAILS: usize = MAX_MISMATCH_DETAILS;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTestProfile {
    Fast,
    Standard,
    Extended,
}

impl MemoryTestProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Standard => "standard",
            Self::Extended => "extended",
        }
    }

    const fn default_budget_bytes(self) -> u64 {
        match self {
            Self::Fast => 512 * 1024 * 1024,
            Self::Standard => 2 * 1024 * 1024 * 1024,
            Self::Extended => 4 * 1024 * 1024 * 1024,
        }
    }

    const fn passes(self) -> u32 {
        match self {
            Self::Fast | Self::Standard => 1,
            Self::Extended => 2,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryTestConfig {
    pub profile: MemoryTestProfile,
    /// Orçamento cumulativo de bytes a percorrer, limitado a 8 GiB.
    pub memory_budget_bytes: u64,
    /// Tamanho desejado de cada bloco; o runner reduz conforme a pressão.
    pub preferred_block_bytes: usize,
    /// Seed base persistível do A04.
    pub seed: u64,
}

impl MemoryTestConfig {
    pub fn for_profile(profile: MemoryTestProfile) -> Self {
        Self {
            profile,
            memory_budget_bytes: profile.default_budget_bytes(),
            preferred_block_bytes: DEFAULT_BLOCK_BYTES,
            seed: default_seed(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.memory_budget_bytes == 0 {
            return Err("O orçamento de memória deve ser maior que zero.".to_string());
        }
        if self.preferred_block_bytes < MIN_BLOCK_BYTES
            || self.preferred_block_bytes > MAX_BLOCK_BYTES
        {
            return Err(format!(
                "O bloco deve ter entre {} MiB e {} MiB.",
                MIN_BLOCK_BYTES / (1024 * 1024),
                MAX_BLOCK_BYTES / (1024 * 1024)
            ));
        }
        Ok(())
    }

    fn effective_budget_bytes(&self) -> u64 {
        self.memory_budget_bytes.min(MAX_SESSION_BYTES)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTestStatus {
    Passed,
    ErrorsDetected,
    Cancelled,
    Inconclusive,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Completed,
    UserCancelled,
    MemoryPressure,
    AllocationFailed,
    MemoryStatusUnavailable,
    InvalidConfiguration,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryMismatchDetail {
    pub algorithm: String,
    pub pass: u32,
    pub block_index: u64,
    /// Offset dentro do buffer virtual do bloco, nunca um endereço físico.
    pub offset_in_block: usize,
    pub expected: u8,
    pub actual: u8,
    pub seed: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryTestReport {
    pub schema_version: u32,
    pub engine_version: String,
    pub profile: MemoryTestProfile,
    pub status: MemoryTestStatus,
    pub stop_reason: StopReason,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    pub memory_budget_bytes: u64,
    pub memory_tested_bytes: u64,
    pub block_size_preferred_bytes: usize,
    pub largest_block_allocated_bytes: usize,
    pub blocks_completed: u64,
    pub passes_planned: u32,
    pub passes_completed: u32,
    pub algorithms_planned: Vec<String>,
    pub algorithms_executed: Vec<String>,
    pub a04_seeds: Vec<u64>,
    pub mismatches_total: u64,
    /// Amostra global limitada a 100 detalhes, sem perder o agregado.
    pub mismatches: Vec<MemoryMismatchDetail>,
    pub coverage_description: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryTestProgress {
    pub profile: MemoryTestProfile,
    pub phase: String,
    pub memory_budget_bytes: u64,
    pub memory_tested_bytes: u64,
    pub current_block_index: u64,
    pub blocks_completed: u64,
    pub current_block_bytes: usize,
    pub mismatches_total: u64,
    pub percent: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryStatus {
    pub total_physical_bytes: u64,
    pub available_physical_bytes: u64,
}

/// Abstração pequena para simular pressão de memória em testes unitários.
pub trait MemoryStatusProvider {
    fn memory_status(&mut self) -> Result<MemoryStatus, String>;
}

#[derive(Default)]
pub struct SystemMemoryStatusProvider;

#[cfg(target_os = "windows")]
impl MemoryStatusProvider for SystemMemoryStatusProvider {
    fn memory_status(&mut self) -> Result<MemoryStatus, String> {
        #[repr(C)]
        struct MemoryStatusEx {
            length: u32,
            memory_load: u32,
            total_physical: u64,
            available_physical: u64,
            total_page_file: u64,
            available_page_file: u64,
            total_virtual: u64,
            available_virtual: u64,
            available_extended_virtual: u64,
        }

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
        }

        let mut status = MemoryStatusEx {
            length: std::mem::size_of::<MemoryStatusEx>() as u32,
            memory_load: 0,
            total_physical: 0,
            available_physical: 0,
            total_page_file: 0,
            available_page_file: 0,
            total_virtual: 0,
            available_virtual: 0,
            available_extended_virtual: 0,
        };

        // SAFETY: a estrutura segue MEMORYSTATUSEX e seu tamanho é informado.
        let succeeded = unsafe { GlobalMemoryStatusEx(&mut status) } != 0;
        if !succeeded || status.total_physical == 0 {
            return Err("O Windows não forneceu o estado da memória física.".to_string());
        }
        Ok(MemoryStatus {
            total_physical_bytes: status.total_physical,
            available_physical_bytes: status.available_physical,
        })
    }
}

#[cfg(not(target_os = "windows"))]
impl MemoryStatusProvider for SystemMemoryStatusProvider {
    fn memory_status(&mut self) -> Result<MemoryStatus, String> {
        Err("O teste de memória exige Windows 10/11 x64.".to_string())
    }
}

pub fn run(
    config: MemoryTestConfig,
    cancel: &AtomicBool,
    progress: impl FnMut(&MemoryTestProgress),
) -> MemoryTestReport {
    run_with_provider(config, cancel, SystemMemoryStatusProvider, progress)
}

/// Variante injetável para testes de política adaptativa sem alocar RAM real.
pub fn run_with_provider<P: MemoryStatusProvider>(
    config: MemoryTestConfig,
    cancel: &AtomicBool,
    mut provider: P,
    mut progress: impl FnMut(&MemoryTestProgress),
) -> MemoryTestReport {
    let started_clock = Instant::now();
    let started_at = timestamp_now();
    let profile = config.profile;
    let budget = config.effective_budget_bytes();
    let planned_algorithms = planned_algorithms(profile);
    let planned_names = planned_algorithms
        .iter()
        .map(|id| id.as_str().to_string())
        .collect::<Vec<_>>();
    let passes_planned = profile.passes();

    let mut report = MemoryTestReport {
        schema_version: 1,
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        profile,
        status: MemoryTestStatus::Failed,
        stop_reason: StopReason::InvalidConfiguration,
        started_at,
        finished_at: String::new(),
        duration_ms: 0,
        memory_budget_bytes: budget,
        memory_tested_bytes: 0,
        block_size_preferred_bytes: config.preferred_block_bytes,
        largest_block_allocated_bytes: 0,
        blocks_completed: 0,
        passes_planned,
        passes_completed: 0,
        algorithms_planned: planned_names,
        algorithms_executed: Vec::new(),
        a04_seeds: Vec::new(),
        mismatches_total: 0,
        mismatches: Vec::new(),
        coverage_description: coverage_description(profile),
        warnings: Vec::new(),
    };

    if let Err(error) = config.validate() {
        report.warnings.push(error);
        report.stop_reason = StopReason::InvalidConfiguration;
        return finish_report(report, started_clock);
    }

    if config.memory_budget_bytes > MAX_SESSION_BYTES {
        report.warnings.push(format!(
            "Orçamento solicitado limitado ao teto de {} GiB por sessão.",
            MAX_SESSION_BYTES / (1024 * 1024 * 1024)
        ));
    }

    let mut stop_reason = StopReason::Completed;
    let mut fatal_error = false;
    let mut block_index = 0u64;
    let mut passes_completed = 0u32;
    let mut tested_bytes = 0u64;
    let mut algorithms_executed = Vec::<String>::new();
    let mut seeds = Vec::<u64>::new();

    'passes: for pass in 0..passes_planned {
        let mut pass_tested_bytes = 0u64;
        let pass_budget = budget / u64::from(passes_planned)
            + if pass + 1 == passes_planned {
                budget % u64::from(passes_planned)
            } else {
                0
            };
        let mut pass_had_block = false;

        while pass_tested_bytes < pass_budget {
            if cancel.load(Ordering::Acquire) {
                stop_reason = StopReason::UserCancelled;
                break 'passes;
            }

            let memory_status = match provider.memory_status() {
                Ok(status) => status,
                Err(error) => {
                    report.warnings.push(error);
                    stop_reason = StopReason::MemoryStatusUnavailable;
                    fatal_error = true;
                    break 'passes;
                }
            };

            if memory_status.total_physical_bytes == 0 {
                report.warnings.push("Memória física total inválida.".to_string());
                stop_reason = StopReason::MemoryStatusUnavailable;
                fatal_error = true;
                break 'passes;
            }

            // Reserva dinâmica: o maior valor entre 1 GiB e 20% da RAM física.
            let reserve = MIN_AVAILABLE_RESERVE_BYTES
                .max(memory_status.total_physical_bytes / 5);
            let safe_available = memory_status
                .available_physical_bytes
                .saturating_sub(reserve);
            let remaining = pass_budget.saturating_sub(pass_tested_bytes);
            let desired = (config.preferred_block_bytes as u64).min(remaining);
            let allocation_bytes = desired.min(safe_available) as usize;

            if allocation_bytes == 0
                || (allocation_bytes < MIN_BLOCK_BYTES && remaining > allocation_bytes as u64)
            {
                report.warnings.push(format!(
                    "Execução interrompida para preservar pelo menos {} MiB de memória disponível.",
                    reserve / (1024 * 1024)
                ));
                stop_reason = StopReason::MemoryPressure;
                break 'passes;
            }

            let mut buffer = Vec::<u8>::new();
            if let Err(error) = buffer.try_reserve_exact(allocation_bytes) {
                report.warnings.push(format!(
                    "Falha ao reservar um bloco de {} MiB: {error}",
                    allocation_bytes / (1024 * 1024)
                ));
                stop_reason = StopReason::AllocationFailed;
                fatal_error = true;
                break 'passes;
            }
            buffer.resize(allocation_bytes, 0);
            report.largest_block_allocated_bytes = report
                .largest_block_allocated_bytes
                .max(buffer.len());

            pass_had_block = true;
            block_index = block_index.saturating_add(1);

            emit_progress(
                &mut progress,
                profile,
                "bloco_alocado",
                budget,
                tested_bytes,
                block_index,
                report.blocks_completed,
                buffer.len(),
                report.mismatches_total,
            );

            for algorithm in planned_algorithms.iter().copied() {
                if cancel.load(Ordering::Acquire) {
                    stop_reason = StopReason::UserCancelled;
                    break 'passes;
                }

                if algorithm == AlgorithmId::A04
                    && profile == MemoryTestProfile::Fast
                    && !fast_a04_sample(block_index)
                {
                    continue;
                }

                let seed = (algorithm == AlgorithmId::A04)
                    .then(|| derive_seed(config.seed, pass, block_index));
                if let Some(value) = seed {
                    seeds.push(value);
                }

                emit_progress(
                    &mut progress,
                    profile,
                    &format!("executando_{}", algorithm.as_str()),
                    budget,
                    tested_bytes,
                    block_index,
                    report.blocks_completed,
                    buffer.len(),
                    report.mismatches_total,
                );

                let result = match algorithm {
                    AlgorithmId::A01 => algorithms::run_a01(&mut buffer),
                    AlgorithmId::A02 => algorithms::run_a02(&mut buffer),
                    AlgorithmId::A03 => algorithms::run_a03(&mut buffer),
                    AlgorithmId::A04 => algorithms::run_a04(&mut buffer, seed.unwrap_or(config.seed)),
                    AlgorithmId::A05 => algorithms::run_a05(&mut buffer),
                };
                report.mismatches_total = report
                    .mismatches_total
                    .saturating_add(result.mismatches_total);
                append_mismatches(
                    &mut report.mismatches,
                    &result.mismatches,
                    algorithm,
                    pass + 1,
                    block_index,
                    seed,
                );
                if !algorithms_executed.iter().any(|name| name == algorithm.as_str()) {
                    algorithms_executed.push(algorithm.as_str().to_string());
                }

                emit_progress(
                    &mut progress,
                    profile,
                    &format!("concluido_{}", algorithm.as_str()),
                    budget,
                    tested_bytes,
                    block_index,
                    report.blocks_completed,
                    buffer.len(),
                    report.mismatches_total,
                );
            }

            // A block is counted once per pass and reflects bytes whose planned
            // algorithm set completed; release happens automatically at loop end.
            tested_bytes = tested_bytes.saturating_add(buffer.len() as u64);
            pass_tested_bytes = pass_tested_bytes.saturating_add(buffer.len() as u64);
            report.blocks_completed = report.blocks_completed.saturating_add(1);
            emit_progress(
                &mut progress,
                profile,
                "bloco_concluido",
                budget,
                tested_bytes,
                block_index,
                report.blocks_completed,
                0,
                report.mismatches_total,
            );
            drop(buffer);

            // Recheck memory pressure on each block; do not assume availability
            // stayed constant while patterns were running.
        }

        if stop_reason != StopReason::Completed {
            break;
        }
        if pass_had_block {
            passes_completed = passes_completed.saturating_add(1);
        }
        if pass_tested_bytes < pass_budget {
            // Do not count a pass as complete when its requested coverage was not reached.
            break;
        }
    }

    report.memory_tested_bytes = tested_bytes;
    report.passes_completed = passes_completed;
    report.algorithms_executed = algorithms_executed;
    report.a04_seeds = seeds;

    if stop_reason == StopReason::Completed && tested_bytes < budget {
        stop_reason = StopReason::MemoryPressure;
        report.warnings.push(
            "A cobertura planejada não foi concluída; o resultado é inconclusivo.".to_string(),
        );
    }
    if stop_reason == StopReason::Completed && tested_bytes >= budget {
        report.warnings.push(
            "O relatório cobre apenas os buffers virtuais alocados nesta sessão, não toda a RAM física.".to_string(),
        );
    }

    report.stop_reason = stop_reason.clone();
    report.status = match stop_reason {
        StopReason::UserCancelled => MemoryTestStatus::Cancelled,
        StopReason::MemoryPressure | StopReason::MemoryStatusUnavailable => {
            MemoryTestStatus::Inconclusive
        }
        StopReason::AllocationFailed | StopReason::InvalidConfiguration => {
            MemoryTestStatus::Failed
        }
        StopReason::Completed if report.mismatches_total > 0 => {
            MemoryTestStatus::ErrorsDetected
        }
        StopReason::Completed => MemoryTestStatus::Passed,
    };

    if fatal_error {
        report.warnings.push(
            "A execução foi encerrada antes de completar o orçamento solicitado.".to_string(),
        );
    }

    finish_report(report, started_clock)
}

fn planned_algorithms(profile: MemoryTestProfile) -> Vec<AlgorithmId> {
    match profile {
        MemoryTestProfile::Fast => vec![AlgorithmId::A01, AlgorithmId::A03, AlgorithmId::A04],
        MemoryTestProfile::Standard | MemoryTestProfile::Extended => vec![
            AlgorithmId::A01,
            AlgorithmId::A02,
            AlgorithmId::A03,
            AlgorithmId::A04,
            AlgorithmId::A05,
        ],
    }
}

fn fast_a04_sample(block_index: u64) -> bool {
    // Uma amostra em quatro blocos, incluindo o primeiro.
    block_index == 1 || block_index % 4 == 0
}

fn derive_seed(base: u64, pass: u32, block_index: u64) -> u64 {
    let mixed = base
        ^ (u64::from(pass).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        ^ block_index.wrapping_mul(0xD1B5_4A32_D192_ED03);
    if mixed == 0 { 0xA076_1D64_78BD_642F } else { mixed }
}

fn append_mismatches(
    destination: &mut Vec<MemoryMismatchDetail>,
    source: &[MemoryMismatch],
    algorithm: AlgorithmId,
    pass: u32,
    block_index: u64,
    seed: Option<u64>,
) {
    for mismatch in source {
        if destination.len() >= MAX_SESSION_MISMATCH_DETAILS {
            break;
        }
        destination.push(MemoryMismatchDetail {
            algorithm: algorithm.as_str().to_string(),
            pass,
            block_index,
            offset_in_block: mismatch.offset,
            expected: mismatch.expected,
            actual: mismatch.actual,
            seed,
        });
    }
}

fn emit_progress(
    callback: &mut impl FnMut(&MemoryTestProgress),
    profile: MemoryTestProfile,
    phase: &str,
    budget: u64,
    tested: u64,
    current_block_index: u64,
    blocks_completed: u64,
    current_block_bytes: usize,
    mismatches_total: u64,
) {
    callback(&MemoryTestProgress {
        profile,
        phase: phase.to_string(),
        memory_budget_bytes: budget,
        memory_tested_bytes: tested.min(budget),
        current_block_index,
        blocks_completed,
        current_block_bytes,
        mismatches_total,
        percent: if budget == 0 {
            0
        } else {
            ((tested.saturating_mul(100) / budget).min(100)) as u8
        },
    });
}

fn coverage_description(profile: MemoryTestProfile) -> String {
    match profile {
        MemoryTestProfile::Fast => {
            "Cobertura parcial: A01 e A03 em cada bloco; A04 amostrado em um a cada quatro blocos.".to_string()
        }
        MemoryTestProfile::Standard => {
            "A01–A05 aplicados a cada bloco concluído no orçamento configurado.".to_string()
        }
        MemoryTestProfile::Extended => {
            "A01–A05 repetidos em dois passes; A04 usa seeds distintas por passe e bloco.".to_string()
        }
    }
}

fn default_seed() -> u64 {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        ^ (std::process::id() as u64).rotate_left(17);
    if seed == 0 { 0xA076_1D64_78BD_642F } else { seed }
}

fn finish_report(mut report: MemoryTestReport, started: Instant) -> MemoryTestReport {
    report.finished_at = timestamp_now();
    report.duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    report
}

fn timestamp_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        day_seconds / 3600,
        (day_seconds % 3600) / 60,
        day_seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    struct SequenceProvider {
        statuses: Vec<MemoryStatus>,
        index: usize,
    }

    impl SequenceProvider {
        fn stable(total: u64, available: u64) -> Self {
            Self {
                statuses: vec![MemoryStatus {
                    total_physical_bytes: total,
                    available_physical_bytes: available,
                }],
                index: 0,
            }
        }
    }

    impl MemoryStatusProvider for SequenceProvider {
        fn memory_status(&mut self) -> Result<MemoryStatus, String> {
            let index = self.index.min(self.statuses.len().saturating_sub(1));
            self.index = self.index.saturating_add(1);
            self.statuses
                .get(index)
                .copied()
                .ok_or_else(|| "Nenhum estado de memória configurado.".to_string())
        }
    }

    fn tiny_config(profile: MemoryTestProfile) -> MemoryTestConfig {
        MemoryTestConfig {
            profile,
            memory_budget_bytes: 1024 * 1024,
            preferred_block_bytes: MIN_BLOCK_BYTES,
            seed: 1234,
        }
    }

    #[test]
    fn validation_rejects_zero_budget_and_invalid_block_sizes() {
        let mut config = tiny_config(MemoryTestProfile::Fast);
        config.memory_budget_bytes = 0;
        assert!(config.validate().is_err());
        config.memory_budget_bytes = 1024;
        config.preferred_block_bytes = MIN_BLOCK_BYTES - 1;
        assert!(config.validate().is_err());
        config.preferred_block_bytes = MAX_BLOCK_BYTES + 1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn config_defaults_define_distinct_profiles_under_the_hard_ceiling() {
        let fast = MemoryTestConfig::for_profile(MemoryTestProfile::Fast);
        let standard = MemoryTestConfig::for_profile(MemoryTestProfile::Standard);
        let extended = MemoryTestConfig::for_profile(MemoryTestProfile::Extended);
        assert!(fast.memory_budget_bytes < standard.memory_budget_bytes);
        assert!(standard.memory_budget_bytes < extended.memory_budget_bytes);
        assert!(extended.memory_budget_bytes <= MAX_SESSION_BYTES);
    }

    #[test]
    fn clamps_requested_budget_to_eight_gib() {
        let mut config = tiny_config(MemoryTestProfile::Fast);
        config.memory_budget_bytes = MAX_SESSION_BYTES + 1;
        assert_eq!(config.effective_budget_bytes(), MAX_SESSION_BYTES);
    }

    #[test]
    fn completes_fast_profile_and_reports_explicit_partial_coverage() {
        let config = tiny_config(MemoryTestProfile::Fast);
        let cancel = AtomicBool::new(false);
        let report = run_with_provider(
            config,
            &cancel,
            SequenceProvider::stable(8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024),
            |_| {},
        );
        assert_eq!(report.status, MemoryTestStatus::Passed);
        assert_eq!(report.memory_tested_bytes, 1024 * 1024);
        assert!(report.algorithms_executed.contains(&"A01".to_string()));
        assert!(report.algorithms_executed.contains(&"A03".to_string()));
        assert!(report.coverage_description.contains("parcial"));
        assert!(report.mismatches.is_empty());
    }

    #[test]
    fn reports_inconclusive_if_memory_reserve_would_be_violated() {
        let config = tiny_config(MemoryTestProfile::Standard);
        let cancel = AtomicBool::new(false);
        let report = run_with_provider(
            config,
            &cancel,
            SequenceProvider::stable(8 * 1024 * 1024 * 1024, 1024 * 1024 * 1024),
            |_| {},
        );
        assert_eq!(report.status, MemoryTestStatus::Inconclusive);
        assert_eq!(report.stop_reason, StopReason::MemoryPressure);
        assert_eq!(report.memory_tested_bytes, 0);
    }

    #[test]
    fn cancellation_never_reports_passed() {
        let config = tiny_config(MemoryTestProfile::Standard);
        let cancel = AtomicBool::new(true);
        let report = run_with_provider(
            config,
            &cancel,
            SequenceProvider::stable(8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024),
            |_| {},
        );
        assert_eq!(report.status, MemoryTestStatus::Cancelled);
        assert_eq!(report.stop_reason, StopReason::UserCancelled);
        assert_eq!(report.memory_tested_bytes, 0);
    }

    #[test]
    fn records_progress_and_never_exceeds_one_hundred_percent() {
        let config = tiny_config(MemoryTestProfile::Fast);
        let cancel = AtomicBool::new(false);
        let mut latest = None;
        let report = run_with_provider(
            config,
            &cancel,
            SequenceProvider::stable(8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024),
            |event| latest = Some(event.clone()),
        );
        assert_eq!(report.status, MemoryTestStatus::Passed);
        let latest = latest.expect("deve emitir progresso");
        assert_eq!(latest.percent, 100);
        assert_eq!(latest.memory_tested_bytes, report.memory_tested_bytes);
    }

    #[test]
    fn unavailable_memory_status_is_inconclusive_not_passed() {
        struct FailedProvider;
        impl MemoryStatusProvider for FailedProvider {
            fn memory_status(&mut self) -> Result<MemoryStatus, String> {
                Err("indisponível".to_string())
            }
        }
        let cancel = AtomicBool::new(false);
        let report = run_with_provider(
            tiny_config(MemoryTestProfile::Fast),
            &cancel,
            FailedProvider,
            |_| {},
        );
        assert_eq!(report.status, MemoryTestStatus::Inconclusive);
        assert_eq!(report.stop_reason, StopReason::MemoryStatusUnavailable);
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn keeps_global_mismatch_sample_with_limit_of_one_hundred() {
        let mut destination = Vec::new();
        let source = vec![
            MemoryMismatch {
                offset: 5,
                expected: 0,
                actual: 1,
            };
            MAX_SESSION_MISMATCH_DETAILS + 5
        ];
        append_mismatches(
            &mut destination,
            &source,
            AlgorithmId::A01,
            1,
            1,
            None,
        );
        append_mismatches(
            &mut destination,
            &source,
            AlgorithmId::A02,
            1,
            2,
            None,
        );
        assert_eq!(destination.len(), MAX_SESSION_MISMATCH_DETAILS);
    }

    #[test]
    fn extended_profile_has_two_passes_and_five_algorithms() {
        let config = MemoryTestConfig::for_profile(MemoryTestProfile::Extended);
        assert_eq!(config.profile.passes(), 2);
        assert_eq!(planned_algorithms(config.profile).len(), 5);
        assert_eq!(config.profile.as_str(), "extended");
    }
}
