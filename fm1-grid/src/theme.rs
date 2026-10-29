//! The look. App chrome follows the gpui-kit theme (`cx.theme()` tokens at the call
//! site), and the front panel's colours are derived from it in `panel.rs`; this
//! module keeps only the colour helpers they share, the bundled theme catalog and
//! the layout constants. The one computed colour in the app is the grid background
//! (see `grid.rs`).

/// Linear per-channel mix of two theme colours, `t` of the way from `a` to `b`.
pub(crate) fn mix_hsla(a: gpui_kit::Hsla, b: gpui_kit::Hsla, t: f32) -> gpui_kit::Hsla {
    let (a, b) = (gpui_kit::Rgba::from(a), gpui_kit::Rgba::from(b));
    gpui_kit::Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
    .into()
}

/// Halfway between two theme colours, per channel in sRGB: the health ramp's orange
/// sits between the theme's `warning` and `danger`.
pub(crate) fn halfway(a: gpui_kit::Hsla, b: gpui_kit::Hsla) -> gpui_kit::Hsla {
    mix_hsla(a, b, 0.5)
}

/// Layout constants shared across the window.
///
/// The sidebar split's initial/min/max widths: the screen at 2x, 1x and 4x plus its
/// bezel and padding.
pub(crate) const SIDEBAR_INITIAL: f32 = 512.0;
pub(crate) const SIDEBAR_MIN: f32 = 272.0;
pub(crate) const SIDEBAR_MAX: f32 = 992.0;
/// The firmware comboboxes' dropdown width (sidebar and devices pane).
pub(crate) const COMBO_MENU_W: f32 = 260.0;

// The bundled gpui-kit theme sets — every `themes/*.json`, listed by build.rs at
// build time — one file per picker family: (id = file stem, label = the set's
// `name`, embedded JSON). Drop a JSON file in themes/ and it ships.
include!(concat!(env!("OUT_DIR"), "/theme_files.rs"));

/// The picker's first entry: gpui-kit's own Default Light/Dark pair.
pub(crate) const DEFAULT_FAMILY: &str = "default";

/// One picker family: the registered config names that fill the light and dark
/// slots (the family's first of each mode; a missing side keeps the default theme).
pub(crate) struct ThemeFamily {
    pub id: &'static str,
    pub label: &'static str,
    pub light: Option<String>,
    pub dark: Option<String>,
}

#[derive(serde::Deserialize)]
struct ThemeSetHead {
    themes: Vec<ThemeHead>,
}

#[derive(serde::Deserialize)]
struct ThemeHead {
    name: String,
    mode: String,
}

/// Every picker family: Default first, then the bundled sets in table order. The
/// heads parse once; a file that fails to parse just doesn't get an entry (the same
/// failure is logged at load).
static FAMILIES: std::sync::LazyLock<Vec<ThemeFamily>> = std::sync::LazyLock::new(|| {
    let mut families = vec![ThemeFamily {
        id: DEFAULT_FAMILY,
        label: "Default",
        light: None,
        dark: None,
    }];
    for &(id, label, json) in THEME_FILES {
        let Ok(set) = serde_json::from_str::<ThemeSetHead>(json) else {
            continue;
        };
        let take = |mode: &str| {
            set.themes
                .iter()
                .find(|head| head.mode == mode)
                .map(|head| head.name.clone())
        };
        families.push(ThemeFamily {
            id,
            label,
            light: take("light"),
            dark: take("dark"),
        });
    }
    families
});

pub(crate) fn theme_families() -> &'static [ThemeFamily] {
    &FAMILIES
}

/// A family by picker id; an unknown or stale id (an edited session record) falls
/// back to Default.
pub(crate) fn family(id: &str) -> &'static ThemeFamily {
    theme_families()
        .iter()
        .find(|family| family.id == id)
        .unwrap_or(&theme_families()[0])
}

/// Register every bundled theme set with gpui-kit's registry. Idempotent: the
/// registry keeps the first config under each name.
pub(crate) fn load_bundled_themes(cx: &mut gpui_kit::App) {
    let registry = gpui_kit::component::ThemeRegistry::global_mut(cx);
    for &(id, _, json) in THEME_FILES {
        if let Err(e) = registry.load_themes_from_str(json) {
            eprintln!("fm1-grid: bundled theme {id} failed to load: {e}");
        }
    }
}
