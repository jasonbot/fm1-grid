//! Library persistence keys, status checks and the builtin/index folding.

use super::*;
use crate::builtin::BUNDLED_INDEX;
use crate::download_heuristics::looks_like_web_page;
use fm1_base::store::Store;
use std::collections::HashMap;
use std::sync::Arc;
use super::util::blob_key;

/// An entry's presence status. Downloading beats presence; files and downloaded hub
/// blobs are statted (never read in full); builtins are always here; hub-only offers
/// (no blob yet) are Available unless downloading. A blob that's a web page counts
/// as Missing so the row offers the download again.
pub(super) fn entry_status(
    firmware: &Firmware,
    downloads: &HashMap<String, Arc<Download>>,
    store: &Store,
) -> Status {
    match &firmware.source {
        FirmwareSource::Builtin => Status::Available,
        _ if downloads.contains_key(&firmware.name) => Status::Downloading,
        FirmwareSource::File { path } => {
            if std::path::Path::new(path).is_file() {
                Status::Available
            } else {
                Status::Missing
            }
        }
        FirmwareSource::Hub { path, blob: Some(_) } => {
            let usable = store
                .blob_head(&blob_key(path), 512)
                .map(|head| !looks_like_web_page(&head))
                .unwrap_or(false);
            if usable {
                Status::Available
            } else {
                Status::Missing
            }
        }
        FirmwareSource::Hub { blob: None, .. } => Status::Available,
    }
}

/// The binary's listing-only entries as catalog offers: metadata and the recorded
/// download URL, no bytes. They seed the hub at load and merge into every online
/// refresh, so a package that may not be redistributed still appears (and downloads
/// on request) without being baked into the app.
pub(super) fn index_listing() -> Vec<Firmware> {
    BUNDLED_INDEX.iter().map(index_entry).collect()
}

pub(super) fn index_entry(b: &crate::builtin::BuiltinFirmware) -> Firmware {
    /// An empty string in the table means the link doesn't exist.
    fn link(text: &str) -> Option<String> {
        (!text.is_empty()).then(|| text.to_string())
    }
    let mut tags: Vec<String> = b
        .tags
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect();
    // The origin tag names the listing the offer came from: the binary's index.
    tags.push("source:index".to_string());
    Firmware {
        name: b.name.to_string(),
        byline: b.byline.to_string(),
        version: b.version.to_string(),
        description: b.description.to_string(),
        tags,
        device: crate::sources::parse_device(b.device),
        website: link(b.website),
        source_url: link(b.source),
        upgradable: false,
        status: Status::Available,
        source: FirmwareSource::Hub {
            path: b.url.to_string(),
            blob: None,
        },
    }
}

pub(super) fn builtin_entry(b: &crate::builtin::BuiltinFirmware) -> Firmware {
    /// An empty string in the table means the link doesn't exist.
    fn link(text: &str) -> Option<String> {
        (!text.is_empty()).then(|| text.to_string())
    }
    let mut tags: Vec<String> = b.tags.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect();
    // The origin tag names the listing the entry's package came from; a merge
    // swaps it when a newer listing supersedes the download.
    tags.push("source:builtin".to_string());
    Firmware {
        name: b.name.to_string(),
        byline: b.byline.to_string(),
        version: b.version.to_string(),
        description: b.description.to_string(),
        tags,
        device: crate::sources::parse_device(b.device),
        website: link(b.website),
        source_url: link(b.source),
        upgradable: false,
        status: Status::Available,
        source: FirmwareSource::Builtin,
    }
}
