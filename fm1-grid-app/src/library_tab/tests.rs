// No `use super::*` — the parent's gpui glob import would drag in GPUI's `test`
// attribute macro, which re-expands the built-in #[test] (see tests/mod.rs).
use super::{
    favorite_key, flatten_library, is_installed, matches_filter, snap_in, tag_index, tag_palette,
    toggle_tag, LibItem,
};
use super::{DeviceFilter, NO_THANKS_GLYPH};
use fm1_performer::library::{Firmware, FirmwareDevice, FirmwareSource, Status};
use std::collections::{HashMap, HashSet};

/// The pill palette walks the hue wheel from the theme base and clamps to
/// readable saturation/lightness; a neutral base still gets real hues.
#[test]
pub(super) fn tag_palette_rotates_hues_and_stays_readable() {
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

pub(super) fn no_glyphs() -> HashMap<String, String> {
    HashMap::new()
}

pub(super) fn firmware(name: &str, tags: &[&str]) -> Firmware {
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
pub(super) fn snap_in_curve() {
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
pub(super) fn installed_means_the_installed_section() {
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
pub(super) fn filter_matches_all_tokens_as_substrings() {
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
pub(super) fn filter_matches_the_synthetic_favorite_tag() {
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
pub(super) fn favorite_key_normalizes() {
    assert_eq!(favorite_key("Felucca"), favorite_key(" felucca "));
    assert_ne!(favorite_key("Felucca"), favorite_key("Beep"));
}

/// Favorites float to the top of their own section, in a stable order, and a
/// favorite hub offer does not jump above the installed rows.
#[test]
pub(super) fn favorites_float_to_the_top() {
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
    assert_eq!(items[0], LibItem::Header("Installed"));
    assert_eq!(
        items[1..4].to_vec(),
        vec![LibItem::Row(2), LibItem::Row(0), LibItem::Row(1)]
    );
    let hub = items
        .iter()
        .position(|item| *item == LibItem::Header("Downloadable From Firmware Hubs"))
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
pub(super) fn tag_toggles_in_filter() {
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
pub(super) fn no_thanks_favorite_sinks_to_the_bottom() {
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

#[test]
pub(super) fn device_type_colours_are_hash_stable() {
    let fm1 = crate::app::type_color("FM-1");
    assert_eq!(fm1, crate::app::type_color("FM-1"));
    assert_ne!(fm1, crate::app::type_color("SMK-37 Pro"));
    assert_eq!(
        crate::app::type_color("MKE-P37"),
        crate::app::type_color("MKE-P37")
    );
}

#[test]
pub(super) fn flash_targets_match_the_package_model() {
    use super::flash::unit_accepts;
    let mut smk = firmware("SLOOP Pro", &["SMK-37 Pro"]);
    smk.device = FirmwareDevice::Smk37;
    assert!(unit_accepts(&smk, Some("SMK-37 Pro_016")));
    assert!(unit_accepts(&smk, Some("STARRYKEY-37 PLAY_014")));
    assert!(!unit_accepts(&smk, Some("SMK-37 Elite_016")));
    assert!(!unit_accepts(&smk, Some("FM-1_916")));
    assert!(!unit_accepts(&smk, None));
    // Custom SMK firmware takes the family.
    assert!(unit_accepts(&smk, Some("SMK-37 Elite_900")));

    let fm1 = firmware("Felucca", &[]);
    assert!(unit_accepts(&fm1, Some("FM-1_916")));
    assert!(!unit_accepts(&fm1, Some("SMK-37 Pro_016")));
}

#[test]
pub(super) fn available_smk_rows_can_flash() {
    use fm1_performer::firmware_picker::pick_of;
    let mut firmware = firmware("Smk Thing", &[]);
    firmware.device = FirmwareDevice::Smk37;
    firmware.source = FirmwareSource::File {
        path: "/tmp/smk.fwsc".to_string(),
    };
    let pick = pick_of(&firmware);
    assert!(super::actions::flashable(&firmware, pick.as_ref()));
    let mut missing = firmware.clone();
    missing.status = Status::Missing;
    assert!(!super::actions::flashable(&missing, pick.as_ref()));
}

/// The device selector is a hard filter: FM-1 only by default; a subtype shows
/// only that model.
#[test]
pub(super) fn device_selector_filters_the_list() {
    let mut smk = firmware("Smk Thing", &[]);
    smk.device = FirmwareDevice::Smk37;
    smk.tags.push("SMK-37 Pro".to_string());
    let view = vec![firmware("Fm One", &[]), smk];
    let favorites = HashSet::new();
    let fm1_only = flatten_library(&view, "", &favorites, &no_glyphs(), DeviceFilter::default());
    assert_eq!(fm1_only[1..2].to_vec(), vec![LibItem::Row(0)]);
    let both = flatten_library(&view, "", &favorites, &no_glyphs(), DeviceFilter::All);
    assert_eq!(both[1..3].to_vec(), vec![LibItem::Row(0), LibItem::Row(1)]);
    let smk_only = flatten_library(&view, "", &favorites, &no_glyphs(), DeviceFilter::Smk37Pro);
    assert_eq!(smk_only[1..2].to_vec(), vec![LibItem::Row(1)]);
}
