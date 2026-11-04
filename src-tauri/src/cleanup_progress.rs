// Cálculo puro do progresso agregado das etapas de limpeza.
//
// O DISM é a etapa mais demorada: recebe 55% do progresso em limpeza média
// (até 5 etapas) e 65% em limpeza pesada (6+ etapas). Seu avanço fica
// limitado a 94% desse peso até o processo encerrar, contemplando a pausa
// frequente após exibir 100%.

pub(crate) fn cleanup_progress_percent(
    current: u32,
    total: u32,
    is_dism: bool,
    dism_percent: f64,
) -> u32 {
    if total == 0 || current == 0 || current > total {
        return 0;
    }

    let dism_weight = if total >= 6 { 0.65 } else { 0.55 };
    let other_weight = (1.0 - dism_weight) / (total - 1).max(1) as f64;

    let progress = if is_dism {
        let stage_progress = dism_percent.clamp(0.0, 100.0) / 100.0 * 0.94;
        (current - 1) as f64 * other_weight + stage_progress * dism_weight
    } else {
        let completed_before = if current > 1 { current - 1 } else { 0 };
        completed_before as f64 * other_weight
    };

    (progress.clamp(0.0, 1.0) * 100.0).floor() as u32
}

#[cfg(test)]
mod tests {
    use super::cleanup_progress_percent;

    #[test]
    fn progresso_inicial_permanece_em_zero() {
        assert_eq!(cleanup_progress_percent(1, 5, false, 0.0), 0);
        assert_eq!(cleanup_progress_percent(1, 6, false, 0.0), 0);
    }

    #[test]
    fn etapas_concluidas_usam_o_peso_restante_na_limpeza_media() {
        assert_eq!(cleanup_progress_percent(2, 5, false, 0.0), 11);
        assert_eq!(cleanup_progress_percent(5, 5, false, 0.0), 45);
    }

    #[test]
    fn etapas_concluidas_usam_o_peso_restante_na_limpeza_pesada() {
        assert_eq!(cleanup_progress_percent(2, 6, false, 0.0), 7);
        assert_eq!(cleanup_progress_percent(6, 6, false, 0.0), 35);
    }

    #[test]
    fn dism_respeita_o_peso_da_limpeza_media() {
        assert_eq!(cleanup_progress_percent(5, 5, true, 0.0), 45);
        assert_eq!(cleanup_progress_percent(5, 5, true, 50.0), 70);
        assert_eq!(cleanup_progress_percent(5, 5, true, 100.0), 96);
    }

    #[test]
    fn dism_respeita_o_peso_da_limpeza_pesada() {
        assert_eq!(cleanup_progress_percent(6, 6, true, 0.0), 35);
        assert_eq!(cleanup_progress_percent(6, 6, true, 50.0), 65);
        assert_eq!(cleanup_progress_percent(6, 6, true, 100.0), 96);
    }

    #[test]
    fn progresso_do_dism_e_limitado_ao_intervalo_de_zero_a_cem() {
        assert_eq!(cleanup_progress_percent(5, 5, true, -20.0), 45);
        assert_eq!(cleanup_progress_percent(5, 5, true, 150.0), 96);
    }

    #[test]
    fn entradas_invalidas_nao_geram_progresso() {
        assert_eq!(cleanup_progress_percent(0, 5, false, 0.0), 0);
        assert_eq!(cleanup_progress_percent(1, 0, false, 0.0), 0);
        assert_eq!(cleanup_progress_percent(6, 5, false, 0.0), 0);
    }

    #[test]
    fn uma_unica_etapa_nao_divide_por_zero() {
        assert_eq!(cleanup_progress_percent(1, 1, false, 0.0), 0);
        assert_eq!(cleanup_progress_percent(1, 1, true, 100.0), 51);
    }
}
