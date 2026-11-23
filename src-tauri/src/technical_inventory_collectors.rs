//! Coletores nativos do Inventário Técnico Completo.
//!
//! Esta primeira etapa coleta somente Sistema e Equipamento. Não executa
//! PowerShell, não altera o Windows e não depende do módulo Capacidades.

use crate::technical_inventory::{
    BiosInfo, CategoryCollection, CpuArchitecture, CpuCategory, CpuInstructionSet, DeviceKind,
    FirmwareCategory, FirmwareMode, InventoryConfidence, InventoryIssue, InventorySource,
    InventoryValueState, MachineIdentity, MotherboardInfo, Observed, OsArchitecture, ProcessorInfo,
    Sensitive, SensitiveDataKind, SystemCategory, TpmInfo,
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
        fn IsProcessorFeaturePresent(feature: u32) -> i32;
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
            return unavailable(
                at,
                "GetFirmwareType não conseguiu identificar o modo de inicialização.",
            );
        }
        let (value, member) = match firmware_type {
            1 => (FirmwareMode::Legacy, "FIRMWARE_TYPE_BIOS"),
            2 => (FirmwareMode::Uefi, "FIRMWARE_TYPE_UEFI"),
            _ => return unavailable(at, "O Windows retornou um tipo de firmware desconhecido."),
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

    fn unavailable_tpm(at: &str, note: &str) -> TpmInfo {
        TpmInfo {
            present: unavailable(at, note),
            ready: unavailable(at, note),
            enabled: unavailable(at, note),
            activated: unavailable(at, note),
            spec_version: unavailable(at, note),
            manufacturer: unavailable(at, note),
        }
    }

    fn not_applicable_tpm(at: &str) -> TpmInfo {
        let note = "Campo não aplicável porque nenhum TPM foi detectado.";
        TpmInfo {
            present: observed(false, "WMI/Win32_Tpm", "instância ausente", at),
            ready: not_applicable(at, note),
            enabled: not_applicable(at, note),
            activated: not_applicable(at, note),
            spec_version: not_applicable(at, note),
            manufacturer: not_applicable(at, note),
        }
    }

    fn tpm_manufacturer_id(value: u32) -> Option<String> {
        if value == 0 {
            return None;
        }
        let bytes = value.to_be_bytes();
        let text: String = bytes
            .into_iter()
            .take_while(|byte| *byte != 0)
            .map(char::from)
            .collect();
        (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_graphic())).then_some(text)
    }

    fn collect_tpm(at: &str) -> (TpmInfo, Option<String>) {
        use ::windows::core::{w, BSTR, PCWSTR};
        use ::windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CoUninitialize,
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
        };
        use ::windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
        use ::windows::Win32::System::Variant::VARIANT;
        use ::windows::Win32::System::Wmi::{
            IEnumWbemClassObject, IWbemClassObject, IWbemLocator, WbemLocator,
            WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE,
            WBEM_INFINITE,
        };

        fn property_bool(object: &IWbemClassObject, name: PCWSTR) -> Option<bool> {
            let mut value = VARIANT::default();
            unsafe { object.Get(name, 0, &mut value, None, None) }.ok()?;
            bool::try_from(&value).ok()
        }

        fn property_u32(object: &IWbemClassObject, name: PCWSTR) -> Option<u32> {
            let mut value = VARIANT::default();
            unsafe { object.Get(name, 0, &mut value, None, None) }.ok()?;
            u32::try_from(&value).ok()
        }

        fn property_string(object: &IWbemClassObject, name: PCWSTR) -> Option<String> {
            let mut value = VARIANT::default();
            unsafe { object.Get(name, 0, &mut value, None, None) }.ok()?;
            let value = BSTR::try_from(&value).ok()?.to_string();
            let value = value.trim().to_string();
            (!value.is_empty() && !value.eq_ignore_ascii_case("not supported")).then_some(value)
        }

        let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let initialized_here = init.is_ok();
        let apartment_already_initialized = init.0 == 0x80010106u32 as i32;

        if !initialized_here && !apartment_already_initialized {
            return (
                unavailable_tpm(
                    at,
                    "Não foi possível inicializar COM para consultar o provedor nativo Win32_Tpm.",
                ),
                Some(format!(
                    "Falha ao inicializar COM para consulta do TPM: {init:?}"
                )),
            );
        }

        let result = (|| -> Result<TpmInfo, String> {
            let locator: IWbemLocator =
                unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }
                    .map_err(|error| format!("CoCreateInstance(WbemLocator): {error}"))?;

            let namespace = BSTR::from("ROOT\\CIMV2\\Security\\MicrosoftTpm");
            let empty = BSTR::new();
            let services = unsafe {
                locator.ConnectServer(&namespace, &empty, &empty, &empty, 0, &empty, None)
            }
            .map_err(|error| format!("ConnectServer(Win32_Tpm): {error}"))?;

            unsafe {
                CoSetProxyBlanket(
                    &services,
                    RPC_C_AUTHN_WINNT,
                    RPC_C_AUTHZ_NONE,
                    None,
                    RPC_C_AUTHN_LEVEL_CALL,
                    RPC_C_IMP_LEVEL_IMPERSONATE,
                    None,
                    EOAC_NONE,
                )
            }
            .map_err(|error| format!("CoSetProxyBlanket(Win32_Tpm): {error}"))?;

            let language = BSTR::from("WQL");
            let query = BSTR::from(
                "SELECT IsEnabled_InitialValue, IsActivated_InitialValue, SpecVersion, ManufacturerId FROM Win32_Tpm",
            );
            let enumerator: IEnumWbemClassObject = unsafe {
                services.ExecQuery(
                    &language,
                    &query,
                    WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                    None,
                )
            }
            .map_err(|error| format!("ExecQuery(Win32_Tpm): {error}"))?;

            let mut objects = [None];
            let mut returned = 0;
            unsafe {
                let status = enumerator.Next(WBEM_INFINITE, &mut objects, &mut returned);
                if status.is_err() {
                    return Err(format!("IEnumWbemClassObject::Next: {status:?}"));
                }
            }

            if returned == 0 {
                return Ok(not_applicable_tpm(at));
            }

            let object = objects[0]
                .as_ref()
                .ok_or_else(|| "O provedor Win32_Tpm retornou uma instância vazia.".to_string())?;

            let enabled = property_bool(object, w!("IsEnabled_InitialValue"))
                .map(|value| observed(value, "WMI/Win32_Tpm", "IsEnabled_InitialValue", at))
                .unwrap_or_else(|| {
                    unavailable(
                        at,
                        "O provedor Win32_Tpm não retornou IsEnabled_InitialValue.",
                    )
                });
            let activated = property_bool(object, w!("IsActivated_InitialValue"))
                .map(|value| observed(value, "WMI/Win32_Tpm", "IsActivated_InitialValue", at))
                .unwrap_or_else(|| {
                    unavailable(
                        at,
                        "O provedor Win32_Tpm não retornou IsActivated_InitialValue.",
                    )
                });
            let spec_version = property_string(object, w!("SpecVersion"))
                .map(|value| observed(value, "WMI/Win32_Tpm", "SpecVersion", at))
                .unwrap_or_else(|| {
                    unavailable(at, "A versão da especificação não foi informada pelo TPM.")
                });
            let manufacturer = property_u32(object, w!("ManufacturerId"))
                .and_then(tpm_manufacturer_id)
                .map(|value| observed(value, "WMI/Win32_Tpm", "ManufacturerId", at))
                .unwrap_or_else(|| {
                    unavailable(
                        at,
                        "O identificador do fabricante do TPM não foi informado.",
                    )
                });

            let ready = property_string(object, w!("__PATH"))
                .and_then(|path| {
                    let path = BSTR::from(path);
                    let method = BSTR::from("IsReady");
                    let mut output: Option<IWbemClassObject> = None;
                    unsafe {
                        services.ExecMethod(
                            &path,
                            &method,
                            WBEM_GENERIC_FLAG_TYPE(0),
                            None,
                            None,
                            Some(&mut output),
                            None,
                        )
                    }
                    .ok()?;
                    let output = output?;
                    property_bool(&output, w!("IsReady"))
                })
                .map(|value| observed(value, "WMI/Win32_Tpm::IsReady", "IsReady", at))
                .unwrap_or_else(|| unavailable(
                    at,
                    "O método Win32_Tpm::IsReady não pôde ser consultado; pode exigir privilégios administrativos.",
                ));

            Ok(TpmInfo {
                present: observed(true, "WMI/Win32_Tpm", "instância detectada", at),
                ready,
                enabled,
                activated,
                spec_version,
                manufacturer,
            })
        })();

        if initialized_here {
            unsafe { CoUninitialize() };
        }
        match result {
            Ok(info) => {
                let warning = if [
                    info.ready.state,
                    info.enabled.state,
                    info.activated.state,
                    info.spec_version.state,
                    info.manufacturer.state,
                ]
                .iter()
                .any(|state| *state == InventoryValueState::Unavailable)
                {
                    Some(
                        "O TPM foi detectado, mas alguns atributos não puderam ser consultados."
                            .into(),
                    )
                } else {
                    None
                };
                (info, warning)
            }
            Err(error) => (
                unavailable_tpm(at, "A consulta nativa ao provedor Win32_Tpm falhou."),
                Some(error),
            ),
        }
    }

    #[derive(Default)]
    struct SmbiosCpu {
        socket: Option<String>,
        manufacturer: Option<String>,
        version: Option<String>,
        processor_id: Option<String>,
        max_clock_mhz: Option<u64>,
        physical_cores: Option<u32>,
        logical_processors: Option<u32>,
        l1_cache_bytes: Option<u64>,
        l2_cache_bytes: Option<u64>,
        l3_cache_bytes: Option<u64>,
    }

    fn smbios_records(table: &[u8]) -> Vec<(u8, Vec<u8>, Vec<String>)> {
        let mut records = Vec::new();
        let mut offset = 0usize;
        while offset.checked_add(4).is_some_and(|end| end <= table.len()) {
            let kind = table[offset];
            let length = table[offset + 1] as usize;
            if length < 4
                || offset
                    .checked_add(length)
                    .is_none_or(|end| end > table.len())
            {
                break;
            }
            if kind == 127 {
                break;
            }
            let strings_start = offset + length;
            let Some(relative_end) = table[strings_start..]
                .windows(2)
                .position(|pair| pair == [0, 0])
            else {
                break;
            };
            let strings_end = strings_start + relative_end;
            let strings = if strings_end == strings_start {
                Vec::new()
            } else {
                table[strings_start..strings_end]
                    .split(|byte| *byte == 0)
                    .map(|value| String::from_utf8_lossy(value).trim().to_string())
                    .collect()
            };
            records.push((kind, table[offset..offset + length].to_vec(), strings));
            offset = strings_end + 2;
        }
        records
    }

    fn smbios_record_string(record: &[u8], strings: &[String], field: usize) -> Option<String> {
        let index = *record.get(field)? as usize;
        if index == 0 {
            return None;
        }
        strings
            .get(index - 1)
            .filter(|value| !value.is_empty() && !is_placeholder(value))
            .cloned()
    }

    fn smbios_u16(record: &[u8], offset: usize) -> Option<u16> {
        Some(u16::from_le_bytes([
            *record.get(offset)?,
            *record.get(offset + 1)?,
        ]))
    }

    fn smbios_cache_size(record: &[u8]) -> Option<u64> {
        let encoded = smbios_u16(record, 0x09)?;
        if encoded != 0 && encoded != 0xffff {
            let size = (encoded & 0x7fff) as u64;
            return Some(if encoded & 0x8000 == 0 {
                size * 1024
            } else {
                size * 64 * 1024
            });
        }
        if record.len() >= 0x1b {
            let extended = u32::from_le_bytes(record[0x17..0x1b].try_into().ok()?);
            if extended != 0 && extended != u32::MAX {
                return Some(extended as u64);
            }
        }
        None
    }

    fn smbios_cpu_records(table: &[u8]) -> Vec<SmbiosCpu> {
        let records = smbios_records(table);
        let caches: std::collections::HashMap<u16, (u8, u64)> = records
            .iter()
            .filter(|(kind, _, _)| *kind == 7)
            .filter_map(|(_, record, _)| {
                let handle = smbios_u16(record, 0x02)?;
                let level = (record.get(0x05).copied()? >> 5) & 0x07;
                let size = smbios_cache_size(record)?;
                Some((handle, (level, size)))
            })
            .collect();

        records
            .iter()
            .filter(|(kind, _, _)| *kind == 4)
            .map(|(_, record, strings)| {
                let cache_size = |offset: usize, wanted_level: u8| -> Option<u64> {
                    let handle = smbios_u16(record, offset)?;
                    caches
                        .get(&handle)
                        .filter(|(level, _)| *level == wanted_level)
                        .map(|(_, size)| *size)
                };
                let core_count = record
                    .get(0x23)
                    .copied()
                    .filter(|value| *value != 0 && *value != 0xff)
                    .map(u32::from)
                    .or_else(|| {
                        (record.len() >= 0x2c)
                            .then(|| u16::from_le_bytes([record[0x2a], record[0x2b]]) as u32)
                            .filter(|value| *value != 0 && *value != 0xffff)
                    });
                let thread_count = record
                    .get(0x25)
                    .copied()
                    .filter(|value| *value != 0 && *value != 0xff)
                    .map(u32::from)
                    .or_else(|| {
                        (record.len() >= 0x30)
                            .then(|| u16::from_le_bytes([record[0x2e], record[0x2f]]) as u32)
                            .filter(|value| *value != 0 && *value != 0xffff)
                    });
                let max_clock = smbios_u16(record, 0x14)
                    .filter(|value| *value != 0 && *value != 0xffff)
                    .map(u64::from);
                let processor_id = if record.len() >= 0x10 {
                    Some(
                        record[0x08..0x10]
                            .iter()
                            .rev()
                            .map(|byte| format!("{byte:02X}"))
                            .collect::<Vec<_>>()
                            .join(""),
                    )
                } else {
                    None
                };
                SmbiosCpu {
                    socket: smbios_record_string(record, strings, 0x04),
                    manufacturer: smbios_record_string(record, strings, 0x07),
                    version: smbios_record_string(record, strings, 0x10),
                    processor_id,
                    max_clock_mhz: max_clock,
                    physical_cores: core_count,
                    logical_processors: thread_count,
                    l1_cache_bytes: cache_size(0x1a, 1),
                    l2_cache_bytes: cache_size(0x1c, 2),
                    l3_cache_bytes: cache_size(0x1e, 3),
                }
            })
            .collect()
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn cpuid_cpu_identity() -> (Option<String>, Option<String>, Vec<CpuInstructionSet>, bool) {
        #[cfg(target_arch = "x86")]
        use std::arch::x86::{__cpuid, __cpuid_count};
        #[cfg(target_arch = "x86_64")]
        use std::arch::x86_64::{__cpuid, __cpuid_count};

        let basic = unsafe { __cpuid(0) };
        let vendor = [
            basic.ebx.to_le_bytes(),
            basic.edx.to_le_bytes(),
            basic.ecx.to_le_bytes(),
        ]
        .concat();
        let vendor = String::from_utf8_lossy(&vendor).trim().to_string();

        let extended = unsafe { __cpuid(0x8000_0000) }.eax;
        let brand = if extended >= 0x8000_0004 {
            let mut bytes = Vec::with_capacity(48);
            for leaf in 0x8000_0002..=0x8000_0004 {
                let value = unsafe { __cpuid(leaf) };
                bytes.extend_from_slice(&value.eax.to_le_bytes());
                bytes.extend_from_slice(&value.ebx.to_le_bytes());
                bytes.extend_from_slice(&value.ecx.to_le_bytes());
                bytes.extend_from_slice(&value.edx.to_le_bytes());
            }
            let value = String::from_utf8_lossy(&bytes)
                .trim_matches(char::from(0))
                .trim()
                .to_string();
            (!value.is_empty()).then_some(value)
        } else {
            None
        };

        let mut sets = Vec::new();
        let leaf1 = unsafe { __cpuid(1) };
        let ecx = leaf1.ecx;
        let edx = leaf1.edx;
        if edx & (1 << 23) != 0 {
            sets.push(CpuInstructionSet::Mmx);
        }
        if edx & (1 << 25) != 0 {
            sets.push(CpuInstructionSet::Sse);
        }
        if edx & (1 << 26) != 0 {
            sets.push(CpuInstructionSet::Sse2);
        }
        if ecx & (1 << 0) != 0 {
            sets.push(CpuInstructionSet::Sse3);
        }
        if ecx & (1 << 9) != 0 {
            sets.push(CpuInstructionSet::Ssse3);
        }
        if ecx & (1 << 19) != 0 {
            sets.push(CpuInstructionSet::Sse41);
        }
        if ecx & (1 << 20) != 0 {
            sets.push(CpuInstructionSet::Sse42);
        }
        if ecx & (1 << 25) != 0 {
            sets.push(CpuInstructionSet::Aes);
        }
        if ecx & (1 << 28) != 0 {
            sets.push(CpuInstructionSet::Avx);
        }
        if ecx & (1 << 12) != 0 {
            sets.push(CpuInstructionSet::Fma);
        }
        if ecx & (1 << 29) != 0 {
            sets.push(CpuInstructionSet::F16c);
        }
        let mut virtualization = ecx & (1 << 5) != 0;
        if basic.eax >= 7 {
            let leaf7 = unsafe { __cpuid_count(7, 0) };
            if leaf7.ebx & (1 << 5) != 0 {
                sets.push(CpuInstructionSet::Avx2);
            }
            if leaf7.ebx & (1 << 3) != 0 {
                sets.push(CpuInstructionSet::Bmi1);
            }
            if leaf7.ebx & (1 << 8) != 0 {
                sets.push(CpuInstructionSet::Bmi2);
            }
            if leaf7.ebx & (1 << 16) != 0 {
                sets.push(CpuInstructionSet::Avx512F);
            }
            if leaf7.ebx & (1 << 29) != 0 {
                sets.push(CpuInstructionSet::Sha);
            }
        }
        if extended >= 0x8000_0001 {
            let ext1 = unsafe { __cpuid(0x8000_0001) };
            virtualization |= ext1.ecx & (1 << 2) != 0;
        }
        (Some(vendor), brand, sets, virtualization)
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    fn cpuid_cpu_identity() -> (Option<String>, Option<String>, Vec<CpuInstructionSet>, bool) {
        (None, None, Vec::new(), false)
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct CpuCacheSizes {
        l1_bytes: Option<u64>,
        l2_bytes: Option<u64>,
        l3_bytes: Option<u64>,
    }

    fn cache_sizes_from_cpuid(
        entries: &[(u32, u32, u32)],
        logical_processors: Option<u32>,
    ) -> CpuCacheSizes {
        let mut sizes = CpuCacheSizes::default();

        for &(eax, ebx, ecx) in entries {
            let cache_type = eax & 0x1f;
            if cache_type == 0 {
                break;
            }

            let level = (eax >> 5) & 0x07;
            let sharing_logical = ((eax >> 14) & 0x0fff).saturating_add(1);
            let line_size = (ebx & 0x0fff).saturating_add(1) as u64;
            let partitions = ((ebx >> 12) & 0x03ff).saturating_add(1) as u64;
            let ways = ((ebx >> 22) & 0x03ff).saturating_add(1) as u64;
            let sets = (ecx as u64).saturating_add(1);

            let Some(instance_size) = line_size
                .checked_mul(partitions)
                .and_then(|value| value.checked_mul(ways))
                .and_then(|value| value.checked_mul(sets))
            else {
                continue;
            };

            let instances = logical_processors
                .map(|count| count.div_ceil(sharing_logical.max(1)))
                .unwrap_or(1) as u64;
            let total_size = instance_size.saturating_mul(instances);

            let target = match (level, cache_type) {
                (1, 1..=3) => &mut sizes.l1_bytes,
                (2, 1..=3) => &mut sizes.l2_bytes,
                (3, 1..=3) => &mut sizes.l3_bytes,
                _ => continue,
            };
            *target = Some(target.unwrap_or(0).saturating_add(total_size));
        }

        sizes
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn cpuid_cpu_cache_sizes(logical_processors: Option<u32>) -> CpuCacheSizes {
        #[cfg(target_arch = "x86")]
        use std::arch::x86::{__cpuid, __cpuid_count};
        #[cfg(target_arch = "x86_64")]
        use std::arch::x86_64::{__cpuid, __cpuid_count};

        let basic = unsafe { __cpuid(0) };
        let vendor = [
            basic.ebx.to_le_bytes(),
            basic.edx.to_le_bytes(),
            basic.ecx.to_le_bytes(),
        ]
        .concat();
        let vendor = String::from_utf8_lossy(&vendor);
        let extended_max = unsafe { __cpuid(0x8000_0000) }.eax;

        let leaf = if vendor == "AuthenticAMD" && extended_max >= 0x8000_001d {
            0x8000_001d
        } else if basic.eax >= 4 {
            4
        } else {
            return CpuCacheSizes::default();
        };

        let mut entries = Vec::new();
        for subleaf in 0..32 {
            let value = unsafe { __cpuid_count(leaf, subleaf) };
            if value.eax & 0x1f == 0 {
                break;
            }
            entries.push((value.eax, value.ebx, value.ecx));
        }

        cache_sizes_from_cpuid(&entries, logical_processors)
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    fn cpuid_cpu_cache_sizes(_logical_processors: Option<u32>) -> CpuCacheSizes {
        CpuCacheSizes::default()
    }

    pub(super) fn collect_cpu_category() -> Result<CategoryCollection<CpuCategory>, String> {
        let started = Instant::now();
        let started_at = now_utc().ok_or_else(|| {
            "Não foi possível obter o horário UTC para iniciar a coleta da CPU.".to_string()
        })?;
        let table = smbios_table();
        let smbios_processors = table.as_deref().map(smbios_cpu_records).unwrap_or_default();
        let (cpuid_vendor, cpuid_brand, instruction_sets, virtualization_supported) =
            cpuid_cpu_identity();

        let mut system_info = std::mem::MaybeUninit::<SystemInfo>::zeroed();
        unsafe { GetNativeSystemInfo(system_info.as_mut_ptr()) };
        let system_info = unsafe { system_info.assume_init() };
        let (architecture, architecture_member) = match system_info.processor_architecture {
            0 => (CpuArchitecture::X86, "PROCESSOR_ARCHITECTURE_INTEL"),
            9 => (CpuArchitecture::X64, "PROCESSOR_ARCHITECTURE_AMD64"),
            5 => (CpuArchitecture::Arm, "PROCESSOR_ARCHITECTURE_ARM"),
            12 => (CpuArchitecture::Arm64, "PROCESSOR_ARCHITECTURE_ARM64"),
            6 => (CpuArchitecture::Itanium, "PROCESSOR_ARCHITECTURE_IA64"),
            _ => (CpuArchitecture::Unknown, "PROCESSOR_ARCHITECTURE_UNKNOWN"),
        };
        let logical_count =
            (system_info.number_of_processors > 0).then_some(system_info.number_of_processors);
        let cpuid_caches = cpuid_cpu_cache_sizes(logical_count);

        let processors = if smbios_processors.is_empty() {
            vec![SmbiosCpu::default()]
        } else {
            smbios_processors
        };
        let mut results = Vec::with_capacity(processors.len());
        for smbios in processors {
            let name = cpuid_brand.clone().or_else(|| smbios.version.clone());
            let manufacturer = cpuid_vendor.clone().or_else(|| smbios.manufacturer.clone());
            let physical_cores = smbios.physical_cores;
            let logical_processors = smbios.logical_processors.or(logical_count);
            let field = |value: Option<String>, member: &str, note: &str| {
                value
                    .map(|value| observed(value, "CPUID/SMBIOS", member, &started_at))
                    .unwrap_or_else(|| unavailable(&started_at, note))
            };
            let number = |value: Option<u64>, member: &str, note: &str| {
                value
                    .map(|value| observed(value, "SMBIOS Type 4", member, &started_at))
                    .unwrap_or_else(|| unavailable(&started_at, note))
            };
            let cache_number = |cpuid_value: Option<u64>,
                                smbios_value: Option<u64>,
                                member: &str,
                                note: &str| {
                if let Some(value) = cpuid_value {
                    observed(value, "CPUID deterministic cache parameters", member, &started_at)
                } else if let Some(value) = smbios_value {
                    observed(value, "SMBIOS Type 4/7", member, &started_at)
                } else {
                    unavailable(&started_at, note)
                }
            };
            let count = |value: Option<u32>, member: &str, note: &str| {
                value
                    .map(|value| {
                        observed(
                            value,
                            "SMBIOS Type 4 / kernel32!GetNativeSystemInfo",
                            member,
                            &started_at,
                        )
                    })
                    .unwrap_or_else(|| unavailable(&started_at, note))
            };
            let virtualization =
                observed(virtualization_supported, "CPUID", "VMX/SVM", &started_at);
            let firmware_enabled = unsafe { IsProcessorFeaturePresent(21) } != 0;
            let info = ProcessorInfo {
                name: field(
                    name,
                    "processor_brand_string",
                    "Nome do processador não informado por CPUID ou SMBIOS.",
                ),
                manufacturer: field(
                    manufacturer,
                    "vendor_id",
                    "Fabricante não informado por CPUID ou SMBIOS.",
                ),
                processor_id: field(
                    smbios.processor_id,
                    "Processor ID",
                    "Identificador do processador não informado pelo SMBIOS Type 4.",
                ),
                socket: field(
                    smbios.socket,
                    "Socket Designation",
                    "Soquete não informado pelo SMBIOS Type 4.",
                ),
                architecture: observed(
                    architecture.clone(),
                    "kernel32!GetNativeSystemInfo",
                    architecture_member,
                    &started_at,
                ),
                physical_cores: count(
                    physical_cores,
                    "Core Count",
                    "Quantidade de núcleos físicos não informada pelo SMBIOS Type 4.",
                ),
                logical_processors: count(
                    logical_processors,
                    "Thread Count / NumberOfProcessors",
                    "Quantidade de processadores lógicos não informada.",
                ),
                max_clock_mhz: number(
                    smbios.max_clock_mhz,
                    "Max Speed",
                    "Clock máximo não informado pelo SMBIOS Type 4.",
                ),
                l1_cache_bytes: cache_number(
                    cpuid_caches.l1_bytes,
                    smbios.l1_cache_bytes,
                    "L1 data + instruction caches",
                    "Cache L1 não informado por CPUID ou SMBIOS.",
                ),
                l2_cache_bytes: cache_number(
                    cpuid_caches.l2_bytes,
                    smbios.l2_cache_bytes,
                    "L2 cache capacity",
                    "Cache L2 não informado por CPUID ou SMBIOS.",
                ),
                l3_cache_bytes: cache_number(
                    cpuid_caches.l3_bytes,
                    smbios.l3_cache_bytes,
                    "L3 cache capacity",
                    "Cache L3 não informado por CPUID ou SMBIOS.",
                ),
                virtualization_supported: virtualization,
                virtualization_firmware_enabled: observed(
                    firmware_enabled,
                    "kernel32!IsProcessorFeaturePresent",
                    "PF_VIRT_FIRMWARE_ENABLED",
                    &started_at,
                ),
                instruction_sets: observed(
                    instruction_sets.clone(),
                    "CPUID",
                    "processor feature flags",
                    &started_at,
                ),
            };
            info.validate()?;
            results.push(info);
        }

        let warnings = if table.is_none() {
            vec!["Tabela SMBIOS indisponível; socket, caches, clock máximo e contagem física podem não ser informados.".into()]
        } else if results.iter().any(|processor| {
            processor.physical_cores.value.is_none() || processor.max_clock_mhz.value.is_none()
        }) {
            vec!["Alguns atributos da CPU não foram informados pelo firmware SMBIOS.".into()]
        } else {
            Vec::new()
        };

        let collection = CategoryCollection {
            state: InventoryValueState::Available,
            started_at: started_at.clone(),
            finished_at: now_utc().unwrap_or(started_at),
            duration_ms: started.elapsed().as_millis().max(1).min(u64::MAX as u128) as u64,
            data: Some(CpuCategory {
                processors: results,
            }),
            issues: Vec::<InventoryIssue>::new(),
            warnings,
        };
        collection.validate()?;
        Ok(collection)
    }

    pub(super) fn collect_firmware_category() -> Result<CategoryCollection<FirmwareCategory>, String>
    {
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
        let (tpm, tpm_warning) = collect_tpm(&started_at);

        let mut warnings = Vec::new();
        if table.is_none() {
            warnings.push("A tabela SMBIOS não pôde ser consultada; dados de placa-mãe e BIOS podem ficar indisponíveis.".into());
        }
        if let Some(warning) = tpm_warning {
            warnings.push(warning);
        }

        let data = FirmwareCategory {
            motherboard: MotherboardInfo {
                commercial_brand: unavailable(
                    &started_at,
                    "Marca comercial não identificada por uma fonte local confiável; não inferida do SMBIOS.",
                ),
                smbios_manufacturer: motherboard_manufacturer,
                oem_manufacturer: unavailable(
                    &started_at,
                    "Fabricante industrial/OEM não identificado por uma fonte verificável.",
                ),
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
        fn cpuid_cache_sizes_sum_cache_instances_across_logical_processors() {
            // L1D: 32 KiB/core; L1I: 32 KiB/core; L2: 512 KiB/core; L3: 16 MiB/package.
            let entries = [
                ((1 << 5) | (1 << 14) | 1, (31) | (0 << 12) | (7 << 22), 127),
                ((1 << 5) | (1 << 14) | 2, (31) | (0 << 12) | (7 << 22), 127),
                ((2 << 5) | (1 << 14) | 3, (63) | (0 << 12) | (7 << 22), 1023),
                ((3 << 5) | (12 << 14) | 3, (63) | (0 << 12) | (15 << 22), 16383),
            ];
            let caches = cache_sizes_from_cpuid(&entries, Some(12));

            assert_eq!(caches.l1_bytes, Some(6 * 64 * 1024));
            assert_eq!(caches.l2_bytes, Some(6 * 512 * 1024));
            assert_eq!(caches.l3_bytes, Some(16 * 1024 * 1024));
        }

        #[test]
        fn cpuid_cache_sizes_stop_at_invalid_cache_type() {
            let entries = [
                ((1 << 5) | (1 << 14) | 1, 31 | (7 << 22), 63),
                (0, 0, 0),
                ((3 << 5) | (1 << 14) | 3, 63 | (15 << 22), 16383),
            ];
            let caches = cache_sizes_from_cpuid(&entries, Some(2));

            assert_eq!(caches.l1_bytes, Some(32 * 1024));
            assert_eq!(caches.l2_bytes, None);
            assert_eq!(caches.l3_bytes, None);
        }

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
                2, 8, 0, 0, 1, 2, 3, 4, b'A', b'C', b'M', b'E', 0, b'P', b'C', b'H', b'-', b'B',
                b'5', b'5', b'0', b'M', 0, b'1', b'.', b'0', 0, b'S', b'N', b'1', b'2', b'3', 0, 0,
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
                0, 9, 0, 0, 1, 2, 0, 0, 3, b'A', b'M', b'I', 0, b'F', b'1', b'.', b'2', 0, b'1',
                b'1', b'/', b'1', b'7', b'/', b'2', b'0', b'2', b'6', 0, 0, 127, 4, 0, 0,
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
pub(crate) fn collect_cpu_category() -> Result<CategoryCollection<CpuCategory>, String> {
    windows::collect_cpu_category()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn collect_cpu_category() -> Result<CategoryCollection<CpuCategory>, String> {
    Err("O Inventário Técnico está disponível somente no Windows.".into())
}

#[cfg(target_os = "windows")]
pub(crate) fn collect_firmware_category() -> Result<CategoryCollection<FirmwareCategory>, String> {
    windows::collect_firmware_category()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn collect_firmware_category() -> Result<CategoryCollection<FirmwareCategory>, String> {
    Err("O Inventário Técnico está disponível somente no Windows.".into())
}
