use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiskClass {
    NotebookHdd,
    DesktopHdd,
    SataSsd,
    Nvme,
    Usb,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Recommendation {
    Ok,
    Inconclusivo,
    Revisar,
    Reparar,
    Trocar,
}

impl Recommendation {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Revisar => "Revisar",
            Self::Reparar => "Reparar",
            Self::Trocar => "Trocar",
            Self::Inconclusivo => "Inconclusivo",
        }
    }
    fn severity(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Inconclusivo => 1,
            Self::Revisar => 2,
            Self::Reparar => 3,
            Self::Trocar => 4,
        }
    }
    fn worst(self, other: Self) -> Self {
        if self.severity() >= other.severity() {
            self
        } else {
            other
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MeasurementKind {
    SeqSmallWrite,
    SeqSmallRead,
    SeqLargeWrite,
    SeqLargeRead,
    RandomSmallWrite,
    RandomSmallRead,
    RandomLargeWrite,
    RandomLargeRead,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct Measurement {
    pub kind: MeasurementKind,
    pub value: Option<f64>,
    pub unit: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Default)]
pub(crate) struct SmartData {
    pub usable: bool,
    pub reallocated: Option<u64>,
    pub pending: Option<u64>,
    pub uncorrectable: Option<u64>,
    pub interface_crc_before: Option<u64>,
    pub interface_crc_after: Option<u64>,
    pub wear_percent: Option<f64>,
    pub wear_limit_percent: Option<f64>,
    pub temperature_c: Option<f64>,
    pub pio_mode: bool,
    pub slow_sata_link: bool,
    pub usb_slow: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct StorageReport {
    pub disk_id: String,
    pub class: DiskClass,
    pub measurements: Vec<Measurement>,
    pub smart: SmartData,
    pub recommendation: Recommendation,
    pub reasons: Vec<String>,
    pub execution_status: String,
    pub space_available: bool,
}

fn threshold(class: DiskClass) -> Option<(f64, f64)> {
    match class {
        DiskClass::NotebookHdd => Some((60.0, 40.0)),
        DiskClass::DesktopHdd => Some((120.0, 70.0)),
        DiskClass::SataSsd => Some((400.0, 150.0)),
        DiskClass::Nvme => Some((1500.0, 400.0)),
        DiskClass::Usb | DiskClass::Unknown => None,
    }
}

fn value(report: &StorageReport, kind: MeasurementKind) -> Option<f64> {
    report
        .measurements
        .iter()
        .find(|measurement| measurement.kind == kind)
        .and_then(|measurement| measurement.value)
        .filter(|value| value.is_finite() && *value >= 0.0)
}

pub(crate) fn evaluate(report: &mut StorageReport) {
    let mut recommendation = Recommendation::Inconclusivo;
    let mut reasons = Vec::new();

    if report.class == DiskClass::Unknown {
        reasons.push("Classe do disco não identificada pelo barramento; desempenho e recomendação inconclusivos.".into());
    } else {
        let read = value(report, MeasurementKind::SeqLargeRead);
        let write = value(report, MeasurementKind::SeqLargeWrite);
        if report.class != DiskClass::Usb {
            if report.smart.pio_mode || report.smart.slow_sata_link {
                recommendation = Recommendation::Reparar;
                reasons.push(
                    "A condição do link/controlador impede usar a velocidade como motivo de troca."
                        .into(),
                );
            } else if let (Some((good, replace_floor)), Some(read), Some(write)) =
                (threshold(report.class), read, write)
            {
                let read_bad = read < good;
                let write_bad = write < good;
                if read_bad && write_bad && read < replace_floor && write < replace_floor {
                    recommendation = Recommendation::Trocar;
                    reasons.push("Leitura e escrita sequenciais grandes ficaram abaixo do piso de troca da classe.".into());
                } else if read_bad || write_bad {
                    recommendation = Recommendation::Revisar;
                    reasons.push(format!("Desempenho sequencial divergente ou abaixo da faixa da classe (leitura: {read:.1} MB/s; escrita: {write:.1} MB/s)."));
                } else {
                    recommendation = Recommendation::Ok;
                    reasons.push("Leitura e escrita sequenciais grandes atingiram a faixa esperada da classe.".into());
                }
            } else {
                reasons.push("Medições sequenciais grandes incompletas; avaliação de velocidade inconclusiva.".into());
            }
        } else {
            recommendation = Recommendation::Revisar;
            reasons.push(
                "Disco conectado por USB: desempenho não é usado para condenar o disco.".into(),
            );
            if report.smart.usb_slow {
                reasons.push("USB lento: revisar cabo ou gabinete.".into());
            }
        }
    }

    if report.smart.pio_mode || report.smart.slow_sata_link {
        recommendation = recommendation.worst(Recommendation::Reparar);
        reasons.push(if report.smart.pio_mode {
            "Modo PIO detectado: revisar cabo, porta ou driver.".into()
        } else {
            "Link SATA lento: revisar cabo, porta ou driver.".into()
        });
    }
    if report.class == DiskClass::Usb && report.smart.usb_slow {
        recommendation = recommendation.worst(Recommendation::Revisar);
    }

    if report.smart.usable {
        for (label, count) in [
            ("realocados", report.smart.reallocated),
            ("pendentes", report.smart.pending),
            ("incorrigíveis", report.smart.uncorrectable),
        ] {
            if count.unwrap_or(0) > 0 {
                recommendation = recommendation.worst(Recommendation::Trocar);
                reasons.push(format!("SMART informa {count:?} setores {label}."));
            } else if count.is_none() {
                reasons.push(format!("Contagem SMART de setores {label} indisponível."));
            }
        }
        if matches!(
            (
                report.smart.interface_crc_before,
                report.smart.interface_crc_after
            ),
            (Some(before), Some(after)) if after > before
        ) {
            recommendation = recommendation.worst(Recommendation::Reparar);
            reasons.push(
                "Contador CRC da interface aumentou durante o teste; revisar cabo ou porta.".into(),
            );
        }
        if let (Some(wear), Some(limit)) =
            (report.smart.wear_percent, report.smart.wear_limit_percent)
        {
            if wear.is_finite() && limit.is_finite() && wear >= limit {
                recommendation = recommendation.worst(Recommendation::Trocar);
                reasons
                    .push("Desgaste atingiu ou ultrapassou o limite informado pelo disco.".into());
            }
        } else {
            reasons.push("Dados de desgaste SMART incompletos.".into());
        }
        if report.smart.temperature_c.is_none() {
            reasons.push("Temperatura SMART indisponível; não foi presumida como zero.".into());
        }
    } else {
        reasons.push("SMART utilizável indisponível; estado SMART inconclusivo.".into());
    }

    if !report.space_available {
        recommendation = Recommendation::Inconclusivo;
        reasons.push(
            "Espaço livre insuficiente para iniciar o teste; nenhuma velocidade foi presumida."
                .into(),
        );
    }
    report.recommendation = recommendation;
    report.reasons = reasons;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(class: DiskClass, read: Option<f64>, write: Option<f64>) -> StorageReport {
        let mut report = StorageReport {
            disk_id: "fake-disk-0".into(),
            class,
            measurements: vec![
                Measurement {
                    kind: MeasurementKind::SeqLargeRead,
                    value: read,
                    unit: "MB/s".into(),
                },
                Measurement {
                    kind: MeasurementKind::SeqLargeWrite,
                    value: write,
                    unit: "MB/s".into(),
                },
            ],
            smart: SmartData {
                usable: true,
                reallocated: Some(0),
                pending: Some(0),
                uncorrectable: Some(0),
                interface_crc_before: Some(0),
                interface_crc_after: Some(0),
                wear_percent: Some(1.0),
                wear_limit_percent: Some(100.0),
                temperature_c: Some(35.0),
                ..SmartData::default()
            },
            recommendation: Recommendation::Inconclusivo,
            reasons: vec![],
            execution_status: "completed".into(),
            space_available: true,
        };
        evaluate(&mut report);
        report
    }

    #[test]
    fn notebook_hdd_100_mbps_is_ok() {
        assert_eq!(
            sample(DiskClass::NotebookHdd, Some(100.0), Some(100.0)).recommendation,
            Recommendation::Ok
        );
    }
    #[test]
    fn usb_slow_never_means_replace() {
        let mut report = sample(DiskClass::Usb, Some(1.0), Some(1.0));
        report.smart.usb_slow = true;
        evaluate(&mut report);
        assert_ne!(report.recommendation, Recommendation::Trocar);
    }
    #[test]
    fn unknown_class_is_inconclusive() {
        assert_eq!(
            sample(DiskClass::Unknown, Some(5000.0), Some(5000.0)).recommendation,
            Recommendation::Inconclusivo
        );
    }
    #[test]
    fn one_slow_sequence_is_not_replace() {
        assert_ne!(
            sample(DiskClass::SataSsd, Some(100.0), Some(500.0)).recommendation,
            Recommendation::Trocar
        );
    }
    #[test]
    fn smart_pending_overrides_good_speed() {
        let mut report = sample(DiskClass::Nvme, Some(3000.0), Some(3000.0));
        report.smart.pending = Some(1);
        evaluate(&mut report);
        assert_eq!(report.recommendation, Recommendation::Trocar);
    }
    #[test]
    fn crc_increase_repairs_not_replaces() {
        let mut report = sample(DiskClass::SataSsd, Some(10.0), Some(10.0));
        report.smart.interface_crc_after = Some(1);
        evaluate(&mut report);
        assert_eq!(report.recommendation, Recommendation::Reparar);
    }
    #[test]
    fn missing_speed_is_not_zero() {
        assert_eq!(
            sample(DiskClass::Nvme, None, None).recommendation,
            Recommendation::Inconclusivo
        );
    }
}
