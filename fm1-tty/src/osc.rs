//! Terminal escape codes for native progress and notifications: OSC 9;4 (the ConEmu
//! progress sequence — taskbar ring/bar in Windows Terminal, Ghostty, Konsole,
//! WezTerm, mintty) and OSC 9 (the iTerm-style desktop notification). Both go
//! straight to stdout: OSC sequences never move the cursor or touch the screen
//! ratatui draws, and terminals without support ignore them.

use std::io::{Write, stdout};

/// One self-contained sequence per write, so a draw can never split it. Inside
/// tmux the pane eats raw OSC, so wrap for passthrough (ESCs doubled inside the
/// DCS envelope); with `allow-passthrough` off even that is dropped — nothing more
/// to do then.
fn emit(sequence: &str) {
    let wrapped;
    let sequence = if std::env::var_os("TMUX").is_some() {
        wrapped = format!("\x1bPtmux;{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"));
        &wrapped
    } else {
        sequence
    };
    let mut out = stdout().lock();
    let _ = out.write_all(sequence.as_bytes());
    let _ = out.flush();
}

/// Show or update the terminal's progress bar (state 1, percent 0–100).
pub fn progress(percent: u32) {
    emit(&format!("\x1b]9;4;1;{}\x07", percent.min(100)));
}

/// The error state (state 2, usually red): a failed download. Pair with
/// [`progress_clear`] once the user has had a moment to see it.
pub fn progress_error() {
    emit("\x1b]9;4;2\x07");
}

/// Hide the progress bar (state 0).
pub fn progress_clear() {
    emit("\x1b]9;4;0\x07");
}

/// A desktop notification. Control characters (BEL among them) would end the
/// sequence early, so they are blanked.
pub fn notify(message: &str) {
    emit(&format!("\x1b]9;{}\x07", sanitize(message)));
}

/// Message minus control characters.
fn sanitize(message: &str) -> String {
    message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn notify_strips_control_characters() {
        // BEL would terminate the sequence early and truncate the message.
        assert_eq!(super::sanitize("a\x07b\x1bc"), "a b c");
        assert_eq!(super::sanitize("plain"), "plain");
    }
}
