//! Headless UI smoke tests (gpui-kit test-support): the window renders, card clicks
//! select panes, the power switch boots an emulator and its LCD frames reach the
//! `RenderImage` views, and the metronome switch toggles the clock.

use crate::app::GridApp;
// Explicit imports, no glob: with test-support the glob pulls in GPUI's `test`
// attribute macro, which then re-expands the `#[test]` this macro emits (the
// gpui-kit docs warn about exactly this).
use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    App, AppContext, Bounds, Entity, Focusable, InputEvent, KeyDownEvent, KeyUpEvent, Keystroke,
    Point, ScrollDelta, TestAppContext, Window, WindowBounds, WindowHandle, WindowOptions,
    open_window, point, px, size,
};
use std::time::Duration;

/// Open the app in a test window at the real default size.
fn open_app(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<GridApp>) {
    open_app_at(cx, 1180.0, 1160.0)
}

/// Open the app in a test window at a given size.
fn open_app_at(cx: &mut TestAppContext, w: f32, h: f32) -> (WindowHandle<Root>, Entity<GridApp>) {
    // Tests share one throwaway store for the process (the pin test writes to it),
    // and stay offline: a real hub answer would move the list mid-click.
    if std::env::var_os("FM1_GRID_DATA_DIR").is_none() {
        unsafe {
            std::env::set_var(
                "FM1_GRID_DATA_DIR",
                std::env::temp_dir().join(format!("fm1-grid-uitest-{}", std::process::id())),
            );
        }
    }
    unsafe {
        std::env::set_var("FM1_GRID_NO_HUB", "1");
    }
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(w), px(h)),
        };
        let (window, content) = open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| GridApp::new(window, cx, &[])),
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Base Root"), content)
    })
}

/// The sidebar takes its proportional share of extra window width (gpui-base rescales
/// panels only when every panel has a size), so a big window lets its screen reach
/// the exact 4x fit.
#[gpui_kit::test]
fn sidebar_grows_with_window(cx: &mut TestAppContext) {
    let (window, app) = open_app_at(cx, 2560.0, 1600.0);
    for _ in 0..3 {
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
    }
    let (sidebar_w, scale) = cx.update(|cx| {
        let app = app.read(cx);
        (app.sidebar_size.0, app.sidebar_scale)
    });
    assert!(sidebar_w > crate::theme::SIDEBAR_INITIAL, "sidebar width {sidebar_w}");
    assert_eq!(scale, 4);
}

#[gpui_kit::test]
fn boots_selects_powers(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        // The sidebar's power switch and the metronome are on screen.
        assert!(window.find("pane-power").visible());
        assert!(window.find("midi-clock").visible());
        // Clicking pane 3's card routes the keyboard there.
        window.click(("slot-card", 2usize), cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(app.read(cx).active, 2));

    // Power the selected pane on from the sidebar switch.
    cx.update_window(window.into(), |_, window, cx| {
        window.click("pane-power", cx)
    })
    .unwrap();
    cx.update(|cx| assert!(app.read(cx).powered[2]));

    // The emulator thread boots in real time; the 16 ms frame pump runs on the test
    // executor. Poll both until the pane's LCD frames reach its views.
    let mut framed = false;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(100));
        cx.background_executor.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        framed = cx.update(|cx| app.read(cx).frames[2].is_some());
        if framed {
            break;
        }
    }
    cx.update(|cx| {
        let app = app.read(cx);
        assert!(app.slots[2].handle.is_some());
        assert!(
            app.slots[2].error.is_none(),
            "pane 3 failed to start: {:?}",
            app.slots[2].error
        );
    });
    assert!(framed, "pane 3 produced no LCD frames");

    // Power pane 2 (Felucca — a firmware that advertises USB-MIDI): the graph notes
    // that its USB-MIDI in/out can't be bridged by the emulator. The graph lives on
    // the Devices tab now (the rail buttons are ("tab", 0/1/2)).
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("slot-card", 1usize), cx);
        window.click("pane-power", cx);
    })
    .unwrap();
    cx.background_executor.advance_clock(Duration::from_millis(100));
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 1usize), cx); // the Devices tab
        window.click(1usize, cx); // the MIDI Graph pill
        window.render_frame(cx);
        assert!(
            window.find("graph-usb-note").visible(),
            "the graph should note the pane's unbridgeable USB-MIDI"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn metronome_toggles(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
    // The tempo readout starts showing the clock's BPM.
    cx.update(|cx| {
        assert_eq!(app.read(cx).tempo_input.read(cx).value().as_ref(), "120");
    });
    let before = cx.update(|cx| app.read(cx).clock.enabled());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("midi-clock", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(app.read(cx).clock.enabled(), !before));
}

/// The tempo field is free-form while focused (nothing applies, whatever is typed)
/// and sanitises onto the metronome on Enter and on blur.
#[gpui_kit::test]
fn tempo_sanitizes_on_blur(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);

    // Type an out-of-range tempo: the text takes it, the clock does not.
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let tempo = app.read(cx).tempo_input.clone();
        tempo.read(cx).focus_handle(cx).focus(window, cx);
        window.press("ctrl-a", cx);
        window.input("999", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let app = app.read(cx);
        assert_eq!(app.tempo_input.read(cx).value().as_ref(), "999");
        assert_eq!(app.clock.bpm(), 120, "a focused edit must not apply");
    });

    // Enter clamps and applies.
    cx.update_window(window.into(), |_, window, cx| window.press("enter", cx))
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let app = app.read(cx);
        assert_eq!(app.clock.bpm(), 300);
        assert_eq!(app.tempo_input.read(cx).value().as_ref(), "300");
    });

    // Garbage snaps back to the live tempo on blur.
    cx.update_window(window.into(), |_, window, cx| {
        let tempo = app.read(cx).tempo_input.clone();
        tempo.read(cx).focus_handle(cx).focus(window, cx);
        window.press("ctrl-a", cx);
        window.input("abc", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(app.read(cx).clock.bpm(), 300));
    cx.update_window(window.into(), |_, window, cx| {
        let root = app.read(cx).focus.clone();
        root.focus(window, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let app = app.read(cx);
        assert_eq!(app.clock.bpm(), 300);
        assert_eq!(app.tempo_input.read(cx).value().as_ref(), "300");
    });
}

/// Dispatch a bare key down/up to the focused view (the rail buttons refocus the
/// root view, so this always reaches the app's own key routing).
fn send_key(window: &mut Window, cx: &mut App, key: &str, down: bool) {
    let keystroke = Keystroke::parse(key).expect("test keystroke");
    if down {
        window.dispatch_event(
            KeyDownEvent {
                keystroke,
                is_held: false,
                prefer_character_input: false,
            }
            .to_platform_input(),
            cx,
        );
    } else {
        window.dispatch_event(KeyUpEvent { keystroke }.to_platform_input(), cx);
    }
}

/// The front panel shows only on the Emulator tab, and the computer keys play the
/// pane only while it shows; switching away lifts the held keys (no stuck notes).
/// The metronome row stays global.
#[gpui_kit::test]
fn panel_and_keys_follow_emulator_tab(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);

    // Power pane 1 (the default active pane) so held keys have somewhere to go.
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("fm1-panel").visible(), "panel on the Emulator tab");
        assert!(window.find("midi-clock").visible(), "metronome is global");
        window.click(("slot-card", 0usize), cx);
        window.click("pane-power", cx);
    })
    .unwrap();
    let mut running = false;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(100));
        cx.background_executor.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        running = cx.update(|cx| app.read(cx).slots[0].handle.is_some());
        if running {
            break;
        }
    }
    assert!(running, "pane 1 failed to start");

    // A piano key down on the Emulator tab reaches the pane's key state.
    cx.update_window(window.into(), |_, window, cx| {
        send_key(window, cx, "a", true);
    })
    .unwrap();
    cx.update(|cx| assert!(app.read(cx).held.iter().any(|&down| down)));

    // Switching to the Devices tab hides the panel, keeps the metronome, and lifts
    // the key that was held when the tab changed.
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 1usize), cx);
        window.render_frame(cx);
        assert!(window.try_find("fm1-panel").is_none(), "panel stays on the Emulator tab");
        assert!(window.find("midi-clock").visible(), "metronome is global");
    })
    .unwrap();
    cx.update(|cx| {
        assert!(
            app.read(cx).held.iter().all(|&down| !down),
            "the tab switch must lift held keys"
        );
    });

    // Keys are inert there and on the Library tab.
    for tab in [1usize, 2] {
        cx.update_window(window.into(), |_, window, cx| {
            window.click(("tab", tab), cx);
            send_key(window, cx, "a", true);
        })
        .unwrap();
        cx.update(|cx| {
            assert!(
                app.read(cx).held.iter().all(|&down| !down),
                "keys must not play on tab {tab}"
            );
        });
        cx.update_window(window.into(), |_, window, cx| send_key(window, cx, "a", false))
            .unwrap();
    }

    // Back on the Emulator tab the panel returns and keys play again.
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 0usize), cx);
        window.render_frame(cx);
        assert!(window.find("fm1-panel").visible(), "panel returns with the tab");
        send_key(window, cx, "a", true);
    })
    .unwrap();
    cx.update(|cx| assert!(app.read(cx).held.iter().any(|&down| down)));
    cx.update_window(window.into(), |_, window, cx| send_key(window, cx, "a", false))
        .unwrap();
    cx.update(|cx| assert!(app.read(cx).held.iter().all(|&down| !down)));
}

/// The health dot appears on the first drop, lingers a second after recovery, then
/// hides until the colour changes again.
#[test]
fn health_dot_lingers_then_hides() {
    use crate::app::{DOT_LINGER, Health, SlotHealth};

    let mut health = SlotHealth::default();
    assert!(!health.shown, "green at boot stays hidden");

    assert!(health.track(2, true));
    assert!(health.shown && health.color == Health::Yellow);

    assert!(health.track(0, true), "recovering to green repaints");
    assert!(health.shown, "the dot lingers after recovery");
    assert!(!health.track(0, true), "inside the linger nothing changes");

    std::thread::sleep(DOT_LINGER + Duration::from_millis(50));
    assert!(health.track(0, true), "the linger expiring hides the dot");
    assert!(!health.shown);

    assert!(health.track(30, true), "a colour change shows the dot again");
    assert!(health.shown && health.color == Health::Red);
    assert!(health.track(0, false), "powering off hides the dot at once");
    assert!(!health.shown);
}

/// At the target device's logical size (1920x1200 at 150% = 1280x800) the grid cards
/// snap to 0.5x and the window still renders.
#[gpui_kit::test]
fn grid_snaps_half_size(cx: &mut TestAppContext) {
    let (window, app) = open_app_at(cx, 1280.0, 800.0);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        // Everything still renders: cards, the power switch, the metronome.
        for slot in 0..4usize {
            assert!(window.find(("slot-card", slot)).visible());
        }
        assert!(window.find("midi-clock").visible());
    })
    .unwrap();
    // The 16 ms pump snaps the scale from the recorded grid size.
    for _ in 0..20 {
        cx.background_executor.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        if cx.update(|cx| app.read(cx).grid_scale) == 0.5 {
            return;
        }
    }
    cx.update(|cx| assert_eq!(app.read(cx).grid_scale, 0.5, "cards never snapped"));
}

/// Pinning: inert while the pane is off; on it snapshots the device's flash at
/// power-off and the next boot restores it.
#[gpui_kit::test]
fn pin_cycle(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);

    // Off: the toggle is inert.
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("pin", 0usize), cx);
    })
    .unwrap();
    cx.update(|cx| assert!(!app.read(cx).slots[0].pinned, "off pane must not pin"));

    // On: pin, then power off and on again.
    cx.update_window(window.into(), |_, window, cx| {
        window.click("pane-power", cx);
    })
    .unwrap();
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(100));
        cx.background_executor.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        if cx.update(|cx| app.read(cx).slots[0].handle.is_some()) {
            break;
        }
    }
    // Pinning swaps the combobox for the firmware's name — the row must not jump.
    let row_h = cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.find("pane-attrs").bounds().size.height
    })
    .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("pin", 0usize), cx);
    })
    .unwrap();
    cx.update(|cx| assert!(app.read(cx).slots[0].pinned, "running pane pins"));
    let pinned_h = cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.find("pane-attrs").bounds().size.height
    })
    .unwrap();
    assert!(
        (row_h - pinned_h).abs() < px(1.0),
        "the attributes row jumps on pin: {row_h} -> {pinned_h}"
    );

    cx.update_window(window.into(), |_, window, cx| {
        window.click("pane-power", cx); // off: snapshot to the store
    })
    .unwrap();
    cx.update(|cx| {
        let app = app.read(cx);
        let store = app.store.as_ref().expect("a store");
        assert!(
            store
                .list_blobs(None)
                .unwrap()
                .iter()
                .any(|p| p == "slot-0.flash"),
            "no flash snapshot in the store"
        );
        assert!(
            store.list_objects(None).iter().any(|p| p == "slot-0"),
            "no slot record in the store"
        );
    });

    // Power back on: the flash blob restores into the boot.
    cx.update_window(window.into(), |_, window, cx| {
        window.click("pane-power", cx);
    })
    .unwrap();
    let mut framed = false;
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        cx.background_executor.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        framed = cx.update(|cx| app.read(cx).frames[0].is_some());
        if framed {
            break;
        }
    }
    assert!(framed, "the restored pane drew nothing");
    cx.update(|cx| {
        let app = app.read(cx);
        assert!(app.slots[0].pinned);
        assert!(app.slots[0].error.is_none(), "{:?}", app.slots[0].error);
    });
}

/// The Library tab lists the builtins with flash targets (panes are off and unpinned
/// at startup), and its add form gates Save on a chosen file. A short window, so the
/// offline-length list (builtins only) overflows and the scroll asserts mean
/// something.
#[gpui_kit::test]
fn library_tab_lists(cx: &mut TestAppContext) {
    let (window, app) = open_app_at(cx, 1180.0, 700.0);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("tab", 2usize), cx); // the Library tab
    })
    .unwrap();
    cx.update(|cx| assert_eq!(app.read(cx).active_tab, 2));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let flash = window.find("lib-flash-SLOOP");
        assert!(flash.visible(), "SLOOP should list with a Flash To… button");
        assert!(window.find("lib-add-file").visible());
        // The hub status item sits at the bottom of the list, spinner while fetching;
        // scroll the list to the end to bring it into view first.
        window.scroll(
            "library-list",
            ScrollDelta::Pixels(point(px(0.0), px(-100000.0))),
            cx,
        );
        window.render_frame(cx);
        assert!(window.find("hub-status").visible());
        assert!(window.find("hub-reload").visible());
        // Download All is a toggle; it flips on click and back.
        assert!(window.find("hub-download-all").visible());
        window.click("hub-download-all", cx);
        assert!(app.read(cx).download_all, "the toggle should turn on");
        window.click("hub-download-all", cx);
        assert!(!app.read(cx).download_all, "the toggle should turn back off");
        // Nothing to remove on a builtin.
        assert!(window.try_find("lib-remove-SLOOP").is_none());

        // PgUp/PgDn page the list (we scrolled to the bottom above: up first).
        let bottom = app.read(cx).lib_scroll.offset().y;
        window.press("pageup", cx);
        let up = app.read(cx).lib_scroll.offset().y;
        assert!(up > bottom, "pageup should scroll up: {bottom} -> {up}");
        window.press("pagedown", cx);
        let down = app.read(cx).lib_scroll.offset().y;
        assert!(down < up, "pagedown should scroll back down: {up} -> {down}");

        // The filter box narrows the list (back to the top first — the list was
        // scrolled to the bottom above).
        window.scroll(
            "library-list",
            ScrollDelta::Pixels(point(px(0.0), px(100000.0))),
            cx,
        );
        window.render_frame(cx);
        window.click("library-filter", cx);
        window.input("sloop", cx);
        window.render_frame(cx);
        assert!(window.find("lib-flash-SLOOP").visible());
        assert!(
            window.try_find("lib-flash-Felucca").is_none(),
            "the filter should hide non-matching firmwares"
        );
    })
    .unwrap();
}

/// A freshly downloaded firmware jumps sections (downloading/hub → installed); the
/// list scrolls to it when it lands. The download itself needs the network, so the
/// test drives the mechanism directly: installed file entries plus the pending
/// scroll target the Download button sets.
#[gpui_kit::test]
fn library_scrolls_to_downloaded(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
    // Enough installed firmwares that the target starts below the viewport. File
    // entries stat as Available only with a real file at hand.
    let dir = std::env::temp_dir().join(format!("fm1-grid-scrolltest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    cx.update(|cx| {
        app.update(cx, |app, _| {
            for i in 0..30 {
                let path = dir.join(format!("faked-{i:02}.fwsc"));
                std::fs::write(&path, b"fake").unwrap();
                app.library.import_file(
                    path.to_str().unwrap(),
                    &format!("faked-{i:02}"),
                    "1.0",
                    "",
                );
            }
        });
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 2usize), cx);
        window.render_frame(cx);
        assert!(
            window.try_find("lib-flash-faked-25").is_none(),
            "the target should start below the viewport"
        );
    })
    .unwrap();
    // What the Download button records when the download starts.
    cx.update(|cx| {
        app.update(cx, |app, _| app.lib_scroll_to = Some("faked-25".to_string()));
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.find("lib-flash-faked-25").visible(),
            "the list should scroll the just-downloaded firmware into view"
        );
    })
    .unwrap();
    cx.update(|cx| {
        assert!(
            app.read(cx).lib_scroll_to.is_none(),
            "the pending scroll target should clear once hit"
        );
    });
    // Leave the shared per-process store as found.
    cx.update(|cx| {
        app.update(cx, |app, _| {
            for i in 0..30 {
                app.library.remove(&format!("faked-{i:02}"));
            }
        });
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// Tag pills toggle the filter: click adds the tag's words as tokens (substring
/// tokens match rows), clicking again removes them. Presence is token-exact, so a
/// typed substring doesn't block the add.
#[gpui_kit::test]
fn library_tag_pills_toggle_filter(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
    let filter_text = |app: &GridApp, cx: &App| {
        app.lib_form.filter.read(cx).value().to_string()
    };
    let dev_pill = ("lib-tag", crate::library_tab::pill_id("Felucca", "dev"));
    let groove_pill = ("lib-tag", crate::library_tab::pill_id("SLOOP", "groove"));

    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 2usize), cx); // the Library tab
        window.render_frame(cx);
        // Token search: "fel gro" matches Felucca (name + tags), not SLOOP.
        window.click("library-filter", cx);
        window.input("fel gro", cx);
        window.render_frame(cx);
        assert!(window.find("lib-flash-Felucca").visible());
        assert!(window.try_find("lib-flash-SLOOP").is_none());
        // Clear the box for the pill steps.
        let filter = app.read(cx).lib_form.filter.clone();
        filter.update(cx, |state, cx| state.set_value("", window, cx));
        window.render_frame(cx);
        assert!(window.find("lib-flash-SLOOP").visible());
    })
    .unwrap();

    // Click Felucca's "dev" pill: the tag joins the filter, SLOOP (no dev tag) hides.
    cx.update_window(window.into(), |_, window, cx| {
        window.click(dev_pill, cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(filter_text(app.read(cx), cx), "dev"));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("lib-flash-Felucca").visible());
        assert!(window.try_find("lib-flash-SLOOP").is_none());
        // Click again: the tag leaves the filter, SLOOP returns.
        window.click(dev_pill, cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(filter_text(app.read(cx), cx), ""));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("lib-flash-SLOOP").visible());
    })
    .unwrap();

    // A typed substring is not the tag's token: clicking adds the full word anyway.
    cx.update_window(window.into(), |_, window, cx| {
        let filter = app.read(cx).lib_form.filter.clone();
        filter.update(cx, |state, cx| state.set_value("groo", window, cx));
        window.click(groove_pill, cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(filter_text(app.read(cx), cx), "groo groove"));
    cx.update_window(window.into(), |_, window, cx| {
        window.click(groove_pill, cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(filter_text(app.read(cx), cx), "groo"));
}

/// The Settings tab: the appearance override pins the mode at once, a bundled theme
/// family applies by name, and each pick lands in the session record (restored to
/// the defaults at the end — the store is shared across the process's tests).
#[gpui_kit::test]
fn settings_theme_and_appearance(cx: &mut TestAppContext) {
    use crate::app::Appearance;
    use gpui_kit::component::Theme;

    let (window, app) = open_app(cx);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("tab", 3usize), cx); // the Settings tab
        window.render_frame(cx);
        assert!(window.find("settings-theme").visible());
        // The dark override pins the mode even in this (light) test window.
        window.click(("settings-appearance", 2usize), cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(Theme::global(cx).is_dark(), "the dark override should apply");
        assert_eq!(app.read(cx).appearance, Appearance::Dark);
    });

    // A bundled family applies at once (its dark side — the override still pins it).
    // The dropdown's items live in a popup overlay, so drive the apply path directly.
    cx.update_window(window.into(), |_, window, cx| {
        app.update(cx, |app, cx| app.set_theme_family("ayu", window, cx));
    })
    .unwrap();
    cx.update(|cx| {
        let theme = Theme::global(cx);
        assert_eq!(theme.theme_name().as_ref(), "Ayu Dark");
    });

    // Back to Auto: the mode follows the test window's (light) appearance again,
    // now in the family's light side — and the session record carries the picks.
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("settings-appearance", 0usize), cx);
    })
    .unwrap();
    cx.update(|cx| {
        let theme = Theme::global(cx);
        assert_eq!(theme.theme_name().as_ref(), "Ayu Light");
        assert!(!theme.is_dark());
        let record: crate::session::Session = app
            .read(cx)
            .store
            .as_ref()
            .expect("a store")
            .fetch_object("session")
            .expect("the settings change should have saved the session");
        assert_eq!(record.theme, "ayu");
        assert_eq!(record.appearance, Appearance::Auto);
    });

    // Leave the shared store at the defaults (the setters save the whole session
    // record, so leave the Settings tab first — it would persist as the boot tab).
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 0usize), cx);
        app.update(cx, |app, cx| {
            app.set_theme_family(crate::theme::DEFAULT_FAMILY, window, cx);
            app.set_appearance(Appearance::Auto, window, cx);
        });
    })
    .unwrap();
}

/// Every bundled theme file parses under the pinned gpui-kit's schema and registers
/// its configs; every family entry names configs that actually registered.
#[gpui_kit::test]
fn bundled_themes_load(cx: &mut TestAppContext) {
    use gpui_kit::component::ThemeRegistry;

    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::load_bundled_themes(cx);
        let registry = ThemeRegistry::global(cx);
        // Default plus every bundled themes/*.json (listed by build.rs).
        assert_eq!(
            crate::theme::theme_families().len(),
            crate::theme::THEME_FILES.len() + 1
        );
        for family in crate::theme::theme_families() {
            for name in [&family.light, &family.dark].into_iter().flatten() {
                assert!(
                    registry.themes().contains_key(name.as_str()),
                    "family {} names {name}, which never registered",
                    family.id
                );
            }
        }
        // The workhorse pair the UI test applies.
        assert!(registry.themes().contains_key("Ayu Light"));
        assert!(registry.themes().contains_key("Ayu Dark"));
    });
}

/// The Devices pane's Rescue toggle: while off, no rescue rows; while on, every
/// swept system MIDI port is a flash target with the shared flash control — whether
/// live FM-1 units are around or not. ALSA owns the port list, so the test drives
/// the mechanism directly (like the download test): a port in the app's sweep-fed
/// rescue list, all inside one window update so the 16 ms tick pump never
/// reconciles the fake port away mid-test.
#[gpui_kit::test]
fn devices_tab_offers_midi_rescue(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 1usize), cx); // the Devices tab
        window.render_frame(cx);
        // Off by default: no rescue rows, whatever ports the sweep fed.
        assert!(window.find("rescue-toggle").visible());
        assert!(window.try_find(("rescue-port", 0usize)).is_none());
        // Arm the toggle; the row and its flash control show, arming works.
        window.click("rescue-toggle", cx);
        assert!(app.read(cx).rescue_on);
        let port = "TestPort:28 TestPort MIDI 1:0".to_string();
        app.update(cx, |app, cx| {
            app.rescue_ports = vec![port.clone()];
            app.open_flash_picker(&port, true, window, cx);
        });
        assert!(
            app.read(cx).rescue_combos.contains_key(&port),
            "the rescue picker should be filed under the port"
        );
        window.render_frame(cx);
        assert!(window.find(("rescue-port", 0usize)).visible());
        window.click(("rescue-flash", 0usize), cx);
        assert_eq!(app.read(cx).flash_armed.get(&port), Some(&true));
        // Disarming hides the rows again.
        window.click("rescue-toggle", cx);
        assert!(!app.read(cx).rescue_on);
    })
    .unwrap();
}

/// The Devices tab's pill sections: Devices and MIDI Graph swap the display area
/// under the centered pill tab bar.
#[gpui_kit::test]
fn devices_tab_sections_switch(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 1usize), cx); // the Devices tab
        window.render_frame(cx);
        assert!(window.find("devices-scroll").visible());
        assert!(window.try_find("graph-scroll").is_none());
        // The pill tab bar sits horizontally centered over the display area.
        let bar = window.find("devices-sections").bounds();
        let pane = window.find("devices-scroll").bounds();
        let bar_center = bar.origin.x + bar.size.width / 2.0;
        let pane_center = pane.origin.x + pane.size.width / 2.0;
        assert!(
            (bar_center - pane_center).abs() < px(2.0),
            "pill bar centered: bar center {bar_center:?} vs pane center {pane_center:?}"
        );
        window.click(1usize, cx); // the MIDI Graph pill
    })
    .unwrap();
    cx.update(|cx| assert_eq!(app.read(cx).devices_tab, 1));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("graph-scroll").visible());
        assert!(window.try_find("devices-scroll").is_none());
        window.click(0usize, cx); // back to the Devices pill
    })
    .unwrap();
    cx.update(|cx| assert_eq!(app.read(cx).devices_tab, 0));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("devices-scroll").visible());
    })
    .unwrap();
}
