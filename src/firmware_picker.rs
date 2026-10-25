//! The firmware dropdown's data: the library's available entries (builtins, imported
//! files, downloaded hub packages), grouped by their first tag. There is no "Other…"
//! file escape hatch anymore — imports happen on the Library tab. gpui-kit's
//! `Combobox` provides the search box and the list; this module provides the data and
//! what a pick means.

use crate::app::GridApp;
use crate::builtin::BUILTINS;
use crate::library::{FirmwareSource, Status};
use gpui_kit::component::searchable_list::{SearchableListDelegate, SearchableListItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, IndexPath};
use gpui_kit::prelude::FluentBuilder;
// Explicit imports, no glob: the glob imports GPUI's `test` attribute macro under
// test-support builds, which then re-expands this module's own `#[test]`s.
use gpui_kit::{
    AnyElement, App, ElementId, FontWeight, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    Window, div, px,
};
use std::path::PathBuf;

/// What a pick installs (or flashes).
#[derive(Clone, PartialEq)]
pub(crate) enum FirmwarePick {
    Builtin(usize),
    File(PathBuf),
    /// A downloaded hub package, by its blob-store key.
    HubBlob(String),
}

impl FirmwarePick {
    /// The package bytes: the builtin table, the filesystem, or the blob store.
    pub(crate) fn bytes(&self, store: Option<&crate::store::Store>) -> Result<Vec<u8>, String> {
        match self {
            FirmwarePick::Builtin(index) => Ok(BUILTINS[*index].bytes.to_vec()),
            FirmwarePick::File(path) => {
                std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
            }
            FirmwarePick::HubBlob(key) => store
                .ok_or("no on-disk store".to_string())
                .and_then(|store| store.fetch_blob(key)),
        }
    }

}

/// The install/flash target for a library entry: None when the package isn't at hand
/// (a hub offer not downloaded yet, or a file that's gone).
pub(crate) fn pick_of(firmware: &crate::library::Firmware) -> Option<FirmwarePick> {
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

#[derive(Default)]
pub(crate) struct FirmwareDelegate {
    items: Vec<Item>,
    /// (group header, indices into `items`) — the current (filtered) view.
    sections: Vec<(Option<String>, Vec<usize>)>,
}

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

#[cfg(test)]
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
