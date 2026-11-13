//! The platform boundary for automatic updates.
//!
//! Each target picks its own [`platform::Backend`]. The current backends are
//! deliberately stubs: updates are disabled, checking reports nothing, installing
//! is unavailable and restart is a no-op. The UI and the update state machine are
//! complete so a real platform backend can replace the stub without touching the
//! window chrome.

use crate::app::GridApp;
use gpui_kit::{Context, Window};
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};

pub(crate) mod platform;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateStage {
    Downloading,
    Installing,
    Processing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UpdateProgress {
    pub stage: UpdateStage,
    pub message: Option<String>,
    /// `None`: no progress bar. `-1`: indeterminate. `0..=100`: determinate.
    pub percentage: Option<i32>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateOutcome {
    Success,
    Fail,
    TryAgain,
}

/// The platform-specific updater contract. The async methods are intentionally
/// stubbed today; a real backend can run in the platform's native updater.
pub(crate) type ProgressCallback = Arc<Mutex<dyn FnMut(UpdateProgress) + Send>>;

#[allow(async_fn_in_trait)]
pub(crate) trait PlatformUpdater {
    fn updates_enabled(&self) -> bool;
    async fn check_for_updates(&self, window: Option<isize>) -> bool;
    fn can_update(&self) -> bool;
    async fn install_update(
        &self,
        window: Option<isize>,
        progress: ProgressCallback,
    ) -> UpdateOutcome;
    fn restart_app(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateAction {
    Current,
    Check,
    Update,
    Updating,
    Restart,
}

impl UpdateAction {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Current => "Current",
            Self::Check => "Check",
            Self::Update => "Update",
            Self::Updating => "Updating",
            Self::Restart => "Restart",
        }
    }

    pub(crate) fn busy(self) -> bool {
        matches!(self, Self::Check | Self::Updating)
    }
}

const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

pub(crate) struct Updates {
    backend: platform::Backend,
    last_auto_check: Option<Instant>,
    checking: bool,
    available: bool,
    updating: bool,
    restart_ready: bool,
    progress: Option<UpdateProgress>,
    progress_shared: Option<Arc<Mutex<Option<UpdateProgress>>>>,
}

impl Updates {
    pub(crate) fn new() -> Self {
        Self {
            backend: platform::Backend,
            last_auto_check: None,
            checking: false,
            available: false,
            updating: false,
            restart_ready: false,
            progress: None,
            progress_shared: None,
        }
    }

    pub(crate) fn action(&self) -> UpdateAction {
        if self.restart_ready {
            UpdateAction::Restart
        } else if self.updating {
            UpdateAction::Updating
        } else if self.available {
            UpdateAction::Update
        } else if self.checking || self.backend.updates_enabled() {
            UpdateAction::Check
        } else {
            UpdateAction::Current
        }
    }

    pub(crate) fn progress(&self) -> Option<&UpdateProgress> {
        self.progress.as_ref()
    }

    pub(crate) fn activate(&mut self, window: &Window, cx: &mut Context<GridApp>) {
        let window = native_window_handle(window);
        match self.action() {
            UpdateAction::Check if !self.checking => self.check(window, cx),
            UpdateAction::Update if self.backend.can_update() => self.install(window, cx),
            UpdateAction::Restart => self.backend.restart_app(),
            _ => {}
        }
    }

    fn check(&mut self, window: Option<isize>, cx: &mut Context<GridApp>) {
        if !self.backend.updates_enabled() {
            return;
        }
        self.checking = true;
        cx.notify();
        let backend = self.backend;
        cx.spawn(async move |this, cx| {
            let found = backend.check_for_updates(window).await;
            let _ = this.update(cx, |this, cx| {
                this.updates.checking = false;
                this.updates.available = found;
                cx.notify();
            });
        })
        .detach();
    }

    fn install(&mut self, window: Option<isize>, cx: &mut Context<GridApp>) {
        if self.updating || !self.backend.can_update() {
            return;
        }
        self.updating = true;
        self.progress = Some(UpdateProgress {
            stage: UpdateStage::Downloading,
            message: None,
            percentage: None,
        });
        cx.notify();
        let backend = self.backend;
        let shared = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&shared);
        let progress: ProgressCallback = Arc::new(Mutex::new(move |value| {
            *slot.lock().unwrap() = Some(value);
        }));
        self.progress_shared = Some(shared);
        cx.spawn(async move |this, cx| {
            let outcome = backend.install_update(window, progress).await;
            let _ = this.update(cx, |this, cx| {
                this.updates.updating = false;
                this.updates.restart_ready = outcome == UpdateOutcome::Success;
                this.updates.progress = None;
                this.updates.progress_shared = None;
                cx.notify();
            });
        })
        .detach();
    }

    /// Move the latest backend progress (written by the platform callback) into the
    /// UI state. Returns true when the indicator changed.
    pub(crate) fn sync_progress(&mut self) -> bool {
        let Some(shared) = &self.progress_shared else {
            return false;
        };
        let Some(progress) = shared.lock().unwrap().take() else {
            return false;
        };
        let changed = self.progress.as_ref() != Some(&progress);
        self.progress = Some(progress);
        changed
    }

    pub(crate) fn auto_check(&mut self, window: &Window, cx: &mut Context<GridApp>) {
        if !self.backend.updates_enabled()
            || self.checking
            || self.updating
            || self.available
            || self.restart_ready
        {
            return;
        }
        if self
            .last_auto_check
            .is_some_and(|last| last.elapsed() < AUTO_CHECK_INTERVAL)
        {
            return;
        }
        self.last_auto_check = Some(Instant::now());
        self.check(native_window_handle(window), cx);
    }
}

#[cfg(windows)]
fn native_window_handle(window: &Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

#[cfg(not(windows))]
fn native_window_handle(_: &Window) -> Option<isize> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_stub_starts_current() {
        let updates = Updates::new();
        assert_eq!(updates.action(), UpdateAction::Current);
        assert_eq!(updates.action().label(), "Current");
    }

    #[test]
    fn action_labels_prefer_update_and_restart() {
        let mut updates = Updates::new();
        updates.available = true;
        assert_eq!(updates.action(), UpdateAction::Update);
        updates.updating = true;
        assert_eq!(updates.action(), UpdateAction::Updating);
        updates.restart_ready = true;
        assert_eq!(updates.action(), UpdateAction::Restart);
    }
}
