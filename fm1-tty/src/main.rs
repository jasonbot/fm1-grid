//! fm1-tty — the pared-down terminal companion for live M-VAVE FM-1 units: the
//! firmware library with hub downloads and flashing, the MIDI graph, the
//! MIDI-keyboard selector and the MIDI-clock metronome. No emulator — a TUI makes no
//! sense for one; everything here talks to units over USB-MIDI. All the machinery is
//! fm1-performer's; only the drawing and the event loop are new.

#[cfg(not(target_arch = "wasm32"))]
mod app;
#[cfg(not(target_arch = "wasm32"))]
mod osc;
#[cfg(not(target_arch = "wasm32"))]
mod ui;

use fm1_performer::session::DeviceFilter;

/// The library does not compile to wasm (ratatui/crossterm are native-only).
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::io::Result<()> {
    use ratatui::crossterm::{event, execute, terminal};
    use std::io;

    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, terminal::EnterAlternateScreen, event::EnableMouseCapture)?;
    osc::title("Synth Firmware Circus");
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        original(info);
    }));

    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout))?;
    let mut app = app::TuiApp::new();
    let result = run(&mut terminal, &mut app);
    app.save_session();
    restore()?;
    result
}

/// Put the terminal back. Idempotent enough for the panic hook.
#[cfg(not(target_arch = "wasm32"))]
fn restore() -> std::io::Result<()> {
    use ratatui::crossterm::{event, execute, terminal};
    use std::io;

    // Never leave a progress bar hanging on the terminal's tab/taskbar.
    osc::progress_clear();
    terminal::disable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, event::DisableMouseCapture, terminal::LeaveAlternateScreen)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn run(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut app::TuiApp,
) -> std::io::Result<()> {
    use ratatui::crossterm::event;
    use std::time::Duration;

    while !app.quit {
        app.tick();
        terminal.draw(|frame| ui::draw(frame, app))?;
        // ~60 fps cap; the poll timeout is also the beat light's refresh rhythm.
        if event::poll(Duration::from_millis(16))? {
            handle(app, event::read()?);
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn handle(app: &mut app::TuiApp, event: ratatui::crossterm::event::Event) {
    use ratatui::crossterm::event::Event;
    match event {
        Event::Key(key) => on_key(app, key),
        Event::Mouse(mouse) => on_mouse(app, mouse),
        _ => {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn on_key(app: &mut app::TuiApp, key: ratatui::crossterm::event::KeyEvent) {
    use app::{Focus, Popup};
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

    if key.kind != KeyEventKind::Press {
        return;
    }
    // Ctrl-C quits from anywhere, even over a modal (a flash runs to completion on
    // its own thread regardless; abandoning it mid-write is the user's call).
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }
    // A popup captures the keyboard.
    if app.popup.is_some() {
        popup_key(app, key.code);
        return;
    }
    // So does the flash modal: Esc closes it any time — a running flash continues on
    // its own thread, and the board stays down until the next flash starts. Enter
    // still closes an error board.
    if let Some(board) = app.flash_board() {
        if key.code == KeyCode::Esc || (board.error && key.code == KeyCode::Enter) {
            app.dismiss_flash();
        }
        return;
    }
    // The filter field takes printable keys; only its own edits and exits apply.
    if app.focus == Focus::Filter {
        match key.code {
            KeyCode::Esc => {
                app.filter_clear();
                app.focus = Focus::Library;
            }
            KeyCode::Enter => app.focus = Focus::Library,
            KeyCode::Tab => app.focus = Focus::Graph,
            KeyCode::Backspace => app.filter_backspace(),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.filter_clear()
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.filter_push(c)
            }
            _ => {}
        }
        return;
    }
    // Global keys, both focuses.
    match key.code {
        KeyCode::Char(' ') => return app.toggle_clock(),
        KeyCode::Char('-') | KeyCode::Char('_') => return app.bump_bpm(-1),
        KeyCode::Char('=') | KeyCode::Char('+') => return app.bump_bpm(1),
        KeyCode::Char('[') => return app.bump_bpm(-10),
        KeyCode::Char(']') => return app.bump_bpm(10),
        KeyCode::Char('m') => {
            let current = app.midi_label();
            let sel = app
                .midi_ports()
                .iter()
                .position(|(_, label)| *label == current)
                .map(|i| i + 1)
                .unwrap_or(0);
            app.popup = Some(Popup::MidiPorts(sel));
            return;
        }
        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::Library => Focus::Graph,
                Focus::Graph => Focus::Filter,
                Focus::Filter => Focus::Library,
            };
            return;
        }
        _ => {}
    }
    match app.focus {
        Focus::Library => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.scroll(-1),
            KeyCode::Down | KeyCode::Char('j') => app.scroll(1),
            KeyCode::PageUp => app.scroll(-10),
            KeyCode::PageDown => app.scroll(10),
            KeyCode::Home => app.scroll(isize::MIN),
            KeyCode::End => app.scroll(isize::MAX),
            KeyCode::Char('/') => app.focus = Focus::Filter,
            KeyCode::Char('v') => {
                app.popup = Some(Popup::DeviceFilter(app.device_filter.index()))
            }
            KeyCode::Char('d') => app.download_selected(),
            KeyCode::Char('x') => app.delete_selected(),
            KeyCode::Char('a') => app.toggle_download_all(),
            KeyCode::Char('f') => app.flash_selected(),
            KeyCode::Char('r') => app.reload_hub(),
            KeyCode::Char('R') => app.toggle_rescue(),
            KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
            _ => {}
        },
        Focus::Graph => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.graph_move(-1),
            KeyCode::Down | KeyCode::Char('j') => app.graph_move(1),
            KeyCode::PageUp => app.graph_move(-5),
            KeyCode::PageDown => app.graph_move(5),
            KeyCode::Home => app.graph_move(isize::MIN),
            KeyCode::End => app.graph_move(isize::MAX),
            KeyCode::Enter => {
                let sel = app.graph_sel;
                app.graph_activate(sel);
            }
            KeyCode::Esc => app.focus = Focus::Library,
            KeyCode::Char('q') => app.quit = true,
            KeyCode::Char('/') => app.focus = Focus::Filter,
            _ => {}
        },
        Focus::Filter => {}
    }
}

/// A popup's keyboard handling: arrows move, Enter confirms, Esc cancels.
#[cfg(not(target_arch = "wasm32"))]
fn popup_key(app: &mut app::TuiApp, code: ratatui::crossterm::event::KeyCode) {
    use app::Popup;
    use ratatui::crossterm::event::KeyCode;

    match app.popup.clone() {
        Some(Popup::MidiPorts(sel)) => {
            let count = 1 + app.midi_ports().len();
            match code {
                KeyCode::Esc => app.popup = None,
                KeyCode::Up => app.popup = Some(Popup::MidiPorts(sel.saturating_sub(1))),
                KeyCode::Down => app.popup = Some(Popup::MidiPorts((sel + 1).min(count - 1))),
                KeyCode::Enter => {
                    let ports = app.midi_ports();
                    let pick = if sel == 0 {
                        None
                    } else {
                        ports.get(sel - 1).map(|(name, _)| name.clone())
                    };
                    app.select_midi(pick);
                    app.popup = None;
                }
                _ => {}
            }
        }
        Some(Popup::FlashTarget(sel, names, pick)) => match code {
            KeyCode::Esc => app.popup = None,
            KeyCode::Up => {
                app.popup = Some(Popup::FlashTarget(sel.saturating_sub(1), names, pick));
            }
            KeyCode::Down => {
                let next = (sel + 1).min(names.len().saturating_sub(1));
                app.popup = Some(Popup::FlashTarget(next, names, pick));
            }
            KeyCode::Enter => {
                if let Some((unit, rescue)) = names.get(sel).cloned() {
                    app.start_flash(&unit, rescue, &pick);
                } else {
                    app.popup = None;
                }
            }
            _ => {}
        },
        Some(Popup::DeviceFilter(sel)) => match code {
            KeyCode::Esc => app.popup = None,
            KeyCode::Up => app.popup = Some(Popup::DeviceFilter(sel.saturating_sub(1))),
            KeyCode::Down => {
                app.popup = Some(Popup::DeviceFilter((sel + 1).min(DeviceFilter::ALL.len() - 1)))
            }
            KeyCode::Enter => {
                app.select_device(sel);
                app.popup = None;
            }
            _ => {}
        },
        Some(Popup::GraphSinks(source, sel)) => {
            let count = app.graph_sinks(&source).len().max(1);
            match code {
                KeyCode::Esc => app.popup = None,
                KeyCode::Up => {
                    app.popup = Some(Popup::GraphSinks(source, sel.saturating_sub(1)))
                }
                KeyCode::Down => {
                    app.popup = Some(Popup::GraphSinks(source, (sel + 1).min(count - 1)))
                }
                KeyCode::Enter => app.confirm_graph_sink(sel),
                _ => {}
            }
        }
        None => {}
    }
}

/// Confirm the clicked popup row, like Enter.
#[cfg(not(target_arch = "wasm32"))]
fn popup_click(app: &mut app::TuiApp, index: usize) {
    use app::Popup;
    match app.popup.clone() {
        Some(Popup::MidiPorts(_)) => {
            let ports = app.midi_ports();
            let pick = if index == 0 {
                None
            } else {
                ports.get(index - 1).map(|(name, _)| name.clone())
            };
            app.select_midi(pick);
            app.popup = None;
        }
        Some(Popup::FlashTarget(_, names, pick)) => {
            if let Some((unit, rescue)) = names.get(index).cloned() {
                app.start_flash(&unit, rescue, &pick);
            } else {
                app.popup = None;
            }
        }
        Some(Popup::DeviceFilter(_)) => {
            app.select_device(index);
            app.popup = None;
        }
        Some(Popup::GraphSinks(_, _)) => app.confirm_graph_sink(index),
        None => {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn on_mouse(app: &mut app::TuiApp, mouse: ratatui::crossterm::event::MouseEvent) {
    use app::{Focus, Popup};
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Position;

    let at = Position::new(mouse.column, mouse.row);

    // Popup clicks confirm a row; a click outside cancels.
    if app.popup.is_some() {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            let hit = app
                .regions
                .popup_items
                .iter()
                .find(|(rect, _)| rect.contains(at))
                .map(|(_, index)| *index);
            match hit {
                Some(index) => popup_click(app, index),
                None => app.popup = None,
            }
        }
        return;
    }
    // The flash modal ignores clicks (Esc closes it).
    if app.flash_board().is_some() {
        return;
    }
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if app.regions.midi_field.contains(at) {
                app.popup = Some(Popup::MidiPorts(0));
            } else if app.regions.lib_filter.contains(at) {
                app.focus = Focus::Filter;
            } else if app.regions.lib_devices.contains(at) {
                app.popup = Some(Popup::DeviceFilter(app.device_filter.index()));
            } else if let Some(&(_, index)) = app
                .regions
                .graph_rows
                .iter()
                .find(|(rect, _)| rect.contains(at))
            {
                app.focus = Focus::Graph;
                app.graph_activate(index);
            } else if app.regions.metro_toggle.contains(at) {
                app.toggle_clock();
            } else if app.regions.bpm_minus.contains(at) {
                app.bump_bpm(-1);
            } else if app.regions.bpm_plus.contains(at) {
                app.bump_bpm(1);
            } else if app.regions.lib_rows.contains(at) {
                let line = (mouse.row - app.regions.lib_rows.y) as usize;
                let row = app.regions.lib_offset + line / 2;
                if row < app.view.len() {
                    app.sel = row;
                }
                app.focus = Focus::Library;
            }
        }
        MouseEventKind::ScrollUp if app.regions.graph_list.contains(at) => app.graph_move(-3),
        MouseEventKind::ScrollDown if app.regions.graph_list.contains(at) => app.graph_move(3),
        MouseEventKind::ScrollUp if app.regions.lib_rows.contains(at) => app.scroll(-3),
        MouseEventKind::ScrollDown if app.regions.lib_rows.contains(at) => app.scroll(3),
        _ => {}
    }
}
