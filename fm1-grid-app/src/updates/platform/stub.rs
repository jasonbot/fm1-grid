use crate::updates::{PlatformUpdater, UpdateOutcome};

/// The no-op updater used until a platform implementation exists.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Backend;

impl PlatformUpdater for Backend {
    fn updates_enabled(&self) -> bool {
        false
    }

    async fn check_for_updates(&self, _window: Option<isize>) -> bool {
        false
    }

    fn can_update(&self) -> bool {
        false
    }

    async fn install_update(
        &self,
        _window: Option<isize>,
        _progress: crate::updates::ProgressCallback,
    ) -> UpdateOutcome {
        UpdateOutcome::Fail
    }

    fn restart_app(&self) -> bool {
        false
    }
}
