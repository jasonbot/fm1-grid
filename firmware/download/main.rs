//! Fetch the firmware packages listed in ../firmwares.json and verify them against the
//! recorded SHA-256 hashes. Files that already exist and hash correctly are left alone;
//! missing files are downloaded to a temporary `.part` file and renamed into place only
//! after the hash matches. Exits non-zero if anything fails.
//!
//! Usage (from the repo root): cargo run --manifest-path firmware/download/Cargo.toml
//! Optional argument: directory containing firmwares.json (defaults to this crate's
//! parent directory, so the command above works from any cwd).

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// One entry in firmwares.json. `url` is the download location; entries added locally
/// by `scripts/firmware.sh add` may not have one. Extra fields (name, version, tags,
/// source, license) are documentation for humans and other tooling; serde ignores them
/// here.
#[derive(Deserialize)]
struct Firmware {
    file: String,
    url: Option<String>,
    sha256: String,
}

fn main() -> ExitCode {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_owned());
    match run(&dir) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(failures) => {
            eprintln!("{failures} of the firmware files failed; see above");
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

/// Processes every entry; returns the number of failures.
fn run(dir: &Path) -> anyhow::Result<usize> {
    let json_path = dir.join("firmwares.json");
    let data = fs::read_to_string(&json_path)
        .with_context(|| format!("reading {}", json_path.display()))?;
    let list: Vec<Firmware> = serde_json::from_str(&data)
        .with_context(|| format!("parsing {}", json_path.display()))?;

    let mut failures = 0;
    for fw in &list {
        let path = dir.join(&fw.file);
        if path.exists() {
            match hash_stream(File::open(&path)?, io::sink()) {
                Ok(sum) if sum == fw.sha256 => println!("ok          {}", fw.file),
                Ok(sum) => {
                    eprintln!(
                        "MISMATCH    {}: expected {}, got {sum} (delete the file to force a re-download)",
                        fw.file, fw.sha256
                    );
                    failures += 1;
                }
                Err(err) => {
                    eprintln!("ERROR       {}: {err:#}", fw.file);
                    failures += 1;
                }
            }
        } else {
            match &fw.url {
                Some(url) => match download(fw, url, dir) {
                    Ok(()) => println!("downloaded  {}", fw.file),
                    Err(err) => {
                        eprintln!("ERROR       {} ({url}): {err:#}", fw.file);
                        failures += 1;
                    }
                },
                None => {
                    eprintln!("MISSING     {}: no url recorded; place the file in {}", fw.file, dir.display());
                    failures += 1;
                }
            }
        }
    }
    println!("{} entries checked, {failures} failed", list.len());
    Ok(failures)
}

/// Downloads url to a temp file, verifies the hash, renames into place.
fn download(fw: &Firmware, url: &str, dir: &Path) -> anyhow::Result<()> {
    let tmp = dir.join(format!(".{}.part", fw.file));
    let sum = fetch(url, &tmp);
    if sum.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    let sum = sum?;
    if sum != fw.sha256 {
        let _ = fs::remove_file(&tmp);
        bail!("hash mismatch: expected {}, got {sum}", fw.sha256);
    }
    fs::rename(&tmp, dir.join(&fw.file))?;
    Ok(())
}

fn fetch(url: &str, dest: &Path) -> anyhow::Result<String> {
    let mut resp = ureq::get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let sum = hash_stream(resp.body_mut().as_reader(), File::create(dest)?)?;
    Ok(sum)
}

/// Streams r into w while hashing; returns the lowercase hex SHA-256.
fn hash_stream(mut r: impl Read, mut w: impl Write) -> anyhow::Result<String> {
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
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}
