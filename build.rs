//! Generates the embedded firmware table from `firmware/firmwares.json`.
//!
//! The JSON list is the single place that lists what gets compiled into the binary. Use
//! `scripts/firmware.sh` to add or remove entries; the generated code is never edited.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One entry in firmwares.json. Extra fields (url, source, license) are used by
/// firmware/download; serde ignores them here.
#[derive(Deserialize)]
struct Firmware {
    name: String,
    version: String,
    file: String,
    sha256: String,
    tags: Vec<String>,
    /// Hub catalog text; empty when the package isn't in the catalog (M-VAVE's own).
    #[serde(default)]
    description: String,
    #[serde(default)]
    byline: String,
    #[serde(default)]
    device: String,
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let json_path = manifest_dir.join("firmware/firmwares.json");
    println!("cargo:rerun-if-changed={}", json_path.display());

    let text = fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", json_path.display()));
    let list: Vec<Firmware> = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("cannot parse {}: {e}", json_path.display()));

    let mut code = String::from("&[\n");
    for fw in &list {
        if fw.name.is_empty()
            || fw.version.is_empty()
            || fw.file.is_empty()
            || fw.sha256.is_empty()
            || fw.tags.is_empty()
        {
            panic!(
                "{}: entry {:?}: name, version, file, sha256 and a non-empty tags list are required \
                 (the first tag is the dropdown group)",
                json_path.display(),
                fw.name
            );
        }
        let path = manifest_dir.join("firmware").join(&fw.file);
        if !path.is_file() {
            panic!(
                "{}: entry {:?}: no such file {}",
                json_path.display(),
                fw.name,
                path.display()
            );
        }
        println!("cargo:rerun-if-changed={}", path.display());
        code.push_str(&format!(
            "    BuiltinFirmware {{\n        name: \"{}\",\n        version: \"{}\",\n        file: \"{}\",\n        sha256: \"{}\",\n        tags: \"{}\",\n        description: \"{}\",\n        byline: \"{}\",\n        device: \"{}\",\n        bytes: include_bytes!(\"{}\"),\n    }},\n",
            escape(&fw.name),
            escape(&fw.version),
            escape(&fw.file),
            escape(&fw.sha256),
            escape(&fw.tags.join(",")),
            escape(&fw.description),
            escape(&fw.byline),
            escape(&fw.device),
            escape(&path.to_string_lossy()),
        ));
    }
    code.push_str("]\n");

    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("builtins.rs");
    fs::write(&out, code).unwrap();
}
