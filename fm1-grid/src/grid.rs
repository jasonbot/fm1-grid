//! The Emulator tab's left split: the 2×2 grid of display-only pane cards centred on
//! the computed background — the one colour in the app that does not come from the
//! theme: the dominant colour of the selected pane's screen, resampled once a second
//! and eased towards over 500 ms (see `GridApp::ease_background`).

use crate::app::GridApp;
use crate::lcd;
use gpui_kit::base::{ElementExt, h_flex, v_flex};
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{TestSupportExt as _, *};

/// Two 262 px cards plus one 3 px gap.
pub(crate) const GRID_W: f32 = 527.0;
/// The grid's 0.5x footprint: two 142 px cards plus one 3 px gap. The split panel's
/// minimum, so the cards always stay whole.
pub(crate) const GRID_MIN_W: f32 = 287.0;
/// The grid area's maximum width: side padding never passes ~1½ screen widths.
const GRID_MAX_W: f32 = GRID_W + 720.0;
/// The status dot's diameter and its inset from the screen's corner.
const DOT_D: f32 = 20.0;
const DOT_INSET: f32 = 5.0;

impl GridApp {
    /// The 2x2 grid on the computed background: content-sized and centred, with the
    /// side padding capped at ~1½ screen widths (past that, the sidebar's split takes
    /// the window's extra width). The rendered size is recorded during prepaint; the
    /// tick snaps the cards between 1x and 0.5x to fit it.
    pub(crate) fn grid_area(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = Rgba {
            r: self.bg[0],
            g: self.bg[1],
            b: self.bg[2],
            a: 1.0,
        };
        let app = cx.entity().clone();
        div()
            .id("grid-area")
            .size_full()
            .bg(bg)
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .on_prepaint(move |bounds, _, cx| {
                let size = (bounds.size.width.into(), bounds.size.height.into());
                app.update(cx, |this, _| {
                    if this.grid_size != size {
                        this.grid_size = size;
                    }
                });
            })
            .child(
                div()
                    .w_full()
                    .max_w(px(GRID_MAX_W))
                    .flex()
                    .justify_center()
                    .child(self.grid_cards(cx)),
            )
    }

    /// The 2x2 card grid. Cards are fixed-size at the current scale; screens are
    /// exactly 1x or 0.5x the 240 px LCD, never a fractional size.
    fn grid_cards(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut cards = Vec::with_capacity(fm1_performer::MAX_SLOTS);
        for slot in 0..fm1_performer::MAX_SLOTS {
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
    /// Screens snap between 1x (240 px) and 0.5x (120 px) to fit the split panel. A
    /// status dot in the screen's corner shows the slot's dropout health. Clicking
    /// routes the computer keyboard to that pane. The selection ring is a
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
        let side = fm1_performer::emulator::LCD_W as f32 * self.grid_scale;
        // The health dot: the five-second dropout window's colour; hidden once the
        // slot has been green for a second (see `SlotHealth::track`).
        let dot = if self.health[slot].shown {
            let theme = cx.theme();
            Some(match self.health[slot].color {
                crate::app::Health::Green => theme.success,
                crate::app::Health::Yellow => theme.warning,
                crate::app::Health::Orange => crate::theme::halfway(theme.warning, theme.danger),
                crate::app::Health::Red => theme.danger,
            })
        } else {
            None
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
                    // The bezel hugs the screen exactly: 1:1 or 1:2, never fractional.
                    div()
                        .rounded(px(6.0))
                        .bg(rgb(lcd::BEZEL))
                        .p(px(3.0))
                        .child(
                            div()
                                .relative()
                                .child(match picture {
                                    Some(image) => img(image)
                                        .w(px(side))
                                        .h(px(side))
                                        .into_any_element(),
                                    None => div()
                                        .w(px(side))
                                        .h(px(side))
                                        .bg(rgb(0x000000))
                                        .into_any_element(),
                                })
                                .when_some(dot, |el, color| {
                                    el.child(
                                        div()
                                            .absolute()
                                            .bottom(px(DOT_INSET))
                                            .right(px(DOT_INSET))
                                            .w(px(DOT_D))
                                            .h(px(DOT_D))
                                            .rounded_full()
                                            .border_1()
                                            .border_color(rgb(0x000000))
                                            .bg(color),
                                    )
                                }),
                        ),
                ),
            )
            .into_any_element()
    }
}
