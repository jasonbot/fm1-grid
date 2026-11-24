//! M5Stack Cardputer-Adv support: serial flashing of the embedded fm1-card
//! release image, and the LAN client for running fm1-card devices.
//!
//! The full-flash image is fetched at build time (see `build.rs`): when
//! `data/fm1-card-full.bin` is missing, the newest `jasonbot/fm1-card` GitHub
//! release supplies it and its `SHA256SUMS` entry verifies it.
//!
//! - [`flash`] writes that image to a Cardputer in ROM download mode.
//! - [`discover`] finds running devices with a UDP broadcast.
//! - [`firmwares`], [`firmware_info`] and [`upload`] copy packages onto them.
//! - [`catalog`] and [`upload_catalog`] check and replace the card's firmware
//!   manifest, so it does not have to fetch the online catalog itself.

mod esp;
mod net;

pub use esp::{FlashableDevice, find_flashable, flash};
pub use net::{
    CatalogInfo, CatalogReply, DISCOVERY_PORT, Discovery, FirmwareInfo, InstalledFirmware, Status,
    UploadMetadata, UploadReply, catalog, discover, firmware_info, firmwares, safe_file_name,
    sha256, status, upload, upload_catalog,
};

/// The merged full-flash image downloaded at build time (bootloader, partition
/// table and application, written at offset 0).
pub const FIRMWARE_IMAGE: &[u8] = include_bytes!("../data/fm1-card-full.bin");

/// The release tag the embedded image came from (`local` for a hand-placed
/// image).
pub const FIRMWARE_VERSION: &str = env!("FM1_CARD_VERSION");

pub const REPOSITORY_URL: &str = "https://github.com/jasonbot/fm1-card";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}
