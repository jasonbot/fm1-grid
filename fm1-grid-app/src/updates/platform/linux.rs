//! Linux Flatpak updates through the host's `flatpak(1)`.
//!
//! A sandbox cannot install into its own deployment, so every host command runs
//! through `flatpak-spawn --host` (the portal allows this only with the
//! `org.freedesktop.Flatpak` bus grant). Checking asks the host for pending
//! updates, installing runs `flatpak update`, and restarting arms a detached
//! host-side watcher that waits for this sandbox to exit and then runs the new
//! deploy. Non-Flatpak builds keep updates disabled.
//!
//! `flatpak update` stages the new deploy atomically; the running sandbox keeps
//! its old bits until the app is relaunched. Failures are mapped to the UI's
//! outcomes: an unusable helper (`flatpak-spawn` missing) is `Fail`, a rejected
//! command is `TryAgain`.

use crate::updates::{
    PlatformUpdater, ProgressCallback, UpdateOutcome, UpdateProgress, UpdateStage,
};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::sync::LazyLock;

const FLATPAK_INFO: &str = "/.flatpak-info";

/// Host-side relaunch: poll `flatpak ps` for this sandbox's instance id to
/// disappear, then start the new deploy. The sandbox PID namespace makes
/// `process::id()` meaningless on the host, so the instance id is the handle.
/// The wait is bounded so a cancelled quit does not leave a watcher forever.
const RELAUNCH_WATCHER_SCRIPT: &str = "for _ in $(seq 1 600); do\n\tflatpak ps --columns=instance | grep -qx \"$1\" || break\n\tsleep 1\ndone\nflatpak ps --columns=instance | grep -qx \"$1\" && exit 0\nexec flatpak run \"$2\"";

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Backend;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scope {
    User,
    System,
}

/// What `/.flatpak-info` (and `FLATPAK_ID`) tell us about this sandbox.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Instance {
    app_id: Option<String>,
    instance_id: Option<String>,
    scope: Option<Scope>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    /// The helper itself could not run; retrying cannot help.
    Unavailable,
    /// The command ran but reported failure (network, remote, deploy).
    Command,
}

fn detected() -> Option<&'static Instance> {
    static INSTANCE: LazyLock<Option<Instance>> = LazyLock::new(|| {
        let info = fs::read_to_string(FLATPAK_INFO).ok();
        instance_from(std::env::var("FLATPAK_ID").ok().as_deref(), info.as_deref())
    });
    INSTANCE.as_ref()
}

/// Detects a Flatpak sandbox from the environment variable and the metadata
/// file. `FLATPAK_ID` wins for the app id; the file supplies the instance id
/// and installation scope.
fn instance_from(env_app_id: Option<&str>, info_text: Option<&str>) -> Option<Instance> {
    let mut instance = info_text.map(parse_flatpak_info).unwrap_or_default();
    let env_app_id = env_app_id.map(str::trim).filter(|id| !id.is_empty());
    if let Some(app_id) = env_app_id {
        instance.app_id = Some(app_id.to_owned());
    }
    instance.app_id.as_ref()?;
    Some(instance)
}

/// Parses the INI-shaped `/.flatpak-info` for the keys the updater needs.
fn parse_flatpak_info(text: &str) -> Instance {
    let mut instance = Instance::default();
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = name.trim();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match (section, key) {
            ("Application", "name") => instance.app_id = Some(value.to_owned()),
            ("Instance", "instance-id") => instance.instance_id = Some(value.to_owned()),
            ("Instance", "app-path") => instance.scope = scope_from_path(value),
            _ => {}
        }
    }
    instance
}

/// A sandbox's `app-path` is the host deployment path, which names the
/// installation the app came from.
fn scope_from_path(path: &str) -> Option<Scope> {
    if path.contains("/.local/share/flatpak/") {
        Some(Scope::User)
    } else if path.contains("/var/lib/flatpak/") {
        Some(Scope::System)
    } else {
        None
    }
}

fn scope_args(scope: Option<Scope>) -> &'static [&'static str] {
    match scope {
        Some(Scope::User) => &["--user"],
        Some(Scope::System) => &["--system"],
        None => &[],
    }
}

fn host_flatpak(scope: Option<Scope>, args: &[&str]) -> Result<Output, Failure> {
    Command::new("flatpak-spawn")
        .arg("--host")
        .arg("flatpak")
        .args(scope_args(scope))
        .args(args)
        .output()
        .map_err(|_| Failure::Unavailable)
        .and_then(|output| {
            if output.status.success() {
                Ok(output)
            } else {
                Err(Failure::Command)
            }
        })
}

/// Finds the app's row in `flatpak remote-ls --updates
/// --columns=application,version` output. `None` means no pending update; the
/// inner `None` means the remote doesn't publish a version.
fn parse_remote_ls_update(stdout: &str, app_id: &str) -> Option<Option<String>> {
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (id, version) = line.split_once('\t').unwrap_or((line, ""));
        if id != app_id {
            continue;
        }
        let version = version.trim();
        return Some((!version.is_empty()).then(|| version.to_owned()));
    }
    None
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

impl PlatformUpdater for Backend {
    fn updates_enabled(&self) -> bool {
        detected().is_some()
    }

    async fn check_for_updates(&self, _window: Option<isize>) -> bool {
        let Some(instance) = detected() else {
            return false;
        };
        let Some(app_id) = instance.app_id.as_deref() else {
            return false;
        };
        let Ok(output) = host_flatpak(
            instance.scope,
            &["remote-ls", "--updates", "--columns=application,version"],
        ) else {
            return false;
        };
        parse_remote_ls_update(&String::from_utf8_lossy(&output.stdout), app_id).is_some()
    }

    fn can_update(&self) -> bool {
        self.updates_enabled()
    }

    async fn install_update(
        &self,
        _window: Option<isize>,
        progress: ProgressCallback,
    ) -> UpdateOutcome {
        let Some(instance) = detected() else {
            return UpdateOutcome::Fail;
        };
        let Some(app_id) = instance.app_id.as_deref() else {
            return UpdateOutcome::Fail;
        };

        // `flatpak update` downloads and deploys in one step; there is no
        // per-byte progress on the host command, so report indeterminate until
        // it returns.
        report(&progress, UpdateStage::Downloading, None);
        match host_flatpak(instance.scope, &["update", "--assumeyes", app_id]) {
            Ok(_) => {
                report(&progress, UpdateStage::Installing, Some(100));
                report(&progress, UpdateStage::Processing, Some(100));
                UpdateOutcome::Success
            }
            Err(Failure::Unavailable) => UpdateOutcome::Fail,
            Err(Failure::Command) => UpdateOutcome::TryAgain,
        }
    }

    fn restart_app(&self) -> bool {
        // The sandbox cannot recreate itself: hand a detached watcher to the
        // host before the UI loop quits. Without an instance id the update
        // still applies on the next manual launch, but the app must not quit.
        let Some(instance) = detected() else {
            return false;
        };
        let (Some(instance_id), Some(app_id)) = (&instance.instance_id, &instance.app_id) else {
            return false;
        };
        Command::new("flatpak-spawn")
            .args([
                "--host",
                "bash",
                "-c",
                RELAUNCH_WATCHER_SCRIPT,
                "fm1-grid-relaunch-watcher",
            ])
            .args([instance_id, app_id])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "\
[Application]
name=org.fm1.Grid
runtime=org.gnome.Platform/x86_64/50
runtime-path=/var/lib/flatpak/runtime/org.gnome.Platform/x86_64/50/active

[Instance]
instance-id=12345
app-path=/var/lib/flatpak/app/org.fm1.Grid/x86_64/stable/active

[Session Bus Policy]
org.freedesktop.Flatpak=talk
";

    #[test]
    fn detects_flatpak_from_environment_or_metadata() {
        let from_env = instance_from(Some("org.fm1.Grid"), None).unwrap();
        assert_eq!(from_env.app_id.as_deref(), Some("org.fm1.Grid"));
        assert_eq!(from_env.instance_id, None);
        assert_eq!(from_env.scope, None);

        let from_info = instance_from(None, Some(INFO)).unwrap();
        assert_eq!(from_info.app_id.as_deref(), Some("org.fm1.Grid"));
        assert_eq!(from_info.instance_id.as_deref(), Some("12345"));
        assert_eq!(from_info.scope, Some(Scope::System));
    }

    #[test]
    fn non_flatpak_is_disabled() {
        assert_eq!(instance_from(None, None), None);
        assert_eq!(instance_from(Some("  "), None), None);
        assert_eq!(
            instance_from(None, Some("[Application]\nruntime=x\n")),
            None
        );
    }

    #[test]
    fn environment_app_id_wins_and_user_scope_is_recognized() {
        let info = INFO.replace(
            "app-path=/var/lib/flatpak",
            "app-path=/home/user/.local/share/flatpak",
        );
        let instance = instance_from(Some("org.other.App"), Some(&info)).unwrap();
        assert_eq!(instance.app_id.as_deref(), Some("org.other.App"));
        assert_eq!(instance.scope, Some(Scope::User));
        assert_eq!(scope_args(instance.scope), ["--user"]);
    }

    #[test]
    fn remote_ls_rows_are_matched_exactly() {
        let stdout = "org.gnome.Platform\t50\norg.fm1.Grid\t0.2.0\n";
        assert_eq!(
            parse_remote_ls_update(stdout, "org.fm1.Grid"),
            Some(Some("0.2.0".into()))
        );
        assert_eq!(
            parse_remote_ls_update("org.fm1.Grid\t\n", "org.fm1.Grid"),
            Some(None)
        );
        assert_eq!(
            parse_remote_ls_update("org.fm1.Grid.Sdk\t1\n", "org.fm1.Grid"),
            None
        );
        assert_eq!(parse_remote_ls_update("", "org.fm1.Grid"), None);
    }
}
