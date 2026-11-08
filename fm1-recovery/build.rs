//! Fetches the bundled recovery binaries at build time.
//!
//! `data/assets.json` pins the release, file names and SHA-256 values. A fresh
//! clone downloads missing files; an existing file is re-used only when its
//! hash still matches.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct Manifest {
    source: String,
    version: String,
    files: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    file: String,
    sha256: String,
    url: String,
}

fn hash(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn download(asset: &Asset, path: &Path) -> Result<(), String> {
    let part = path.with_extension("part");
    let mut response = ureq::get(&asset.url)
        .call()
        .map_err(|error| format!("GET {}: {error}", asset.url))?;
    let mut reader = response.body_mut().as_reader();
    let mut file =
        File::create(&part).map_err(|error| format!("create {}: {error}", part.display()))?;
    io::copy(&mut reader, &mut file).map_err(|error| format!("GET {}: {error}", asset.url))?;
    drop(file);

    let actual = hash(&part).map_err(|error| format!("read {}: {error}", part.display()))?;
    if actual != asset.sha256 {
        let _ = fs::remove_file(&part);
        return Err(format!(
            "hash mismatch: expected {}, got {actual}",
            asset.sha256
        ));
    }
    fs::rename(&part, path).map_err(|error| format!("rename to {}: {error}", path.display()))?;
    Ok(())
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let manifest_path = manifest_dir.join("data/assets.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());

    let text = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", manifest_path.display()));
    let manifest: Manifest = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("cannot parse {}: {error}", manifest_path.display()));

    for asset in &manifest.files {
        let path = manifest_dir.join("data").join(&asset.file);
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_file() {
            match hash(&path) {
                Ok(actual) if actual == asset.sha256 => continue,
                Ok(actual) => panic!(
                    "recovery asset {} does not match {}: expected {}, got {actual}; delete it to re-download",
                    asset.file, manifest.version, asset.sha256
                ),
                Err(error) => panic!("cannot read {}: {error}", path.display()),
            }
        }
        download(asset, &path)
            .unwrap_or_else(|error| panic!("recovery asset {}: {error}", asset.file));
        println!(
            "cargo:warning=downloaded {} from {} ({} {})",
            asset.file, manifest.source, manifest.version, asset.sha256
        );
    }
}
