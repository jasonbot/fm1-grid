use super::*;

impl GridApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let core_cpus = fm1_performer::affinity::core_cpus();

        let rings: [Arc<AudioRing>; MAX_SLOTS] =
            std::array::from_fn(|_| Arc::new(AudioRing::new()));
        let (audio, audio_error) = match start_audio(rings.clone()) {
            Ok(stream) => (Some(stream), None),
            Err(e) => (None, Some(e)),
        };

        let clock = Arc::new(Clock::start(MAX_SLOTS));
        let routing = Routing::start();
        let midi_keys = MidiKeys::start();
        let store = match fm1_performer::store::Store::new() {
            Ok(store) => Some(store),
            Err(e) => {
                eprintln!("fm1-grid: no on-disk store: {e}");
                None
            }
        };
        // The store comes first: the scanner loads its remembered probe names from it.
        let hardware = Hardware::start(clock.clone(), routing.clone());
        // The previous session's layout and tempo; the tempo field below initialises
        // from the clock, so the restored bpm lands on it first.
        let session = crate::session::Session::load(store.as_ref());
        let shared = fm1_performer::session::Shared::load(store.as_ref());
        clock.set_bpm(shared.bpm);
        let emulator_split = cx.new(|_| ResizableState::default());

        let focus = cx.focus_handle().tab_stop(true);
        focus.focus(window, cx);

        // Focus lost (a text field took it, or the window blurred): the keyboard no
        // longer plays the pane, so lift every computer-held key — the same thing
        // egui's `wants_keyboard_input` guard did, and the only chance to avoid stuck
        // notes when the window blurs with keys down.
        let weak = cx.weak_entity();
        let focus_out = window.on_focus_out(&focus, cx, move |_, _, cx| {
            weak.update(cx, |this: &mut Self, cx| {
                if this.kb_down.iter().any(|&down| down) {
                    this.kb_down = [false; 41];
                    this.sync_held();
                    cx.notify();
                }
            })
            .ok();
        });
        let mut subscriptions = vec![focus_out];

        // OS appearance changes apply only while the Settings tab's override is on
        // Auto; Light/Dark pin the mode. The saved theme family and override apply
        // after the app struct exists (below).
        let weak_appearance = cx.weak_entity();
        subscriptions.push(window.observe_window_appearance(move |window, cx| {
            let auto = weak_appearance
                .update(cx, |this: &mut Self, _| this.appearance == Appearance::Auto)
                .unwrap_or(false);
            if auto {
                Theme::sync_system_appearance(Some(window), cx);
            }
        }));

        // One firmware combobox per slot; picking an entry loads it into that slot.
        // The delegates start empty; refresh_firmware_combos fills them from the
        // library once it (and any pins) are loaded.
        let firmware_combos: [Entity<ComboboxState<FirmwareDelegate>>; MAX_SLOTS] =
            std::array::from_fn(|slot| {
                let state = cx.new(|cx| {
                    ComboboxState::new(FirmwareDelegate::default(), vec![], window, cx)
                        .searchable(true)
                });
                // Change fires on the click that selects; Confirm fires when the
                // popup closes — handling both would install twice, so only Change.
                subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    move |this, _, event, window, cx| {
                        if let ComboboxEvent::Change(values) = event {
                            if let Some(pick) = values.last() {
                                this.install_pick(slot, pick.clone());
                            }
                            this.focus.focus(window, cx);
                            cx.notify();
                        }
                    },
                ));
                // A dismissed popup returns the keyboard to the pane.
                subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    |this, _, _: &DismissEvent, window, cx| {
                        this.focus.focus(window, cx);
                    },
                ));
                state
            });

        // The Library tab's add-file form, and the list's filter box (its Change just
        // repaints — the list re-reads the text).
        let lib_form = LibForm {
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Name")),
            version: cx.new(|cx| InputState::new(window, cx).placeholder("Version")),
            description: cx.new(|cx| InputState::new(window, cx).placeholder("Description")),
            file: None,
            filter: cx.new(|cx| InputState::new(window, cx).placeholder("Filter firmwares…")),
        };
        subscriptions.push(
            cx.subscribe_in(&lib_form.filter, window, |_, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        );

        // The tempo field is free-form while it is focused — an invalid or
        // out-of-range value only paints the error state — and the sanitise-and-apply
        // pass runs on blur or Enter. The steppers edit the text without taking
        // focus, so their Change events apply immediately.
        let tempo_input = cx.new(|cx| InputState::new(window, cx));
        tempo_input.update(cx, |state, cx| {
            state.set_value(format!("{:03}", clock.bpm()), window, cx);
        });
        subscriptions.push(cx.subscribe_in(
            &tempo_input,
            window,
            |this, state, event, window, cx| {
                match event {
                    InputEvent::Blur | InputEvent::PressEnter { .. } => {
                        this.apply_tempo(state, window, cx);
                    }
                    InputEvent::Change if !state.read(cx).focus_handle(cx).is_focused(window) => {
                        this.apply_tempo(state, window, cx);
                    }
                    _ => {}
                }
                cx.notify();
            },
        ));

        // The tempo slider next to the field: a drag applies at once and rewrites
        // the field's text; a typed tempo moves the thumb from apply_tempo. The
        // slider's own set_value emits no event, so the mirror can't loop.
        let tempo_slider = cx.new(|_| {
            SliderState::new()
                .min(fm1_performer::clock::MIN_BPM as f32)
                .max(fm1_performer::clock::MAX_BPM as f32)
                .step(1.0)
                .default_value(clock.bpm() as f32)
        });
        subscriptions.push(cx.subscribe_in(
            &tempo_slider,
            window,
            |this, _, event, window, cx| {
                // Both events carry the value; Release simply repeats the last one.
                let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
                let bpm = value.start().round() as u32;
                this.clock.set_bpm(bpm);
                let formatted = format!("{bpm:03}");
                if this.tempo_input.read(cx).value() != formatted {
                    this.tempo_input.update(cx, |state, cx| {
                        state.set_value(formatted, window, cx);
                    });
                }
                if matches!(event, SliderEvent::Release(_)) {
                    this.shared_dirty.bpm = true;
                    this.save_session(cx);
                }
                cx.notify();
            },
        ));

        // The frame pump: 16 ms, replacing egui's request_repaint_after. spawn_in
        // carries the window — the library-generation sweep rebuilds pickers, and
        // that needs one.
        let tick = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.tick(window, cx))
                    .is_err()
                {
                    break; // the window is gone
                }
            }
        });

        // The library needs a store even when the platform has none (a throwaway one).
        let fallback_store;
        let store_ref = match &store {
            Some(store) => store,
            None => {
                fallback_store =
                    fm1_performer::store::Store::in_dir(&std::env::temp_dir().join("fm1-grid-nostore"));
                &fallback_store
            }
        };
        // The hub catalog comes in at app load; later refreshes keep an hourly cadence.
        let mut library = fm1_performer::library::Library::load(store_ref);
        library.set_feeds(session.feeds);
        if hub_online() {
            library.refresh_hub();
        }
        // Download All only walks the device families the Library tab shows.
        library.set_device_filter(Some(shared.device_filter));
        // An armed Download All sweeps when the first catalog lands (the pump's
        // hub-swap trigger), not on a poll.
        library.set_download_all(shared.download_all);
        // The snap-in animation plays for rows the library gains from here on;
        // what's already installed at boot is known, not new.
        let lib_known = library
            .list()
            .iter()
            .filter(|f| crate::library_tab::is_installed(f))
            .map(|f| f.name.clone())
            .collect();

        let mut app = Self {
            slots: std::array::from_fn(|_| Slot::default()),
            core_cpus,
            active: session.slot,
            clock,
            hardware,
            routing,
            midi_keys,
            flash_armed: Default::default(),
            flash_combos: Default::default(),
            flash_was: Default::default(),
            rescue_on: false,
            rescue_warn: false,
            rescue_ports: Default::default(),
            rescue_combos: Default::default(),
            rings,
            _audio: audio,
            audio_error,
            frames: std::array::from_fn(|_| None),
            last_fb: std::array::from_fn(|_| None),
            ghost: lcd::blank_image(),
            ghost_fb: None,
            ghost_live: false,
            kb_down: [false; 41],
            held: [false; 41],
            panel_down: [false; 41],
            leds: [0.0; 41],
            master: [900; MAX_SLOTS],
            powered: [false; MAX_SLOTS],
            bg: [0.0; 3],
            bg_from: [0.0; 3],
            bg_target: [0.0; 3],
            bg_at: Instant::now(),
            bg_sampled_at: Instant::now() - Duration::from_secs(1),
            dominant_slot: usize::MAX,
            sidebar_scale: 1,
            sidebar_size: (512.0, 700.0),
            grid_size: (crate::grid::GRID_W, crate::grid::GRID_W),
            grid_scale: 1.0,
            emulator_split,
            sidebar_width: session.sidebar_width,
            health: [SlotHealth::default(); MAX_SLOTS],
            active_tab: session.tab,
            devices_tab: session.devices_tab,
            about_tab: 0,
            about_scroll: VirtualListScrollHandle::new(),
            about_lines: Rc::new(Vec::new()),
            about_sizes: Rc::new(Vec::new()),
            devices_scroll: ScrollHandle::new(),
            graph_scroll: ScrollHandle::new(),
            devices_view_h: 400.0,
            graph_view_h: 400.0,
            panel_width: 1040.0,
            wheel_acc: (u32::MAX, 0.0),
            last_beat: false,
            tick_count: 0,
            held_slot: None,
            focus,
            firmware_combos,
            tempo_input,
            tempo_slider,
            store,
            library,
            lib_form,
            lib_scroll: VirtualListScrollHandle::new(),
            lib_view_h: 400.0,
            favorites: session
                .favorites
                .iter()
                .map(|name| crate::library_tab::favorite_key(name))
                .collect(),
            favorite_glyphs: session
                .favorite_glyphs
                .iter()
                .map(|(name, glyph)| (crate::library_tab::favorite_key(name), glyph.clone()))
                .collect(),
            device_filter: shared.device_filter,
            lib_scroll_to: None,
            lib_known,
            lib_added: Default::default(),
            lib_view: Rc::new(Vec::new()),
            lib_items: Rc::new(Vec::new()),
            lib_sizes: Rc::new(Vec::new()),
            lib_stream_key: None,
            favorites_rev: 0,
            graph_added: Default::default(),
            graph_removed: Default::default(),
            graph_known: Default::default(),
            download_all: shared.download_all,
            shared_dirty: fm1_performer::session::Dirty::default(),
            theme_id: session.theme.clone(),
            appearance: session.appearance,
            accent: session.accent.clone(),
            feeds_dirty: false,
            rail_h: 740.0,
            recovery: crate::recovery::RecoveryState::start(),
            recovery_generation: 0,
            combos_at_generation: 0,
            _tick: tick,
            _subscriptions: subscriptions,
        };

        // Load the embedded firmwares into the first panes (powered off, like all).
        for slot in 0..BUILTINS.len().min(MAX_SLOTS) {
            app.load_builtin(slot, slot);
        }
        // Slot records come back: each pane's firmware selection and pin. Read every
        // record first (the store borrows the app immutably; applying them mutates it).
        let records: Vec<(usize, SlotRecord)> = match &app.store {
            Some(store) => (0..MAX_SLOTS)
                .filter_map(|slot| {
                    store
                        .fetch_object::<SlotRecord>(&format!("slot-{slot}"))
                        .ok()
                        .map(|record| (slot, record))
                })
                .collect(),
            None => Vec::new(),
        };
        for (slot, record) in records {
            let pinned = record.pinned;
            let restored = match &record.source {
                RecordSource::Builtin { index } if *index < BUILTINS.len() => {
                    app.load_builtin(slot, *index);
                    true
                }
                RecordSource::File { path } if path.is_file() => {
                    app.load_file(slot, path.clone());
                    true
                }
                RecordSource::Blob { key, .. }
                    if app
                        .store
                        .as_ref()
                        .is_some_and(|store| store.fetch_blob(key).is_ok()) =>
                {
                    app.load_blob(slot, key);
                    true
                }
                _ => false,
            };
            if restored {
                app.slots[slot].pinned = pinned;
            }
        }

        // The pickers' lists and selections come from the library.
        app.combos_at_generation = app.library.generation();
        app.refresh_firmware_combos(window, cx);

        // The bundled theme sets register before the saved family applies (a
        // registry change re-applies by name, so the order matters).
        crate::theme::load_bundled_themes(cx);
        app.apply_theme(cx);

        app
    }
}
