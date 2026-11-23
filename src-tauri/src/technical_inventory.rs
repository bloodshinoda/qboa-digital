//! Modelos, serialização e validação do Inventário Técnico Completo.
//!
//! Este módulo não coleta dados. Os coletores preencherão estes contratos em
//! etapas posteriores. Valores desconhecidos permanecem explícitos por meio de
//! Observed<T>; nunca são convertidos em zero ou string vazia.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InventoryValueState {
    Available,
    NotDetected,
    NotApplicable,
    Unavailable,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InventoryConfidence {
    Confirmed,
    Identified,
    Estimated,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InventorySource {
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Observed<T> {
    pub value: Option<T>,
    pub state: InventoryValueState,
    pub confidence: InventoryConfidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<InventorySource>,
    pub collected_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl<T> Observed<T> {
    pub fn validate(&self) -> Result<(), String> {
        let has_value = self.value.is_some();
        match self.state {
            InventoryValueState::Available if !has_value => {
                return Err("estado available exige value".into())
            }
            InventoryValueState::NotDetected
            | InventoryValueState::NotApplicable
            | InventoryValueState::Unavailable
            | InventoryValueState::Error
                if has_value =>
            {
                return Err(format!("estado {:?} não pode carregar value", self.state))
            }
            _ => {}
        }
        if self.state == InventoryValueState::Unavailable
            && self.confidence != InventoryConfidence::Unavailable
        {
            return Err("estado unavailable exige confiança unavailable".into());
        }
        if self.confidence == InventoryConfidence::Unavailable
            && self.state != InventoryValueState::Unavailable
        {
            return Err("confiança unavailable exige estado unavailable".into());
        }
        if self.confidence == InventoryConfidence::Estimated
            && self.note.as_deref().unwrap_or("").trim().is_empty()
        {
            return Err("valor estimado exige observação explicativa".into());
        }
        if self.collected_at.trim().is_empty() {
            return Err("collected_at não pode ser vazio".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InventoryIssue {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<InventorySource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct CategoryCollection<T> {
    pub state: InventoryValueState,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(default)]
    pub issues: Vec<InventoryIssue>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl<T: Serialize> CategoryCollection<T> {
    pub fn validate(&self) -> Result<(), String> {
        if self.started_at.trim().is_empty() || self.finished_at.trim().is_empty() {
            return Err("timestamps da categoria não podem ser vazios".into());
        }
        match self.state {
            InventoryValueState::Available if self.data.is_none() => {
                return Err("categoria available exige data".into())
            }
            InventoryValueState::NotDetected
            | InventoryValueState::NotApplicable
            | InventoryValueState::Unavailable
                if self.data.is_some() =>
            {
                return Err(format!("categoria {:?} não pode carregar data", self.state))
            }
            InventoryValueState::Error if self.issues.is_empty() => {
                return Err("categoria error exige ao menos um issue".into())
            }
            _ => {}
        }
        if let Some(data) = &self.data {
            let value = serde_json::to_value(data)
                .map_err(|e| format!("falha ao validar dados da categoria: {e}"))?;
            validate_observed_tree(&value)?;
        }
        Ok(())
    }
}

fn validate_observed_tree(value: &serde_json::Value) -> Result<(), String> {
    match value {
        serde_json::Value::Object(map) => {
            if map.contains_key("state")
                && map.contains_key("confidence")
                && map.contains_key("collected_at")
                && map.contains_key("value")
            {
                let state = map.get("state").and_then(|v| v.as_str()).unwrap_or("");
                let confidence = map.get("confidence").and_then(|v| v.as_str()).unwrap_or("");
                let has_value = !map
                    .get("value")
                    .unwrap_or(&serde_json::Value::Null)
                    .is_null();
                if (state == "available") != has_value {
                    return Err(format!(
                        "Observed inconsistente: state={state} e presença de value incompatíveis"
                    ));
                }
                if state == "unavailable" && confidence != "unavailable" {
                    return Err("Observed unavailable exige confidence unavailable".into());
                }
                if confidence == "unavailable" && state != "unavailable" {
                    return Err("Observed confidence unavailable exige state unavailable".into());
                }
                if confidence == "estimated"
                    && map
                        .get("note")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .is_empty()
                {
                    return Err("Observed estimated exige note".into());
                }
                if map
                    .get("collected_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .is_empty()
                {
                    return Err("Observed collected_at não pode ser vazio".into());
                }
            }
            for child in map.values() {
                validate_observed_tree(child)?;
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                validate_observed_tree(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InventoryRunStatus {
    Completed,
    Partial,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Desktop,
    Laptop,
    Tablet,
    Workstation,
    Server,
    VirtualMachine,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OsArchitecture {
    X86,
    X64,
    Arm64,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct MachineIdentity {
    pub computer_name: Observed<String>,
    pub manufacturer: Observed<String>,
    pub model: Observed<String>,
    pub device_kind: Observed<DeviceKind>,
    pub is_virtual_machine: Observed<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct InventoryCategories {
    pub system: CategoryCollection<SystemCategory>,
    pub firmware: CategoryCollection<FirmwareCategory>,
    pub cpu: CategoryCollection<CpuCategory>,
    pub memory: CategoryCollection<MemoryCategory>,
    pub gpu: CategoryCollection<GpuCategory>,
    pub storage: CategoryCollection<StorageCategory>,
    pub displays: CategoryCollection<DisplayCategory>,
    pub network: CategoryCollection<NetworkCategory>,
    pub audio_peripherals: CategoryCollection<AudioPeripheralCategory>,
    pub power: CategoryCollection<PowerCategory>,
    pub software: CategoryCollection<SoftwareCategory>,
    pub drivers: CategoryCollection<DriverCategory>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct TechnicalInventory {
    pub schema_version: u32,
    pub inventory_id: String,
    pub app_version: String,
    pub machine: MachineIdentity,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    pub collection_status: InventoryRunStatus,
    pub categories: InventoryCategories,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl TechnicalInventory {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(format!(
                "schema_version {} não suportado (atual: {})",
                self.schema_version, CURRENT_SCHEMA_VERSION
            ));
        }
        if !is_uuid(&self.inventory_id) {
            return Err("inventory_id deve ser UUID válido".into());
        }
        if self.app_version.trim().is_empty() {
            return Err("app_version não pode ser vazia".into());
        }
        if !is_rfc3339_utc(&self.started_at) || !is_rfc3339_utc(&self.finished_at) {
            return Err("timestamps devem ser RFC 3339 em UTC".into());
        }
        if self.finished_at < self.started_at {
            return Err("finished_at antecede started_at".into());
        }

        self.machine.computer_name.validate()?;
        self.machine.manufacturer.validate()?;
        self.machine.model.validate()?;
        self.machine.device_kind.validate()?;
        self.machine.is_virtual_machine.validate()?;

        self.categories.system.validate()?;
        self.categories.firmware.validate()?;
        if self.categories.firmware.state == InventoryValueState::Available {
            self.categories
                .firmware
                .data
                .as_ref()
                .ok_or_else(|| "categoria Placa-mãe e Firmware available exige data".to_string())?
                .validate()?;
        }
        self.categories.cpu.validate()?;
        if self.categories.cpu.state == InventoryValueState::Available {
            self.categories
                .cpu
                .data
                .as_ref()
                .ok_or_else(|| "categoria CPU available exige data".to_string())?
                .validate()?;
        }
        self.categories.memory.validate()?;
        self.categories.gpu.validate()?;
        self.categories.storage.validate()?;
        self.categories.displays.validate()?;
        self.categories.network.validate()?;
        self.categories.audio_peripherals.validate()?;
        self.categories.power.validate()?;
        self.categories.software.validate()?;
        self.categories.drivers.validate()?;

        let states = [
            &self.categories.system.state,
            &self.categories.firmware.state,
            &self.categories.cpu.state,
            &self.categories.memory.state,
            &self.categories.gpu.state,
            &self.categories.storage.state,
            &self.categories.displays.state,
            &self.categories.network.state,
            &self.categories.audio_peripherals.state,
            &self.categories.power.state,
            &self.categories.software.state,
            &self.categories.drivers.state,
        ];
        let failed = states.iter().any(|s| **s == InventoryValueState::Error);
        let processed = states.iter().any(|s| {
            matches!(
                **s,
                InventoryValueState::Available
                    | InventoryValueState::NotDetected
                    | InventoryValueState::NotApplicable
                    | InventoryValueState::Unavailable
            )
        });
        match self.collection_status {
            InventoryRunStatus::Completed if failed => {
                return Err("completed não pode conter categoria em error".into())
            }
            InventoryRunStatus::Partial if !failed || !processed => {
                return Err("partial exige sucesso e ao menos uma falha".into())
            }
            InventoryRunStatus::Failed if processed => {
                return Err("failed não pode conter categorias processadas".into())
            }
            _ => {}
        }
        Ok(())
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let inventory: Self =
            serde_json::from_str(json).map_err(|e| format!("JSON de inventário inválido: {e}"))?;
        inventory.validate()?;
        Ok(inventory)
    }

    pub fn to_json_pretty(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map_err(|e| format!("Falha ao serializar inventário: {e}"))
    }
}

fn is_uuid(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 36
        && [8, 13, 18, 23].iter().all(|i| b[*i] == b'-')
        && b.iter()
            .enumerate()
            .all(|(i, c)| [8, 13, 18, 23].contains(&i) || c.is_ascii_hexdigit())
}

fn is_rfc3339_utc(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() >= 20
        && b[4] == b'-'
        && b[7] == b'-'
        && (b[10] == b'T' || b[10] == b't')
        && b[13] == b':'
        && b[16] == b':'
        && value.ends_with('Z')
        && b[..19].iter().enumerate().all(|(i, c)| {
            if [4, 7].contains(&i) {
                *c == b'-'
            } else if i == 10 {
                *c == b'T' || *c == b't'
            } else if [13, 16].contains(&i) {
                *c == b':'
            } else {
                c.is_ascii_digit()
            }
        })
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveDataKind {
    HardwareSerial,
    NetworkMac,
    PnpInstanceId,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Sensitive<T> {
    pub value: T,
    pub classification: SensitiveDataKind,
}

impl<T> Sensitive<T> {
    pub fn expose(&self, include_sensitive_identifiers: bool) -> Option<&T> {
        include_sensitive_identifiers.then_some(&self.value)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InventoryExportOptions {
    #[serde(default)]
    pub include_sensitive_identifiers: bool,
}

impl Default for InventoryExportOptions {
    fn default() -> Self {
        Self {
            include_sensitive_identifiers: false,
        }
    }
}

// Categoria 1 — sistema e equipamento
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SystemCategory {
    pub os_name: Observed<String>,
    pub os_edition: Observed<String>,
    pub os_version: Observed<String>,
    pub display_version: Observed<String>,
    pub build_number: Observed<u32>,
    pub ubr: Observed<u32>,
    pub architecture: Observed<OsArchitecture>,
    pub ui_language: Observed<String>,
    pub system_locale: Observed<String>,
    pub install_date: Observed<String>,
    pub boot_time: Observed<String>,
    pub computer_name: Observed<String>,
    pub domain_or_workgroup: Observed<String>,
    pub device_kind: Observed<DeviceKind>,
}

// Categoria 2 — placa-mãe e firmware
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FirmwareCategory {
    pub motherboard: MotherboardInfo,
    pub bios: BiosInfo,
    pub tpm: TpmInfo,
    pub secure_boot_enabled: Observed<bool>,
}

impl FirmwareCategory {
    pub fn validate(&self) -> Result<(), String> {
        self.motherboard.validate()?;
        self.bios.vendor.validate()?;
        self.bios.version.validate()?;
        self.bios.release_date.validate()?;
        self.bios.mode.validate()?;
        self.tpm.present.validate()?;
        self.tpm.ready.validate()?;
        self.tpm.enabled.validate()?;
        self.tpm.activated.validate()?;
        self.tpm.spec_version.validate()?;
        self.tpm.manufacturer.validate()?;
        self.secure_boot_enabled.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct MotherboardInfo {
    /// Marca responsável pela comercialização do produto; não inferir do SMBIOS.
    pub commercial_brand: Observed<String>,
    /// Fabricante declarado pelo firmware na estrutura SMBIOS Type 2.
    pub smbios_manufacturer: Observed<String>,
    /// Fabricante industrial/OEM, somente quando identificado por fonte confiável.
    pub oem_manufacturer: Observed<String>,
    pub product: Observed<String>,
    pub version: Observed<String>,
    pub serial_number: Observed<Sensitive<String>>,
}

impl MotherboardInfo {
    pub fn validate(&self) -> Result<(), String> {
        self.commercial_brand.validate()?;
        self.smbios_manufacturer.validate()?;
        self.oem_manufacturer.validate()?;
        self.product.validate()?;
        self.version.validate()?;
        self.serial_number.validate()?;
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BiosInfo {
    pub vendor: Observed<String>,
    pub version: Observed<String>,
    pub release_date: Observed<String>,
    pub mode: Observed<FirmwareMode>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareMode {
    Uefi,
    Legacy,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TpmInfo {
    pub present: Observed<bool>,
    pub ready: Observed<bool>,
    pub enabled: Observed<bool>,
    pub activated: Observed<bool>,
    pub spec_version: Observed<String>,
    pub manufacturer: Observed<String>,
}

// Categoria 3 — CPU
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CpuCategory {
    /// Uma entrada por pacote/socket físico identificado.
    pub processors: Vec<ProcessorInfo>,
}

impl CpuCategory {
    pub fn validate(&self) -> Result<(), String> {
        if self.processors.is_empty() {
            return Err("categoria CPU available exige ao menos um processador".into());
        }
        for (index, processor) in self.processors.iter().enumerate() {
            processor
                .validate()
                .map_err(|error| format!("processadores[{index}]: {error}"))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProcessorInfo {
    pub name: Observed<String>,
    pub manufacturer: Observed<String>,
    pub processor_id: Observed<String>,
    pub socket: Observed<String>,
    pub architecture: Observed<CpuArchitecture>,
    pub physical_cores: Observed<u32>,
    pub logical_processors: Observed<u32>,
    /// Frequência máxima reportada pelo provedor, não necessariamente o boost comercial.
    pub max_clock_mhz: Observed<u64>,
    pub l1_cache_bytes: Observed<u64>,
    pub l2_cache_bytes: Observed<u64>,
    pub l3_cache_bytes: Observed<u64>,
    pub virtualization_supported: Observed<bool>,
    pub virtualization_firmware_enabled: Observed<bool>,
    /// Recursos de instrução anunciados pelo hardware.
    pub instruction_sets: Observed<Vec<CpuInstructionSet>>,
}

impl ProcessorInfo {
    pub fn validate(&self) -> Result<(), String> {
        self.name.validate()?;
        self.manufacturer.validate()?;
        self.processor_id.validate()?;
        self.socket.validate()?;
        self.architecture.validate()?;
        self.physical_cores.validate()?;
        self.logical_processors.validate()?;
        self.max_clock_mhz.validate()?;
        self.l1_cache_bytes.validate()?;
        self.l2_cache_bytes.validate()?;
        self.l3_cache_bytes.validate()?;
        self.virtualization_supported.validate()?;
        self.virtualization_firmware_enabled.validate()?;
        self.instruction_sets.validate()?;

        if let (Some(physical), Some(logical)) =
            (self.physical_cores.value, self.logical_processors.value)
        {
            if logical < physical {
                return Err(
                    "processadores lógicos não podem ser inferiores aos núcleos físicos".into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CpuArchitecture {
    X86,
    X64,
    Arm,
    Arm64,
    Itanium,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CpuInstructionSet {
    Mmx,
    Sse,
    Sse2,
    Sse3,
    Ssse3,
    Sse41,
    Sse42,
    Aes,
    Avx,
    Avx2,
    Fma,
    F16c,
    Bmi1,
    Bmi2,
    Avx512F,
    Sha,
    Neon,
    Sve,
    Sve2,
}

// Categoria 4 — RAM
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct MemoryCategory {
    pub total_physical_bytes: Observed<u64>,
    pub available_physical_bytes: Observed<u64>,
    pub total_virtual_bytes: Observed<u64>,
    pub available_virtual_bytes: Observed<u64>,
    pub reported_slot_count: Observed<u32>,
    pub modules: Vec<MemoryModuleInfo>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct MemoryModuleInfo {
    pub locator: Observed<String>,
    pub bank_label: Observed<String>,
    pub capacity_bytes: Observed<u64>,
    pub manufacturer: Observed<String>,
    pub part_number: Observed<String>,
    pub memory_type: Observed<MemoryTechnology>,
    pub form_factor: Observed<MemoryFormFactor>,
    pub configured_speed_mhz: Observed<u64>,
    pub smbios_speed_mhz: Observed<u64>,
    pub data_width_bits: Observed<u32>,
    pub total_width_bits: Observed<u32>,
    pub configured_voltage_mv: Observed<u32>,
    pub cas_latency: Observed<u32>,
    pub xmp_expo_max_speed_mhz: Observed<u64>,
    pub raw_smbios_memory_type: Observed<u16>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTechnology {
    Ddr,
    Ddr2,
    Ddr3,
    Ddr4,
    Ddr5,
    Lpddr3,
    Lpddr4,
    Lpddr4x,
    Lpddr5,
    Lpddr5x,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFormFactor {
    Dimm,
    Sodimm,
    MicroDimm,
    MiniDimm,
    Onboard,
    RowOfChips,
    Unknown,
}

// Categoria 5 — GPU
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GpuCategory {
    pub adapters: Vec<GpuAdapterInfo>,
    pub graphics_apis: Vec<GraphicsApiInfo>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GpuAdapterInfo {
    pub name: Observed<String>,
    pub vendor: Observed<String>,
    pub device_id: Observed<String>,
    pub pnp_device_id: Observed<Sensitive<String>>,
    pub adapter_kind: Observed<GpuAdapterKind>,
    pub driver_provider: Observed<String>,
    pub driver_version: Observed<String>,
    pub driver_date: Observed<String>,
    pub dedicated_memory_bytes: Observed<u64>,
    pub shared_memory_bytes: Observed<u64>,
    pub is_primary: Observed<bool>,
    pub active_outputs: Observed<u32>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GpuAdapterKind {
    Integrated,
    Dedicated,
    Virtual,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GraphicsApiInfo {
    pub api: String,
    pub version: Observed<String>,
    pub feature_level: Observed<String>,
}

// Categoria 6 — armazenamento
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct StorageCategory {
    pub physical_disks: Vec<PhysicalDiskInfo>,
    pub volumes: Vec<VolumeInfo>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PhysicalDiskInfo {
    pub model: Observed<String>,
    pub manufacturer: Observed<String>,
    pub serial_number: Observed<Sensitive<String>>,
    pub firmware_revision: Observed<String>,
    pub interface_type: Observed<String>,
    pub media_type: Observed<StorageMediaType>,
    pub bus_type: Observed<String>,
    pub capacity_bytes: Observed<u64>,
    pub logical_sector_bytes: Observed<u32>,
    pub physical_sector_bytes: Observed<u32>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StorageMediaType {
    Hdd,
    Ssd,
    Scm,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct VolumeInfo {
    pub volume_id: Observed<String>,
    pub drive_letter: Observed<String>,
    pub mount_points: Vec<String>,
    pub label: Observed<String>,
    pub file_system: Observed<String>,
    pub capacity_bytes: Observed<u64>,
    pub free_bytes: Observed<u64>,
    pub associated_disk_ids: Vec<String>,
}

// Categoria 7 — monitores
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DisplayCategory {
    pub displays: Vec<DisplayInfo>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DisplayInfo {
    pub manufacturer: Observed<String>,
    pub model: Observed<String>,
    pub friendly_name: Observed<String>,
    pub edid_product_code: Observed<String>,
    pub serial_number: Observed<Sensitive<String>>,
    pub connection_type: Observed<String>,
    pub connection_state: Observed<DisplayConnectionState>,
    pub is_primary: Observed<bool>,
    pub current_mode: Observed<DisplayMode>,
    pub native_mode: Observed<DisplayMode>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayConnectionState {
    Connected,
    Disconnected,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DisplayMode {
    pub width_px: u32,
    pub height_px: u32,
    pub refresh_rate_millihz: u32,
    pub bits_per_color: Option<u8>,
}

// Categoria 8 — rede
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct NetworkCategory {
    pub adapters: Vec<NetworkAdapterInfo>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct NetworkAdapterInfo {
    pub name: Observed<String>,
    pub description: Observed<String>,
    pub manufacturer: Observed<String>,
    pub interface_kind: Observed<String>,
    pub is_physical: Observed<bool>,
    pub status: Observed<NetworkLinkState>,
    pub link_speed_bps: Observed<u64>,
    pub mac_address: Observed<Sensitive<String>>,
    pub driver_provider: Observed<String>,
    pub driver_version: Observed<String>,
    pub dhcp_enabled: Observed<bool>,
    pub ipv4: Vec<IpConfiguration>,
    pub ipv6: Vec<IpConfiguration>,
    pub gateways: Vec<IpAddr>,
    pub dns_servers: Vec<IpAddr>,
    pub interface_metric: Observed<u32>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkLinkState {
    Up,
    Down,
    Dormant,
    NotPresent,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct IpConfiguration {
    pub address: IpAddr,
    pub prefix_length: u8,
    pub is_temporary: Option<bool>,
}

// Categoria 9 — áudio e periféricos
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AudioPeripheralCategory {
    pub audio_devices: Vec<AudioDeviceInfo>,
    pub peripherals: Vec<PeripheralInfo>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    pub name: Observed<String>,
    pub manufacturer: Observed<String>,
    pub device_id: Observed<Sensitive<String>>,
    pub device_role: Observed<AudioDeviceRole>,
    pub is_default: Observed<bool>,
    pub state: Observed<String>,
    pub driver_version: Observed<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioDeviceRole {
    Input,
    Output,
    Controller,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PeripheralInfo {
    pub name: Observed<String>,
    pub manufacturer: Observed<String>,
    pub device_class: Observed<String>,
    pub bus_type: Observed<String>,
    pub connection_state: Observed<String>,
    pub driver_version: Observed<String>,
    pub is_present: Observed<bool>,
}

// Categoria 10 — bateria e energia
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PowerCategory {
    pub batteries: Vec<BatteryInfo>,
    pub ac_power_state: Observed<AcPowerState>,
    pub active_power_scheme: Observed<PowerSchemeInfo>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BatteryInfo {
    pub name: Observed<String>,
    pub manufacturer: Observed<String>,
    pub chemistry: Observed<String>,
    pub design_capacity_mwh: Observed<u64>,
    pub full_charge_capacity_mwh: Observed<u64>,
    pub remaining_capacity_mwh: Observed<u64>,
    pub charge_percent: Observed<f64>,
    pub cycle_count: Observed<u32>,
    pub status: Observed<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcPowerState {
    Online,
    Offline,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PowerSchemeInfo {
    pub name: Observed<String>,
    pub guid: Observed<String>,
}

// Categoria 11 — softwares
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SoftwareCategory {
    pub applications: Vec<InstalledApplicationInfo>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstalledApplicationInfo {
    pub display_name: Observed<String>,
    pub display_version: Observed<String>,
    pub publisher: Observed<String>,
    pub architecture: Observed<String>,
    pub install_date: Observed<String>,
    pub install_scope: Observed<InstallScope>,
    pub source_registry: Observed<String>,
    pub estimated_size_bytes: Observed<u64>,
    pub uninstall_entry_id: Observed<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallScope {
    Machine,
    User,
    Unknown,
}

// Categoria 12 — drivers
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DriverCategory {
    pub devices: Vec<DriverDeviceInfo>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DriverDeviceInfo {
    pub device_instance_id: Observed<Sensitive<String>>,
    pub friendly_name: Observed<String>,
    pub device_class: Observed<String>,
    pub manufacturer: Observed<String>,
    pub driver_provider: Observed<String>,
    pub driver_version: Observed<String>,
    pub driver_date: Observed<String>,
    pub inf_name: Observed<String>,
    pub problem_code: Observed<u32>,
    pub status: Observed<PnpDeviceState>,
    pub is_present: Observed<bool>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PnpDeviceState {
    Started,
    Disabled,
    Error,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed<T>(
        value: Option<T>,
        state: InventoryValueState,
        confidence: InventoryConfidence,
    ) -> Observed<T> {
        Observed {
            value,
            state,
            confidence,
            source: Some(InventorySource {
                provider: "test".into(),
                member: None,
                native_code: None,
            }),
            collected_at: "2026-11-16T12:00:00Z".into(),
            note: None,
        }
    }

    fn category<T>() -> CategoryCollection<T> {
        CategoryCollection {
            state: InventoryValueState::NotApplicable,
            started_at: "2026-11-16T12:00:00Z".into(),
            finished_at: "2026-11-16T12:00:00Z".into(),
            duration_ms: 0,
            data: None,
            issues: vec![],
            warnings: vec![],
        }
    }

    fn mark_failed<T>(item: &mut CategoryCollection<T>) {
        item.state = InventoryValueState::Error;
        item.issues.push(InventoryIssue {
            code: "orchestration_failed".into(),
            message: "Falha simulada".into(),
            source: None,
            native_code: None,
        });
    }

    fn sample(status: InventoryRunStatus) -> TechnicalInventory {
        let mut system: CategoryCollection<SystemCategory> = category();
        let mut firmware: CategoryCollection<FirmwareCategory> = category();
        let mut cpu: CategoryCollection<CpuCategory> = category();
        let mut memory: CategoryCollection<MemoryCategory> = category();
        let mut gpu: CategoryCollection<GpuCategory> = category();
        let mut storage: CategoryCollection<StorageCategory> = category();
        let mut displays: CategoryCollection<DisplayCategory> = category();
        let mut network: CategoryCollection<NetworkCategory> = category();
        let mut audio_peripherals: CategoryCollection<AudioPeripheralCategory> = category();
        let mut power: CategoryCollection<PowerCategory> = category();
        let mut software: CategoryCollection<SoftwareCategory> = category();
        let mut drivers: CategoryCollection<DriverCategory> = category();

        if status == InventoryRunStatus::Completed {
            system.state = InventoryValueState::Available;
            system.data = Some(SystemCategory {
                os_name: observed(
                    Some("Windows".into()),
                    InventoryValueState::Available,
                    InventoryConfidence::Confirmed,
                ),
                os_edition: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                os_version: observed(
                    Some("10.0".into()),
                    InventoryValueState::Available,
                    InventoryConfidence::Identified,
                ),
                display_version: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                build_number: observed(
                    Some(26100),
                    InventoryValueState::Available,
                    InventoryConfidence::Confirmed,
                ),
                ubr: observed(
                    Some(1),
                    InventoryValueState::Available,
                    InventoryConfidence::Confirmed,
                ),
                architecture: observed(
                    Some(OsArchitecture::X64),
                    InventoryValueState::Available,
                    InventoryConfidence::Confirmed,
                ),
                ui_language: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                system_locale: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                install_date: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                boot_time: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                computer_name: observed(
                    Some("QBOA-TEST".into()),
                    InventoryValueState::Available,
                    InventoryConfidence::Confirmed,
                ),
                domain_or_workgroup: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                device_kind: observed(
                    Some(DeviceKind::Desktop),
                    InventoryValueState::Available,
                    InventoryConfidence::Identified,
                ),
            });
        } else if status == InventoryRunStatus::Partial {
            system.state = InventoryValueState::Available;
            system.data = Some(SystemCategory {
                os_name: observed(
                    Some("Windows".into()),
                    InventoryValueState::Available,
                    InventoryConfidence::Confirmed,
                ),
                os_edition: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                os_version: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                display_version: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                build_number: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                ubr: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                architecture: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                ui_language: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                system_locale: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                install_date: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                boot_time: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                computer_name: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                domain_or_workgroup: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                device_kind: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
            });
            firmware.state = InventoryValueState::Error;
            firmware.issues.push(InventoryIssue {
                code: "query_failed".into(),
                message: "Consulta simulada falhou".into(),
                source: None,
                native_code: None,
            });
        } else {
            mark_failed(&mut system);
            mark_failed(&mut firmware);
            mark_failed(&mut cpu);
            mark_failed(&mut memory);
            mark_failed(&mut gpu);
            mark_failed(&mut storage);
            mark_failed(&mut displays);
            mark_failed(&mut network);
            mark_failed(&mut audio_peripherals);
            mark_failed(&mut power);
            mark_failed(&mut software);
            mark_failed(&mut drivers);
        }

        TechnicalInventory {
            schema_version: CURRENT_SCHEMA_VERSION,
            inventory_id: "2c9e2b2e-7b7b-4c8f-9d18-6f5d1b3f8e10".into(),
            app_version: "3.3.0-dev".into(),
            machine: MachineIdentity {
                computer_name: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                manufacturer: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                model: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                device_kind: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
                is_virtual_machine: observed(
                    None,
                    InventoryValueState::Unavailable,
                    InventoryConfidence::Unavailable,
                ),
            },
            started_at: "2026-11-16T12:00:00Z".into(),
            finished_at: "2026-11-16T12:00:01Z".into(),
            duration_ms: 1000,
            collection_status: status,
            categories: InventoryCategories {
                system,
                firmware,
                cpu,
                memory,
                gpu,
                storage,
                displays,
                network,
                audio_peripherals,
                power,
                software,
                drivers,
            },
            warnings: vec![],
        }
    }

    fn cpu_available<T>(value: T) -> Observed<T> {
        observed(
            Some(value),
            InventoryValueState::Available,
            InventoryConfidence::Confirmed,
        )
    }

    fn cpu_processor_fixture() -> ProcessorInfo {
        ProcessorInfo {
            name: cpu_available("AMD Ryzen 5 5600G".to_string()),
            manufacturer: cpu_available("AuthenticAMD".to_string()),
            processor_id: cpu_available("00870F10".to_string()),
            socket: cpu_available("AM4".to_string()),
            architecture: cpu_available(CpuArchitecture::X64),
            physical_cores: cpu_available(6),
            logical_processors: cpu_available(12),
            max_clock_mhz: cpu_available(4400),
            l1_cache_bytes: cpu_available(384 * 1024),
            l2_cache_bytes: cpu_available(3 * 1024 * 1024),
            l3_cache_bytes: cpu_available(16 * 1024 * 1024),
            virtualization_supported: cpu_available(true),
            virtualization_firmware_enabled: cpu_available(true),
            instruction_sets: cpu_available(vec![
                CpuInstructionSet::Sse2,
                CpuInstructionSet::Avx,
                CpuInstructionSet::Avx2,
            ]),
        }
    }

    #[test]
    fn motherboard_separa_marca_comercial_smbios_e_oem() {
        let motherboard = MotherboardInfo {
            commercial_brand: observed(
                None,
                InventoryValueState::Unavailable,
                InventoryConfidence::Unavailable,
            ),
            smbios_manufacturer: observed(
                Some("PICHAU".into()),
                InventoryValueState::Available,
                InventoryConfidence::Confirmed,
            ),
            oem_manufacturer: observed(
                None,
                InventoryValueState::Unavailable,
                InventoryConfidence::Unavailable,
            ),
            product: observed(
                Some("PCH-B550M-CR".into()),
                InventoryValueState::Available,
                InventoryConfidence::Confirmed,
            ),
            version: observed(
                Some("V2.0".into()),
                InventoryValueState::Available,
                InventoryConfidence::Confirmed,
            ),
            serial_number: observed(
                None,
                InventoryValueState::Unavailable,
                InventoryConfidence::Unavailable,
            ),
        };

        motherboard.validate().unwrap();
        assert_eq!(motherboard.smbios_manufacturer.value.as_deref(), Some("PICHAU"));
        assert_eq!(motherboard.commercial_brand.value, None);
        assert_eq!(motherboard.oem_manufacturer.value, None);

        let json = serde_json::to_value(&motherboard).unwrap();
        assert_eq!(json["smbios_manufacturer"]["value"], "PICHAU");
        assert!(json["commercial_brand"]["value"].is_null());
        assert!(json["oem_manufacturer"]["value"].is_null());
        assert!(json.get("manufacturer").is_none());
    }

    #[test]
    fn motherboard_rejeita_metadados_observed_inconsistentes() {
        let mut motherboard = MotherboardInfo {
            commercial_brand: observed(
                None,
                InventoryValueState::Unavailable,
                InventoryConfidence::Unavailable,
            ),
            smbios_manufacturer: observed(
                Some("PICHAU".into()),
                InventoryValueState::Available,
                InventoryConfidence::Confirmed,
            ),
            oem_manufacturer: observed(
                None,
                InventoryValueState::Unavailable,
                InventoryConfidence::Unavailable,
            ),
            product: observed(
                None,
                InventoryValueState::Unavailable,
                InventoryConfidence::Unavailable,
            ),
            version: observed(
                None,
                InventoryValueState::Unavailable,
                InventoryConfidence::Unavailable,
            ),
            serial_number: observed(
                None,
                InventoryValueState::Unavailable,
                InventoryConfidence::Unavailable,
            ),
        };
        motherboard.oem_manufacturer = observed(
            Some("Fabricante presumido".into()),
            InventoryValueState::Unavailable,
            InventoryConfidence::Unavailable,
        );
        assert!(motherboard.validate().is_err());
    }

    #[test]
    fn cpu_modelo_serializa_e_preserva_enums_e_recursos() {
        let cpu = CpuCategory {
            processors: vec![cpu_processor_fixture()],
        };
        cpu.validate().unwrap();
        let json = serde_json::to_string(&cpu).unwrap();
        assert!(json.contains("\"architecture\":{\"value\":\"x64\""));
        assert!(json.contains("\"instruction_sets\":{\"value\":[\"sse2\",\"avx\",\"avx2\"]"));
        let restored: CpuCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(cpu, restored);
    }

    #[test]
    fn cpu_rejeita_lista_vazia_quando_categoria_disponivel() {
        assert!(CpuCategory { processors: vec![] }.validate().is_err());
    }

    #[test]
    fn cpu_rejeita_threads_inferiores_a_nucleos() {
        let mut processor = cpu_processor_fixture();
        processor.logical_processors = observed(
            Some(4),
            InventoryValueState::Available,
            InventoryConfidence::Confirmed,
        );
        assert!(processor.validate().is_err());
    }

    #[test]
    fn cpu_aceita_propriedades_parcialmente_indisponiveis() {
        let mut processor = cpu_processor_fixture();
        processor.l3_cache_bytes = observed(
            None,
            InventoryValueState::Unavailable,
            InventoryConfidence::Unavailable,
        );
        processor.max_clock_mhz = observed(
            None,
            InventoryValueState::Unavailable,
            InventoryConfidence::Unavailable,
        );
        assert!(processor.validate().is_ok());
    }

    #[test]
    fn cpu_preserva_virtualizacao_suportada_mas_desabilitada_no_firmware() {
        let mut processor = cpu_processor_fixture();
        processor.virtualization_firmware_enabled = observed(
            Some(false),
            InventoryValueState::Available,
            InventoryConfidence::Confirmed,
        );
        assert!(processor.validate().is_ok());
        assert_eq!(processor.virtualization_supported.value, Some(true));
        assert_eq!(processor.virtualization_firmware_enabled.value, Some(false));
    }

    #[test]
    fn observed_rejeita_available_sem_valor() {
        assert!(observed::<u32>(
            None,
            InventoryValueState::Available,
            InventoryConfidence::Confirmed
        )
        .validate()
        .is_err());
    }

    #[test]
    fn observed_rejeita_estado_unavailable_inconsistente() {
        assert!(observed(
            Some(1),
            InventoryValueState::Available,
            InventoryConfidence::Unavailable
        )
        .validate()
        .is_err());
    }

    #[test]
    fn observed_exige_nota_para_estimativa() {
        assert!(observed(
            Some(1),
            InventoryValueState::Available,
            InventoryConfidence::Estimated
        )
        .validate()
        .is_err());
    }

    #[test]
    fn lista_vazia_valida_e_distinta_de_falha_de_enumeracao() {
        let empty: CategoryCollection<Vec<String>> = CategoryCollection {
            state: InventoryValueState::Available,
            started_at: "now".into(),
            finished_at: "now".into(),
            duration_ms: 0,
            data: Some(vec![]),
            issues: vec![],
            warnings: vec![],
        };
        assert!(empty.validate().is_ok());
        let failed: CategoryCollection<Vec<String>> = CategoryCollection {
            state: InventoryValueState::Error,
            started_at: "now".into(),
            finished_at: "now".into(),
            duration_ms: 0,
            data: None,
            issues: vec![],
            warnings: vec![],
        };
        assert!(failed.validate().is_err());
    }

    #[test]
    fn json_round_trip_preserva_modelo_e_estados() {
        let original = sample(InventoryRunStatus::Completed);
        let json = original.to_json_pretty().unwrap();
        let restored = TechnicalInventory::from_json(&json).unwrap();
        assert_eq!(original, restored);
        assert!(json.contains("\"not_applicable\""));
        assert!(json.contains("\"unavailable\""));
    }

    #[test]
    fn partial_exige_ao_menos_uma_categoria_processada_e_uma_falha() {
        assert!(sample(InventoryRunStatus::Partial).validate().is_ok());
    }

    #[test]
    fn failed_nao_pode_conter_categoria_processada() {
        assert!(sample(InventoryRunStatus::Failed).validate().is_ok());
    }

    #[test]
    fn completed_rejeita_categoria_com_erro() {
        let mut inventory = sample(InventoryRunStatus::Completed);
        inventory.categories.gpu.state = InventoryValueState::Error;
        inventory.categories.gpu.issues.push(InventoryIssue {
            code: "gpu_failed".into(),
            message: "Falha simulada".into(),
            source: None,
            native_code: None,
        });
        assert!(inventory.validate().is_err());
    }

    #[test]
    fn uuid_e_timestamp_invalidos_sao_rejeitados() {
        let mut inventory = sample(InventoryRunStatus::Completed);
        inventory.inventory_id = "not-a-uuid".into();
        assert!(inventory.validate().is_err());
        let mut inventory = sample(InventoryRunStatus::Completed);
        inventory.finished_at = "ontem".into();
        assert!(inventory.validate().is_err());
    }

    #[test]
    fn identificadores_sensiveis_nao_sao_expostos_por_padrao() {
        let serial = Sensitive {
            value: "SERIAL-123".to_string(),
            classification: SensitiveDataKind::HardwareSerial,
        };
        let options = InventoryExportOptions::default();
        assert!(!options.include_sensitive_identifiers);
        assert!(serial
            .expose(options.include_sensitive_identifiers)
            .is_none());
        assert_eq!(serial.expose(true), Some(&"SERIAL-123".to_string()));
    }

    #[test]
    fn ip_e_serializado_com_tipo_nativo() {
        let ip = IpConfiguration {
            address: "192.168.1.10".parse().unwrap(),
            prefix_length: 24,
            is_temporary: Some(false),
        };
        let json = serde_json::to_string(&ip).unwrap();
        assert_eq!(ip, serde_json::from_str(&json).unwrap());
    }
}
