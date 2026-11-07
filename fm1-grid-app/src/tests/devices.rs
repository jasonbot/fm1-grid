use super::support::*;

/// The Devices pane's Rescue toggle: while off, no rescue rows; while on, every
/// swept system MIDI port is a flash target with the shared flash control — whether
/// live FM-1 units are around or not. ALSA owns the port list, so the test drives
/// the mechanism directly (like the download test): a port in the app's sweep-fed
/// rescue list, all inside one window update so the 16 ms tick pump never
/// reconciles the fake port away mid-test.
#[gpui_kit::test]
fn devices_tab_offers_midi_rescue(cx: &mut TestAppContext) {
    let _guard = ui_lock();
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
/// The graph's animation map follows the routing diff: a new link snaps in, a
/// removed one ghosts out, and both settle after the window.
#[gpui_kit::test]
fn graph_rows_animate_on_link_changes(cx: &mut TestAppContext) {
    use fm1_performer::routing::Port;
    let _guard = ui_lock();
    let (_window, app) = open_app(cx);
    let link = (Port::Pane(0), Port::Pane(1));
    let gone = (Port::Device("unit".to_string()), Port::Pane(0));

    cx.update(|cx| {
        app.update(cx, |app, _| {
            assert!(app.sync_graph_anim(vec![link.clone(), gone.clone()]));
            assert!(app.graph_added.contains_key(&link));
            assert!(!app.graph_removed.contains_key(&link));

            // Both vanish: the rows ghost out instead of popping.
            assert!(app.sync_graph_anim(Vec::new()));
            assert!(app.graph_added.is_empty());
            assert!(app.graph_removed.contains_key(&link));
            assert!(app.graph_removed.contains_key(&gone));

            // A link that comes back drops its ghost and snaps in again.
            assert!(app.sync_graph_anim(vec![link.clone()]));
            assert!(app.graph_added.contains_key(&link));
            assert!(!app.graph_removed.contains_key(&link));

            // Settled entries drop; nothing left to repaint.
            std::thread::sleep(crate::app::GRAPH_ANIM + Duration::from_millis(10));
            assert!(!app.sync_graph_anim(vec![link.clone()]));
            assert!(app.graph_added.is_empty() && app.graph_removed.is_empty());
        })
    });
}
/// The Devices tab's pill sections: Devices and MIDI Graph swap the display area
/// under the centered pill tab bar.
#[gpui_kit::test]
fn devices_tab_sections_switch(cx: &mut TestAppContext) {
    let _guard = ui_lock();
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
