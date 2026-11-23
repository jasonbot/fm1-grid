//! The Cardputers section of the Devices tab: flash the embedded fm1-card release
//! to a Cardputer in ROM download mode, and inspect the packages on the Cardputers
//! fm1-card finds on the LAN.

use crate::app::GridApp;
use crate::cardputers::{FlashableDevice, OnlineDevice};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::accordion::Accordion;
use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{TestSupportExt as _, *};

const PANEL_MAX_H: f32 = 180.0;

impl GridApp {
    pub(crate) fn cardputers_tab(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (background, ink, dim, accent, border, surface, danger) = {
            let theme = cx.theme();
            (
                theme.background,
                theme.foreground,
                theme.muted_foreground,
                theme.primary,
                theme.border,
                theme.muted,
                theme.danger,
            )
        };
        let intro = v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_size(px(13.5))
                    .text_color(ink)
                    .child(
                        "This tab works with the M5Stack Cardputer-Adv, a tiny handheld \
                         computer that runs fm1-card. Plug one in while it is in flash \
                         mode and you can put the fm1-card firmware on it from here. If \
                         one is already running fm1-card on your Wi-Fi, it shows up below \
                         and you can copy firmware packages to it from the Library tab.",
                    ),
            )
            .child(
                h_flex()
                    .id("cardputers-repo")
                    .test_support()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(accent)
                    .on_click(|_, _, cx| cx.open_url(fm1_cardputer::REPOSITORY_URL))
                    .child(Icon::new(Lucide::Github).size_3())
                    .child("github.com/jasonbot/fm1-card"),
            );

        if !self.cardputers_enabled {
            return v_flex()
                .id("cardputers-scroll")
                .test_support()
                .size_full()
                .bg(background)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .p_4()
                        .child(
                            v_flex()
                                .w_full()
                                .max_w(px(880.0))
                                .gap_5()
                                .child(intro)
                                .child(
                                    Button::new("cardputers-enable")
                                        .label("Enable Cardputers")
                                        .icon(Icon::new(Lucide::Keyboard))
                                        .large()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.set_cardputers_enabled(true, cx);
                                        })),
                                ),
                        ),
                );
        }

        let serial = self.cardputers.serial();
        let online = self.cardputers.online();

        let mut flashable = v_flex()
            .id("cardputers-flashable")
            .test_support()
            .w_full()
            .gap_2()
            .child(section_title("FLASHABLE DEVICES", dim));
        if serial.is_empty() {
            flashable = flashable.child(hint(
                "No Cardputer is in flash mode. Power it off, hold G0, plug in USB-C, \
                 then let go of G0.",
                dim,
            ));
        } else {
            for (ix, device) in serial.into_iter().enumerate() {
                flashable = flashable.child(
                    self.flashable_row(ix, device, border, surface, ink, dim, accent, cx),
                );
            }
        }

        let hosts: Vec<String> = online.iter().map(|device| device.host.clone()).collect();
        let mut accordion = Accordion::new("cardputers-online").multiple(true);
        for (ix, device) in online.iter().cloned().enumerate() {
            let open = self.cardputer_open.contains(&device.host);
            let busy = self.cardputers.busy(&device.host);
            let title = h_flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ink)
                        .child(device.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(dim)
                        .child(device.host.clone()),
                )
                .when(busy, |this| {
                    this.child(div().text_size(px(10.0)).text_color(accent).child("copying…"))
                });
            let panel = self.cardputer_panel(ix, &device, border, surface, ink, dim, danger);
            accordion = accordion.item(move |item| item.open(open).title(title).child(panel));
        }
        let toggle_app = cx.entity().clone();
        let hosts_for_toggle = hosts.clone();
        accordion = accordion.on_toggle_click(move |open, _, cx| {
            let selected: std::collections::HashSet<String> = open
                .iter()
                .filter_map(|&ix| hosts_for_toggle.get(ix).cloned())
                .collect();
            toggle_app.update(cx, |this, cx| {
                this.cardputer_open = selected;
                cx.notify();
            });
        });

        let online_body = if hosts.is_empty() {
            hint(
                "No Cardputer running fm1-card answered on the network. Check that it \
                 is on the same Wi-Fi and that its screen shows an IP address.",
                dim,
            )
        } else {
            accordion.into_any_element()
        };
        let online_section = v_flex()
            .id("cardputers-online")
            .test_support()
            .w_full()
            .gap_2()
            .child(section_title("ONLINE DEVICES", dim))
            .child(online_body);

        v_flex()
            .id("cardputers-scroll")
            .test_support()
            .size_full()
            .bg(background)
            .p_4()
            .gap_4()
            .overflow_hidden()
            .child(div().w_full().max_w(px(880.0)).child(intro))
            .child(div().w_full().max_w(px(880.0)).child(flashable))
            .child(div().w_full().max_w(px(880.0)).child(online_section))
    }

    fn flashable_row(
        &mut self,
        ix: usize,
        device: FlashableDevice,
        border: Hsla,
        surface: Hsla,
        ink: Hsla,
        dim: Hsla,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let busy = self.cardputers.busy(&device.path);
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_3()
            .p_2()
            .rounded(px(6.0))
            .border_1()
            .border_color(border)
            .bg(surface)
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ink)
                            .child(device.description.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(dim)
                            .child(format!(
                                "{} · fm1-card {}",
                                device.path,
                                fm1_cardputer::FIRMWARE_VERSION
                            )),
                    ),
            )
            .child(if busy {
                div().text_size(px(11.0)).text_color(accent).child("Flashing…").into_any_element()
            } else {
                Button::new(("cardputer-flash", ix))
                    .label("Flash")
                    .icon(Icon::new(Lucide::Zap))
                    .small()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.cardputers.flash(
                            device.path.clone(),
                            device.description.clone(),
                        ) {
                            return;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }

    fn cardputer_panel(
        &self,
        ix: usize,
        device: &OnlineDevice,
        border: Hsla,
        surface: Hsla,
        ink: Hsla,
        dim: Hsla,
        danger: Hsla,
    ) -> AnyElement {
        let mut body = v_flex().w_full().gap_2();
        if let Some(status) = &device.status {
            body = body.child(
                div()
                    .text_size(px(10.0))
                    .text_color(dim)
                    .child(format!(
                        "fm1-card {} · Wi-Fi {} · SD {}",
                        status.version,
                        if status.wifi.is_empty() {
                            "unknown"
                        } else {
                            status.wifi.as_str()
                        },
                        if status.sd { "ready" } else { "missing" }
                    )),
            );
        }
        if let Some(error) = &device.error {
            body = body.child(
                div()
                    .text_size(px(10.0))
                    .text_color(danger)
                    .child(error.clone()),
            );
        }
        body = body.child(
            div()
                .text_size(px(10.0))
                .font_weight(FontWeight::BOLD)
                .text_color(dim)
                .child(format!("INSTALLED FIRMWARE ({})", device.firmwares.len())),
        );
        let mut list = v_flex().w_full().gap_1();
        if device.firmwares.is_empty() {
            list = list.child(
                div()
                    .text_size(px(10.5))
                    .text_color(dim)
                    .child("No firmware packages on this card yet."),
            );
        }
        for firmware in &device.firmwares {
            let size = firmware
                .size
                .map(|size| format!("{:.0} KB", size as f64 / 1024.0))
                .unwrap_or_default();
            let hash = firmware
                .sha256
                .as_deref()
                .map(|hash| &hash[..hash.len().min(12)])
                .unwrap_or("");
            let detail = [size.as_str(), hash]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            list = list.child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(ink)
                            .child(firmware.name.clone()),
                    )
                    .child(div().text_size(px(9.5)).text_color(dim).child(detail)),
            );
        }
        body.child(
            div()
                .id(("cardputer-firmwares", ix))
                .test_support()
                .w_full()
                .max_h(px(PANEL_MAX_H))
                .overflow_y_scroll()
                .p_2()
                .pr(px(14.0))
                .rounded(px(5.0))
                .border_1()
                .border_color(border)
                .bg(surface)
                .child(list),
        )
        .into_any_element()
    }
}

fn section_title(text: &str, dim: Hsla) -> AnyElement {
    div()
        .text_size(px(10.5))
        .font_weight(FontWeight::BOLD)
        .text_color(dim)
        .child(text.to_string())
        .into_any_element()
}

fn hint(text: &str, dim: Hsla) -> AnyElement {
    div()
        .text_size(px(11.0))
        .text_color(dim)
        .child(text.to_string())
        .into_any_element()
}
