//! Generates the embedded firmware table from `firmware/manifest.txt`.
//!
//! The manifest is the single place that lists what gets compiled into the binary. Use
//! `scripts/firmware.sh` to add or remove entries; the generated code is never edited.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let manifest_path = manifest_dir.join("firmware/manifest.txt");
    println!("cargo:rerun-if-changed={}", manifest_path.display());

    let text = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest_path.display()));

    let mut code = String::from("&[\n");
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        if fields.len() != 4 || fields.iter().any(|field| field.is_empty()) {
            panic!(
                "{}:{}: expected `file | name | version | sha256`",
                manifest_path.display(),
                line_number + 1
            );
        }
        let (file, name, version, sha256) = (fields[0], fields[1], fields[2], fields[3]);
        let path = manifest_dir.join("firmware").join(file);
        if !path.is_file() {
            panic!(
                "{}:{}: no such file {}",
                manifest_path.display(),
                line_number + 1,
                path.display()
            );
        }
        println!("cargo:rerun-if-changed={}", path.display());
        code.push_str(&format!(
            "    BuiltinFirmware {{\n        name: \"{}\",\n        version: \"{}\",\n        file: \"{}\",\n        sha256: \"{}\",\n        bytes: include_bytes!(\"{}\"),\n    }},\n",
            escape(name),
            escape(version),
            escape(file),
            escape(sha256),
            escape(&path.to_string_lossy()),
        ));
    }
    code.push_str("]\n");

    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("builtins.rs");
    fs::write(&out, code).unwrap();
}
