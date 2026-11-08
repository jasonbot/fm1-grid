//! SCSI passthrough to the WL82 ROM's USB disk interface, and its discovery.
//!
//! Linux uses SCSI generic devices and SG_IO; Windows uses SCSI pass-through
//! on physical drives; macOS uses USB bulk-only transport.

use std::fmt;

#[derive(Debug, Clone)]
pub enum TransportError {
    Permission(String),
    Failed(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Permission(message) | Self::Failed(message) => f.write_str(message),
        }
    }
}

pub enum Direction<'a> {
    In(&'a mut [u8]),
    Out(&'a [u8]),
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::os::fd::AsRawFd;

    #[repr(C)]
    struct SgIo {
        interface_id: i32,
        dxfer_direction: i32,
        cmd_len: u8,
        mx_sb_len: u8,
        iovec_count: u16,
        dxfer_len: u32,
        dxferp: *mut libc::c_void,
        cmdp: *const u8,
        sbp: *mut u8,
        timeout: u32,
        flags: u32,
        pack_id: i32,
        usr_ptr: *mut libc::c_void,
        status: u8,
        masked_status: u8,
        msg_status: u8,
        sb_len_wr: u8,
        host_status: u16,
        driver_status: u16,
        resid: i32,
        duration: u32,
        info: u32,
    }

    const SG_IO: libc::c_ulong = 0x2285;

    pub struct Device {
        file: File,
    }

    impl Device {
        pub fn open(path: &str) -> Result<Self, TransportError> {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::PermissionDenied {
                        TransportError::Permission(format!(
                            "no permission for {path}: install a udev rule for the unit or run with root"
                        ))
                    } else {
                        TransportError::Failed(format!("cannot open {path}: {error}"))
                    }
                })?;
            Ok(Self { file })
        }
    }

    impl super::Transport for Device {
        fn command(&mut self, cdb: &[u8], data: Direction<'_>) -> Result<usize, TransportError> {
            let mut sense = [0u8; 32];
            let (direction, pointer, length) = match data {
                Direction::Out(bytes) => (-2, bytes.as_ptr() as *mut libc::c_void, bytes.len()),
                Direction::In(bytes) => (-3, bytes.as_mut_ptr() as *mut libc::c_void, bytes.len()),
            };
            let mut io = SgIo {
                interface_id: i32::from(b'S'),
                dxfer_direction: direction,
                cmd_len: cdb.len() as u8,
                mx_sb_len: sense.len() as u8,
                iovec_count: 0,
                dxfer_len: length as u32,
                dxferp: pointer,
                cmdp: cdb.as_ptr(),
                sbp: sense.as_mut_ptr(),
                timeout: 10_000,
                flags: 0,
                pack_id: 0,
                usr_ptr: std::ptr::null_mut(),
                status: 0,
                masked_status: 0,
                msg_status: 0,
                sb_len_wr: 0,
                host_status: 0,
                driver_status: 0,
                resid: 0,
                duration: 0,
                info: 0,
            };
            // SAFETY: every pointer points at storage alive for this call.
            let result =
                unsafe { libc::ioctl(self.file.as_raw_fd(), SG_IO as _, &mut io as *mut SgIo) };
            if result < 0 {
                return Err(TransportError::Failed(format!(
                    "SG_IO: {}",
                    std::io::Error::last_os_error()
                )));
            }
            if io.info & 1 != 0 {
                return Err(TransportError::Failed(format!(
                    "SCSI command failed (status {:#04x})",
                    io.status
                )));
            }
            Ok(length.saturating_sub(io.resid.max(0) as usize))
        }
    }

    pub fn find_devices() -> Vec<(String, String)> {
        let mut devices = Vec::new();
        let Ok(entries) = std::fs::read_dir("/sys/class/scsi_generic") else {
            return devices;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let base = format!("/sys/class/scsi_generic/{name}/device");
            let read = |file: &str| {
                std::fs::read_to_string(format!("{base}/{file}"))
                    .map(|text| text.trim().to_string())
                    .unwrap_or_default()
            };
            if read("vendor") == "WL82" && read("model") == "UBOOT1.00" {
                devices.push((
                    format!("/dev/{name}"),
                    format!("WL82 UBOOT1.00 (/dev/{name})"),
                ));
            }
        }
        devices
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, GENERIC_READ, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const IOCTL_SCSI_PASS_THROUGH_DIRECT: u32 = 0x4D014;

    #[repr(C)]
    struct Sptd {
        length: u16,
        scsi_status: u8,
        path_id: u8,
        target_id: u8,
        lun: u8,
        cdb_length: u8,
        sense_info_length: u8,
        data_in: u8,
        data_transfer_length: u32,
        timeout_value: u32,
        data_buffer: *mut c_void,
        sense_info_offset: u32,
        cdb: [u8; 16],
    }

    #[repr(C)]
    struct SptdSense {
        packet: Sptd,
        sense: [u8; 32],
    }

    fn path_wide(path: &str) -> Vec<u16> {
        path.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub struct Device {
        handle: HANDLE,
    }

    impl Device {
        pub fn open(path: &str) -> Result<Self, TransportError> {
            let wide = path_wide(path);
            // SAFETY: wide is a valid NUL-terminated UTF-16 path.
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                // SAFETY: no pointers involved.
                let code = unsafe { GetLastError() };
                return Err(if code == ERROR_ACCESS_DENIED {
                    TransportError::Permission(format!(
                        "access to {path} denied: run FM1-Grid as administrator"
                    ))
                } else {
                    TransportError::Failed(format!("cannot open {path}: error {code}"))
                });
            }
            Ok(Self { handle })
        }
    }

    impl Drop for Device {
        fn drop(&mut self) {
            // SAFETY: the handle came from CreateFileW and is closed once.
            unsafe { CloseHandle(self.handle) };
        }
    }

    impl super::Transport for Device {
        fn command(&mut self, cdb: &[u8], data: Direction<'_>) -> Result<usize, TransportError> {
            let (pointer, length, data_in) = match data {
                Direction::In(bytes) => (bytes.as_mut_ptr() as *mut c_void, bytes.len(), 1),
                Direction::Out(bytes) => (bytes.as_ptr() as *mut c_void, bytes.len(), 0),
            };
            let mut request = SptdSense {
                packet: Sptd {
                    length: std::mem::size_of::<Sptd>() as u16,
                    scsi_status: 0,
                    path_id: 0,
                    target_id: 0,
                    lun: 0,
                    cdb_length: cdb.len() as u8,
                    sense_info_length: 32,
                    data_in,
                    data_transfer_length: length as u32,
                    timeout_value: 10,
                    data_buffer: pointer,
                    sense_info_offset: std::mem::size_of::<Sptd>() as u32,
                    cdb: [0; 16],
                },
                sense: [0; 32],
            };
            request.packet.cdb[..cdb.len()].copy_from_slice(cdb);
            let mut returned = 0u32;
            // SAFETY: request points at valid storage for the call.
            let ok = unsafe {
                DeviceIoControl(
                    self.handle,
                    IOCTL_SCSI_PASS_THROUGH_DIRECT,
                    &request as *const _ as *const c_void,
                    std::mem::size_of::<SptdSense>() as u32,
                    &mut request as *mut _ as *mut c_void,
                    std::mem::size_of::<SptdSense>() as u32,
                    &mut returned,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(TransportError::Failed(format!(
                    "SCSI pass-through failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            if request.packet.scsi_status != 0 {
                return Err(TransportError::Failed(format!(
                    "SCSI command failed (status {})",
                    request.packet.scsi_status
                )));
            }
            Ok(length)
        }
    }

    pub fn find_devices() -> Vec<(String, String)> {
        let mut found = Vec::new();
        for number in 0..32 {
            let path = format!(r"\\.\PhysicalDrive{number}");
            let Ok(mut device) = Device::open(&path) else {
                continue;
            };
            let Ok((vendor, product)) = super::inquiry(&mut device) else {
                continue;
            };
            if vendor == "WL82" && product == "UBOOT1.00" {
                found.push((path.clone(), format!("{vendor} {product} (disk {number})")));
            }
        }
        found
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use rusb::{Direction as UsbDirection, TransferType};
    use std::time::Duration;

    const VID: u16 = 0x4C4A;
    const PID: u16 = 0x8057;
    const TIMEOUT: Duration = Duration::from_secs(10);
    const CBW: u32 = 0x4342_5355;
    const CSW: u32 = 0x5342_5355;

    pub struct Device {
        handle: rusb::DeviceHandle<rusb::GlobalContext>,
        interface: u8,
        ep_in: u8,
        ep_out: u8,
        tag: u32,
    }

    fn usb_path(device: &rusb::Device<rusb::GlobalContext>) -> String {
        format!("usb:{}-{}", device.bus_number(), device.address())
    }

    fn find(path: &str) -> Option<(rusb::Device<rusb::GlobalContext>, u8, u8, u8)> {
        let devices = rusb::devices().ok()?;
        for device in devices.iter() {
            if usb_path(&device) != path {
                continue;
            }
            let config = device.active_config_descriptor().ok()?;
            for interface in config.interfaces() {
                for alt in interface.descriptors().filter(|alt| alt.class_code() == 8) {
                    let endpoint = |direction| {
                        alt.endpoint_descriptors()
                            .find(|ep| {
                                ep.transfer_type() == TransferType::Bulk
                                    && ep.direction() == direction
                            })
                            .map(|ep| ep.address())
                    };
                    if let (Some(ep_in), Some(ep_out)) =
                        (endpoint(UsbDirection::In), endpoint(UsbDirection::Out))
                    {
                        return Some((device, alt.interface_number(), ep_in, ep_out));
                    }
                }
            }
        }
        None
    }

    impl Device {
        pub fn open(path: &str) -> Result<Self, TransportError> {
            let (device, interface, ep_in, ep_out) = find(path).ok_or_else(|| {
                TransportError::Failed(format!("{path}: download-mode unit not found"))
            })?;
            let handle = device.open().map_err(|error| match error {
                rusb::Error::Access => TransportError::Permission(
                    "macOS denied USB access: run FM1-Grid with sudo".into(),
                ),
                error => TransportError::Failed(format!("cannot open the unit: {error}")),
            })?;
            if handle.kernel_driver_active(interface).unwrap_or(false) {
                handle.detach_kernel_driver(interface).map_err(|error| {
                    TransportError::Permission(format!("cannot take the USB interface: {error}"))
                })?;
            }
            handle.claim_interface(interface).map_err(|error| {
                TransportError::Permission(format!("cannot claim the USB interface: {error}"))
            })?;
            Ok(Self {
                handle,
                interface,
                ep_in,
                ep_out,
                tag: 0,
            })
        }
    }

    impl Drop for Device {
        fn drop(&mut self) {
            let _ = self.handle.release_interface(self.interface);
        }
    }

    impl super::Transport for Device {
        fn command(&mut self, cdb: &[u8], data: Direction<'_>) -> Result<usize, TransportError> {
            self.tag = self.tag.wrapping_add(1);
            let (length, flags) = match &data {
                Direction::In(bytes) => (bytes.len(), 0x80),
                Direction::Out(bytes) => (bytes.len(), 0),
            };
            let mut command = [0u8; 31];
            command[..4].copy_from_slice(&CBW.to_le_bytes());
            command[4..8].copy_from_slice(&self.tag.to_le_bytes());
            command[8..12].copy_from_slice(&(length as u32).to_le_bytes());
            command[12] = flags;
            command[14] = cdb.len() as u8;
            command[15..15 + cdb.len()].copy_from_slice(cdb);
            self.handle
                .write_bulk(self.ep_out, &command, TIMEOUT)
                .map_err(|error| TransportError::Failed(format!("USB command: {error}")))?;
            let moved = match data {
                Direction::In(bytes) => self
                    .handle
                    .read_bulk(self.ep_in, bytes, TIMEOUT)
                    .map_err(|error| TransportError::Failed(format!("USB read: {error}")))?,
                Direction::Out(bytes) => self
                    .handle
                    .write_bulk(self.ep_out, bytes, TIMEOUT)
                    .map_err(|error| TransportError::Failed(format!("USB write: {error}")))?,
            };
            let mut status = [0u8; 13];
            self.handle
                .read_bulk(self.ep_in, &mut status, TIMEOUT)
                .map_err(|error| TransportError::Failed(format!("USB status: {error}")))?;
            if u32::from_le_bytes(status[0..4].try_into().unwrap()) != CSW
                || u32::from_le_bytes(status[4..8].try_into().unwrap()) != self.tag
                || status[12] != 0
            {
                return Err(TransportError::Failed("invalid USB storage status".into()));
            }
            Ok(moved)
        }
    }

    pub fn find_devices() -> Vec<(String, String)> {
        let Ok(devices) = rusb::devices() else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for device in devices.iter() {
            let Ok(descriptor) = device.device_descriptor() else {
                continue;
            };
            if descriptor.vendor_id() != VID || descriptor.product_id() != PID {
                continue;
            }
            let path = usb_path(&device);
            found.push((path.clone(), format!("WL82 UBOOT1.00 ({path})")));
        }
        found
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod platform {
    use super::*;

    pub struct Device;

    impl Device {
        pub fn open(_: &str) -> Result<Self, TransportError> {
            Err(TransportError::Failed("unsupported platform".into()))
        }
    }

    impl super::Transport for Device {
        fn command(&mut self, _: &[u8], _: Direction<'_>) -> Result<usize, TransportError> {
            Err(TransportError::Failed("unsupported platform".into()))
        }
    }

    pub fn find_devices() -> Vec<(String, String)> {
        Vec::new()
    }
}

pub use platform::{find_devices, Device};

pub trait Transport {
    fn command(&mut self, cdb: &[u8], data: Direction<'_>) -> Result<usize, TransportError>;
}

pub fn inquiry(device: &mut impl Transport) -> Result<(String, String), TransportError> {
    let mut buffer = [0u8; 36];
    device.command(&[0x12, 0, 0, 0, 36, 0], Direction::In(&mut buffer))?;
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_string();
    Ok((text(&buffer[8..16]), text(&buffer[16..32])))
}
