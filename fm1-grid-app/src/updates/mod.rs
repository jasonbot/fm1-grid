//! The platform boundary for automatic updates.
//!
//! Each target picks its own [`platform::Backend`]. The current backends are
//! deliberately stubs: updates are disabled, checking reports nothing, installing
//! is unavailable and restart is a no-op. The UI and the update state machine are
//! complete so a real platform backend can replace the stub without touching the
//! window chrome.

use crate::app::GridApp;
use gpui_kit::Context;

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
#[allow(async_fn_in_trait)]
pub(crate) trait PlatformUpdater {
    fn updates_enabled(&self) -> bool;
    async fn check_for_updates(&self) -> bool;
    fn can_update(&self) -> bool;
    #[allow(dead_code)]
    async fn install_update(
        &self,
        progress: &mut (dyn FnMut(UpdateProgress) + Send),
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

pub(crate) struct Updates {
    backend: platform::Backend,
    checking: bool,
    available: bool,
    updating: bool,
    restart_ready: bool,
    progress: Option<UpdateProgress>,
}

impl Updates {
    pub(crate) fn new() -> Self {
        Self {
            backend: platform::Backend,
            checking: false,
            available: false,
            updating: false,
            restart_ready: false,
            progress: None,
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

    pub(crate) fn activate(&mut self, cx: &mut Context<GridApp>) {
        match self.action() {
            UpdateAction::Check if !self.checking => self.check(cx),
            UpdateAction::Update if self.backend.can_update() => self.install(cx),
            UpdateAction::Restart => self.backend.restart_app(),
            _ => {}
        }
    }

    fn check(&mut self, cx: &mut Context<GridApp>) {
        if !self.backend.updates_enabled() {
            return;
        }
        self.checking = true;
        cx.notify();
        let backend = self.backend;
        cx.spawn(async move |this, cx| {
            let found = backend.check_for_updates().await;
            let _ = this.update(cx, |this, cx| {
                this.updates.checking = false;
                this.updates.available = found;
                cx.notify();
            });
        })
        .detach();
    }

    fn install(&mut self, cx: &mut Context<GridApp>) {
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
        // A real backend streams through the callback and then sets `restart_ready`;
        // the stubs never reach this branch because `can_update()` is false.
        let _ = &self.backend;
    }
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
