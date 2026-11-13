//! Windows Store updates through WinRT's `Windows.Services.Store` APIs.
//!
//! Only a packaged app (MSIX/Store install) has package identity. Unpackaged
//! builds keep updates disabled. A Store app uses the normal Store consent flow
//! when a silent download is not allowed.

use crate::updates::{
    PlatformUpdater, ProgressCallback, UpdateOutcome, UpdateProgress, UpdateStage,
};
use windows::core::Interface;
use windows_future::AsyncOperationProgressHandler;
use windows::Services::Store::{StoreContext, StorePackageUpdateResult, StorePackageUpdateState};
use windows::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, HWND};
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::UI::Shell::IInitializeWithWindow;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Backend;

fn packaged() -> bool {
    let mut length = 0u32;
    // A null buffer asks for the required size. An unpackaged process gets
    // APPMODEL_ERROR_NO_PACKAGE; a packaged one gets ERROR_INSUFFICIENT_BUFFER.
    let result = unsafe { GetCurrentPackageFullName(&mut length, None) };
    result != APPMODEL_ERROR_NO_PACKAGE
}

fn context(window: Option<isize>) -> Option<StoreContext> {
    if !packaged() {
        return None;
    }
    let context = StoreContext::GetDefault().ok()?;
    if let Some(hwnd) = window {
        let initialize: IInitializeWithWindow = context.cast().ok()?;
        unsafe {
            initialize
                .Initialize(HWND(hwnd as *mut std::ffi::c_void))
                .ok()?;
        }
    }
    Some(context)
}

fn report(progress: &ProgressCallback, stage: UpdateStage, percentage: Option<i32>) {
    if let Ok(mut callback) = progress.lock() {
        callback(UpdateProgress {
            stage,
            message: None,
            percentage,
        });
    }
}

fn downloaded(progress: &ProgressCallback) {
    report(progress, UpdateStage::Downloading, Some(100));
}

fn attach_progress(
    operation: &windows_future::IAsyncOperationWithProgress<
        StorePackageUpdateResult,
        windows::Services::Store::StorePackageUpdateStatus,
    >,
    progress: ProgressCallback,
) {
    let handler = AsyncOperationProgressHandler::new(move |_, status: windows::core::Ref<'_, windows::Services::Store::StorePackageUpdateStatus>| {
        let pct = (status.PackageDownloadProgress * 100.0).round() as i32;
        report(&progress, UpdateStage::Downloading, Some(pct.clamp(0, 100)));
        Ok(())
    });
    let _ = operation.SetProgress(&handler);
}

fn state(result: &StorePackageUpdateResult) -> Option<StorePackageUpdateState> {
    result.OverallState().ok()
}

impl PlatformUpdater for Backend {
    fn updates_enabled(&self) -> bool {
        packaged()
    }

    async fn check_for_updates(&self, window: Option<isize>) -> bool {
        let Some(context) = context(window) else {
            return false;
        };
        let Ok(operation) = context.GetAppAndOptionalStorePackageUpdatesAsync() else {
            return false;
        };
        let Ok(updates) = operation.await else {
            return false;
        };
        updates.Size().is_ok_and(|count| count > 0)
    }

    fn can_update(&self) -> bool {
        packaged()
    }

    async fn install_update(
        &self,
        window: Option<isize>,
        progress: ProgressCallback,
    ) -> UpdateOutcome {
        let Some(context) = context(window) else {
            return UpdateOutcome::Fail;
        };
        let Ok(operation) = context.GetAppAndOptionalStorePackageUpdatesAsync() else {
            return UpdateOutcome::Fail;
        };
        let Ok(updates) = operation.await else {
            return UpdateOutcome::Fail;
        };
        if updates.Size().map_or(true, |count| count == 0) {
            return UpdateOutcome::TryAgain;
        }

        if context
            .CanSilentlyDownloadStorePackageUpdates()
            .unwrap_or(false)
        {
            let Ok(download) = context.TrySilentDownloadStorePackageUpdatesAsync(&updates) else {
                return UpdateOutcome::Fail;
            };
            attach_progress(&download, progress.clone());
            let Ok(result) = download.await else {
                return UpdateOutcome::Fail;
            };
            if state(&result) != Some(StorePackageUpdateState::Completed) {
                return UpdateOutcome::Fail;
            }
            downloaded(&progress);
            report(&progress, UpdateStage::Installing, None);
            let Ok(install) = context.TrySilentDownloadAndInstallStorePackageUpdatesAsync(&updates)
            else {
                return UpdateOutcome::Fail;
            };
            attach_progress(&install, progress.clone());
            return match install.await {
                Ok(result) if state(&result) == Some(StorePackageUpdateState::Completed) => {
                    report(&progress, UpdateStage::Processing, Some(100));
                    UpdateOutcome::Success
                }
                Ok(_) => UpdateOutcome::Fail,
                Err(_) => UpdateOutcome::TryAgain,
            };
        }

        report(&progress, UpdateStage::Downloading, None);
        let Ok(install) = context.RequestDownloadAndInstallStorePackageUpdatesAsync(&updates) else {
            return UpdateOutcome::Fail;
        };
        attach_progress(&install, progress.clone());
        match install.await {
            Ok(result) if state(&result) == Some(StorePackageUpdateState::Completed) => {
                report(&progress, UpdateStage::Processing, Some(100));
                UpdateOutcome::Success
            }
            Ok(_) => UpdateOutcome::Fail,
            Err(_) => UpdateOutcome::TryAgain,
        }
    }

    fn restart_app(&self) {}
}
