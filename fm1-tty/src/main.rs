//! fm1-tty — the pared-down terminal companion for live M-VAVE FM-1 units: the
//! firmware library with hub downloads and flashing, the virtual keyboard with a
//! MIDI-input selector, and the MIDI-clock metronome. No emulator — a TUI makes no
//! sense for one; everything here talks to units over USB-MIDI. All the machinery is
//! fm1-performer's; only the drawing and the event loop are new.

#[cfg(not(target_arch = "wasm32"))]
mod app;
#[cfg(not(target_arch = "wasm32"))]
mod ui;

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
    // Key releases let the computer keyboard hold notes; plain terminals get the
    // momentary fallback in app.rs.
    let key_release = terminal::supports_keyboard_enhancement().unwrap_or(false);
    if key_release {
        execute!(
            stdout,
            event::PushKeyboardEnhancementFlags(event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        )?;
    }
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore(key_release);
        original(info);
    }));

    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout))?;
    let mut app = app::TuiApp::new(key_release);
    let result = run(&mut terminal, &mut app);
    restore(key_release)?;
    result
}

/// Put the terminal back. Idempotent enough for the panic hook.
#[cfg(not(target_arch = "wasm32"))]
fn restore(key_release: bool) -> std::io::Result<()> {
    use ratatui::crossterm::{event, execute, terminal};
    use std::io;

    terminal::disable_raw_mode()?;
    let mut stdout = io::stdout();
    if key_release {
        execute!(stdout, event::PopKeyboardEnhancementFlags)?;
    }
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

/// The computer key that plays an FM-1 note key, per the shared layout table.
#[cfg(not(target_arch = "wasm32"))]
fn piano_key(name: &str) -> Option<u8> {
    fm1_performer::keys::PIANO
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, id)| *id)
}

#[cfg(not(target_arch = "wasm32"))]
fn on_key(app: &mut app::TuiApp, key: ratatui::crossterm::event::KeyEvent) {
    use app::{Focus, Popup};
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

    // Ctrl-C quits from anywhere, even over a modal (a flash runs to completion on
    // its own thread regardless; abandoning it mid-write is the user's call).
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }
    // A popup captures the keyboard.
    if app.popup.is_some() {
        if key.kind == KeyEventKind::Press {
            popup_key(app, key.code);
        }
        return;
    }
    // So does the flash modal: closable once it carries an error (success clears the
    // board by itself); running flashes ignore keys, like the GUI's dialog.
    if let Some(board) = app.flash_board() {
        if board.error
            && key.kind == KeyEventKind::Press
            && matches!(key.code, KeyCode::Esc | KeyCode::Enter)
        {
            app.dismiss_flash();
        }
        return;
    }
    // Key releases lift piano keys (terminals with the kitty protocol only).
    if key.kind == KeyEventKind::Release {
        if let KeyCode::Char(c) = key.code {
            if let Some(id) = piano_key(c.to_ascii_lowercase().encode_utf8(&mut [0; 4])) {
                app.note_up(id);
            }
        }
        return;
    }
    if key.kind == KeyEventKind::Repeat {
        return;
    }
    // Global keys, both focuses.
    match key.code {
        KeyCode::Char(' ') => return app.toggle_clock(),
        KeyCode::Char('-') | KeyCode::Char('_') => return app.bump_bpm(-1),
        KeyCode::Char('=') | KeyCode::Char('+') => return app.bump_bpm(1),
        KeyCode::Char('[') => return app.bump_bpm(-10),
        KeyCode::Char(']') => return app.bump_bpm(10),
        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::Library => Focus::Keys,
                Focus::Keys => Focus::Library,
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
            KeyCode::Char('d') => app.download_selected(),
            KeyCode::Char('f') => app.flash_selected(),
            KeyCode::Char('r') => app.reload_hub(),
            KeyCode::Char('m') => {
                let current = app.midi_label();
                let sel = app
                    .midi_ports()
                    .iter()
                    .position(|(_, label)| *label == current)
                    .map(|i| i + 1)
                    .unwrap_or(0);
                app.popup = Some(Popup::MidiPorts(sel));
            }
            KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
            _ => {}
        },
        Focus::Keys => match key.code {
            KeyCode::Esc => app.focus = Focus::Library,
            KeyCode::Char(c) => {
                if let Some(id) = piano_key(c.to_ascii_lowercase().encode_utf8(&mut [0; 4])) {
                    app.note_down(id);
                }
            }
            _ => {}
        },
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
                if let Some(unit) = names.get(sel) {
                    let unit = unit.clone();
                    app.start_flash(&unit, &pick);
                } else {
                    app.popup = None;
                }
            }
            _ => {}
        },
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
            if let Some(unit) = names.get(index) {
                let unit = unit.clone();
                app.start_flash(&unit, &pick);
            } else {
                app.popup = None;
            }
        }
        None => {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn on_mouse(app: &mut app::TuiApp, mouse: ratatui::crossterm::event::MouseEvent) {
    use app::{Focus, Popup};
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Position;

    let at = Position::new(mouse.column, mouse.row);
    let key_at = |app: &app::TuiApp| -> Option<u8> {
        app.regions
            .keys
            .iter()
            .find(|(rect, _)| rect.contains(at))
            .map(|(_, id)| *id)
    };

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
    // The flash modal ignores clicks (Esc when it errors); keep the piano silent.
    if app.flash_board().is_some() {
        return;
    }
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(id) = key_at(app) {
                app.focus = Focus::Keys;
                app.mouse_note(Some(id));
            } else if app.regions.midi_field.contains(at) {
                app.popup = Some(Popup::MidiPorts(0));
            } else if app.regions.metro_toggle.contains(at) {
                app.toggle_clock();
            } else if app.regions.bpm_minus.contains(at) {
                app.bump_bpm(-1);
            } else if app.regions.bpm_plus.contains(at) {
                app.bump_bpm(1);
            } else if app.regions.lib_rows.contains(at) {
                let row = app.regions.lib_offset + (mouse.row - app.regions.lib_rows.y) as usize;
                if row < app.firmwares.len() {
                    app.sel = row;
                }
                app.focus = Focus::Library;
            }
        }
        MouseEventKind::Up(MouseButton::Left) => app.mouse_note(None),
        MouseEventKind::Drag(MouseButton::Left) => {
            // Glide across the keys; leaving the piano keeps the last note held.
            if app.mouse_holding() {
                if let Some(id) = key_at(app) {
                    app.mouse_note(Some(id));
                }
            }
        }
        MouseEventKind::ScrollUp if app.regions.lib_rows.contains(at) => app.scroll(-3),
        MouseEventKind::ScrollDown if app.regions.lib_rows.contains(at) => app.scroll(3),
        _ => {}
    }
}
