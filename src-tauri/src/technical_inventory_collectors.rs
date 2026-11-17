//! Coletores nativos do Inventário Técnico Completo.
//!
//! Esta primeira etapa coleta somente Sistema e Equipamento. Não executa
//! PowerShell, não altera o Windows e não depende do módulo Capacidades.

use crate::technical_inventory::{
    BiosInfo, CategoryCollection, DeviceKind, FirmwareCategory, FirmwareMode,
    InventoryConfidence, InventoryIssue, InventorySource, InventoryValueState, MachineIdentity,
    MotherboardInfo, Observed, OsArchitecture, Sensitive, SensitiveDataKind, SystemCategory,
    TpmInfo,
};
use std::time::Instant;

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    const HKEY_LOCAL_MACHINE: isize = 0x8000_0002u32 as isize;
    const RRF_RT_REG_SZ: u32 = 0x0000_0002;
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }

    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform_id: u32,
        service_pack: [u16; 128],
        service_pack_major: u16,
        service_pack_minor: u16,
        suite_mask: u16,
        product_type: u8,
        reserved: u8,
    }

    #[repr(C)]
    struct SystemInfo {
        processor_architecture: u16,
        reserved: u16,
        page_size: u32,
        minimum_application_address: *mut c_void,
        maximum_application_address: *mut c_void,
        active_processor_mask: usize,
        number_of_processors: u32,
        processor_type: u32,
        allocation_granularity: u32,
        processor_level: u16,
        processor_revision: u16,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemTimeAsFileTime(time: *mut FileTime);
        fn FileTimeToSystemTime(time: *const FileTime, system_time: *mut SystemTime) -> i32;
        fn GetNativeSystemInfo(info: *mut SystemInfo);
        fn GetTickCount64() -> u64;
        fn GetComputerNameExW(name_type: i32, buffer: *mut u16, size: *mut u32) -> i32;
        fn GetUserDefaultUILanguage() -> u16;
        fn LCIDToLocaleName(locale: u32, name: *mut u16, count: i32, flags: u32) -> i32;
        fn GetSystemDefaultLocaleName(name: *mut u16, count: i32) -> i32;
        fn GetSystemFirmwareTable(
            provider: u32,
            table_id: u32,
            buffer: *mut c_void,
            size: u32,
        ) -> u32;
        fn GetFirmwareType(firmware_type: *mut u32) -> i32;
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(version: *mut OsVersionInfo) -> i32;
    }
    #[link(name = "netapi32")]
    unsafe extern "system" {
        fn NetGetJoinInformation(server: *const u16, name: *mut *mut u16, status: *mut u32) -> u32;
        fn NetApiBufferFree(buffer: *mut c_void) -> u32;
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: isize,
            sub_key: *const u16,
            value: *const u16,
            flags: u32,
            value_type: *mut u32,
            data: *mut c_void,
            data_size: *mut u32,
        ) -> i32;
    }

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn now_utc() -> Option<String> {
        let mut file_time = FileTime { low: 0, high: 0 };
        let mut system_time = SystemTime {
            year: 0,
            month: 0,
            day_of_week: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            milliseconds: 0,
        };
        unsafe {
            GetSystemTimeAsFileTime(&mut file_time);
            if FileTimeToSystemTime(&file_time, &mut system_time) == 0 {
                return None;
            }
        }
        Some(format_system_time(&system_time))
    }

    fn format_system_time(value: &SystemTime) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            value.year, value.month, value.day, value.hour, value.minute, value.second
        )
    }

    fn file_time_to_iso(value: FileTime) -> Option<String> {
        let mut system_time = SystemTime {
            year: 0,
            month: 0,
            day_of_week: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            milliseconds: 0,
        };
        let ok = unsafe { FileTimeToSystemTime(&value, &mut system_time) };
        (ok != 0).then(|| format_system_time(&system_time))
    }

    fn read_registry_string(path: &str, name: &str) -> Option<String> {
        let path = wide(path);
        let name = wide(name);
        let mut buffer = vec![0u16; 4096];
        let mut bytes = (buffer.len() * std::mem::size_of::<u16>()) as u32;
        let mut kind = 0u32;
        let result = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                path.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if result != 0 {
            return None;
        }
        let length = (bytes as usize / 2).min(buffer.len());
        let end = buffer[..length]
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(length);
        let value = String::from_utf16_lossy(&buffer[..end]).trim().to_string();
        (!value.is_empty() && !is_placeholder(&value)).then_some(value)
    }

    fn read_registry_dword(path: &str, name: &str) -> Option<u32> {
        let path = wide(path);
        let name = wide(name);
        let mut value = 0u32;
        let mut bytes = std::mem::size_of::<u32>() as u32;
        let mut kind = 0u32;
        let result = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                path.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                &mut kind,
                (&mut value as *mut u32).cast(),
                &mut bytes,
            )
        };
        (result == 0 && bytes == 4).then_some(value)
    }

    fn is_placeholder(value: &str) -> bool {
        let normalized = value.trim().to_ascii_lowercase();
        normalized.is_empty()
            || normalized.contains("to be filled")
            || normalized == "default string"
            || normalized == "system product name"
            || normalized == "unknown"
    }

    fn observed<T>(value: T, provider: &str, member: &str, at: &str) -> Observed<T> {
        Observed {
            value: Some(value),
            state: InventoryValueState::Available,
            confidence: InventoryConfidence::Confirmed,
            source: Some(InventorySource {
                provider: provider.into(),
                member: Some(member.into()),
                native_code: None,
            }),
            collected_at: at.into(),
            note: None,
        }
    }

    fn unavailable<T>(at: &str, note: &str) -> Observed<T> {
        Observed {
            value: None,
            state: InventoryValueState::Unavailable,
            confidence: InventoryConfidence::Unavailable,
            source: None,
            collected_at: at.into(),
            note: Some(note.into()),
        }
    }

    fn registry_string(name: &str, at: &str) -> Observed<String> {
        read_registry_string(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", name)
            .map(|value| observed(value, "Windows Registry", name, at))
            .unwrap_or_else(|| unavailable(at, "Valor não informado pelo Registro do Windows."))
    }

    fn registry_dword(name: &str, at: &str) -> Observed<u32> {
        read_registry_dword(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", name)
            .map(|value| observed(value, "Windows Registry", name, at))
            .unwrap_or_else(|| unavailable(at, "Valor não informado pelo Registro do Windows."))
    }

    fn os_version(at: &str) -> Observed<String> {
        let mut version = OsVersionInfo {
            size: std::mem::size_of::<OsVersionInfo>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform_id: 0,
            service_pack: [0; 128],
            service_pack_major: 0,
            service_pack_minor: 0,
            suite_mask: 0,
            product_type: 0,
            reserved: 0,
        };
        if unsafe { RtlGetVersion(&mut version) } != 0 {
            return unavailable(
                at,
                "RtlGetVersion não conseguiu identificar a versão do Windows.",
            );
        }
        observed(
            format!("{}.{}.{}", version.major, version.minor, version.build),
            "ntdll!RtlGetVersion",
            "major.minor.build",
            at,
        )
    }

    fn domain_or_workgroup(at: &str) -> Observed<String> {
        let mut name = ptr::null_mut();
        let mut status = 0u32;
        let result = unsafe { NetGetJoinInformation(ptr::null(), &mut name, &mut status) };
        if result != 0 || name.is_null() {
            return unavailable(
                at,
                "NetGetJoinInformation não retornou domínio ou grupo de trabalho.",
            );
        }

        let mut length = 0usize;
        unsafe {
            while *name.add(length) != 0 {
                length += 1;
            }
        }
        let value = unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(name, length)) };
        unsafe {
            NetApiBufferFree(name.cast());
        }

        if value.trim().is_empty() {
            return unavailable(at, "O Windows retornou um nome de associação vazio.");
        }
        let member = match status {
            2 => "workgroup",
            3 => "domain",
            _ => "join_name",
        };
        observed(value, "Netapi32!NetGetJoinInformation", member, at)
    }

    fn smbios_strings(table: &[u8], wanted_type: u8, indexes: &[usize]) -> Option<Vec<String>> {
        let mut offset = 0usize;
        while offset.checked_add(4)? <= table.len() {
            let structure_type = table[offset];
            let formatted_length = table[offset + 1] as usize;
            if formatted_length < 4 || offset.checked_add(formatted_length)? > table.len() {
                return None;
            }
            if structure_type == 127 {
                break;
            }

            let strings_start = offset + formatted_length;
            let strings_end = table[strings_start..]
                .windows(2)
                .position(|pair| pair == [0, 0])
                .map(|position| strings_start + position)?;
            if structure_type == wanted_type {
                let strings: Vec<&[u8]> = if strings_end == strings_start {
                    Vec::new()
                } else {
                    table[strings_start..strings_end]
                        .split(|byte| *byte == 0)
                        .collect()
                };
                return Some(
                    indexes
                        .iter()
                        .map(|index| {
                            let string_index =
                                table.get(offset + *index).copied().unwrap_or(0) as usize;
                            if string_index == 0 {
                                String::new()
                            } else {
                                strings
                                    .get(string_index - 1)
                                    .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
                                    .unwrap_or_default()
                            }
                        })
                        .collect(),
                );
            }
            offset = strings_end + 2;
        }
        None
    }

    fn smbios_byte_field(table: &[u8], wanted_type: u8, field_offset: usize) -> Option<u8> {
        let mut offset = 0usize;
        while offset.checked_add(4)? <= table.len() {
            let structure_type = table[offset];
            let formatted_length = table[offset + 1] as usize;
            if formatted_length < 4 || offset.checked_add(formatted_length)? > table.len() {
                return None;
            }
            if structure_type == 127 {
                break;
            }
            if structure_type == wanted_type {
                return (field_offset < formatted_length).then(|| table[offset + field_offset]);
            }
            let strings_start = offset + formatted_length;
            let strings_end = table[strings_start..]
                .windows(2)
                .position(|pair| pair == [0, 0])
                .map(|position| strings_start + position)?;
            offset = strings_end + 2;
        }
        None
    }

    fn smbios_table() -> Option<Vec<u8>> {
        const RSMB: u32 = 0x5253_4D42;
        let size = unsafe { GetSystemFirmwareTable(RSMB, 0, ptr::null_mut(), 0) };
        if !(8..=1_048_576).contains(&size) {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        let written = unsafe { GetSystemFirmwareTable(RSMB, 0, buffer.as_mut_ptr().cast(), size) };
        if written < 8 || written > size {
            return None;
        }
        buffer.truncate(written as usize);

        let table_length = u32::from_le_bytes(buffer[4..8].try_into().ok()?) as usize;
        if table_length > buffer.len().saturating_sub(8) {
            return None;
        }
        Some(buffer[8..8 + table_length].to_vec())
    }

    fn hardware_identity(
        at: &str,
    ) -> (
        Observed<String>,
        Observed<String>,
        Observed<DeviceKind>,
        Observed<bool>,
    ) {
        let Some(table) = smbios_table() else {
            return (
                unavailable(at, "Tabela SMBIOS indisponível."),
                unavailable(at, "Tabela SMBIOS indisponível."),
                unavailable(at, "Tipo de equipamento não identificado via SMBIOS."),
                unavailable(at, "Sem evidência SMBIOS conclusiva de virtualização."),
            );
        };

        let fields = smbios_strings(&table, 1, &[4, 5]).unwrap_or_default();
        let make = fields.first().cloned().unwrap_or_default();
        let model_name = fields.get(1).cloned().unwrap_or_default();

        let manufacturer = if !make.is_empty() && !is_placeholder(&make) {
            observed(
                make.clone(),
                "kernel32!GetSystemFirmwareTable/SMBIOS",
                "Type 1 Manufacturer",
                at,
            )
        } else {
            unavailable(at, "Fabricante não informado na estrutura SMBIOS Type 1.")
        };
        let model = if !model_name.is_empty() && !is_placeholder(&model_name) {
            observed(
                model_name.clone(),
                "kernel32!GetSystemFirmwareTable/SMBIOS",
                "Type 1 Product Name",
                at,
            )
        } else {
            unavailable(at, "Modelo não informado na estrutura SMBIOS Type 1.")
        };

        let vm_text = format!("{make} {model_name}").to_ascii_lowercase();
        let is_vm = [
            "vmware",
            "virtualbox",
            "qemu",
            "kvm",
            "xen",
            "parallels",
            "virtual machine",
            "microsoft corporation virtual",
        ]
        .iter()
        .any(|signature| vm_text.contains(signature));

        let chassis = smbios_byte_field(&table, 3, 5).map(|value| value & 0x7f);
        let kind = if is_vm {
            Some(DeviceKind::VirtualMachine)
        } else {
            chassis.and_then(|value| match value {
                3..=7 | 13 | 15 | 16 | 35 | 36 => Some(DeviceKind::Desktop),
                8..=10 | 14 | 31 | 32 => Some(DeviceKind::Laptop),
                30 => Some(DeviceKind::Tablet),
                17 | 23 => Some(DeviceKind::Server),
                _ => None,
            })
        };
        let device_kind = kind
            .map(|value| {
                observed(
                    value,
                    "kernel32!GetSystemFirmwareTable/SMBIOS",
                    "Type 3 Chassis Type / Type 1 signature",
                    at,
                )
            })
            .unwrap_or_else(|| {
                unavailable(
                    at,
                    "Tipo de equipamento não reconhecido pela estrutura SMBIOS.",
                )
            });
        let virtual_machine = if is_vm {
            observed(
                true,
                "kernel32!GetSystemFirmwareTable/SMBIOS",
                "Type 1 virtual hardware signature",
                at,
            )
        } else {
            unavailable(
                at,
                "Nenhuma assinatura SMBIOS conclusiva de VM; ausência de assinatura não comprova hardware físico.",
            )
        };

        (manufacturer, model, device_kind, virtual_machine)
    }

    fn not_applicable<T>(at: &str, note: &str) -> Observed<T> {
        Observed {
            value: None,
            state: InventoryValueState::NotApplicable,
            confidence: InventoryConfidence::Confirmed,
            source: None,
            collected_at: at.into(),
            note: Some(note.into()),
        }
    }

    fn smbios_string_values(
        table: Option<&[u8]>,
        structure_type: u8,
        indexes: &[usize],
        at: &str,
        unavailable_note: &str,
    ) -> Vec<Observed<String>> {
        let Some(table) = table else {
            return indexes
                .iter()
                .map(|_| unavailable(at, unavailable_note))
                .collect();
        };
        let values = smbios_strings(table, structure_type, indexes).unwrap_or_default();
        indexes
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let value = values.get(index).cloned().unwrap_or_default();
                if value.is_empty() || is_placeholder(&value) {
                    unavailable(
                        at,
                        &format!("Campo SMBIOS Type {structure_type} ({field}) não informado."),
                    )
                } else {
                    observed(
                        value,
                        "kernel32!GetSystemFirmwareTable/SMBIOS",
                        &format!("Type {structure_type} string index field {field}"),
                        at,
                    )
                }
            })
            .collect()
    }

    fn firmware_mode(at: &str) -> Observed<FirmwareMode> {
        let mut firmware_type = 0u32;
        if unsafe { GetFirmwareType(&mut firmware_type) } == 0 {
            return unavailable(at, "GetFirmwareType não conseguiu identificar o modo de inicialização.");
        }
        let (value, member) = match firmware_type {
            1 => (FirmwareMode::Legacy, "FIRMWARE_TYPE_BIOS"),
            2 => (FirmwareMode::Uefi, "FIRMWARE_TYPE_UEFI"),
            _ => {
                return unavailable(
                    at,
                    "O Windows retornou um tipo de firmware desconhecido.",
                )
            }
        };
        observed(value, "kernel32!GetFirmwareType", member, at)
    }

    fn secure_boot_enabled(at: &str, mode: &Observed<FirmwareMode>) -> Observed<bool> {
        match mode.value {
            Some(FirmwareMode::Legacy) => not_applicable(
                at,
                "Secure Boot não se aplica ao modo de inicialização Legacy/BIOS.",
            ),
            Some(FirmwareMode::Uefi) => {
                const SECURE_BOOT_KEY: &str =
                    r"SYSTEM\CurrentControlSet\Control\SecureBoot\State";
                match read_registry_dword(SECURE_BOOT_KEY, "UEFISecureBootEnabled") {
                    Some(0) => observed(
                        false,
                        "Windows Registry",
                        "UEFISecureBootEnabled",
                        at,
                    ),
                    Some(1) => observed(
                        true,
                        "Windows Registry",
                        "UEFISecureBootEnabled",
                        at,
                    ),
                    Some(_) => unavailable(
                        at,
                        "O Registro retornou um valor inválido para UEFISecureBootEnabled.",
                    ),
                    None => unavailable(
                        at,
                        "O estado do Secure Boot não está disponível no Registro do Windows.",
                    ),
                }
            }
            _ => unavailable(
                at,
                "Não foi possível determinar o estado do Secure Boot sem identificar o modo de firmware.",
            ),
        }
    }

    fn unavailable_tpm(at: &str) -> TpmInfo {
        let note = "A coleta nativa do estado do TPM ainda não foi implementada.";
        TpmInfo {
            present: unavailable(at, note),
            ready: unavailable(at, note),
            enabled: unavailable(at, note),
            activated: unavailable(at, note),
            spec_version: unavailable(at, note),
            manufacturer: unavailable(at, note),
        }
    }

    pub(super) fn collect_firmware_category(
    ) -> Result<CategoryCollection<FirmwareCategory>, String> {
        let started = Instant::now();
        let started_at = now_utc().ok_or_else(|| {
            "Não foi possível obter o horário UTC para iniciar a coleta de placa-mãe e firmware."
                .to_string()
        })?;

        let table = smbios_table();
        let motherboard_fields = smbios_string_values(
            table.as_deref(),
            2,
            &[4, 5, 6, 7],
            &started_at,
            "Tabela SMBIOS indisponível.",
        );
        let bios_fields = smbios_string_values(
            table.as_deref(),
            0,
            &[4, 5, 8],
            &started_at,
            "Tabela SMBIOS indisponível.",
        );

        let mut motherboard_fields = motherboard_fields.into_iter();
        let motherboard_manufacturer = motherboard_fields.next().unwrap();
        let motherboard_product = motherboard_fields.next().unwrap();
        let motherboard_version = motherboard_fields.next().unwrap();
        let motherboard_serial = motherboard_fields
            .next()
            .unwrap()
            .value
            .map(|value| Sensitive {
                value,
                classification: SensitiveDataKind::HardwareSerial,
            });
        let motherboard_serial_number = match motherboard_serial {
            Some(value) => observed(
                value,
                "kernel32!GetSystemFirmwareTable/SMBIOS",
                "Type 2 Serial Number",
                &started_at,
            ),
            None => unavailable(
                &started_at,
                "Número de série da placa-mãe não informado pela estrutura SMBIOS Type 2.",
            ),
        };

        let mut bios_fields = bios_fields.into_iter();
        let bios_vendor = bios_fields.next().unwrap();
        let bios_version = bios_fields.next().unwrap();
        let bios_release_date = bios_fields.next().unwrap();

        let mode = firmware_mode(&started_at);
        let secure_boot = secure_boot_enabled(&started_at, &mode);
        let tpm = unavailable_tpm(&started_at);

        let mut warnings = Vec::new();
        if table.is_none() {
            warnings.push("A tabela SMBIOS não pôde ser consultada; dados de placa-mãe e BIOS podem ficar indisponíveis.".into());
        }
        warnings.push("Os campos de TPM permanecem indisponíveis até a implementação do coletor nativo específico.".into());

        let data = FirmwareCategory {
            motherboard: MotherboardInfo {
                manufacturer: motherboard_manufacturer,
                product: motherboard_product,
                version: motherboard_version,
                serial_number: motherboard_serial_number,
            },
            bios: BiosInfo {
                vendor: bios_vendor,
                version: bios_version,
                release_date: bios_release_date,
                mode,
            },
            tpm,
            secure_boot_enabled: secure_boot,
        };

        let finished_at = now_utc().unwrap_or_else(|| started_at.clone());
        let duration_ms = started.elapsed().as_millis().max(1).min(u64::MAX as u128) as u64;
        let collection = CategoryCollection {
            state: InventoryValueState::Available,
            started_at,
            finished_at,
            duration_ms,
            data: Some(data),
            issues: Vec::<InventoryIssue>::new(),
            warnings,
        };
        collection.validate()?;
        Ok(collection)
    }

    fn architecture(at: &str) -> Observed<OsArchitecture> {
        let mut info = std::mem::MaybeUninit::<SystemInfo>::zeroed();
        unsafe { GetNativeSystemInfo(info.as_mut_ptr()) };
        let architecture = unsafe { info.assume_init() }.processor_architecture;
        let value = match architecture {
            0 => OsArchitecture::X86,
            9 => OsArchitecture::X64,
            12 => OsArchitecture::Arm64,
            _ => {
                return unavailable(
                    at,
                    "Arquitetura nativa não reconhecida pela API do Windows.",
                )
            }
        };
        observed(
            value,
            "kernel32!GetNativeSystemInfo",
            "wProcessorArchitecture",
            at,
        )
    }

    fn computer_name(at: &str) -> Observed<String> {
        let mut buffer = vec![0u16; 256];
        let mut size = buffer.len() as u32;
        let ok = unsafe { GetComputerNameExW(5, buffer.as_mut_ptr(), &mut size) };
        if ok == 0 || size == 0 {
            return unavailable(at, "GetComputerNameExW não retornou o nome do dispositivo.");
        }
        let value = String::from_utf16_lossy(&buffer[..size as usize]);
        observed(
            value,
            "kernel32!GetComputerNameExW",
            "ComputerNamePhysicalDnsHostname",
            at,
        )
    }

    fn ui_language(at: &str) -> Observed<String> {
        let language = unsafe { GetUserDefaultUILanguage() };
        let mut buffer = vec![0u16; 85];
        let written = unsafe { LCIDToLocaleName(language as u32, buffer.as_mut_ptr(), 85, 0) };
        if written <= 1 {
            return unavailable(at, "Não foi possível resolver o idioma da interface.");
        }
        let value = String::from_utf16_lossy(&buffer[..written as usize - 1]);
        observed(
            value,
            "kernel32!GetUserDefaultUILanguage",
            "LCIDToLocaleName",
            at,
        )
    }

    fn system_locale(at: &str) -> Observed<String> {
        let mut buffer = vec![0u16; 85];
        let written = unsafe { GetSystemDefaultLocaleName(buffer.as_mut_ptr(), 85) };
        if written <= 1 {
            return unavailable(at, "Não foi possível consultar a localidade do sistema.");
        }
        let value = String::from_utf16_lossy(&buffer[..written as usize - 1]);
        observed(
            value,
            "kernel32!GetSystemDefaultLocaleName",
            "locale_name",
            at,
        )
    }

    fn boot_time(at: &str) -> Observed<String> {
        let mut current = FileTime { low: 0, high: 0 };
        unsafe { GetSystemTimeAsFileTime(&mut current) };
        let current_ticks = ((current.high as u64) << 32) | current.low as u64;
        let uptime_ticks = unsafe { GetTickCount64() }.saturating_mul(10_000);
        let boot_ticks = current_ticks.saturating_sub(uptime_ticks);
        let value = FileTime {
            low: boot_ticks as u32,
            high: (boot_ticks >> 32) as u32,
        };
        file_time_to_iso(value)
            .map(|value| observed(value, "kernel32!GetTickCount64", "system_boot_time", at))
            .unwrap_or_else(|| {
                unavailable(at, "Não foi possível calcular o horário de inicialização.")
            })
    }

    fn install_date(at: &str) -> Observed<String> {
        const WINDOWS_EPOCH_OFFSET_SECONDS: u64 = 11_644_473_600;
        let Some(seconds) = read_registry_dword(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "InstallDate",
        ) else {
            return unavailable(at, "InstallDate não está disponível no Registro.");
        };
        let ticks = (seconds as u64 + WINDOWS_EPOCH_OFFSET_SECONDS) * 10_000_000;
        let file_time = FileTime {
            low: ticks as u32,
            high: (ticks >> 32) as u32,
        };
        file_time_to_iso(file_time)
            .map(|value| observed(value, "Windows Registry", "InstallDate", at))
            .unwrap_or_else(|| unavailable(at, "InstallDate não pôde ser convertido para UTC."))
    }

    pub(super) fn collect() -> Result<(CategoryCollection<SystemCategory>, MachineIdentity), String>
    {
        let started = Instant::now();
        let started_at = now_utc().ok_or_else(|| {
            "Não foi possível obter o horário UTC do Windows para iniciar o inventário.".to_string()
        })?;
        let os_name = registry_string("ProductName", &started_at);
        let os_edition = registry_string("EditionID", &started_at);
        let os_version = os_version(&started_at);
        let display_version = registry_string("DisplayVersion", &started_at);
        let build_text = registry_string("CurrentBuildNumber", &started_at);
        let build_number = match build_text
            .value
            .as_deref()
            .and_then(|v| v.parse::<u32>().ok())
        {
            Some(value) => observed(value, "Windows Registry", "CurrentBuildNumber", &started_at),
            None => unavailable(&started_at, "Build numérico não informado pelo Registro."),
        };
        let ubr = registry_dword("UBR", &started_at);
        let architecture = architecture(&started_at);
        let ui_language = ui_language(&started_at);
        let system_locale = system_locale(&started_at);
        let install_date = install_date(&started_at);
        let boot_time = boot_time(&started_at);
        let computer_name = computer_name(&started_at);

        let domain_or_workgroup = domain_or_workgroup(&started_at);
        let (manufacturer, model, device_kind, is_virtual_machine) = hardware_identity(&started_at);

        let data = SystemCategory {
            os_name,
            os_edition,
            os_version,
            display_version,
            build_number,
            ubr,
            architecture,
            ui_language,
            system_locale,
            install_date,
            boot_time,
            computer_name: computer_name.clone(),
            domain_or_workgroup,
            device_kind: device_kind.clone(),
        };

        let machine = MachineIdentity {
            computer_name,
            manufacturer,
            model,
            device_kind,
            is_virtual_machine,
        };

        let mut warnings = Vec::new();
        if data.os_name.value.is_none() || data.build_number.value.is_none() {
            warnings.push(
                "Identificação básica do Windows ficou incompleta; alguns valores do Registro não foram lidos."
                    .into(),
            );
        }
        let finished_at = now_utc().unwrap_or_else(|| started_at.clone());
        let duration_ms = started.elapsed().as_millis().max(1).min(u64::MAX as u128) as u64;
        let collection = CategoryCollection {
            state: InventoryValueState::Available,
            started_at,
            finished_at,
            duration_ms,
            data: Some(data),
            issues: Vec::<InventoryIssue>::new(),
            warnings,
        };
        collection.validate()?;
        Ok((collection, machine))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn placeholder_values_are_not_reported_as_real_hardware() {
            assert!(is_placeholder("To Be Filled By O.E.M."));
            assert!(is_placeholder("System Product Name"));
            assert!(!is_placeholder("ASUSTeK COMPUTER INC."));
        }

        #[test]
        fn smbios_parser_resolves_indexed_strings() {
            let table = [
                1, 6, 0, 0, 1, 2, b'A', b'C', b'M', b'E', 0, b'M', b'o', b'd', b'e', b'l', b' ',
                b'X', 0, 0, 127, 4, 0, 0,
            ];
            assert_eq!(
                smbios_strings(&table, 1, &[4, 5]).unwrap(),
                vec!["ACME".to_string(), "Model X".to_string()]
            );
        }

        #[test]
        fn smbios_type_2_resolves_motherboard_fields() {
            let table = [
                2, 8, 0, 0, 1, 2, 3, 4,
                b'A', b'C', b'M', b'E', 0,
                b'P', b'C', b'H', b'-', b'B', b'5', b'5', b'0', b'M', 0,
                b'1', b'.', b'0', 0,
                b'S', b'N', b'1', b'2', b'3', 0, 0,
                127, 4, 0, 0,
            ];
            assert_eq!(
                smbios_strings(&table, 2, &[4, 5, 6, 7]).unwrap(),
                vec![
                    "ACME".to_string(),
                    "PCH-B550M".to_string(),
                    "1.0".to_string(),
                    "SN123".to_string()
                ]
            );
        }

        #[test]
        fn smbios_type_0_resolves_bios_fields() {
            let table = [
                0, 9, 0, 0, 1, 2, 0, 0, 3,
                b'A', b'M', b'I', 0,
                b'F', b'1', b'.', b'2', 0,
                b'1', b'1', b'/', b'1', b'7', b'/', b'2', b'0', b'2', b'6', 0, 0,
                127, 4, 0, 0,
            ];
            assert_eq!(
                smbios_strings(&table, 0, &[4, 5, 8]).unwrap(),
                vec![
                    "AMI".to_string(),
                    "F1.2".to_string(),
                    "11/17/2026".to_string()
                ]
            );
        }

        #[test]
        fn smbios_chassis_type_is_read_from_the_formatted_structure() {
            let table = [3, 6, 0, 0, 1, 3, b'A', b'C', b'M', b'E', 0, 0, 127, 4, 0, 0];
            assert_eq!(smbios_byte_field(&table, 3, 5), Some(3));
        }

        #[test]
        fn system_time_is_formatted_as_utc_rfc3339() {
            let value = SystemTime {
                year: 2026,
                month: 11,
                day_of_week: 2,
                day: 17,
                hour: 8,
                minute: 15,
                second: 30,
                milliseconds: 0,
            };
            assert_eq!(format_system_time(&value), "2026-11-17T08:15:30Z");
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn collect_system_category(
) -> Result<(CategoryCollection<SystemCategory>, MachineIdentity), String> {
    windows::collect()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn collect_system_category(
) -> Result<(CategoryCollection<SystemCategory>, MachineIdentity), String> {
    Err("O Inventário Técnico está disponível somente no Windows.".into())
}

#[cfg(target_os = "windows")]
pub(crate) fn collect_firmware_category(
) -> Result<CategoryCollection<FirmwareCategory>, String> {
    windows::collect_firmware_category()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn collect_firmware_category(
) -> Result<CategoryCollection<FirmwareCategory>, String> {
    Err("O Inventário Técnico está disponível somente no Windows.".into())
}
