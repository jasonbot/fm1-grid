//! The drawing half of fm1-tty: the devices block, the library list, the piano, the
//! metronome bar and the popups. State lives in `app.rs`; this reads it and refills
//! the mouse hit regions (`app.regions`) for the next event pass.

use crate::app::{Focus, Popup, Regions, TuiApp};
use fm1_performer::clock::{MAX_BPM, MIN_BPM};
use fm1_performer::hardware::{DeviceRow, FlashState};
use fm1_performer::keys::{panel_key_hint, BLACK_KEYS, PANEL_KEYS, WHITE_KEYS};
use fm1_performer::library::{Firmware, FirmwareDevice, FirmwareSource, HubStatus, Status};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Gauge, List, ListItem, Paragraph};
use ratatui::Frame;

const ACCENT: Color = Color::Cyan;
const OK: Color = Color::Green;
const ERR: Color = Color::Red;
const DIM: Color = Color::DarkGray;
/// The black keys' fill: plain black disappears on a dark terminal.
const KEY_BLACK: Color = Color::DarkGray;

/// Display width for the ASCII-ish strings drawn here (the symbols used are all one
/// column wide).
fn cols(text: &str) -> usize {
    text.chars().count()
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(DIM))
}

fn bold(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().add_modifier(Modifier::BOLD))
}

/// The frame: every section, then whichever modal is up.
pub fn draw(frame: &mut Frame, app: &mut TuiApp) {
    let area = frame.area();
    let device_rows =
        app.devices.len() + if app.rescue { app.rescue_ports.len() } else { 0 };
    let devices_h = (device_rows as u16 + 2).clamp(3, 6);
    let chunks = Layout::vertical([
        Constraint::Length(devices_h),
        Constraint::Min(6),
        Constraint::Length(9),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    app.regions = Regions::default();
    devices(frame, app, chunks[0]);
    library(frame, app, chunks[1]);
    keyboard(frame, app, chunks[2]);
    metronome(frame, app, chunks[3]);
    footer(frame, app, chunks[4]);

    match app.popup.clone() {
        Some(Popup::MidiPorts(sel)) => midi_popup(frame, app, sel),
        Some(Popup::FlashTarget(sel, names, _)) => flash_target_popup(frame, app, sel, &names),
        None => {}
    }
    if let Some(board) = app.flash_board() {
        flash_modal(frame, &board);
    }
}

/// The live FM-1 units on USB, one row each. The rescue toggle's state rides the
/// title: RESCUE ON while armed, a warning nudge when no FM-1 answered but MIDI
/// ports exist.
fn devices(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let mut title = vec![Span::raw(" DEVICES ")];
    if app.rescue {
        title.push(Span::styled(
            "· RESCUE ON ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    } else if app.rescue_warn() {
        title.push(Span::styled(
            "· R: RESCUE? ",
            Style::default().fg(Color::Yellow),
        ));
    }
    let block = Block::bordered().title(Line::from(title));
    if let Some(error) = &app.hardware.error {
        frame.render_widget(Paragraph::new(dim(error.clone())).block(block), area);
        return;
    }
    let mut rows: Vec<ListItem> = app
        .devices
        .iter()
        .map(|device| ListItem::new(device_row(device)))
        .collect();
    if app.rescue {
        rows.extend(app.rescue_ports.iter().map(|port| {
            ListItem::new(Line::from(vec![
                Span::styled(" ◌ ", Style::default().fg(Color::Yellow)),
                bold(fm1_performer::routing::label_of(port)),
                dim("  rescue target — flash only a unit stuck in its loader"),
            ]))
        }));
    }
    if rows.is_empty() {
        let text = if app.rescue {
            "no MIDI port in sight"
        } else if app.rescue_warn() {
            "no FM-1 on USB — R offers every MIDI port as a rescue target"
        } else {
            "no FM-1 on USB — plug a unit in"
        };
        frame.render_widget(Paragraph::new(dim(text)).block(block), area);
        return;
    }
    frame.render_widget(List::new(rows).block(block), area);
}

fn device_row(device: &DeviceRow) -> Line<'static> {
    let flashing = device.flash.is_some();
    let mut spans = vec![
        Span::styled(" ● ", Style::default().fg(if flashing { ACCENT } else { OK })),
        bold(device.port.clone()),
        dim(format!("  {}", device.display)),
    ];
    if let Some(usb) = &device.usb {
        spans.push(dim(format!("  ·  usb {usb}")));
    }
    if let Some(flash) = &device.flash {
        let text = match flash.progress {
            Some(pct) => format!("  ·  {} {:.0}%", flash.step, pct * 100.0),
            None => format!("  ·  {}", flash.step),
        };
        spans.push(Span::styled(text, Style::default().fg(ACCENT)));
    } else if let Some(outcome) = &device.outcome {
        let good = outcome.starts_with("flashed:");
        spans.push(Span::styled(
            format!("  ·  {outcome}"),
            Style::default().fg(if good { OK } else { ERR }),
        ));
    }
    Line::from(spans)
}

/// The firmware library: installed entries and hub offers, with the hub fetch status
/// in the title.
fn library(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let installed = app
        .firmwares
        .iter()
        .filter(|firmware| !matches!(&firmware.source, FirmwareSource::Hub { blob: None, .. }))
        .count();
    let hub = match app.hub_status() {
        None => "no data dir".to_string(),
        Some(HubStatus::Loading) => "hub: refreshing…".to_string(),
        Some(HubStatus::Failed(_)) => "hub offline".to_string(),
        Some(HubStatus::Idle) => "hub: online".to_string(),
    };
    let focused = app.focus == Focus::Library;
    let all = if app.download_all { " · ALL" } else { "" };
    let block = Block::bordered()
        .title(format!(" LIBRARY — {installed} installed · {hub}{all} "))
        .border_style(Style::default().fg(if focused { ACCENT } else { DIM }));
    let width = area.width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = app
        .firmwares
        .iter()
        .map(|firmware| ListItem::new(library_row(app, firmware, width)))
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    app.list_state.select(Some(app.sel));
    frame.render_stateful_widget(list, area, &mut app.list_state);
    app.regions.lib_rows = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    app.regions.lib_offset = app.list_state.offset();
}

/// One library row: name and version, the byline, then a right-aligned status pill.
fn library_row(app: &TuiApp, firmware: &Firmware, width: usize) -> Line<'static> {
    let left = format!(" {} {}", firmware.name, firmware.version);
    let byline = if firmware.byline.is_empty() {
        String::new()
    } else {
        format!("  {}", firmware.byline)
    };
    let mut pill = match firmware.status {
        Status::Downloading => match app.download_percent(&firmware.name) {
            Some(pct) => format!("{pct}%"),
            None => "downloading".to_string(),
        },
        Status::Missing => "missing".to_string(),
        Status::Available => match &firmware.source {
            FirmwareSource::Builtin => "builtin".to_string(),
            FirmwareSource::File { .. } => "installed".to_string(),
            FirmwareSource::Hub { blob: Some(_), .. } => "installed".to_string(),
            FirmwareSource::Hub { blob: None, .. } => "download".to_string(),
        },
    };
    if firmware.upgradable {
        pill = format!("↑ {pill}");
    }
    if firmware.device == FirmwareDevice::Smk37Pro {
        pill = format!("{pill} · smk37");
    }
    let pill_color = match firmware.status {
        Status::Missing => ERR,
        Status::Downloading => ACCENT,
        Status::Available => match &firmware.source {
            FirmwareSource::Hub { blob: None, .. } => ACCENT,
            _ => DIM,
        },
    };
    let pad = width.saturating_sub(cols(&left) + cols(&byline) + cols(&pill) + 1);
    Line::from(vec![
        bold(left),
        dim(byline),
        dim(" ".repeat(pad)),
        Span::styled(pill, Style::default().fg(pill_color)),
    ])
}

/// The piano with its MIDI-input selector: a field row, then the keys.
fn keyboard(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let focused = app.focus == Focus::Keys;
    let block = Block::bordered()
        .title(" KEYBOARD ")
        .border_style(Style::default().fg(if focused { ACCENT } else { DIM }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 2 {
        return;
    }
    let field = Rect::new(inner.x, inner.y, inner.width, 1);
    let keys_area = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);

    let label = app.midi_label();
    let text = Line::from(vec![
        dim("MIDI in:  "),
        Span::styled(
            format!("▾ {label}"),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        dim("   (m to change; plays every connected unit)"),
    ]);
    frame.render_widget(Paragraph::new(text), field);
    app.regions.midi_field = Rect::new(field.x, field.y, (12 + cols(&label)) as u16, 1);

    let (whites, blacks) = layout_piano(keys_area);
    let buf = frame.buffer_mut();
    for &(rect, id) in &whites {
        paint_key(buf, keys_area, rect, id, false, app.held[id as usize]);
    }
    for &(rect, id) in &blacks {
        paint_key(buf, keys_area, rect, id, true, app.held[id as usize]);
    }
    // Black keys first: they overlap the whites and must win the hit test.
    app.regions.keys = blacks
        .iter()
        .chain(whites.iter())
        .map(|(rect, id)| (rect.intersection(keys_area), *id))
        .filter(|(rect, _)| !rect.is_empty())
        .collect();
}

#[allow(clippy::too_many_arguments)]
fn paint_key(buf: &mut Buffer, area: Rect, rect: Rect, id: u8, black: bool, held: bool) {
    let rect = rect.intersection(area);
    if rect.is_empty() {
        return;
    }
    let style = if held {
        Style::default()
            .fg(Color::Black)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD)
    } else if black {
        Style::default().fg(Color::Black).bg(KEY_BLACK)
    } else {
        Style::default().fg(Color::Black).bg(Color::White)
    };
    // A one-column gap on the right edge separates the white keys.
    let fill = if black {
        rect
    } else {
        Rect::new(
            rect.x,
            rect.y,
            rect.width.saturating_sub(1).max(1),
            rect.height,
        )
    };
    buf.set_style(fill, style);
    let note = PANEL_KEYS
        .iter()
        .find(|(key, _, _)| *key == id)
        .map(|(_, name, _)| *name)
        .unwrap_or("");
    let hint = panel_key_hint(id);
    if black {
        if !hint.is_empty() {
            let x = fill.x + fill.width.saturating_sub(1) / 2;
            buf.set_stringn(x, fill.y + fill.height - 1, hint, 1, style);
        }
    } else {
        let label = if fill.width >= 5 && !hint.is_empty() {
            format!("{note} {hint}")
        } else {
            note.to_string()
        };
        let x = fill.x + (fill.width.saturating_sub(cols(&label) as u16)) / 2;
        buf.set_stringn(x, fill.y + fill.height - 1, &label, fill.width as usize, style);
    }
}

/// The piano's geometry: white keys partition the width; black keys straddle the seam
/// after the white key they follow. Returns (whites, blacks) as (rect, key id).
fn layout_piano(area: Rect) -> (Vec<(Rect, u8)>, Vec<(Rect, u8)>) {
    let ww = (area.width / WHITE_KEYS.len() as u16).max(2);
    let total = ww * WHITE_KEYS.len() as u16;
    let x0 = area.x + area.width.saturating_sub(total) / 2;
    let whites: Vec<(Rect, u8)> = WHITE_KEYS
        .iter()
        .enumerate()
        .map(|(i, &id)| (Rect::new(x0 + i as u16 * ww, area.y, ww, area.height), id))
        .collect();
    let bw = ww.saturating_sub(2).clamp(2, ww);
    let bh = (area.height * 3 / 5).max(1);
    let blacks: Vec<(Rect, u8)> = BLACK_KEYS
        .iter()
        .map(|&(id, after)| {
            let seam = x0 + (after as u16 + 1) * ww;
            (Rect::new(seam - bw / 2, area.y, bw, bh), id)
        })
        .collect();
    (whites, blacks)
}

/// The metronome bar: clock switch, tempo with steppers, the beat light, and the
/// right-hand range note. One line; the clickable spans are measured into regions.
fn metronome(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let running = app.clock.enabled();
    let lit = app.clock.beat_light();
    let toggle = format!(" {} MIDI CLOCK ", if running { "●" } else { "○" });
    let tempo = format!(" {} BPM ", app.clock.bpm());
    let minus = "[−]";
    let plus = " [+] ";
    let beat = format!("  BEAT {}", if lit { "●" } else { "○" });
    let state = if running {
        "  RUNNING · 24 PPQN"
    } else {
        "  STOPPED · 24 PPQN"
    };
    let right = format!("{MIN_BPM}–{MAX_BPM} BPM · MIDI 0xF8 ");
    let left_width =
        cols(&toggle) + cols(&tempo) + cols(minus) + cols(plus) + cols(&beat) + cols(state);
    let pad = (area.width as usize).saturating_sub(left_width + cols(&right));

    let mut spans = vec![
        Span::styled(
            toggle.clone(),
            Style::default()
                .fg(if running { OK } else { DIM })
                .add_modifier(Modifier::BOLD),
        ),
        bold(tempo.clone()),
        dim(format!("{minus} ")),
        dim(plus),
        Span::styled(beat.clone(), Style::default().fg(if lit { OK } else { DIM })),
        Span::styled(state, Style::default().fg(if running { ACCENT } else { DIM })),
    ];
    if pad > 0 {
        spans.push(dim(format!("{}{}", " ".repeat(pad), right)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    let mut x = area.x;
    let mut region = |w: usize| {
        let rect = Rect::new(x, area.y, w as u16, 1);
        x += w as u16;
        rect
    };
    app.regions.metro_toggle = region(cols(&toggle));
    region(cols(&tempo));
    app.regions.bpm_minus = region(cols(minus) + 1);
    app.regions.bpm_plus = region(cols(plus));
}

/// The hint line; a transient status message takes it over.
fn footer(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let line = if let Some(message) = app.status() {
        Line::from(Span::styled(
            format!(" {message}"),
            Style::default().fg(Color::Yellow),
        ))
    } else {
        let hints = match app.focus {
            Focus::Library => {
                // d and x apply only to the selected row: upgrade when the hub
                // carries a newer version, delete when the entry can leave.
                let mut hints = String::from(if app.selected_upgradable() {
                    " ↑↓ select · d upgrade"
                } else {
                    " ↑↓ select · d download"
                });
                if app.selected_deletable() {
                    hints.push_str(" · x delete");
                }
                hints.push_str(" · a all · f flash · R rescue · r refresh hub · m MIDI in · space clock · −/+ tempo · Tab keys · q quit");
                hints
            }
            Focus::Keys => " letters and clicks play · Tab/Esc library · space clock · −/+ tempo · Ctrl-C quit".to_string(),
        };
        Line::from(dim(hints))
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// A centered rect, clamped to the screen.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width.min(area.width),
        height.min(area.height),
    )
}

/// A centered list modal; fills `app.regions.popup_items` with the row rects.
fn list_popup(
    frame: &mut Frame,
    app: &mut TuiApp,
    title: &str,
    items: &[Line<'static>],
    sel: usize,
) {
    let width = items
        .iter()
        .map(|line| line.width() as u16)
        .max()
        .unwrap_or(10)
        .max(title.len() as u16)
        + 6;
    let height = items.len() as u16 + 2;
    let area = centered(frame.area(), width, height);
    frame.render_widget(Clear, area);
    let block = Block::bordered().title(format!(" {title} "));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    for (i, item) in items.iter().enumerate() {
        let row = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let style = if i == sel {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        frame.render_widget(Paragraph::new(item.clone().style(style)), row);
        app.regions.popup_items.push((row, i));
    }
}

/// The physical MIDI keyboard selector.
fn midi_popup(frame: &mut Frame, app: &mut TuiApp, sel: usize) {
    let current = app.midi_label();
    let mut items = vec![Line::from(if current == "none" {
        "● no keyboard".to_string()
    } else {
        "  no keyboard".to_string()
    })];
    for (_, label) in app.midi_ports() {
        items.push(Line::from(if label == current {
            format!("● {label}")
        } else {
            format!("  {label}")
        }));
    }
    list_popup(frame, app, "MIDI keyboard", &items, sel);
}

/// Pick which target gets the firmware: the live units, plus every MIDI port while
/// the rescue toggle is on (flagged, in warning yellow).
fn flash_target_popup(frame: &mut Frame, app: &mut TuiApp, sel: usize, targets: &[(String, bool)]) {
    let items: Vec<Line> = targets
        .iter()
        .map(|(name, rescue)| {
            let label = fm1_performer::routing::label_of(name);
            if *rescue {
                Line::from(Span::styled(
                    format!("RESCUE: {label}"),
                    Style::default().fg(Color::Yellow),
                ))
            } else {
                Line::from(label)
            }
        })
        .collect();
    list_popup(frame, app, "Flash to…", &items, sel);
}

/// The flash progress modal: phase, gauge and the tail of the message log. Closable
/// once the board carries an error (success clears the board by itself).
fn flash_modal(frame: &mut Frame, board: &FlashState) {
    let area = centered(frame.area(), 62, 13);
    frame.render_widget(Clear, area);
    let block = Block::bordered().title(" FLASHING ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 3 {
        return;
    }
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(Span::styled(
            board.step.clone(),
            Style::default().fg(if board.error { ERR } else { ACCENT }),
        )),
        chunks[0],
    );
    if let Some(progress) = board.progress {
        frame.render_widget(
            Gauge::default()
                .ratio(f64::from(progress.clamp(0.0, 1.0)))
                .gauge_style(Style::default().fg(ACCENT)),
            chunks[1],
        );
    }
    let tail: Vec<Line> = board
        .log
        .iter()
        .rev()
        .take(chunks[2].height as usize)
        .rev()
        .map(|line| Line::from(dim(line.clone())))
        .collect();
    frame.render_widget(Paragraph::new(tail), chunks[2]);
    frame.render_widget(Paragraph::new(dim("Esc close")), chunks[3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every FM-1 note key gets a rect; blacks stay inside the piano and shorter
    /// than the whites.
    #[test]
    fn piano_covers_all_keys() {
        let area = Rect::new(0, 0, 100, 7);
        let (whites, blacks) = layout_piano(area);
        assert_eq!(whites.len(), WHITE_KEYS.len());
        assert_eq!(blacks.len(), BLACK_KEYS.len());
        for (rect, _) in whites.iter().chain(&blacks) {
            assert!(rect.right() <= area.right() && rect.bottom() <= area.bottom());
        }
        let white_h = whites[0].0.height;
        for (rect, _) in &blacks {
            assert!(rect.height < white_h);
        }
        // The whites tile the piano without gaps or overlaps.
        let mut x = whites[0].0.x;
        for (rect, _) in &whites {
            assert_eq!(rect.x, x);
            x = rect.right();
        }
    }

    /// A black key straddles the seam after its white key: it overlaps both
    /// neighbours, so a click near a seam hits the black key first.
    #[test]
    fn black_keys_straddle_seams() {
        let area = Rect::new(0, 0, 96, 7); // 6 columns per white key
        let (whites, blacks) = layout_piano(area);
        for (rect, id) in &blacks {
            let after = BLACK_KEYS
                .iter()
                .find(|(black_id, _)| black_id == id)
                .map(|(_, after)| *after)
                .unwrap();
            let seam = whites[after].0.right();
            assert!(rect.x < seam && rect.right() > seam);
        }
    }
}
