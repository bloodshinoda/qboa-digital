//! Coletores nativos do Inventário Técnico Completo.
//!
//! Esta primeira etapa coleta somente Sistema e Equipamento. Não executa
//! PowerShell, não altera o Windows e não depende do módulo Capacidades.

use crate::technical_inventory::{
    CategoryCollection, DeviceKind, InventoryConfidence, InventoryIssue, InventorySource,
    InventoryValueState, MachineIdentity, Observed, OsArchitecture, SystemCategory,
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
        let os_version = registry_string("CurrentVersion", &started_at);
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

        let domain = read_registry_string(
            r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters",
            "Domain",
        );
        let domain_or_workgroup = domain
            .map(|value| {
                observed(
                    value,
                    "Windows Registry",
                    "Tcpip\\Parameters\\Domain",
                    &started_at,
                )
            })
            .unwrap_or_else(|| {
                unavailable(
                    &started_at,
                    "Domínio ou grupo de trabalho não identificado.",
                )
            });

        // A classificação do equipamento depende de evidência SMBIOS/WMI e será
        // preenchida pelo coletor de firmware/equipamento, sem inferência por nome.
        let device_kind = unavailable(
            &started_at,
            "Classificação depende da enumeração SMBIOS/Win32_ComputerSystem.",
        );

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
            manufacturer: unavailable(
                &started_at,
                "Fabricante será obtido pelo coletor de SMBIOS/Win32_ComputerSystem.",
            ),
            model: unavailable(
                &started_at,
                "Modelo será obtido pelo coletor de SMBIOS/Win32_ComputerSystem.",
            ),
            device_kind,
            is_virtual_machine: unavailable(
                &started_at,
                "Detecção de virtualização será implementada com evidência SMBIOS/WMI.",
            ),
        };

        let mut warnings = Vec::new();
        if data.os_name.value.is_none() || data.build_number.value.is_none() {
            warnings.push(
                "Identificação básica do Windows ficou incompleta; alguns valores do Registro não foram lidos."
                    .into(),
            );
        }
        let finished_at = now_utc().unwrap_or_else(|| started_at.clone());
        let duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
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
