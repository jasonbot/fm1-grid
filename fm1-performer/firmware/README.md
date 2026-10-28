# Embedded firmware

`firmwares.json` is the single source of truth for the images compiled into the
binary: name, version, file, tags, download URL, source repo, SHA-256 and licence,
one object per line. `build.rs` turns that list into the table used by
`src/builtin.rs` and fetches and verifies the packages against it.
Manage the list with `scripts/firmware.sh add/del/list`, then rebuild.

The first tag is the group the firmware dropdown shows (`synth`, `groove`, `chords`,
`drums`, `official`, `other`); `vintage` and `beta` are extra flags.

## Downloading the packages

The `.fwsc` files are gitignored. The build downloads the missing ones from the
recorded URLs and verifies every file against `firmwares.json`, so no separate step is
needed. Downloads go to a `.part` file and are renamed into place only after the hash
matches. A file whose hash doesn't match the recorded one fails the build; delete it to
force a re-download.

## Where the packages come from

* Everything except the packages below is mirrored by The FM-1 Firmware Hub; hashes
  were checked against the hub's catalogue on 2026-10-25.
* The RedtailWorks pack (LOF-1, ECO-1, GRAV-1, CTY-1) downloads from RedtailWorks'
  own installer pages; hashes were recorded on download on 2026-10-23.
* **FM1 Stock V16** is M-VAVE's official firmware (`FM-1.fwsc`, released 2026-10-21)
  from m-vave.com/download; the hub does not mirror it yet. The hash was recorded on
  download on 2026-10-25.
* **FM1 Open V14** is the official open firmware bundled with M-VAVE's web workbench
  (open.m-vave.com/fm1-workbench); the hash was recorded on download on 2026-10-24.
* **FM-1+VA** downloads from baudgirl.com (it is not open source, so the hub does not
  mirror it); the hash was recorded on download on 2026-10-24.

## Notes

* The packages are used unmodified: no patching, no re-signing.
* Only FM-1 (not SMK-37) packages work in the emulator.
* Licences stay with the authors: GPL-3.0 for everything hub-mirrored, Apache-2.0 for
  FM-1_016, GPLv3 with the FM1 SDK linking exception for FM1 Open V14, proprietary for
  FM-1+VA. The Rust application code is MIT.
