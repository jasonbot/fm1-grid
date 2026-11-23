use super::support::*;
use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;

struct Same;
struct Keyed;

/// Cardputers is the third pill on the Devices tab and renders its explanation,
/// repository link, and the two device sections.
#[gpui_kit::test]
fn cardputers_tab_shows_both_sections(cx: &mut TestAppContext) {
    let _guard = ui_lock();
    let (window, app) = open_app(cx);
    app.update(cx, |app, cx| app.set_cardputers_enabled(true, cx));
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 1usize), cx);
        app.update(cx, |app, cx| {
            app.devices_tab = 2;
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(app.read(cx).active_tab, 1);
        assert_eq!(app.read(cx).devices_tab, 2);
        assert!(window.find("cardputers-scroll").visible());
        assert!(window.find("cardputers-repo").visible());
        assert!(window.find("cardputers-flashable").visible());
        assert!(window.find("cardputers-online").visible());
    })
    .unwrap();
}

/// A stable notification id replaces rather than stacks: one Download All run
/// leaves one download toast, while distinct Cardputer copies keep their own.
#[gpui_kit::test]
fn notification_identity_replaces_and_keyed_ids_stack(cx: &mut TestAppContext) {
    let _guard = ui_lock();
    let (window, _) = open_app(cx);
    cx.update_window(window.into(), |_, window, cx| {
        window.push_notification(Notification::success("one").id::<Same>(), cx);
        window.push_notification(Notification::success("two").id::<Same>(), cx);
        assert_eq!(window.notifications(cx).len(), 1);
        window.push_notification(Notification::success("a").id1::<Keyed>("a"), cx);
        window.push_notification(Notification::success("b").id1::<Keyed>("b"), cx);
        assert_eq!(window.notifications(cx).len(), 3);
    })
    .unwrap();
}

/// Turning the preference off leaves the pill in place but replaces the
/// sections with one big enable button; that button turns discovery back on.
#[gpui_kit::test]
fn disabling_cardputers_shows_the_enable_button(cx: &mut TestAppContext) {
    let _guard = ui_lock();
    let (window, app) = open_app(cx);
    app.update(cx, |app, cx| app.set_cardputers_enabled(false, cx));
    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 1usize), cx);
        window.render_frame(cx);
        window.click(2usize, cx); // the Cardputers pill still works
        assert_eq!(app.read(cx).devices_tab, 2);
        assert!(window.find("cardputers-enable").visible());
        assert!(window.try_find("cardputers-online").is_none());
        window.click("cardputers-enable", cx);
        assert!(app.read(cx).cardputers_enabled);
        window.render_frame(cx);
        assert!(window.find("cardputers-online").visible());
        assert!(window.try_find("cardputers-enable").is_none());
    })
    .unwrap();
}
