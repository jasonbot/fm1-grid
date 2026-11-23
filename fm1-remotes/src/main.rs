//! fm1-remotes — print the online FM-1 firmware catalog as one JSON array on
//! stdout. Each row carries the name, version, author, description, device, tags
//! and project links, and a direct `.fwsc` URL resolved through the same download
//! heuristics the app uses (release and install pages become package assets).
//! Entries no `.fwsc` can be found for are omitted.
//!
//! Warnings (an additive feed down, an unresolvable entry) go to stderr, so stdout
//! stays a clean document for hosting. A hub failure exits non-zero without output.

use std::io::Write;

fn main() {
    match fm1_firmware::library::fetch_remotes() {
        Ok(firmwares) => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            serde_json::to_writer(&mut out, &firmwares).expect("serialize the firmware feed");
            writeln!(out).expect("write the firmware feed");
        }
        Err(e) => {
            eprintln!("fm1-remotes: {e}");
            std::process::exit(1);
        }
    }
}
