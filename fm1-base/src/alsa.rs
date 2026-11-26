//! ALSA's library-level diagnostics print to stderr, independently of the return
//! values every midir call already provides.

/// Stop alsa-lib from printing its own error messages to stderr.
///
/// The failures are still reported normally: midir's `MidiInput::new` / `MidiOutput::new`
/// return them as `Err`, which the MIDI modules handle. But a TUI shares the terminal
/// with stderr and polls MIDI twice a second, so one unreadable `/dev/snd/seq`
/// (`Permission denied`, a missing device, …) prints a line on every poll and wrecks
/// the screen. Call once, before any MIDI thread starts; later calls are no-ops.
pub fn silence_errors() {
    #[cfg(target_os = "linux")]
    {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        // SAFETY: the handler lives for the rest of the process, ignores every
        // argument and cannot unwind; `Once` installs it exactly once.
        ONCE.call_once(|| unsafe {
            alsa_sys::snd_lib_error_set_handler(Some(silent));
        });
    }
}

/// The replacement printer: the ABI of `snd_lib_error_handler_t`, all arguments ignored.
#[cfg(target_os = "linux")]
unsafe extern "C" fn silent(
    _file: *const std::ffi::c_char,
    _line: std::ffi::c_int,
    _function: *const std::ffi::c_char,
    _err: std::ffi::c_int,
    _fmt: *const std::ffi::c_char,
    _: ...
) {
}
