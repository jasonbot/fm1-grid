use super::*;
use fm1_performer::hardware::device_type;

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
        let package_model = match firmware.device {
            FirmwareDevice::Fm1 => Some("FM-1"),
            FirmwareDevice::Smk37 => firmware.variant(),
        };
        let units: Vec<(String, Option<&'static str>)> = self
            .hardware
            .devices()
            .iter()
            .filter(|row| {
                row.flash.is_none()
                    && package_model.is_none_or(|model| {
                        row.identity.as_deref().is_some_and(|identity| {
                            fm1_performer::hardware::package_fits(
                                identity,
                                &format!("{model}_0"),
                            )
                        })
                    })
            })
            .map(|row| (row.name.clone(), row.identity.as_deref().and_then(device_type)))
            .collect();
        let slots: Vec<usize> = (0..fm1_performer::MAX_SLOTS)
            .filter(|&slot| !self.powered[slot] && !self.slots[slot].pinned)
            .collect();
        let smk37 = firmware.device == FirmwareDevice::Smk37;
        // SMK-37 builds run on the real keyboard only; emulator panes are FM-1.
        let has_targets = if smk37 {
            !units.is_empty()
        } else {
            !units.is_empty() || !slots.is_empty()
        };
        let app = cx.entity().clone();
        // The dropdown closure is 'static; the firmware name moves in owned.
        let flash_name = firmware.name.clone();
        Button::new(format!("lib-flash-{name}"))
            .label("Flash To…")
            .icon(Icon::new(Lucide::Zap))
            .small()
            .ghost()
            .rounded_none()
            .disabled(!flashable || !has_targets)
            .when(smk37 && flashable && units.is_empty(), |button| {
                button.tooltip(
                    "SMK-37 firmware needs a live unit on USB; emulator panes only run \
                     FM-1 firmware",
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
                    let name = flash_name.clone();
                    let label = kind.map_or_else(
                        || format!("USB: {unit}"),
                        |kind| format!("USB: {unit} ({kind})"),
                    );
                    menu = menu.item(PopupMenuItem::new(label).on_click(
                        move |_, window, cx| {
                            let unit = unit.clone();
                            let pick = pick.clone();
                            let name = name.clone();
                            app.update(cx, |this, cx| {
                                this.flash_to_unit(unit, &name, pick, window, cx)
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
                menu
            })
            .into_any_element()
    }
    /// Flash a library firmware to a live FM-1 unit: the blocking dialog shows the
    /// flash board's progress and message log.
    pub(crate) fn flash_to_unit(
        &mut self,
        unit: String,
        name: &str,
        pick: FirmwarePick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match pick.bytes(self.store.as_ref()) {
            Ok(bytes) => {
                self.hardware.flash(&unit, bytes);
                crate::flash_dialog::FlashDialog::open(cx.entity(), window, cx, name);
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
}
