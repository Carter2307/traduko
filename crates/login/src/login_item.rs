//! The front door: `enable` / `disable` / `is_enabled` over both mechanisms.
//!
//! macOS owns the truth. Never cache "enabled" in the app's settings as the state;
//! read [`LoginItem::status`] whenever the settings UI is shown, because the user can
//! remove the item in System Settings > General > Login Items & Extensions at any time.

use crate::bundle::{self, InstallLocation};
use crate::launch_agent::LaunchAgent;
use crate::smapp::{self, SmError, SmStatus};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// `SMAppService.mainApp`: listed under "Open at Login" with the app's name and icon.
    MainApp,
    /// `~/Library/LaunchAgents/<label>.plist`: listed under "Background App Activity".
    LaunchAgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginStatus {
    /// The app starts at login, through this mechanism.
    Enabled(Backend),
    /// Nothing is registered.
    Disabled,
    /// Registered, but switched off in System Settings. Only the user can switch it
    /// back on: show a hint and call [`LoginItem::open_system_settings`].
    RequiresApproval(Backend),
}

#[derive(Debug)]
pub enum LoginItemError {
    /// The app does not run from `/Applications` or `~/Applications` (a build folder,
    /// `cargo run`, a translocated copy). Registering from there would pin that path.
    NotInstalled(InstallLocation),
    /// ServiceManagement refused; the NSError is kept whole.
    ServiceManagement(SmError),
    /// The LaunchAgent plist could not be written or removed.
    Io(io::Error),
}

impl std::fmt::Display for LoginItemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginItemError::NotInstalled(location) => {
                write!(
                    f,
                    "the app is not installed in an Applications folder ({location:?})"
                )
            }
            LoginItemError::ServiceManagement(error) => {
                write!(f, "macOS refused the login item: {error}")
            }
            LoginItemError::Io(error) => write!(f, "cannot update the launch agent: {error}"),
        }
    }
}

impl std::error::Error for LoginItemError {}

impl From<SmError> for LoginItemError {
    fn from(error: SmError) -> Self {
        LoginItemError::ServiceManagement(error)
    }
}

impl From<io::Error> for LoginItemError {
    fn from(error: io::Error) -> Self {
        LoginItemError::Io(error)
    }
}

/// Launch at login for the running app. Cheap to build; holds no system handle,
/// so it is `Send` and can be used from a background task.
#[derive(Debug, Clone)]
pub struct LoginItem {
    agent: LaunchAgent,
}

impl LoginItem {
    /// `agent` describes the fallback (label, executable path). Nothing is written yet.
    pub fn new(agent: LaunchAgent) -> Self {
        LoginItem { agent }
    }

    /// What macOS has on file right now. Read-only; may block briefly (XPC).
    pub fn status(&self) -> LoginStatus {
        match smapp::status() {
            SmStatus::Enabled => return LoginStatus::Enabled(Backend::MainApp),
            SmStatus::RequiresApproval => return LoginStatus::RequiresApproval(Backend::MainApp),
            _ => {}
        }
        if self.agent.is_installed() {
            return match smapp::status_for_legacy_plist(&self.agent.plist_path()) {
                SmStatus::RequiresApproval => LoginStatus::RequiresApproval(Backend::LaunchAgent),
                _ => LoginStatus::Enabled(Backend::LaunchAgent),
            };
        }
        LoginStatus::Disabled
    }

    pub fn is_enabled(&self) -> bool {
        matches!(self.status(), LoginStatus::Enabled(_))
    }

    /// Turns launch at login on through `backend`, and the other mechanism off so the
    /// app is never started twice. Returns the status read back from macOS.
    pub fn enable(&self, backend: Backend) -> Result<LoginStatus, LoginItemError> {
        let location = bundle::install_location();
        if !location.may_register() {
            return Err(LoginItemError::NotInstalled(location));
        }
        match backend {
            Backend::MainApp => {
                let status = smapp::register()?;
                self.agent.remove()?;
                Ok(match status {
                    SmStatus::Enabled => LoginStatus::Enabled(Backend::MainApp),
                    SmStatus::RequiresApproval => LoginStatus::RequiresApproval(Backend::MainApp),
                    // Accepted, yet not on file: report what macOS says, not what was asked.
                    _ => LoginStatus::Disabled,
                })
            }
            Backend::LaunchAgent => {
                self.agent.install()?;
                if matches!(
                    smapp::status(),
                    SmStatus::Enabled | SmStatus::RequiresApproval
                ) {
                    smapp::unregister()?;
                }
                Ok(self.status())
            }
        }
    }

    /// The primary mechanism, then the fallback when ServiceManagement refuses.
    /// The second value is the refusal that caused the fallback, for the log.
    pub fn enable_with_fallback(&self) -> Result<(LoginStatus, Option<SmError>), LoginItemError> {
        match self.enable(Backend::MainApp) {
            Ok(status) if status != LoginStatus::Disabled => Ok((status, None)),
            Ok(_) => Ok((self.enable(Backend::LaunchAgent)?, None)),
            Err(LoginItemError::ServiceManagement(refusal)) => {
                Ok((self.enable(Backend::LaunchAgent)?, Some(refusal)))
            }
            Err(other) => Err(other),
        }
    }

    /// Turns launch at login off, whichever mechanism holds it. Safe to call twice.
    /// The running app keeps running.
    pub fn disable(&self) -> Result<(), LoginItemError> {
        if matches!(
            smapp::status(),
            SmStatus::Enabled | SmStatus::RequiresApproval
        ) {
            smapp::unregister()?;
        }
        self.agent.remove()?;
        Ok(())
    }

    /// After an install or an update: make what is on file point at this copy.
    /// Rewrites a stale LaunchAgent plist (the app moved). Touches nothing else.
    pub fn repair(&self) -> Result<(), LoginItemError> {
        if self.agent.is_installed()
            && !self.agent.is_current()
            && bundle::install_location().may_register()
        {
            self.agent.install()?;
        }
        Ok(())
    }

    /// Opens System Settings > General > Login Items & Extensions.
    pub fn open_system_settings() {
        smapp::open_login_items_settings();
    }
}

/// What to do at every app start, given the user's preference and what macOS reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupAction {
    /// Leave everything as it is.
    Nothing,
    /// Call [`LoginItem::enable`], then store the current install fingerprint.
    Register,
    /// Store the current install fingerprint (the item is enabled for this copy).
    RecordFingerprint,
    /// The user removed the item in System Settings: set the preference to off.
    TurnPreferenceOff,
    /// The item is switched off in System Settings: tell the user where to allow it.
    ShowApprovalHint,
}

/// The start-up rule. Pure: no system call, so it is unit-tested.
///
/// * `wanted`: the app's own "launch at login" preference (the user's intent).
/// * `status`: [`LoginItem::status`] read just now.
/// * `recorded`: the install fingerprint stored when registration last succeeded.
/// * `current`: `bundle::install_fingerprint()` of the running copy.
///
/// It never registers against the user's wish: with the preference off it does
/// nothing, and it tells "the user removed the item" (same installed copy, item gone)
/// from "a reinstall lost the item" (new copy, item gone).
pub fn startup_action(
    wanted: bool,
    status: LoginStatus,
    recorded: Option<&str>,
    current: &str,
) -> StartupAction {
    if !wanted {
        return StartupAction::Nothing;
    }
    match status {
        LoginStatus::Enabled(_) if recorded == Some(current) => StartupAction::Nothing,
        LoginStatus::Enabled(_) => StartupAction::RecordFingerprint,
        LoginStatus::RequiresApproval(_) => StartupAction::ShowApprovalHint,
        LoginStatus::Disabled if recorded == Some(current) => StartupAction::TurnPreferenceOff,
        LoginStatus::Disabled => StartupAction::Register,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_startup_rule() {
        use LoginStatus::*;
        use StartupAction::*;
        let on = Enabled(Backend::MainApp);
        let held = RequiresApproval(Backend::MainApp);
        // Preference off: never touch anything, whatever macOS says.
        for status in [on, held, Disabled] {
            assert_eq!(startup_action(false, status, None, "copy-1"), Nothing);
        }
        // First run with the preference on: register.
        assert_eq!(startup_action(true, Disabled, None, "copy-1"), Register);
        // Registered and still on for this copy: nothing to do.
        assert_eq!(startup_action(true, on, Some("copy-1"), "copy-1"), Nothing);
        // An update kept the item: remember the new copy.
        assert_eq!(
            startup_action(true, on, Some("copy-1"), "copy-2"),
            RecordFingerprint
        );
        // An update lost the item: register again.
        assert_eq!(
            startup_action(true, Disabled, Some("copy-1"), "copy-2"),
            Register
        );
        // Same copy, item gone: the user removed it in System Settings. Respect that.
        assert_eq!(
            startup_action(true, Disabled, Some("copy-1"), "copy-1"),
            TurnPreferenceOff
        );
        // Switched off in System Settings: only the user can switch it back on.
        assert_eq!(
            startup_action(true, held, Some("copy-1"), "copy-1"),
            ShowApprovalHint
        );
    }

    fn item(tag: &str) -> (LoginItem, PathBuf) {
        let dir = crate::test_scratch(tag);
        let agent = LaunchAgent {
            label: "dev.spike.mascot.login".into(),
            program: PathBuf::from("/Applications/Mascot.app/Contents/MacOS/mascot"),
            args: Vec::new(),
            bundle_id: Some("dev.spike.mascot".into()),
            restart_on_crash: false,
            agents_dir: dir.clone(),
        };
        (LoginItem::new(agent), dir)
    }

    /// The test binary lives in `target/`, outside any bundle: both backends must
    /// refuse before they reach `register` or write a plist.
    #[test]
    fn enable_refuses_outside_an_applications_folder() {
        let (item, dir) = item("facade-refuse");
        for backend in [Backend::MainApp, Backend::LaunchAgent] {
            match item.enable(backend) {
                Err(LoginItemError::NotInstalled(InstallLocation::Unbundled)) => {}
                other => panic!("expected NotInstalled(Unbundled), got {other:?}"),
            }
        }
        assert!(matches!(
            item.enable_with_fallback(),
            Err(LoginItemError::NotInstalled(_))
        ));
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            0,
            "nothing was written"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Status and disable are safe from a test binary: status only reads, and disable
    /// only unregisters when macOS reports a registration (it reports none here).
    #[test]
    fn status_follows_the_plist_and_disable_is_idempotent() {
        let (item, dir) = item("facade-status");
        assert_eq!(item.status(), LoginStatus::Disabled);
        assert!(!item.is_enabled());
        item.agent.install().unwrap();
        assert_eq!(item.status(), LoginStatus::Enabled(Backend::LaunchAgent));
        item.disable().unwrap();
        assert_eq!(item.status(), LoginStatus::Disabled);
        item.disable().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
