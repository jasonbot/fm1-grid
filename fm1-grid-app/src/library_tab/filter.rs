use super::*;

pub(crate) fn tag_palette(base: Hsla) -> [Hsla; 8] {
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
pub(crate) fn tag_index(tag: &str) -> usize {
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
pub(crate) fn fav_glyph<'a>(
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
pub(crate) fn action_group(items: Vec<AnyElement>, border: Hsla) -> AnyElement {
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
pub(crate) fn matches_filter(firmware: &Firmware, needle: &str, glyph: &str) -> bool {
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
pub(crate) fn toggle_tag(filter: &str, tag: &str) -> String {
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
pub(crate) const SECTIONS: [(&str, Section); 4] = [
    ("INSTALLED", Section::Installed),
    ("DOWNLOADING", Section::Downloading),
    ("MISSING", Section::Missing),
    ("AVAILABLE ON THE HUB", Section::Hub),
];

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Section {
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
pub(crate) const LIB_ROW_H: Pixels = px(96.0);
/// Section header: small caps over a hairline, including the section's top gap.
pub(crate) const LIB_HEADER_H: Pixels = px(28.0);
/// The hub status line: spinner/dot + text + the Download All toggle and Reload.
pub(crate) const LIB_STATUS_H: Pixels = px(34.0);
/// The empty notice's single line.
pub(crate) const LIB_EMPTY_H: Pixels = px(24.0);

impl LibItem {
    pub(crate) fn height(&self) -> Pixels {
        match self {
            LibItem::Header(_) => LIB_HEADER_H,
            LibItem::Row(_) => LIB_ROW_H,
            LibItem::Empty => LIB_EMPTY_H,
            LibItem::HubStatus => LIB_STATUS_H,
        }
    }
}

pub(crate) fn section_of(firmware: &Firmware) -> Section {
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
/// filtered by the needle and the device selector, with favorites first inside
/// each section and NoThanks favorites last (stable, so the library's own order
/// survives within the groups).
pub(crate) fn flatten_library(
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
                    && devices.shows(firmware)
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
