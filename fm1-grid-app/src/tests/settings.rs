use super::support::*;

#[gpui_kit::test]
fn metronome_toggles(cx: &mut TestAppContext) {
    let _guard = ui_lock();
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
    let _guard = ui_lock();
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
/// The tempo slider and the text field mirror each other: a track click rewrites
/// the field and the clock, and a typed tempo moves the thumb.
#[gpui_kit::test]
fn tempo_slider_syncs(cx: &mut TestAppContext) {
    let _guard = ui_lock();
    let (window, app) = open_app(cx);
    cx.update(|cx| {
        assert_eq!(
            app.read(cx).tempo_slider.read(cx).value(),
            gpui_kit::component::slider::SliderValue::Single(120.0)
        );
    });

    // A click halfway along the track (20..300 BPM) lands on 160 everywhere.
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let track = window.find("slider-bar-container").bounds();
        let at = point(track.left() + track.size.width * 0.5, track.center().y);
        window.pointer_move(at, None, cx);
        window.pointer_down(at, gpui_kit::MouseButton::Left, cx);
        window.pointer_up(at, gpui_kit::MouseButton::Left, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let app = app.read(cx);
        assert_eq!(app.clock.bpm(), 160);
        assert_eq!(app.tempo_input.read(cx).value().as_ref(), "160");
    });

    // A typed tempo moves the thumb.
    cx.update_window(window.into(), |_, window, cx| {
        let tempo = app.read(cx).tempo_input.clone();
        tempo.read(cx).focus_handle(cx).focus(window, cx);
        window.press("ctrl-a", cx);
        window.input("90", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let app = app.read(cx);
        assert_eq!(app.clock.bpm(), 90);
        assert_eq!(
            app.tempo_slider.read(cx).value(),
            gpui_kit::component::slider::SliderValue::Single(90.0)
        );
    });
}
/// The Settings tab: the appearance override pins the mode at once, a bundled theme
/// family applies by name, and each pick lands in the session record (restored to
/// the defaults at the end — the store is shared across the process's tests).
#[gpui_kit::test]
fn settings_theme_and_appearance(cx: &mut TestAppContext) {
    use crate::app::Appearance;
    use gpui_kit::component::Theme;

    let _guard = ui_lock();
    let (window, app) = open_app(cx);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("tab", 3usize), cx); // the Settings tab
        window.render_frame(cx);
        // The settings component renders both fields as dropdown buttons (scoped:
        // both use the same "btn" id).
        assert!(window.within("item-0").find("btn").visible());
        assert!(window.within("item-1").find("btn").visible());
        // The accent field is a swatch row, not a dropdown.
        assert!(window.find("accent-swatch-blue").visible());
        assert!(window.find("accent-swatch-graphite").visible());
        // The default family ignores accents, so the row is disabled and a click
        // must not change the stored choice.
        window.click("accent-swatch-red", cx);
        // Dropdown fields live in popup overlays, so drive the apply path directly.
        // The dark override pins the mode even in this (light) test window.
        app.update(cx, |app, cx| app.set_appearance(Appearance::Dark, cx));
    })
    .unwrap();
    cx.update(|cx| assert_eq!(app.read(cx).accent, "blue"));
    cx.update(|cx| {
        assert!(
            Theme::global(cx).is_dark(),
            "the dark override should apply"
        );
        assert_eq!(app.read(cx).appearance, Appearance::Dark);
    });

    // A bundled family applies at once (its dark side — the override still pins it).
    cx.update_window(window.into(), |_, _window, cx| {
        app.update(cx, |app, cx| app.set_theme_family("beos", cx));
    })
    .unwrap();
    cx.update(|cx| {
        let theme = Theme::global(cx);
        assert_eq!(theme.theme_name().as_ref(), "BeOS Dark");
    });

    // Back to Auto: the mode follows the test platform's (light) appearance again,
    // now in the family's light side — and the session record carries the picks.
    cx.update_window(window.into(), |_, _window, cx| {
        app.update(cx, |app, cx| app.set_appearance(Appearance::Auto, cx));
    })
    .unwrap();
    cx.update(|cx| {
        let theme = Theme::global(cx);
        assert_eq!(theme.theme_name().as_ref(), "BeOS Light");
        assert!(!theme.is_dark());
        let record: crate::session::Session = app
            .read(cx)
            .store
            .as_ref()
            .expect("a store")
            .fetch_object("session")
            .expect("the settings change should have saved the session");
        assert_eq!(record.theme, "beos");
        assert_eq!(record.appearance, Appearance::Auto);
    });

    // Before leaving: Influenza computes from the accent, not from the registry.
    // Pick it, re-render (which enables the swatch row) and choose Red by click.
    cx.update_window(window.into(), |_, _window, cx| {
        app.update(cx, |app, cx| {
            app.set_theme_family(crate::theme::INFLUENZA, cx)
        });
    })
    .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("accent-swatch-red", cx);
    })
    .unwrap();
    cx.update(|cx| {
        let theme = Theme::global(cx);
        assert_eq!(theme.theme_name().as_ref(), "Influenza Light (Red)");
        assert_eq!(
            theme.colors.primary,
            gpui_kit::component::try_parse_color("#ff5257").unwrap()
        );
        assert_eq!(app.read(cx).accent, "red");
        assert!(
            !theme.font_family.contains(','),
            "the theme font list resolves to one installed family: {}",
            theme.font_family
        );
        assert!(
            !theme.mono_font_family.contains(','),
            "the mono list resolves to one installed family: {}",
            theme.mono_font_family
        );
        let record: crate::session::Session = app
            .read(cx)
            .store
            .as_ref()
            .expect("a store")
            .fetch_object("session")
            .expect("the accent change should have saved the session");
        assert_eq!(record.theme, crate::theme::INFLUENZA);
        assert_eq!(record.accent, "red");
    });

    // Meltysand recomputes from the same accent.
    cx.update_window(window.into(), |_, _window, cx| {
        app.update(cx, |app, cx| {
            app.set_theme_family(crate::theme::MELTYSAND, cx)
        });
    })
    .unwrap();
    cx.update(|cx| {
        let theme = Theme::global(cx);
        assert_eq!(theme.theme_name().as_ref(), "Meltysand Light (Red)");
        assert_eq!(
            theme.colors.primary,
            gpui_kit::component::try_parse_color("#ff5257").unwrap()
        );
        // Tahoe's larger surface rounding.
        assert_eq!(theme.radius_lg, px(26.0));
    });

    // Visualia uses the same accent, with its cream light canvas.
    cx.update_window(window.into(), |_, _window, cx| {
        app.update(cx, |app, cx| app.set_theme_family(crate::theme::VISUALIA, cx));
    })
    .unwrap();
    cx.update(|cx| {
        let theme = Theme::global(cx);
        assert_eq!(theme.theme_name().as_ref(), "Visualia Light (Red)");
        assert_eq!(
            theme.colors.background,
            gpui_kit::component::try_parse_color("#fbf9f7").unwrap()
        );
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.click(("tab", 0usize), cx);
        app.update(cx, |app, cx| {
            app.set_theme_family(crate::theme::DEFAULT_FAMILY, cx);
            app.set_accent("blue", cx);
            app.set_appearance(Appearance::Auto, cx);
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
        // Default, Influenza, Meltysand, Visualia, plus every bundled
        // themes/*.json (listed by build.rs).
        assert_eq!(
            crate::theme::theme_families().len(),
            crate::theme::THEME_FILES.len() + 4
        );
        for id in [
            crate::theme::INFLUENZA,
            crate::theme::MELTYSAND,
            crate::theme::VISUALIA,
        ] {
            assert!(crate::theme::theme_families()
                .iter()
                .any(|family| family.id == id));
        }
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
        assert!(registry.themes().contains_key("BeOS Light"));
        assert!(registry.themes().contains_key("BeOS Dark"));
    });
}
