# Embedded firmware

The images compiled into the binary are listed in `manifest.txt` (`file | name | version |
sha256 | tags`); `build.rs` turns it into the table used by `src/builtin.rs`. Manage the list
with `scripts/firmware.sh add/del/list` and rebuild. The tags are The FM-1 Firmware Hub's
categories (`kind`) plus its vintage/beta flags; the first tag is the group the firmware
dropdowns show it under. `firmwares.json` is the point of truth for where each package
came from: name, version, file name, tags, download URL, source repository, SHA-256 and
licence, one object per line. The SHA-256 hashes were checked against the hub's catalogue
on 2026-10-23. The RedtailWorks pack (HYP-1, LOF-1, ECO-1, GRAV-1, CTY-1) is not hub-listed
except HYP-1, whose hash matches the catalogue; the other four packages download from
RedtailWorks' own installer pages, hashes recorded on download on 2026-10-23.

## Downloading the packages

The `.fwsc` files are gitignored. To download the missing packages and verify every file
against the SHA-256 recorded in `firmwares.json`, run from the repo root:

    cargo run --manifest-path firmware/download/Cargo.toml

Files that already hash correctly are skipped; downloads go to a temporary `.part` file
and are renamed into place only after the hash matches. Any mismatch or failed download is
reported and makes the tool exit non-zero, so it can be re-run at any time to re-check
integrity. The tool is the single source file `download/main.rs`; an optional argument
overrides the directory containing `firmwares.json` (default: this `firmware` directory).

## About "FM1 Open"

M-VAVE distributes FM1 Open as source plus a browser Workbench, not as a standalone
`.fwsc` of the open application. The latest development kit (V14) ships M-VAVE's official
`FM-1_015.fwsc` (V15) as its base image, and that is the package embedded here and labelled
**FM1 Open V15**. It is the same package the hub offers as the stock/return-to-official
image. Everything else is a community firmware package mirrored by the hub.

## Not embedded

* **Baud Girl**: the hub lists it as a link-only card (it is not open source, so the hub
  may not mirror it); it installs from its maker's own installer on baudgirl.com.

## Notes

* The packages are used unmodified: no patching, no re-signing.
* Only FM-1 (not SMK-37) packages are accepted by the emulator.
* The firmware licences are the authors'; redistributing a binary that contains them means
  complying with GPL-3.0 (everything hub-mirrored) and Apache-2.0 (FM-1_015). The Rust
  application code is separate and licensed MIT.
