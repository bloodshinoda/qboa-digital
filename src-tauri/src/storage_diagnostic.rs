use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const STORAGE_REPORT_SCHEMA_VERSION: u32 = 1;
const MAX_STORED_STORAGE_REPORT_BYTES: u64 = 4 * 1024 * 1024;
static NEXT_STORAGE_REPORT_ID: AtomicU64 = AtomicU64::new(0);

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
    pub predicted_failure: Option<bool>,
    pub reallocated: Option<u64>,
    pub pending: Option<u64>,
    pub uncorrectable: Option<u64>,
    pub interface_crc_before: Option<u64>,
    pub interface_crc_after: Option<u64>,
    pub wear_percent: Option<f64>,
    pub wear_limit_percent: Option<f64>,
    pub temperature_c: Option<f64>,
    pub pio_mode: Option<bool>,
    pub slow_sata_link: Option<bool>,
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

fn storage_reports_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("diagnostics").join("storage-reports")
}

pub(crate) fn timestamp_now() -> String {
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

fn safe_storage_report_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub(crate) fn persist_report(
    app_data_dir: &Path,
    report: &mut serde_json::Value,
    started_at: String,
) -> Result<(), String> {
    let object = report
        .as_object_mut()
        .ok_or_else(|| "O laudo de armazenamento precisa ser um objeto JSON.".to_string())?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = NEXT_STORAGE_REPORT_ID.fetch_add(1, Ordering::Relaxed);
    let report_id = format!(
        "storage-{nanos:032x}-{:08x}-{counter:08x}",
        std::process::id()
    );
    if !safe_storage_report_id(&report_id) {
        return Err("Não foi possível gerar o identificador do laudo de armazenamento.".into());
    }
    object.insert(
        "schema_version".into(),
        serde_json::json!(STORAGE_REPORT_SCHEMA_VERSION),
    );
    object.insert("report_id".into(), serde_json::json!(report_id));
    object.insert("started_at".into(), serde_json::json!(started_at));
    object.insert("finished_at".into(), serde_json::json!(timestamp_now()));

    let directory = storage_reports_dir(app_data_dir);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Não foi possível criar o histórico de armazenamento: {error}"))?;
    let destination = directory.join(format!("{}.json", report_id));
    let temporary = directory.join(format!(".{}.tmp", report_id));
    let bytes = serde_json::to_vec_pretty(report).map_err(|error| {
        format!("Não foi possível serializar o laudo de armazenamento: {error}")
    })?;
    let write_result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                format!("Não foi possível criar o arquivo temporário do laudo: {error}")
            })?;
        file.write_all(&bytes)
            .map_err(|error| format!("Falha ao gravar laudo de armazenamento: {error}"))?;
        file.flush()
            .map_err(|error| format!("Falha ao descarregar laudo de armazenamento: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Falha ao sincronizar laudo de armazenamento: {error}"))?;
        fs::rename(&temporary, &destination).map_err(|error| {
            format!("Não foi possível finalizar o laudo de armazenamento: {error}")
        })?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

pub(crate) fn load_stored_reports(app_data_dir: &Path) -> Result<serde_json::Value, String> {
    let directory = storage_reports_dir(app_data_dir);
    if !directory.exists() {
        return Ok(serde_json::Value::Array(Vec::new()));
    }
    let entries = fs::read_dir(&directory).map_err(|error| {
        format!("Não foi possível listar o histórico de armazenamento: {error}")
    })?;
    let mut reports = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let metadata = match fs::metadata(&path) {
            Ok(metadata) if metadata.len() <= MAX_STORED_STORAGE_REPORT_BYTES => metadata,
            _ => continue,
        };
        let _ = metadata;
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let report: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(report) => report,
            Err(_) => continue,
        };
        let report_id = report
            .get("report_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let schema = report
            .get("schema_version")
            .and_then(serde_json::Value::as_u64);
        if !safe_storage_report_id(report_id)
            || schema != Some(STORAGE_REPORT_SCHEMA_VERSION as u64)
        {
            continue;
        }
        reports.push(report);
    }
    reports.sort_by(|left, right| {
        let left_time = left
            .get("started_at")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let right_time = right
            .get("started_at")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        right_time.cmp(left_time)
    });
    Ok(serde_json::Value::Array(reports))
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
    let pio_mode = report.smart.pio_mode == Some(true);
    let slow_sata_link = report.smart.slow_sata_link == Some(true);

    if report.class == DiskClass::Unknown {
        reasons.push("Classe do disco não identificada pelo barramento; desempenho e recomendação inconclusivos.".into());
    } else {
        let read = value(report, MeasurementKind::SeqLargeRead);
        let write = value(report, MeasurementKind::SeqLargeWrite);
        if report.class != DiskClass::Usb {
            if pio_mode || slow_sata_link {
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

    let crc_increased = matches!(
        (
            report.smart.interface_crc_before,
            report.smart.interface_crc_after
        ),
        (Some(before), Some(after)) if after > before
    );

    if pio_mode || slow_sata_link || crc_increased {
        // Problemas de interface/transporte não devem condenar o disco pela velocidade.
        // SMART crítico abaixo ainda pode elevar a recomendação para Trocar.
        if recommendation == Recommendation::Trocar {
            recommendation = Recommendation::Reparar;
        } else {
            recommendation = recommendation.worst(Recommendation::Reparar);
        }
        if pio_mode {
            reasons.push("Modo PIO detectado: revisar cabo, porta ou driver.".into());
        }
        if slow_sata_link {
            reasons.push("Link SATA lento: revisar cabo, porta ou driver.".into());
        }
    }
    if report.class == DiskClass::Usb && report.smart.usb_slow {
        recommendation = recommendation.worst(Recommendation::Revisar);
    }

    if report.smart.usable {
        if report.smart.predicted_failure == Some(true) {
            recommendation = recommendation.worst(Recommendation::Trocar);
            reasons.push("O provedor SMART sinalizou previsão de falha do dispositivo.".into());
        }
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
        if crc_increased {
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
        // A falta de espaço invalida apenas o benchmark; não apaga achados SMART independentes.
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
