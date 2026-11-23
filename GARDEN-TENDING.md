# Garden tending

Routine upkeep for the repo. Change the source data, rebuild, and let the build
scripts produce the generated files; do not hand-edit generated tables.

## First checks

Run these before and after any dependency or data refresh:

```sh
cargo test --workspace
cargo build --release -p fm1-grid
```

Launch the app and power at least one pane. For UI changes, run
`scripts/screenshots.sh` and inspect the changed images. The script needs
`Xvfb`, `xdotool` and ImageMagick (`import`, `convert`): it starts a private
Xvfb on `:99`, launches the app with a scratch data dir and null ALSA, powers
four panes, drives the UI at 1440x900 with xdotool, and writes
`screenshot/*.png`. Set `FM1_GRID_BIN=target/release/fm1-grid` to reuse an
existing release build; the script unsets `WAYLAND_DISPLAY` so the window stays
on Xvfb instead of appearing on the desktop.

## Baked-in firmware

`fm1-firmware/firmware/firmwares.json` is the single source of truth for the
firmware compiled into the binary. The `.fwsc` files are gitignored; `build.rs`
downloads any missing baked entry and verifies its recorded SHA-256.
`scripts/firmware.sh` needs `jq`.

Add or replace an entry with:

```sh
scripts/firmware.sh add ~/Downloads/sloop-2.8.6.fwsc SLOOP 2.8.6 "groove,new chip" https://thefirmwarehub.com/fw/sloop-2.8.6.fwsc
scripts/firmware.sh list
```

`add` copies the package into `fm1-firmware/firmware/`, records its hash, and
replaces an entry with the same file name or display name in place, keeping JSON
order. JSON order is the default pane order. After replacing a version, delete
the old `.fwsc` if it is no longer referenced.

`add` writes `name`, `version`, `file`, `tags`, `url` and `sha256`, preserves
`source`, `license` and `index_only`, and drops the prose fields from the old
entry. Restore `description`, `byline`, `device` and `website` by hand when the
upstream package changes. Record where the file came from and when it was
checked in `fm1-firmware/firmware/README.md`.

Use `index_only` for packages that may be listed but not redistributed or baked:

```sh
scripts/firmware.sh index "Baud Girl (FM-1+VA)"   # metadata only
scripts/firmware.sh baked "Baud Girl (FM-1+VA)"   # embed again
```

Index-only entries still require a valid `url`. They are never downloaded by
the build and ship no bytes.

After editing the catalog:

```sh
cargo test -p fm1-firmware
cargo build --release -p fm1-grid
```

If a file's hash does not match, delete that `.fwsc` and rebuild to download it
again. Give every baked entry a download URL; without one a fresh clone cannot
restore the package and the build fails. A package with no `url` must be placed
in `fm1-firmware/firmware/` manually.

## fm1-emu pin

The emulator is pinned by full commit SHA, not by a branch or tag. The same
revision appears in four places:

- `fm1-base/Cargo.toml` — `fm1-soc`
- `fm1-performer/Cargo.toml` — `fm1-core` and `fm1-soc`
- `fm1-grid-app/Cargo.toml` — `fm1-soc`

Pick the upstream commit, replace every old SHA with the same new SHA, then
refresh the lockfile:

```sh
cargo update -p fm1-core -p fm1-soc -p fm1-isa
cargo tree -i fm1-core
cargo test --workspace
cargo build --release -p fm1-grid
```

Commit the manifest changes and `Cargo.lock` together. Check the emulator by
powering a pane, playing notes, and watching the LCD and audio for a few
minutes. If the upstream API changed, fix the callers before committing.

## Rust and crate dependencies

- `cargo update` for routine dependency refreshes; commit `Cargo.lock`.
- `gpui-kit` is pinned exactly (`=0.8.0`) in both `fm1-grid` and
  `fm1-grid-app`, and the test-support dev-dependency uses the same version.
  Bump all three together, then run the full test suite and the screenshots.
- `ratatui`, `midir`, `cpal`, `ureq`, `windows` and the other direct
  dependencies can be updated independently, but major-version bumps need a
  compile and a smoke test of the affected surface.
- The README states the minimum Rust version, while
  `scripts/deploy-devterm.sh` pins the toolchain used by the cross-build image.
  Update both when raising it.
- `.cargo/config.toml` sets `target-cpu=native` for local builds. Leave it
  alone for local work; remove or override it when producing binaries for a
  different machine.

## Recovery binaries

`fm1-recovery/data/assets.json` pins the m-bootloader release, file names, URLs
and SHA-256 values. To upgrade them:

1. Update `source`, `version`, every `file`, `url` and `sha256`.
2. Delete the old gitignored `fm1-recovery/data/*.boot` and `*.bin` files.
3. Build `fm1-recovery` (or the app) so `build.rs` downloads and verifies the
   new files.
4. Update `fm1-recovery/data/m-bootloader-LICENSE.txt` if the upstream licence
   changed, and update the m-bootloader version in `README.md`.

A mismatch fails the build; delete the bad file to force a new download.

## Online firmware feeds

The listings live in `fm1-firmware/src/sources/`:

- `hub.rs` — the Firmware Hub catalog, the primary feed.
- `mutant.rs` — a catalog in the Hub's format.
- `editor.rs`, `reverbose.rs` — scraped hand-maintained pages.
- `shrub.rs`, `jakehandy.rs` — one hosted package each.

When a site changes, update its URL constant, parser and offline parser test.
`download_heuristics.rs` handles links that point at a page rather than a direct
`.fwsc`. If a feed is added or removed, update the `Firmware sources` items in
`fm1-grid-app/src/settings.rs`, the defaults in `HubFeeds::default` in
`fm1-firmware/src/library/model.rs`, and the README source list too. Verify with:

```sh
cargo test -p fm1-firmware
```

The Hub is treated as the source of truth; the other feeds are additive. A
listing's `source:` tag and the device/chip tags affect deduplication and
filtering, so preserve them when editing a parser.

`cargo run --release -p fm1-remotes > firmware.json` prints the merged listing
with every entry resolved to a direct `.fwsc` URL, ready to host for clients
without the download heuristics. Regenerate it after a feed or parser change;
entries the heuristics can't resolve are omitted, so the output is only as
complete as the current sources and network.

## Releases and update feeds

The crate versions are not the release version. Packaging stamps
`FM1_GRID_BUILD_VERSION` into the About tab and the update pill; a plain build
falls back to the crate version. Release tags are `vX.Y.Z`.

Keep the build stamp, artifact and feed version in sync:

- **Windows MSIX**: set `FM1_GRID_MSIX_URL` at build time. It points at a
  `.msix` or `.msixbundle`; Store installs update through the Store and need no
  variable.
- **macOS app**: set `MAC_APP_URL` at build time to a JSON feed
  `{"version":"0.2.0","url":"https://…/FM1-Grid-0.2.0.zip"}`. Build, sign,
  notarize and staple `FM1 Grid.app`, then create the artifact with
  `ditto -c -k --sequesterRsrc --keepParent "FM1 Grid.app" FM1-Grid-0.2.0.zip`.
  The zip must contain the `.app` at its top level (or one directory down). The
  feed version must equal the zip's `CFBundleShortVersionString`; the bundle
  ID must match the installed app. Both the installed app and the update must
  have the same Team ID; unsigned or ad-hoc builds cannot update. Upload the zip
  before publishing the feed. Serve both over HTTPS.
- **Linux Flatpak**: updates come from the Flatpak remote; other Linux builds
  stay on the current version.

A missing update URL disables updates for that platform. Build with the release
stamp, for example:

```sh
FM1_GRID_BUILD_VERSION=0.2.0 \
MAC_APP_URL=https://downloads.example.com/latest.json \
cargo build --release -p fm1-grid
```

The macOS swap needs write access to the installed `.app`; a read-only or
translocated app cannot update itself.

## App icon

`logo.svg` is the icon source. The Windows binaries (`fm1-grid` and `fm1-tty`)
embed it through `build-icon.rs`: the build tries to rasterize the SVG with
ImageMagick 7 (`magick`) and falls back to the committed `logo.ico`. Regenerate
the fallback whenever the logo changes:

```sh
magick -background none logo.svg \
  -define icon:auto-resize=256,128,64,48,32,24,20,16 logo.ico
```

Commit `logo.svg` and `logo.ico` together, rebuild both Windows binaries, and
check the executable icon in Explorer/taskbar. The README logo follows
`logo.svg`. The repo does not contain a macOS `.icns` or a Linux desktop icon;
if a packaging job supplies one, regenerate that artifact from `logo.svg` in
the same change.

## Generated files and housekeeping

- `3rdparty-licenses.txt` is generated and gitignored. After a dependency,
  firmware or recovery change, delete it and rebuild the app so the About →
  Licenses view reflects the new set.
- `screenshot/*.png` is tracked. Run `scripts/screenshots.sh` after visible UI
  changes and review the diff.
- `Cargo.lock` is tracked. Commit it with dependency, emulator-pin or toolchain
  changes.
- `target/`, `*.fwsc`, recovery binaries and `.part` files are ignored; never
  force-add them.
- Remove completed items from `BACKLOG.md`, and keep README instructions in
  sync with the code and scripts.
- If the file ~/projects/homepage/firmware-zoo/firmwares.json exists, refresh
  it by running: `cargo run --release -p fm1-remotes > ~/projects/homepage/firmware-zoo/firmwares.json`
