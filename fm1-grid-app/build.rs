//! Bundles every `themes/*.json` at build time: emits `THEME_FILES` (id from the
//! file stem, label from the set's `name`, JSON via `include_str!`) into OUT_DIR
//! for `src/theme.rs`. Adding a file to themes/ is all it takes to ship a set.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let themes_dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("themes");
    println!("cargo:rerun-if-changed={}", themes_dir.display());

    let mut files: Vec<_> = fs::read_dir(&themes_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();

    let mut out = String::from("pub(crate) const THEME_FILES: &[(&str, &str, &str)] = &[\n");
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let id = path.file_stem().unwrap().to_str().unwrap();
        let set: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let label = set["name"].as_str().unwrap();
        out += &format!(
            "    ({id:?}, {label:?}, include_str!({path:?})),\n",
            path = path.display().to_string()
        );
    }
    out += "];\n";

    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("theme_files.rs"),
        out,
    )
    .unwrap();

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    write_third_party_licenses(&manifest_dir);
}

// ---------------------------------------------------------------------------
// Third-party licenses
// ---------------------------------------------------------------------------

use std::collections::HashMap;

#[derive(serde::Deserialize)]
struct Lockfile {
    package: Vec<LockPackage>,
}

#[derive(serde::Deserialize)]
struct LockPackage {
    name: String,
    version: String,
    source: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct PackageInfo {
    #[serde(default)]
    license: Option<String>,
    #[serde(rename = "license-file", default)]
    license_file: Option<String>,
    #[serde(default)]
    repository: Option<String>,
}

#[derive(serde::Deserialize)]
struct FirmwareManifest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    byline: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    index_only: bool,
}

#[derive(serde::Deserialize)]
struct RecoveryManifest {
    source: String,
    version: String,
}

#[derive(serde::Deserialize)]
struct CratesIo {
    version: Option<CratesIoVersion>,
    #[serde(rename = "crate")]
    krate: Option<CratesIoCrate>,
}

#[derive(serde::Deserialize)]
struct CratesIoVersion {
    license: Option<String>,
}

#[derive(serde::Deserialize)]
struct CratesIoCrate {
    repository: Option<String>,
}

fn cargo_home() -> PathBuf {
    if let Some(home) = env::var_os("CARGO_HOME") {
        return PathBuf::from(home);
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".cargo");
    }
    if let Some(home) = env::var_os("USERPROFILE") {
        return PathBuf::from(home).join(".cargo");
    }
    PathBuf::new()
}

fn registry_dir(cargo_home: &Path, name: &str, version: &str) -> Option<PathBuf> {
    let src = cargo_home.join("registry").join("src");
    for index in fs::read_dir(src).ok()?.flatten() {
        let path = index.path().join(format!("{name}-{version}"));
        if path.is_dir() {
            return Some(path);
        }
    }
    None
}

fn workspace_value(dir: &Path, key: &str) -> Option<String> {
    let mut current = Some(dir);
    while let Some(path) = current {
        let Some(text) = fs::read_to_string(path.join("Cargo.toml")).ok() else {
            current = path.parent();
            continue;
        };
        if let Ok(parsed) = toml::from_str::<toml::Value>(&text) {
            if let Some(value) = parsed
                .get("workspace")
                .and_then(|workspace| workspace.get("package"))
                .and_then(|package| package.get(key))
                .and_then(|value| value.as_str())
            {
                return Some(value.to_string());
            }
        }
        current = path.parent();
    }
    None
}

fn manifest_at(dir: &Path) -> Option<PackageInfo> {
    let text = fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let parsed: toml::Value = toml::from_str(&text).ok()?;
    let package = parsed.get("package")?;
    let inherited = |key: &str| {
        package
            .get(key)
            .and_then(|value| value.get("workspace"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    };
    let value = |key: &str| -> Option<String> {
        package
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| {
                if inherited(key) {
                    workspace_value(dir, key)
                } else {
                    None
                }
            })
    };
    Some(PackageInfo {
        license: value("license"),
        license_file: value("license-file"),
        repository: value("repository"),
    })
}

fn git_dir(root: &Path, name: &str, version: &str) -> Option<PathBuf> {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if let Ok(manifest) = fs::read_to_string(dir.join("Cargo.toml")) {
            if let Ok(parsed) = toml::from_str::<toml::Value>(&manifest) {
                if let Some(package) = parsed.get("package") {
                    let found_name =
                        package.get("name").and_then(|value| value.as_str()) == Some(name);
                    let found_version = package
                        .get("version")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                        .or_else(|| workspace_value(&dir, "version"));
                    if found_name && found_version.as_deref() == Some(version) {
                        return Some(dir);
                    }
                }
            }
        }
        if depth >= 6 {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push((path, depth + 1));
                }
            }
        }
    }
    None
}

fn package_dir(cargo_home: &Path, package: &LockPackage) -> Option<PathBuf> {
    let source = package.source.as_deref()?;
    if source.starts_with("git+") {
        return git_dir(
            &cargo_home.join("git").join("checkouts"),
            &package.name,
            &package.version,
        );
    }
    registry_dir(cargo_home, &package.name, &package.version)
}

fn license_files(dir: &Path, info: &PackageInfo) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(relative) = &info.license_file {
        let path = dir.join(relative);
        if path.is_file() {
            files.push(path);
        }
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if name.starts_with("license")
                || name.starts_with("copying")
                || name.starts_with("notice")
                || name == "unlicense"
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

fn add_text(
    index: &mut HashMap<String, usize>,
    texts: &mut Vec<(String, String)>,
    title: String,
    body: String,
) -> usize {
    if let Some(existing) = index.get(&body) {
        return *existing;
    }
    texts.push((title, body.clone()));
    let number = texts.len();
    index.insert(body, number);
    number
}

fn license_file_text(
    index: &mut HashMap<String, usize>,
    texts: &mut Vec<(String, String)>,
    file: &Path,
) -> usize {
    let body = fs::read(file)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|error| format!("[could not read {}: {error}]\n", file.display()));
    let title = file
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    add_text(index, texts, title, body)
}

fn spdx_ids(expression: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for token in expression
        .split(|character: char| character.is_whitespace() || matches!(character, '(' | ')' | '/'))
    {
        if token.is_empty() || matches!(token, "OR" | "AND" | "WITH") {
            continue;
        }
        let id = match token {
            "GPL-3.0" => "GPL-3.0-only",
            "GPL-2.0" => "GPL-2.0-only",
            "GPL-1.0" => "GPL-1.0-only",
            "LGPL-3.0" => "LGPL-3.0-only",
            "LGPL-2.1" => "LGPL-2.1-only",
            "LGPL-2.0" => "LGPL-2.0-only",
            "GPL-3.0+" => "GPL-3.0-or-later",
            "GPL-2.0+" => "GPL-2.0-or-later",
            "GPL-1.0+" => "GPL-1.0-or-later",
            "LGPL-3.0+" => "LGPL-3.0-or-later",
            "LGPL-2.1+" => "LGPL-2.1-or-later",
            "LGPL-2.0+" => "LGPL-2.0-or-later",
            other => other,
        };
        let id = id.to_string();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

fn spdx_text(
    id: &str,
    cache: &Path,
    index: &mut HashMap<String, usize>,
    texts: &mut Vec<(String, String)>,
) -> Option<usize> {
    if id.starts_with("LicenseRef-") {
        return None;
    }
    let path = cache.join(format!("{id}.txt"));
    let body = if path.is_file() {
        fs::read_to_string(&path).ok()?
    } else {
        let url =
            format!("https://raw.githubusercontent.com/spdx/license-list-data/main/text/{id}.txt");
        let mut response = match ureq::get(&url).call() {
            Ok(response) => response,
            Err(error) => {
                println!("cargo:warning=license {id}: {error}");
                return None;
            }
        };
        let mut body = String::new();
        let mut reader = response.body_mut().as_reader();
        if let Err(error) = std::io::Read::read_to_string(&mut reader, &mut body) {
            println!("cargo:warning=license {id}: {error}");
            return None;
        }
        let _ = fs::write(&path, &body);
        body
    };
    Some(add_text(index, texts, format!("SPDX {id}"), body))
}

fn expression_texts(
    expression: &str,
    cache: &Path,
    index: &mut HashMap<String, usize>,
    texts: &mut Vec<(String, String)>,
) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    for id in spdx_ids(expression) {
        if let Some(number) = spdx_text(&id, cache, index, texts) {
            found.push((id, number));
        }
    }
    found
}

fn crates_io_info(name: &str, version: &str, cache: &Path) -> Option<(String, Option<String>)> {
    let path = cache.join(format!("{name}-{version}.json"));
    let body = if path.is_file() {
        fs::read_to_string(&path).ok()?
    } else {
        let url = format!("https://crates.io/api/v1/crates/{name}/{version}");
        let mut response = match ureq::get(&url)
            .header("User-Agent", "fm1-grid-build")
            .call()
        {
            Ok(response) => response,
            Err(error) => {
                println!("cargo:warning=license {name} {version}: {error}");
                return None;
            }
        };
        let mut body = String::new();
        let mut reader = response.body_mut().as_reader();
        if let Err(error) = std::io::Read::read_to_string(&mut reader, &mut body) {
            println!("cargo:warning=license {name} {version}: {error}");
            return None;
        }
        let _ = fs::write(&path, &body);
        body
    };
    let parsed: CratesIo = serde_json::from_str(&body).ok()?;
    let license = parsed.version?.license?;
    Some((license, parsed.krate.and_then(|krate| krate.repository)))
}

fn collect_licenses(root: &Path) -> Result<String, String> {
    let lock_path = root.join("Cargo.lock");
    let lock: Lockfile = toml::from_str(
        &fs::read_to_string(&lock_path)
            .map_err(|error| format!("read {}: {error}", lock_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", lock_path.display()))?;

    let cargo_home = cargo_home();
    let mut packages: Vec<_> = lock
        .package
        .iter()
        .filter(|package| package.source.is_some())
        .collect();
    packages.sort_by(|left, right| (&left.name, &left.version).cmp(&(&right.name, &right.version)));

    let mut texts = Vec::<(String, String)>::new();
    let mut seen = HashMap::<String, usize>::new();
    let cache = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("license-texts");
    let _ = fs::create_dir_all(&cache);
    let mut out = String::from(
        "THIRD-PARTY LICENSES\n\n\
         Generated by fm1-grid-app/build.rs. Delete this file and rebuild to regenerate.\n\n\
         Components\n==========\n\n",
    );

    for package in packages {
        let source = package.source.as_deref().unwrap_or("");
        out.push_str(&format!(
            "{name} {version}\n  Source: {source}\n",
            name = package.name,
            version = package.version
        ));
        match package_dir(&cargo_home, package) {
            Some(dir) => {
                let info = manifest_at(&dir).unwrap_or_default();
                let license = info
                    .license
                    .clone()
                    .or_else(|| info.license_file.as_ref().map(|file| format!("see {file}")))
                    .unwrap_or_else(|| "unspecified".into());
                out.push_str(&format!("  License: {license}\n"));
                if let Some(repository) = &info.repository {
                    out.push_str(&format!("  Repository: {repository}\n"));
                }
                let files = license_files(&dir, &info);
                if files.is_empty() {
                    let found = info
                        .license
                        .as_deref()
                        .map(|expression| {
                            expression_texts(expression, &cache, &mut seen, &mut texts)
                        })
                        .unwrap_or_default();
                    if found.is_empty() {
                        out.push_str("  License text: not found in the downloaded crate source\n");
                    } else {
                        out.push_str("  License text:\n");
                        for (id, number) in found {
                            out.push_str(&format!("    SPDX {id} [text {number}]\n"));
                        }
                    }
                } else {
                    out.push_str("  License text:\n");
                    for file in files {
                        let number = license_file_text(&mut seen, &mut texts, &file);
                        let name = file.file_name().unwrap_or_default().to_string_lossy();
                        out.push_str(&format!("    {name} [text {number}]\n"));
                    }
                }
            }
            None => {
                if let Some((license, repository)) =
                    crates_io_info(&package.name, &package.version, &cache)
                {
                    out.push_str(&format!("  License: {license}\n"));
                    if let Some(repository) = repository {
                        out.push_str(&format!("  Repository: {repository}\n"));
                    }
                    let found = expression_texts(&license, &cache, &mut seen, &mut texts);
                    if found.is_empty() {
                        out.push_str("  License text: not available\n");
                    } else {
                        out.push_str("  License text:\n");
                        for (id, number) in found {
                            out.push_str(&format!("    SPDX {id} [text {number}]\n"));
                        }
                    }
                } else {
                    out.push_str("  License: see package metadata\n");
                    out.push_str("  License text: source not downloaded\n");
                }
            }
        }
        out.push('\n');
    }

    // Embedded assets that are not Cargo packages.
    let recovery_manifest = root.join("fm1-recovery/data/assets.json");
    if let Ok(text) = fs::read_to_string(&recovery_manifest) {
        if let Ok(assets) = serde_json::from_str::<RecoveryManifest>(&text) {
            out.push_str(&format!(
                "m-bootloader and loader {version}\n  Source: {source}\n  License: MIT\n",
                version = assets.version,
                source = assets.source
            ));
            let license = root.join("fm1-recovery/data/m-bootloader-LICENSE.txt");
            if license.is_file() {
                let number = license_file_text(&mut seen, &mut texts, &license);
                out.push_str(&format!("  License text: [text {number}]\n"));
            } else {
                out.push_str("  License text: m-bootloader-LICENSE.txt not found\n");
            }
            out.push('\n');
        }
    }

    // Firmware images are third-party works too. Their licenses and source
    // locations live in the firmware manifest; full texts stay with the
    // upstream packages.
    let firmware_manifest = root.join("fm1-firmware/firmware/firmwares.json");
    if let Ok(text) = fs::read_to_string(&firmware_manifest) {
        if let Ok(firmware) = serde_json::from_str::<Vec<FirmwareManifest>>(&text) {
            out.push_str("Embedded firmware\n=================\n\n");
            for entry in firmware.iter().filter(|entry| !entry.index_only) {
                out.push_str(&format!(
                    "{} {}\n  By: {}\n  License: {}\n  Source: {}\n",
                    entry.name,
                    entry.version,
                    if entry.byline.is_empty() {
                        "unknown"
                    } else {
                        &entry.byline
                    },
                    if entry.license.is_empty() {
                        "unspecified"
                    } else {
                        &entry.license
                    },
                    if entry.source.is_empty() {
                        "not recorded"
                    } else {
                        &entry.source
                    },
                ));
                if !entry.license.is_empty() {
                    let found = expression_texts(&entry.license, &cache, &mut seen, &mut texts);
                    for (id, number) in found {
                        out.push_str(&format!("  License text: SPDX {id} [text {number}]\n"));
                    }
                }
                out.push('\n');
            }
        }
    }

    out.push_str("\nLicense texts\n=============\n\n");
    for (number, (title, body)) in texts.iter().enumerate() {
        out.push_str(&format!("--- text {} ({title}) ---\n\n", number + 1));
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    Ok(out)
}

fn workspace_root(manifest_dir: &Path) -> Option<PathBuf> {
    let mut dir = manifest_dir;
    loop {
        if dir.join("Cargo.lock").is_file() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

fn write_third_party_licenses(manifest_dir: &Path) {
    let Some(root) = workspace_root(manifest_dir) else {
        println!("cargo:warning=third-party licenses: workspace root not found");
        return;
    };
    let output = root.join("3rdparty-licenses.txt");
    println!("cargo:rerun-if-changed={}", output.display());
    if output.exists() {
        return;
    }
    match collect_licenses(&root) {
        Ok(text) => match fs::write(&output, text) {
            Ok(()) => println!("cargo:warning=wrote {}", output.display()),
            Err(error) => println!(
                "cargo:warning=could not write {}: {error}",
                output.display()
            ),
        },
        Err(error) => println!("cargo:warning=third-party licenses: {error}"),
    }
}
