//! The transport bar at the bottom of the window: the miniature front panel and the
//! MIDI-clock metronome row. The tempo readout is a themed number input now (the egui
//! LCD-styled DragValue gave way to strict theme colours); its steppers and typed
//! edits both land in [`crate::clock::Clock::set_bpm`].

use crate::app::GridApp;
use crate::clock::{MAX_BPM, MIN_BPM};
use crate::routing::{Port, label_of};
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::NumberInput;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

impl GridApp {
    /// The transport sits in a flat theme-coloured bar at the bottom of the window.
    pub fn transport(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (bar_bg, border) = {
            let theme = cx.theme();
            (theme.background, theme.border)
        };
        v_flex()
            .w_full()
            .bg(bar_bg)
            .border_t_1()
            .border_color(border)
            .px_3()
            .py_2()
            .gap_2()
            .child(self.control_display(cx))
            .child(div().h(px(1.0)).w_full().bg(border))
            .child(self.metronome(window, cx))
    }

    /// The metronome row: clock switch, tempo readout, beat light, MIDI-keyboard
    /// selector; the range note and audio error sit on the right.
    fn metronome(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (surface, border, ink, dim, danger, accent, success) = {
            let theme = cx.theme();
            (
                theme.popover,
                theme.border,
                theme.foreground,
                theme.muted_foreground,
                theme.danger,
                theme.primary,
                theme.success,
            )
        };
        let mono = cx.theme().mono_font_family.clone();
        let running = self.clock.enabled();
        let lit = self.clock.beat_light();

        // The physical MIDI keyboard whose notes play the on-screen virtual keyboard;
        // the port list comes from the graph's poller.
        let selected = self.midi_keys.selected();
        let keyboards: Vec<(String, String)> = self
            .routing
            .endpoints()
            .into_iter()
            .filter_map(|endpoint| match endpoint.port {
                Port::System(name) if endpoint.source => Some((name, endpoint.label)),
                _ => None,
            })
            .collect();
        let current = selected.as_ref().map_or_else(
            || "no keyboard".to_string(),
            |name| {
                keyboards
                    .iter()
                    .find(|(n, _)| n == name)
                    .map_or_else(|| label_of(name), |(_, label)| label.clone())
            },
        );
        let app = cx.entity().clone();
        let keys_menu = Button::new("midi-keys-port")
            .label(current)
            .compact()
            .small()
            .tooltip(
                "A physical MIDI keyboard device whose notes play the virtual keyboard (F3..G5)",
            )
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.item(PopupMenuItem::new("no keyboard").on_click({
                    let app = app.clone();
                    move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            this.midi_keys.select(None);
                            this.focus.focus(window, cx);
                            cx.notify();
                        });
                    }
                }));
                for (name, label) in &keyboards {
                    let app = app.clone();
                    let name = name.clone();
                    let label = label.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let name = name.clone();
                        app.update(cx, |this, cx| {
                            this.midi_keys.select(Some(name));
                            this.focus.focus(window, cx);
                            cx.notify();
                        });
                    }));
                }
                menu
            });

        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .rounded(px(8.0))
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .px(px(10.0))
                    .py(px(6.0))
                    .child(
                        Switch::new("midi-clock")
                            .checked(running)
                            .tooltip("Start or stop the MIDI clock")
                            .on_change(cx.listener(|this, &on, window, cx| {
                                this.clock.set_enabled(on);
                                this.refocus(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        // Fixed width: RUNNING and STOPPED have the same number of
                        // characters in a monospace font, so the tempo display never
                        // shifts by a pixel when the clock starts.
                        v_flex().w(px(108.0)).child(
                            div()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(ink)
                                .child("MIDI CLOCK"),
                        )
                        .child(
                            div()
                                .text_size(px(9.0))
                                .font_family(mono.clone())
                                .text_color(if running { accent } else { dim })
                                .child(if running {
                                    "RUNNING · 24 PPQN"
                                } else {
                                    "STOPPED · 24 PPQN"
                                }),
                        ),
                    )
                    .child(
                        // Wide enough that the steppers and the BPM suffix cannot
                        // squeeze the digits out of the field.
                        div()
                            .w(px(140.0))
                            .child(NumberInput::new(&self.tempo_input).suffix(
                                div()
                                    .text_size(px(9.0))
                                    .text_color(dim)
                                    .child("BPM"),
                            )),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .w(px(18.0))
                                    .h(px(18.0))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(border)
                                    // The unlit lamp is a quiet disc in the border colour.
                                    .bg(if lit { success } else { border }),
                            )
                            .child(
                                div()
                                    .text_size(px(9.0))
                                    .text_color(if lit { accent } else { dim })
                                    .child("BEAT"),
                            ),
                    )
                    .child(
                        v_flex()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(ink)
                                    .child("MIDI KEYS"),
                            )
                            .child(keys_menu),
                    ),
            )
            .child(
                v_flex()
                    .items_end()
                    .gap_1()
                    .when_some(self.audio_error.clone(), |el, error| {
                        el.child(
                            div()
                                .text_size(px(9.0))
                                .text_color(danger)
                                .child(format!("audio off: {error}")),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(9.0))
                            .text_color(dim)
                            .child(format!("{MIN_BPM}–{MAX_BPM} BPM · MIDI 0xF8")),
                    ),
            )
    }
}
