//! The drawing half of fm1-tty: the devices block, the MIDI graph, the library list
//! with its filter row, the metronome bar and the popups. State lives in `app.rs`;
//! this reads it and refills the mouse hit regions (`app.regions`) for the next event
//! pass.

use crate::app::{Focus, GraphRow, Popup, Regions, TuiApp};
use fm1_performer::clock::{MAX_BPM, MIN_BPM};
use fm1_performer::hardware::{DeviceRow, FlashState};
use fm1_performer::library::{Firmware, FirmwareDevice, FirmwareSource, HubStatus, Status};
use fm1_performer::routing::Port;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Gauge, List, ListItem, Paragraph};
use ratatui::Frame;

const ACCENT: Color = Color::Cyan;
const OK: Color = Color::Green;
const ERR: Color = Color::Red;
const DIM: Color = Color::DarkGray;

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
    // The graph takes the piano's old space, growing with its rows (and scrolling
    // past that) so the library keeps the rest.
    let graph_h = (app.graph_rows().len() as u16 + 3).clamp(4, 12);
    let chunks = Layout::vertical([
        Constraint::Length(devices_h),
        Constraint::Length(graph_h),
        Constraint::Min(6),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    app.regions = Regions::default();
    devices(frame, app, chunks[0]);
    graph(frame, app, chunks[1]);
    library(frame, app, chunks[2]);
    metronome(frame, app, chunks[3]);
    footer(frame, app, chunks[4]);

    match app.popup.clone() {
        Some(Popup::MidiPorts(sel)) => midi_popup(frame, app, sel),
        Some(Popup::FlashTarget(sel, names, _)) => flash_target_popup(frame, app, sel, &names),
        Some(Popup::DeviceFilter(sel)) => device_popup(frame, app, sel),
        Some(Popup::GraphSinks(source, sel)) => graph_sinks_popup(frame, app, sel, &source),
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

/// The MIDI graph, in text: every source (system MIDI outs, live units) with its
/// links and a connect action, plus the physical-keyboard selector that forwards one
/// keyboard to every unit. Rows are mouse-clickable and keyboard-selectable.
fn graph(frame: &mut Frame, app: &mut TuiApp, area: Rect) {
    let focused = app.focus == Focus::Graph;
    let block = Block::bordered()
        .title(" MIDI GRAPH · one out feeds any number of ins ")
        .border_style(Style::default().fg(if focused { ACCENT } else { DIM }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let label = app.midi_label();
    let field = Line::from(vec![
        dim("MIDI in: "),
        Span::styled(
            format!("▾ {label}"),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        dim("   (m to change; forwards to every FM-1)"),
    ]);
    let field_area = Rect::new(inner.x, inner.y, inner.width, 1);
    frame.render_widget(Paragraph::new(field), field_area);
    app.regions.midi_field =
        Rect::new(field_area.x, field_area.y, (11 + cols(&label)) as u16, 1);
    if inner.height < 2 {
        return;
    }

    let list_area = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    app.regions.graph_list = list_area;
    let rows = app.graph_rows();
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(dim("No MIDI outs in sight — plug something in.")),
            list_area,
        );
        return;
    }
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| ListItem::new(graph_line(app, row)))
        .collect();
    let highlight = if focused {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    let list = List::new(items).highlight_style(highlight);
    app.graph_state
        .select(Some(app.graph_sel.min(rows.len() - 1)));
    frame.render_stateful_widget(list, list_area, &mut app.graph_state);
    let offset = app.graph_state.offset();
    app.regions.graph_rows = (offset..rows.len())
        .map_while(|index| {
            let y = list_area.y + (index - offset) as u16;
            (y < list_area.bottom()).then(|| {
                (Rect::new(list_area.x, y, list_area.width, 1), index)
            })
        })
        .collect();
}

/// One graph row's line: a bold source heading, an unroute pill and its sink, or the
/// connect action.
fn graph_line(app: &TuiApp, row: &GraphRow) -> Line<'static> {
    match row {
        GraphRow::Source(port) => Line::from(bold(format!(" {}", app.graph_source_label(port)))),
        GraphRow::Link(_, sink) => Line::from(vec![
            Span::styled("   × ", Style::default().fg(ERR)),
            dim(app.graph_source_label(sink)),
        ]),
        GraphRow::Connect(_) => {
            Line::from(Span::styled("   + connect…", Style::default().fg(ACCENT)))
        }
    }
}

/// The firmware library: installed entries and hub offers, with the filter field and
/// device selector on top and the hub fetch status in the title.
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
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // The filter field on the left, the device selector on the right. Both are
    // measured into regions so a click can focus or open them.
    let editing = app.focus == Focus::Filter;
    let left = if app.filter.is_empty() && !editing {
        " filter: / to search".to_string()
    } else {
        format!(" filter: {}{}", app.filter, if editing { "▏" } else { "" })
    };
    let right = format!("▾ {} ", app.device_filter_label());
    let pad = (inner.width as usize).saturating_sub(cols(&left) + cols(&right));
    let header = Line::from(vec![
        Span::styled(
            left,
            Style::default().fg(if editing { ACCENT } else { Color::Reset }),
        ),
        Span::raw(" ".repeat(pad)),
        Span::styled(right, Style::default().fg(ACCENT)),
    ]);
    let header_area = Rect::new(inner.x, inner.y, inner.width, 1);
    frame.render_widget(Paragraph::new(header), header_area);
    let right_w = (cols(&format!("▾ {} ", app.device_filter_label())) as u16).min(inner.width);
    app.regions.lib_devices =
        Rect::new(header_area.right().saturating_sub(right_w), header_area.y, right_w, 1);
    app.regions.lib_filter =
        Rect::new(header_area.x, header_area.y, header_area.width.saturating_sub(right_w), 1);

    let list_area = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );
    let width = list_area.width as usize;
    let items: Vec<ListItem> = app
        .visible()
        .map(|firmware| library_row(firmware, app.download_percent(&firmware.name), width))
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    app.list_state
        .select((!app.view.is_empty()).then_some(app.sel));
    frame.render_stateful_widget(list, list_area, &mut app.list_state);
    app.regions.lib_rows = list_area;
    app.regions.lib_offset = app.list_state.offset();
}

/// One library row, two lines: name, version, byline and the right-aligned status
/// pill; then as much of the description as the width holds, in dim text.
fn library_row(firmware: &Firmware, percent: Option<u32>, width: usize) -> ListItem<'static> {
    let left = format!(" {} {}", firmware.name, firmware.version);
    let byline = if firmware.byline.is_empty() {
        String::new()
    } else {
        format!("  {}", firmware.byline)
    };
    let mut pill = match firmware.status {
        Status::Downloading => match percent {
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
    if firmware.device == FirmwareDevice::Smk37 {
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
    // Source descriptions can hold paragraph breaks; a list line can't.
    let description = firmware.description.split_whitespace().collect::<Vec<_>>().join(" ");
    ListItem::new(vec![
        Line::from(vec![
            bold(left),
            dim(byline),
            dim(" ".repeat(pad)),
            Span::styled(pill, Style::default().fg(pill_color)),
        ]),
        Line::from(dim(format!(" {description}"))),
    ])
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
                hints.push_str(" · / filter · v devices · a all · f flash · R rescue · r refresh hub · m MIDI in · space clock · −/+ tempo · Tab graph · q quit");
                hints
            }
            Focus::Graph => " ↑↓ select · Enter connect/unroute · m MIDI in · Tab filter/list · space clock · −/+ tempo · q quit".to_string(),
            Focus::Filter => " type to filter · Backspace edit · Ctrl-U clear · Enter/Esc back · Tab graph · Ctrl-C quit".to_string(),
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

/// The library's device-family selector.
fn device_popup(frame: &mut Frame, app: &mut TuiApp, sel: usize) {
    let check = |on: bool| if on { "[✓]" } else { "[ ]" };
    let items = vec![
        Line::from(format!("{} FM-1", check(app.device_filter.fm1))),
        Line::from(format!("{} SMK-37", check(app.device_filter.smk37))),
    ];
    list_popup(frame, app, "Library devices", &items, sel);
}

/// The graph's connect menu for one source: every sink it feeds nothing yet.
fn graph_sinks_popup(frame: &mut Frame, app: &mut TuiApp, sel: usize, source: &Port) {
    let sinks = app.graph_sinks(source);
    let mut items: Vec<Line> = sinks
        .iter()
        .map(|(_, label)| Line::from(label.clone()))
        .collect();
    if items.is_empty() {
        items.push(Line::from(dim("no other sinks")));
    }
    let title = format!("Connect {}", app.graph_source_label(source));
    list_popup(frame, app, &title, &items, sel);
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
    use ratatui::buffer::Buffer;
    use ratatui::widgets::{ListState, StatefulWidget};

    fn firmware(description: &str) -> Firmware {
        Firmware {
            name: "Felucca".to_string(),
            byline: "author".to_string(),
            version: "1.2".to_string(),
            description: description.to_string(),
            tags: Vec::new(),
            device: FirmwareDevice::Fm1,
            upgradable: false,
            status: Status::Available,
            website: None,
            source_url: None,
            source: FirmwareSource::Builtin,
        }
    }

    #[test]
    fn library_row_puts_the_description_on_a_second_dim_line() {
        let item = library_row(&firmware(&"x".repeat(100)), None, 40);
        assert_eq!(item.height(), 2);

        let area = Rect::new(0, 0, 40, 2);
        let mut buf = Buffer::empty(area);
        let mut state = ListState::default();
        StatefulWidget::render(List::new([item]), area, &mut buf, &mut state);

        let text = |y: u16| (0..40).map(|x| buf[(x, y)].symbol()).collect::<String>();
        assert!(text(0).contains("Felucca 1.2"));
        assert_eq!(text(1), format!(" {}", "x".repeat(39)));
        assert_eq!(buf[(0, 1)].fg, DIM);
    }
}
