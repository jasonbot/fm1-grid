//! Windows updates: the Store path for Store-distributed packages, and a
//! hosted-MSIX path for sideloaded (vanilla) MSIX installs.
//!
//! Only a packaged app has package identity. Unpackaged builds keep updates
//! disabled. For a packaged app:
//!
//! - a Store signature keeps the `Windows.Services.Store` consent flow;
//! - any other signature reads `AppxManifest.xml` from the MSIX hosted at
//!   `FM1_GRID_MSIX_URL` (a build-time environment variable, compiled in only
//!   when set) and deploys it with `Windows.Management.Deployment`.
//!
//! The hosted archive is an OPC ZIP: the check fetches only the archive tail,
//! the manifest's local header and the manifest bytes, so an hourly check does
//! not download the whole package.

use super::msix::{self, HostedUrl, Version};
use crate::updates::{
    PlatformUpdater, ProgressCallback, UpdateOutcome, UpdateProgress, UpdateStage,
};
use std::ffi::c_void;
use std::sync::LazyLock;
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::ApplicationModel::{Package, PackageSignatureKind};
use windows::Foundation::Uri;
use windows::Management::Deployment::{
    AddPackageOptions, DeploymentProgress, DeploymentProgressState, DeploymentResult,
    PackageManager,
};
use windows::Services::Store::{StoreContext, StorePackageUpdateResult, StorePackageUpdateState};
use windows::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, HWND};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts, INTERNET_DEFAULT_HTTPS_PORT, INTERNET_DEFAULT_HTTP_PORT,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_REDIRECT_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS, WINHTTP_QUERY_CUSTOM, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::System::Recovery::{
    RegisterApplicationRestart, UnregisterApplicationRestart, RESTART_NO_CRASH, RESTART_NO_HANG,
    RESTART_NO_REBOOT,
};
use windows::Win32::UI::Shell::IInitializeWithWindow;
use windows_future::AsyncOperationProgressHandler;

/// The hosted vanilla-MSIX URL. `build.rs` emits `cargo:rustc-env` only when the
/// variable is set, so this is `None` in ordinary builds.
const HOSTED_MSIX_URL: Option<&str> = option_env!("FM1_GRID_MSIX_URL");

/// The ZIP tail must hold the end-of-central-directory record and the central
/// directory. An MSIX has a handful of members, so 64 KiB is ample.
const TAIL_BYTES: u64 = 64 * 1024;

/// The largest archive we are willing to buffer when a server ignores `Range`.
const MAX_BUFFERED_ARCHIVE: usize = 64 * 1024 * 1024;

const USER_AGENT: &str = "FM1-Grid-Updater/1.0";

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Backend;

#[derive(Clone, Copy, Debug)]
struct Identity {
    version: Version,
    store: bool,
}

enum Mode {
    Store,
    Hosted(HostedUrl),
    Disabled,
}

fn packaged() -> bool {
    let mut length = 0u32;
    // A null buffer asks for the required size. An unpackaged process gets
    // APPMODEL_ERROR_NO_PACKAGE; a packaged one gets ERROR_INSUFFICIENT_BUFFER.
    let result = unsafe { GetCurrentPackageFullName(&mut length, None) };
    result != APPMODEL_ERROR_NO_PACKAGE
}

fn identity() -> Option<Identity> {
    // Package identity is fixed for the life of the process; `action()` and the
    // auto-check consult this every frame.
    static IDENTITY: LazyLock<Option<Identity>> = LazyLock::new(|| {
        if !packaged() {
            return None;
        }
        let package = Package::Current().ok()?;
        let version = package.Id().ok()?.Version().ok()?;
        Some(Identity {
            version: Version::new(
                version.Major,
                version.Minor,
                version.Build,
                version.Revision,
            ),
            store: package.SignatureKind().ok() == Some(PackageSignatureKind::Store),
        })
    });
    *IDENTITY
}

fn hosted_url() -> Option<&'static str> {
    HOSTED_MSIX_URL.map(str::trim).filter(|url| !url.is_empty())
}

fn mode() -> &'static Mode {
    // Both the package identity and the compile-time URL are fixed, so parse
    // once instead of on every frame's `action()`.
    static MODE: LazyLock<Mode> = LazyLock::new(|| {
        let Some(identity) = identity() else {
            return Mode::Disabled;
        };
        if identity.store {
            return Mode::Store;
        }
        match hosted_url().and_then(HostedUrl::parse) {
            Some(url) => Mode::Hosted(url),
            None => Mode::Disabled,
        }
    });
    &MODE
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

// ---------------------------------------------------------------------------
// Store path
// ---------------------------------------------------------------------------

fn context(window: Option<isize>) -> Option<StoreContext> {
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

type StoreStatus = windows::Services::Store::StorePackageUpdateStatus;

fn attach_store_progress(
    operation: &windows_future::IAsyncOperationWithProgress<StorePackageUpdateResult, StoreStatus>,
    progress: ProgressCallback,
) {
    let handler = AsyncOperationProgressHandler::new(
        move |_, status: windows::core::Ref<'_, StoreStatus>| {
            let pct = (status.PackageDownloadProgress * 100.0).round() as i32;
            report(&progress, UpdateStage::Downloading, Some(pct.clamp(0, 100)));
            Ok(())
        },
    );
    let _ = operation.SetProgress(&handler);
}

fn store_state(result: &StorePackageUpdateResult) -> Option<StorePackageUpdateState> {
    result.OverallState().ok()
}

async fn store_check(window: Option<isize>) -> bool {
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

async fn store_install(window: Option<isize>, progress: ProgressCallback) -> UpdateOutcome {
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
        attach_store_progress(&download, progress.clone());
        let Ok(result) = download.await else {
            return UpdateOutcome::Fail;
        };
        if store_state(&result) != Some(StorePackageUpdateState::Completed) {
            return UpdateOutcome::Fail;
        }
        downloaded(&progress);
        report(&progress, UpdateStage::Installing, None);
        let Ok(install) = context.TrySilentDownloadAndInstallStorePackageUpdatesAsync(&updates)
        else {
            return UpdateOutcome::Fail;
        };
        attach_store_progress(&install, progress.clone());
        return match install.await {
            Ok(result) if store_state(&result) == Some(StorePackageUpdateState::Completed) => {
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
    attach_store_progress(&install, progress.clone());
    match install.await {
        Ok(result) if store_state(&result) == Some(StorePackageUpdateState::Completed) => {
            report(&progress, UpdateStage::Processing, Some(100));
            UpdateOutcome::Success
        }
        Ok(_) => UpdateOutcome::Fail,
        Err(_) => UpdateOutcome::TryAgain,
    }
}

// ---------------------------------------------------------------------------
// Hosted MSIX path: check
// ---------------------------------------------------------------------------

struct Response {
    status: u32,
    body: Vec<u8>,
    content_range: Option<(u64, u64, u64)>,
}

/// A WinHTTP request. `range` is an inclusive byte range; `limit` caps the body
/// buffered when the server ignores the range and answers with the whole file.
fn http_request(
    url: &HostedUrl,
    verb: &str,
    range: Option<(u64, u64)>,
    limit: usize,
) -> Option<Response> {
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }

    let agent = wide(USER_AGENT);
    let open = |access_type| unsafe {
        WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            access_type,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    };
    let mut session = open(WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY);
    if session.is_null() {
        // Automatic proxy resolution needs Windows 8.1+; fall back to the
        // per-user proxy settings on anything older.
        session = open(WINHTTP_ACCESS_TYPE_DEFAULT_PROXY);
    }
    let session = Handle(session);
    if session.0.is_null() {
        return None;
    }
    unsafe {
        let _ = WinHttpSetTimeouts(session.0, 5_000, 10_000, 15_000, 30_000);
        let redirect = WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS.to_ne_bytes();
        let _ = WinHttpSetOption(
            Some(session.0.cast_const()),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&redirect),
        );
    }

    let host = wide(&url.host);
    let port = if url.port != 0 {
        url.port
    } else if url.secure {
        INTERNET_DEFAULT_HTTPS_PORT
    } else {
        INTERNET_DEFAULT_HTTP_PORT
    };
    let connection = Handle(unsafe { WinHttpConnect(session.0, PCWSTR(host.as_ptr()), port, 0) });
    if connection.0.is_null() {
        return None;
    }

    let verb = wide(verb);
    let path = wide(&url.path);
    let flags = if url.secure {
        WINHTTP_FLAG_SECURE
    } else {
        WINHTTP_OPEN_REQUEST_FLAGS(0)
    };
    let request = Handle(unsafe {
        WinHttpOpenRequest(
            connection.0,
            PCWSTR(verb.as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        )
    });
    if request.0.is_null() {
        return None;
    }

    let headers: Option<Vec<u16>> = range.map(|(start, end)| {
        format!("Range: bytes={start}-{end}\r\n")
            .encode_utf16()
            .collect()
    });
    unsafe {
        WinHttpSendRequest(request.0, headers.as_deref(), None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).ok()?;
    }

    let mut status = 0u32;
    let mut length = std::mem::size_of::<u32>() as u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut length,
            std::ptr::null_mut(),
        )
        .ok()?;
    }

    let mut content_range = None;
    let mut range_header = [0u16; 96];
    let mut range_length = (range_header.len() * 2) as u32;
    let name: Vec<u16> = "Content-Range\0".encode_utf16().collect();
    let queried = unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_CUSTOM,
            PCWSTR(name.as_ptr()),
            Some(range_header.as_mut_ptr().cast()),
            &mut range_length,
            std::ptr::null_mut(),
        )
    };
    if queried.is_ok() {
        let value = String::from_utf16_lossy(&range_header[..range_length as usize / 2]);
        content_range = msix::parse_content_range(&value);
    }

    let mut body = Vec::new();
    loop {
        let mut available = 0u32;
        unsafe {
            WinHttpQueryDataAvailable(request.0, &mut available).ok()?;
        }
        if available == 0 {
            break;
        }
        let start = body.len();
        if start + available as usize > limit {
            return None;
        }
        body.resize(start + available as usize, 0);
        let mut read = 0u32;
        let read_result = unsafe {
            WinHttpReadData(
                request.0,
                body[start..].as_mut_ptr().cast(),
                available,
                &mut read,
            )
        };
        body.truncate(start + read as usize);
        read_result.ok()?;
        if read == 0 {
            break;
        }
    }

    Some(Response {
        status,
        body,
        content_range,
    })
}

/// Fetches an exact byte range, coping with a server that answers 200 with the
/// whole archive instead.
fn fetch_range(url: &HostedUrl, start: u64, length: u64) -> Option<Vec<u8>> {
    let last = start.checked_add(length)?.checked_sub(1)?;
    let response = http_request(url, "GET", Some((start, last)), MAX_BUFFERED_ARCHIVE)?;
    match response.status {
        206 => {
            let (got_start, got_end, _) = response.content_range?;
            (got_start == start && got_end == last && response.body.len() as u64 == length)
                .then_some(response.body)
        }
        200 => {
            let start = usize::try_from(start).ok()?;
            let end = usize::try_from(last).ok()?.checked_add(1)?;
            response.body.get(start..end).map(<[u8]>::to_vec)
        }
        _ => None,
    }
}

fn hosted_manifest_version(url: &HostedUrl) -> Option<Version> {
    let response = http_request(url, "GET", Some((0, TAIL_BYTES - 1)), MAX_BUFFERED_ARCHIVE)?;
    match response.status {
        200 => msix::manifest_version(&response.body),
        206 => {
            let (start, end, _total) = response.content_range?;
            if response.body.len() as u64 != end.checked_sub(start)?.checked_add(1)? {
                return None;
            }
            let entry = msix::find_entry(&response.body, start, msix::MANIFEST_NAME)?;
            let local = fetch_range(
                url,
                entry.local_header_offset,
                msix::LOCAL_HEADER_LEN as u64,
            )?;
            let (data_start, last) = entry.data_range(&local)?;
            let data = fetch_range(
                url,
                data_start,
                last.checked_sub(data_start)?.checked_add(1)?,
            )?;
            msix::identity_version(&msix::read_compressed(entry, &data)?)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Hosted MSIX path: install
// ---------------------------------------------------------------------------

fn arm_restart() -> bool {
    // The deployment force-shuts the process down. RegisterApplicationRestart
    // is what brings the app back after the OS relaunches it.
    unsafe {
        RegisterApplicationRestart(
            PCWSTR::null(),
            RESTART_NO_CRASH | RESTART_NO_HANG | RESTART_NO_REBOOT,
        )
        .is_ok()
    }
}

fn attach_deployment_progress(
    operation: &windows_future::IAsyncOperationWithProgress<DeploymentResult, DeploymentProgress>,
    progress: ProgressCallback,
) {
    let handler = AsyncOperationProgressHandler::new(
        move |_, value: windows::core::Ref<'_, DeploymentProgress>| {
            let stage = if value.state == DeploymentProgressState::Processing {
                UpdateStage::Installing
            } else {
                UpdateStage::Downloading
            };
            report(&progress, stage, Some(value.percentage.min(100) as i32));
            Ok(())
        },
    );
    let _ = operation.SetProgress(&handler);
}

async fn hosted_install(url: &str, progress: ProgressCallback) -> UpdateOutcome {
    let Ok(manager) = PackageManager::new() else {
        return UpdateOutcome::Fail;
    };
    let Ok(options) = AddPackageOptions::new() else {
        return UpdateOutcome::Fail;
    };
    if options.SetForceUpdateFromAnyVersion(true).is_err()
        || options.SetForceAppShutdown(true).is_err()
    {
        return UpdateOutcome::Fail;
    }

    report(&progress, UpdateStage::Downloading, None);
    let restart_armed = arm_restart();

    let install = (|| -> windows::core::Result<_> {
        let uri = Uri::CreateUri(&HSTRING::from(url))?;
        manager.AddPackageByUriAsync(&uri, &options)
    })();
    let Ok(install) = install else {
        if restart_armed {
            unsafe {
                let _ = UnregisterApplicationRestart();
            }
        }
        return UpdateOutcome::Fail;
    };
    attach_deployment_progress(&install, progress.clone());

    match install.await {
        Ok(result) if result.IsRegistered() == Ok(true) => {
            report(&progress, UpdateStage::Processing, Some(100));
            UpdateOutcome::Success
        }
        Ok(_) => {
            // The package did not register: retrying the same artifact will not
            // help. Leave the restart armed only when the OS already owns it.
            if restart_armed {
                unsafe {
                    let _ = UnregisterApplicationRestart();
                }
            }
            UpdateOutcome::Fail
        }
        Err(_) => {
            if restart_armed {
                unsafe {
                    let _ = UnregisterApplicationRestart();
                }
            }
            UpdateOutcome::TryAgain
        }
    }
}

impl PlatformUpdater for Backend {
    fn updates_enabled(&self) -> bool {
        !matches!(mode(), Mode::Disabled)
    }

    async fn check_for_updates(&self, window: Option<isize>) -> bool {
        match mode() {
            Mode::Store => store_check(window).await,
            Mode::Hosted(url) => {
                let Some(local) = identity() else {
                    return false;
                };
                hosted_manifest_version(url).is_some_and(|remote| remote > local.version)
            }
            Mode::Disabled => false,
        }
    }

    fn can_update(&self) -> bool {
        self.updates_enabled()
    }

    async fn install_update(
        &self,
        window: Option<isize>,
        progress: ProgressCallback,
    ) -> UpdateOutcome {
        match mode() {
            Mode::Store => store_install(window, progress).await,
            Mode::Hosted(_) => match hosted_url() {
                Some(url) => hosted_install(url, progress).await,
                None => UpdateOutcome::Fail,
            },
            Mode::Disabled => UpdateOutcome::Fail,
        }
    }

    fn restart_app(&self) -> bool {
        // Store and MSIX both relaunch through the Windows Restart Manager.
        // Arm it here as well as during MSIX install so a Restart click after a
        // Store update cannot simply close the app.
        match mode() {
            Mode::Store | Mode::Hosted(_) => arm_restart(),
            Mode::Disabled => false,
        }
    }
}
