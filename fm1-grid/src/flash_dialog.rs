//! The blocking flash dialog the library's Flash To… opens for a live unit: the
//! board's current phase and progress plus a scrollable terminal of every phase line
//! so far, following the tail. Reuses the devices pane's data (`Hardware::flash_state`);
//! the flash itself runs on the hardware module's thread.

use crate::app::GridApp;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::Button;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// Terminal rows visible at once — under ten lines, per the spec.
const TERMINAL_ROWS: f32 = 9.0;
const TERMINAL_ROW_H: f32 = 14.0;

pub(crate) struct FlashDialog {
    app: Entity<GridApp>,
    scroll: ScrollHandle,
    /// The board carried a flash at some point; it clears on success, which is how the
    /// dialog learns the flash finished.
    saw_flash: bool,
}

impl FlashDialog {
    /// Open the modal over the window, titled with the firmware's name. The dialog
    /// re-renders with the window's frames, so the live board state flows in on its
    /// own.
    pub(crate) fn open(app: Entity<GridApp>, window: &mut Window, cx: &mut App, name: &str) {
        let view = cx.new(|cx| {
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            Self {
                app,
                scroll: ScrollHandle::new(),
                saw_flash: false,
            }
        });
        let title: SharedString = format!("Flashing {name}…").into();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title(title.clone()).child(view.clone())
        });
    }
}

impl Render for FlashDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (ink, dim, danger, mono, surface) = {
            let theme = cx.theme();
            (
                theme.foreground,
                theme.muted_foreground,
                theme.danger,
                theme.mono_font_family.clone(),
                theme.muted,
            )
        };
        let board = self.app.read(cx).hardware.flash_state();
        if board.is_some() {
            self.saw_flash = true;
        }
        let running = board.as_ref().is_some_and(|b| !b.error);
        let done = self.saw_flash && board.is_none();

        // The terminal follows its tail while the flash runs.
        let lines = board.as_ref().map(|b| b.log.clone()).unwrap_or_default();
        if running {
            self.scroll.scroll_to_bottom();
        }

        v_flex()
            .w(px(420.0))
            .gap_2()
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(match &board {
                        Some(b) if b.error => danger,
                        _ => ink,
                    })
                    .child(match &board {
                        Some(b) => b.step.clone(),
                        None if done => "done — the unit is back.".into(),
                        None => "starting…".into(),
                    }),
            )
            .when_some(board.as_ref().and_then(|b| b.progress), |el, progress| {
                el.child(Progress::new("dialog-flash-progress").value(progress * 100.0))
            })
            .child(
                div()
                    .id("flash-terminal")
                    .w_full()
                    .h(px(TERMINAL_ROWS * TERMINAL_ROW_H))
                    .rounded(px(6.0))
                    .bg(surface)
                    .p_2()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        v_flex().w_full().children(lines.iter().map(|line| {
                            div()
                                .h(px(TERMINAL_ROW_H))
                                .text_size(px(10.0))
                                .font_family(mono.clone())
                                .text_color(dim)
                                .child(line.clone())
                        })),
                    ),
            )
            .child(
                h_flex().justify_end().child(
                    Button::new("flash-dialog-close")
                        .label("Close")
                        .icon(Icon::new(Lucide::X))
                        .small()
                        .disabled(running)
                        .on_click(cx.listener(|_, _, window, cx| {
                            window.close_dialog(cx);
                        })),
                ),
            )
    }
}
