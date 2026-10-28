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
    // Tests share one throwaway store for the process (the pin test writes to it).
    if std::env::var_os("FM1_GRID_DATA_DIR").is_none() {
        unsafe {
            std::env::set_var(
                "FM1_GRID_DATA_DIR",
                std::env::temp_dir().join(format!("fm1-grid-uitest-{}", std::process::id())),
            );
        }
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
/// at startup), and its add form gates Save on a chosen file.
#[gpui_kit::test]
fn library_tab_lists(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
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

/// The Devices tab's MIDI rescue: with no FM-1 found, every system MIDI port is
/// offered as a flash target (a "MIDI RESCUE: <port>" row with the shared flash
/// control); with a live unit connected, the rescue rows stay hidden. ALSA owns both
/// lists, so the test drives the mechanism directly (like the download test): a port
/// in the app's sweep-fed rescue list, asserted against whichever side of the
/// "no FM-1" line the machine running the test is on. All inside one window update,
/// so the 16 ms tick pump never reconciles the fake port away mid-test.
#[gpui_kit::test]
fn devices_tab_offers_midi_rescue(cx: &mut TestAppContext) {
    let (window, app) = open_app(cx);
    cx.update_window(window.into(), |_, window, cx| {
        let port = "TestPort:28 TestPort MIDI 1:0".to_string();
        let live = app.read(cx).hardware.devices().len();
        app.update(cx, |app, cx| {
            app.rescue_ports = vec![port.clone()];
            app.open_flash_picker(&port, true, window, cx);
        });
        assert!(
            app.read(cx).rescue_combos.contains_key(&port),
            "the rescue picker should be filed under the port"
        );
        window.click(("tab", 1usize), cx); // the Devices tab
        window.render_frame(cx);
        assert_eq!(app.read(cx).active_tab, 1);
        if live == 0 {
            // The rescue row and its flash control show; arming works like a row's.
            assert!(window.find(("rescue-port", 0usize)).visible());
            window.click(("rescue-flash", 0usize), cx);
            assert_eq!(app.read(cx).flash_armed.get(&port), Some(&true));
        } else {
            // A live unit's rows own the pane; no rescue row shows.
            assert!(window.try_find(("rescue-port", 0usize)).is_none());
        }
    })
    .unwrap();
}
