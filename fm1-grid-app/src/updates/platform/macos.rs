//! macOS app updates: a build-time JSON feed points at a zipped `.app`.
//!
//! The feed is `{ "version": "1.2.3", "url": "https://…/FM1-Grid.zip" }`.
//! Checking downloads only the feed. Installing downloads the zip, extracts it
//! with `ditto` (which preserves the bundle's signature metadata) and stages
//! the `.app`. The running app is never modified in place: a detached `/bin/sh`
//! waits for this process to exit, swaps the bundle's `Contents` directory
//! (with a backup and rollback on failure), and reopens the app.
//!
//! A build without `MAC_APP_URL` keeps updates disabled. Updates require valid
//! signatures with the same Team ID on both the running app and the download;
//! unsigned or ad-hoc development bundles stay on their current build.

use crate::updates::macos_feed;
use crate::updates::{
    PlatformUpdater, ProgressCallback, UpdateOutcome, UpdateProgress, UpdateStage,
};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAC_APP_URL: Option<&str> = option_env!("MAC_APP_URL");

const MAX_FEED_BYTES: u64 = 64 * 1024;
const MAX_APP_BYTES: u64 = 512 * 1024 * 1024;
const FEED_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const USER_AGENT: &str = "FM1-Grid-Updater/1.0";

/// Runs after the GUI exits. `$1` is the old PID, `$2` the installed `.app`,
/// `$3` the staged `.app`, `$4` the temporary download directory. Moving
/// `Contents` preserves the bundle path and its writable parent. A trap restores
/// the old contents if the script is interrupted between the two moves.
const SWAP_SCRIPT: &str = r#"
while kill -0 "$1" 2>/dev/null; do /bin/sleep 0.2; done
APP=$2
NEW=$3
TMP=$4
BACKUP=$APP/Contents.fm1-old
installed=0
restore() {
    if [ "$installed" = 0 ] && [ -d "$BACKUP" ] && [ ! -d "$APP/Contents" ]; then
        /bin/mv "$BACKUP" "$APP/Contents"
    fi
}
trap 'restore' 0 1 2 15
[ -d "$APP/Contents" ] && [ -d "$NEW/Contents" ] || exit 1
/bin/rm -rf "$BACKUP"
/bin/mv "$APP/Contents" "$BACKUP" || exit 1
if ! /bin/mv "$NEW/Contents" "$APP/Contents"; then
    exit 1
fi
installed=1
/bin/rm -rf "$BACKUP" "$TMP"
/usr/bin/open "$APP"
"#;

#[derive(Clone, Debug, Default)]
pub(crate) struct Backend {
    staged: Arc<Mutex<Option<Staged>>>,
}

#[derive(Clone, Debug)]
struct Staged {
    app: PathBuf,
    bundle: PathBuf,
    temp: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    /// Network, extraction, or a transient filesystem problem.
    Retry,
    /// The feed or the downloaded bundle cannot be installed.
    Fatal,
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

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .https_only(true)
        .user_agent(USER_AGENT)
        .build()
        .into()
}

fn fetch_feed(url: &str) -> Option<macos_feed::Feed> {
    let mut response = agent(FEED_TIMEOUT).get(url).call().ok()?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .with_config()
        .limit(MAX_FEED_BYTES)
        .reader()
        .read_to_end(&mut bytes)
        .ok()?;
    macos_feed::parse(&bytes)
}

fn download(url: &str, destination: &Path, progress: &ProgressCallback) -> Result<(), Failure> {
    let mut response = agent(DOWNLOAD_TIMEOUT)
        .get(url)
        .call()
        .map_err(|_| Failure::Retry)?;
    let total = response.body().content_length();
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MAX_APP_BYTES)
        .reader();
    let mut file = BufWriter::new(File::create(destination).map_err(|_| Failure::Retry)?);
    let mut buffer = [0u8; 64 * 1024];
    let mut received = 0u64;
    report(progress, UpdateStage::Downloading, Some(0));
    loop {
        let read = reader.read(&mut buffer).map_err(|_| Failure::Retry)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|_| Failure::Retry)?;
        received += read as u64;
        if let Some(total) = total.filter(|total| *total > 0) {
            let percentage = (received.saturating_mul(100) / total).min(100) as i32;
            report(progress, UpdateStage::Downloading, Some(percentage));
        }
    }
    file.flush().map_err(|_| Failure::Retry)?;
    Ok(())
}

fn extract(zip: &Path, destination: &Path) -> Result<(), Failure> {
    let status = Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(destination)
        .status()
        .map_err(|_| Failure::Retry)?;
    if status.success() {
        Ok(())
    } else {
        Err(Failure::Fatal)
    }
}

/// Finds the first `.app` in the extracted archive, without following symlinks.
fn find_app(directory: &Path, depth: u8) -> Option<PathBuf> {
    let entries = fs::read_dir(directory).ok()?;
    for entry in entries.flatten() {
        let file_type = entry.file_type().ok()?;
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "app") {
            return Some(path);
        }
        if depth > 0 {
            if let Some(found) = find_app(&path, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn plist_value(app: &Path, key: &str) -> Option<String> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn bundle_version(app: &Path) -> Option<String> {
    plist_value(app, "CFBundleShortVersionString")
}

fn bundle_identifier(app: &Path) -> Option<String> {
    plist_value(app, "CFBundleIdentifier")
}

fn team_identifier(app: &Path) -> Option<String> {
    let output = Command::new("/usr/bin/codesign")
        .arg("-dvv")
        .arg(app)
        .output()
        .ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr.lines().find_map(|line| {
        let value = line.strip_prefix("TeamIdentifier=")?.trim();
        (!value.is_empty() && value != "not set").then(|| value.to_owned())
    })
}

fn signature_valid(app: &Path) -> bool {
    Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .status()
        .is_ok_and(|status| status.success())
}

/// An update must be signed by the same team as the running app. Unsigned and
/// ad-hoc builds have no team to compare, so they cannot update themselves.
fn signature_matches(current: &Path, update: &Path) -> bool {
    if !signature_valid(current) || !signature_valid(update) {
        return false;
    }
    match (team_identifier(current), team_identifier(update)) {
        (Some(current), Some(update)) => current == update,
        _ => false,
    }
}

fn valid_bundle(current: &Path, update: &Path, expected_version: &str) -> bool {
    let Some(version) = bundle_version(update) else {
        return false;
    };
    if !macos_feed::same_version(&version, expected_version) {
        return false;
    }
    match (bundle_identifier(current), bundle_identifier(update)) {
        (Some(current), Some(update)) if current == update => {}
        _ => return false,
    }
    signature_matches(current, update)
}

fn app_bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let macos = executable.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    if app.extension()? != "app" || app.to_string_lossy().contains("/AppTranslocation/") {
        return None;
    }
    Some(app.to_path_buf())
}

fn current_version(app: &Path) -> String {
    bundle_version(app).unwrap_or_else(|| crate::APP_VERSION.to_owned())
}

fn temporary_directory() -> Option<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("fm1-grid-update-{}-{nonce}", std::process::id()));
    fs::create_dir(&path).ok()?;
    Some(path)
}

impl PlatformUpdater for Backend {
    fn updates_enabled(&self) -> bool {
        MAC_APP_URL.is_some() && app_bundle().is_some()
    }

    async fn check_for_updates(&self, _window: Option<isize>) -> bool {
        let Some(app) = app_bundle() else {
            return false;
        };
        let Some(feed) = MAC_APP_URL.and_then(fetch_feed) else {
            return false;
        };
        macos_feed::is_newer(&feed.version, &current_version(&app))
    }

    fn can_update(&self) -> bool {
        self.updates_enabled()
    }

    async fn install_update(
        &self,
        _window: Option<isize>,
        progress: ProgressCallback,
    ) -> UpdateOutcome {
        let Some(app) = app_bundle() else {
            return UpdateOutcome::Fail;
        };
        let Some(feed) = MAC_APP_URL.and_then(fetch_feed) else {
            return UpdateOutcome::TryAgain;
        };
        if !macos_feed::is_newer(&feed.version, &current_version(&app)) {
            return UpdateOutcome::Fail;
        }

        let Some(temp) = temporary_directory() else {
            return UpdateOutcome::TryAgain;
        };
        let zip = temp.join("update.zip");
        let extracted = temp.join("extracted");
        let result = (|| {
            download(&feed.url, &zip, &progress)?;
            fs::create_dir(&extracted).map_err(|_| Failure::Retry)?;
            report(&progress, UpdateStage::Installing, None);
            extract(&zip, &extracted)?;
            let update = find_app(&extracted, 2).ok_or(Failure::Fatal)?;
            if !valid_bundle(&app, &update, &feed.version) {
                return Err(Failure::Fatal);
            }
            Ok((update, temp.clone()))
        })();

        match result {
            Ok((bundle, temp)) => {
                let Ok(mut staged) = self.staged.lock() else {
                    let _ = fs::remove_dir_all(&temp);
                    return UpdateOutcome::Fail;
                };
                *staged = Some(Staged { app, bundle, temp });
                report(&progress, UpdateStage::Processing, Some(100));
                UpdateOutcome::Success
            }
            Err(Failure::Fatal) => {
                let _ = fs::remove_dir_all(&temp);
                UpdateOutcome::Fail
            }
            Err(Failure::Retry) => {
                let _ = fs::remove_dir_all(&temp);
                UpdateOutcome::TryAgain
            }
        }
    }

    fn restart_app(&self) -> bool {
        let Ok(mut slot) = self.staged.lock() else {
            return false;
        };
        let Some(staged) = slot.take() else {
            return false;
        };
        let launched = Command::new("/bin/sh")
            .arg("-c")
            .arg(SWAP_SCRIPT)
            .arg("fm1-grid-updater")
            .arg(std::process::id().to_string())
            .arg(&staged.app)
            .arg(&staged.bundle)
            .arg(&staged.temp)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .is_ok();
        if !launched {
            *slot = Some(staged);
        }
        launched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "fm1-grid-mac-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn finds_a_nested_app_bundle() {
        let root = temp_dir("find-app");
        let nested = root.join("payload").join("FM1 Grid.app");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_app(&root, 2), Some(nested));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsigned_apps_cannot_update() {
        let root = temp_dir("unsigned");
        let current = root.join("Current.app");
        let update = root.join("Update.app");
        fs::create_dir_all(&current).unwrap();
        fs::create_dir_all(&update).unwrap();
        assert!(!signature_matches(&current, &update));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn swap_restores_the_old_contents_on_failure() {
        let root = temp_dir("swap");
        let app = root.join("FM1 Grid.app");
        let new = root.join("staged.app");
        let temp = root.join("download");
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::create_dir_all(new.join("Contents")).unwrap();
        fs::create_dir_all(&temp).unwrap();
        fs::write(app.join("Contents/old"), b"old").unwrap();
        fs::write(new.join("Contents/new"), b"new").unwrap();

        // Make the second move fail after the first move has already happened.
        let broken = SWAP_SCRIPT.replace(
            "/bin/mv \"$NEW/Contents\" \"$APP/Contents\"",
            "/bin/false",
        );
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(broken)
            .arg("fm1-grid-updater-test")
            .arg(i32::MAX.to_string())
            .arg(&app)
            .arg(&new)
            .arg(&temp)
            .status()
            .unwrap();
        assert!(!status.success());
        assert_eq!(
            fs::read_to_string(app.join("Contents/old")).unwrap(),
            "old",
            "the old Contents must be restored"
        );
        assert!(!app.join("Contents.fm1-old").exists());
        let _ = fs::remove_dir_all(root);
    }
}
