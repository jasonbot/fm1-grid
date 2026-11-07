use super::support::*;

#[gpui_kit::test]
fn boots_selects_powers(cx: &mut TestAppContext) {
    let _guard = ui_lock();
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
        cx.background_executor
            .advance_clock(Duration::from_millis(100));
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
    cx.background_executor
        .advance_clock(Duration::from_millis(100));
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
/// The front panel shows only on the Emulator tab, and the computer keys play the
/// pane only while it shows; switching away lifts the held keys (no stuck notes).
/// The metronome row stays global.
#[gpui_kit::test]
fn panel_and_keys_follow_emulator_tab(cx: &mut TestAppContext) {
    let _guard = ui_lock();
    let (window, app) = open_app(cx);

    // Power pane 1 (the default active pane) so held keys have somewhere to go.
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.find("fm1-panel").visible(),
            "panel on the Emulator tab"
        );
        assert!(window.find("midi-clock").visible(), "metronome is global");
        window.click(("slot-card", 0usize), cx);
        window.click("pane-power", cx);
    })
    .unwrap();
    let mut running = false;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(100));
        cx.background_executor
            .advance_clock(Duration::from_millis(100));
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
        assert!(
            window.try_find("fm1-panel").is_none(),
            "panel stays on the Emulator tab"
        );
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
        cx.update_window(window.into(), |_, window, cx| {
            send_key(window, cx, "a", false)
        })
        .unwrap();
    }

    // Back on the Emulator tab the panel returns and keys play again.
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 0usize), cx);
        window.render_frame(cx);
        assert!(
            window.find("fm1-panel").visible(),
            "panel returns with the tab"
        );
        send_key(window, cx, "a", true);
    })
    .unwrap();
    cx.update(|cx| assert!(app.read(cx).held.iter().any(|&down| down)));
    cx.update_window(window.into(), |_, window, cx| {
        send_key(window, cx, "a", false)
    })
    .unwrap();
    cx.update(|cx| assert!(app.read(cx).held.iter().all(|&down| !down)));
}
/// Pinning: inert while the pane is off; on it snapshots the device's flash at
/// power-off and the next boot restores it.
#[gpui_kit::test]
fn pin_cycle(cx: &mut TestAppContext) {
    let _guard = ui_lock();
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
        cx.background_executor
            .advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        if cx.update(|cx| app.read(cx).slots[0].handle.is_some()) {
            break;
        }
    }
    // Pinning swaps the combobox for the firmware's name — the row must not jump.
    let row_h = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.find("pane-attrs").bounds().size.height
        })
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("pin", 0usize), cx);
    })
    .unwrap();
    cx.update(|cx| assert!(app.read(cx).slots[0].pinned, "running pane pins"));
    let pinned_h = cx
        .update_window(window.into(), |_, window, cx| {
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
        cx.background_executor
            .advance_clock(Duration::from_millis(100));
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
/// The quit save keeps every slot's firmware selection and pin: the next boot
/// re-seats them. Unpinning forgets the slot's saved flash at once. The shared
/// store goes back to factory-default records at the end.
#[gpui_kit::test]
fn slot_records_restore(cx: &mut TestAppContext) {
    let _guard = ui_lock();
    let (_window, app) = open_app(cx);

    // Slot 0 runs pinned; slot 2 takes a non-default firmware (still off).
    cx.update(|cx| {
        app.update(cx, |app, _| {
            app.set_power(0, true);
            app.set_pinned(0, true);
            app.load_builtin(2, 1);
        })
    });
    cx.update(|cx| {
        let app = app.read(cx);
        assert!(app.slots[0].handle.is_some(), "slot 0 must run to pin");
        assert!(app.slots[0].pinned);
    });
    cx.update(|cx| app.update(cx, |app, _| app.save_slots()));

    // The "restart": a fresh app over the same store.
    let (_window2, app2) = open_app(cx);
    cx.update(|cx| {
        let app = app2.read(cx);
        assert!(app.slots[0].pinned, "the pin did not restore");
        assert!(matches!(
            app.slots[0].source,
            Some(crate::app::Source::Builtin(0))
        ));
        assert!(
            matches!(app.slots[2].source, Some(crate::app::Source::Builtin(1))),
            "the firmware selection did not restore"
        );
        assert!(!app.slots[2].pinned, "an unpinned slot came back pinned");
    });

    // Unpinning slot 0 forgets it at once: the flash blob goes and the record says
    // unpinned.
    cx.update(|cx| {
        app2.update(cx, |app, _| {
            app.set_power(0, true);
            app.set_pinned(0, false);
        })
    });
    cx.update(|cx| {
        let app = app2.read(cx);
        let store = app.store.as_ref().expect("a store");
        assert!(
            !store
                .list_blobs(None)
                .unwrap()
                .iter()
                .any(|p| p == "slot-0.flash"),
            "unpin left the flash blob behind"
        );
        let record: serde_json::Value = store.fetch_object("slot-0").unwrap();
        assert_eq!(
            record["pinned"],
            serde_json::json!(false),
            "unpin left the record pinned"
        );
    });

    // Leave the shared store at factory defaults for the other tests.
    cx.update(|cx| {
        app2.update(cx, |app, _| {
            for slot in 0..fm1_performer::MAX_SLOTS {
                app.load_builtin(slot, slot);
            }
            app.save_slots();
        })
    });
}
/// A slot set to a library file shows the library's firmware name (and version),
/// not the package's file name; a file outside the library keeps its file name.
#[gpui_kit::test]
fn firmware_selector_shows_library_name(cx: &mut TestAppContext) {
    let _guard = ui_lock();
    let (_window, app) = open_app(cx);
    let dir = std::env::temp_dir().join(format!("fm1-grid-labeltest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let named = dir.join("raw-file-name.fwsc");
    std::fs::write(&named, b"not a real package").unwrap();
    let plain = dir.join("not-in-library.fwsc");
    std::fs::write(&plain, b"not a real package").unwrap();

    cx.update(|cx| {
        app.update(cx, |app, _| {
            app.library
                .import_file(&named.to_string_lossy(), "Nice Firmware Name", "1.2", "");
            app.load_file(1, named.clone());
            app.load_file(2, plain.clone());
            assert_eq!(
                app.slots[1].label.as_deref(),
                Some("Nice Firmware Name 1.2"),
                "the selector must show the library name, not the file name"
            );
            assert_eq!(
                app.slots[2].label.as_deref(),
                Some("not-in-library.fwsc"),
                "a file outside the library keeps its own name"
            );
        })
    });

    // Leave the shared store as the other tests expect it.
    cx.update(|cx| app.update(cx, |app, _| app.library.remove("Nice Firmware Name")));
    std::fs::remove_dir_all(&dir).ok();
}
