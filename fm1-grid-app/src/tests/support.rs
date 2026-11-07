pub(super) use crate::app::GridApp;
// Explicit imports, no glob: with test-support the glob pulls in GPUI's `test`
// attribute macro, which then re-expands the `#[test]` this macro emits (the
// gpui-kit docs warn about exactly this).
pub(super) use gpui_kit::base::Root;
pub(super) use gpui_kit::test::{TestEventExt, TestWindowExt};
pub(super) use gpui_kit::{
    open_window, point, px, size, App, AppContext, Bounds, Entity, Focusable, InputEvent,
    KeyDownEvent, KeyUpEvent, Keystroke, Point, ScrollDelta, TestAppContext, Window, WindowBounds,
    WindowHandle, WindowOptions,
};
pub(super) use std::time::Duration;

/// The UI tests run one at a time: they share a single throwaway store, and a
/// writer's mid-test save must not leak into a sibling's boot session or its reads.
static UI_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn ui_lock() -> std::sync::MutexGuard<'static, ()> {
    UI_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}



/// Open the app in a test window at the real default size.
pub(super) fn open_app(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<GridApp>) {
    open_app_at(cx, 1180.0, 1160.0)
}
/// Open the app in a test window at a given size.
pub(super) fn open_app_at(cx: &mut TestAppContext, w: f32, h: f32) -> (WindowHandle<Root>, Entity<GridApp>) {
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
    // Boot every test app off a default session: writers (the favorite toggle, the
    // settings pickers) save into this shared store, and a previous test's leftover
    // record must not decide which tab or sidebar width a fresh app boots with.
    if let Some(dir) = std::env::var_os("FM1_GRID_DATA_DIR") {
        let store = fm1_performer::store::Store::in_dir(dir.as_ref());
        let _ = store.store_object("session", &crate::session::Session::default());
        // The tempo/filter/Download-All record is shared with fm1-tty and saved by
        // earlier tests in this process; start each app from its defaults too.
        let _ = store.store_object(
            "shared",
            &fm1_performer::session::Shared::default(),
        );
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
            |window, cx| cx.new(|cx| GridApp::new(window, cx)),
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Base Root"), content)
    })
}
/// Dispatch a bare key down/up to the focused view (the rail buttons refocus the
/// root view, so this always reaches the app's own key routing).
pub(super) fn send_key(window: &mut Window, cx: &mut App, key: &str, down: bool) {
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
