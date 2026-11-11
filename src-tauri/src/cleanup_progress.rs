// Cálculo puro do progresso agregado das etapas de limpeza.
//
// O percentual representa a operação inteira. Etapas simples consomem seu peso
// quando a próxima começa; DISM, ResetBase e SFC avançam dentro do próprio peso
// usando o percentual real informado pela ferramenta.
//
// Limpeza média (4 etapas): 25% por etapa.
// Limpeza pesada (8 etapas): 6% para cada etapa inicial, 35% para RestoreHealth,
// 15% para ResetBase e 20% para SFC. Os pesos somam 100%.

fn stage_weight(total: u32, index: u32) -> f64 {
    if total == 4 {
        return 25.0;
    }

    if total == 8 {
        return match index {
            1..=5 => 6.0,
            6 => 35.0,
            7 => 15.0,
            8 => 20.0,
            _ => 0.0,
        };
    }

    if total == 0 {
        0.0
    } else {
        100.0 / total as f64
    }
}

/// Calcula o percentual global com base nas etapas concluídas e no avanço da atual.
pub(crate) fn cleanup_progress_percent(current: u32, total: u32, stage_percent: f64) -> u32 {
    if total == 0 || current == 0 || current > total {
        return 0;
    }

    let completed: f64 = (1..current).map(|index| stage_weight(total, index)).sum();
    let current_weight = stage_weight(total, current);
    let current_progress = current_weight * stage_percent.clamp(0.0, 100.0) / 100.0;

    (completed + current_progress).clamp(0.0, 100.0).floor() as u32
}

/// Extrai o percentual de linhas de progresso do DISM/SFC sem interpretar
/// números de outras mensagens como avanço da ferramenta.
pub(crate) fn parse_tool_progress_percent(line: &str) -> Option<f64> {
    let percent_pos = line.find('%')?;
    let prefix = &line[..percent_pos];
    let number: String = prefix
        .chars()
        .rev()
        .take_while(|character| {
            character.is_ascii_digit() || *character == '.' || *character == ','
        })
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    if number.is_empty() {
        return None;
    }

    number
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .map(|value| value.clamp(0.0, 100.0))
}

#[cfg(test)]
mod tests {
    use super::{cleanup_progress_percent, parse_tool_progress_percent};

    #[test]
    fn limpeza_media_distribui_o_progresso_igualmente_entre_as_etapas() {
        assert_eq!(cleanup_progress_percent(1, 4, 0.0), 0);
        assert_eq!(cleanup_progress_percent(2, 4, 0.0), 25);
        assert_eq!(cleanup_progress_percent(3, 4, 0.0), 50);
        assert_eq!(cleanup_progress_percent(4, 4, 0.0), 75);
        assert_eq!(cleanup_progress_percent(4, 4, 50.0), 87);
        assert_eq!(cleanup_progress_percent(4, 4, 100.0), 100);
    }

    #[test]
    fn limpeza_pesada_mantem_o_percentual_global_linear_entre_dism_resetbase_e_sfc() {
        assert_eq!(cleanup_progress_percent(6, 8, 0.0), 30);
        assert_eq!(cleanup_progress_percent(6, 8, 50.0), 47);
        assert_eq!(cleanup_progress_percent(6, 8, 100.0), 65);
        assert_eq!(cleanup_progress_percent(7, 8, 0.0), 65);
        assert_eq!(cleanup_progress_percent(7, 8, 100.0), 80);
        assert_eq!(cleanup_progress_percent(8, 8, 0.0), 80);
        assert_eq!(cleanup_progress_percent(8, 8, 50.0), 90);
        assert_eq!(cleanup_progress_percent(8, 8, 100.0), 100);
    }

    #[test]
    fn etapas_simples_sao_consolidadas_quando_a_proxima_comeca() {
        assert_eq!(cleanup_progress_percent(5, 8, 0.0), 24);
        assert_eq!(cleanup_progress_percent(6, 8, 0.0), 30);
    }

    #[test]
    fn etapas_de_tarefas_sem_pesos_especificos_dividem_o_total() {
        assert_eq!(cleanup_progress_percent(2, 5, 0.0), 20);
        assert_eq!(cleanup_progress_percent(5, 5, 50.0), 90);
    }

    #[test]
    fn progresso_global_nao_regride_ao_trocar_de_etapa() {
        let stage_percentages = [
            (1, 8, 100.0),
            (2, 8, 100.0),
            (3, 8, 100.0),
            (4, 8, 100.0),
            (5, 8, 100.0),
            (6, 8, 100.0),
            (7, 8, 100.0),
            (8, 8, 100.0),
        ];
        let values = stage_percentages
            .into_iter()
            .map(|(current, total, local)| cleanup_progress_percent(current, total, local))
            .collect::<Vec<_>>();

        assert!(values.windows(2).all(|pair| pair[1] >= pair[0]));
        assert_eq!(values.last(), Some(&100));
    }

    #[test]
    fn percentual_da_etapa_e_limitado_ao_intervalo_de_zero_a_cem() {
        assert_eq!(cleanup_progress_percent(6, 8, -20.0), 30);
        assert_eq!(cleanup_progress_percent(6, 8, 150.0), 65);
    }

    #[test]
    fn entradas_invalidas_nao_geram_progresso() {
        assert_eq!(cleanup_progress_percent(0, 5, 0.0), 0);
        assert_eq!(cleanup_progress_percent(1, 0, 0.0), 0);
        assert_eq!(cleanup_progress_percent(6, 5, 0.0), 0);
    }

    #[test]
    fn interpreta_percentual_real_de_dism_e_sfc() {
        assert_eq!(parse_tool_progress_percent(" 45.7%"), Some(45.7));
        assert_eq!(
            parse_tool_progress_percent("Verification 82% complete."),
            Some(82.0)
        );
        assert_eq!(
            parse_tool_progress_percent("Progresso 12,5% concluído"),
            Some(12.5)
        );
        assert_eq!(
            parse_tool_progress_percent("Nenhum percentual disponível"),
            None
        );
    }
}
