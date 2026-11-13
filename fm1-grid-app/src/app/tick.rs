use super::*;

impl GridApp {
    /// The 16 ms pump: merge inputs, upload LCD frames, refresh the ghost, decay the
    /// LEDs and ease the grid background. Replaces egui's per-frame `update()` work —
    /// with one retained-mode difference: nothing repaints unless something changed
    /// (plus a half-second sweep for state arriving on background threads), so an idle
    /// window costs a few memcmps instead of 60 renders a second.
    pub(super) fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The pickers rebuild when the library's installed list moved (a download
        // landed, an import, a removal).
        let generation = self.library.generation();
        if generation != self.combos_at_generation {
            self.combos_at_generation = generation;
            self.refresh_firmware_combos(window, cx);
            // A name the installed list gains snaps its row in.
            let installed: std::collections::HashSet<String> = self
                .library
                .list()
                .iter()
                .filter(|f| crate::library_tab::is_installed(f))
                .map(|f| f.name.clone())
                .collect();
            for name in installed.difference(&self.lib_known) {
                self.lib_added.insert(name.clone(), Instant::now());
            }
            self.lib_known = installed;
        }
        // Snap-in springs keep repainting until they settle, then drop.
        if !self.lib_added.is_empty() {
            self.lib_added
                .retain(|_, at| at.elapsed() < Duration::from_millis(400));
            cx.notify();
        }
        // The graph's connection rows animate in and out. The routing graph lives on
        // its poller thread and pane power-offs prune links there too, so this diff
        // catches every change, not just the (X)/Connect clicks.
        if self.sync_graph_anim(self.routing.links()) {
            cx.notify();
        }
        // Download All advances on events only — a download finishing, a hub swap,
        // the toggle's on-click; the pump is cheap while nothing moves. Unlike the
        // row's Download button it leaves lib_scroll_to alone — the list doesn't
        // chase automatic fetches.
        self.library.pump_downloads();
        self.updates.auto_check(cx);

        let mut dirty = self.sync_held();
        dirty |= self.upload_lcds();
        dirty |= self.refresh_ghost();
        dirty |= self.ease_background();
        // The metronome's beat light flips without any UI input.
        let beat = self.clock.beat_light();
        dirty |= std::mem::replace(&mut self.last_beat, beat) != beat;
        // The cards' health dots follow each emulator's five-second dropout window.
        for slot in 0..MAX_SLOTS {
            let (running, drops) = match &self.slots[slot].handle {
                Some(handle) => (true, handle.drops_5s()),
                None => (false, 0),
            };
            let was_degraded = matches!(self.health[slot].color, Health::Orange | Health::Red);
            dirty |= self.health[slot].track(drops, running);
            // The sidebar's hqx/nearest choice is sampled only when the framebuffer
            // changes; a class flip forces one rebuild so a pane that recovers on a
            // static screen gets its hqx back (and a degrading one drops it).
            if slot == self.active
                && matches!(self.health[slot].color, Health::Orange | Health::Red) != was_degraded
            {
                self.last_fb[slot] = None;
            }
        }
        // The cards snap between 1x and 0.5x screens to fit the split panel.
        let scale =
            if self.grid_size.0 >= crate::grid::GRID_W && self.grid_size.1 >= crate::grid::GRID_W {
                1.0
            } else {
                0.5
            };
        dirty |= std::mem::replace(&mut self.grid_scale, scale) != scale;
        // Raw download-mode probes arrive on their own thread.
        dirty |= self.recovery.changed(&mut self.recovery_generation);
        // Flash progress and hotplugged USB devices arrive on the hardware module's
        // threads; sweep them up twice a second.
        self.tick_count = self.tick_count.wrapping_add(1);
        let sweeping = self.tick_count.is_multiple_of(30);
        dirty |= self.hardware.flash_state().is_some() || sweeping;
        if sweeping {
            // Every connected unit gets a flash combobox (built here, where a window
            // lives); gone units drop theirs. A flash that just succeeded re-safes the
            // row's toggle.
            let rows = self.hardware.devices();
            let names: Vec<String> = rows.iter().map(|row| row.name.clone()).collect();
            self.flash_combos.retain(|name, _| names.contains(name));
            for row in rows {
                if !self.flash_combos.contains_key(&row.name) {
                    self.open_flash_picker(&row.name.clone(), false, window, cx);
                    dirty = true;
                }
                let was = self.flash_was.insert(row.name.clone(), row.flash.is_some());
                if was == Some(true)
                    && row.flash.is_none()
                    && row
                        .outcome
                        .as_deref()
                        .is_some_and(|outcome| outcome.starts_with("flashed:"))
                {
                    self.flash_armed.insert(row.name.clone(), false);
                    dirty = true;
                }
            }

            // MIDI rescue follows the Devices pane's Rescue toggle: while armed,
            // every system MIDI port is a flash target, live FM-1 units or not.
            // The sweep also keeps the toggle's warning ring: no FM-1 answered,
            // but something MIDI is out there.
            let ports = self.hardware.midi_ports();
            let warn = names.is_empty() && !ports.is_empty();
            dirty |= std::mem::replace(&mut self.rescue_warn, warn) != warn;
            let rescue = if self.rescue_on { ports } else { Vec::new() };
            if rescue != self.rescue_ports {
                self.rescue_ports = rescue.clone();
                dirty = true;
            }
            self.rescue_combos.retain(|name, _| rescue.contains(name));
            for name in rescue {
                if !self.rescue_combos.contains_key(&name) {
                    self.open_flash_picker(&name.clone(), true, window, cx);
                    dirty = true;
                }
            }
        }
        if dirty {
            cx.notify();
        }
    }
    /// Upload the live LCD pictures. A pane whose framebuffer is unchanged since the
    /// last upload is skipped after a single compare — powered panes with a static
    /// screen cost nothing.
    pub(super) fn upload_lcds(&mut self) -> bool {
        let mut changed = false;
        for slot in 0..MAX_SLOTS {
            let Some(handle) = &self.slots[slot].handle else {
                if self.last_fb[slot].take().is_some() || self.frames[slot].take().is_some() {
                    changed = true;
                }
                continue;
            };
            let Ok(fb) = handle.lcd.lock() else { continue };
            if self.last_fb[slot].as_deref() == Some(fb.as_slice()) {
                continue;
            }
            self.last_fb[slot] = Some(fb.as_slice().into());
            // The selected pane's frame feeds the sidebar, hqx-upscaled to the exact
            // factor its layout chose last frame; the other running panes' grid cards
            // show the recessed look at 1x.
            self.frames[slot] = Some(if slot == self.active {
                let faithful: Vec<lcd::Rgb> = fb.iter().map(|&px| lcd::rgb565(px)).collect();
                let scale = if matches!(self.sidebar_scale, 2 | 3 | 4 | 6) {
                    self.sidebar_scale
                } else {
                    1
                };
                // hqx is the priciest filter here; a pane already dropping audio
                // (orange or worse) gets nearest-neighbour at the same size instead.
                let pixels = if matches!(self.health[slot].color, Health::Orange | Health::Red) {
                    lcd::magnified(&faithful, scale)
                } else {
                    lcd::upscaled(&faithful, scale)
                };
                lcd::frame_image(&pixels, LCD_W * scale, LCD_H * scale)
            } else {
                lcd::frame_image(&lcd::recede(&fb), LCD_W, LCD_H)
            });
            changed = true;
        }

        // LEDs are scanned by the firmware in short pulses; decay them slowly so the panel
        // does not flicker at 60 Hz.
        let lit_before = self.lit_bits();
        let fresh = self.slots[self.active]
            .handle
            .as_ref()
            .map(|handle| handle.leds())
            .unwrap_or([0.0; 41]);
        for (shown, &now) in self.leds.iter_mut().zip(fresh.iter()) {
            *shown = now.max(*shown * 0.8);
        }
        changed || lit_before != self.lit_bits()
    }
    /// The panel's lit-LED set as a bitmap (the rendering threshold), for change
    /// detection across ticks.
    pub(super) fn lit_bits(&self) -> u64 {
        self.leds
            .iter()
            .enumerate()
            .fold(0u64, |bits, (id, &v)| bits | ((v > 0.04) as u64) << id)
    }
    /// The dominant colour of the selected pane's screen. A pane that is off shows black,
    /// and so its dominant colour is black too.
    pub(super) fn dominant_lcd_color(&self) -> lcd::Rgb {
        match self.slots[self.active]
            .handle
            .as_ref()
            .and_then(|handle| handle.lcd.lock().ok())
        {
            Some(fb) => lcd::dominant(&fb),
            None => [0, 0, 0],
        }
    }
    /// Resample the selected pane's dominant colour once a second (and right after a
    /// pane switch) and ease the background towards it over 500 ms whenever it changes.
    /// Reports whether the background moved (easing in progress).
    pub(super) fn ease_background(&mut self) -> bool {
        let switched = self.dominant_slot != self.active;
        if self.bg_sampled_at.elapsed() >= Duration::from_secs(1) || switched {
            self.bg_sampled_at = Instant::now();
            self.dominant_slot = self.active;
            let [r, g, b] = self.dominant_lcd_color();
            let target = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
            if target != self.bg_target {
                self.bg_from = self.bg;
                self.bg_target = target;
                self.bg_at = Instant::now();
            }
        }
        let t = (self.bg_at.elapsed().as_secs_f32() / 0.5).min(1.0);
        if t >= 1.0 && self.bg == self.bg_target {
            return false;
        }
        for ch in 0..3 {
            self.bg[ch] = self.bg_from[ch] + (self.bg_target[ch] - self.bg_from[ch]) * t;
        }
        true
    }
    /// Refresh the snapshot the selected pane shows on its grid card: its screen with
    /// a very slight fisheye bulge, rebuilt only when the framebuffer changed since the
    /// last build (a static screen costs one memcmp per tick). A pane that is off shows
    /// black, blanked once.
    pub(super) fn refresh_ghost(&mut self) -> bool {
        match self.slots[self.active]
            .handle
            .as_ref()
            .and_then(|handle| handle.lcd.lock().ok())
        {
            Some(fb) => {
                if self.ghost_fb.as_deref() == Some(fb.as_slice()) {
                    return false;
                }
                self.ghost_fb = Some(fb.as_slice().into());
                let pixels = lcd::ghost(&fb);
                self.ghost = lcd::frame_image(&pixels, LCD_W, LCD_H);
                self.ghost_live = true;
                true
            }
            None => {
                self.ghost_fb = None;
                if self.ghost_live {
                    self.ghost = lcd::blank_image();
                    self.ghost_live = false;
                    true
                } else {
                    false
                }
            }
        }
    }
    /// Refocus the root view after a button/switch/menu interaction, so the computer
    /// keyboard keeps playing the active pane (egui surrendered widget focus for the
    /// same reason).
    pub(crate) fn refocus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }
}
