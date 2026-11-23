use super::*;
use fm1_performer::builtin::BUILTINS;
use fm1_performer::hardware::device_type;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;

/// Whether a live unit can take this package. A plain/unknown unit never matches.
pub(super) fn unit_accepts(firmware: &Firmware, identity: Option<&str>) -> bool {
let package_model = match firmware.device {
    FirmwareDevice::Fm1 => Some("FM-1"),
    FirmwareDevice::Smk37 => firmware.variant(),
};
package_model.is_none_or(|model| {
    identity.is_some_and(|identity| {
        fm1_performer::hardware::package_fits(identity, &format!("{model}_0"))
    })
})
}

impl GridApp {

    /// The Flash To… dropdown: live flashable units, then powered-off unpinned panes.
    pub(super) fn flash_to_menu(
        &mut self,
        firmware: &Firmware,
        pick: Option<FirmwarePick>,
        flashable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = firmware.name.clone();
        let installed = firmware.clone();
        let units: Vec<(String, Option<&'static str>)> = self
            .hardware
            .devices()
            .iter()
            .filter(|row| row.flash.is_none() && unit_accepts(firmware, row.identity.as_deref()))
            .map(|row| (row.name.clone(), row.identity.as_deref().and_then(device_type)))
            .collect();
        let slots: Vec<usize> = (0..fm1_performer::MAX_SLOTS)
            .filter(|&slot| !self.powered[slot] && !self.slots[slot].pinned)
            .collect();
        let cardputers = if self.cardputers_enabled {
            self.cardputers.targets()
        } else {
            Vec::new()
        };
        let networked = !cardputers.is_empty();
        let smk37 = firmware.device == FirmwareDevice::Smk37;
        // SMK-37 builds run on the real keyboard only; emulator panes are FM-1.
        // Any Cardputer can store any package on its SD card.
        let has_targets = if smk37 {
            !units.is_empty() || networked
        } else {
            !units.is_empty() || !slots.is_empty() || networked
        };
        let app = cx.entity().clone();
        Button::new(format!("lib-flash-{name}"))
            .label("Flash To…")
            .icon(Icon::new(Lucide::Zap))
            .small()
            .ghost()
            .rounded_none()
            .disabled(!flashable || !has_targets)
            .when(smk37 && flashable && units.is_empty() && !networked, |button| {
                button.tooltip(
                    "SMK-37 firmware needs a live unit on USB or a Cardputer on the \
                     network; emulator panes only run FM-1 firmware",
                )
            })
            .when(!smk37 && !(flashable && has_targets), |button| {
                button.tooltip(
                    "Flashable needs the package at hand (builtin, on disk, or downloaded) \
                     and a target (a live FM-1 on USB, or a powered-off unpinned pane)",
                )
            })
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                let Some(pick) = pick.clone() else {
                    return menu;
                };
                for (unit, kind) in &units {
                    let app = app.clone();
                    let unit = unit.clone();
                    let kind = *kind;
                    let pick = pick.clone();
                    let firmware = installed.clone();
                    let label = kind.map_or_else(
                        || format!("USB: {unit}"),
                        |kind| format!("USB: {unit} ({kind})"),
                    );
                    menu = menu.item(PopupMenuItem::new(label).on_click(
                        move |_, window, cx| {
                            let unit = unit.clone();
                            let pick = pick.clone();
                            let firmware = firmware.clone();
                            app.update(cx, |this, cx| {
                                this.flash_to_unit(unit, &firmware, pick, window, cx)
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
                            app.update(cx, |this, cx| this.flash_to_slot(slot, pick, window, cx));
                        },
                    ));
                }
                for (cardputer, host) in &cardputers {
                    let app = app.clone();
                    let cardputer = cardputer.clone();
                    let host = host.clone();
                    let pick = pick.clone();
                    let firmware = installed.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format!("Cardputer: {cardputer} ({host})")).on_click(
                            move |_, window, cx| {
                                let host = host.clone();
                                let cardputer = cardputer.clone();
                                let pick = pick.clone();
                                let firmware = firmware.clone();
                                app.update(cx, |this, cx| {
                                    this.send_to_cardputer(
                                        host,
                                        cardputer,
                                        &firmware,
                                        pick,
                                        window,
                                        cx,
                                    )
                                });
                            },
                        ),
                    );
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
        firmware: &Firmware,
        pick: FirmwarePick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match pick.bytes(self.store.as_ref()) {
            Ok(bytes) => {
                self.flash_install.start(firmware.clone());
                self.hardware.flash(&unit, bytes);
                crate::flash_dialog::FlashDialog::open(
                    cx.entity(),
                    window,
                    cx,
                    &firmware.name,
                );
            }
            Err(e) => self.hardware.report(&unit, e),
        }
    }
    /// Copy a library firmware onto a running Cardputer's SD card. The sent
    /// package is named after its local file; the completion notification
    /// replaces the "sending" one.
    pub(crate) fn send_to_cardputer(
        &mut self,
        host: String,
        device: String,
        firmware: &Firmware,
        pick: FirmwarePick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let file = cardputer_file_name(firmware, &pick);
        let key = format!("{host}:{file}");
        let bytes = match pick.bytes(self.store.as_ref()) {
            Ok(bytes) => bytes,
            Err(error) => {
                window.push_notification(
                    Notification::error(format!(
                        "Could not copy {} to {}: {error}",
                        firmware.name, device
                    ))
                    .title("Cardputer firmware")
                    .id1::<crate::app::CardputerSendNotification>(key),
                    cx,
                );
                return;
            }
        };
        if !self
            .cardputers
            .send(host, device.clone(), firmware.name.clone(), file, bytes)
        {
            window.push_notification(
                Notification::warning(format!(
                    "Another copy to {device} is already running."
                ))
                .title("Cardputer firmware")
                .id1::<crate::app::CardputerSendNotification>(key),
                cx,
            );
            return;
        }
        window.push_notification(
            Notification::info(format!("Sending {} to {}…", firmware.name, device))
                .title("Cardputer firmware")
                .id1::<crate::app::CardputerSendNotification>(key),
            cx,
        );
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
}

/// The name the Cardputer stores a package under. Builtin and on-disk packages
/// keep their real file name; a blob key can carry a hash suffix, so a display
/// name stands in when it does not end in `.fwsc`.
fn cardputer_file_name(firmware: &Firmware, pick: &FirmwarePick) -> String {
    let candidate = match pick {
        FirmwarePick::Builtin(index) => Some(BUILTINS[*index].file.to_string()),
        FirmwarePick::File(path) => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        FirmwarePick::HubBlob(key) => key
            .rsplit('/')
            .next()
            .filter(|name| name.to_ascii_lowercase().ends_with(".fwsc"))
            .map(str::to_string),
    };
    candidate.unwrap_or_else(|| {
        fm1_cardputer::safe_file_name(&format!("{}-{}", firmware.name, firmware.version))
    })
}
