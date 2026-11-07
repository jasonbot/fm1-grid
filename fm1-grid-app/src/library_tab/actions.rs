use super::*;

/// Whether the row can offer Flash To…. SMK-37 rows are allowed; the target menu
/// limits them to live units.
pub(super) fn flashable(firmware: &Firmware, pick: Option<&FirmwarePick>) -> bool {
    pick.is_some() && firmware.status == Status::Available
}

impl GridApp {
    /// The Library tab's content: the grouped list, then the add-file form.
    pub(crate) fn library_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let needle = self.lib_form.filter.read(cx).value().trim().to_string();
        // `list()` folds finished downloads in and re-stats missing files (throttled);
        // calling it here keeps those side effects, then the cached stream below only
        // rebuilds when its view, the filter or the favorites moved.
        self.library.list();
        self.ensure_lib_stream(&needle);
        let view = Rc::clone(&self.lib_view);
        let items = Rc::clone(&self.lib_items);
        let sizes = Rc::clone(&self.lib_sizes);

        // A freshly downloaded firmware jumps into the Installed section; bring it
        // into view. Set by the Download button; clears when hit. (A filter hiding
        // the row just delays the scroll until the filter clears.)
        if let Some(target) = self.lib_scroll_to.clone() {
            let hit = items.iter().position(|item| match item {
                LibItem::Row(ix) => {
                    let firmware = &view[*ix];
                    firmware.name == target && section_of(firmware) == Section::Installed
                }
                _ => false,
            });
            if let Some(ix) = hit {
                self.lib_scroll.scroll_to_item(ix, ScrollStrategy::Center);
                self.lib_scroll_to = None;
            }
        }

        let devices = self.device_filter;
        let device_label = devices.label();
        let device_app = cx.entity().clone();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .pt_2()
                    .gap_2()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.lib_form.filter)
                                .id("library-filter")
                                .prefix(Icon::new(IconName::Search).xsmall()),
                        ),
                    )
                    .child(
                        // Medium, like the filter input beside it.
                        Button::new("library-devices")
                            .label(device_label)
                            .icon(Icon::new(Lucide::ListFilter))
                            .outline()
                            .tooltip("Which device family the library lists")
                            .dropdown_menu({
                                let app = device_app.clone();
                                move |menu, _, _| {
                                    let mut menu = menu;
                                    for choice in DeviceFilter::ALL {
                                        let app = app.clone();
                                        let on = choice == devices;
                                        menu = menu.item(
                                            PopupMenuItem::new(choice.label()).checked(on).on_click(
                                                move |_, _, cx| {
                                                    app.update(cx, |this, cx| {
                                                        this.device_filter = choice;
                                                        this.library
                                                            .set_device_filter(Some(choice));
                                                        this.shared_dirty.device_filter = true;
                                                        this.save_session(cx);
                                                        cx.notify();
                                                    });
                                                },
                                            ),
                                        );
                                    }
                                    menu
                                }
                            }),
                    ),
            )
            .child(
                // The scrollbar pins to this outer element; the scrolling virtual
                // list is inside it (stacking them on one element scrolled the bar
                // away).
                div()
                    .id("library-list")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .vertical_scrollbar(&self.lib_scroll)
                    // PgUp/PgDn scroll by the viewport; record it.
                    .on_prepaint({
                        let app = cx.entity().clone();
                        move |bounds, _, cx| {
                            let height: f32 = bounds.size.height.into();
                            app.update(cx, |this, _| this.lib_view_h = height);
                        }
                    })
                    .child(
                        v_virtual_list(cx.entity().clone(), "library-items", sizes, {
                            let items = Rc::clone(&items);
                            let view = Rc::clone(&view);
                            move |this, range, _window, cx| {
                                range
                                    .map(|ix| this.lib_item(&items[ix], &view, cx))
                                    .collect()
                            }
                        })
                        .track_scroll(&self.lib_scroll)
                        // The scrollbar overlays the list's right edge (16 px,
                        // gpui-base's WIDTH); keep the content out from under it.
                        .p_2()
                        .pr(px(16.0)),
                    ),
            )
            .child(self.library_add_form(window, cx))
    }
    /// Rebuild the flattened row stream and its size table when an input moves.
    /// Favorites sort first within each section.
    pub(super) fn ensure_lib_stream(&mut self, needle: &str) {
        let key = (
            self.library.view_generation(),
            self.library.generation(),
            needle.to_string(),
            self.favorites_rev,
            self.device_filter,
        );
        if self.lib_stream_key.as_ref() == Some(&key) {
            return;
        }
        let view: Vec<Firmware> = self.library.list().to_vec();
        let items = flatten_library(
            &view,
            needle,
            &self.favorites,
            &self.favorite_glyphs,
            self.device_filter,
        );
        let sizes: Vec<Size<Pixels>> = items
            .iter()
            .map(|item| size(px(0.0), item.height()))
            .collect();

        self.lib_view = Rc::new(view);
        self.lib_sizes = Rc::new(sizes);
        self.lib_items = Rc::new(items);
        self.lib_stream_key = Some(key);
    }
    /// One item of the flattened stream, rendered at its pinned height.
    pub(super) fn lib_item(
        &mut self,
        item: &LibItem,
        view: &[Firmware],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match item {
            LibItem::Header(header) => {
                let (dim, border) = {
                    let theme = cx.theme();
                    (theme.muted_foreground, theme.border)
                };
                v_flex()
                    .w_full()
                    .h(LIB_HEADER_H)
                    .justify_end()
                    .child(
                        div()
                            .w_full()
                            .pb_1()
                            .text_size(px(9.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(dim)
                            .child(*header)
                            .border_b_1()
                            .border_color(border),
                    )
                    .into_any_element()
            }
            LibItem::Row(view_ix) => match view.get(*view_ix).cloned() {
                Some(firmware) => self.library_item(&firmware, cx),
                // The view is fixed while the cached stream is valid; a missing row
                // costs one blank item rather than a panic.
                None => div().w_full().h(LIB_ROW_H).into_any_element(),
            },
            LibItem::Empty => {
                let dim = cx.theme().muted_foreground;
                let needle = self.lib_form.filter.read(cx).value().trim().to_lowercase();
                div()
                    .w_full()
                    .h(LIB_EMPTY_H)
                    .text_size(px(11.0))
                    .text_color(dim)
                    .child(if needle.is_empty() {
                        "No firmwares? That can't be right."
                    } else {
                        "No firmware matches the filter."
                    })
                    .into_any_element()
            }
            LibItem::HubStatus => div()
                .w_full()
                .h(LIB_STATUS_H)
                .overflow_hidden()
                .child(self.hub_status_item(cx))
                .into_any_element(),
        }
    }
    /// The list's last item: the hub's fetch status — a spinner while it loads, then
    /// the Download All toggle and Reload ("Retry" after a failure) as one group.
    pub(super) fn hub_status_item(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use fm1_performer::library::HubStatus;
        let (dim, danger, border) = {
            let theme = cx.theme();
            (theme.muted_foreground, theme.danger, theme.border)
        };
        let status = self.library.hub_status();
        let loading = matches!(status, HubStatus::Loading);
        let failed = matches!(status, HubStatus::Failed(_));

        let text = match &status {
            HubStatus::Loading => "checking the FM-1 Firmware Hub…".to_string(),
            HubStatus::Failed(e) => format!("the hub didn't answer: {e}"),
            HubStatus::Idle => {
                let n = self.library.hub_len();
                if n > 0 {
                    format!("{n} firmwares on the hub")
                } else {
                    "the hub hasn't been checked".to_string()
                }
            }
        };

        h_flex()
            .id("hub-status")
            .test_support()
            .w_full()
            .items_center()
            .gap_2()
            .pt_2()
            .child(div().w(px(14.0)).h(px(14.0)).child(if loading {
                Spinner::new().into_any_element()
            } else {
                div()
                    .size_full()
                    .rounded_full()
                    .bg(if failed { danger } else { dim })
                    .into_any_element()
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(10.0))
                    .text_color(if failed { danger } else { dim })
                    .child(text),
            )
            .child(action_group(
                vec![
                    // Download All is a toggle: while on, the tick walks the list,
                    // downloading one item at a time (see GridApp::tick).
                    Button::new("hub-download-all")
                        .label("Download All")
                        .icon(Icon::new(Lucide::Download))
                        .xsmall()
                        .ghost()
                        .rounded_none()
                        .selected(self.download_all)
                        .toggled(self.download_all)
                        .tooltip(
                            "Download every downloadable or upgradable firmware, one at a time",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.download_all = !this.download_all;
                            // Turning it on sweeps at once (and retries what failed
                            // last time); from there the tick's pump only advances
                            // on a finished download or a hub swap.
                            this.library.set_download_all(this.download_all);
                            this.shared_dirty.download_all = true;
                            this.save_session(cx);
                            cx.notify();
                        }))
                        .into_any_element(),
                    // Reload shows pressed while the fetch is in flight.
                    Button::new("hub-reload")
                        .label(if failed { "Retry" } else { "Reload" })
                        .icon(Icon::new(Lucide::RefreshCw))
                        .xsmall()
                        .ghost()
                        .rounded_none()
                        .selected(loading)
                        .toggled(loading)
                        .disabled(loading)
                        .tooltip("Fetch every online listing again")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.library.reload_hub();
                            cx.notify();
                        }))
                        .into_any_element(),
                ],
                border,
            ))
            .into_any_element()
    }
    /// One firmware row: name + pills over byline + description, then the actions.
    pub(super) fn library_item(&mut self, firmware: &Firmware, cx: &mut Context<Self>) -> AnyElement {
        let (ink, dim, border) = {
            let theme = cx.theme();
            (theme.foreground, theme.muted_foreground, theme.border)
        };
        let firmware = firmware.clone();
        let name = firmware.name.clone();
        let pick = pick_of(&firmware);
        let flashable = flashable(&firmware, pick.as_ref());
        let downloading = firmware.status == Status::Downloading;
        let percent = self.library.download_percent(&firmware.name);

        // The pills: the favorite glyph (★ by default, ☆ when not a favorite) first,
        // then one per tag, then the device. A tag pill click toggles the tag's words
        // in the filter box. One row only — a taller wrap would break the row's fixed
        // height; extras clip instead.
        let favorite = self.favorites.contains(&favorite_key(&firmware.name));
        let glyph = fav_glyph(&self.favorite_glyphs, &firmware.name, favorite).to_string();
        // A NoThanks favorite greys its row out.
        let no_thanks = favorite && glyph == NO_THANKS_GLYPH;
        let tags = std::iter::once(glyph.clone()).chain(firmware.tags.iter().cloned());
        // The pill hues follow the theme: the accent for the computed families,
        // the primary otherwise. Same tag, same slice of the wheel.
        let base = if matches!(
            self.theme_id.as_str(),
            crate::theme::INFLUENZA | crate::theme::MELTYSAND | crate::theme::VISUALIA
        ) {
            crate::themes::color(crate::themes::accent(&self.accent))
        } else {
            cx.theme().primary
        };
        let palette = tag_palette(base);
        let mut pills = h_flex().gap_1().flex_nowrap().min_w_0().overflow_hidden();
        for tag in tags {
            let color = palette[tag_index(&tag)];
            let id_tag = tag.clone();
            pills = pills.child(
                div()
                    .id(("lib-tag", pill_id(&firmware.name, &id_tag)))
                    .test_support()
                    .rounded_full()
                    .px(px(6.0))
                    .py(px(1.0))
                    .text_size(px(9.0))
                    .bg(color.opacity(0.16))
                    .text_color(color)
                    .cursor_pointer()
                    .hover(move |el| el.bg(color.opacity(0.3)))
                    .tooltip(|window, cx| {
                        Tooltip::new("Toggle this tag in the filter").build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let text = this.lib_form.filter.read(cx).value().to_string();
                        let next = toggle_tag(&text, &tag);
                        this.lib_form.filter.update(cx, |state, cx| {
                            state.set_value(next, window, cx);
                        });
                        this.refocus(window, cx);
                        cx.notify();
                    }))
                    .child(id_tag),
            );
        }
        let device = match firmware.device {
            FirmwareDevice::Fm1 => "FM-1".to_string(),
            // The model (Pro, Elite, MKE-P37) is the variant tag.
            FirmwareDevice::Smk37 => firmware.variant().unwrap_or("SMK-37").to_string(),
        };
        pills = pills.child(
            div()
                .rounded_full()
                .px(px(6.0))
                .py(px(1.0))
                .text_size(px(9.0))
                .bg(cx.theme().muted)
                .text_color(dim)
                .child(device),
        );

        // The favorite toggle + glyph menu, promoted to the title row and sized
        // large: the glyph pill in the tag row stays its searchable readout.
        let favorite_control = {
            let name = name.clone();
            let glyph = glyph.clone();
            let star_name = name.clone();
            let star = Button::new(format!("lib-fav-{star_name}"))
                .label(glyph.clone())
                .large()
                .ghost()
                .rounded_none()
                .px_1()
                .selected(favorite)
                .toggled(favorite)
                .tooltip(if favorite {
                    "Drop the favorite"
                } else {
                    "Mark as a favorite (its glyph pill filters the list)"
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_favorite(&star_name, cx);
                    cx.notify();
                }))
                .into_any_element();

            let current = glyph.clone();
            let app = cx.entity().clone();
            let caret = Button::new(format!("lib-fav-glyph-{name}"))
                .icon(Icon::new(Lucide::ChevronDown))
                .small()
                .ghost()
                .rounded_none()
                .px_1()
                .tooltip("Choose the favorite's glyph")
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for glyph in GLYPHS {
                        let app = app.clone();
                        let name = name.clone();
                        let shown = glyph.to_string();
                        menu = menu.item(
                            PopupMenuItem::new(glyph)
                                .checked(current == shown)
                                .on_click(move |_, _, cx| {
                                    app.update(cx, |this, cx| {
                                        this.set_favorite_glyph(&name, &shown, cx);
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    menu
                })
                .into_any_element();

            // The same fused, border-only chrome as the row's action group: two
            // ghost hit areas divided by a hairline, no fill of its own.
            action_group(vec![star, caret], border)
        };

        // The actions as one fused group: the website/source link pair (disabled when
        // the listing carries no such link), Remove (never builtins, and only when
        // the package is at hand), Download (hub offers; its percent while in
        // flight), Flash To…
        let locally = pick.is_some() && firmware.status == Status::Available;
        let mut buttons: Vec<AnyElement> = Vec::new();
        for (kind, label, icon, url) in [
            ("website", "Web site", Lucide::Globe, &firmware.website),
            ("source", "Github", Lucide::Github, &firmware.source_url),
        ] {
            let url = url.clone();
            buttons.push(
                Button::new(format!("lib-{kind}-{name}"))
                    .label(label)
                    .icon(Icon::new(icon))
                    .small()
                    .ghost()
                    .rounded_none()
                    .disabled(url.is_none())
                    .tooltip(match &url {
                        Some(url) => format!("Open the {kind}: {url}"),
                        None => format!("No {kind} link known"),
                    })
                    .on_click(move |_, _, cx| {
                        if let Some(url) = &url {
                            cx.open_url(url);
                        }
                    })
                    .into_any_element(),
            );
        }
        if !matches!(firmware.source, FirmwareSource::Builtin) {
            let name = name.clone();
            buttons.push(
                Button::new(format!("lib-remove-{name}"))
                    .label("Remove")
                    .icon(Icon::new(Lucide::Trash))
                    .small()
                    .ghost()
                    .rounded_none()
                    .disabled(!locally)
                    .tooltip(if locally {
                        "Forget this firmware (the package stays on disk)"
                    } else {
                        "Only a firmware that's here can be removed"
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.remove_firmware(&name);
                        this.refresh_firmware_combos(window, cx);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        // Hub offers download; a stored hub blob that's gone (or a web page saved by
        // the page-link bug) offers the download again.
        let downloadable = match &firmware.source {
            FirmwareSource::Hub { blob: None, .. } => true,
            FirmwareSource::Hub { .. } => firmware.status == Status::Missing,
            _ => false,
        };
        if downloadable {
            if downloading {
                buttons.push(
                    Button::new(format!("lib-download-{name}"))
                        .label(format!("{}%", percent.unwrap_or(0)))
                        .icon(Icon::new(Lucide::Download))
                        .small()
                        .ghost()
                        .rounded_none()
                        .disabled(true)
                        .into_any_element(),
                );
            } else {
                let name = name.clone();
                buttons.push(
                    Button::new(format!("lib-download-{name}"))
                        .label("Download")
                        .icon(Icon::new(Lucide::Download))
                        .small()
                        .ghost()
                        .rounded_none()
                        .tooltip("Download from the FM-1 Firmware Hub into the library")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.library.start_download(&name);
                            // On completion the row lands in Installed; scroll to it.
                            this.lib_scroll_to = Some(name.clone());
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        buttons.push(self.flash_to_menu(&firmware, pick, flashable, cx));

        let mut line = String::new();
        if !firmware.byline.is_empty() {
            line.push_str(&format!("by {} — ", firmware.byline));
        }
        line.push_str(&firmware.description);

        // A row the Installed section just gained snaps in: a 6 px rise on the
        // spring curve, faded in over the first 120 ms. Settled entries are gone
        // from lib_added, so a resting row pays one map lookup.
        let (nudge, fade) = match self.lib_added.get(&firmware.name) {
            Some(at) => {
                let t = at.elapsed().as_secs_f32();
                ((1.0 - snap_in(t)) * 6.0, (t / 0.12).min(1.0))
            }
            None => (0.0, 1.0),
        };

        // Fixed height (the virtual list positions rows from LIB_ROW_H): the
        // description is one truncated line and overflow clips.
        v_flex()
            .w_full()
            .h(LIB_ROW_H)
            .overflow_hidden()
            .items_start()
            .gap_1()
            .py_1()
            .pt(px(4.0 + nudge))
            .opacity(fade * if no_thanks { 0.5 } else { 1.0 })
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(ink)
                                    .child(firmware.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(dim)
                                    .child(firmware.version.clone()),
                            )
                            .child(favorite_control),
                    )
                    .child(pills),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(10.5))
                    .text_color(dim)
                    .truncate()
                    .child(line),
            )
            .child(action_group(buttons, border))
            .into_any_element()
    }
    /// The add-file form at the bottom of the list.
    pub(super) fn library_add_form(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (dim, border) = {
            let theme = cx.theme();
            (theme.muted_foreground, theme.border)
        };
        let chosen = self
            .lib_form
            .file
            .as_ref()
            .map(|path| path.display().to_string());
        let ready = self.lib_form.file.is_some();

        h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_2()
            .p_2()
            .border_t_1()
            .border_color(border)
            .child(
                Button::new("lib-add-file")
                    .label("Add file…")
                    .icon(Icon::new(Lucide::FilePlus))
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| {
                        let Some(path) = rfd::FileDialog::new()
                            .set_title("Choose an FM-1 firmware")
                            .add_filter("FM-1 firmware", &["fwsc", "bin", "xip"])
                            .add_filter("All files", &["*"])
                            .pick_file()
                        else {
                            return;
                        };
                        // Pre-fill the name from the file when the field is empty.
                        if this.lib_form.name.read(cx).value().trim().is_empty() {
                            let stem = path
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            this.lib_form
                                .name
                                .update(cx, |state, cx| state.set_value(stem, window, cx));
                        }
                        this.lib_form.file = Some(path);
                        cx.notify();
                    })),
            )
            .when_some(chosen, |el, path| {
                el.child(
                    div()
                        .text_size(px(10.0))
                        .text_color(dim)
                        .max_w(px(220.0))
                        .truncate()
                        .child(path),
                )
            })
            .child(div().w(px(140.0)).child(Input::new(&self.lib_form.name)))
            .child(div().w(px(90.0)).child(Input::new(&self.lib_form.version)))
            .child(
                div()
                    .w(px(220.0))
                    .flex_1()
                    .min_w(px(140.0))
                    .child(Input::new(&self.lib_form.description)),
            )
            .child(
                Button::new("lib-add-save")
                    .label("Save")
                    .icon(Icon::new(Lucide::Save))
                    .small()
                    .primary()
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let Some(path) = this.lib_form.file.take() else {
                            return;
                        };
                        let name = this.lib_form.name.read(cx).value().trim().to_string();
                        let version = this.lib_form.version.read(cx).value().trim().to_string();
                        let description = this
                            .lib_form
                            .description
                            .read(cx)
                            .value()
                            .trim()
                            .to_string();
                        this.library.import_file(
                            path.to_string_lossy().as_ref(),
                            if name.is_empty() { "imported" } else { &name },
                            &version,
                            &description,
                        );
                        for field in [
                            &this.lib_form.name,
                            &this.lib_form.version,
                            &this.lib_form.description,
                        ] {
                            field.update(cx, |state, cx| state.set_value("", window, cx));
                        }
                        this.refresh_firmware_combos(window, cx);
                        cx.notify();
                    })),
            )
    }
}
