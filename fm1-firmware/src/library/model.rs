//! The firmware rows and their source/status types.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::atomic::AtomicU32;

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum FirmwareSource {
    Builtin,
    File { path: String },
    /// `path` is the package's path on the hub ("fw/<file>") or, for entries scraped
    /// from the fm1-editor page, the package's absolute URL; `blob` is its key in
    /// the blob store (see `blob_key`), set once the download completes.
    Hub { path: String, blob: Option<String> },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum FirmwareDevice {
    Fm1,
    /// Any SMK-37 model (Pro, Elite, MKE-P37); the row's variant tag names which.
    /// The stored key stays "smk37pro" for old library records.
    #[serde(rename = "smk37pro")]
    Smk37,
}

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum Status {
    #[default]
    Available,
    Downloading,
    Missing,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Firmware {
    pub name: String,
    pub byline: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub device: FirmwareDevice,
    /// An installed entry (builtin or downloaded hub package) older than the hub's
    /// version of the same name. Computed, not stored.
    #[serde(skip)]
    pub upgradable: bool,
    /// Computed on every `list()`, never stored.
    #[serde(skip)]
    pub status: Status,
    /// The project's web page, when the listing source carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    /// The source code's home, when the listing source carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub source: FirmwareSource,
}

impl Firmware {
    /// The SMK-37 submodel this row is built for, from the catalog tag.
    pub fn variant(&self) -> Option<&str> {
        self.tags.iter().find_map(|tag| match tag.as_str() {
            "SMK-37 Pro" | "SMK-37 Elite" | "MKE-P37" => Some(tag.as_str()),
            _ => None,
        })
    }
}

/// Which online listing sources the Library is allowed to fetch. The Hub is the
/// source of truth; the others are additive. mutant.cloud and Reverbose are off by
/// default because they are the noisiest mirrors.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct HubFeeds {
    pub hub: bool,
    pub editor: bool,
    pub mutant: bool,
    pub reverbose: bool,
    pub shrub: bool,
}

impl Default for HubFeeds {
    fn default() -> Self {
        Self {
            hub: true,
            editor: true,
            mutant: false,
            reverbose: false,
            shrub: true,
        }
    }
}

impl HubFeeds {
    /// Whether at least one source will be fetched.
    pub fn any(self) -> bool {
        self.hub || self.editor || self.mutant || self.reverbose || self.shrub
    }

    /// Whether an entry carrying `source:<origin>` belongs to an enabled feed.
    pub fn allows(self, origin: &str) -> bool {
        match origin {
            "hub" => self.hub,
            "fm1-editor" => self.editor,
            "mutant" => self.mutant,
            "reverbose" => self.reverbose,
            "shrub" => self.shrub,
            // Bundled index metadata is always visible.
            _ => true,
        }
    }
}

/// A download in flight: the percent the UI shows, and the outcome the next `list()`
/// picks up.
pub struct Download {
    pub(super) percent: AtomicU32,
    pub(super) outcome: Mutex<Option<Result<Firmware, String>>>,
}

/// Where a hub refresh stands (the Library tab's status item reads it).
#[derive(Clone)]
pub enum HubStatus {
    /// Not fetching right now.
    Idle,
    Loading,
    Failed(String),
}
