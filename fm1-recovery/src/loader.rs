//! JieLi ROM upload and flash-loader commands.

use crate::transport::{self, Direction, Transport, TransportError};

const LOADER_ADDRESS: u32 = 0x01C2_0000;
const LOADER: &[u8] = include_bytes!("../data/mflr-loader-a.bin");
const LOADER_ARG: u16 = 1;
const WRITE_MEMORY: u16 = 0xFB06;
const JUMP: u16 = 0xFB08;
const ERASE_BLOCK: u16 = 0xFB00;
const ERASE_SECTOR: u16 = 0xFB01;
const WRITE_FLASH: u16 = 0xFB04;
const READ_FLASH: u16 = 0xFD05;
const READ_KEY: u16 = 0xFC09;
const ONLINE_DEVICE: u16 = 0xFC0A;
const IDENTIFY: u16 = 0xFC40;
const CRC32: u16 = 0xFC41;
const BUFFER_SIZE: u16 = 0xFC14;
const FLASH_SIZE: usize = 0x100000;

fn retry<T>(mut action: impl FnMut() -> Result<T, TransportError>) -> Result<T, TransportError> {
    let mut last = None;
    for attempt in 0..3 {
        match action() {
            Ok(value) => return Ok(value),
            Err(error) => {
                last = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(150 * (attempt + 1)));
            }
        }
    }
    Err(last.unwrap_or_else(|| TransportError::Failed("transfer failed".into())))
}

pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn cipher(bytes: &[u8], key: u32) -> Vec<u8> {
    const MAGIC: &[u8] = &[
        0xC3, 0xCF, 0xC0, 0xE8, 0xCE, 0xD2, 0xB0, 0xAE, 0xC4, 0xE3, 0xA3, 0xAC, 0xD3, 0xF1,
        0xC1, 0xD6,
    ];
    let mut crc = crc16(&(key >> 16).to_le_bytes());
    bytes
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            crc = crc16(&[MAGIC[index % MAGIC.len()]]);
            byte ^ (crc as u8)
        })
        .collect()
}

fn cdb(command: u16, args: &[u8]) -> [u8; 16] {
    let mut packet = [0xFF; 16];
    packet[..2].copy_from_slice(&command.to_be_bytes());
    packet[2..2 + args.len()].copy_from_slice(args);
    packet
}

fn sized_args(address: u32, bytes: &[u8]) -> Vec<u8> {
    let mut args = address.to_be_bytes().to_vec();
    args.extend((bytes.len() as u16).to_be_bytes());
    args.push(0);
    args.extend(crc16(bytes).to_le_bytes());
    args
}

pub struct Loader {
    device: transport::Device,
    chip_key: u16,
    flash_id: u32,
    io: usize,
}

impl Loader {
    pub fn open(path: &str, log: &mut dyn FnMut(String)) -> Result<Self, TransportError> {
        let mut device = transport::Device::open(path)?;
        let (vendor, product) = transport::inquiry(&mut device)?;
        if vendor != "WL82" || product != "UBOOT1.00" {
            return Err(TransportError::Failed(format!(
                "{path} is not a WL82 UBOOT1.00 download-mode unit"
            )));
        }
        let mut loader = Self {
            device,
            chip_key: 0,
            flash_id: 0,
            io: 4096,
        };
        if loader.identify().is_none() {
            log("starting the recovery loader".into());
            loader.upload(LOADER_ADDRESS, LOADER)?;
            loader.jump(LOADER_ADDRESS)?;
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
        if loader.identify().is_none() {
            return Err(TransportError::Failed(
                "the recovery loader did not answer".into(),
            ));
        }
        let answer = loader.call(READ_KEY, &0xAC6900u32.to_be_bytes())?;
        let key = u16::from_le_bytes([answer[5], answer[4]]);
        loader.chip_key = u16::from_le_bytes(cipher(&key.to_le_bytes(), 0xFFFF_FFFF).try_into().unwrap());
        let answer = loader.call(ONLINE_DEVICE, &[])?;
        loader.flash_id = u32::from_le_bytes([answer[2], answer[3], answer[4], answer[5]]);
        if let Ok(answer) = loader.call(BUFFER_SIZE, &[]) {
            let size = u32::from_be_bytes([answer[0], answer[1], answer[2], answer[3]]) as usize;
            loader.io = size.clamp(256, 4096);
        }
        log(format!(
            "detected flash 0x{:06X}, chip key 0x{:04X}",
            loader.flash_id, loader.chip_key
        ));
        Ok(loader)
    }

    pub fn chip_key(&self) -> u16 { self.chip_key }
    pub fn flash_id(&self) -> u32 { self.flash_id }

    fn upload(&mut self, address: u32, image: &[u8]) -> Result<(), TransportError> {
        for (index, block) in image.chunks(512).enumerate() {
            let at = address + (index * 512) as u32;
            let scrambled = cipher(block, 0xFFFF_FFFF);
            let packet = cdb(WRITE_MEMORY, &sized_args(at, &scrambled));
            retry(|| self.device.command(&packet, Direction::Out(&scrambled)).map(|_| ()))?;
        }
        Ok(())
    }

    fn jump(&mut self, address: u32) -> Result<(), TransportError> {
        let mut args = address.to_be_bytes().to_vec();
        args.extend(LOADER_ARG.to_be_bytes());
        let packet = cdb(JUMP, &args);
        let mut answer = [0u8; 16];
        retry(|| self.device.command(&packet, Direction::In(&mut answer)).map(|_| ()))?;
        Ok(())
    }

    fn identify(&mut self) -> Option<()> {
        let answer = self.call(IDENTIFY, &[]).ok()?;
        (&answer[..4] == b"MFLR").then_some(())
    }

    fn call(&mut self, command: u16, args: &[u8]) -> Result<[u8; 14], TransportError> {
        let packet = cdb(command, args);
        let mut answer = [0u8; 16];
        retry(|| {
            self.device
                .command(&packet, Direction::In(&mut answer))
                .map(|_| ())
        })?;
        if u16::from_be_bytes([answer[0], answer[1]]) != command {
            return Err(TransportError::Failed("the loader answered the wrong command".into()));
        }
        Ok(answer[2..].try_into().unwrap())
    }


    pub fn read(&mut self, address: usize, length: usize, progress: &mut dyn FnMut(usize)) -> Result<Vec<u8>, TransportError> {
        let mut output = vec![0u8; length];
        let mut done = 0;
        while done < length {
            let chunk = self.io.min(length - done);
            let mut args = ((address + done) as u32).to_be_bytes().to_vec();
            args.extend((chunk as u16).to_be_bytes());
            let packet = cdb(READ_FLASH, &args);
            let target = &mut output[done..done + chunk];
            retry(|| {
                let got = self.device.command(&packet, Direction::In(target))?;
                if got == chunk { Ok(()) } else { Err(TransportError::Failed("short flash read".into())) }
            })?;
            done += chunk;
            progress(done);
        }
        Ok(output)
    }

    pub fn read_all(&mut self, progress: &mut dyn FnMut(usize)) -> Result<Vec<u8>, TransportError> {
        self.read(0, FLASH_SIZE, progress)
    }

    pub fn erase(&mut self, address: usize, length: usize) -> Result<(), TransportError> {
        let end = address + length;
        let mut at = address;
        while at < end {
            let (command, size) = if at % 0x10000 == 0 && end - at >= 0x10000 {
                (ERASE_BLOCK, 0x10000)
            } else {
                (ERASE_SECTOR, 0x1000)
            };
            self.call(command, &(at as u32).to_be_bytes())?;
            at += size;
        }
        Ok(())
    }

    pub fn write(&mut self, address: usize, bytes: &[u8], progress: &mut dyn FnMut(usize)) -> Result<(), TransportError> {
        for (index, chunk) in bytes.chunks(self.io).enumerate() {
            if chunk.iter().any(|&byte| byte != 0xFF) {
                let at = (address + index * self.io) as u32;
                let packet = cdb(WRITE_FLASH, &sized_args(at, chunk));
                retry(|| self.device.command(&packet, Direction::Out(chunk)).map(|_| ()))?;
            }
            progress(index * self.io + chunk.len());
        }
        Ok(())
    }

    pub fn crc32(&mut self, address: usize, length: usize) -> Result<u32, TransportError> {
        let mut args = (address as u32).to_be_bytes().to_vec();
        args.extend((length as u32).to_be_bytes());
        let answer = self.call(CRC32, &args)?;
        Ok(u32::from_be_bytes([answer[0], answer[1], answer[2], answer[3]]))
    }
}
