//! The Library tab: every firmware the app knows, as a grouped scrollable list —
//! installed on top (builtins, imported files, downloaded hub packages), the hub's
//! online offers below. Items remove, download and flash (a live unit via the blocking
//! dialog in [`crate::flash_dialog`], or a powered-off unpinned grid slot, which then
//! powers on). At the bottom, the add-a-file form.

use crate::app::{DeviceFilter, GridApp};
use fm1_performer::firmware_picker::{pick_of, FirmwarePick};
use fm1_performer::library::{Firmware, FirmwareDevice, FirmwareSource, Status};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::{h_flex, v_flex, v_virtual_list, ElementExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{TestSupportExt as _, *};
use std::rc::Rc;

/// Tag pill colors are eight hue rotations of the theme's base colour, picked by
/// the tag's hash so the same tag always lands on the same hue. The base is the
/// chosen accent for Influenza/Meltysand and the theme's primary otherwise.
/// Saturation/lightness are clamped so every pill stays readable on light and dark
/// surfaces, and a neutral primary gets a blue hue to rotate.
fn tag_palette(base: Hsla) -> [Hsla; 8] {
    let h = if base.s < 0.05 { 0.58 } else { base.h };
    let s = base.s.clamp(0.5, 0.85);
    let l = base.l.clamp(0.42, 0.60);
    std::array::from_fn(|i| Hsla {
        h: (h + i as f32 / 8.0) % 1.0,
        s,
        l,
        a: 1.0,
    })
}

/// FNV-1a of the tag, into the eight-hue palette.
fn tag_index(tag: &str) -> usize {
    let mut hash = 0x811c_9dc5u32;
    for byte in tag.bytes() {
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
    }
    (hash % 8) as usize
}

/// The glyphs a favorite can show, in menu order.
pub(crate) const GLYPHS: [&str; 5] = ["★", "⮾", "𓅓", "𓅃", "𓃱"];

/// The default favorite glyph.
pub(crate) const FAV_TAG: &str = "★";
/// NoThanks (⮾): such a favorite sorts last and its row greys out.
pub(crate) const NO_THANKS_GLYPH: &str = "⮾";
/// The synthetic tag every non-favorite row carries.
pub(crate) const UNFAV_TAG: &str = "☆";

/// A firmware's favorite key: the name normalized so case and stray whitespace
/// variants (a hub offer vs. the installed entry) share one favorite.
pub(crate) fn favorite_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// The glyph a row shows: its favorite's glyph, or the hollow star when it isn't a
/// favorite.
fn fav_glyph<'a>(
    glyphs: &'a std::collections::HashMap<String, String>,
    name: &str,
    favorite: bool,
) -> &'a str {
    if favorite {
        glyphs
            .get(&favorite_key(name))
            .map(String::as_str)
            .unwrap_or(FAV_TAG)
    } else {
        UNFAV_TAG
    }
}

/// A tag pill's element id component: FNV-1a over name+tag, stable across frames.
pub(crate) fn pill_id(name: &str, tag: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in name.bytes().chain([0]).chain(tag.bytes()) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The row's actions as one button group: ghost buttons fused in a bordered rounded
/// container with hairline dividers. (The component's ButtonGroup would do the fusion,
/// but it overwrites each button's own `disabled` at push time, and this row's buttons
/// gate independently.)
fn action_group(items: Vec<AnyElement>, border: Hsla) -> AnyElement {
    let mut group = h_flex()
        .border_1()
        .border_color(border)
        .rounded_md()
        .overflow_hidden();
    let mut first = true;
    for item in items {
        if !first {
            group = group.child(div().w(px(1.0)).h_full().bg(border));
        }
        first = false;
        group = group.child(item);
    }
    group.into_any_element()
}

/// The filter box's match: every space-separated token of the needle must appear as
/// a case-insensitive substring of name, version, tags (including the row's glyph),
/// byline or description. "f ll gro" finds "Felucca (Groovebox)".
fn matches_filter(firmware: &Firmware, needle: &str, glyph: &str) -> bool {
    let needle = needle.to_lowercase();
    let haystack = format!(
        "{} {} {} {} {} {}",
        firmware.name,
        firmware.version,
        firmware.tags.join(" "),
        glyph,
        firmware.byline,
        firmware.description
    )
    .to_lowercase();
    needle
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

/// Toggle a tag's words in the filter text (a pill click): if every word is already
/// there as a token (case-insensitive), they all come out; otherwise each missing
/// word is appended. Returns the rebuilt filter text.
fn toggle_tag(filter: &str, tag: &str) -> String {
    let words: Vec<&str> = tag.split_whitespace().collect();
    let tokens: Vec<&str> = filter.split_whitespace().collect();
    let present = |word: &str| tokens.iter().any(|token| token.eq_ignore_ascii_case(word));
    if words.iter().all(|word| present(word)) {
        tokens
            .iter()
            .filter(|token| !words.iter().any(|word| token.eq_ignore_ascii_case(word)))
            .copied()
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        let mut out: Vec<&str> = tokens.clone();
        out.extend(words.iter().copied().filter(|word| !present(word)));
        out.join(" ")
    }
}

/// The list's sections, in order: installed first, hub offers at the bottom.
const SECTIONS: [(&str, Section); 4] = [
    ("INSTALLED", Section::Installed),
    ("DOWNLOADING", Section::Downloading),
    ("MISSING", Section::Missing),
    ("AVAILABLE ON THE HUB", Section::Hub),
];

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Installed,
    Downloading,
    Missing,
    Hub,
}

/// The flattened list stream: section headers and firmware rows. The list is a
/// virtual list — only the visible slice is laid out each frame — so every item is
/// a fixed, pinned height (the virtual list positions items from these sizes; a
/// taller render would overlap the next item).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LibItem {
    Header(&'static str),
    /// Index into `Library::list()`'s view.
    Row(usize),
    /// The "no firmware" notice (empty library or a too-narrow filter).
    Empty,
    /// The hub's fetch status, last in the list.
    HubStatus,
}

/// Row: favorite selector + name/version + pills, one-line description, the action
/// group, plus the inter-row slack that used to be the flex gap (a virtual list has
/// no gaps). Tall enough for the large favorite selector in the title row.
const LIB_ROW_H: Pixels = px(96.0);
/// Section header: small caps over a hairline, including the section's top gap.
const LIB_HEADER_H: Pixels = px(28.0);
/// The hub status line: spinner/dot + text + the Download All toggle and Reload.
const LIB_STATUS_H: Pixels = px(34.0);
/// The empty notice's single line.
const LIB_EMPTY_H: Pixels = px(24.0);

impl LibItem {
    fn height(&self) -> Pixels {
        match self {
            LibItem::Header(_) => LIB_HEADER_H,
            LibItem::Row(_) => LIB_ROW_H,
            LibItem::Empty => LIB_EMPTY_H,
            LibItem::HubStatus => LIB_STATUS_H,
        }
    }
}

fn section_of(firmware: &Firmware) -> Section {
    if firmware.status == Status::Downloading {
        return Section::Downloading;
    }
    match &firmware.source {
        FirmwareSource::Hub { blob: None, .. } => Section::Hub,
        _ if firmware.status == Status::Missing => Section::Missing,
        _ => Section::Installed,
    }
}

/// Rows in the Installed section — the app watches for names entering it to play
/// the snap-in animation.
pub(crate) fn is_installed(firmware: &Firmware) -> bool {
    matches!(section_of(firmware), Section::Installed)
}

/// Flatten the library view into the virtual list's stream: sections in order,
/// filtered by the needle and the device checkboxes, with favorites first inside
/// each section and NoThanks favorites last (stable, so the library's own order
/// survives within the groups).
fn flatten_library(
    view: &[Firmware],
    needle: &str,
    favorites: &std::collections::HashSet<String>,
    glyphs: &std::collections::HashMap<String, String>,
    devices: DeviceFilter,
) -> Vec<LibItem> {
    let favorite: Vec<bool> = view
        .iter()
        .map(|firmware| favorites.contains(&favorite_key(&firmware.name)))
        .collect();
    // 0: favorite, 1: plain, 2: NoThanks (sorts to the bottom).
    let rank: Vec<u8> = view
        .iter()
        .enumerate()
        .map(|(ix, firmware)| {
            if !favorite[ix] {
                1
            } else if fav_glyph(glyphs, &firmware.name, true) == NO_THANKS_GLYPH {
                2
            } else {
                0
            }
        })
        .collect();
    let mut items: Vec<LibItem> = Vec::new();
    for (header, section) in SECTIONS {
        let mut indices: Vec<usize> = (0..view.len())
            .filter(|&ix| {
                let firmware = &view[ix];
                section_of(firmware) == section
                    && devices.shows(firmware.device)
                    && matches_filter(
                        firmware,
                        needle,
                        fav_glyph(glyphs, &firmware.name, favorite[ix]),
                    )
            })
            .collect();
        indices.sort_by_key(|&ix| rank[ix]);
        if !indices.is_empty() {
            items.push(LibItem::Header(header));
            items.extend(indices.into_iter().map(LibItem::Row));
        }
    }
    if items.is_empty() {
        items.push(LibItem::Empty);
    }
    // The hub's status sits at the very bottom of the list.
    items.push(LibItem::HubStatus);
    items
}

/// The snap-in curve: a slightly underdamped spring (200 ms response, damping
/// 0.72 — a hint of overshoot), settled well inside the 400 ms the app keeps the
/// entry. `t` in seconds.
pub(crate) fn snap_in(t: f32) -> f32 {
    const W: f32 = 2.0 * std::f32::consts::PI / 0.2;
    const Z: f32 = 0.72;
    let wd = W * (1.0 - Z * Z).sqrt();
    1.0 - (-Z * W * t).exp() * ((wd * t).cos() + (Z * W / wd) * (wd * t).sin())
}

impl GridApp {
    /// The Library tab's content: the grouped list, then the add-file form.
    pub(crate) fn library_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let needle = self.lib_form.filter.read(cx).value().trim().to_string();
        // `list()` folds finished downloads in and re-stats missing files (throttled);
        // calling it here keeps those side effects, then the cached stream below only
        // rebuilds when its view, the filter or the favorites moved.
        self.library.list();
        self.ensure_lib_stream(&needle);
        let view = Rc::clone(&self.lib_view);
        let items = Rc::clone(&self.lib_items);
        let sizes = Rc::clone(&self.lib_sizes);

        // A freshly downloaded firmware jumps into the Installed section; bring it
        // into view. Set by the Download button; clears when hit. (A filter hiding
        // the row just delays the scroll until the filter clears.)
        if let Some(target) = self.lib_scroll_to.clone() {
            let hit = items.iter().position(|item| match item {
                LibItem::Row(ix) => {
                    let firmware = &view[*ix];
                    firmware.name == target && section_of(firmware) == Section::Installed
                }
                _ => false,
            });
            if let Some(ix) = hit {
                self.lib_scroll.scroll_to_item(ix, ScrollStrategy::Center);
                self.lib_scroll_to = None;
            }
        }

        let devices = self.device_filter;
        let device_label = match (devices.fm1, devices.smk37) {
            (true, true) => "FM-1 + SMK-37",
            (true, false) => "FM-1",
            (false, true) => "SMK-37",
            (false, false) => "No devices",
        };
        let device_app = cx.entity().clone();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .pt_2()
                    .gap_2()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.lib_form.filter)
                                .id("library-filter")
                                .prefix(Icon::new(IconName::Search).xsmall()),
                        ),
                    )
                    .child(
                        // Medium, like the filter input beside it.
                        Button::new("library-devices")
                            .label(device_label)
                            .icon(Icon::new(Lucide::ListFilter))
                            .outline()
                            .tooltip("Which device families the library lists")
                            .dropdown_menu({
                                let app = device_app.clone();
                                move |menu, _, _| {
                                    let mut menu = menu;
                                    for (on, label, fm1) in [
                                        (devices.fm1, "FM-1", true),
                                        (devices.smk37, "SMK-37", false),
                                    ] {
                                        let app = app.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(label).checked(on).on_click(
                                                move |_, _, cx| {
                                                    app.update(cx, |this, cx| {
                                                        if fm1 {
                                                            this.device_filter.fm1 =
                                                                !this.device_filter.fm1;
                                                        } else {
                                                            this.device_filter.smk37 =
                                                                !this.device_filter.smk37;
                                                        }
                                                        this.library.set_device_filter(Some((
                                                            this.device_filter.fm1,
                                                            this.device_filter.smk37,
                                                        )));
                                                        this.save_session(cx);
                                                        cx.notify();
                                                    });
                                                },
                                            ),
                                        );
                                    }
                                    menu
                                }
                            }),
                    ),
            )
            .child(
                // The scrollbar pins to this outer element; the scrolling virtual
                // list is inside it (stacking them on one element scrolled the bar
                // away).
                div()
                    .id("library-list")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .vertical_scrollbar(&self.lib_scroll)
                    // PgUp/PgDn scroll by the viewport; record it.
                    .on_prepaint({
                        let app = cx.entity().clone();
                        move |bounds, _, cx| {
                            let height: f32 = bounds.size.height.into();
                            app.update(cx, |this, _| this.lib_view_h = height);
                        }
                    })
                    .child(
                        v_virtual_list(cx.entity().clone(), "library-items", sizes, {
                            let items = Rc::clone(&items);
                            let view = Rc::clone(&view);
                            move |this, range, _window, cx| {
                                range
                                    .map(|ix| this.lib_item(&items[ix], &view, cx))
                                    .collect()
                            }
                        })
                        .track_scroll(&self.lib_scroll)
                        // The scrollbar overlays the list's right edge (16 px,
                        // gpui-base's WIDTH); keep the content out from under it.
                        .p_2()
                        .pr(px(16.0)),
                    ),
            )
            .child(self.library_add_form(window, cx))
    }

    /// Rebuild the flattened row stream and its size table when an input moves.
    /// Favorites sort first within each section.
    fn ensure_lib_stream(&mut self, needle: &str) {
        let key = (
            self.library.view_generation(),
            self.library.generation(),
            needle.to_string(),
            self.favorites_rev,
            self.device_filter.fm1,
            self.device_filter.smk37,
        );
        if self.lib_stream_key.as_ref() == Some(&key) {
            return;
        }
        let view: Vec<Firmware> = self.library.list().to_vec();
        let items = flatten_library(
            &view,
            needle,
            &self.favorites,
            &self.favorite_glyphs,
            self.device_filter,
        );
        let sizes: Vec<Size<Pixels>> = items
            .iter()
            .map(|item| size(px(0.0), item.height()))
            .collect();

        self.lib_view = Rc::new(view);
        self.lib_sizes = Rc::new(sizes);
        self.lib_items = Rc::new(items);
        self.lib_stream_key = Some(key);
    }

    /// One item of the flattened stream, rendered at its pinned height.
    fn lib_item(
        &mut self,
        item: &LibItem,
        view: &[Firmware],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match item {
            LibItem::Header(header) => {
                let (dim, border) = {
                    let theme = cx.theme();
                    (theme.muted_foreground, theme.border)
                };
                v_flex()
                    .w_full()
                    .h(LIB_HEADER_H)
                    .justify_end()
                    .child(
                        div()
                            .w_full()
                            .pb_1()
                            .text_size(px(9.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(dim)
                            .child(*header)
                            .border_b_1()
                            .border_color(border),
                    )
                    .into_any_element()
            }
            LibItem::Row(view_ix) => match view.get(*view_ix).cloned() {
                Some(firmware) => self.library_item(&firmware, cx),
                // The view is fixed while the cached stream is valid; a missing row
                // costs one blank item rather than a panic.
                None => div().w_full().h(LIB_ROW_H).into_any_element(),
            },
            LibItem::Empty => {
                let dim = cx.theme().muted_foreground;
                let needle = self.lib_form.filter.read(cx).value().trim().to_lowercase();
                div()
                    .w_full()
                    .h(LIB_EMPTY_H)
                    .text_size(px(11.0))
                    .text_color(dim)
                    .child(if needle.is_empty() {
                        "No firmwares? That can't be right."
                    } else {
                        "No firmware matches the filter."
                    })
                    .into_any_element()
            }
            LibItem::HubStatus => div()
                .w_full()
                .h(LIB_STATUS_H)
                .overflow_hidden()
                .child(self.hub_status_item(cx))
                .into_any_element(),
        }
    }

    /// The list's last item: the hub's fetch status — a spinner while it loads, then
    /// the Download All toggle and Reload ("Retry" after a failure) as one group.
    fn hub_status_item(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use fm1_performer::library::HubStatus;
        let (dim, danger, border) = {
            let theme = cx.theme();
            (theme.muted_foreground, theme.danger, theme.border)
        };
        let status = self.library.hub_status();
        let loading = matches!(status, HubStatus::Loading);
        let failed = matches!(status, HubStatus::Failed(_));

        let text = match &status {
            HubStatus::Loading => "checking the FM-1 Firmware Hub…".to_string(),
            HubStatus::Failed(e) => format!("the hub didn't answer: {e}"),
            HubStatus::Idle => {
                let n = self.library.hub_len();
                if n > 0 {
                    format!("{n} firmwares on the hub")
                } else {
                    "the hub hasn't been checked".to_string()
                }
            }
        };

        h_flex()
            .id("hub-status")
            .test_support()
            .w_full()
            .items_center()
            .gap_2()
            .pt_2()
            .child(div().w(px(14.0)).h(px(14.0)).child(if loading {
                Spinner::new().into_any_element()
            } else {
                div()
                    .size_full()
                    .rounded_full()
                    .bg(if failed { danger } else { dim })
                    .into_any_element()
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(10.0))
                    .text_color(if failed { danger } else { dim })
                    .child(text),
            )
            .child(action_group(
                vec![
                    // Download All is a toggle: while on, the tick walks the list,
                    // downloading one item at a time (see GridApp::tick).
                    Button::new("hub-download-all")
                        .label("Download All")
                        .icon(Icon::new(Lucide::Download))
                        .xsmall()
                        .ghost()
                        .rounded_none()
                        .selected(self.download_all)
                        .toggled(self.download_all)
                        .tooltip(
                            "Download every downloadable or upgradable firmware, one at a time",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.download_all = !this.download_all;
                            // Turning it on sweeps at once (and retries what failed
                            // last time); from there the tick's pump only advances
                            // on a finished download or a hub swap.
                            this.library.set_download_all(this.download_all);
                            cx.notify();
                        }))
                        .into_any_element(),
                    // Reload shows pressed while the fetch is in flight.
                    Button::new("hub-reload")
                        .label(if failed { "Retry" } else { "Reload" })
                        .icon(Icon::new(Lucide::RefreshCw))
                        .xsmall()
                        .ghost()
                        .rounded_none()
                        .selected(loading)
                        .toggled(loading)
                        .disabled(loading)
                        .tooltip("Fetch every online listing again")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.library.reload_hub();
                            cx.notify();
                        }))
                        .into_any_element(),
                ],
                border,
            ))
            .into_any_element()
    }

    /// Toggle a firmware's favorite. Dropping it forgets its glyph too; the session
    /// is saved at once, like the theme pick — a crash shouldn't lose the toggle.
    pub(crate) fn toggle_favorite(&mut self, name: &str, cx: &App) {
        let key = favorite_key(name);
        if !self.favorites.insert(key.clone()) {
            self.favorites.remove(&key);
            self.favorite_glyphs.remove(&key);
        }
        self.favorites_rev = self.favorites_rev.wrapping_add(1);
        self.save_session(cx);
    }

    /// Give a firmware a favorite glyph, which also favorites it. ★ is the default
    /// and stores no override.
    pub(crate) fn set_favorite_glyph(&mut self, name: &str, glyph: &str, cx: &App) {
        let key = favorite_key(name);
        self.favorites.insert(key.clone());
        if glyph == FAV_TAG {
            self.favorite_glyphs.remove(&key);
        } else {
            self.favorite_glyphs.insert(key, glyph.to_string());
        }
        self.favorites_rev = self.favorites_rev.wrapping_add(1);
        self.save_session(cx);
    }

    /// One firmware row: name + pills over byline + description, then the actions.
    fn library_item(&mut self, firmware: &Firmware, cx: &mut Context<Self>) -> AnyElement {
        let (ink, dim, border) = {
            let theme = cx.theme();
            (theme.foreground, theme.muted_foreground, theme.border)
        };
        let firmware = firmware.clone();
        let name = firmware.name.clone();
        let pick = pick_of(&firmware);
        let smk37 = firmware.device == FirmwareDevice::Smk37;
        let flashable = pick.is_some() && firmware.status == Status::Available && !smk37;
        let downloading = firmware.status == Status::Downloading;
        let percent = self.library.download_percent(&firmware.name);

        // The pills: the favorite glyph (★ by default, ☆ when not a favorite) first,
        // then one per tag, then the device. A tag pill click toggles the tag's words
        // in the filter box. One row only — a taller wrap would break the row's fixed
        // height; extras clip instead.
        let favorite = self.favorites.contains(&favorite_key(&firmware.name));
        let glyph = fav_glyph(&self.favorite_glyphs, &firmware.name, favorite).to_string();
        // A NoThanks favorite greys its row out.
        let no_thanks = favorite && glyph == NO_THANKS_GLYPH;
        let tags = std::iter::once(glyph.clone()).chain(firmware.tags.iter().cloned());
        // The pill hues follow the theme: the accent for the computed families,
        // the primary otherwise. Same tag, same slice of the wheel.
        let base = if matches!(
            self.theme_id.as_str(),
            crate::theme::INFLUENZA | crate::theme::MELTYSAND | crate::theme::VISUALIA
        ) {
            crate::themes::color(crate::themes::accent(&self.accent))
        } else {
            cx.theme().primary
        };
        let palette = tag_palette(base);
        let mut pills = h_flex().gap_1().flex_nowrap().min_w_0().overflow_hidden();
        for tag in tags {
            let color = palette[tag_index(&tag)];
            let id_tag = tag.clone();
            pills = pills.child(
                div()
                    .id(("lib-tag", pill_id(&firmware.name, &id_tag)))
                    .test_support()
                    .rounded_full()
                    .px(px(6.0))
                    .py(px(1.0))
                    .text_size(px(9.0))
                    .bg(color.opacity(0.16))
                    .text_color(color)
                    .cursor_pointer()
                    .hover(move |el| el.bg(color.opacity(0.3)))
                    .tooltip(|window, cx| {
                        Tooltip::new("Toggle this tag in the filter").build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let text = this.lib_form.filter.read(cx).value().to_string();
                        let next = toggle_tag(&text, &tag);
                        this.lib_form.filter.update(cx, |state, cx| {
                            state.set_value(next, window, cx);
                        });
                        this.refocus(window, cx);
                        cx.notify();
                    }))
                    .child(id_tag),
            );
        }
        let device = match firmware.device {
            FirmwareDevice::Fm1 => "FM-1",
            // The model (Pro, Elite, MKE-P37) is the variant tag.
            FirmwareDevice::Smk37 => "SMK-37",
        };
        pills = pills.child(
            div()
                .rounded_full()
                .px(px(6.0))
                .py(px(1.0))
                .text_size(px(9.0))
                .bg(cx.theme().muted)
                .text_color(dim)
                .child(device),
        );

        // The favorite toggle + glyph menu, promoted to the title row and sized
        // large: the glyph pill in the tag row stays its searchable readout.
        let favorite_control = {
            let name = name.clone();
            let glyph = glyph.clone();
            let star_name = name.clone();
            let star = Button::new(format!("lib-fav-{star_name}"))
                .label(glyph.clone())
                .large()
                .ghost()
                .rounded_none()
                .px_1()
                .selected(favorite)
                .toggled(favorite)
                .tooltip(if favorite {
                    "Drop the favorite"
                } else {
                    "Mark as a favorite (its glyph pill filters the list)"
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_favorite(&star_name, cx);
                    cx.notify();
                }))
                .into_any_element();

            let current = glyph.clone();
            let app = cx.entity().clone();
            let caret = Button::new(format!("lib-fav-glyph-{name}"))
                .icon(Icon::new(Lucide::ChevronDown))
                .small()
                .ghost()
                .rounded_none()
                .px_1()
                .tooltip("Choose the favorite's glyph")
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for glyph in GLYPHS {
                        let app = app.clone();
                        let name = name.clone();
                        let shown = glyph.to_string();
                        menu = menu.item(
                            PopupMenuItem::new(glyph)
                                .checked(current == shown)
                                .on_click(move |_, _, cx| {
                                    app.update(cx, |this, cx| {
                                        this.set_favorite_glyph(&name, &shown, cx);
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    menu
                })
                .into_any_element();

            // The same fused, border-only chrome as the row's action group: two
            // ghost hit areas divided by a hairline, no fill of its own.
            action_group(vec![star, caret], border)
        };

        // The actions as one fused group: the website/source link pair (disabled when
        // the listing carries no such link), Remove (never builtins, and only when
        // the package is at hand), Download (hub offers; its percent while in
        // flight), Flash To…
        let locally = pick.is_some() && firmware.status == Status::Available;
        let mut buttons: Vec<AnyElement> = Vec::new();
        for (kind, label, icon, url) in [
            ("website", "Web site", Lucide::Globe, &firmware.website),
            ("source", "Github", Lucide::Github, &firmware.source_url),
        ] {
            let url = url.clone();
            buttons.push(
                Button::new(format!("lib-{kind}-{name}"))
                    .label(label)
                    .icon(Icon::new(icon))
                    .small()
                    .ghost()
                    .rounded_none()
                    .disabled(url.is_none())
                    .tooltip(match &url {
                        Some(url) => format!("Open the {kind}: {url}"),
                        None => format!("No {kind} link known"),
                    })
                    .on_click(move |_, _, cx| {
                        if let Some(url) = &url {
                            cx.open_url(url);
                        }
                    })
                    .into_any_element(),
            );
        }
        if !matches!(firmware.source, FirmwareSource::Builtin) {
            let name = name.clone();
            buttons.push(
                Button::new(format!("lib-remove-{name}"))
                    .label("Remove")
                    .icon(Icon::new(Lucide::Trash))
                    .small()
                    .ghost()
                    .rounded_none()
                    .disabled(!locally)
                    .tooltip(if locally {
                        "Forget this firmware (the package stays on disk)"
                    } else {
                        "Only a firmware that's here can be removed"
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.remove_firmware(&name);
                        this.refresh_firmware_combos(window, cx);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        // Hub offers download; a stored hub blob that's gone (or a web page saved by
        // the page-link bug) offers the download again.
        let downloadable = match &firmware.source {
            FirmwareSource::Hub { blob: None, .. } => true,
            FirmwareSource::Hub { .. } => firmware.status == Status::Missing,
            _ => false,
        };
        if downloadable {
            if downloading {
                buttons.push(
                    Button::new(format!("lib-download-{name}"))
                        .label(format!("{}%", percent.unwrap_or(0)))
                        .icon(Icon::new(Lucide::Download))
                        .small()
                        .ghost()
                        .rounded_none()
                        .disabled(true)
                        .into_any_element(),
                );
            } else {
                let name = name.clone();
                buttons.push(
                    Button::new(format!("lib-download-{name}"))
                        .label("Download")
                        .icon(Icon::new(Lucide::Download))
                        .small()
                        .ghost()
                        .rounded_none()
                        .tooltip("Download from the FM-1 Firmware Hub into the library")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.library.start_download(&name);
                            // On completion the row lands in Installed; scroll to it.
                            this.lib_scroll_to = Some(name.clone());
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        buttons.push(self.flash_to_menu(&firmware, pick, flashable, cx));

        let mut line = String::new();
        if !firmware.byline.is_empty() {
            line.push_str(&format!("by {} — ", firmware.byline));
        }
        line.push_str(&firmware.description);

        // A row the Installed section just gained snaps in: a 6 px rise on the
        // spring curve, faded in over the first 120 ms. Settled entries are gone
        // from lib_added, so a resting row pays one map lookup.
        let (nudge, fade) = match self.lib_added.get(&firmware.name) {
            Some(at) => {
                let t = at.elapsed().as_secs_f32();
                ((1.0 - snap_in(t)) * 6.0, (t / 0.12).min(1.0))
            }
            None => (0.0, 1.0),
        };

        // Fixed height (the virtual list positions rows from LIB_ROW_H): the
        // description is one truncated line and overflow clips.
        v_flex()
            .w_full()
            .h(LIB_ROW_H)
            .overflow_hidden()
            .items_start()
            .gap_1()
            .py_1()
            .pt(px(4.0 + nudge))
            .opacity(fade * if no_thanks { 0.5 } else { 1.0 })
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(ink)
                                    .child(firmware.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(dim)
                                    .child(firmware.version.clone()),
                            )
                            .child(favorite_control),
                    )
                    .child(pills),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(10.5))
                    .text_color(dim)
                    .truncate()
                    .child(line),
            )
            .child(action_group(buttons, border))
            .into_any_element()
    }

    /// The Flash To… dropdown: live flashable units, then powered-off unpinned panes.
    fn flash_to_menu(
        &mut self,
        firmware: &Firmware,
        pick: Option<FirmwarePick>,
        flashable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = firmware.name.clone();
        let units: Vec<String> = self
            .hardware
            .devices()
            .iter()
            .filter(|row| row.flash.is_none())
            .map(|row| row.name.clone())
            .collect();
        let slots: Vec<usize> = (0..fm1_performer::MAX_SLOTS)
            .filter(|&slot| !self.powered[slot] && !self.slots[slot].pinned)
            .collect();
        let has_targets = !units.is_empty() || !slots.is_empty();
        let smk37 = firmware.device == FirmwareDevice::Smk37;
        let app = cx.entity().clone();
        // The dropdown closure is 'static; the firmware name moves in owned.
        let flash_name = firmware.name.clone();
        Button::new(format!("lib-flash-{name}"))
            .label("Flash To…")
            .icon(Icon::new(Lucide::Zap))
            .small()
            .ghost()
            .rounded_none()
            .disabled(smk37 || !flashable || !has_targets)
            .when(smk37, |button| {
                button.tooltip(
                    "SMK-37 firmware installs from its maker's installer; \
                     this app only flashes FM-1 units",
                )
            })
            .when(!smk37 && !(flashable && has_targets), |button| {
                button.tooltip(
                    "Flashable needs the package at hand (builtin, on disk, or downloaded) \
                     and a target (a live FM-1 on USB, or a powered-off unpinned pane)",
                )
            })
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                let Some(pick) = pick.clone() else {
                    return menu;
                };
                for unit in &units {
                    let app = app.clone();
                    let unit = unit.clone();
                    let pick = pick.clone();
                    let name = flash_name.clone();
                    menu = menu.item(PopupMenuItem::new(format!("USB: {unit}")).on_click(
                        move |_, window, cx| {
                            let unit = unit.clone();
                            let pick = pick.clone();
                            let name = name.clone();
                            app.update(cx, |this, cx| {
                                this.flash_to_unit(unit, &name, pick, window, cx)
                            });
                        },
                    ));
                }
                for slot in &slots {
                    let app = app.clone();
                    let slot = *slot;
                    let pick = pick.clone();
                    menu = menu.item(PopupMenuItem::new(format!("Pane {}", slot + 1)).on_click(
                        move |_, window, cx| {
                            let pick = pick.clone();
                            app.update(cx, |this, cx| this.flash_to_slot(slot, pick, window, cx));
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }

    /// Flash a library firmware to a live FM-1 unit: the blocking dialog shows the
    /// flash board's progress and message log.
    pub(crate) fn flash_to_unit(
        &mut self,
        unit: String,
        name: &str,
        pick: FirmwarePick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match pick.bytes(self.store.as_ref()) {
            Ok(bytes) => {
                self.hardware.flash(&unit, bytes);
                crate::flash_dialog::FlashDialog::open(cx.entity(), window, cx, name);
            }
            Err(e) => self.hardware.report(&unit, e),
        }
    }

    /// Flash a library firmware to a grid pane: install it and power the pane on.
    pub(crate) fn flash_to_slot(
        &mut self,
        slot: usize,
        pick: FirmwarePick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.install_pick(slot, pick);
        self.set_power(slot, true);
        self.refresh_firmware_combos(window, cx);
        self.active_tab = 0;
        cx.notify();
    }

    /// The add-file form at the bottom of the list.
    fn library_add_form(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (dim, border) = {
            let theme = cx.theme();
            (theme.muted_foreground, theme.border)
        };
        let chosen = self
            .lib_form
            .file
            .as_ref()
            .map(|path| path.display().to_string());
        let ready = self.lib_form.file.is_some();

        h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_2()
            .p_2()
            .border_t_1()
            .border_color(border)
            .child(
                Button::new("lib-add-file")
                    .label("Add file…")
                    .icon(Icon::new(Lucide::FilePlus))
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| {
                        let Some(path) = rfd::FileDialog::new()
                            .set_title("Choose an FM-1 firmware")
                            .add_filter("FM-1 firmware", &["fwsc", "bin", "xip"])
                            .add_filter("All files", &["*"])
                            .pick_file()
                        else {
                            return;
                        };
                        // Pre-fill the name from the file when the field is empty.
                        if this.lib_form.name.read(cx).value().trim().is_empty() {
                            let stem = path
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            this.lib_form
                                .name
                                .update(cx, |state, cx| state.set_value(stem, window, cx));
                        }
                        this.lib_form.file = Some(path);
                        cx.notify();
                    })),
            )
            .when_some(chosen, |el, path| {
                el.child(
                    div()
                        .text_size(px(10.0))
                        .text_color(dim)
                        .max_w(px(220.0))
                        .truncate()
                        .child(path),
                )
            })
            .child(div().w(px(140.0)).child(Input::new(&self.lib_form.name)))
            .child(div().w(px(90.0)).child(Input::new(&self.lib_form.version)))
            .child(
                div()
                    .w(px(220.0))
                    .flex_1()
                    .min_w(px(140.0))
                    .child(Input::new(&self.lib_form.description)),
            )
            .child(
                Button::new("lib-add-save")
                    .label("Save")
                    .icon(Icon::new(Lucide::Save))
                    .small()
                    .primary()
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let Some(path) = this.lib_form.file.take() else {
                            return;
                        };
                        let name = this.lib_form.name.read(cx).value().trim().to_string();
                        let version = this.lib_form.version.read(cx).value().trim().to_string();
                        let description = this
                            .lib_form
                            .description
                            .read(cx)
                            .value()
                            .trim()
                            .to_string();
                        this.library.import_file(
                            path.to_string_lossy().as_ref(),
                            if name.is_empty() { "imported" } else { &name },
                            &version,
                            &description,
                        );
                        for field in [
                            &this.lib_form.name,
                            &this.lib_form.version,
                            &this.lib_form.description,
                        ] {
                            field.update(cx, |state, cx| state.set_value("", window, cx));
                        }
                        this.refresh_firmware_combos(window, cx);
                        cx.notify();
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    // No `use super::*` — the parent's gpui glob import would drag in GPUI's `test`
    // attribute macro, which re-expands the built-in #[test] (see tests/mod.rs).
    use super::{
        favorite_key, flatten_library, is_installed, matches_filter, snap_in, tag_index,
        tag_palette, toggle_tag, LibItem,
    };
    use super::{DeviceFilter, NO_THANKS_GLYPH};
    use fm1_performer::library::{Firmware, FirmwareDevice, FirmwareSource, Status};
    use std::collections::{HashMap, HashSet};

    /// The pill palette walks the hue wheel from the theme base and clamps to
    /// readable saturation/lightness; a neutral base still gets real hues.
    #[test]
    fn tag_palette_rotates_hues_and_stays_readable() {
        let palette = tag_palette(gpui_kit::hsla(0.0, 0.8, 0.5, 1.0));
        for (i, color) in palette.iter().enumerate() {
            assert!((color.h - i as f32 / 8.0).abs() < 1e-6);
            assert!((0.5..=0.85).contains(&color.s));
            assert!((0.42..=0.60).contains(&color.l));
        }
        assert!(tag_index("groove") < 8);
        assert_eq!(tag_index("groove"), tag_index("groove"));

        let neutral = tag_palette(gpui_kit::hsla(0.0, 0.0, 0.1, 1.0));
        assert!((neutral[0].h - 0.58).abs() < 1e-6);
        assert!(neutral[0].s >= 0.5);
    }

    fn no_glyphs() -> HashMap<String, String> {
        HashMap::new()
    }

    fn firmware(name: &str, tags: &[&str]) -> Firmware {
        Firmware {
            name: name.to_string(),
            byline: "Some Author".to_string(),
            version: "1.0".to_string(),
            description: "A test firmware".to_string(),
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            device: FirmwareDevice::Fm1,
            website: None,
            source_url: None,
            upgradable: false,
            status: Status::Available,
            source: FirmwareSource::Builtin,
        }
    }

    /// The snap-in curve starts at 0, lands at 1 with a slight overshoot on the way,
    /// and stays settled.
    #[test]
    fn snap_in_curve() {
        assert_eq!(snap_in(0.0), 0.0);
        assert!(snap_in(0.1) > 0.5, "well underway at 100 ms");
        let peak = (0..=400)
            .map(|ms| snap_in(ms as f32 / 1000.0))
            .fold(f32::MIN, f32::max);
        assert!(peak > 1.0, "the spring overshoots its target");
        assert!(peak < 1.2, "but only slightly");
        let settled = snap_in(0.4);
        assert!((settled - 1.0).abs() < 0.01, "settled by 400 ms: {settled}");
    }

    /// Only Installed-section rows animate: hub offers, downloads in flight and
    /// missing packages don't count as added.
    #[test]
    fn installed_means_the_installed_section() {
        assert!(is_installed(&firmware("x", &[])));
        let mut offer = firmware("y", &[]);
        offer.source = FirmwareSource::Hub {
            path: "fw/y.fwsc".to_string(),
            blob: None,
        };
        assert!(!is_installed(&offer));
        let mut downloading = firmware("z", &[]);
        downloading.status = Status::Downloading;
        assert!(!is_installed(&downloading));
    }

    /// The filter is space-separated tokens, each a case-insensitive substring of the
    /// row's searchable text; every token must hit.
    #[test]
    fn filter_matches_all_tokens_as_substrings() {
        let felucca = firmware("Felucca", &["groove", "dev"]);
        assert!(matches_filter(&felucca, "", "☆"));
        assert!(matches_filter(&felucca, "fel gro", "☆"));
        assert!(matches_filter(&felucca, "GROOVE Dev", "☆"));
        assert!(matches_filter(&felucca, "author", "☆"));
        assert!(!matches_filter(&felucca, "fel zzz", "☆"));
    }

    /// The synthetic ★/☆ tag is searchable: ★ hits only favorites, ☆ only the rest,
    /// and either composes with the other tokens.
    #[test]
    fn filter_matches_the_synthetic_favorite_tag() {
        let felucca = firmware("Felucca", &["groove"]);
        assert!(matches_filter(&felucca, "★", "★"));
        assert!(!matches_filter(&felucca, "★", "☆"));
        assert!(matches_filter(&felucca, "☆", "☆"));
        assert!(!matches_filter(&felucca, "☆", "★"));
        assert!(matches_filter(&felucca, "★ groove", "★"));
        assert!(!matches_filter(&felucca, "★ groove", "☆"));
        // A favorite's custom glyph is searchable too.
        assert!(matches_filter(&felucca, "𓅓", "𓅓"));
    }

    /// The favorite key ignores case and surrounding whitespace, so differently
    /// cased listings of one firmware share the favorite.
    #[test]
    fn favorite_key_normalizes() {
        assert_eq!(favorite_key("Felucca"), favorite_key(" felucca "));
        assert_ne!(favorite_key("Felucca"), favorite_key("Beep"));
    }

    /// Favorites float to the top of their own section, in a stable order, and a
    /// favorite hub offer does not jump above the installed rows.
    #[test]
    fn favorites_float_to_the_top() {
        let mut view = vec![
            firmware("Alpha", &[]),
            firmware("Beta", &[]),
            firmware("Gamma", &[]),
        ];
        let mut offer = firmware("Delta", &[]);
        offer.source = FirmwareSource::Hub {
            path: "fw/delta.fwsc".to_string(),
            blob: None,
        };
        view.push(offer);
        let favorites: HashSet<String> = ["gamma".to_string(), "delta".to_string()]
            .into_iter()
            .collect();
        let items = flatten_library(&view, "", &favorites, &no_glyphs(), DeviceFilter::default());
        assert_eq!(items[0], LibItem::Header("INSTALLED"));
        assert_eq!(
            items[1..4].to_vec(),
            vec![LibItem::Row(2), LibItem::Row(0), LibItem::Row(1)]
        );
        let hub = items
            .iter()
            .position(|item| *item == LibItem::Header("AVAILABLE ON THE HUB"))
            .expect("the hub section");
        assert_eq!(items[hub + 1], LibItem::Row(3));
        // The filter still applies after the sort.
        let filtered = flatten_library(
            &view,
            "bet",
            &favorites,
            &no_glyphs(),
            DeviceFilter::default(),
        );
        assert_eq!(filtered[1..2].to_vec(), vec![LibItem::Row(1)]);
    }

    /// A pill click toggles the tag's words: appended when any is missing as a token
    /// (case-insensitive), all removed when all are present.
    #[test]
    fn tag_toggles_in_filter() {
        assert_eq!(toggle_tag("", "groove"), "groove");
        assert_eq!(toggle_tag("groove", "groove"), "");
        assert_eq!(toggle_tag("groove dev", "dev"), "groove");
        assert_eq!(toggle_tag("GROOVE", "groove"), "");
        // Multi-word tags toggle as a unit.
        assert_eq!(toggle_tag("groove", "dev tools"), "groove dev tools");
        assert_eq!(toggle_tag("groove dev tools", "dev tools"), "groove");
        assert_eq!(toggle_tag("groove dev", "dev tools"), "groove dev tools");
        // Presence is exact tokens: a bare substring does not count.
        assert_eq!(toggle_tag("groo", "groove"), "groo groove");
    }

    /// A NoThanks favorite sorts to the bottom of its section, below plain rows,
    /// and its custom glyph still matches the filter.
    #[test]
    fn no_thanks_favorite_sinks_to_the_bottom() {
        let view = vec![
            firmware("Alpha", &[]),
            firmware("Beta", &[]),
            firmware("Gamma", &[]),
        ];
        let favorites: HashSet<String> = ["alpha".to_string(), "gamma".to_string()]
            .into_iter()
            .collect();
        let glyphs: HashMap<String, String> =
            [("gamma".to_string(), NO_THANKS_GLYPH.to_string())].into();
        let items = flatten_library(&view, "", &favorites, &glyphs, DeviceFilter::default());
        // Alpha (favorite) leads, Beta (plain) follows, Gamma (NoThanks) last.
        assert_eq!(
            items[1..4].to_vec(),
            vec![LibItem::Row(0), LibItem::Row(1), LibItem::Row(2)]
        );
        let filtered = flatten_library(
            &view,
            NO_THANKS_GLYPH,
            &favorites,
            &glyphs,
            DeviceFilter::default(),
        );
        assert_eq!(filtered[1..2].to_vec(), vec![LibItem::Row(2)]);
    }

    /// The device checkboxes are a hard filter: FM-1 only by default, SMK-37 when
    /// asked for.
    #[test]
    fn device_checkboxes_filter_the_list() {
        let mut smk = firmware("Smk Thing", &[]);
        smk.device = FirmwareDevice::Smk37;
        let view = vec![firmware("Fm One", &[]), smk];
        let favorites = HashSet::new();
        let fm1_only =
            flatten_library(&view, "", &favorites, &no_glyphs(), DeviceFilter::default());
        assert_eq!(fm1_only[1..2].to_vec(), vec![LibItem::Row(0)]);
        let both = flatten_library(
            &view,
            "",
            &favorites,
            &no_glyphs(),
            DeviceFilter {
                fm1: true,
                smk37: true,
            },
        );
        assert_eq!(both[1..3].to_vec(), vec![LibItem::Row(0), LibItem::Row(1)]);
        let smk_only = flatten_library(
            &view,
            "",
            &favorites,
            &no_glyphs(),
            DeviceFilter {
                fm1: false,
                smk37: true,
            },
        );
        assert_eq!(smk_only[1..2].to_vec(), vec![LibItem::Row(1)]);
    }
}
