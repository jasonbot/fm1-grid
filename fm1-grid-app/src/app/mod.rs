//! The window's core: pane state and lifecycle (firmware loading, power), keyboard and
//! encoder routing, the per-tick LCD upload, and the gpui view itself.
//!
//! The UI areas are separate modules: [`crate::grid`] (the 2x2 card grid),
//! [`crate::sidebar`] (device sidebar), [`crate::graph`] (the MIDI graph editor in the
//! sidebar), [`crate::panel`] (the miniature front panel) and [`crate::transport`] (the
//! bottom bar). The look lives in [`crate::theme`] plus the gpui-kit theme tokens, LCD
//! pixel work in [`crate::lcd`], and the FM-1 control maps in [`fm1_performer::keys`].
//!
//! egui used to poll the keyboard once per frame; gpui delivers key events instead, so
//! the held computer keys are tracked in [`GridApp::kb_down`] from `on_key_down` /
//! `on_key_up` and merged with the panel mouse and the physical MIDI keyboard in
//! [`GridApp::sync_held`]. The computer keyboard and panel mouse play only while the
//! Emulator tab shows the panel; the MIDI keyboard forwards on any tab. A timer task
//! ticks every 16 ms to upload LCD frames, decay LEDs, ease the grid background and
//! repaint.

use crate::audio::start_audio;
use crate::firmware_picker::FirmwareDelegate;
use crate::lcd;
use fm1_performer::MAX_SLOTS;
use fm1_performer::audio::AudioRing;
use fm1_performer::builtin::BUILTINS;
use fm1_performer::clock::Clock;
use fm1_performer::emulator::{EmulatorHandle, LCD_H, LCD_W};
use fm1_performer::hardware::Hardware;
use fm1_performer::keys::{BUTTONS, PIANO};
use fm1_performer::midi_keys::MidiKeys;
use fm1_performer::library::Firmware;
use fm1_performer::routing::{Port, Routing};
use fm1_soc::Action;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::{ElementExt, VirtualListScrollHandle, resizable_panel};
use gpui_kit::component::combobox::{ComboboxEvent, ComboboxState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::component::resizable::{ResizableState, h_resizable};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, Icon, Theme, ThemeMode, ThemeRegistry, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{TestSupportExt as _, *};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a graph row's snap-in or fade-out plays; the library rows use the same
/// 400 ms window.
pub(crate) const GRAPH_ANIM: Duration = Duration::from_millis(400);

#[derive(Clone)]
pub(crate) enum Source {
    Builtin(usize),
    File(PathBuf),
    /// A downloaded hub package, by blob-store key (the label is its display name).
    Blob {
        key: String,
        label: String,
    },
}

/// The Settings tab's appearance override: follow the OS, or pin light/dark.
/// Serialized lowercase in the session record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Appearance {
    #[default]
    Auto,
    Light,
    Dark,
}

/// The Library tab's device selector, shared with fm1-tty.
pub(crate) use fm1_performer::session::DeviceFilter;

#[derive(Default)]
pub(crate) struct Slot {
    pub handle: Option<EmulatorHandle>,
    pub source: Option<Source>,
    pub label: Option<String>,
    pub error: Option<String>,
    /// The firmware advertises USB-MIDI on real hardware (the emulator cannot bridge
    /// it; the MIDI graph shows it for context).
    pub usb_midi: bool,
    /// Pinned: the pane's device storage persists across power-offs and runs (the
    /// flash snapshot lives in the store).
    pub pinned: bool,
}

/// A slot's record in the store, `slot-N`: the firmware selection and the pin. A
/// pinned slot's device storage sits beside it as the `slot-N.flash` blob.
#[derive(serde::Serialize, serde::Deserialize)]
struct SlotRecord {
    firmware: String,
    source: RecordSource,
    /// Records written before every slot had one were pinned by definition.
    #[serde(default = "default_pinned")]
    pinned: bool,
}

fn default_pinned() -> bool {
    true
}

/// An emulator's audio health, from its five-second dropout window: the colour of
/// the grid card's status dot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    Green,
    Yellow,
    Orange,
    Red,
}

impl Health {
    /// The ramp, in drops per window. Audio callbacks run every few milliseconds, so
    /// [`RED_AT`] drops is already several percent of them running dry.
    pub(super) fn of(drops_5s: u64) -> Self {
        const ORANGE_AT: u64 = 6;
        const RED_AT: u64 = 26;
        match drops_5s {
            0 => Self::Green,
            1..ORANGE_AT => Self::Yellow,
            ORANGE_AT..RED_AT => Self::Orange,
            _ => Self::Red,
        }
    }
}

/// The dot's painted state: it appears on any non-green colour and lingers
/// [`DOT_LINGER`] after recovering to green, then hides until the colour changes.
#[derive(Clone, Copy)]
pub(crate) struct SlotHealth {
    pub color: Health,
    since: Instant,
    pub shown: bool,
}

/// How long a recovered (green) dot stays visible before hiding.
pub(crate) const DOT_LINGER: Duration = Duration::from_secs(1);

impl SlotHealth {
    /// Fold in the latest window count; true when the painted state changed.
    pub(crate) fn track(&mut self, drops_5s: u64, running: bool) -> bool {
        let color = if running {
            Health::of(drops_5s)
        } else {
            Health::Green
        };
        if color != self.color {
            *self = Self {
                color,
                since: Instant::now(),
                shown: running,
            };
            return true;
        }
        let shown = running && (color != Health::Green || self.since.elapsed() < DOT_LINGER);
        std::mem::replace(&mut self.shown, shown) != shown
    }
}

impl Default for SlotHealth {
    fn default() -> Self {
        Self {
            color: Health::Green,
            since: Instant::now(),
            shown: false,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum RecordSource {
    Builtin { index: usize },
    File { path: PathBuf },
    Blob { key: String, label: String },
}

/// The Library tab's add-file form: three text fields and the chosen file.
pub(crate) struct LibForm {
    pub name: Entity<InputState>,
    pub version: Entity<InputState>,
    pub description: Entity<InputState>,
    pub file: Option<PathBuf>,
    /// The list's filter box.
    pub filter: Entity<InputState>,
}

/// A pill for a recognized keyboard model. The colour is a hash of the model
/// name, so it stays the same across runs and themes.
pub(crate) fn type_color(kind: &str) -> Hsla {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in kind.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hsla((hash % 360) as f32 / 360.0, 0.62, 0.55, 1.0)
}

pub(crate) fn type_pill(kind: &str) -> AnyElement {
    let color = type_color(kind);
    div()
        .rounded_full()
        .px(px(6.0))
        .py(px(1.0))
        .text_size(px(9.0))
        .bg(color.opacity(0.16))
        .text_color(color)
        .child(kind.to_string())
        .into_any_element()
}

pub struct GridApp {
    pub(crate) slots: [Slot; MAX_SLOTS],
    /// One logical CPU per physical core, used to pin emulator threads on Linux.
    core_cpus: Vec<usize>,
    pub(crate) active: usize,
    pub(crate) clock: Arc<Clock>,
    /// The live FM-1 units on USB: discovery, firmware detection, flashing and clock
    /// routing, all on background threads.
    pub(crate) hardware: Hardware,
    /// The MIDI graph: routes system MIDI ports, running panes and live units to each
    /// other, on its own poller thread.
    pub(crate) routing: Routing,
    /// A physical MIDI keyboard driving the on-screen virtual keyboard, on its own
    /// poller thread; the transport bar's selector picks the port.
    pub(crate) midi_keys: MidiKeys,
    /// Per-device flash arming (the row's "Flash" toggle) and its combobox, and the
    /// was-flashing marker that re-safes the toggle when a flash succeeds.
    pub(crate) flash_armed: std::collections::HashMap<String, bool>,
    pub(crate) flash_combos:
        std::collections::HashMap<String, Entity<ComboboxState<FirmwareDelegate>>>,
    flash_was: std::collections::HashMap<String, bool>,
    /// MIDI rescue: the Devices pane's Rescue toggle offers every system MIDI port
    /// as a flash target, for units too bricked to answer to their name (those
    /// flash through `Hardware::rescue_flash`). `rescue_warn` rings the toggle when
    /// no FM-1 answered but some MIDI port exists. The sweep fills the ports and
    /// their flash comboboxes while the toggle is on.
    pub(crate) rescue_on: bool,
    pub(crate) rescue_warn: bool,
    pub(crate) rescue_ports: Vec<String>,
    pub(crate) rescue_combos:
        std::collections::HashMap<String, Entity<ComboboxState<FirmwareDelegate>>>,
    rings: [Arc<AudioRing>; MAX_SLOTS],
    /// Kept alive for as long as the app wants sound.
    _audio: Option<cpal::Stream>,
    pub(crate) audio_error: Option<String>,
    /// The per-slot live pictures, rebuilt on tick while a pane runs; `None` when off.
    /// The selected slot's frame is the sidebar's upscaled picture (hqx, or
    /// nearest-neighbour while its dropout health is orange or worse).
    pub(crate) frames: [Option<Arc<RenderImage>>; MAX_SLOTS],
    /// The framebuffer each slot's frame was built from; a pane whose screen has not
    /// changed costs a memcmp per tick instead of a full decode + upload.
    last_fb: [Option<Box<[u16]>>; MAX_SLOTS],
    /// The selected pane's screen with a slight fisheye bulge; this is what its grid
    /// card shows.
    pub(crate) ghost: Arc<RenderImage>,
    /// The framebuffer the ghost was built from; like `last_fb`, an unchanged screen
    /// costs a memcmp per tick instead of the fisheye pass + upload.
    ghost_fb: Option<Box<[u16]>>,
    /// Whether the ghost currently holds a live (powered) snapshot.
    ghost_live: bool,
    /// FM-1 keys/buttons currently held on the computer keyboard (event-driven; only
    /// set while the Emulator tab shows the panel).
    kb_down: [bool; 41],
    /// Keys/buttons currently sent as down to the active emulator (keyboard or panel mouse).
    pub(crate) held: [bool; 41],
    /// Front-panel keys/buttons currently held down with the mouse.
    pub(crate) panel_down: [bool; 41],
    /// Smoothed front-panel LED brightness for the active slot.
    pub(crate) leds: [f32; 41],
    /// The MASTER pot position for each slot (0..1023).
    master: [u16; MAX_SLOTS],
    /// Whether each slot's emulator is switched on. Off by default; a slot with no power
    /// has a loaded firmware but no running emulator thread.
    pub(crate) powered: [bool; MAX_SLOTS],
    /// The computed grid background: the dominant colour of the selected pane's screen,
    /// resampled once a second (and on pane switches) and eased towards over 500 ms.
    pub(crate) bg: [f32; 3],
    bg_from: [f32; 3],
    bg_target: [f32; 3],
    bg_at: Instant,
    bg_sampled_at: Instant,
    dominant_slot: usize,
    /// The sidebar screen's exact upscale factor (1, 2, 3, 4 or 6), decided by the
    /// sidebar panel's rendered size last frame and applied by `upload_lcds` with the
    /// hqx filter (nearest-neighbour while the pane's dropout health is orange or worse).
    pub(crate) sidebar_scale: usize,
    /// The sidebar panel's rendered size, recorded during prepaint (one frame of lag,
    /// the same trade the egui layout made with the graph height).
    pub(crate) sidebar_size: (f32, f32),
    /// The grid area's rendered size, recorded the same way; the cards snap between
    /// 1x and 0.5x screens to fit it.
    pub(crate) grid_size: (f32, f32),
    /// The grid cards' screen scale: exactly 1.0 or 0.5.
    pub(crate) grid_scale: f32,
    /// The split's bound state, so the quit handler can read the panel widths, and
    /// the width restored from the session record (the panel's initial size).
    pub(crate) emulator_split: Entity<ResizableState>,
    pub(crate) sidebar_width: f32,
    /// Per-slot audio health behind the grid cards' status dot.
    pub(crate) health: [SlotHealth; MAX_SLOTS],
    /// The main area's tab: 0 = Emulator (grid + sidebar split), 1 = Devices, 2 =
    /// Library, 3 = Settings, 4 = About.
    pub(crate) active_tab: usize,
    /// The Devices tab's section: 0 = Devices, 1 = MIDI Graph (saved in the session).
    pub(crate) devices_tab: usize,
    /// The About tab's section: 0 = About this app, 1 = Licenses.
    pub(crate) about_tab: usize,
    pub(crate) about_scroll: VirtualListScrollHandle,
    pub(crate) about_lines: Rc<Vec<&'static str>>,
    pub(crate) about_sizes: Rc<Vec<Size<Pixels>>>,
    /// The Devices and MIDI Graph sections' scroll state (scrollbars, PgUp/PgDn and
    /// Home/End) and their viewport heights for the page step.
    pub(crate) devices_scroll: ScrollHandle,
    pub(crate) graph_scroll: ScrollHandle,
    pub(crate) devices_view_h: f32,
    pub(crate) graph_view_h: f32,
    /// The front panel's rendered width, recorded during prepaint.
    pub(crate) panel_width: f32,
    /// Mouse-wheel detent accumulator for the panel knobs: (group, fractional detents).
    pub(crate) wheel_acc: (u32, f32),
    /// The beat light's state last tick, so its flips repaint without a constant poll.
    last_beat: bool,
    /// Ticks since start, driving the half-second external-state sweep.
    tick_count: u64,
    held_slot: Option<usize>,
    /// The keyboard focus for playing the active pane: the root view's handle.
    pub(crate) focus: FocusHandle,
    /// One searchable firmware combobox per slot (the sidebar shows the active one).
    pub(crate) firmware_combos: [Entity<ComboboxState<FirmwareDelegate>>; MAX_SLOTS],
    /// The tempo readout's text field.
    pub(crate) tempo_input: Entity<InputState>,
    /// The tempo slider next to the field; the two mirror each other.
    pub(crate) tempo_slider: Entity<SliderState>,
    /// On-disk state (the slots' firmware/pin records and pinned slots' flash
    /// snapshots). None if the platform has no data directory.
    pub(crate) store: Option<fm1_performer::store::Store>,
    /// The firmware library (the Library tab's data; pickers read it too).
    pub(crate) library: fm1_performer::library::Library,
    /// The Library tab's add-file form.
    pub(crate) lib_form: LibForm,
    /// The library list's scroll state (shared with its scrollbar, PgUp/PgDn, and
    /// scroll-to-item for freshly downloaded firmware).
    pub(crate) lib_scroll: VirtualListScrollHandle,
    /// The library list's viewport height, for page scrolling.
    pub(crate) lib_view_h: f32,
    /// A firmware whose download was just kicked off: when it lands in the Installed
    /// section, the list scrolls to it (its section changes on completion). Only
    /// the row's own Download button sets this — Download All's automatic fetches
    /// don't move the list under the user.
    pub(crate) lib_scroll_to: Option<String>,
    /// Installed firmware names seen so far; a name the library gains lands in
    /// `lib_added` instead and snaps in.
    lib_known: std::collections::HashSet<String>,
    /// Newly installed rows and when they appeared; the row plays a 200 ms snap-in
    /// spring and the entries drop once settled.
    pub(crate) lib_added: std::collections::HashMap<String, Instant>,
    /// The Library tab's cached view and flattened row stream. The virtual list's
    /// render closure runs on every scroll frame, so the stream is rebuilt only
    /// when [`LibStreamKey`] moves, never per frame.
    pub(crate) lib_view: Rc<Vec<Firmware>>,
    pub(crate) lib_items: Rc<Vec<crate::library_tab::LibItem>>,
    pub(crate) lib_sizes: Rc<Vec<Size<Pixels>>>,
    pub(crate) lib_stream_key: Option<(u64, u64, String, u64, DeviceFilter)>,
    /// Bumped when a favorite flips, so the cached stream re-sorts favorites first.
    pub(crate) favorites_rev: u64,
    /// MIDI graph rows the sections animate: links that just appeared (snap in) or
    /// vanished (fade out). The tick diffs the routing graph, so a pane powering off
    /// and pruning its links animates like an (X) click.
    pub(crate) graph_added: std::collections::HashMap<(Port, Port), Instant>,
    pub(crate) graph_removed: std::collections::HashMap<(Port, Port), Instant>,
    graph_known: std::collections::HashSet<(Port, Port)>,
    /// Favorited firmwares, as normalized name keys (see `favorite_key`; persisted
    /// in the session). A favorite's library row carries its glyph tag, every
    /// other row the synthetic ☆.
    pub(crate) favorites: std::collections::HashSet<String>,
    /// The glyph a favorite shows (keyed like `favorites`); a missing entry is ★.
    pub(crate) favorite_glyphs: std::collections::HashMap<String, String>,
    /// Which device families the Library tab lists (persisted in the session).
    pub(crate) device_filter: DeviceFilter,
    /// The Library tab's Download All toggle (persisted in the session): while on,
    /// event-driven sweeps keep one hub download going until nothing downloadable
    /// or upgradable remains.
    pub(crate) download_all: bool,
    /// Which shared fields this app changed and must write (see `Shared::save`).
    pub(crate) shared_dirty: fm1_performer::session::Dirty,
    /// The Settings tab's state (persisted in the session): the theme family, the
    /// appearance override and the accent colour Influenza is computed from.
    pub(crate) theme_id: String,
    pub(crate) appearance: Appearance,
    /// The accent colour id (`themes::ACCENTS`); ignored by JSON-backed families.
    pub(crate) accent: String,
    /// The tab rail's rendered height, recorded during prepaint — the sliding chip
    /// aims the gear's bottom slot with it (one frame of lag, like the sidebar's).
    pub(crate) rail_h: f32,
    /// Raw download-mode units and their flash analysis.
    pub(crate) recovery: crate::recovery::RecoveryState,
    recovery_generation: u64,
    /// The library generation the firmware pickers were last built from.
    combos_at_generation: u64,
    _tick: Task<()>,
    /// Event subscriptions: the firmware comboboxes, the tempo input, the flash picker.
    pub(crate) _subscriptions: Vec<Subscription>,
}


mod boot;
mod input;
mod panes;
mod render;
mod theme_actions;
mod tick;
pub(crate) fn hub_online() -> bool {
    std::env::var_os("FM1_GRID_NO_HUB").is_none()
}

/// Paged-scroll math shared by the library list and the Devices tab's sections:
/// scroll offsets run negative going down; clamp to the content.
fn page_offset(current: f32, max: f32, view_h: f32, pages: f32) -> f32 {
    (current - pages * view_h * 0.9).clamp(-max.abs(), 0.0)
}

/// The window height the full stack needs (title bar, tabs, full panel, 1x grid);
/// below it the front panel's keyboard sheds its compact allowance.
const PANEL_COMPACT_BELOW: f32 = 950.0;
