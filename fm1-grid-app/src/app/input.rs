use super::*;

impl GridApp {
    /// Reconcile the emulator's key state with the computer keyboard (event-tracked),
    /// the mouse-held controls on the front panel and the physical MIDI keyboard.
    /// Returns whether the emulated key state changed (the panel's LEDs follow it).
    pub(crate) fn sync_held(&mut self) -> bool {
        if self.held_slot.is_some_and(|slot| slot != self.active) {
            self.release_held_keys();
        }

        let mut want = self.panel_down;
        // Notes held on the selected physical MIDI keyboard light the same note keys.
        for (id, down) in self.midi_keys.held().into_iter().enumerate() {
            want[id] |= down;
        }
        for (id, down) in self.kb_down.iter().enumerate() {
            want[id] |= down;
        }

        let before = self.held;
        let Some(handle) = self.slots[self.active].handle.as_ref() else {
            self.held = [false; 41];
            self.held_slot = None;
            return before != self.held;
        };

        for (id, &down) in want.iter().enumerate() {
            if down != self.held[id] {
                handle.send(Action::Key { id: id as u8, down });
                self.held[id] = down;
            }
        }
        self.held_slot = want.iter().any(|&down| down).then_some(self.active);
        before != self.held
    }
    /// PgUp/PgDn on the Library tab: scroll the list by ~a viewport.
    pub(super) fn scroll_library(&mut self, pages: f32) {
        let next = page_offset(
            self.lib_scroll.offset().y.into(),
            self.lib_scroll.max_offset().y.into(),
            self.lib_view_h,
            pages,
        );
        self.lib_scroll.set_offset(point(px(0.0), px(next)));
    }
    /// The scroll handle of the Devices tab's visible section.
    pub(super) fn devices_handle(&self) -> &ScrollHandle {
        match self.devices_tab {
            0 => &self.devices_scroll,
            _ => &self.graph_scroll,
        }
    }
    /// PgUp/PgDn on the Devices tab: scroll the visible section by ~a viewport.
    pub(super) fn scroll_devices(&mut self, pages: f32) {
        let view_h = match self.devices_tab {
            0 => self.devices_view_h,
            _ => self.graph_view_h,
        };
        let handle = self.devices_handle();
        let next = page_offset(handle.offset().y.into(), handle.max_offset().y.into(), view_h, pages);
        handle.set_offset(point(px(0.0), px(next)));
    }
    pub(crate) fn turn_encoder(&self, enc: u8, clicks: i32) {
        if let Some(handle) = &self.slots[self.active].handle {
            handle.send(Action::Enc { enc, clicks });
        }
    }
    pub(crate) fn adjust_master(&mut self, clicks: i32) {
        let value = (self.master[self.active] as i32 + clicks * 16).clamp(0, 1023) as u16;
        self.master[self.active] = value;
        if let Some(handle) = &self.slots[self.active].handle {
            handle.send(Action::Adc { ch: 4, value });
        }
    }
    /// Computer key pressed while the root view has focus: an encoder turn for the
    /// arrow and comma/period keys, an FM-1 key/button down for the rest — but only
    /// on the Emulator tab, the only place the front panel shows; the Library tab
    /// keeps PgUp/PgDn for its list. Modifier chords (Ctrl/Alt/Super) are left
    /// alone; Shift is allowed so Caps Lock and shifted letters still play.
    pub(super) fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        let modifiers = &stroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return;
        }
        match stroke.key.as_str() {
            // The Library tab pages its list.
            "pageup" if self.active_tab == 2 => self.scroll_library(-1.0),
            "pagedown" if self.active_tab == 2 => self.scroll_library(1.0),
            // The Devices tab pages its visible section.
            "pageup" if self.active_tab == 1 => self.scroll_devices(-1.0),
            "pagedown" if self.active_tab == 1 => self.scroll_devices(1.0),
            "home" if self.active_tab == 1 => {
                self.devices_handle().set_offset(point(px(0.0), px(0.0)))
            }
            "end" if self.active_tab == 1 => self.devices_handle().scroll_to_bottom(),
            // Everything below plays the panel's pane; with the panel hidden the
            // keys stay inert.
            _ if self.active_tab != 0 => return,
            "left" => self.turn_encoder(6, -1),
            "right" => self.turn_encoder(6, 1),
            "down" => self.turn_encoder(1, 1),
            "up" => self.turn_encoder(1, -1),
            "," => self.turn_encoder(0, -1),
            "." => self.turn_encoder(0, 1),
            key => {
                let key = key.to_ascii_lowercase();
                let Some((_, id)) = PIANO
                    .iter()
                    .chain(BUTTONS.iter())
                    .find(|(name, _)| *name == key)
                else {
                    return;
                };
                self.kb_down[*id as usize] = true;
                self.sync_held();
            }
        }
        cx.notify();
    }
    /// Key releases only matter where presses are played (the Emulator tab); with the
    /// panel hidden `kb_down` is already empty, so this is a cheap early-out.
    pub(super) fn key_up(&mut self, event: &KeyUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab != 0 {
            return;
        }
        let key = event.keystroke.key.to_ascii_lowercase();
        if let Some((_, id)) = PIANO
            .iter()
            .chain(BUTTONS.iter())
            .find(|(name, _)| *name == key)
        {
            if self.kb_down[*id as usize] {
                self.kb_down[*id as usize] = false;
                self.sync_held();
                cx.notify();
            }
        }
    }
    /// A mouse release anywhere lifts the mouse-held panel keys/buttons. The panel
    /// shows only on the Emulator tab, so only there can any be held.
    pub(super) fn mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab == 0 && self.panel_down.iter().any(|&down| down) {
            self.panel_down = [false; 41];
            self.sync_held();
            cx.notify();
        }
    }
    /// Sanitise the tempo field onto the metronome: parse, clamp into the clock's
    /// range, reformat. Text that is not a number snaps back to the live tempo.
    pub(super) fn apply_tempo(
        &mut self,
        state: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = state.read(cx).value().to_string();
        let bpm = match text.trim().parse::<u32>() {
            Ok(bpm) => bpm.clamp(fm1_performer::clock::MIN_BPM, fm1_performer::clock::MAX_BPM),
            Err(_) => self.clock.bpm(),
        };
        self.clock.set_bpm(bpm);
        let formatted = format!("{bpm:03}");
        if text != formatted {
            state.update(cx, |state, cx| state.set_value(formatted, window, cx));
        }
        self.tempo_slider
            .update(cx, |slider, cx| slider.set_value(bpm as f32, window, cx));
        self.shared_dirty.bpm = true;
        self.save_session(cx);
    }
    /// Fold the routing graph's current links into the animation maps: a new link
    /// snaps in, a vanished one ghosts out. True while rows are still animating.
    pub(crate) fn sync_graph_anim(&mut self, links: Vec<(Port, Port)>) -> bool {
        let links: std::collections::HashSet<(Port, Port)> = links.into_iter().collect();
        for link in links.difference(&self.graph_known) {
            self.graph_removed.remove(link);
            self.graph_added.insert(link.clone(), Instant::now());
        }
        for link in self.graph_known.difference(&links) {
            self.graph_added.remove(link);
            self.graph_removed.insert(link.clone(), Instant::now());
        }
        self.graph_known = links;
        self.graph_added.retain(|_, at| at.elapsed() < GRAPH_ANIM);
        self.graph_removed.retain(|_, at| at.elapsed() < GRAPH_ANIM);
        // A link added and then dropped within the window is gone from both maps.
        !self.graph_added.is_empty() || !self.graph_removed.is_empty()
    }
}
