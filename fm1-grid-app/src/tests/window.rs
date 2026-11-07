use super::support::*;

/// The health dot appears on the first drop, lingers a second after recovery, then
/// hides until the colour changes again.
#[test]
fn health_dot_lingers_then_hides() {
    use crate::app::{Health, SlotHealth, DOT_LINGER};

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

    assert!(
        health.track(30, true),
        "a colour change shows the dot again"
    );
    assert!(health.shown && health.color == Health::Red);
    assert!(health.track(0, false), "powering off hides the dot at once");
    assert!(!health.shown);
}
/// At the target device's logical size (1920x1200 at 150% = 1280x800) the grid cards
/// snap to 0.5x and the window still renders.
#[gpui_kit::test]
fn grid_snaps_half_size(cx: &mut TestAppContext) {
    let _guard = ui_lock();
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
        cx.background_executor
            .advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        if cx.update(|cx| app.read(cx).grid_scale) == 0.5 {
            return;
        }
    }
    cx.update(|cx| assert_eq!(app.read(cx).grid_scale, 0.5, "cards never snapped"));
}
/// The sidebar takes its proportional share of extra window width (gpui-base rescales
/// panels only when every panel has a size), so a big window lets its screen reach
/// the exact 4x fit.
#[gpui_kit::test]
fn sidebar_grows_with_window(cx: &mut TestAppContext) {
    let _guard = ui_lock();
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
    assert!(
        sidebar_w > crate::theme::SIDEBAR_INITIAL,
        "sidebar width {sidebar_w}"
    );
    assert_eq!(scale, 4);
}
/// The front panel follows the window in both directions: a narrowed window must not
/// pin its recorded width, so widening it again restores the full panel. At the
/// minimum window width the left control columns and the function-button block stay
/// clear of each other.
#[gpui_kit::test]
fn panel_resizes_with_window(cx: &mut TestAppContext) {
    let _guard = ui_lock();
    let (window, _app) = open_app(cx);
    cx.update_window(window.into(), |_, window, cx| {
        let panel = |window: &mut Window, cx: &mut App| {
            window.render_frame(cx);
            window.render_frame(cx);
            window.find("fm1-panel").bounds()
        };
        assert_eq!(f32::from(panel(window, cx).size.width), 1040.0);

        // 700 wide minus the 48 px icon rail; the old width ratcheted here.
        window.resize(size(px(700.0), px(800.0)));
        window.bounds_changed(cx);
        assert_eq!(f32::from(panel(window, cx).size.width), 652.0);

        window.resize(size(px(1180.0), px(1160.0)));
        window.bounds_changed(cx);
        assert_eq!(
            f32::from(panel(window, cx).size.width),
            1040.0,
            "the panel must grow back with the window"
        );

        // The minimum window: every panel button keeps its own rectangle.
        window.resize(size(px(620.0), px(680.0)));
        window.bounds_changed(cx);
        window.render_frame(cx);
        window.render_frame(cx);
        let buttons: Vec<_> = (0usize..14)
            .map(|id| window.find(("panel-btn", id)).bounds())
            .collect();
        for (i, a) in buttons.iter().enumerate() {
            for (j, b) in buttons.iter().enumerate().skip(i + 1) {
                let overlap = f32::from(a.origin.x)
                    < f32::from(b.origin.x) + f32::from(b.size.width)
                    && f32::from(b.origin.x) < f32::from(a.origin.x) + f32::from(a.size.width)
                    && f32::from(a.origin.y) < f32::from(b.origin.y) + f32::from(b.size.height)
                    && f32::from(b.origin.y) < f32::from(a.origin.y) + f32::from(a.size.height);
                assert!(!overlap, "panel buttons {i} and {j} overlap: {a:?} {b:?}");
            }
        }
    })
    .unwrap();
}
