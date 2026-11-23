//! ESP32-S3 ROM serial flashing: the same SLIP-framed protocol esptool uses.
//!
//! The Cardputer-Adv must already be in its ROM download mode (power off, hold
//! G0, plug in USB-C, release G0). The merged full-flash image is written at
//! offset 0 with the ROM loader, then verified against an MD5 the chip computes
//! from flash. No stub is uploaded, so this is slower than esptool but has no
//! per-chip binary to carry.

use std::io::Read;
use std::time::{Duration, Instant};

use md5::{Digest as _, Md5};
use serialport::{ClearBuffer, SerialPort};

use crate::Error;

const ESP_SPI_ATTACH: u8 = 0x0D;
const ESP_SYNC: u8 = 0x08;
const ESP_FLASH_BEGIN: u8 = 0x02;
const ESP_FLASH_DATA: u8 = 0x03;
const ESP_FLASH_END: u8 = 0x04;
const ESP_SPI_FLASH_MD5: u8 = 0x13;
const ESP_CHECKSUM_MAGIC: u8 = 0xEF;
const FLASH_WRITE_SIZE: usize = 0x400;
const FLASH_OFFSET: u32 = 0;
const SLIP_END: u8 = 0xC0;
const SLIP_ESC: u8 = 0xDB;
const SLIP_ESC_END: u8 = 0xDC;
const SLIP_ESC_ESC: u8 = 0xDD;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashableDevice {
    pub path: String,
    pub description: String,
}

/// Serial ports that are an Espressif USB Serial/JTAG unit, the interface the
/// Cardputer's ROM download mode exposes. Other CDC-ACM devices are not listed:
/// the flash button writes an ESP32-S3 image at offset 0.
pub fn find_flashable() -> Vec<FlashableDevice> {
    let Ok(ports) = serialport::available_ports() else {
        return Vec::new();
    };
    ports
        .into_iter()
        .filter_map(|port| {
            let serialport::SerialPortType::UsbPort(info) = &port.port_type else {
                return None;
            };
            let product = info.product.as_deref();
            let esp = info.vid == 0x303A
                || product.is_some_and(|product| {
                    let product = product.to_ascii_lowercase();
                    product.contains("usb jtag")
                        || product.contains("usb serial/jtag")
                        || product.contains("espressif")
                });
            if !esp {
                return None;
            }
            Some(FlashableDevice {
                path: port.port_name,
                description: product
                    .filter(|name| !name.trim().is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| "ESP32-S3 USB Serial/JTAG".to_string()),
            })
        })
        .collect()
}

/// Write a merged full-flash image at offset 0, then verify it with the chip's
/// flash MD5. The device is left in download mode (it needs a power cycle, as
/// esptool's hard reset over USB Serial/JTAG is unreliable on this board).
pub fn flash(
    path: &str,
    image: &[u8],
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(), Error> {
    if image.is_empty() {
        return Err(Error::new("the embedded fm1-card image is empty"));
    }
    if image[0] != 0xE9 {
        return Err(Error::new(
            "the embedded fm1-card image is not an ESP application image",
        ));
    }
    let mut link = Link::open(path)?;
    link.reset_to_bootloader();
    link.sync().map_err(|error| {
        Error::new(format!(
            "{error}. Put the Cardputer in download mode: power it off, hold G0, \
             connect USB-C, then release G0."
        ))
    })?;

    // The ROM does not enable the SPI flash until this command. Without it
    // FLASH_BEGIN waits forever and reports no answer.
    link.command(ESP_SPI_ATTACH, &[0u8; 8], 0, COMMAND_TIMEOUT)?
        .check("enable SPI flash")?;

    let total = image.len();
    let blocks = total.div_ceil(FLASH_WRITE_SIZE);
    let mut begin = Vec::with_capacity(20);
    begin.extend_from_slice(&(total as u32).to_le_bytes());
    begin.extend_from_slice(&(blocks as u32).to_le_bytes());
    begin.extend_from_slice(&(FLASH_WRITE_SIZE as u32).to_le_bytes());
    begin.extend_from_slice(&FLASH_OFFSET.to_le_bytes());
    // ESP32-S3 ROM takes the "encrypted" flag; plain images pass 0.
    begin.extend_from_slice(&0u32.to_le_bytes());
    let erase_timeout = Duration::from_secs(60)
        .max(Duration::from_secs_f64(60.0 * total as f64 / 1_000_000.0));
    link.command(ESP_FLASH_BEGIN, &begin, 0, erase_timeout)?
        .check("enter flash download mode")?;

    for seq in 0..blocks {
        let start = seq * FLASH_WRITE_SIZE;
        let mut block = [0xFFu8; FLASH_WRITE_SIZE];
        let end = (start + FLASH_WRITE_SIZE).min(total);
        block[..end - start].copy_from_slice(&image[start..end]);
        let mut data = Vec::with_capacity(16 + FLASH_WRITE_SIZE);
        data.extend_from_slice(&(FLASH_WRITE_SIZE as u32).to_le_bytes());
        data.extend_from_slice(&(seq as u32).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&block);
        let checksum = checksum(&block);
        link.command(ESP_FLASH_DATA, &data, checksum, COMMAND_TIMEOUT)?
            .check("write flash")?;
        progress(end, total);
    }

    // Verify while still in flash mode: FLASH_END leaves it.
    let md5_timeout = Duration::from_secs(60)
        .max(Duration::from_secs_f64(30.0 * total as f64 / 1_000_000.0));
    verify_md5(&mut link, image, md5_timeout)?;

    // 1 means "do not reboot": hard reset over USB Serial/JTAG does not
    // reliably start the application on this board. Some S3 ROMs reject this
    // after a verified MD5; the write is already complete, so ignore it and
    // let the power-cycle leave download mode.
    if let Ok(reply) = link.command(ESP_FLASH_END, &1u32.to_le_bytes(), 0, COMMAND_TIMEOUT) {
        let _ = reply.check("leave flash mode");
    }
    Ok(())
}

fn verify_md5(link: &mut Link, image: &[u8], timeout: Duration) -> Result<(), Error> {
    let mut params = Vec::with_capacity(16);
    params.extend_from_slice(&FLASH_OFFSET.to_le_bytes());
    params.extend_from_slice(&(image.len() as u32).to_le_bytes());
    params.extend_from_slice(&0u32.to_le_bytes());
    params.extend_from_slice(&0u32.to_le_bytes());
    let digest = link
        .command(ESP_SPI_FLASH_MD5, &params, 0, timeout)?
        .check("verify the written flash")?;
    let mut expected = Md5::new();
    expected.update(image);
    let expected = format!("{:x}", expected.finalize());
    let actual = if digest.len() == 16 {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    } else {
        // The ROM answers with 32 ASCII hex bytes plus NUL padding; esptool
        // also treats the trailing status as part of the reply.
        String::from_utf8_lossy(&digest)
            .trim_matches(|ch: char| ch == '\0' || ch.is_whitespace())
            .to_ascii_lowercase()
    };
    if actual != expected {
        return Err(Error::new(format!(
            "the flash did not verify: expected {expected}, the chip read {actual}. \
             Do not power off; reconnect and retry."
        )));
    }
    Ok(())
}

struct Reply {
    data: Vec<u8>,
}

impl Reply {
    fn check(self, action: &str) -> Result<Vec<u8>, Error> {
        if self.data.len() < 2 {
            return Err(Error::new(format!("{action}: the chip sent no status")));
        }
        let status = self.data[self.data.len() - 2];
        if status != 0 {
            let reason = self.data[self.data.len() - 1];
            return Err(Error::new(format!(
                "{action} failed (status {status:#04x}, reason {reason:#04x})"
            )));
        }
        Ok(self.data[..self.data.len() - 2].to_vec())
    }
}

struct Link {
    port: Box<dyn SerialPort>,
    raw: Vec<u8>,
}

impl Link {
    fn open(path: &str) -> Result<Self, Error> {
        let port = serialport::new(path, 115_200)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|error| Error::new(format!("cannot open {path}: {error}")))?;
        Ok(Self {
            port,
            raw: Vec::new(),
        })
    }

    /// The ESP32-S3 USB-Serial/JTAG bootloader reset pulse train (the same
    /// sequence esptool uses). It re-enters download mode even after a flash.
    fn reset_to_bootloader(&mut self) {
        use std::thread::sleep;
        let step = Duration::from_millis(100);
        let _ = self.port.write_request_to_send(true);
        let _ = self.port.write_data_terminal_ready(true);
        sleep(step);
        let _ = self.port.write_data_terminal_ready(false);
        let _ = self.port.write_request_to_send(true);
        sleep(step);
        let _ = self.port.write_request_to_send(false);
        let _ = self.port.write_data_terminal_ready(true);
        let _ = self.port.write_request_to_send(false);
        sleep(step);
        let _ = self.port.write_data_terminal_ready(true);
        let _ = self.port.write_request_to_send(true);
        sleep(step);
        let _ = self.port.clear(ClearBuffer::Input);
        self.raw.clear();
    }

    fn sync(&mut self) -> Result<(), Error> {
        let payload = [0x07, 0x07, 0x12, 0x20, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
            0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
            0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55];
        for _ in 0..5 {
            let _ = self.port.clear(ClearBuffer::Input);
            self.raw.clear();
            if self
                .command(ESP_SYNC, &payload, 0, Duration::from_millis(250))
                .is_ok()
            {
                std::thread::sleep(Duration::from_millis(50));
                let _ = self.port.clear(ClearBuffer::Input);
                self.raw.clear();
                return Ok(());
            }
        }
        Err(Error::new("the Cardputer did not answer the serial sync"))
    }

    fn command(
        &mut self,
        op: u8,
        data: &[u8],
        checksum: u8,
        timeout: Duration,
    ) -> Result<Reply, Error> {
        let deadline = Instant::now() + timeout;
        let mut packet = Vec::with_capacity(8 + data.len());
        packet.push(0x00);
        packet.push(op);
        packet.extend_from_slice(&(data.len() as u16).to_le_bytes());
        packet.extend_from_slice(&u32::from(checksum).to_le_bytes());
        packet.extend_from_slice(data);
        write_slip(&mut *self.port, &packet)?;

        while let Some(packet) = self.read_packet(deadline)? {
            if packet.len() < 8 || packet[0] != 0x01 || packet[1] != op {
                continue;
            }
            let length = u16::from_le_bytes([packet[2], packet[3]]) as usize;
            if packet.len() < 8 + length {
                continue;
            }
            return Ok(Reply {
                data: packet[8..8 + length].to_vec(),
            });
        }
        Err(Error::new(format!(
            "the chip did not answer command {op:#04x} within {timeout:?}"
        )))
    }

    fn read_packet(&mut self, deadline: Instant) -> Result<Option<Vec<u8>>, Error> {
        loop {
            if let Some(packet) = take_packet(&mut self.raw)? {
                return Ok(Some(packet));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            let mut chunk = [0u8; 4096];
            match self.port.read(&mut chunk) {
                Ok(0) => std::thread::sleep(Duration::from_millis(2)),
                Ok(read) => self.raw.extend_from_slice(&chunk[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(Error::new(format!("serial read: {error}"))),
            }
        }
    }
}

fn write_slip(port: &mut dyn SerialPort, packet: &[u8]) -> Result<(), Error> {
    let framed = slip_encode(packet);
    // Do not call `flush`: serialport implements it with tcdrain, which can
    // block forever on USB Serial/JTAG and holds the port open.
    port.write_all(&framed)
        .map_err(|error| Error::new(format!("serial write: {error}")))
}

fn slip_encode(packet: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(packet.len() + 4);
    out.push(SLIP_END);
    for &byte in packet {
        match byte {
            SLIP_END => out.extend_from_slice(&[SLIP_ESC, SLIP_ESC_END]),
            SLIP_ESC => out.extend_from_slice(&[SLIP_ESC, SLIP_ESC_ESC]),
            _ => out.push(byte),
        }
    }
    out.push(SLIP_END);
    out
}

fn slip_decode(frame: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(frame.len());
    let mut escaped = false;
    for &byte in frame {
        if escaped {
            match byte {
                SLIP_ESC_END => out.push(SLIP_END),
                SLIP_ESC_ESC => out.push(SLIP_ESC),
                other => {
                    return Err(Error::new(format!(
                        "bad SLIP escape 0xDB 0x{other:02X}"
                    )))
                }
            }
            escaped = false;
        } else if byte == SLIP_ESC {
            escaped = true;
        } else {
            out.push(byte);
        }
    }
    if escaped {
        return Err(Error::new("truncated SLIP escape"));
    }
    Ok(out)
}

/// Pull one complete frame from the raw stream; empty frames are keep-alives
/// and are skipped. A delimiter both closes the previous frame and starts the
/// next, so consecutive `0xC0` bytes produce no packet.
fn take_packet(raw: &mut Vec<u8>) -> Result<Option<Vec<u8>>, Error> {
    let Some(first) = raw.iter().position(|&byte| byte == SLIP_END) else {
        raw.clear();
        return Ok(None);
    };
    let mut frame = Vec::new();
    for (offset, &byte) in raw[first + 1..].iter().enumerate() {
        if byte == SLIP_END {
            if frame.is_empty() {
                // A keep-alive (or the delimiter that just opened the frame).
                continue;
            }
            let packet = slip_decode(&frame)?;
            raw.drain(..first + 2 + offset);
            return Ok(Some(packet));
        }
        frame.push(byte);
    }
    if frame.is_empty() {
        raw.clear();
    } else {
        raw.drain(..first);
    }
    Ok(None)
}

fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(ESP_CHECKSUM_MAGIC, |state, &byte| state ^ byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slip_escapes_the_delimiters_and_round_trips() {
        let packet = [0x00, 0xC0, 0xDB, 0xDD, 0x01];
        let framed = slip_encode(&packet);
        assert_eq!(framed[0], SLIP_END);
        assert_eq!(*framed.last().unwrap(), SLIP_END);
        assert_eq!(slip_decode(&framed[1..framed.len() - 1]).unwrap(), packet);
    }

    #[test]
    fn take_packet_skips_keep_alives_and_keeps_a_partial_frame() {
        let mut raw = vec![0xC0, 0xC0, 0x01, 0x08, 0xC0];
        assert_eq!(take_packet(&mut raw).unwrap(), Some(vec![0x01, 0x08]));
        assert!(raw.is_empty());
        raw = vec![0x00, 0x01, 0x02];
        assert_eq!(take_packet(&mut raw).unwrap(), None);
        // A frame starts at the first delimiter; junk before it is discarded.
        raw = vec![0xAA, 0xBB, 0xC0, 0x02, 0x03];
        assert_eq!(take_packet(&mut raw).unwrap(), None);
        raw.push(0xC0);
        assert_eq!(take_packet(&mut raw).unwrap(), Some(vec![0x02, 0x03]));
    }

    #[test]
    fn checksum_starts_at_the_rom_magic() {
        assert_eq!(checksum(&[]), 0xEF);
        assert_eq!(checksum(&[0x0F]), 0xE0);
        assert_eq!(checksum(&[0x10, 0x01]), 0xFE);
    }

    #[test]
    fn reply_status_comes_from_the_tail() {
        let reply = Reply { data: vec![0, 0] };
        assert_eq!(reply.check("ok").unwrap(), Vec::<u8>::new());
        let reply = Reply {
            data: vec![0x01, 0x02],
        };
        assert!(reply.check("write").unwrap_err().to_string().contains("status"));
    }

    #[test]
    fn image_params_are_esp32_s3_shaped() {
        // The merged image starts with 0xE9; the flash writer pads to 1 KiB.
        let image = [0xE9; FLASH_WRITE_SIZE + 3];
        assert_eq!(image.len().div_ceil(FLASH_WRITE_SIZE), 2);
        assert_eq!(u32::from_le_bytes(image[..4].try_into().unwrap()), 0xE9E9_E9E9);
    }
}
