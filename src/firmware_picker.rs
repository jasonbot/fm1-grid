//! The searchable firmware dropdown the sidebar's firmware selector and the devices
//! pane's flash selector share: a combo whose popup starts with a filter box that
//! narrows the grouped builtin list as you type.

use eframe::egui::{self, Id, Response, RichText, TextEdit, Ui};

use crate::builtin::{self, BUILTINS};
use crate::theme::DIM;

/// What a picker click produced.
pub(crate) enum Pick {
    /// An embedded firmware, by index into `BUILTINS`.
    Builtin(usize),
    /// The "Other…" file dialog.
    File,
}

/// Does a firmware match the filter? An empty filter matches everything; otherwise the
/// needle must appear in the name, the version or the tags.
fn matches(needle: &str, index: usize) -> bool {
    if needle.is_empty() {
        return true;
    }
    let firmware = &BUILTINS[index];
    firmware.name.to_lowercase().contains(needle)
        || firmware.version.to_lowercase().contains(needle)
        || firmware.tags.to_lowercase().contains(needle)
}

/// Shows the firmware combo and returns its response plus what was picked this frame.
/// `id_salt` must be unique per picker. The filter resets and is focused every time the
/// popup opens, so you can type right away; Escape or a click outside closes it.
pub(crate) fn show(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash,
    selected_text: RichText,
    width: f32,
    selected_builtin: Option<usize>,
    selected_file: bool,
) -> (Response, Option<Pick>) {
    let ctx = ui.ctx().clone();
    let open_key = Id::new(("firmware-picker-open", &id_salt));
    let filter_key = Id::new(("firmware-picker-filter", &id_salt));
    let was_open = ctx.data(|data| data.get_temp::<bool>(open_key).unwrap_or(false));

    let mut pick = None;
    let mut ran = false;
    let combo = egui::ComboBox::from_id_salt(("firmware-picker", &id_salt))
        .selected_text(selected_text)
        .width(width)
        .show_ui(ui, |ui| {
            ran = true;
            let just_opened = !was_open;
            if just_opened {
                ctx.data_mut(|data| data.insert_temp(filter_key, String::new()));
            }
            let mut filter = ctx
                .data(|data| data.get_temp::<String>(filter_key))
                .unwrap_or_default();
            let response = ui.add(
                TextEdit::singleline(&mut filter)
                    .hint_text("Filter…")
                    .desired_width(width),
            );
            if just_opened {
                response.request_focus();
            }
            let needle = filter.trim().to_lowercase();
            ctx.data_mut(|data| {
                data.insert_temp(filter_key, filter);
                data.insert_temp(open_key, true);
            });

            // Grouped by the hub's categories, each group under its name; groups with
            // no match disappear while filtering.
            let mut any = false;
            for (category, indices) in builtin::groups() {
                let hits: Vec<usize> = indices
                    .into_iter()
                    .filter(|&index| matches(&needle, index))
                    .collect();
                if hits.is_empty() {
                    continue;
                }
                builtin::group_header(ui, category, !any);
                any = true;
                for index in hits {
                    let firmware = &BUILTINS[index];
                    let tip = format!("{}\nSHA-256 {}", firmware.file, firmware.sha256);
                    if ui
                        .selectable_label(selected_builtin == Some(index), firmware.label())
                        .on_hover_text(tip)
                        .clicked()
                    {
                        pick = Some(Pick::Builtin(index));
                        ui.close();
                    }
                }
            }
            if !any {
                ui.label(RichText::new("No firmware matches").size(10.0).color(DIM));
            }
            // The file dialog is the escape hatch out of the builtin list, not a
            // firmware: it only shows while the filter is empty.
            if needle.is_empty() {
                ui.separator();
                if ui.selectable_label(selected_file, "Other…").clicked() {
                    pick = Some(Pick::File);
                    ui.close();
                }
            }
        });
    // The closure only runs while the popup is open; a frame without it means the
    // popup just closed, so the next open starts fresh.
    if !ran {
        ctx.data_mut(|data| data.insert_temp(open_key, false));
    }
    (combo.response, pick)
}
