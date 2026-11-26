//! Coleta nativa de discos, partições e volumes no Windows.
//!
//! Usa APIs Win32 de armazenamento e volume. Não executa PowerShell, não altera
//! o estado dos dispositivos e mantém falhas locais como avisos de coleta.

use crate::technical_inventory::{
    CategoryCollection, InventoryConfidence, InventoryIssue, InventorySource, InventoryValueState,
    Observed, PartitionInfo, PartitionStyle, PhysicalDiskInfo, Sensitive, SensitiveDataKind,
    StorageBusType, StorageCategory, StorageDeviceKind, StorageMediaType, VolumeInfo,
};
use std::time::Instant;

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    type Handle = isize;
    type Bool = i32;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[repr(C)]
    struct DeviceInterfaceData {
        cb_size: u32,
        interface_class_guid: Guid,
        flags: u32,
        reserved: usize,
    }

    #[repr(C)]
    struct DeviceInfoData {
        cb_size: u32,
        class_guid: Guid,
        dev_inst: u32,
        reserved: usize,
    }

    #[repr(C)]
    struct DevPropKey {
        fmtid: Guid,
        pid: u32,
    }

    const INVALID_HANDLE_VALUE: Handle = -1;
    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;

    const IOCTL_DISK_GET_DRIVE_GEOMETRY: u32 = 0x0007_0000;
    const IOCTL_DISK_GET_DRIVE_LAYOUT_EX: u32 = 0x0007_0050;
    const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007_405c;
    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002d_1400;
    const IOCTL_STORAGE_GET_DEVICE_NUMBER: u32 = 0x002d_1080;
    const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x0056_0000;

    #[link(name = "setupapi")]
    unsafe extern "system" {
        fn SetupDiGetClassDevsW(
            class_guid: *const Guid,
            enumerator: *const u16,
            hwnd_parent: Handle,
            flags: u32,
        ) -> Handle;
        fn SetupDiEnumDeviceInterfaces(
            device_info_set: Handle,
            device_info_data: *mut c_void,
            interface_class_guid: *const Guid,
            member_index: u32,
            device_interface_data: *mut DeviceInterfaceData,
        ) -> Bool;
        fn SetupDiGetDeviceInterfaceDetailW(
            device_info_set: Handle,
            device_interface_data: *mut DeviceInterfaceData,
            device_interface_detail_data: *mut c_void,
            device_interface_detail_data_size: u32,
            required_size: *mut u32,
            device_info_data: *mut DeviceInfoData,
        ) -> Bool;
        fn SetupDiDestroyDeviceInfoList(device_info_set: Handle) -> Bool;
    }

    #[link(name = "cfgmgr32")]
    unsafe extern "system" {
        fn CM_Get_DevNode_PropertyW(
            dev_inst: u32,
            property_key: *const DevPropKey,
            property_type: *mut u32,
            property_buffer: *mut u8,
            property_buffer_size: *mut u32,
            flags: u32,
        ) -> u32;
        fn CM_Get_Parent(parent: *mut u32, dev_inst: u32, flags: u32) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn CloseHandle(handle: Handle) -> Bool;
        fn GetLastError() -> u32;
        fn DeviceIoControl(
            handle: Handle,
            code: u32,
            input: *mut c_void,
            input_size: u32,
            output: *mut c_void,
            output_size: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> Bool;
        fn FindFirstVolumeW(name: *mut u16, length: u32) -> Handle;
        fn FindNextVolumeW(search: Handle, name: *mut u16, length: u32) -> Bool;
        fn FindVolumeClose(search: Handle) -> Bool;
        fn GetVolumeInformationW(
            root: *const u16,
            label: *mut u16,
            label_length: u32,
            serial: *mut u32,
            max_component: *mut u32,
            flags: *mut u32,
            filesystem: *mut u16,
            filesystem_length: u32,
        ) -> Bool;
        fn GetDiskFreeSpaceW(
            root: *const u16,
            sectors_per_cluster: *mut u32,
            bytes_per_sector: *mut u32,
            free_clusters: *mut u32,
            total_clusters: *mut u32,
        ) -> Bool;
        fn GetDiskFreeSpaceExW(
            root: *const u16,
            available: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> Bool;
        fn GetVolumePathNamesForVolumeNameW(
            volume: *const u16,
            paths: *mut u16,
            length: u32,
            required: *mut u32,
        ) -> Bool;
    }

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn now_utc() -> String {
        // FILETIME é contado em intervalos de 100 ns desde 1601-01-01 UTC.
        #[repr(C)]
        struct FileTime {
            low: u32,
            high: u32,
        }
        #[repr(C)]
        struct SystemTime {
            year: u16,
            month: u16,
            weekday: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            millis: u16,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetSystemTimeAsFileTime(value: *mut FileTime);
            fn FileTimeToSystemTime(value: *const FileTime, output: *mut SystemTime) -> Bool;
        }
        let mut file = FileTime { low: 0, high: 0 };
        let mut system = SystemTime {
            year: 0,
            month: 0,
            weekday: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            millis: 0,
        };
        unsafe {
            GetSystemTimeAsFileTime(&mut file);
            if FileTimeToSystemTime(&file, &mut system) == 0 {
                return "1970-01-01T00:00:00Z".into();
            }
        }
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            system.year, system.month, system.day, system.hour, system.minute, system.second
        )
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

    fn estimated<T>(value: T, provider: &str, member: &str, at: &str, note: &str) -> Observed<T> {
        Observed {
            value: Some(value),
            state: InventoryValueState::Available,
            confidence: InventoryConfidence::Estimated,
            source: Some(InventorySource {
                provider: provider.into(),
                member: Some(member.into()),
                native_code: None,
            }),
            collected_at: at.into(),
            note: Some(note.into()),
        }
    }

    struct Device(Handle);

    impl Device {
        fn open_with_error(path: &str) -> Result<Self, u32> {
            let path = wide(path);
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    GENERIC_READ,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    0,
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                Err(unsafe { GetLastError() })
            } else {
                Ok(Self(handle))
            }
        }

        fn open(path: &str) -> Option<Self> {
            Self::open_with_error(path).ok()
        }

        fn ioctl(&self, code: u32, output: &mut [u8]) -> Option<usize> {
            self.ioctl_with_input(code, &mut [], output)
        }

        fn ioctl_with_input(
            &self,
            code: u32,
            input: &mut [u8],
            output: &mut [u8],
        ) -> Option<usize> {
            self.ioctl_with_input_result(code, input, output).ok()
        }

        fn ioctl_with_input_result(
            &self,
            code: u32,
            input: &mut [u8],
            output: &mut [u8],
        ) -> Result<usize, u32> {
            let mut returned = 0u32;
            let ok = unsafe {
                DeviceIoControl(
                    self.0,
                    code,
                    if input.is_empty() {
                        ptr::null_mut()
                    } else {
                        input.as_mut_ptr().cast()
                    },
                    input.len() as u32,
                    output.as_mut_ptr().cast(),
                    output.len() as u32,
                    &mut returned,
                    ptr::null_mut(),
                )
            };
            if ok != 0 {
                Ok(returned as usize)
            } else {
                Err(unsafe { GetLastError() })
            }
        }
    }

    impl Drop for Device {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
        Some(u32::from_le_bytes(
            data.get(offset..offset + 4)?.try_into().ok()?,
        ))
    }

    fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
        Some(u64::from_le_bytes(
            data.get(offset..offset + 8)?.try_into().ok()?,
        ))
    }

    fn read_i64(data: &[u8], offset: usize) -> Option<i64> {
        Some(i64::from_le_bytes(
            data.get(offset..offset + 8)?.try_into().ok()?,
        ))
    }

    fn is_generic_manufacturer(value: &str) -> bool {
        let normalized = value.trim().to_ascii_lowercase();
        normalized.is_empty()
            || [
                "standard disk drives",
                "disk drives",
                "unidades de disco padrão",
                "generic",
                "microsoft",
                "msft",
                "unknown",
                "n/a",
            ]
            .iter()
            .any(|generic| normalized == *generic || normalized.contains(generic))
            || matches!(
                normalized.as_str(),
                "ata" | "scsi" | "nvme" | "usb" | "raid"
            )
    }

    const STORAGE_DEVICE_DESCRIPTOR_HEADER_SIZE: usize = 36;

    // STORAGE_DEVICE_DESCRIPTOR declara o tamanho da estrutura (Version) e o
    // tamanho total, incluindo strings (Size). Nunca interpretamos bytes além
    // do menor limite confirmado pelo Windows e pelo buffer recebido.
    fn validate_storage_device_descriptor(buffer: &[u8], returned: usize) -> Result<&[u8], String> {
        let available = returned.min(buffer.len());
        if available < STORAGE_DEVICE_DESCRIPTOR_HEADER_SIZE {
            return Err(format!(
                "cabeçalho truncado: retornado={available}, mínimo={STORAGE_DEVICE_DESCRIPTOR_HEADER_SIZE}"
            ));
        }

        let version =
            read_u32(buffer, 0).ok_or_else(|| "campo Version ausente no cabeçalho".to_string())?;
        let size =
            read_u32(buffer, 4).ok_or_else(|| "campo Size ausente no cabeçalho".to_string())?;

        if version < STORAGE_DEVICE_DESCRIPTOR_HEADER_SIZE as u32 {
            return Err(format!(
                "Version inválido: {version} (mínimo={STORAGE_DEVICE_DESCRIPTOR_HEADER_SIZE})"
            ));
        }
        if size < version {
            return Err(format!(
                "Size inconsistente: {size} menor que Version={version}"
            ));
        }
        if size as usize > available {
            return Err(format!(
                "descritor truncado: Version={version}, Size declarado={size}, bytes retornados={available}"
            ));
        }

        Ok(&buffer[..size as usize])
    }

    // Os offsets de STORAGE_DEVICE_DESCRIPTOR são ULONG (u32), relativos ao
    // início da estrutura. Zero significa campo ausente; offsets fora da área
    // de strings ou sem terminador NUL são inconsistências, não ausência normal.
    fn read_ansi_string(data: &[u8], offset_field: usize) -> Result<Option<String>, String> {
        let start = read_u32(data, offset_field)
            .ok_or_else(|| format!("campo de offset em {offset_field} está truncado"))?
            as usize;
        if start == 0 {
            return Ok(None);
        }
        let header_size =
            read_u32(data, 0).unwrap_or(STORAGE_DEVICE_DESCRIPTOR_HEADER_SIZE as u32) as usize;
        if start < header_size {
            return Err(format!(
                "offset {start} aponta para dentro do cabeçalho (mínimo={header_size})"
            ));
        }
        if start >= data.len() {
            return Err(format!(
                "offset {start} ultrapassa o tamanho validado do descritor ({})",
                data.len()
            ));
        }

        let tail = &data[start..];
        let end = tail
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| format!("string no offset {start} não termina em NUL"))?;
        let value = String::from_utf8_lossy(&tail[..end]).trim().to_string();
        Ok((!value.is_empty()).then_some(value))
    }

    fn read_descriptor_string(
        data: &[u8],
        offset_field: usize,
        field_name: &str,
        disk_number: u32,
        warnings: &mut Vec<String>,
    ) -> Option<String> {
        match read_ansi_string(data, offset_field) {
            Ok(value) => value,
            Err(reason) => {
                warnings.push(format!(
                    "Descritor de armazenamento do disco {disk_number}: {field_name} inválido ({reason})."
                ));
                None
            }
        }
    }

    fn classify_device_kind(
        bus: &StorageBusType,
        removable: Option<bool>,
        scsi_device_type: Option<u8>,
        media: &StorageMediaType,
        model: Option<&str>,
        vendor: Option<&str>,
    ) -> (StorageDeviceKind, bool) {
        let identity_hint = format!(
            "{} {}",
            model.unwrap_or_default(),
            vendor.unwrap_or_default()
        )
        .to_ascii_lowercase();
        let usb_flash_hint = *bus == StorageBusType::Usb
            && removable == Some(true)
            && !identity_hint.contains("reader")
            && [
                "flash",
                "thumb",
                "pendrive",
                "pen drive",
                "datatraveler",
                "cruzer",
                "jetflash",
                "usb disk",
            ]
            .iter()
            .any(|hint| identity_hint.contains(hint));

        let kind = if scsi_device_type == Some(5) {
            StorageDeviceKind::OpticalDrive
        } else if usb_flash_hint {
            StorageDeviceKind::UsbFlashDrive
        } else {
            match bus {
                StorageBusType::Sd | StorageBusType::Mmc => StorageDeviceKind::MemoryCard,
                StorageBusType::Virtual | StorageBusType::FileBackedVirtual => {
                    StorageDeviceKind::VirtualDisk
                }
                StorageBusType::Spaces => StorageDeviceKind::StorageSpaces,
                StorageBusType::Nvme | StorageBusType::Ufs => StorageDeviceKind::SolidStateDrive,
                StorageBusType::Scm => StorageDeviceKind::Other,
                StorageBusType::NvmeOf | StorageBusType::Iscsi | StorageBusType::Fibre => {
                    StorageDeviceKind::NetworkDisk
                }
                StorageBusType::Sata | StorageBusType::Ata => match media {
                    StorageMediaType::Hdd => StorageDeviceKind::HardDisk,
                    StorageMediaType::Ssd => StorageDeviceKind::SolidStateDrive,
                    _ => StorageDeviceKind::Unknown,
                },
                StorageBusType::Usb => {
                    if removable == Some(true) {
                        StorageDeviceKind::UsbRemovableStorage
                    } else {
                        match media {
                            StorageMediaType::Hdd => StorageDeviceKind::HardDisk,
                            StorageMediaType::Ssd => StorageDeviceKind::SolidStateDrive,
                            _ => StorageDeviceKind::Unknown,
                        }
                    }
                }
                _ => match media {
                    StorageMediaType::Hdd => StorageDeviceKind::HardDisk,
                    StorageMediaType::Ssd => StorageDeviceKind::SolidStateDrive,
                    _ => StorageDeviceKind::Unknown,
                },
            }
        };

        (kind, usb_flash_hint)
    }

    fn bus_type(value: u32) -> StorageBusType {
        match value {
            1 => StorageBusType::Scsi,
            2 => StorageBusType::Atapi,
            3 => StorageBusType::Ata,
            4 => StorageBusType::Ieee1394,
            5 => StorageBusType::Ssa,
            6 => StorageBusType::Fibre,
            7 => StorageBusType::Usb,
            8 => StorageBusType::Raid,
            9 => StorageBusType::Iscsi,
            10 => StorageBusType::Sas,
            11 => StorageBusType::Sata,
            12 => StorageBusType::Sd,
            13 => StorageBusType::Mmc,
            14 => StorageBusType::Virtual,
            15 => StorageBusType::FileBackedVirtual,
            16 => StorageBusType::Spaces,
            17 => StorageBusType::Nvme,
            18 => StorageBusType::Scm,
            19 => StorageBusType::Ufs,
            20 => StorageBusType::NvmeOf,
            _ => StorageBusType::Unknown,
        }
    }

    const GUID_DEVINTERFACE_DISK: Guid = Guid {
        data1: 0x53f56307,
        data2: 0xb6bf,
        data3: 0x11d0,
        data4: [0x94, 0xf2, 0x00, 0xa0, 0xc9, 0x1e, 0xfb, 0x8b],
    };
    const GUID_DEVPROP_DEVICE: Guid = Guid {
        data1: 0xa45c254e,
        data2: 0xdf1c,
        data3: 0x4efd,
        data4: [0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0],
    };
    const GUID_DEVPROP_PCI: Guid = Guid {
        data1: 0x3ab22e31,
        data2: 0x8264,
        data3: 0x4b4e,
        data4: [0x9a, 0xf5, 0xa8, 0xd2, 0xd8, 0xe3, 0x3e, 0x62],
    };

    fn devnode_property(dev_inst: u32, key: &DevPropKey) -> Option<Vec<u8>> {
        let mut property_type = 0u32;
        let mut required = 0u32;
        let first = unsafe {
            CM_Get_DevNode_PropertyW(
                dev_inst,
                key,
                &mut property_type,
                ptr::null_mut(),
                &mut required,
                0,
            )
        };
        if required == 0 || first == 0 {
            return None;
        }
        let mut buffer = vec![0u8; required as usize];
        let result = unsafe {
            CM_Get_DevNode_PropertyW(
                dev_inst,
                key,
                &mut property_type,
                buffer.as_mut_ptr(),
                &mut required,
                0,
            )
        };
        (result == 0).then(|| {
            buffer.truncate(required as usize);
            buffer
        })
    }

    fn devnode_string(dev_inst: u32, key: &DevPropKey) -> Option<String> {
        let bytes = devnode_property(dev_inst, key)?;
        let wide: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|value| *value != 0)
            .collect();
        let value = String::from_utf16_lossy(&wide).trim().to_string();
        (!value.is_empty()).then_some(value)
    }

    fn devnode_multi_string(dev_inst: u32, key: &DevPropKey) -> Vec<String> {
        let Some(bytes) = devnode_property(dev_inst, key) else {
            return Vec::new();
        };
        let wide: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        wide.split(|value| *value == 0)
            .take_while(|part| !part.is_empty())
            .map(String::from_utf16_lossy)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .take(16)
            .collect()
    }

    #[derive(Default)]
    struct PnpDiskMetadata {
        manufacturer: Option<String>,
        device_description: Option<String>,
        friendly_name: Option<String>,
        hardware_ids: Vec<String>,
        current_pcie_speed: Option<u32>,
        current_pcie_width: Option<u32>,
        max_pcie_speed: Option<u32>,
        max_pcie_width: Option<u32>,
    }

    fn collect_pnp_disk_metadata(target_disk_number: u32) -> PnpDiskMetadata {
        let mut metadata = PnpDiskMetadata::default();
        let set = unsafe {
            SetupDiGetClassDevsW(
                &GUID_DEVINTERFACE_DISK,
                ptr::null(),
                0,
                0x2 | 0x10, // DIGCF_PRESENT | DIGCF_DEVICEINTERFACE
            )
        };
        if set == INVALID_HANDLE_VALUE {
            return metadata;
        }

        let manufacturer_key = DevPropKey {
            fmtid: GUID_DEVPROP_DEVICE,
            pid: 13,
        };
        let device_description_key = DevPropKey {
            fmtid: GUID_DEVPROP_DEVICE,
            pid: 2,
        };
        let hardware_ids_key = DevPropKey {
            fmtid: GUID_DEVPROP_DEVICE,
            pid: 3,
        };
        let friendly_name_key = DevPropKey {
            fmtid: GUID_DEVPROP_DEVICE,
            pid: 14,
        };
        let current_speed_key = DevPropKey {
            fmtid: GUID_DEVPROP_PCI,
            pid: 9,
        };
        let current_width_key = DevPropKey {
            fmtid: GUID_DEVPROP_PCI,
            pid: 10,
        };
        let max_speed_key = DevPropKey {
            fmtid: GUID_DEVPROP_PCI,
            pid: 11,
        };
        let max_width_key = DevPropKey {
            fmtid: GUID_DEVPROP_PCI,
            pid: 12,
        };

        for index in 0..4096u32 {
            let mut interface = DeviceInterfaceData {
                cb_size: std::mem::size_of::<DeviceInterfaceData>() as u32,
                interface_class_guid: GUID_DEVINTERFACE_DISK,
                flags: 0,
                reserved: 0,
            };
            if unsafe {
                SetupDiEnumDeviceInterfaces(
                    set,
                    ptr::null_mut(),
                    &GUID_DEVINTERFACE_DISK,
                    index,
                    &mut interface,
                )
            } == 0
            {
                break;
            }

            let mut required = 0u32;
            let mut info = DeviceInfoData {
                cb_size: std::mem::size_of::<DeviceInfoData>() as u32,
                class_guid: Guid {
                    data1: 0,
                    data2: 0,
                    data3: 0,
                    data4: [0; 8],
                },
                dev_inst: 0,
                reserved: 0,
            };
            let _ = unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    set,
                    &mut interface,
                    ptr::null_mut(),
                    0,
                    &mut required,
                    &mut info,
                )
            };
            if required < 6 {
                continue;
            }
            let mut detail = vec![0u8; required as usize];
            detail[..4].copy_from_slice(
                &(if cfg!(target_pointer_width = "64") {
                    8u32
                } else {
                    6u32
                })
                .to_le_bytes(),
            );
            if unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    set,
                    &mut interface,
                    detail.as_mut_ptr().cast(),
                    required,
                    &mut required,
                    &mut info,
                )
            } == 0
            {
                continue;
            }
            let path_wide: Vec<u16> = detail[4..]
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .take_while(|value| *value != 0)
                .collect();
            let path = String::from_utf16_lossy(&path_wide);
            let Some(device) = Device::open(&path) else {
                continue;
            };
            let mut number_buffer = [0u8; 16];
            let Some(disk_number) = device
                .ioctl(IOCTL_STORAGE_GET_DEVICE_NUMBER, &mut number_buffer)
                .and_then(|_| read_u32(&number_buffer, 4))
            else {
                continue;
            };
            if disk_number != target_disk_number {
                continue;
            }

            metadata.manufacturer = devnode_string(info.dev_inst, &manufacturer_key);
            metadata.device_description = devnode_string(info.dev_inst, &device_description_key);
            metadata.friendly_name = devnode_string(info.dev_inst, &friendly_name_key);
            metadata.hardware_ids = devnode_multi_string(info.dev_inst, &hardware_ids_key);

            // O disco pode ser um nó SCSI filho de um controlador NVMe PCIe.
            // Percorremos a cadeia de pais para localizar as propriedades PCIe.
            let mut pcie_dev_inst = info.dev_inst;
            for _ in 0..8 {
                metadata.current_pcie_speed = devnode_property(pcie_dev_inst, &current_speed_key)
                    .and_then(|bytes| read_u32(&bytes, 0));
                metadata.current_pcie_width = devnode_property(pcie_dev_inst, &current_width_key)
                    .and_then(|bytes| read_u32(&bytes, 0));
                metadata.max_pcie_speed = devnode_property(pcie_dev_inst, &max_speed_key)
                    .and_then(|bytes| read_u32(&bytes, 0));
                metadata.max_pcie_width = devnode_property(pcie_dev_inst, &max_width_key)
                    .and_then(|bytes| read_u32(&bytes, 0));
                if metadata.current_pcie_speed.is_some() || metadata.max_pcie_speed.is_some() {
                    break;
                }
                let mut parent = 0u32;
                if unsafe { CM_Get_Parent(&mut parent, pcie_dev_inst, 0) } != 0
                    || parent == pcie_dev_inst
                {
                    break;
                }
                pcie_dev_inst = parent;
            }
            break;
        }

        unsafe {
            SetupDiDestroyDeviceInfoList(set);
        }
        metadata
    }

    fn pcie_speed_label(speed: u32, width: Option<u32>) -> Option<String> {
        let (generation, rate) = match speed {
            1 => (1, "2.5 GT/s"),
            2 => (2, "5.0 GT/s"),
            3 => (3, "8.0 GT/s"),
            4 => (4, "16.0 GT/s"),
            5 => (5, "32.0 GT/s"),
            6 => (6, "64.0 GT/s"),
            _ => return None,
        };
        Some(match width {
            Some(width) if width > 0 => format!("PCIe Gen{generation} x{width} ({rate})"),
            _ => format!("PCIe Gen{generation} ({rate})"),
        })
    }

    fn protocol_specific_data(
        device: &Device,
        protocol_type: u32,
        request_value: u32,
        data_length: u32,
    ) -> Option<Vec<u8>> {
        // STORAGE_PROPERTY_QUERY + STORAGE_PROTOCOL_SPECIFIC_DATA.
        // ProtocolDataOffset é relativo ao início de STORAGE_PROTOCOL_SPECIFIC_DATA.
        let mut input = [0u8; 48];
        input[0..4].copy_from_slice(&50u32.to_le_bytes()); // StorageDeviceProtocolSpecificProperty
        input[4..8].copy_from_slice(&0u32.to_le_bytes()); // PropertyStandardQuery
        input[8..12].copy_from_slice(&protocol_type.to_le_bytes());
        input[12..16].copy_from_slice(&1u32.to_le_bytes()); // Identify
        input[16..20].copy_from_slice(&request_value.to_le_bytes());
        input[24..28].copy_from_slice(&40u32.to_le_bytes());
        input[28..32].copy_from_slice(&data_length.to_le_bytes());
        let mut output = vec![0u8; 8192];
        let returned =
            device.ioctl_with_input(IOCTL_STORAGE_QUERY_PROPERTY, &mut input, &mut output)?;
        let payload_start = 8usize.checked_add(40)?;
        let payload_end = payload_start.checked_add(data_length as usize)?;
        (returned >= payload_end).then(|| output[payload_start..payload_end].to_vec())
    }

    fn ata_identify_word(data: &[u8], word: usize) -> Option<u16> {
        let offset = word.checked_mul(2)?;
        Some(u16::from_le_bytes(
            data.get(offset..offset + 2)?.try_into().ok()?,
        ))
    }

    fn partition_style(value: u32) -> PartitionStyle {
        match value {
            0 => PartitionStyle::Mbr,
            1 => PartitionStyle::Gpt,
            2 => PartitionStyle::Raw,
            _ => PartitionStyle::Unknown,
        }
    }

    fn query_storage_property(
        device: &Device,
        property_id: u32,
        output: &mut [u8],
    ) -> Option<usize> {
        let mut query = [0u8; 12];
        query[..4].copy_from_slice(&property_id.to_le_bytes());
        device.ioctl_with_input(IOCTL_STORAGE_QUERY_PROPERTY, &mut query, output)
    }

    fn collect_disk(
        number: u32,
        at: &str,
        warnings: &mut Vec<String>,
    ) -> Option<(PhysicalDiskInfo, Vec<PartitionInfo>)> {
        let path = format!(r"\\.\PhysicalDrive{number}");
        let device = match Device::open_with_error(&path) {
            Ok(device) => device,
            Err(error) => {
                // Ausência de um índice PhysicalDrive é normal. Outros erros
                // (por exemplo acesso negado ou dispositivo ocupado) são relevantes.
                if !matches!(error, 2 | 3 | 21) {
                    warnings.push(format!(
                        "Não foi possível abrir {path} (GetLastError={error}); os demais dispositivos continuarão sendo coletados."
                    ));
                }
                return None;
            }
        };

        let mut device_number_buffer = [0u8; 16];
        let actual_disk_number = device
            .ioctl(IOCTL_STORAGE_GET_DEVICE_NUMBER, &mut device_number_buffer)
            .and_then(|_| read_u32(&device_number_buffer, 4));
        let disk_number = actual_disk_number.unwrap_or(number);
        let pnp = collect_pnp_disk_metadata(disk_number);

        let mut length_buffer = [0u8; 8];
        let capacity = device
            .ioctl(IOCTL_DISK_GET_LENGTH_INFO, &mut length_buffer)
            .and_then(|_| read_i64(&length_buffer, 0))
            .filter(|value| *value > 0)
            .map(|value| value as u64);

        let mut geometry = [0u8; 32];
        let sector_size = device
            .ioctl(IOCTL_DISK_GET_DRIVE_GEOMETRY, &mut geometry)
            .and_then(|_| read_u32(&geometry, 20))
            .filter(|value| *value > 0);

        // StorageAccessAlignmentProperty = 6.
        let mut alignment = [0u8; 64];
        let alignment_length = query_storage_property(&device, 6, &mut alignment);
        let physical_sector_size = alignment_length
            .filter(|length| *length >= 24)
            .and_then(|_| read_u32(&alignment, 20))
            .filter(|value| *value > 0);
        let alignment_offset = alignment_length
            .filter(|length| *length >= 28)
            .and_then(|_| read_u32(&alignment, 24));

        // StorageDeviceSeekPenaltyProperty = 7; descriptor BOOL is at offset 8.
        let mut seek_penalty_buffer = [0u8; 16];
        let seek_penalty = query_storage_property(&device, 7, &mut seek_penalty_buffer)
            .filter(|length| *length >= 9)
            .map(|_| seek_penalty_buffer[8] != 0);

        // StorageDeviceTrimProperty = 8; descriptor BOOL is at offset 8.
        let mut trim_buffer = [0u8; 16];
        let trim_supported = query_storage_property(&device, 8, &mut trim_buffer)
            .filter(|length| *length >= 9)
            .map(|_| trim_buffer[8] != 0);

        // STORAGE_PROPERTY_QUERY: PropertyId=StorageDeviceProperty, QueryType=PropertyStandardQuery.
        // STORAGE_PROPERTY_QUERY inclui AdditionalParameters[1], totalizando 12 bytes.
        let mut query = [0u8; 12];
        let mut descriptor = vec![0u8; 4096];
        let descriptor_result = device.ioctl_with_input_result(
            IOCTL_STORAGE_QUERY_PROPERTY,
            &mut query,
            &mut descriptor,
        );
        let descriptor_length = match descriptor_result {
            Ok(length) if length >= 36 => Some(length.min(descriptor.len())),
            Ok(length) => {
                warnings.push(format!(
                    "Descritor de armazenamento do disco {number} incompleto: Windows retornou {length} bytes (mínimo esperado: 36)."
                ));
                None
            }
            Err(error) => {
                warnings.push(format!(
                    "IOCTL_STORAGE_QUERY_PROPERTY falhou no disco {number} (GetLastError={error})."
                ));
                None
            }
        };
        let descriptor_data = descriptor_length.and_then(|length| {
            match validate_storage_device_descriptor(&descriptor, length) {
                Ok(data) => Some(data),
                Err(reason) => {
                    let version = read_u32(&descriptor, 0);
                    let size = read_u32(&descriptor, 4);
                    warnings.push(format!(
                        "Descritor de armazenamento do disco {number} inválido: {reason} (Version={version:?}, Size={size:?}, bytes retornados={length})."
                    ));
                    None
                }
            }
        });
        let descriptor_vendor = descriptor_data
            .and_then(|data| read_descriptor_string(data, 12, "VendorIdOffset", number, warnings));
        let model = descriptor_data
            .and_then(|data| read_descriptor_string(data, 16, "ProductIdOffset", number, warnings));
        let firmware = descriptor_data.and_then(|data| {
            read_descriptor_string(data, 20, "ProductRevisionOffset", number, warnings)
        });
        let serial = descriptor_data.and_then(|data| {
            read_descriptor_string(data, 24, "SerialNumberOffset", number, warnings)
        });
        let vendor = descriptor_vendor
            .clone()
            .filter(|value| !is_generic_manufacturer(value))
            .or_else(|| {
                pnp.manufacturer
                    .clone()
                    .filter(|value| !is_generic_manufacturer(value))
            });
        let manufacturer_note =
            "Fabricante não identificado por fontes confiáveis disponíveis.".to_string();
        if vendor.is_none() {
            warnings.push(format!(
                "Diagnóstico do fabricante do disco {number}: VendorIdOffset={:?}, VendorId={:?}, DEVPKEY_Device_Manufacturer={:?}, DEVPKEY_Device_FriendlyName={:?}, DEVPKEY_Device_DeviceDesc={:?}, DEVPKEY_Device_HardwareIds={:?}.",
                descriptor_data.and_then(|data| read_u32(data, 12)),
                descriptor_vendor,
                pnp.manufacturer,
                pnp.friendly_name,
                pnp.device_description,
                pnp.hardware_ids
            ));
        }
        let bus_raw = descriptor_data.and_then(|data| read_u32(data, 28));
        let bus = bus_raw.map(bus_type);
        let removable = descriptor_data.map(|data| data.get(10).copied().unwrap_or(0) != 0);
        let scsi_device_type = descriptor_data.and_then(|data| data.get(8).copied());

        let ata_identify = if matches!(
            bus.as_ref(),
            Some(StorageBusType::Ata | StorageBusType::Sata)
        ) {
            protocol_specific_data(&device, 2, 0, 512)
        } else {
            None
        };
        let nvme_identify = if matches!(bus.as_ref(), Some(StorageBusType::Nvme)) {
            protocol_specific_data(&device, 3, 1, 4096)
        } else {
            None
        };
        let ata_current_speed = ata_identify
            .as_deref()
            .and_then(|data| ata_identify_word(data, 77))
            .map(|word| (word >> 1) & 0x7)
            .and_then(|code| match code {
                1 => Some("SATA Gen1 (1,5 Gb/s)".to_string()),
                2 => Some("SATA Gen2 (3,0 Gb/s)".to_string()),
                3 => Some("SATA Gen3 (6,0 Gb/s)".to_string()),
                _ => None,
            });
        let is_nvme = matches!(bus.as_ref(), Some(StorageBusType::Nvme));
        let pcie_current_speed = if is_nvme {
            pcie_speed_label(pnp.current_pcie_speed.unwrap_or(0), pnp.current_pcie_width)
        } else {
            None
        };
        let pcie_max_speed = if is_nvme {
            pcie_speed_label(pnp.max_pcie_speed.unwrap_or(0), pnp.max_pcie_width)
        } else {
            None
        };
        let ata_max_speed = ata_identify
            .as_deref()
            .and_then(|data| ata_identify_word(data, 76))
            .and_then(|word| {
                if word == 0 || word == u16::MAX {
                    None
                } else if word & (1 << 3) != 0 {
                    Some("SATA até Gen3 (6,0 Gb/s)".to_string())
                } else if word & (1 << 2) != 0 {
                    Some("SATA até Gen2 (3,0 Gb/s)".to_string())
                } else if word & (1 << 1) != 0 {
                    Some("SATA até Gen1 (1,5 Gb/s)".to_string())
                } else {
                    None
                }
            });
        let ata_rotation = ata_identify
            .as_deref()
            .and_then(|data| ata_identify_word(data, 217));
        let nvme_protocol_version = nvme_identify
            .as_deref()
            .and_then(|data| read_u32(data, 80))
            .filter(|version| *version != 0 && *version != u32::MAX)
            .map(|version| {
                format!(
                    "NVMe {}.{}",
                    (version >> 16) & 0xffff,
                    (version >> 8) & 0xff
                )
            });

        let mut layout = vec![0u8; 1024 * 1024];
        let layout_length = device.ioctl(IOCTL_DISK_GET_DRIVE_LAYOUT_EX, &mut layout);
        let mut partitions = Vec::new();
        let style = layout_length
            .filter(|length| *length >= 48)
            .and_then(|_| read_u32(&layout, 0))
            .map(partition_style);

        if let Some(length) = layout_length.filter(|length| *length >= 48) {
            let count = read_u32(&layout, 4).unwrap_or(0).min(4096) as usize;
            let entry_size = 144usize;
            for index in 0..count {
                let offset = 48usize.saturating_add(index.saturating_mul(entry_size));
                if offset + 32 > length {
                    break;
                }
                let Some(partition_number) = read_u32(&layout, offset + 24) else {
                    continue;
                };
                let start = read_i64(&layout, offset + 8)
                    .filter(|value| *value >= 0)
                    .map(|v| v as u64);
                let size = read_i64(&layout, offset + 16)
                    .filter(|value| *value > 0)
                    .map(|v| v as u64);

                // DRIVE_LAYOUT_INFORMATION_EX também reserva entradas para slots
                // livres. Elas vêm com PartitionNumber/Length zerados e não são
                // partições reais; incluí-las cria IDs repetidos (partition:0).
                if partition_number == 0 || start.is_none() || size.is_none() {
                    continue;
                }

                let style_value = read_u32(&layout, offset).unwrap_or(2);
                let partition_type = match style_value {
                    0 => layout
                        .get(offset + 32)
                        .map(|value| format!("MBR 0x{value:02X}")),
                    1 if offset + 48 <= length => {
                        let guid = &layout[offset + 32..offset + 48];
                        Some(format!(
                            "GPT {{{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
                            guid[3], guid[2], guid[1], guid[0], guid[5], guid[4], guid[7], guid[6],
                            guid[8], guid[9], guid[10], guid[11], guid[12], guid[13], guid[14], guid[15]
                        ))
                    }
                    _ => None,
                };
                let boot = if style_value == 0 {
                    layout.get(offset + 33).map(|value| *value != 0)
                } else {
                    None
                };
                let system = partition_type.as_deref().is_some_and(|value| {
                    value.contains("0xEF") || value.contains("C12A7328-F81F-11D2-BA4B-00A0C93EC93B")
                });
                let partition_id = format!("disk:{disk_number}:partition:{partition_number}");
                partitions.push(PartitionInfo {
                    partition_id,
                    disk_id: format!("disk:{disk_number}"),
                    partition_number: observed(
                        partition_number,
                        "DeviceIoControl(IOCTL_DISK_GET_DRIVE_LAYOUT_EX)",
                        "PartitionNumber",
                        at,
                    ),
                    offset_bytes: start
                        .map(|value| {
                            observed(
                                value,
                                "DeviceIoControl(IOCTL_DISK_GET_DRIVE_LAYOUT_EX)",
                                "StartingOffset",
                                at,
                            )
                        })
                        .unwrap_or_else(|| unavailable(at, "Offset da partição indisponível.")),
                    size_bytes: size
                        .map(|value| {
                            observed(
                                value,
                                "DeviceIoControl(IOCTL_DISK_GET_DRIVE_LAYOUT_EX)",
                                "PartitionLength",
                                at,
                            )
                        })
                        .unwrap_or_else(|| unavailable(at, "Tamanho da partição indisponível.")),
                    partition_type: partition_type
                        .map(|value| {
                            observed(
                                value,
                                "DeviceIoControl(IOCTL_DISK_GET_DRIVE_LAYOUT_EX)",
                                "PartitionType",
                                at,
                            )
                        })
                        .unwrap_or_else(|| unavailable(at, "Tipo da partição indisponível.")),
                    is_boot: boot
                        .map(|value| {
                            observed(
                                value,
                                "DeviceIoControl(IOCTL_DISK_GET_DRIVE_LAYOUT_EX)",
                                "BootIndicator",
                                at,
                            )
                        })
                        .unwrap_or_else(|| {
                            unavailable(
                                at,
                                "Indicador de inicialização não aplicável ou indisponível.",
                            )
                        }),
                    is_system: observed(
                        system,
                        "QBoa/Windows partition layout",
                        "system_partition_type",
                        at,
                    ),
                });
            }
        } else {
            warnings.push(format!(
                "O Windows não forneceu o layout de partições do disco {number}."
            ));
        }

        let bus = bus.unwrap_or(StorageBusType::Unknown);

        // A penalidade de busca é uma evidência de mídia magnética vs. flash,
        // mas não substitui a identificação específica do protocolo.
        let media = if matches!(
            &bus,
            StorageBusType::Virtual | StorageBusType::FileBackedVirtual
        ) {
            StorageMediaType::Unknown
        } else if bus == StorageBusType::Scm {
            StorageMediaType::Scm
        } else if scsi_device_type == Some(5) {
            StorageMediaType::Unknown
        } else {
            match seek_penalty {
                Some(true) => StorageMediaType::Hdd,
                Some(false) => StorageMediaType::Ssd,
                None => match ata_rotation {
                    Some(1) => StorageMediaType::Ssd,
                    Some(value) if (0x0401..=0xfffe).contains(&value) => StorageMediaType::Hdd,
                    _ if matches!(
                        &bus,
                        StorageBusType::Nvme
                            | StorageBusType::Sd
                            | StorageBusType::Mmc
                            | StorageBusType::Ufs
                    ) =>
                    {
                        StorageMediaType::Ssd
                    }
                    _ => StorageMediaType::Unknown,
                },
            }
        };

        let (device_kind, usb_flash_hint) = classify_device_kind(
            &bus,
            removable,
            scsi_device_type,
            &media,
            model.as_deref(),
            vendor.as_deref(),
        );

        let device_kind_observed = if usb_flash_hint {
            estimated(
                device_kind,
                "QBoa/storage descriptor heuristic",
                "removable USB flash product identity",
                at,
                "Classificação estimada por barramento USB, mídia removível e identificador comercial; leitores de cartão e dispositivos híbridos podem exigir confirmação.",
            )
        } else {
            observed(
                device_kind,
                "QBoa/storage bus + media classification",
                "device_kind",
                at,
            )
        };

        let disk = PhysicalDiskInfo {
            disk_id: format!("disk:{disk_number}"),
            disk_number: actual_disk_number
                .map(|value| {
                    observed(
                        value,
                        "DeviceIoControl(IOCTL_STORAGE_GET_DEVICE_NUMBER)",
                        "DeviceNumber",
                        at,
                    )
                })
                .unwrap_or_else(|| {
                    observed(number, "PhysicalDrive enumeration", "device_path_index", at)
                }),
            model: model
                .map(|value| {
                    observed(
                        value,
                        "DeviceIoControl(IOCTL_STORAGE_QUERY_PROPERTY)",
                        "ProductId",
                        at,
                    )
                })
                .unwrap_or_else(|| {
                    unavailable(at, "Modelo não informado pelo descritor do dispositivo.")
                }),
            manufacturer: vendor
                .map(|value| {
                    observed(
                        value,
                        if descriptor_data
                            .is_some_and(|data| matches!(read_ansi_string(data, 12), Ok(Some(_))))
                        {
                            "DeviceIoControl(IOCTL_STORAGE_QUERY_PROPERTY)"
                        } else {
                            "Configuration Manager / DEVPKEY_Device_Manufacturer"
                        },
                        "Manufacturer",
                        at,
                    )
                })
                .unwrap_or_else(|| unavailable(at, &manufacturer_note)),
            serial_number: serial
                .filter(|value| !value.trim().is_empty())
                .map(|value| {
                    observed(
                        Sensitive {
                            value,
                            classification: SensitiveDataKind::HardwareSerial,
                        },
                        "DeviceIoControl(IOCTL_STORAGE_QUERY_PROPERTY)",
                        "SerialNumber",
                        at,
                    )
                })
                .unwrap_or_else(|| {
                    unavailable(at, "Número de série não informado pelo dispositivo.")
                }),
            firmware_revision: firmware
                .map(|value| {
                    observed(
                        value,
                        "DeviceIoControl(IOCTL_STORAGE_QUERY_PROPERTY)",
                        "ProductRevision",
                        at,
                    )
                })
                .unwrap_or_else(|| {
                    unavailable(at, "Firmware não informado pelo descritor do dispositivo.")
                }),
            interface_type: bus_raw
                .map(|value| {
                    observed(
                        format!("Windows STORAGE_BUS_TYPE ({value})"),
                        "DeviceIoControl(IOCTL_STORAGE_QUERY_PROPERTY)",
                        "BusType",
                        at,
                    )
                })
                .unwrap_or_else(|| {
                    unavailable(at, "Interface não informada pelo descritor do dispositivo.")
                }),
            media_type: if matches!(
                &bus,
                StorageBusType::Virtual | StorageBusType::FileBackedVirtual
            ) {
                unavailable(
                    at,
                    "Disco virtual não expõe o tipo de mídia do armazenamento subjacente.",
                )
            } else if scsi_device_type == Some(5) {
                unavailable(
                    at,
                    "Unidade óptica não possui classificação de mídia HDD/SSD.",
                )
            } else if seek_penalty.is_some()
                || matches!(ata_rotation, Some(1..=65534))
                || matches!(
                    &bus,
                    StorageBusType::Nvme
                        | StorageBusType::Sd
                        | StorageBusType::Mmc
                        | StorageBusType::Ufs
                        | StorageBusType::Scm
                )
            {
                observed(
                    media,
                    "IOCTL_STORAGE_QUERY_PROPERTY / ATA IDENTIFY DEVICE / storage bus",
                    "seek_penalty / nominal_media_rotation_rate / BusType",
                    at,
                )
            } else {
                unavailable(
                    at,
                    "Tipo de mídia não pôde ser determinado pelas propriedades disponíveis.",
                )
            },
            device_kind: device_kind_observed,
            bus_type: bus_raw
                .map(|value| {
                    observed(
                        bus.clone(),
                        "DeviceIoControl(IOCTL_STORAGE_QUERY_PROPERTY)",
                        "BusType",
                        at,
                    )
                })
                .unwrap_or_else(|| {
                    unavailable(at, "Tipo de barramento não informado pelo dispositivo.")
                }),
            interface_generation: ata_current_speed
                .map(|value| {
                    observed(
                        value,
                        "ATA IDENTIFY DEVICE",
                        "SATA current signaling speed (word 77)",
                        at,
                    )
                })
                .or_else(|| {
                    pcie_current_speed.map(|value| {
                        observed(
                            value,
                            "Configuration Manager / DEVPKEY_PciDevice_CurrentLinkSpeed",
                            "PCIe current link speed + width",
                            at,
                        )
                    })
                })
                .unwrap_or_else(|| {
                    unavailable(
                        at,
                        "Geração negociada da interface não foi exposta pelo dispositivo/driver.",
                    )
                }),
            interface_max_generation: ata_max_speed
                .map(|value| {
                    observed(
                        value,
                        "ATA IDENTIFY DEVICE",
                        "SATA capabilities (word 76)",
                        at,
                    )
                })
                .or_else(|| {
                    pcie_max_speed.map(|value| {
                        observed(
                            value,
                            "Configuration Manager / DEVPKEY_PciDevice_MaxLinkSpeed",
                            "PCIe maximum link speed + width",
                            at,
                        )
                    })
                })
                .unwrap_or_else(|| {
                    unavailable(at, "Capacidade máxima da interface não foi informada.")
                }),
            protocol_version: nvme_protocol_version
                .map(|value| observed(value, "NVMe Identify Controller", "VER", at))
                .unwrap_or_else(|| {
                    unavailable(
                        at,
                        "Versão do protocolo não foi informada pelo dispositivo.",
                    )
                }),
            seek_penalty: seek_penalty
                .map(|value| {
                    observed(
                        value,
                        "IOCTL_STORAGE_QUERY_PROPERTY",
                        "IncursSeekPenalty",
                        at,
                    )
                })
                .unwrap_or_else(|| unavailable(at, "Windows não informou a penalidade de busca.")),
            trim_supported: trim_supported
                .map(|value| observed(value, "IOCTL_STORAGE_QUERY_PROPERTY", "TrimEnabled", at))
                .unwrap_or_else(|| unavailable(at, "Suporte a TRIM indisponível.")),
            alignment_offset_bytes: alignment_offset
                .map(|value| {
                    observed(
                        value,
                        "IOCTL_STORAGE_QUERY_PROPERTY",
                        "BytesOffsetForSectorAlignment",
                        at,
                    )
                })
                .unwrap_or_else(|| unavailable(at, "Deslocamento de alinhamento indisponível.")),
            capacity_bytes: capacity
                .map(|value| {
                    observed(
                        value,
                        "DeviceIoControl(IOCTL_DISK_GET_LENGTH_INFO)",
                        "Length",
                        at,
                    )
                })
                .unwrap_or_else(|| unavailable(at, "Capacidade não informada pelo dispositivo.")),
            logical_sector_bytes: sector_size
                .map(|value| {
                    observed(
                        value,
                        "DeviceIoControl(IOCTL_DISK_GET_DRIVE_GEOMETRY)",
                        "BytesPerSector",
                        at,
                    )
                })
                .unwrap_or_else(|| unavailable(at, "Tamanho do setor lógico indisponível.")),
            physical_sector_bytes: physical_sector_size
                .map(|value| {
                    observed(
                        value,
                        "IOCTL_STORAGE_QUERY_PROPERTY",
                        "BytesPerPhysicalSector",
                        at,
                    )
                })
                .unwrap_or_else(|| unavailable(at, "Tamanho do setor físico indisponível.")),
            is_removable: removable
                .map(|value| {
                    observed(
                        value,
                        "DeviceIoControl(IOCTL_STORAGE_QUERY_PROPERTY)",
                        "RemovableMedia",
                        at,
                    )
                })
                .unwrap_or_else(|| unavailable(at, "Indicador de mídia removível indisponível.")),
            partition_style: style
                .map(|value| {
                    observed(
                        value,
                        "DeviceIoControl(IOCTL_DISK_GET_DRIVE_LAYOUT_EX)",
                        "PartitionStyle",
                        at,
                    )
                })
                .unwrap_or_else(|| unavailable(at, "Estilo de partição indisponível.")),
        };
        Some((disk, partitions))
    }

    fn volume_mount_paths(volume: &str) -> Vec<String> {
        let volume_wide = wide(volume);
        let mut buffer = vec![0u16; 32768];
        let mut required = 0u32;
        let ok = unsafe {
            GetVolumePathNamesForVolumeNameW(
                volume_wide.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut required,
            )
        };
        if ok == 0 && required as usize > buffer.len() {
            buffer.resize(required as usize, 0);
            let _ = unsafe {
                GetVolumePathNamesForVolumeNameW(
                    volume_wide.as_ptr(),
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    &mut required,
                )
            };
        }
        let mut paths = Vec::new();
        let mut start = 0usize;
        while start < buffer.len() && buffer[start] != 0 {
            let end = buffer[start..]
                .iter()
                .position(|value| *value == 0)
                .map(|offset| start + offset)
                .unwrap_or(buffer.len());
            paths.push(String::from_utf16_lossy(&buffer[start..end]));
            start = end + 1;
        }
        paths
    }

    fn volume_extents(device: &Device, at: &str) -> (Vec<u32>, Vec<(u32, u64, u64)>) {
        let mut buffer = vec![0u8; 64 * 1024];
        let Some(length) = device.ioctl(IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, &mut buffer) else {
            return (Vec::new(), Vec::new());
        };
        let count = read_u32(&buffer, 0).unwrap_or(0).min(1024) as usize;
        let mut disk_numbers = Vec::new();
        let mut extents = Vec::new();
        for index in 0..count {
            let offset = 8 + index * 24;
            if offset + 24 > length {
                break;
            }
            let Some(disk) = read_u32(&buffer, offset) else {
                continue;
            };
            let start = read_i64(&buffer, offset + 8)
                .filter(|value| *value >= 0)
                .unwrap_or(0) as u64;
            let size = read_i64(&buffer, offset + 16)
                .filter(|value| *value >= 0)
                .unwrap_or(0) as u64;
            if !disk_numbers.contains(&disk) {
                disk_numbers.push(disk);
            }
            extents.push((disk, start, size));
        }
        let _ = at;
        (disk_numbers, extents)
    }

    fn collect_volumes(
        at: &str,
        disks: &[PhysicalDiskInfo],
        partitions: &[PartitionInfo],
        warnings: &mut Vec<String>,
    ) -> Vec<VolumeInfo> {
        let mut volumes = Vec::new();
        let mut name_buffer = vec![0u16; 1024];
        let search =
            unsafe { FindFirstVolumeW(name_buffer.as_mut_ptr(), name_buffer.len() as u32) };
        if search == INVALID_HANDLE_VALUE {
            warnings.push("O Windows não permitiu enumerar os volumes montados.".into());
            return volumes;
        }

        let mut current = String::from_utf16_lossy(
            &name_buffer[..name_buffer
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(name_buffer.len())],
        );
        loop {
            let paths = volume_mount_paths(&current);
            let root = wide(&current);
            let mut label_buffer = vec![0u16; 512];
            let mut filesystem_buffer = vec![0u16; 128];
            let mut serial = 0u32;
            let mut max_component = 0u32;
            let mut flags = 0u32;
            let info_ok = unsafe {
                GetVolumeInformationW(
                    root.as_ptr(),
                    label_buffer.as_mut_ptr(),
                    label_buffer.len() as u32,
                    &mut serial,
                    &mut max_component,
                    &mut flags,
                    filesystem_buffer.as_mut_ptr(),
                    filesystem_buffer.len() as u32,
                )
            } != 0;
            let label = if info_ok {
                let end = label_buffer
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(label_buffer.len());
                Some(String::from_utf16_lossy(&label_buffer[..end]))
            } else {
                None
            };
            let filesystem = if info_ok {
                let end = filesystem_buffer
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(filesystem_buffer.len());
                Some(String::from_utf16_lossy(&filesystem_buffer[..end]))
            } else {
                None
            };

            let mut available = 0u64;
            let mut capacity = 0u64;
            let mut free = 0u64;
            let space_ok = unsafe {
                GetDiskFreeSpaceExW(root.as_ptr(), &mut available, &mut capacity, &mut free)
            } != 0;
            let mut sectors_per_cluster = 0u32;
            let mut bytes_per_sector = 0u32;
            let mut free_clusters = 0u32;
            let mut total_clusters = 0u32;
            let allocation = if unsafe {
                GetDiskFreeSpaceW(
                    root.as_ptr(),
                    &mut sectors_per_cluster,
                    &mut bytes_per_sector,
                    &mut free_clusters,
                    &mut total_clusters,
                )
            } != 0
            {
                sectors_per_cluster
                    .checked_mul(bytes_per_sector)
                    .filter(|value| *value > 0)
            } else {
                None
            };

            let open_path = current.trim_end_matches('\\');
            let extent_device = Device::open(open_path);
            let (disk_numbers, extents) = extent_device
                .as_ref()
                .map(|device| volume_extents(device, at))
                .unwrap_or_default();
            let associated_disk_ids: Vec<String> = disk_numbers
                .iter()
                .filter(|number| {
                    disks
                        .iter()
                        .any(|disk| disk.disk_number.value == Some(**number))
                })
                .map(|number| format!("disk:{number}"))
                .collect();
            let mut associated_partition_ids = Vec::new();
            for (disk_number, extent_start, extent_size) in extents {
                let Some(extent_end) = extent_start.checked_add(extent_size) else {
                    continue;
                };
                for partition in partitions
                    .iter()
                    .filter(|partition| partition.disk_id == format!("disk:{disk_number}"))
                {
                    let (Some(start), Some(size)) =
                        (partition.offset_bytes.value, partition.size_bytes.value)
                    else {
                        continue;
                    };
                    let Some(partition_end) = start.checked_add(size) else {
                        continue;
                    };
                    if extent_start < partition_end
                        && start < extent_end
                        && !associated_partition_ids.contains(&partition.partition_id)
                    {
                        associated_partition_ids.push(partition.partition_id.clone());
                    }
                }
            }

            let drive_letter = paths
                .iter()
                .find(|path| path.len() >= 2 && path.as_bytes().get(1) == Some(&b':'))
                .map(|path| path[..2].to_string());
            volumes.push(VolumeInfo {
                volume_id: observed(current.clone(), "kernel32!FindFirstVolumeW", "Volume GUID path", at),
                drive_letter: drive_letter
                    .map(|value| observed(value, "kernel32!GetVolumePathNamesForVolumeNameW", "drive_letter", at))
                    .unwrap_or_else(|| unavailable(at, "Volume sem letra de unidade; pode usar apenas ponto de montagem.")),
                mount_points: paths,
                label: label
                    .map(|value| observed(value, "kernel32!GetVolumeInformationW", "VolumeName", at))
                    .unwrap_or_else(|| unavailable(at, "Rótulo do volume indisponível ou volume sem sistema de arquivos reconhecido.")),
                file_system: filesystem
                    .map(|value| observed(value, "kernel32!GetVolumeInformationW", "FileSystemName", at))
                    .unwrap_or_else(|| unavailable(at, "Sistema de arquivos indisponível.")),
                capacity_bytes: space_ok
                    .then(|| observed(capacity, "kernel32!GetDiskFreeSpaceExW", "TotalNumberOfBytes", at))
                    .unwrap_or_else(|| unavailable(at, "Capacidade do volume indisponível.")),
                free_bytes: space_ok
                    .then(|| observed(free, "kernel32!GetDiskFreeSpaceExW", "TotalNumberOfFreeBytes", at))
                    .unwrap_or_else(|| unavailable(at, "Espaço livre do volume indisponível.")),
                allocation_unit_bytes: allocation
                    .map(|value| observed(value, "kernel32!GetDiskFreeSpaceW", "sectors_per_cluster * bytes_per_sector", at))
                    .unwrap_or_else(|| unavailable(at, "Unidade de alocação indisponível.")),
                associated_disk_ids,
                associated_partition_ids,
            });

            name_buffer.fill(0);
            if unsafe {
                FindNextVolumeW(search, name_buffer.as_mut_ptr(), name_buffer.len() as u32)
            } == 0
            {
                break;
            }
            current = String::from_utf16_lossy(
                &name_buffer[..name_buffer
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(name_buffer.len())],
            );
        }
        unsafe {
            FindVolumeClose(search);
        }
        volumes
    }

    #[cfg(test)]
    mod storage_device_descriptor_tests {
        use super::*;

        fn descriptor_fixture(value: Option<&[u8]>) -> Vec<u8> {
            let payload = value.unwrap_or_default();
            let size = STORAGE_DEVICE_DESCRIPTOR_HEADER_SIZE + payload.len();
            let mut data = vec![0u8; size];
            data[0..4]
                .copy_from_slice(&(STORAGE_DEVICE_DESCRIPTOR_HEADER_SIZE as u32).to_le_bytes());
            data[4..8].copy_from_slice(&(size as u32).to_le_bytes());
            if value.is_some() {
                data[12..16]
                    .copy_from_slice(&(STORAGE_DEVICE_DESCRIPTOR_HEADER_SIZE as u32).to_le_bytes());
                data[STORAGE_DEVICE_DESCRIPTOR_HEADER_SIZE..].copy_from_slice(payload);
            }
            data
        }

        #[test]
        fn accepts_complete_descriptor_and_reads_ansi_vendor() {
            let data = descriptor_fixture(Some(b"Kingston\0"));
            let validated = validate_storage_device_descriptor(&data, data.len()).unwrap();
            assert_eq!(
                read_ansi_string(validated, 12).unwrap().as_deref(),
                Some("Kingston")
            );
        }

        #[test]
        fn rejects_declared_size_larger_than_returned_bytes() {
            let mut data = descriptor_fixture(Some(b"Kingston\0"));
            let declared = (data.len() + 8) as u32;
            data[4..8].copy_from_slice(&declared.to_le_bytes());
            assert!(validate_storage_device_descriptor(&data, data.len()).is_err());
        }

        #[test]
        fn rejects_string_offset_inside_header() {
            let mut data = descriptor_fixture(Some(b"Kingston\0"));
            data[12..16].copy_from_slice(&35u32.to_le_bytes());
            let validated = validate_storage_device_descriptor(&data, data.len()).unwrap();
            assert!(read_ansi_string(validated, 12).is_err());
        }

        #[test]
        fn rejects_unterminated_string() {
            let mut data = descriptor_fixture(Some(b"Kingston"));
            let validated = validate_storage_device_descriptor(&data, data.len()).unwrap();
            assert!(read_ansi_string(validated, 12).is_err());
        }

        #[test]
        fn treats_zero_offset_as_absent() {
            let data = descriptor_fixture(None);
            let validated = validate_storage_device_descriptor(&data, data.len()).unwrap();
            assert_eq!(read_ansi_string(validated, 12).unwrap(), None);
        }

        #[test]
        fn rejects_generic_windows_and_virtual_vendor_labels_as_manufacturer() {
            assert!(is_generic_manufacturer("(Unidades de disco padrão)"));
            assert!(is_generic_manufacturer("Msft"));
            assert!(!is_generic_manufacturer("Kingston"));
        }

        #[test]
        fn keeps_missing_manufacturer_note_concise_without_inference() {
            let note = "Fabricante não identificado por fontes confiáveis disponíveis.";
            assert!(note.len() < 100);
            assert!(!note.contains("Kingston"));
        }

        #[test]
        fn usb_removivel_generico_nao_e_classificado_automaticamente_como_pendrive() {
            let (kind, estimated) = classify_device_kind(
                &StorageBusType::Usb,
                Some(true),
                Some(0),
                &StorageMediaType::Unknown,
                Some("USB Storage Device"),
                None,
            );
            assert_eq!(kind, StorageDeviceKind::UsbRemovableStorage);
            assert!(!estimated);
        }

        #[test]
        fn usb_com_identificador_de_flash_e_classificado_como_estimativa() {
            let (kind, estimated) = classify_device_kind(
                &StorageBusType::Usb,
                Some(true),
                Some(0),
                &StorageMediaType::Unknown,
                Some("SanDisk Cruzer Blade"),
                None,
            );
            assert_eq!(kind, StorageDeviceKind::UsbFlashDrive);
            assert!(estimated);
        }

        #[test]
        fn leitor_de_cartao_usb_nao_e_classificado_como_pendrive() {
            let (kind, estimated) = classify_device_kind(
                &StorageBusType::Usb,
                Some(true),
                Some(0),
                &StorageMediaType::Unknown,
                Some("USB Card Reader Flash"),
                None,
            );
            assert_eq!(kind, StorageDeviceKind::UsbRemovableStorage);
            assert!(!estimated);
        }

        #[test]
        fn armazenamento_usb_fixo_preserva_classificacao_por_midia() {
            let (kind, estimated) = classify_device_kind(
                &StorageBusType::Usb,
                Some(false),
                Some(0),
                &StorageMediaType::Ssd,
                Some("External SSD"),
                None,
            );
            assert_eq!(kind, StorageDeviceKind::SolidStateDrive);
            assert!(!estimated);
        }

        #[test]
        fn barramentos_sd_e_mmc_sao_classificados_como_cartao_de_memoria() {
            for bus in [StorageBusType::Sd, StorageBusType::Mmc] {
                let (kind, estimated) = classify_device_kind(
                    &bus,
                    Some(true),
                    Some(0),
                    &StorageMediaType::Unknown,
                    Some("Generic Storage"),
                    None,
                );
                assert_eq!(kind, StorageDeviceKind::MemoryCard);
                assert!(!estimated);
            }
        }

        #[test]
        fn dispositivo_optico_tem_precedencia_sobre_identificador_usb() {
            let (kind, estimated) = classify_device_kind(
                &StorageBusType::Usb,
                Some(true),
                Some(5),
                &StorageMediaType::Unknown,
                Some("USB Flash Drive"),
                None,
            );
            assert_eq!(kind, StorageDeviceKind::OpticalDrive);
            assert!(estimated);
        }
    }

    pub(super) fn collect() -> Result<CategoryCollection<StorageCategory>, String> {
        let started = Instant::now();
        let started_at = now_utc();
        let mut warnings = Vec::new();
        let mut physical_disks = Vec::new();
        let mut partitions = Vec::new();

        // A enumeração por número evita depender de letras de unidade e inclui
        // discos USB, virtuais e sem volumes montados. Buracos não encerram a busca.
        for number in 0..256u32 {
            if let Some((disk, mut disk_partitions)) =
                collect_disk(number, &started_at, &mut warnings)
            {
                physical_disks.push(disk);
                partitions.append(&mut disk_partitions);
            }
        }

        let volumes = collect_volumes(&started_at, &physical_disks, &partitions, &mut warnings);
        if physical_disks.is_empty() {
            warnings.push(
                "Nenhum disco físico pôde ser enumerado; os volumes ainda foram consultados."
                    .into(),
            );
        }
        let data = StorageCategory {
            physical_disks,
            partitions,
            volumes,
        };
        data.validate()?;

        let finished_at = now_utc();
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
}

#[cfg(target_os = "windows")]
pub(crate) fn collect_storage_category() -> Result<CategoryCollection<StorageCategory>, String> {
    windows::collect()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn collect_storage_category() -> Result<CategoryCollection<StorageCategory>, String> {
    Err("O Inventário Técnico está disponível somente no Windows.".into())
}
