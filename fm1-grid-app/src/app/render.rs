use super::*;

/// Space above and below the front panel.
const PANEL_PAD_Y: f32 = 2.0;

impl GridApp {
    /// The Emulator tab: the 2x2 grid and the selected pane's sidebar in a draggable
    /// split, with the miniature front panel tucked at the bottom — the only tab that
    /// shows it. The sidebar's range holds the screen at 1x to 6x; the grid panel's
    /// minimum keeps the 0.5x cards whole (the cards snap down when it shrinks).
    pub(super) fn emulator_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = window.viewport_size().height < px(PANEL_COMPACT_BELOW);
        v_flex()
            .size_full()
            .child(
                div().flex_1().min_h_0().w_full().child(
                    h_resizable("emulator-split")
                        .with_state(&self.emulator_split)
                        .child(
                            // gpui-base rescales panels with the container only when
                            // every panel has a size; an unsized grid panel would pin
                            // the sidebar to its initial width (and its screen would
                            // never reach 6x).
                            resizable_panel()
                                .size(px(1152.0 - self.sidebar_width))
                                .size_range(px(crate::grid::GRID_MIN_W)..Pixels::MAX)
                                .child(self.grid_area(cx)),
                        )
                        .child(
                            resizable_panel()
                                .size(px(self.sidebar_width))
                                .size_range(
                                    px(crate::theme::SIDEBAR_MIN)..px(crate::theme::SIDEBAR_MAX),
                                )
                                .child(self.device_sidebar(window, cx)),
                        ),
                ),
            )
            .child(
                div()
                    .w_full()
                    .pt(px(PANEL_PAD_Y))
                    .pb(px(PANEL_PAD_Y))
                    .child(self.control_display(cx, compact)),
            )
    }
    /// The left icon rail: the app tabs as icon-only buttons stacked vertically, the
    /// gear pinned to the bottom. One chip slides between the buttons on the same
    /// spring the pill tab bar's indicator uses; the buttons stay transparent —
    /// the selected one keeps the tab_active_foreground ink, the rest take a
    /// tab_active wash on hover. Icons run at size_5 (20px).
    pub(super) fn tab_rail(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (rail_bg, ink, dim, wash, border, chip) = {
            let theme = cx.theme();
            (
                theme.tab_bar,
                theme.tab_active_foreground,
                theme.tab_foreground,
                theme.tab_active,
                theme.border,
                theme.tokens.background,
            )
        };
        const TABS: [(Lucide, usize); 4] = [
            (Lucide::Monitor, 0),
            (Lucide::KeyboardMusic, 1),
            (Lucide::Library, 2),
            (Lucide::Settings, 3),
        ];

        // Slot tops: pt_2, then 36px buttons with gap_2; the gear pins to the
        // bottom (its slot needs the rail's height, recorded last frame).
        let slot_top = |ix: usize, rail_h: f32| {
            if ix == 3 {
                (rail_h - 8.0 - 36.0).max(8.0 + 3.0 * 44.0)
            } else {
                8.0 + ix as f32 * 44.0
            }
        };
        let chip_top = gpui_kit::base::spring(
            ("tab-rail", "chip-top"),
            px(slot_top(self.active_tab, self.rail_h)),
            cx.theme().motion_tokens().spring_move,
            window,
            cx,
        );

        let button = |icon: Lucide, ix: usize, cx: &mut Context<Self>| {
            let selected = ix == self.active_tab;
            div()
                .id(("tab", ix))
                .test_support()
                .size(px(36.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .cursor_pointer()
                .text_color(if selected { ink } else { dim })
                .when(!selected, |this| this.hover(|this| this.bg(wash)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this.active_tab == 0 && ix != 0 {
                        // The panel hides with the tab: lift computer- and
                        // mouse-held keys so nothing sticks while it's gone.
                        // Physical-MIDI notes survive — sync_held re-merges
                        // them into the wanted state.
                        this.kb_down = [false; 41];
                        this.panel_down = [false; 41];
                        this.sync_held();
                    }
                    this.active_tab = ix;
                    if ix == 2 && hub_online() {
                        // Opening the library refreshes the online listing.
                        this.library.refresh_hub();
                    }
                    this.refocus(window, cx);
                    cx.notify();
                }))
                .child(Icon::new(icon).size_5())
                .into_any_element()
        };

        let app = cx.entity().clone();
        div()
            .w(px(48.0))
            .h_full()
            .flex_shrink_0()
            .relative()
            .bg(rail_bg)
            .border_r_1()
            .border_color(border)
            .on_prepaint(move |bounds, _, cx| {
                let height: f32 = bounds.size.height.into();
                app.update(cx, |this, _| {
                    if (this.rail_h - height).abs() > 0.5 {
                        this.rail_h = height;
                    }
                });
            })
            // The sliding chip under the selected button.
            .child(
                div()
                    .absolute()
                    .left(px(6.0))
                    .top(chip_top)
                    .size(px(36.0))
                    .rounded(px(6.0))
                    .bg(chip)
                    .shadow_xs(),
            )
            .child(
                v_flex()
                    .size_full()
                    .pt_2()
                    .pb_2()
                    .items_center()
                    .gap_2()
                    .children(TABS[..3].iter().map(|&(icon, ix)| button(icon, ix, cx)))
                    .child(div().flex_1())
                    .child(button(TABS[3].0, 3, cx)),
            )
    }
    /// The Devices tab: a centered pill tab bar over the two sections — the live USB
    /// units and the MIDI graph. The picked section is session state.
    pub(super) fn devices_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().background;
        let app = cx.entity().clone();
        v_flex()
            .size_full()
            .bg(background)
            .child(
                h_flex().w_full().justify_center().p_2().pb_0().child(
                    // The shrink-wrap div carries the observable id: the bar's own
                    // root doesn't register one.
                    // A component Tab paints its icon OR its label, never both — the
                    // captions go through the children slot instead.
                    div()
                        .id("devices-sections")
                        .test_support()
                        .child(
                            TabBar::new("devices-sections-bar")
                                .pill()
                                .selected_index(self.devices_tab)
                                .on_click(move |ix, window, cx| {
                                    app.update(cx, |this, cx| {
                                        this.devices_tab = *ix;
                                        this.refocus(window, cx);
                                        cx.notify();
                                    });
                                })
                                .child(
                                    Tab::new().aria_label("Devices").child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .child(Icon::new(Lucide::Usb).size_4())
                                            .child("Devices"),
                                    ),
                                )
                                .child(
                                    Tab::new().aria_label("MIDI Graph").child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .child(Icon::new(Lucide::Waypoints).size_4())
                                            .child("MIDI Graph"),
                                    ),
                                ),
                        ),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(match self.devices_tab {
                        0 => self.devices_pane(window, cx).into_any_element(),
                        _ => self.graph_section(cx).into_any_element(),
                    }),
            )
    }
}

impl Render for GridApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (background, dim) = {
            let theme = cx.theme();
            (theme.background, theme.muted_foreground)
        };
        // The high-contrast family asks for a bold UI; Japanese Stationery's
        // Linux fallback (Noto Sans CJK JP) reads too thin at the normal weight.
        let weight = if self.theme_id == crate::theme::VERY_HIGH_CONTRAST {
            FontWeight::BOLD
        } else if cfg!(target_os = "linux") && self.theme_id == "japanese-stationery" {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        div()
            .size_full()
            .font_weight(weight)
            .flex()
            .flex_col()
            .bg(background)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            // The in-app title bar: draws the window controls where the compositor
            // doesn't supply decorations (GNOME), and owns window dragging.
            .child(
                TitleBar::new().child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(dim)
                        .child(crate::APP_TITLE),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.tab_rail(window, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(match self.active_tab {
                                0 => self.emulator_tab(window, cx).into_any_element(),
                                1 => self.devices_tab(window, cx).into_any_element(),
                                2 => self.library_tab(window, cx).into_any_element(),
                                _ => self.settings_tab(window, cx).into_any_element(),
                            }),
                    ),
            )
            .child(self.transport(window, cx))
    }
}
