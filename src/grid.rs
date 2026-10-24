//! The central area: the 2×2 grid of display-only pane cards on the left with the
//! devices pane for the live USB units under it, the computed background colour, and
//! the hand-off to the device sidebar on the right. The background is the one colour
//! in the app that does not come from the theme: the dominant colour of the selected
//! pane's screen, resampled once a second and eased towards over 500 ms (see
//! `GridApp::ease_background`).

use crate::app::GridApp;
use crate::lcd;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme;
use gpui_kit::{TestSupportExt as _, *};

impl GridApp {
    /// The grid column plus the device sidebar, sitting on the computed background.
    pub(crate) fn central(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let bg = Rgba {
            r: self.bg[0],
            g: self.bg[1],
            b: self.bg[2],
            a: 1.0,
        };
        h_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .bg(bg)
            .p_2()
            .gap_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap_2()
                    // The grid is content-sized and centred in the space the devices
                    // pane leaves; spare pixels stay background.
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(self.grid_cards(cx)),
                    )
                    .child(self.devices_pane(window, cx)),
            )
            .child(self.device_sidebar(window, cx))
    }

    /// The 2x2 card grid. Cards and screens are fixed-size — the LCD picture is always
    /// 240x240, never scaled.
    fn grid_cards(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut cards = Vec::with_capacity(crate::MAX_SLOTS);
        for slot in 0..crate::MAX_SLOTS {
            cards.push(self.slot_card(slot, cx));
        }
        let mut rows = Vec::with_capacity(2);
        for _row in 0..2 {
            rows.push(
                h_flex()
                    .gap(px(3.0))
                    .children(cards.drain(..2))
                    .into_any_element(),
            );
        }
        v_flex().gap(px(3.0)).children(rows)
    }

    /// One pane card: the pane's picture (the NTSC-distorted realtime snapshot for the
    /// selected pane, the receding live picture for the rest) on the dark LCD bezel.
    /// Clicking routes the computer keyboard to that pane. The selection ring is a
    /// constant-width border — selecting never changes the card's size, so nothing
    /// jumps.
    fn slot_card(&mut self, slot: usize, cx: &mut Context<Self>) -> AnyElement {
        let active = self.active == slot;
        let (card_bg, ring) = {
            let theme = cx.theme();
            (theme.popover, theme.primary)
        };
        let picture = if active {
            Some(self.ghost.clone())
        } else {
            self.frames[slot].clone()
        };

        div()
            .id(("slot-card", slot))
            .test_support()
            .rounded(px(10.0))
            .bg(card_bg)
            .border_2()
            .border_color(if active {
                ring
            } else {
                transparent_black()
            })
            .overflow_hidden()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.set_active(slot);
                    this.refocus(window, cx);
                    cx.notify();
                }),
            )
            .child(
                div().p(px(6.0)).child(
                    // The bezel hugs the screen exactly: 240x240, 1:1, never scaled.
                    div()
                        .rounded(px(6.0))
                        .bg(rgb(lcd::BEZEL))
                        .p(px(3.0))
                        .child(match picture {
                            Some(image) => img(image)
                                .w(px(crate::emulator::LCD_W as f32))
                                .h(px(crate::emulator::LCD_H as f32))
                                .into_any_element(),
                            None => div()
                                .w(px(crate::emulator::LCD_W as f32))
                                .h(px(crate::emulator::LCD_H as f32))
                                .bg(rgb(0x000000))
                                .into_any_element(),
                        }),
                ),
            )
            .into_any_element()
    }
}
