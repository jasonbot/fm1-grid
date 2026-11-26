# Embedded firmware

`firmwares.json` is the single source of truth for the images compiled into the
binary: name, version, file, tags, download URL, source repo, SHA-256 and licence,
one object per line. `build.rs` turns that list into the tables used by
`src/builtin.rs` and fetches and verifies the packages against it.
Manage the list with `scripts/firmware.sh add/del/index/baked/list`, then rebuild.

The first tag is the group the firmware dropdown shows (`synth`, `groove`, `chords`,
`drums`, `official`, `other`); `vintage` and `beta` are extra flags.

## Listing-only entries

An entry with `"index_only": true` ships **metadata only**: `build.rs` never fetches
its package and nothing is embedded. The app seeds its catalog with the entry and
offers it as a download from the recorded URL, before and alongside the online
listings. Use it for packages the app may point at but not redistribute; Baud Girl
(FM-1+VA) is the current case. Mark or unmark one with
`scripts/firmware.sh index <name-or-file>` / `baked <name-or-file>`.

The flag is additive: older entries without it keep being baked, and every other
schema field is read exactly as before.

## Downloading the packages

The `.fwsc` files are gitignored. The build downloads the missing **baked** ones from
the recorded URLs and verifies every file against `firmwares.json`, so no separate
step is needed. Downloads go to a `.part` file and are renamed into place only after
the hash matches. A file whose hash doesn't match the recorded one fails the build;
delete it to force a re-download. `index_only` entries are never fetched at build
time.

## Where the packages come from

* The FM-1 Firmware Hub mirrors SLOOP 2.8.5, Felucca 1.8.3 and ChoralRoot 0.46.
  Hashes were checked against the hub's catalogue on 2026-11-26; Felucca 1.8.3
  and ChoralRoot 0.46 were last refreshed on 2026-11-24.
* **Felucca** tracks the hub's 1.8.3 release package.
* **FM1 Stock V16** is M-VAVE's official firmware (`FM-1.fwsc`, released 2026-10-21)
  from m-vave.com/download; the hub does not mirror it yet. The hash was recorded on
  download on 2026-10-25 and still matches on 2026-11-26.
* **FM1 Open V14** is the official open firmware bundled with M-VAVE's web workbench
  (open.m-vave.com/fm1-workbench); the hash was recorded on download on 2026-10-24
  and still matches the V14 release on 2026-11-26.
* **Baud Girl (FM-1+VA)** downloads from baudgirl.com (it is not open source, so the
  hub does not mirror it); the entry is `index_only`, so only its metadata ships and
  the package is never baked into the binary. Version 102 and its hash were checked
  on 2026-11-26.

## Notes

* The packages are used unmodified: no patching, no re-signing.
* Only FM-1 (not SMK-37) packages work in the emulator.
* Licences stay with the authors: GPL-3.0 for everything hub-mirrored, Apache-2.0 for
  FM-1_016, GPLv3 with the FM1 SDK linking exception for FM1 Open V14, proprietary for
  Baud Girl (FM-1+VA), which is metadata-only. The Rust application code is MIT.
