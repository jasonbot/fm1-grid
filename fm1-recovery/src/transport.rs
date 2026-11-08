//! SCSI passthrough to the WL82 ROM's USB disk interface, and its discovery.
//!
//! Linux uses the SCSI generic device (`/dev/sgN`) and SG_IO. Other systems
//! report no devices; the recovery UI stays usable and shows the platform
//! limitation instead of crashing.

use std::fmt;
use std::path::Path;

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
            let result = unsafe {
                libc::ioctl(self.file.as_raw_fd(), SG_IO as _, &mut io as *mut SgIo)
            };
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
                devices.push((format!("/dev/{name}"), format!("WL82 UBOOT1.00 (/dev/{name})")));
            }
        }
        devices
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::*;

    pub struct Device;

    impl Device {
        pub fn open(_: &str) -> Result<Self, TransportError> {
            Err(TransportError::Failed(
                "raw USB recovery is available on Linux only".into(),
            ))
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

#[allow(dead_code)]
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}
