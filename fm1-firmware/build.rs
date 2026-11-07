//! Generates the embedded firmware table from `firmware/firmwares.json`.
//!
//! The JSON list is the single place that lists what gets compiled into the binary. Use
//! `scripts/firmware.sh` to add or remove entries; the generated code is never edited.
//!
//! The .fwsc packages are gitignored; missing ones are downloaded from the recorded
//! url and verified against the recorded SHA-256, so a fresh clone builds as-is.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// One entry in firmwares.json. Extra fields (license) are documentation for
/// humans and other tooling; serde ignores them here.
#[derive(Deserialize)]
struct Firmware {
    name: String,
    version: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    sha256: String,
    /// Download location for missing packages; entries added locally by
    /// `scripts/firmware.sh add` may not have one.
    url: Option<String>,
    tags: Vec<String>,
    /// Hub catalog text; empty when the package isn't in the catalog (M-VAVE's own).
    #[serde(default)]
    description: String,
    #[serde(default)]
    byline: String,
    #[serde(default)]
    device: String,
    /// The project's web page and source code home; empty when none.
    #[serde(default)]
    website: String,
    #[serde(default)]
    source: String,
    /// Listing-only: ship the metadata (so the app can offer the package before the
    /// online catalog arrives) but never download or embed its bytes.
    #[serde(default)]
    index_only: bool,
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Streams r into w while hashing; returns the lowercase hex SHA-256.
fn hash_stream(mut r: impl Read, mut w: impl Write) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        w.write_all(&buf[..n])?;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn fetch(url: &str, dest: &Path) -> Result<String, String> {
    let mut resp = ureq::get(url).call().map_err(|e| format!("GET {url}: {e}"))?;
    let file = File::create(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    hash_stream(resp.body_mut().as_reader(), file).map_err(|e| format!("GET {url}: {e}"))
}

/// Downloads url to a temporary `.part` file and renames it into place only after the
/// recorded hash matches.
fn download(fw: &Firmware, url: &str, path: &Path) -> Result<(), String> {
    let tmp = path.with_file_name(format!(".{}.part", fw.file));
    let sum = fetch(url, &tmp);
    if sum.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    let sum = sum?;
    if sum != fw.sha256 {
        let _ = fs::remove_file(&tmp);
        return Err(format!("hash mismatch: expected {}, got {sum}", fw.sha256));
    }
    fs::rename(&tmp, path).map_err(|e| format!("rename to {}: {e}", path.display()))?;
    Ok(())
}

/// Makes sure the package file exists and matches the recorded hash, downloading it
/// from the recorded url when missing. Fails the build otherwise: the table below
/// embeds these bytes, so there is nothing to compile without them.
fn ensure_firmware(fw: &Firmware, path: &Path) {
    if path.is_file() {
        match hash_stream(File::open(path).unwrap(), io::sink()) {
            Ok(sum) if sum == fw.sha256 => return,
            Ok(sum) => panic!(
                "firmware {:?}: {} does not match the recorded hash (expected {}, got \
                 {sum}); delete the file to force a re-download",
                fw.name,
                path.display(),
                fw.sha256
            ),
            Err(e) => panic!("firmware {:?}: cannot read {}: {e}", fw.name, path.display()),
        }
    }
    match &fw.url {
        Some(url) => {
            download(fw, url, path)
                .unwrap_or_else(|e| panic!("firmware {:?}: download failed: {e}", fw.name));
            println!("cargo:warning=downloaded firmware {} from {url}", fw.file);
        }
        None => panic!(
            "firmware {:?}: no such file {} and firmwares.json records no url for it; \
             place the package there manually",
            fw.name,
            path.display()
        ),
    }
}

/// One generated `BuiltinFirmware` literal. `path` is None for index-only entries,
/// whose bytes are never embedded.
fn entry(fw: &Firmware, path: Option<&Path>) -> String {
    let bytes = match path {
        Some(path) => format!(
            "include_bytes!(\"{}\")",
            escape(&path.to_string_lossy())
        ),
        None => "&[]".to_string(),
    };
    format!(
        "    BuiltinFirmware {{\n        name: \"{}\",\n        version: \"{}\",\n        file: \"{}\",\n        sha256: \"{}\",\n        tags: \"{}\",\n        description: \"{}\",\n        byline: \"{}\",\n        device: \"{}\",\n        website: \"{}\",\n        source: \"{}\",\n        url: \"{}\",\n        bytes: {},\n    }},\n",
        escape(&fw.name),
        escape(&fw.version),
        escape(&fw.file),
        escape(&fw.sha256),
        escape(&fw.tags.join(",")),
        escape(&fw.description),
        escape(&fw.byline),
        escape(&fw.device),
        escape(&fw.website),
        escape(&fw.source),
        escape(fw.url.as_deref().unwrap_or("")),
        bytes,
    )
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let json_path = manifest_dir.join("firmware/firmwares.json");
    println!("cargo:rerun-if-changed={}", json_path.display());

    let text = fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", json_path.display()));
    let list: Vec<Firmware> = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("cannot parse {}: {e}", json_path.display()));

    let mut baked = String::from("&[\n");
    let mut index = String::from("&[\n");
    for fw in &list {
        if fw.name.is_empty() || fw.version.is_empty() || fw.tags.is_empty() {
            panic!(
                "{}: entry {:?}: name, version and a non-empty tags list are required \
                 (the first tag is the dropdown group)",
                json_path.display(),
                fw.name
            );
        }
        if fw.index_only {
            if fw.url.as_deref().unwrap_or("").is_empty() {
                panic!(
                    "{}: entry {:?}: index_only entries need a url to download from",
                    json_path.display(),
                    fw.name
                );
            }
            index.push_str(&entry(fw, None));
            continue;
        }
        if fw.file.is_empty() || fw.sha256.is_empty() {
            panic!(
                "{}: entry {:?}: file and sha256 are required unless the entry is \
                 index_only",
                json_path.display(),
                fw.name
            );
        }
        let path = manifest_dir.join("firmware").join(&fw.file);
        ensure_firmware(fw, &path);
        println!("cargo:rerun-if-changed={}", path.display());
        baked.push_str(&entry(fw, Some(&path)));
    }
    baked.push_str("]\n");
    index.push_str("]\n");

    let out_dir = env::var("OUT_DIR").unwrap();
    let out = Path::new(&out_dir);
    fs::write(out.join("builtins.rs"), baked).unwrap();
    fs::write(out.join("bundled_index.rs"), index).unwrap();
}
