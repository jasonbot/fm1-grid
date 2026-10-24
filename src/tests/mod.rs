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
    AppContext, Bounds, Entity, Point, TestAppContext, WindowBounds, WindowHandle,
    WindowOptions, open_window, px, size,
};
use std::time::Duration;

/// Open the app in a test window at the real default size.
fn open_app(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<GridApp>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1180.0), px(1160.0)),
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
    // the Devices tab now (tab bar children are indexed 0/1).
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("slot-card", 1usize), cx);
        window.click("pane-power", cx);
    })
    .unwrap();
    cx.background_executor.advance_clock(Duration::from_millis(100));
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.click(1usize, cx); // the Devices tab
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
