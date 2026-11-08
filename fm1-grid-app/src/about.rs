//! The About tab: a short app identity/release history page and the generated
//! third-party licence text.

use crate::app::GridApp;
use gpui_kit::base::{VirtualListScrollHandle, h_flex, v_flex, v_virtual_list};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Icon, IconName};
use gpui_kit::*;
use std::io::Read;
use std::rc::Rc;
use std::sync::OnceLock;

const LICENSES_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/3rdparty-licenses.txt.gz"));

pub(crate) const LICENSE_LINE_H: f32 = 14.0;

pub(crate) fn license_lines() -> Vec<&'static str> {
    licenses_text().lines().collect()
}

fn licenses_text() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut text = String::new();
        match flate2::read::GzDecoder::new(LICENSES_GZ).read_to_string(&mut text) {
            Ok(_) => text,
            Err(error) => format!("Could not read the third-party licences: {error}"),
        }
    })
}

impl GridApp {
    fn load_about_licenses(&mut self) {
        if !self.about_lines.is_empty() {
            return;
        }
        let lines = Rc::new(license_lines());
        let sizes = Rc::new(
            lines
                .iter()
                .map(|_| size(px(0.0), px(LICENSE_LINE_H)))
                .collect(),
        );
        self.about_lines = lines;
        self.about_sizes = sizes;
    }

    /// The About tab with its two pill sections.
    pub(crate) fn about_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.about_tab == 1 {
            self.load_about_licenses();
        }
        let background = cx.theme().background;
        let mut tabs = h_flex().items_center().gap_1();
        for (ix, icon, label) in [
            (0usize, IconName::Info, "About this app"),
            (1, IconName::FileText, "Licenses"),
        ] {
            let selected = self.about_tab == ix;
            let app = cx.entity().clone();
            tabs = tabs.child(
                div()
                    .id(("about-section", ix))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px(px(12.0))
                    .py(px(4.0))
                    .rounded_full()
                    .cursor_pointer()
                    .bg(if selected {
                        cx.theme().primary
                    } else {
                        gpui_kit::transparent_black()
                    })
                    .text_color(if selected {
                        cx.theme().primary_foreground
                    } else {
                        cx.theme().tab_foreground
                    })
                    .on_click(move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            this.about_tab = ix;
                            this.refocus(window, cx);
                            cx.notify();
                        });
                    })
                    .child(Icon::new(icon).size_4())
                    .child(label),
            );
        }
        v_flex()
            .size_full()
            .bg(background)
            .child(
                h_flex()
                    .w_full()
                    .justify_center()
                    .p_2()
                    .pb_0()
                    .child(div().id("about-sections").test_support().child(tabs)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(match self.about_tab {
                        0 => about_app(cx).into_any_element(),
                        _ => licenses(
                            cx,
                            &self.about_scroll,
                            Rc::clone(&self.about_lines),
                            Rc::clone(&self.about_sizes),
                        )
                        .into_any_element(),
                    }),
            )
    }
}

fn about_app(cx: &mut Context<GridApp>) -> impl IntoElement {
    let (ink, dim) = {
        let theme = cx.theme();
        (theme.foreground, theme.muted_foreground)
    };
    v_flex()
        .w_full()
        .p_6()
        .gap_4()
        .child(
            div()
                .text_size(px(26.0))
                .font_weight(FontWeight::BOLD)
                .text_color(ink)
                .child(crate::APP_TITLE),
        )
        .child(
            div()
                .text_size(px(14.0))
                .text_color(dim)
                .child("Four FM-1 emulators in a 2×2 grid, built with gpui-kit."),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(dim)
                .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
        )
        .child(
            div()
                .pt_2()
                .text_size(px(16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ink)
                .child("Release history"),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(dim)
                .child(format!("{} — current build.", env!("CARGO_PKG_VERSION"))),
        )
}

fn licenses(
    cx: &mut Context<GridApp>,
    scroll: &VirtualListScrollHandle,
    lines: Rc<Vec<&'static str>>,
    sizes: Rc<Vec<Size<Pixels>>>,
) -> impl IntoElement {
    let (ink, mono) = {
        let theme = cx.theme();
        (theme.foreground, theme.mono_font_family.clone())
    };
    div()
        .id("about-licenses")
        .test_support()
        .size_full()
        .overflow_hidden()
        .vertical_scrollbar(scroll)
        .child(
            v_virtual_list(cx.entity().clone(), "about-license-lines", sizes, {
                let lines = Rc::clone(&lines);
                move |_, range, _, _| {
                    range
                        .map(|ix| {
                            div()
                                .w_full()
                                .h(px(LICENSE_LINE_H))
                                .truncate()
                                .text_size(px(10.0))
                                .font_family(mono.clone())
                                .text_color(ink)
                                .child(lines[ix])
                                .into_any_element()
                        })
                        .collect()
                }
            })
            .track_scroll(scroll)
            .p_4()
            .pr_6(),
        )
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_licenses_decompress() {
        let text = super::licenses_text();
        assert!(text.len() > 100_000);
        assert!(text.contains("THIRD-PARTY LICENSES"));
        assert!(text.contains("accesskit"));
    }
}
