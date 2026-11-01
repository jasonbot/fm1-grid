//! Window bootstrap; the app itself lives in the `fm1_grid` library.

use fm1_grid::app::GridApp;
use gpui_kit::assets::{icon_assets, Assets as ComponentAssets};
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use std::borrow::Cow;

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

fn main() {
    // Room for two card rows, the device sidebar and the transport bar. The target
    // display is 1920x1200 at 150% — 1280x800 logical — so the default and minimum
    // sizes must live inside that: the panel's compact keyboard and the grid's 0.5x
    // cards take over below the full-height stack.
    application().with_assets(AppAssets).run(|cx| {
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
            move |window, cx| cx.new(|cx| GridApp::new(window, cx)),
        )
        .expect("failed to open window");

        // Quit when the last window closes; gpui-kit leaves close policy to the app.
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        // Going away: every slot's firmware selection and pin land in the store (a
        // pinned slot still running snapshots its device storage too), and the
        // session record keeps the layout, selection and tempo.
        cx.on_app_quit(move |cx| {
            app.update(cx, |app, cx| {
                app.save_slots();
                app.save_session(cx);
            });
            async {}
        })
        .detach();
    });
}

