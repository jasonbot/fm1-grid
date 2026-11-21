//! The small state machine shared by the GUI and TUI: remember which firmware a
//! hardware flash is installing, then yield it once when the board clears after a
//! successful flash. A board error before success drops the pending entry.

use crate::hardware::FlashState;

pub struct FlashInstall<T> {
    pending: Option<T>,
    seen: bool,
}

impl<T> Default for FlashInstall<T> {
    fn default() -> Self {
        Self {
            pending: None,
            seen: false,
        }
    }
}

impl<T> FlashInstall<T> {
    /// Arm a flash with the firmware it installs.
    pub fn start(&mut self, firmware: T) {
        self.pending = Some(firmware);
        self.seen = false;
    }

    /// Feed the hardware board state. Returns the firmware once, only when a flash
    /// was observed running and then cleared successfully.
    pub fn observe(&mut self, board: Option<&FlashState>) -> Option<T> {
        match board {
            Some(board) if board.error => {
                if self.seen {
                    self.pending = None;
                    self.seen = false;
                }
            }
            Some(_) => self.seen = true,
            None => {}
        }
        if self.seen && board.is_none() {
            self.seen = false;
            return self.pending.take();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> FlashState {
        FlashState {
            step: "sending".into(),
            progress: None,
            error: false,
            log: Vec::new(),
        }
    }

    fn failed() -> FlashState {
        FlashState {
            step: "failed".into(),
            progress: None,
            error: true,
            log: Vec::new(),
        }
    }

    #[test]
    fn yields_once_after_success() {
        let mut install = FlashInstall::default();
        install.start("firmware");
        assert_eq!(install.observe(None), None);
        assert_eq!(install.observe(Some(&running())), None);
        assert_eq!(install.observe(None), Some("firmware"));
        assert_eq!(install.observe(None), None);
    }

    #[test]
    fn failure_drops_the_pending_install() {
        let mut install = FlashInstall::default();
        install.start("firmware");
        assert_eq!(install.observe(Some(&running())), None);
        assert_eq!(install.observe(Some(&failed())), None);
        assert_eq!(install.observe(None), None);
    }

    #[test]
    fn a_stale_error_does_not_drop_a_new_install() {
        let mut install = FlashInstall::default();
        install.start("firmware");
        assert_eq!(install.observe(Some(&failed())), None);
        assert_eq!(install.observe(Some(&running())), None);
        assert_eq!(install.observe(None), Some("firmware"));
    }
}
