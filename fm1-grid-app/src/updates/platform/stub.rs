use crate::updates::{PlatformUpdater, UpdateOutcome, UpdateProgress};

/// The no-op updater used until a platform implementation exists.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Backend;

impl PlatformUpdater for Backend {
    fn updates_enabled(&self) -> bool {
        false
    }

    async fn check_for_updates(&self) -> bool {
        false
    }

    fn can_update(&self) -> bool {
        false
    }

    async fn install_update(
        &self,
        _progress: &mut (dyn FnMut(UpdateProgress) + Send),
    ) -> UpdateOutcome {
        UpdateOutcome::Fail
    }

    fn restart_app(&self) {}
}
