//! The Emulator tab's left split: the 2×2 grid of display-only pane cards centred on
//! the computed background — the one colour in the app that does not come from the
//! theme: the dominant colour of the selected pane's screen, resampled once a second
//! and eased towards over 500 ms (see `GridApp::ease_background`).

use crate::app::GridApp;
use crate::lcd;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme;
use gpui_kit::{TestSupportExt as _, *};

/// Two 262 px cards plus one 3 px gap.
pub(crate) const GRID_W: f32 = 527.0;
/// The grid area's maximum width: side padding never passes ~1½ screen widths.
const GRID_MAX_W: f32 = GRID_W + 720.0;

impl GridApp {
    /// The 2x2 grid on the computed background: content-sized and centred, with the
    /// side padding capped at ~1½ screen widths (past that, the sidebar's split takes
    /// the window's extra width).
    pub(crate) fn grid_area(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = Rgba {
            r: self.bg[0],
            g: self.bg[1],
            b: self.bg[2],
            a: 1.0,
        };
        div()
            .size_full()
            .bg(bg)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(GRID_MAX_W))
                    .flex()
                    .justify_center()
                    .child(self.grid_cards(cx)),
            )
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
