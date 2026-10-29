//! Bundles every `themes/*.json` at build time: emits `THEME_FILES` (id from the
//! file stem, label from the set's `name`, JSON via `include_str!`) into OUT_DIR
//! for `src/theme.rs`. Adding a file to themes/ is all it takes to ship a set.

use std::{env, fs, path::Path};

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
}
