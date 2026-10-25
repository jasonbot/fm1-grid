//! The Library tab: every firmware the app knows, as a grouped scrollable list —
//! installed on top (builtins, imported files, downloaded hub packages), the hub's
//! online offers below. Items remove, download and flash (a live unit via the blocking
//! dialog in [`crate::flash_dialog`], or a powered-off unpinned grid slot, which then
//! powers on). At the bottom, the add-a-file form.

use crate::app::GridApp;
use crate::firmware_picker::{FirmwarePick, pick_of};
use crate::library::{Firmware, FirmwareDevice, FirmwareSource, Status};
use gpui_kit::base::{ElementExt, h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// Tag pill colors, picked by the tag's hash: the same tag always lands on the same
/// color. Saturated mid-tones that read on the light theme.
const TAG_COLORS: [u32; 8] = [
    0x3584e4, // blue
    0x2ec27e, // green
    0xc64600, // orange
    0x9141ac, // purple
    0xe01b24, // red
    0x9f6e03, // ochre
    0x0e7a86, // teal
    0x5e5c64, // slate
];

/// FNV-1a of the tag, into the palette.
fn tag_color(tag: &str) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in tag.bytes() {
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
    }
    TAG_COLORS[(hash % TAG_COLORS.len() as u32) as usize]
}

/// The row's actions as one button group: ghost buttons fused in a bordered rounded
/// container with hairline dividers. (The component's ButtonGroup would do the fusion,
/// but it overwrites each button's own `disabled` at push time, and this row's buttons
/// gate independently.)
fn action_group(items: Vec<AnyElement>, border: Hsla) -> AnyElement {
    let mut group = h_flex()
        .border_1()
        .border_color(border)
        .rounded_md()
        .overflow_hidden();
    let mut first = true;
    for item in items {
        if !first {
            group = group.child(div().w(px(1.0)).h_full().bg(border));
        }
        first = false;
        group = group.child(item);
    }
    group.into_any_element()
}

/// The list's sections, in order: installed first, hub offers at the bottom.
const SECTIONS: [(&str, Section); 4] = [
    ("INSTALLED", Section::Installed),
    ("DOWNLOADING", Section::Downloading),
    ("MISSING", Section::Missing),
    ("AVAILABLE ON THE HUB", Section::Hub),
];

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Installed,
    Downloading,
    Missing,
    Hub,
}

fn section_of(firmware: &Firmware) -> Section {
    if firmware.status == Status::Downloading {
        return Section::Downloading;
    }
    match &firmware.source {
        FirmwareSource::Hub { blob: None, .. } => Section::Hub,
        _ if firmware.status == Status::Missing => Section::Missing,
        _ => Section::Installed,
    }
}

impl GridApp {
    /// The Library tab's content: the grouped list, then the add-file form.
    pub(crate) fn library_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (dim, border) = {
            let theme = cx.theme();
            (theme.muted_foreground, theme.border)
        };
        let items = self.library.list_clone();

        // The scrollbar overlays the list's right edge (16 px, gpui-base's WIDTH);
        // keep the content out from under it.
        let mut list = v_flex().w_full().gap_2().p_2().pr(px(16.0));
        for (header, section) in SECTIONS {
            let group: Vec<&Firmware> = items
                .iter()
                .filter(|f| section_of(f) == section)
                .collect();
            if group.is_empty() {
                continue;
            }
            list = list.child(
                v_flex().w_full().gap_1().child(
                    div()
                        .pt_2()
                        .pb_1()
                        .text_size(px(9.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(dim)
                        .child(header)
                        .border_b_1()
                        .border_color(border),
                ),
            );
            for firmware in group {
                list = list.child(self.library_item(firmware, cx));
            }
        }
        if items.is_empty() {
            list = list.child(
                div()
                    .text_size(px(11.0))
                    .text_color(dim)
                    .child("No firmwares? That can't be right."),
            );
        }

        // The hub's status sits at the very bottom of the list.
        list = list.child(self.hub_status_item(cx));

        v_flex()
            .size_full()
            .child(
                // The scrollbar pins to this outer element; the scrolling child is
                // inside it (stacking them on one element scrolled the bar away).
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .vertical_scrollbar(&self.lib_scroll)
                    .child(
                        div()
                            .id("library-list")
                            .test_support()
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.lib_scroll)
                            // PgUp/PgDn scroll by the viewport; record it.
                            .on_prepaint({
                                let app = cx.entity().clone();
                                move |bounds, _, cx| {
                                    let height: f32 = bounds.size.height.into();
                                    app.update(cx, |this, _| this.lib_view_h = height);
                                }
                            })
                            .child(list),
                    ),
            )
            .child(self.library_add_form(window, cx))
    }

    /// The list's last item: the hub's fetch status — a spinner while it loads, a
    /// reload button ("retry" after a failure).
    fn hub_status_item(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use crate::library::HubStatus;
        let (dim, danger) = {
            let theme = cx.theme();
            (theme.muted_foreground, theme.danger)
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
            .child(
                div()
                    .w(px(14.0))
                    .h(px(14.0))
                    .child(if loading {
                        Spinner::new().into_any_element()
                    } else {
                        div()
                            .size_full()
                            .rounded_full()
                            .bg(if failed { danger } else { dim })
                            .into_any_element()
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(10.0))
                    .text_color(if failed { danger } else { dim })
                    .child(text),
            )
            .child(
                Button::new("hub-reload")
                    .label(if failed { "retry" } else { "reload" })
                    .xsmall()
                    .disabled(loading)
                    .tooltip("Fetch the hub's catalog again")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.library.reload_hub();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// One firmware row: name + pills over byline + description, then the actions.
    fn library_item(&mut self, firmware: &Firmware, cx: &mut Context<Self>) -> AnyElement {
        let (ink, dim, border) = {
            let theme = cx.theme();
            (theme.foreground, theme.muted_foreground, theme.border)
        };
        let firmware = firmware.clone();
        let name = firmware.name.clone();
        let pick = pick_of(&firmware);
        let flashable = pick.is_some() && firmware.status == Status::Available;
        let downloading = firmware.status == Status::Downloading;
        let percent = self.library.download_percent(&firmware.name);

        // The pills: one per tag, then the device.
        let mut pills = h_flex().gap_1().flex_wrap();
        for tag in &firmware.tags {
            let color: Hsla = rgb(tag_color(tag)).into();
            pills = pills.child(
                div()
                    .rounded_full()
                    .px(px(6.0))
                    .py(px(1.0))
                    .text_size(px(9.0))
                    .bg(color.opacity(0.16))
                    .text_color(color)
                    .child(tag.clone()),
            );
        }
        let device = match firmware.device {
            FirmwareDevice::Fm1 => "FM-1",
            FirmwareDevice::Smk37Pro => "SMK-37 Pro",
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

        // The actions as one fused group: Remove (never builtins, and only when the
        // package is at hand), Download (hub offers; its percent while in flight),
        // Flash To…
        let locally = pick.is_some() && firmware.status == Status::Available;
        let mut buttons: Vec<AnyElement> = Vec::new();
        if !matches!(firmware.source, FirmwareSource::Builtin) {
            let name = name.clone();
            buttons.push(
                Button::new(format!("lib-remove-{name}"))
                    .label("Remove")
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
                        this.library.remove(&name);
                        this.refresh_firmware_combos(window, cx);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        if let FirmwareSource::Hub { blob: None, .. } = &firmware.source {
            if downloading {
                buttons.push(
                    Button::new(format!("lib-download-{name}"))
                        .label(format!("{}%", percent.unwrap_or(0)))
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
                        .small()
                        .ghost()
                        .rounded_none()
                        .tooltip("Download from the FM-1 Firmware Hub into the library")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.library.start_download(&name);
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

        v_flex()
            .w_full()
            .items_start()
            .gap_1()
            .py_1()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_baseline()
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
                            ),
                    )
                    .child(pills),
            )
            .when(!line.trim().is_empty(), |el| {
                el.child(
                    div()
                        .w_full()
                        .text_size(px(10.5))
                        .text_color(dim)
                        .child(line),
                )
            })
            .child(action_group(buttons, border))
            .into_any_element()
    }

    /// The Flash To… dropdown: live flashable units, then powered-off unpinned panes.
    fn flash_to_menu(
        &mut self,
        firmware: &Firmware,
        pick: Option<FirmwarePick>,
        flashable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = firmware.name.clone();
        let units: Vec<String> = self
            .hardware
            .devices()
            .iter()
            .filter(|row| row.flash.is_none())
            .map(|row| row.name.clone())
            .collect();
        let slots: Vec<usize> = (0..crate::MAX_SLOTS)
            .filter(|&slot| !self.powered[slot] && !self.slots[slot].pinned)
            .collect();
        let has_targets = !units.is_empty() || !slots.is_empty();
        let app = cx.entity().clone();
        Button::new(format!("lib-flash-{name}"))
            .label("Flash To…")
            .small()
            .ghost()
            .rounded_none()
            .disabled(!flashable || !has_targets)
            .when(!(flashable && has_targets), |button| {
                button.tooltip(
                    "Flashable needs the package at hand (builtin, on disk, or downloaded) \
                     and a target (a live FM-1 on USB, or a powered-off unpinned pane)",
                )
            })
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                let Some(pick) = pick.clone() else { return menu };
                for unit in &units {
                    let app = app.clone();
                    let unit = unit.clone();
                    let pick = pick.clone();
                    menu = menu.item(PopupMenuItem::new(format!("USB: {unit}")).on_click(
                        move |_, window, cx| {
                            let unit = unit.clone();
                            let pick = pick.clone();
                            app.update(cx, |this, cx| {
                                this.flash_to_unit(unit, pick, window, cx)
                            });
                        },
                    ));
                }
                for slot in &slots {
                    let app = app.clone();
                    let slot = *slot;
                    let pick = pick.clone();
                    menu = menu.item(PopupMenuItem::new(format!("Pane {}", slot + 1)).on_click(
                        move |_, window, cx| {
                            let pick = pick.clone();
                            app.update(cx, |this, cx| {
                                this.flash_to_slot(slot, pick, window, cx)
                            });
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }

    /// Flash a library firmware to a live FM-1 unit: the blocking dialog shows the
    /// flash board's progress and message log.
    pub(crate) fn flash_to_unit(
        &mut self,
        unit: String,
        pick: FirmwarePick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match pick.bytes(self.store.as_ref()) {
            Ok(bytes) => {
                self.hardware.flash(&unit, bytes);
                crate::flash_dialog::FlashDialog::open(cx.entity(), window, cx);
            }
            Err(e) => self.hardware.report(&unit, e),
        }
    }

    /// Flash a library firmware to a grid pane: install it and power the pane on.
    pub(crate) fn flash_to_slot(
        &mut self,
        slot: usize,
        pick: FirmwarePick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.install_pick(slot, pick);
        self.set_power(slot, true);
        self.refresh_firmware_combos(window, cx);
        self.active_tab = 0;
        cx.notify();
    }

    /// The add-file form at the bottom of the list.
    fn library_add_form(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                            this.lib_form.name.update(cx, |state, cx| {
                                state.set_value(stem, window, cx)
                            });
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
                    .small()
                    .primary()
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let Some(path) = this.lib_form.file.take() else { return };
                        let name = this.lib_form.name.read(cx).value().trim().to_string();
                        let version = this.lib_form.version.read(cx).value().trim().to_string();
                        let description =
                            this.lib_form.description.read(cx).value().trim().to_string();
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
