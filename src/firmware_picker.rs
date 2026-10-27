//! The firmware dropdown's data: the library's available entries (builtins, imported
//! files, downloaded hub packages), grouped by their first tag. There is no "Other…"
//! file escape hatch anymore — imports happen on the Library tab. gpui-kit's
//! `Combobox` provides the search box and the list; this module provides the data and
//! what a pick means.

#[cfg(feature = "gui")]
use crate::app::GridApp;
use crate::builtin::BUILTINS;
#[cfg(feature = "gui")]
use crate::library::Status;
use crate::library::FirmwareSource;
#[cfg(feature = "gui")]
use gpui_kit::component::searchable_list::{SearchableListDelegate, SearchableListItem};
#[cfg(feature = "gui")]
use gpui_kit::component::tooltip::Tooltip;
#[cfg(feature = "gui")]
use gpui_kit::component::{ActiveTheme, IndexPath};
#[cfg(feature = "gui")]
use gpui_kit::prelude::FluentBuilder;
// Explicit imports, no glob: the glob imports GPUI's `test` attribute macro under
// test-support builds, which then re-expands this module's own `#[test]`s.
#[cfg(feature = "gui")]
use gpui_kit::{
    AnyElement, App, ElementId, FontWeight, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    Window, div, px,
};
use std::path::PathBuf;

/// What a pick installs (or flashes).
#[derive(Clone, PartialEq)]
pub enum FirmwarePick {
    Builtin(usize),
    File(PathBuf),
    /// A downloaded hub package, by its blob-store key.
    HubBlob(String),
}

impl FirmwarePick {
    /// The package bytes: the builtin table, the filesystem, or the blob store.
    pub fn bytes(&self, store: Option<&crate::store::Store>) -> Result<Vec<u8>, String> {
        match self {
            FirmwarePick::Builtin(index) => Ok(BUILTINS[*index].bytes.to_vec()),
            FirmwarePick::File(path) => {
                std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
            }
            FirmwarePick::HubBlob(key) => store
                .ok_or("no on-disk store".to_string())
                .and_then(|store| store.fetch_blob(key))
                .and_then(unwrap_fwsc),
        }
    }

}

/// Some download URLs wrap the package in a zip; installing pulls the first .fwsc
/// member out. Anything that isn't a zip passes through untouched — except a web
/// page: a listing that linked an HTML page instead of a package used to download
/// the page itself, and booting it does nothing. Fail those loudly.
fn unwrap_fwsc(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if !bytes.starts_with(b"PK\x03\x04") {
        if crate::library::looks_like_web_page(&bytes[..bytes.len().min(512)]) {
            return Err(
                "the download is a web page, not a firmware package — download it again"
                    .to_string(),
            );
        }
        return Ok(bytes);
    }
    zip_first_fwsc(&bytes)
}

/// The first `.fwsc` member of a zip, found through the central directory (local
/// headers may carry zeroed sizes with the data in a trailing descriptor, so the
/// central directory is the only reliable index). Methods: stored and deflated.
fn zip_first_fwsc(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let u16_at = |at: usize| -> Result<u16, String> {
        let field = bytes
            .get(at..at + 2)
            .ok_or_else(|| "zip: truncated archive".to_string())?;
        Ok(u16::from_le_bytes([field[0], field[1]]))
    };
    let u32_at = |at: usize| -> Result<u32, String> {
        let field = bytes
            .get(at..at + 4)
            .ok_or_else(|| "zip: truncated archive".to_string())?;
        Ok(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
    };
    // The end-of-central-directory record sits within the trailing 64 KiB + 22.
    let scan_from = bytes.len().saturating_sub(22 + 65_535);
    let eocd = bytes[scan_from..]
        .windows(22)
        .rposition(|window| window[..4] == *b"PK\x05\x06")
        .map(|i| scan_from + i)
        .ok_or_else(|| "zip: no end of central directory".to_string())?;
    let entries = u16_at(eocd + 10)? as usize;
    let mut directory = u32_at(eocd + 16)? as usize;
    for _ in 0..entries {
        if bytes.get(directory..directory + 4) != Some(b"PK\x01\x02") {
            return Err("zip: bad central directory".to_string());
        }
        let method = u16_at(directory + 10)?;
        let packed = u32_at(directory + 20)? as usize;
        let unpacked = u32_at(directory + 24)? as usize;
        let name_len = u16_at(directory + 28)? as usize;
        let extra_len = u16_at(directory + 30)? as usize;
        let comment_len = u16_at(directory + 32)? as usize;
        let local = u32_at(directory + 42)? as usize;
        let name = bytes
            .get(directory + 46..directory + 46 + name_len)
            .ok_or_else(|| "zip: truncated archive".to_string())?;
        let name = String::from_utf8_lossy(name).into_owned();
        if name.to_ascii_lowercase().ends_with(".fwsc") {
            if bytes.get(local..local + 4) != Some(b"PK\x03\x04") {
                return Err(format!("zip: {name} has a bad local header"));
            }
            let local_name = u16_at(local + 26)? as usize;
            let local_extra = u16_at(local + 28)? as usize;
            let data = bytes
                .get(local + 30 + local_name + local_extra..local + 30 + local_name + local_extra + packed)
                .ok_or_else(|| "zip: truncated archive".to_string())?;
            let member = match method {
                0 => data.to_vec(),
                8 => {
                    use std::io::Read;
                    let mut out = Vec::with_capacity(unpacked);
                    flate2::read::DeflateDecoder::new(data)
                        .read_to_end(&mut out)
                        .map_err(|e| format!("zip: inflate {name}: {e}"))?;
                    out
                }
                other => return Err(format!("zip: {name} uses unsupported method {other}")),
            };
            if member.len() != unpacked {
                return Err(format!(
                    "zip: {name} unpacked to {} bytes, expected {unpacked}",
                    member.len()
                ));
            }
            return Ok(member);
        }
        directory += 46 + name_len + extra_len + comment_len;
    }
    Err("zip: no .fwsc member".to_string())
}

/// The install/flash target for a library entry: None when the package isn't at hand
/// (a hub offer not downloaded yet, or a file that's gone).
pub fn pick_of(firmware: &crate::library::Firmware) -> Option<FirmwarePick> {
    match &firmware.source {
        FirmwareSource::Builtin => BUILTINS
            .iter()
            .position(|b| b.name == firmware.name && b.version == firmware.version)
            .map(FirmwarePick::Builtin),
        FirmwareSource::File { path } => Some(FirmwarePick::File(PathBuf::from(path))),
        FirmwareSource::Hub { blob: Some(key), .. } => Some(FirmwarePick::HubBlob(key.clone())),
        FirmwareSource::Hub { blob: None, .. } => None,
    }
}

// Everything below is the gpui-kit combobox delegate; fm1-tty uses only the pick
// data above.
#[cfg(feature = "gui")]
#[derive(Clone)]
pub(crate) struct Item {
    pick: FirmwarePick,
    title: SharedString,
    /// Lowercase name + version + tags, searched by the filter box.
    haystack: String,
    /// Tooltip: byline and description, then the file and its digest when known.
    tip: SharedString,
    /// The group header: the first tag.
    group: String,
}

#[cfg(feature = "gui")]
impl SearchableListItem for Item {
    type Value = FirmwarePick;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.pick
    }

    fn matches(&self, query: &str) -> bool {
        self.haystack.contains(&query.to_lowercase())
    }
}

#[cfg(feature = "gui")]
#[derive(Default)]
pub(crate) struct FirmwareDelegate {
    items: Vec<Item>,
    /// (group header, indices into `items`) — the current (filtered) view.
    sections: Vec<(Option<String>, Vec<usize>)>,
}

#[cfg(feature = "gui")]
impl FirmwareDelegate {
    /// The library's available entries as pickable items. Called when a combobox is
    /// built and again whenever the library changes (`GridApp::refresh_firmware_combos`).
    pub(crate) fn from_app(app: &mut GridApp) -> Self {
        let mut items = Vec::new();
        for firmware in app.library.list_clone() {
            if firmware.status != Status::Available {
                continue;
            }
            let Some(pick) = pick_of(&firmware) else {
                continue;
            };
            let mut tip = String::new();
            if !firmware.byline.is_empty() {
                tip.push_str(&format!("by {}\n", firmware.byline));
            }
            if !firmware.description.is_empty() {
                tip.push_str(&firmware.description);
                tip.push('\n');
            }
            if let FirmwarePick::Builtin(index) = &pick {
                let b = &BUILTINS[*index];
                tip.push_str(&format!("{}\nSHA-256 {}", b.file, b.sha256));
            }
            items.push(Item {
                pick,
                title: format!("{} {}", firmware.name, firmware.version).into(),
                haystack: format!(
                    "{} {} {}",
                    firmware.name,
                    firmware.version,
                    firmware.tags.join(" ")
                )
                .to_lowercase(),
                tip: tip.trim_end().into(),
                group: firmware
                    .tags
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "other".into()),
            });
        }
        let mut delegate = Self {
            items,
            sections: Vec::new(),
        };
        delegate.refilter("");
        delegate
    }

    /// Group the matching items by their first tag; empty groups disappear.
    fn refilter(&mut self, query: &str) {
        let needle = query.trim().to_lowercase();
        let mut sections: Vec<(Option<String>, Vec<usize>)> = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            if !item.matches(&needle) {
                continue;
            }
            match sections
                .iter_mut()
                .find(|(group, _)| group.as_deref() == Some(item.group.as_str()))
            {
                Some((_, indices)) => indices.push(index),
                None => sections.push((Some(item.group.clone()), vec![index])),
            }
        }
        self.sections = sections;
    }
}

#[cfg(feature = "gui")]
impl SearchableListDelegate for FirmwareDelegate {
    type Item = Item;

    fn sections_count(&self, _: &App) -> usize {
        self.sections.len()
    }

    fn items_count(&self, section: usize) -> usize {
        // The list may probe a stale section index right after a search emptied the
        // view; tolerate it (gpui-component's own delegates do the same).
        self.sections.get(section).map_or(0, |s| s.1.len())
    }

    fn item(&self, ix: IndexPath) -> Option<&Self::Item> {
        self.sections
            .get(ix.section)?
            .1
            .get(ix.row)
            .map(|&index| &self.items[index])
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Self::Item: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        self.sections
            .iter()
            .enumerate()
            .find_map(|(section, (_, indices))| {
                indices.iter().enumerate().find_map(|(row, &index)| {
                    (self.items[index].value() == value)
                        .then_some(IndexPath::new(row).section(section))
                })
            })
    }

    fn perform_search(&mut self, query: &str, _: &mut Window, _: &mut App) -> Task<()> {
        self.refilter(query);
        Task::ready(())
    }

    /// Rows carry the provenance tooltip; the trailing check mirrors the current
    /// selection (the default row layout is suppressed on override).
    fn render_item(
        &self,
        ix: IndexPath,
        item: &Self::Item,
        checked: bool,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let (ink, accent) = {
            let theme = cx.theme();
            (theme.foreground, theme.primary)
        };
        let tip = item.tip.clone();
        let mut row = div()
            .id(ElementId::from(ix))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .px_2()
            .py_1()
            .child(
                div()
                    .truncate()
                    .text_color(ink)
                    .child(item.title()),
            )
            .when(checked, |el| {
                el.child(div().text_color(accent).child("✓"))
            });
        if !tip.is_empty() {
            row = row.tooltip(move |window, cx| {
                Tooltip::new(tip.clone()).build(window, cx)
            });
        }
        Some(row.into_any_element())
    }

    /// The quiet caps group headers, with a rule above all but the first.
    fn render_section_header(
        &self,
        section: usize,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let (category, _) = self.sections.get(section)?;
        let (muted, border) = {
            let theme = cx.theme();
            (theme.muted_foreground, theme.border)
        };
        let header = div().px_2().pt_2().pb_1().text_size(px(9.0)).text_color(muted);
        Some(
            match category {
                Some(category) => header
                    .font_weight(FontWeight::BOLD)
                    .child(category.to_uppercase()),
                None => header.child(
                    div().h(px(1.0)).w_full().bg(border),
                ),
            }
            .into_any_element(),
        )
    }
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_result_is_safe() {
        // A search that matches nothing leaves no sections; the list must be able to
        // probe section 0 without panicking (it crashed once: index out of bounds).
        let mut delegate = FirmwareDelegate::default();
        delegate.refilter("nothing matches this");
        assert_eq!(delegate.sections.len(), 0);
        assert_eq!(delegate.items_count(0), 0);
        assert!(delegate.item(IndexPath::new(0)).is_none());
    }
}

#[cfg(test)]
mod zip_tests {
    use super::*;

    /// A minimal zip with the given members (name, method, plain content); CRCs are
    /// zero — the reader takes the central directory's sizes, not its checksums.
    fn zip_with(members: &[(&str, u16, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, method, content) in members {
            let stored = match method {
                0 => content.to_vec(),
                8 => {
                    let mut encoder = flate2::write::DeflateEncoder::new(
                        Vec::new(),
                        flate2::Compression::fast(),
                    );
                    encoder.write_all(content).unwrap();
                    encoder.finish().unwrap()
                }
                other => panic!("test zip: method {other}"),
            };
            let offset = out.len() as u32;
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 8]); // time, date, crc
            out.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            out.extend_from_slice(&(content.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0; 2]); // extra length
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&stored);

            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&20u16.to_le_bytes()); // version made by
            central.extend_from_slice(&20u16.to_le_bytes()); // version needed
            central.extend_from_slice(&0u16.to_le_bytes()); // flags
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 8]); // time, date, crc
            central.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            central.extend_from_slice(&(content.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 8]); // extra, comment, disk, attributes
            central.extend_from_slice(&[0; 4]); // external attributes
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let directory_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&[0; 4]); // disk numbers
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&[0; 2]); // comment length
        out
    }

    #[test]
    fn zip_yields_its_fwsc_member_stored_or_deflated() {
        // The .fwsc is picked by name, not position: a readme comes first.
        let zip = zip_with(&[
            ("README.txt", 0, b"read me first"),
            ("firmware/pack.fwsc", 0, b"the package bytes"),
        ]);
        assert_eq!(unwrap_fwsc(zip).unwrap(), b"the package bytes");
        let zip = zip_with(&[("pack.fwsc", 8, b"the deflated package bytes")]);
        assert_eq!(unwrap_fwsc(zip).unwrap(), b"the deflated package bytes");
    }

    #[test]
    fn non_zip_bytes_pass_through_and_zip_without_fwsc_fails() {
        assert_eq!(unwrap_fwsc(b"raw firmware".to_vec()).unwrap(), b"raw firmware");
        let zip = zip_with(&[("notes.txt", 0, b"hi")]);
        assert_eq!(unwrap_fwsc(zip).unwrap_err(), "zip: no .fwsc member");
        assert!(unwrap_fwsc(b"PK\x03\x04 too short".to_vec()).is_err());
    }

    #[test]
    fn hub_blob_pick_unpacks_the_fwsc() {
        let dir = std::env::temp_dir().join(format!("fm1-grid-zip-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = crate::store::Store::in_dir(&dir);
        let zip = zip_with(&[("pack.fwsc", 8, b"flashable bytes")]);
        store.store_blob("hub/pack.zip", &zip).unwrap();
        let pick = FirmwarePick::HubBlob("hub/pack.zip".to_string());
        assert_eq!(pick.bytes(Some(&store)).unwrap(), b"flashable bytes");
        std::fs::remove_dir_all(dir).ok();
    }

    /// A listing that linked an HTML page used to store the page as the "package";
    /// booting it did nothing. Those blobs fail loudly now.
    #[test]
    fn html_blobs_are_rejected() {
        assert!(unwrap_fwsc(b"<!DOCTYPE html><html><body>page</body></html>".to_vec()).is_err());
        assert!(unwrap_fwsc(b"<html><body>page</body></html>".to_vec()).is_err());
        assert!(unwrap_fwsc(b"  \n<!doctype html>".to_vec()).is_err());
        // Real packages and other binaries still pass through.
        assert_eq!(unwrap_fwsc(b"fw-binary-bytes".to_vec()).unwrap(), b"fw-binary-bytes");
    }
}
