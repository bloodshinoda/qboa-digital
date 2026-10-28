//! Algoritmos de teste de memória sobre buffers pequenos e controlados.
//!
//! Esta camada não aloca memória por conta própria e não acessa endereços físicos.
//! Cada função recebe um slice mutável fornecido pelo chamador.

/// Versão do gerador pseudoaleatório usado pelo A04. Deve ser persistida no laudo.
pub const A04_PRNG_VERSION: &str = "xorshift64star-byte-v1";

/// Limite de divergências detalhadas mantidas por execução de algoritmo.
pub const MAX_MISMATCH_DETAILS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlgorithmId {
    A01,
    A02,
    A03,
    A04,
    A05,
}

impl AlgorithmId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A01 => "A01",
            Self::A02 => "A02",
            Self::A03 => "A03",
            Self::A04 => "A04",
            Self::A05 => "A05",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryMismatch {
    /// Offset no slice virtual testado; não é um endereço físico.
    pub offset: usize,
    pub expected: u8,
    pub actual: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlgorithmReport {
    pub algorithm: AlgorithmId,
    /// Contador total, mesmo quando a lista detalhada atinge seu limite.
    pub mismatches_total: u64,
    /// No máximo MAX_MISMATCH_DETAILS divergências detalhadas.
    pub mismatches: Vec<MemoryMismatch>,
}

impl AlgorithmReport {
    fn new(algorithm: AlgorithmId) -> Self {
        Self {
            algorithm,
            mismatches_total: 0,
            mismatches: Vec::new(),
        }
    }

    fn record(&mut self, offset: usize, expected: u8, actual: u8) {
        self.mismatches_total = self.mismatches_total.saturating_add(1);
        if self.mismatches.len() < MAX_MISMATCH_DETAILS {
            self.mismatches.push(MemoryMismatch {
                offset,
                expected,
                actual,
            });
        }
    }

    fn merge(&mut self, other: Self) {
        self.mismatches_total = self
            .mismatches_total
            .saturating_add(other.mismatches_total);
        for mismatch in other.mismatches {
            if self.mismatches.len() >= MAX_MISMATCH_DETAILS {
                break;
            }
            self.mismatches.push(mismatch);
        }
    }
}

fn verify_pattern(
    buffer: &[u8],
    expected_at: impl Fn(usize) -> u8,
    report: &mut AlgorithmReport,
) {
    for (offset, actual) in buffer.iter().copied().enumerate() {
        let expected = expected_at(offset);
        if actual != expected {
            report.record(offset, expected, actual);
        }
    }
}

/// A01 — escreve e verifica os padrões sólidos 0x00 e 0xFF.
pub fn run_a01(buffer: &mut [u8]) -> AlgorithmReport {
    let mut report = AlgorithmReport::new(AlgorithmId::A01);
    for pattern in [0x00, 0xFF] {
        buffer.fill(pattern);
        verify_pattern(buffer, |_| pattern, &mut report);
    }
    report
}

/// A02 — padrões walking-bits e seus complementos, um bit por vez.
pub fn run_a02(buffer: &mut [u8]) -> AlgorithmReport {
    let mut report = AlgorithmReport::new(AlgorithmId::A02);
    for bit in 0..u8::BITS {
        let pattern = 1u8 << bit;
        for value in [pattern, !pattern] {
            buffer.fill(value);
            verify_pattern(buffer, |_| value, &mut report);
        }
    }
    report
}

/// A03 — tabuleiro de xadrez alternando 0xAA e 0x55.
pub fn run_a03(buffer: &mut [u8]) -> AlgorithmReport {
    let mut report = AlgorithmReport::new(AlgorithmId::A03);
    for pattern in [0xAA, 0x55] {
        buffer.fill(pattern);
        verify_pattern(buffer, |_| pattern, &mut report);
    }
    report
}

#[derive(Clone, Copy, Debug)]
struct XorShift64Star {
    state: u64,
}

impl XorShift64Star {
    fn new(seed: u64) -> Self {
        // O estado zero é absorvente no xorshift; normalize-o de forma definida.
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_byte(&mut self) -> u8 {
        (self.next_u64() >> 56) as u8
    }
}

fn seeded_pattern(len: usize, seed: u64) -> Vec<u8> {
    let mut rng = XorShift64Star::new(seed);
    (0..len).map(|_| rng.next_byte()).collect()
}

/// A04 — padrão pseudoaleatório determinístico.
/// A mesma versão, seed e tamanho produzem os mesmos bytes.
pub fn run_a04(buffer: &mut [u8], seed: u64) -> AlgorithmReport {
    let mut report = AlgorithmReport::new(AlgorithmId::A04);
    let expected = seeded_pattern(buffer.len(), seed);
    buffer.copy_from_slice(&expected);
    verify_pattern(buffer, |offset| expected[offset], &mut report);
    report
}

/// A05 — inversões móveis: zero para FF em ordem direta e FF para zero em
/// ordem reversa. O buffer termina preenchido com zero.
pub fn run_a05(buffer: &mut [u8]) -> AlgorithmReport {
    let mut report = AlgorithmReport::new(AlgorithmId::A05);
    buffer.fill(0x00);

    for offset in 0..buffer.len() {
        let actual = buffer[offset];
        if actual != 0x00 {
            report.record(offset, 0x00, actual);
        }
        buffer[offset] = 0xFF;
    }

    for offset in (0..buffer.len()).rev() {
        let actual = buffer[offset];
        if actual != 0xFF {
            report.record(offset, 0xFF, actual);
        }
        buffer[offset] = 0x00;
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_checks_both_solid_patterns_and_handles_empty_buffer() {
        let mut buffer = [0xA5; 17];
        let report = run_a01(&mut buffer);
        assert_eq!(report.algorithm, AlgorithmId::A01);
        assert_eq!(report.mismatches_total, 0);
        assert!(report.mismatches.is_empty());
        assert!(buffer.iter().all(|byte| *byte == 0xFF));

        let report = run_a01(&mut []);
        assert_eq!(report.mismatches_total, 0);
    }

    #[test]
    fn a02_covers_each_single_bit_and_its_complement() {
        let mut buffer = [0x00; 9];
        let report = run_a02(&mut buffer);
        assert_eq!(report.algorithm, AlgorithmId::A02);
        assert_eq!(report.mismatches_total, 0);
        // O último padrão é o complemento do bit 7.
        assert!(buffer.iter().all(|byte| *byte == 0x7F));
    }

    #[test]
    fn a03_checks_alternating_checkerboard_patterns() {
        let mut buffer = [0x00; 13];
        let report = run_a03(&mut buffer);
        assert_eq!(report.mismatches_total, 0);
        assert!(buffer.iter().all(|byte| *byte == 0x55));
    }

    #[test]
    fn a04_is_reproducible_for_same_seed_and_version() {
        let mut first = [0; 257];
        let mut second = [0; 257];
        let mut other_seed = [0; 257];

        assert_eq!(run_a04(&mut first, 0x1234).mismatches_total, 0);
        assert_eq!(run_a04(&mut second, 0x1234).mismatches_total, 0);
        assert_eq!(run_a04(&mut other_seed, 0x1235).mismatches_total, 0);

        assert_eq!(first, second);
        assert_ne!(first, other_seed);
        assert_eq!(A04_PRNG_VERSION, "xorshift64star-byte-v1");
    }

    #[test]
    fn a04_zero_seed_is_deterministic_and_not_an_all_zero_stream() {
        let mut first = [0; 64];
        let mut second = [0; 64];
        run_a04(&mut first, 0);
        run_a04(&mut second, 0);
        assert_eq!(first, second);
        assert!(first.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn a05_runs_forward_and_reverse_inversions_and_leaves_zeroes() {
        let mut buffer = [0xA5; 31];
        let report = run_a05(&mut buffer);
        assert_eq!(report.mismatches_total, 0);
        assert!(buffer.iter().all(|byte| *byte == 0));
        assert_eq!(run_a05(&mut []).mismatches_total, 0);
    }

    #[test]
    fn verification_detects_a_simulated_mismatch_and_reports_virtual_offset() {
        let buffer = [0x00, 0x7F, 0x00];
        let mut report = AlgorithmReport::new(AlgorithmId::A01);
        verify_pattern(&buffer, |_| 0x00, &mut report);

        assert_eq!(report.mismatches_total, 1);
        assert_eq!(
            report.mismatches,
            vec![MemoryMismatch {
                offset: 1,
                expected: 0x00,
                actual: 0x7F,
            }]
        );
    }

    #[test]
    fn mismatch_details_are_capped_but_total_count_is_preserved() {
        let buffer = [0xFF; MAX_MISMATCH_DETAILS + 23];
        let mut report = AlgorithmReport::new(AlgorithmId::A03);
        verify_pattern(&buffer, |_| 0x00, &mut report);

        assert_eq!(report.mismatches_total, (MAX_MISMATCH_DETAILS + 23) as u64);
        assert_eq!(report.mismatches.len(), MAX_MISMATCH_DETAILS);
        assert_eq!(report.mismatches[0].offset, 0);
        assert_eq!(
            report.mismatches[MAX_MISMATCH_DETAILS - 1].offset,
            MAX_MISMATCH_DETAILS - 1
        );
    }

    #[test]
    fn algorithms_respect_small_odd_sized_buffers() {
        for len in [0, 1, 2, 3, 7, 8, 9, 15, 16, 17, 63, 64, 65, 127] {
            let mut buffer = vec![0x3C; len];
            assert_eq!(run_a01(&mut buffer).mismatches_total, 0);
            assert_eq!(run_a02(&mut buffer).mismatches_total, 0);
            assert_eq!(run_a03(&mut buffer).mismatches_total, 0);
            assert_eq!(run_a04(&mut buffer, 42).mismatches_total, 0);
            assert_eq!(run_a05(&mut buffer).mismatches_total, 0);
            assert_eq!(buffer.len(), len);
        }
    }
}
