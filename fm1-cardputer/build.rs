//! Fetches the latest fm1-card Cardputer-Adv full-flash image at build time.
//!
//! The merged image (`fm1-card-<tag>-full.bin`) is gitignored. When it is
//! missing, this asks GitHub for the newest `jasonbot/fm1-card` release,
//! downloads the image and its `SHA256SUMS`, verifies the hash, and records the
//! release in `data/assets.json`. When both files exist, the hash is verified
//! and no network is used. Delete the image (or the manifest) to refresh it.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RELEASE_API: &str = "https://api.github.com/repos/jasonbot/fm1-card/releases/latest";
const IMAGE_FILE: &str = "fm1-card-full.bin";
const MANIFEST_FILE: &str = "assets.json";

#[derive(Deserialize, Serialize)]
struct Manifest {
    source: String,
    version: String,
    file: String,
    sha256: String,
    url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
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

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent("fm1-grid-build")
        .build()
        .into()
}

fn fetch_text(url: &str) -> Result<String, String> {
    let mut response = agent()
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("read {url}: {e}"))
}

fn download(url: &str, path: &Path) -> Result<(), String> {
    let part = path.with_extension("part");
    let mut response = agent()
        .get(url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?;
    let mut file = File::create(&part).map_err(|e| format!("create {}: {e}", part.display()))?;
    io::copy(&mut response.body_mut().as_reader(), &mut file)
        .map_err(|e| format!("GET {url}: {e}"))?;
    file.flush()
        .map_err(|e| format!("write {}: {e}", part.display()))?;
    fs::rename(&part, path).map_err(|e| format!("rename to {}: {e}", path.display()))
}

/// The release's merged image name and URL, plus its SHA256SUMS URL.
fn release_image(release: &Release) -> Result<(String, String, String), String> {
    let sums = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS")
        .ok_or_else(|| format!("{} has no SHA256SUMS asset", release.tag_name))?;
    let image = release
        .assets
        .iter()
        .find(|asset| asset.name.ends_with("-full.bin"))
        .ok_or_else(|| format!("{} has no -full.bin asset", release.tag_name))?;
    Ok((
        sums.browser_download_url.clone(),
        image.name.clone(),
        image.browser_download_url.clone(),
    ))
}

fn expected_hash(sums: &str, file: &str) -> Result<String, String> {
    for line in sums.lines() {
        let mut fields = line.split_whitespace();
        let (Some(hash), Some(name)) = (fields.next(), fields.next()) else {
            continue;
        };
        if name.trim_start_matches('*') == file {
            return Ok(hash.to_ascii_lowercase());
        }
    }
    Err(format!("SHA256SUMS has no entry for {file}"))
}

fn write_manifest(path: &Path, manifest: &Manifest) {
    let text = serde_json::to_string_pretty(manifest).expect("serialize manifest");
    fs::write(path, format!("{text}\n")).expect("write manifest");
}

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("data");
    fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
    let image = dir.join(IMAGE_FILE);
    let manifest_path = dir.join(MANIFEST_FILE);
    println!("cargo:rerun-if-changed={}", image.display());
    println!("cargo:rerun-if-changed={}", manifest_path.display());

    let manifest: Manifest = if manifest_path.is_file() {
        let text = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest_path.display()));
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("cannot parse {}: {e}", manifest_path.display()))
    } else if image.is_file() {
        // A manually placed image: use it, but label the version honestly.
        let sha256 = hash(&image).unwrap_or_else(|e| panic!("cannot hash {}: {e}", image.display()));
        let manifest = Manifest {
            source: "locally placed image".into(),
            version: "local".into(),
            file: IMAGE_FILE.into(),
            sha256,
            url: String::new(),
        };
        write_manifest(&manifest_path, &manifest);
        manifest
    } else {
        println!("cargo:warning=fetching the latest fm1-card release from GitHub");
        let text = fetch_text(RELEASE_API).unwrap_or_else(|e| panic!("fm1-card release: {e}"));
        let release: Release = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("fm1-card release JSON: {e}"));
        let (sums_url, image_name, image_url) = release_image(&release)
            .unwrap_or_else(|e| panic!("fm1-card release {}: {e}", release.tag_name));
        let sums = fetch_text(&sums_url)
            .unwrap_or_else(|e| panic!("fm1-card SHA256SUMS: {e}"));
        let sha256 = expected_hash(&sums, &image_name)
            .unwrap_or_else(|e| panic!("fm1-card SHA256SUMS: {e}"));
        let manifest = Manifest {
            source: release.html_url,
            version: release.tag_name,
            file: image_name,
            sha256,
            url: image_url,
        };
        // Record the release before the download: an interrupted first build
        // then resumes this release instead of mistaking the image for a
        // hand-placed one.
        write_manifest(&manifest_path, &manifest);
        manifest
    };

    if !image.is_file() {
        if manifest.url.is_empty() {
            panic!(
                "{} is missing and {} records no download URL; delete the manifest to fetch the latest release",
                image.display(),
                manifest_path.display()
            );
        }
        download(&manifest.url, &image)
            .unwrap_or_else(|e| panic!("fm1-card image {}: {e}", manifest.url));
        println!("cargo:warning=downloaded {} from {}", manifest.file, manifest.url);
    }
    let actual = hash(&image).unwrap_or_else(|e| panic!("cannot hash {}: {e}", image.display()));
    if actual != manifest.sha256 {
        panic!(
            "{} does not match {} (expected {}, got {actual}); delete it to re-download",
            image.display(),
            manifest.version,
            manifest.sha256
        );
    }

    println!("cargo:rustc-env=FM1_CARD_VERSION={}", manifest.version);
}
