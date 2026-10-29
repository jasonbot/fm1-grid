//! CLI and window bootstrap; the app itself lives in the `fm1_grid` library.

use fm1_grid::app::GridApp;
use fm1_performer::MAX_SLOTS;
use gpui_kit::assets::{icon_assets, Assets as ComponentAssets};
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use std::borrow::Cow;
use std::path::PathBuf;

// Lucide icons beyond gpui-kit's default set: the sidebar's pin and power toggles,
// the tab and pill-tab icons, the rescue toggle's life buoy, the settings tab's
// palette and auto-appearance icons, and the button icons (the default set already
// covers RefreshCw, Plus, ChevronDown, ArrowRight, Settings, Sun and Moon).
// The macro embeds exactly these SVGs; a misspelled name fails the build.
icon_assets!(
    ExtraIcons,
    [Pin, Monitor, KeyboardMusic, Library, Download, Trash, Zap, FilePlus, Save, X, Usb, Waypoints, Power, LifeBuoy, Palette, SunMoon]
);

actions!(fm1_grid, [Quit]);

/// The app's asset source: the selected extra icons first (native `ComponentAssets`
/// errors on an unknown path, so it answers last), then the default component icons.
struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        ComponentAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ComponentAssets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

const USAGE: &str = "\
fm1-grid — a 2x2 grid of FM-1 emulators with a shared MIDI-clock metronome

usage: fm1-grid [FIRMWARE ...]

Each FIRMWARE argument (up to four) is loaded into a pane, left to right, top to bottom. A
firmware is either a .fwsc update package or a raw .bin/.xip image (entry 0x02000120). With
no firmware arguments the embedded firmwares load into the first panes. Panes are powered
off at startup; the power switch in the device sidebar starts and stops each pane's
emulator.

The device sidebar's dropdown lists every firmware the library has at hand; import
your own files on the Library tab. Click a pane (or its display) to route the
computer keyboard there. The
clickable front panel sits at the bottom of the Emulator tab; the keys below play
while it shows:
A W S E D F T G Y H U J K O L P ; ' play C4..F5; C V B N M Q I play F3..B3;
Z/X octave, Space PLAY, R REC, 1..0 functions; ←/→ PRESETS, ↑/↓ ALGORITHM, ,/. SELECT.
";

fn usage_error(message: &str) -> ! {
    eprintln!("fm1-grid: {message}");
    eprint!("{USAGE}");
    std::process::exit(2);
}

fn main() {
    let mut initial = Vec::new();
    for arg in std::env::args_os().skip(1) {
        if arg == "-h" || arg == "--help" {
            print!("{USAGE}");
            return;
        }
        let path = PathBuf::from(arg);
        if path.to_string_lossy().starts_with('-') {
            usage_error(&format!("unknown option {}", path.display()));
        }
        initial.push(path);
    }

    if initial.len() > MAX_SLOTS {
        usage_error(&format!(
            "{} firmware paths were given, but the grid only has {MAX_SLOTS} panes",
            initial.len()
        ));
    }

    // Room for two card rows, the device sidebar and the transport bar. The target
    // display is 1920x1200 at 150% — 1280x800 logical — so the default and minimum
    // sizes must live inside that: the panel's compact keyboard and the grid's 0.5x
    // cards take over below the full-height stack.
    application().with_assets(AppAssets).run(move |cx| {
        init(cx);
        // The app menu's Quit (Cmd+Q) lands on the same quit path as closing the last
        // window (below); without a menu the shortcut reaches no handler.
        cx.set_menus([Menu::new("FM-1 Grid").items([MenuItem::action("Quit FM-1 Grid", Quit)])]);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        let bounds = Bounds::centered(None, size(px(1152.0), px(740.0)), cx);
        let (_, app) = open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // Grid at 0.5x plus the sidebar's floor across; title bar, tabs,
                // compact transport and the 0.5x card rows down.
                window_min_size: Some(size(px(620.0), px(680.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("FM-1 Grid".into()),
                    ..TitleBar::title_bar_options()
                }),
                // TitleBar::window_options: the in-app title bar owns dragging and
                // draws the window controls where the platform lacks them (GNOME).
                ..TitleBar::window_options()
            },
            cx,
            move |window, cx| cx.new(|cx| GridApp::new(window, cx, &initial)),
        )
        .expect("failed to open window");

        // Quit when the last window closes; gpui-kit leaves close policy to the app.
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        // Going away: pinned slots snapshot their device storage to the store, and the
        // session record keeps the layout, selection and tempo.
        cx.on_app_quit(move |cx| {
            app.update(cx, |app, cx| {
                app.save_pinned();
                app.save_session(cx);
            });
            async {}
        })
        .detach();
    });
}

