//! The searchable firmware list behind the sidebar's combobox: the embedded builtins
//! grouped by hub category, plus the "Other…" file escape hatch while unfiltered.
//! egui needed a hand-rolled ComboBox popup for this; gpui-kit's `Combobox` provides
//! the search box and the list, this module only provides the data.

use crate::builtin::{BUILTINS, groups};
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

/// What a pick produces: an embedded firmware by index into `BUILTINS`, or the
/// "Other…" file dialog (`None`).
pub(crate) type Pick = Option<usize>;

#[derive(Clone)]
pub(crate) struct FirmwareItem {
    value: Pick,
    title: SharedString,
    /// Lowercase name + version + tags, searched by the filter box.
    haystack: String,
    /// Tooltip: the package file name and its SHA-256 (provenance from the hub).
    tip: SharedString,
}

impl SearchableListItem for FirmwareItem {
    type Value = Pick;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }

    /// An empty needle matches everything; otherwise it must appear in the name, the
    /// version or the tags.
    fn matches(&self, query: &str) -> bool {
        self.haystack.contains(&query.to_lowercase())
    }
}

pub(crate) struct FirmwareDelegate {
    /// All builtins in firmwares.json order, then the "Other…" entry as the last item.
    all: Vec<FirmwareItem>,
    /// The current view: (category header, indices into `all`). The `None` header is
    /// the "Other…" section — separated, but unlabeled.
    sections: Vec<(Option<&'static str>, Vec<usize>)>,
}

impl FirmwareDelegate {
    pub(crate) fn new() -> Self {
        let mut all: Vec<FirmwareItem> = BUILTINS
            .iter()
            .enumerate()
            .map(|(index, firmware)| FirmwareItem {
                value: Some(index),
                title: firmware.label().into(),
                haystack: format!(
                    "{} {} {}",
                    firmware.name, firmware.version, firmware.tags
                )
                .to_lowercase(),
                tip: format!("{}\nSHA-256 {}", firmware.file, firmware.sha256).into(),
            })
            .collect();
        all.push(FirmwareItem {
            value: None,
            title: "Other…".into(),
            haystack: String::new(),
            tip: "Load a firmware file from disk".into(),
        });
        let mut delegate = Self {
            all,
            sections: Vec::new(),
        };
        delegate.refilter("");
        delegate
    }

    /// Group the matching firmwares by the hub's categories; groups with no match
    /// disappear while filtering.
    fn refilter(&mut self, query: &str) {
        let needle = query.trim().to_lowercase();
        let mut sections: Vec<(Option<&'static str>, Vec<usize>)> = Vec::new();
        for (category, indices) in groups() {
            let hits: Vec<usize> = indices
                .into_iter()
                .filter(|&index| self.all[index].matches(&needle))
                .collect();
            if !hits.is_empty() {
                sections.push((Some(category), hits));
            }
        }
        // The file dialog is the escape hatch out of the builtin list, not a firmware:
        // it only shows while the filter is empty.
        if needle.is_empty() {
            sections.push((None, vec![self.all.len() - 1]));
        }
        self.sections = sections;
    }
}

impl SearchableListDelegate for FirmwareDelegate {
    type Item = FirmwareItem;

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
            .map(|&index| &self.all[index])
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
                    (self.all[index].value() == value)
                        .then_some(IndexPath::new(row).section(section))
                })
            })
    }

    fn perform_search(&mut self, query: &str, _: &mut Window, _: &mut App) -> Task<()> {
        self.refilter(query);
        Task::ready(())
    }

    /// Rows carry the package's provenance tooltip; the trailing check mirrors the
    /// current selection (the default row layout is suppressed on override).
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
        Some(
            div()
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
                })
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .into_any_element(),
        )
    }

    /// The quiet caps group headers the egui dropdown drew, with a rule between groups.
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
        let mut delegate = FirmwareDelegate::new();
        delegate.refilter("nothing matches this");
        assert_eq!(delegate.sections.len(), 0);
        assert_eq!(delegate.items_count(0), 0);
        assert!(delegate.item(IndexPath::new(0)).is_none());
    }
}
