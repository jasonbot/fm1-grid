use super::support::*;

/// Removing a library firmware powers off the slots set to it and re-seats them
/// with a firmware that's still available.
#[gpui_kit::test]
fn library_remove_reseats_slots(cx: &mut TestAppContext) {
    let _guard = ui_lock();
    let (_window, app) = open_app(cx);

    // A throwaway file firmware, loaded into slot 1 and powered on (the bogus bytes
    // fail the boot; the slot stays set to the file).
    let dir = std::env::temp_dir().join(format!("fm1-grid-rmtest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("zz-remove-me.fwsc");
    std::fs::write(&path, b"not a real package").unwrap();
    cx.update(|cx| {
        app.update(cx, |app, _| {
            app.library
                .import_file(&path.to_string_lossy(), "ZZ Remove Me", "0.0", "");
            app.load_file(1, path.clone());
            app.set_power(1, true);
        })
    });
    cx.update(|cx| {
        let app = app.read(cx);
        assert!(app.powered[1]);
        assert!(matches!(
            app.slots[1].source,
            Some(crate::app::Source::File(_))
        ));
    });

    cx.update(|cx| {
        app.update(cx, |app, _| {
            app.remove_firmware("ZZ Remove Me");
            assert!(!app.powered[1], "the slot kept running a removed firmware");
            match &app.slots[1].source {
                Some(crate::app::Source::File(p)) => {
                    assert!(p != &path, "the slot kept the removed firmware")
                }
                Some(_) => {}
                None => panic!("the slot was not re-seated"),
            }
            assert!(
                app.library
                    .list_clone()
                    .iter()
                    .all(|f| f.name != "ZZ Remove Me"),
                "the library kept the removed entry"
            );
        })
    });

    // Leave the shared store at factory defaults for the other tests.
    cx.update(|cx| {
        app.update(cx, |app, _| {
            for slot in 0..fm1_performer::MAX_SLOTS {
                app.load_builtin(slot, slot);
            }
            app.save_slots();
        })
    });
    std::fs::remove_dir_all(&dir).ok();
}
/// The Library tab lists the builtins with flash targets (panes are off and unpinned
/// at startup), and its add form gates Save on a chosen file. A short window, so the
/// offline-length list (builtins only) overflows and the scroll asserts mean
/// something.
#[gpui_kit::test]
fn library_tab_lists(cx: &mut TestAppContext) {
    let _guard = ui_lock();
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
        // SLOOP knows its pages: the website/source link buttons show.
        assert!(window.find("lib-website-SLOOP").visible());
        assert!(window.find("lib-source-SLOOP").visible());
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
        assert!(
            !app.read(cx).download_all,
            "the toggle should turn back off"
        );
        // Nothing to remove on a builtin.
        assert!(window.try_find("lib-remove-SLOOP").is_none());

        // PgUp/PgDn page the list (we scrolled to the bottom above: up first).
        let bottom = app.read(cx).lib_scroll.offset().y;
        window.press("pageup", cx);
        let up = app.read(cx).lib_scroll.offset().y;
        assert!(up > bottom, "pageup should scroll up: {bottom} -> {up}");
        window.press("pagedown", cx);
        let down = app.read(cx).lib_scroll.offset().y;
        assert!(
            down < up,
            "pagedown should scroll back down: {up} -> {down}"
        );

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
    let _guard = ui_lock();
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
        app.update(cx, |app, _| {
            app.lib_scroll_to = Some("faked-25".to_string())
        });
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
    let _guard = ui_lock();
    let (window, app) = open_app(cx);
    let filter_text = |app: &GridApp, cx: &App| app.lib_form.filter.read(cx).value().to_string();
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
/// The favorite toggle: the star button flips the firmware's membership in
/// `favorites` (persisted in the session record at once, like the theme pick), the
/// row's synthetic ★/☆ pill follows, and the pill filters the list like any tag.
#[gpui_kit::test]
fn library_favorite_toggles(cx: &mut TestAppContext) {
    use crate::library_tab::pill_id;
    let _guard = ui_lock();
    let (window, app) = open_app(cx);
    let filter_text = |app: &GridApp, cx: &App| app.lib_form.filter.read(cx).value().to_string();
    let fav_key = || crate::library_tab::favorite_key("Felucca");
    let record = |app: &GridApp, _cx: &App| {
        app.store
            .as_ref()
            .expect("a store")
            .fetch_object::<crate::session::Session>("session")
            .expect("the favorite toggle should have saved the session")
    };

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("tab", 2usize), cx); // the Library tab
        window.render_frame(cx);
        // Both rows start unfavorited: ☆ pills, and the favorite set is empty.
        assert!(window.find(("lib-tag", pill_id("Felucca", "☆"))).visible());
        assert!(window.find(("lib-tag", pill_id("SLOOP", "☆"))).visible());
        assert!(!app.read(cx).favorites.contains(&fav_key()));
        window.click("lib-fav-Felucca", cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = app.read(cx);
        assert!(app.favorites.contains(&fav_key()));
        assert!(record(app, cx).favorites.contains("felucca"));
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        // Felucca's pill flipped to ★; SLOOP's stays ☆. Clicking SLOOP's ☆ filters
        // to the non-favorites: Felucca drops out.
        assert!(window.find(("lib-tag", pill_id("Felucca", "★"))).visible());
        window.click(("lib-tag", pill_id("SLOOP", "☆")), cx);
    })
    .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(filter_text(app.read(cx), cx), "☆");
        assert!(window.try_find("lib-flash-Felucca").is_none());
        assert!(window.find("lib-flash-SLOOP").visible());
        window.click(("lib-tag", pill_id("SLOOP", "☆")), cx);
    })
    .unwrap();

    // Felucca's ★ pill filters to the favorites only.
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("lib-tag", pill_id("Felucca", "★")), cx);
    })
    .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(filter_text(app.read(cx), cx), "★");
        assert!(window.find("lib-flash-Felucca").visible());
        assert!(window.try_find("lib-flash-SLOOP").is_none());
        // Toggle back off; the record follows.
        window.click("lib-fav-Felucca", cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = app.read(cx);
        assert!(!app.favorites.contains(&fav_key()));
        assert!(!record(app, cx).favorites.contains("felucca"));
    });
}
/// The Library tab's device combo and glyph favorite render and take effect: FM-1
/// off empties the list, and a favorite's custom glyph replaces the ★ pill.
#[gpui_kit::test]
fn library_device_filter_and_favorite_glyph(cx: &mut TestAppContext) {
    use crate::library_tab::{favorite_key, pill_id, NO_THANKS_GLYPH};
    let _guard = ui_lock();
    let (window, app) = open_app_at(cx, 1180.0, 700.0);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(("tab", 2usize), cx); // the Library tab
        window.render_frame(cx);
        assert!(window.find("library-devices").visible());
        assert!(window.find("lib-fav-glyph-Felucca").visible());
        assert!(window.find(("lib-tag", pill_id("Felucca", "☆"))).visible());
        // The favorite selector graduated to the title row and grew: it overlaps the
        // tag pills vertically, and its hit box is large (the old small button was
        // ~24 px; the large one is ~32).
        let star = window.find("lib-fav-Felucca").bounds();
        let pill = window.find(("lib-tag", pill_id("Felucca", "☆"))).bounds();
        assert!(
            star.origin.y < pill.origin.y + pill.size.height
                && pill.origin.y < star.origin.y + star.size.height,
            "the favorite selector shares the title row with the pills: {star:?} vs {pill:?}"
        );
        assert!(
            f32::from(star.size.height) >= 28.0,
            "the favorite selector is large: {star:?}"
        );
        let caret = window.find("lib-fav-glyph-Felucca").bounds();
        assert!(
            f32::from(caret.size.height) < f32::from(star.size.height),
            "the glyph disclosure is smaller than the favorite button: {caret:?} vs {star:?}"
        );
    })
    .unwrap();

    // FM-1 off: no FM-1 row matches a Pro-only filter, so the library is empty.
    cx.update(|cx| {
        app.update(cx, |app, _| {
            app.device_filter = crate::app::DeviceFilter::Smk37Pro;
        });
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("lib-flash-Felucca").is_none());
        assert!(window
            .try_find(("lib-tag", pill_id("Felucca", "☆")))
            .is_none());
    })
    .unwrap();

    // FM-1 back on, Felucca favorited with NoThanks: the row carries it and
    // the session records it.
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.device_filter = crate::app::DeviceFilter::Fm1;
            app.set_favorite_glyph("Felucca", NO_THANKS_GLYPH, cx);
        });
    });
    cx.update(|cx| {
        let app = app.read(cx);
        assert!(app.favorites.contains(&favorite_key("Felucca")));
        let session = app
            .store
            .as_ref()
            .expect("a store")
            .fetch_object::<crate::session::Session>("session")
            .expect("the glyph pick should have saved the session");
        assert_eq!(
            session
                .favorite_glyphs
                .get(&favorite_key("Felucca"))
                .map(String::as_str),
            Some(NO_THANKS_GLYPH)
        );
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window
            .find(("lib-tag", pill_id("Felucca", NO_THANKS_GLYPH)))
            .visible());
        assert!(window
            .try_find(("lib-tag", pill_id("Felucca", "★")))
            .is_none());
        assert!(window.find("lib-fav-glyph-Felucca").visible());
    })
    .unwrap();
}
