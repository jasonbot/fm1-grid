use super::*;

impl GridApp {
    pub(crate) fn set_active(&mut self, slot: usize) {
        if slot >= MAX_SLOTS {
            return;
        }
        if self.active != slot {
            self.release_held_keys();
            self.active = slot;
            self.leds = [0.0; 41];
        }
    }
    pub(super) fn release_held_keys(&mut self) {
        if let Some(slot) = self.held_slot.take() {
            if let Some(handle) = &self.slots[slot].handle {
                for (id, down) in self.held.iter().enumerate() {
                    if *down {
                        handle.send(Action::Key {
                            id: id as u8,
                            down: false,
                        });
                    }
                }
            }
        }
        self.held = [false; 41];
        self.panel_down = [false; 41];
        self.kb_down = [false; 41];
    }
    pub(crate) fn load_builtin(&mut self, slot: usize, index: usize) {
        let Some(firmware) = BUILTINS.get(index) else {
            return;
        };
        let label = firmware.label();
        let bytes = firmware.bytes.to_vec();
        self.install(slot, label, bytes, true, Source::Builtin(index));
    }
    /// The library entry's display label (its name and version) for a slot source.
    /// Files and blobs not imported into the library have none.
    pub(super) fn source_label(&mut self, source: &Source) -> Option<String> {
        use fm1_performer::library::FirmwareSource;
        self.library.list_clone().into_iter().find_map(|firmware| {
            let matches = match (&firmware.source, source) {
                (FirmwareSource::File { path }, Source::File(file)) => {
                    std::path::Path::new(path) == file.as_path()
                }
                (FirmwareSource::Hub { blob: Some(key), .. }, Source::Blob { key: blob, .. }) => {
                    key == blob
                }
                _ => false,
            };
            matches.then(|| format!("{} {}", firmware.name, firmware.version))
        })
    }
    pub(crate) fn load_file(&mut self, slot: usize, path: PathBuf) {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.slots[slot].error = Some(format!("{}: {e}", path.display()));
                return;
            }
        };
        let package = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("fwsc"));
        // The selector shows the library's firmware name, not the file's name.
        let label = self
            .source_label(&Source::File(path.clone()))
            .unwrap_or_else(|| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string())
            });
        self.install(slot, label, bytes, package, Source::File(path));
    }
    /// What a picker selected, installed into the slot.
    pub(crate) fn install_pick(&mut self, slot: usize, pick: fm1_performer::firmware_picker::FirmwarePick) {
        use fm1_performer::firmware_picker::FirmwarePick;
        match pick {
            FirmwarePick::Builtin(index) => self.load_builtin(slot, index),
            FirmwarePick::File(path) => self.load_file(slot, path),
            FirmwarePick::HubBlob(key) => self.load_blob(slot, &key),
        }
    }
    /// A downloaded hub package from the blob store into a slot. Goes through the
    /// pick so zip-wrapped packages are unwrapped (and web-page blobs rejected)
    /// exactly like the USB flash path.
    pub(super) fn load_blob(&mut self, slot: usize, key: &str) {
        let Some(store) = &self.store else { return };
        let pick = fm1_performer::firmware_picker::FirmwarePick::HubBlob(key.to_string());
        let bytes = match pick.bytes(Some(store)) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.slots[slot].error = Some(e);
                return;
            }
        };
        // The selector shows the library's firmware name, not the package file's.
        let label = self
            .source_label(&Source::Blob {
                key: key.to_string(),
                label: String::new(),
            })
            .unwrap_or_else(|| key.rsplit('/').next().unwrap_or(key).to_string());
        self.install(
            slot,
            label.clone(),
            bytes,
            true,
            Source::Blob {
                key: key.to_string(),
                label,
            },
        );
    }
    /// Rebuild every slot's firmware combobox from the library and re-select the
    /// slot's current firmware (a download, import or removal changed the list).
    pub(crate) fn refresh_firmware_combos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for slot in 0..MAX_SLOTS {
            let delegate = crate::firmware_picker::FirmwareDelegate::from_app(self);
            let selected = match &self.slots[slot].source {
                Some(Source::Builtin(index)) => {
                    Some(fm1_performer::firmware_picker::FirmwarePick::Builtin(*index))
                }
                Some(Source::File(path)) => {
                    Some(fm1_performer::firmware_picker::FirmwarePick::File(path.clone()))
                }
                Some(Source::Blob { key, .. }) => {
                    Some(fm1_performer::firmware_picker::FirmwarePick::HubBlob(key.clone()))
                }
                None => None,
            };
            self.firmware_combos[slot].update(cx, |state, cx| {
                state.set_items(delegate, window, cx);
                if let Some(selected) = &selected {
                    state.set_selected_values(std::slice::from_ref(selected), window, cx);
                }
            });
        }
        // The devices' and rescue ports' flash comboboxes list the same library (no
        // selection to keep: each flash is a fresh choice).
        let flash_combos: Vec<_> = self
            .flash_combos
            .values()
            .chain(self.rescue_combos.values())
            .cloned()
            .collect();
        for combo in flash_combos {
            let delegate = crate::firmware_picker::FirmwareDelegate::from_app(self);
            combo.update(cx, |state, cx| {
                state.set_items(delegate, window, cx);
            });
        }
    }
    pub(super) fn install(
        &mut self,
        slot: usize,
        label: String,
        bytes: Vec<u8>,
        package: bool,
        source: Source,
    ) {
        self.release_held_keys();
        // Unplug the clock before the emulator thread goes away.
        self.clock.set_target(slot, None);
        self.slots[slot].handle = None;
        self.sync_pane(slot);
        self.slots[slot].usb_midi = fm1_performer::usbmidi::advertised(&bytes);
        self.slots[slot].source = Some(source);
        self.slots[slot].label = Some(label.clone());
        self.slots[slot].error = None;
        self.master[slot] = 900;
        if slot == self.active {
            self.leds = [0.0; 41];
        }
        self.rings[slot].reset();

        // A powered-off slot keeps its firmware selection but runs nothing.
        if !self.powered[slot] {
            return;
        }

        // A pinned slot's device storage comes back with the boot.
        let resume_flash = if self.slots[slot].pinned {
            self.store
                .as_ref()
                .and_then(|store| store.fetch_blob(&format!("slot-{slot}.flash")).ok())
        } else {
            None
        };

        match EmulatorHandle::start(
            label,
            bytes,
            package,
            self.rings[slot].clone(),
            self._audio.is_some(),
            self.core_cpus.get(slot).copied(),
            resume_flash,
        ) {
            Ok(handle) => {
                self.clock.set_target(slot, Some(handle.tx.clone()));
                self.slots[slot].handle = Some(handle);
            }
            Err(e) => self.slots[slot].error = Some(e),
        }
        self.sync_pane(slot);
    }
    /// Keep the pane's MIDI-graph registration in step with its emulator handle: a
    /// running pane is a graph sink (its MIDI IN jack); anything else leaves the graph.
    pub(super) fn sync_pane(&self, slot: usize) {
        let sink = self.slots[slot].handle.as_ref().map(|handle| {
            (
                fm1_performer::routing::pane_label(slot, self.slots[slot].label.as_ref()),
                handle.tx.clone(),
            )
        });
        self.routing.set_pane(slot, sink);
    }
    /// Switch a slot's emulator on or off. Powering on restarts whatever firmware the
    /// slot is set to; powering off stops its thread, silences it and blanks its screen.
    pub(crate) fn set_power(&mut self, slot: usize, on: bool) {
        if self.powered[slot] == on {
            return;
        }
        self.powered[slot] = on;
        if on {
            match self.slots[slot].source.clone() {
                Some(Source::Builtin(index)) => self.load_builtin(slot, index),
                Some(Source::File(path)) => self.load_file(slot, path),
                Some(Source::Blob { key, .. }) => self.load_blob(slot, &key),
                // Nothing to run yet; the switch stays off until a firmware is chosen.
                None => self.powered[slot] = false,
            }
        } else {
            if self.held_slot == Some(slot) {
                self.release_held_keys();
            }
            self.clock.set_target(slot, None);
            // A pinned slot's flash goes to the store before the thread dies.
            if self.slots[slot].pinned {
                self.save_pinned_slot(slot);
            }
            self.slots[slot].handle = None;
            self.sync_pane(slot);
            self.rings[slot].reset();
            if slot == self.active {
                self.leds = [0.0; 41];
            }
            self.frames[slot] = None;
        }
    }
    /// The sidebar's pin toggle. It only moves while the pane's emulator runs — a
    /// stopped pane keeps its state (and its pin) as is. Unpinning forgets the slot
    /// at once: the saved flash blob goes and the record keeps only the firmware.
    pub(crate) fn set_pinned(&mut self, slot: usize, on: bool) {
        if self.slots[slot].handle.is_none() {
            return;
        }
        self.slots[slot].pinned = on;
        if !on {
            if let Some(store) = &self.store {
                let _ = store.delete_blob(&format!("slot-{slot}.flash"));
            }
            self.save_slot_record(slot);
        }
    }
    /// The slot's record for the store: its firmware selection and pin.
    pub(super) fn slot_record(&self, slot: usize) -> Option<SlotRecord> {
        let source = match &self.slots[slot].source {
            Some(Source::Builtin(index)) => RecordSource::Builtin { index: *index },
            Some(Source::File(path)) => RecordSource::File { path: path.clone() },
            Some(Source::Blob { key, label }) => RecordSource::Blob {
                key: key.clone(),
                label: label.clone(),
            },
            None => return None,
        };
        Some(SlotRecord {
            firmware: self.slots[slot].label.clone().unwrap_or_default(),
            source,
            pinned: self.slots[slot].pinned,
        })
    }
    /// Persist the slot's record, if it has a firmware to record.
    pub(super) fn save_slot_record(&self, slot: usize) {
        if let (Some(store), Some(record)) = (&self.store, self.slot_record(slot)) {
            if let Err(e) = store.store_object(&format!("slot-{slot}"), &record) {
                eprintln!("fm1-grid: {e}");
            }
        }
    }
    /// Snapshot and store a pinned slot's running emulator (the power-off and quit
    /// paths share this). Takes and stops the handle either way.
    pub(super) fn save_pinned_slot(&mut self, slot: usize) {
        let mut handle = self.slots[slot].handle.take();
        if let (Some(store), Some(handle)) = (&self.store, handle.as_mut()) {
            if let Some(flash) = handle.stop_with_snapshot() {
                if let Err(e) = store.store_blob(&format!("slot-{slot}.flash"), &flash) {
                    eprintln!("fm1-grid: {e}");
                }
                self.save_slot_record(slot);
            }
        }
    }
    /// The app is going away (the quit hook in main.rs): every slot's firmware
    /// selection and pin land in the store; a pinned slot still running also
    /// snapshots its device storage (one already off saved at power-off).
    pub fn save_slots(&mut self) {
        for slot in 0..MAX_SLOTS {
            if self.slots[slot].pinned && self.slots[slot].handle.is_some() {
                self.save_pinned_slot(slot);
            } else {
                self.save_slot_record(slot);
            }
        }
    }
    /// The Library tab's Remove: the library forgets the entry, then every slot set
    /// to it — running ones power off first — picks a random firmware that's still
    /// available (a stale selection would fail the slot's next boot).
    pub(crate) fn remove_firmware(&mut self, name: &str) {
        use fm1_performer::firmware_picker::{FirmwarePick, pick_of};
        use std::hash::{BuildHasher as _, Hasher as _};
        // What the entry installs, captured before the library forgets it; builtins
        // can't be removed, so the pick is a file or a hub blob.
        let gone = self
            .library
            .list_clone()
            .into_iter()
            .find(|firmware| firmware.name == name)
            .and_then(|firmware| pick_of(&firmware));
        self.library.remove(name);
        let Some(gone) = gone else { return };
        let available: Vec<FirmwarePick> = self
            .library
            .list_clone()
            .iter()
            .filter(|firmware| firmware.status == fm1_performer::library::Status::Available)
            .filter_map(|firmware| pick_of(firmware))
            .collect();
        for slot in 0..MAX_SLOTS {
            let uses_gone = match (&self.slots[slot].source, &gone) {
                (Some(Source::File(path)), FirmwarePick::File(gone)) => path == gone,
                (Some(Source::Blob { key, .. }), FirmwarePick::HubBlob(gone)) => key == gone,
                _ => false,
            };
            if !uses_gone || available.is_empty() {
                continue;
            }
            if self.powered[slot] {
                self.set_power(slot, false);
            }
            // No rand dependency: a fresh RandomState's keys make an unwritten
            // hasher's finish() unpredictable.
            let roll = std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish() as usize;
            self.install_pick(slot, available[roll % available.len()].clone());
            self.save_slot_record(slot);
        }
    }
}
