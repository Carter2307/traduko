//! Opening Coco at login. macOS holds the state; this keeps it in step with
//! what the user asked for. Every call here talks to a system service and
//! takes tens of milliseconds, so it runs off the main thread.

use coco_login::launch_agent::LaunchAgent;
use coco_login::{LoginItem, LoginStatus, StartupAction, bundle, startup_action};

use crate::settings::{BUNDLE_ID, Settings};

const AGENT_LABEL: &str = "com.github.carter2307.coco.login";

fn item() -> Option<LoginItem> {
    let exe = bundle::current_exe().ok()?;
    let mut agent = LaunchAgent::for_current_user(AGENT_LABEL, &exe).ok()?;
    agent.bundle_id = Some(BUNDLE_ID.into());
    Some(LoginItem::new(agent))
}

/// Where the login item stands, for the screen that asks for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// Nothing is registered: Coco has to ask.
    NotAsked,
    /// Coco opens at login.
    Allowed,
    /// Registered, but switched off in System Settings, where only the
    /// user can switch it back on.
    NeedsApproval,
    /// This copy is not in an Applications folder (`cargo run`, a build
    /// folder), and must never become the login item.
    Unavailable,
}

impl Permission {
    /// True when there is something for the user to allow.
    pub fn is_pending(self) -> bool {
        matches!(self, Permission::NotAsked | Permission::NeedsApproval)
    }
}

/// What macOS says now. `COCO_LOGIN_ITEM=ask|allowed|approval` answers in
/// its place, to capture each state of the onboarding from a development
/// build.
pub fn permission() -> Permission {
    match std::env::var("COCO_LOGIN_ITEM").as_deref() {
        Ok("ask") => return Permission::NotAsked,
        Ok("allowed") => return Permission::Allowed,
        Ok("approval") => return Permission::NeedsApproval,
        _ => {}
    }
    if !bundle::install_location().may_register() {
        return Permission::Unavailable;
    }
    match item().map(|item| item.status()) {
        Some(LoginStatus::Enabled(_)) => Permission::Allowed,
        Some(LoginStatus::RequiresApproval(_)) => Permission::NeedsApproval,
        Some(LoginStatus::Disabled) => Permission::NotAsked,
        None => Permission::Unavailable,
    }
}

/// Opens System Settings where the login items are.
pub fn open_system_settings() {
    LoginItem::open_system_settings();
}

/// What changed in the settings after looking at the login item.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Outcome {
    pub open_at_login: Option<bool>,
    pub registered_install: Option<Option<String>>,
    pub note: Option<String>,
}

impl Outcome {
    pub fn apply(self, settings: &mut Settings) -> bool {
        let before = settings.clone();
        if let Some(on) = self.open_at_login {
            settings.open_at_login = on;
        }
        if let Some(install) = self.registered_install {
            settings.registered_install = install;
        }
        *settings != before
    }
}

/// At start-up: registers the app when the user wants it and it is not yet,
/// and gives the preference up when the user removed the item in System
/// Settings. Does nothing for a copy that is not in an Applications folder
/// (`cargo run`, a build folder), which must never become the login item.
pub fn reconcile(wanted: bool, registered_install: Option<String>) -> Outcome {
    if !bundle::install_location().may_register() {
        return Outcome::default();
    }
    let (Some(item), Ok(current)) = (item(), bundle::install_fingerprint()) else {
        return Outcome::default();
    };
    let _ = item.repair();
    match startup_action(wanted, item.status(), registered_install.as_deref(), &current) {
        StartupAction::Nothing | StartupAction::ShowApprovalHint => Outcome::default(),
        StartupAction::RecordFingerprint => Outcome { registered_install: Some(Some(current)), ..Outcome::default() },
        StartupAction::TurnPreferenceOff => Outcome { open_at_login: Some(false), registered_install: Some(None), ..Outcome::default() },
        StartupAction::Register => register(&item, current),
    }
}

/// The switch in the panel.
pub fn set(on: bool) -> Outcome {
    let Some(item) = item() else {
        return Outcome { note: Some("cannot find the app on disk".into()), ..Outcome::default() };
    };
    if !on && !bundle::install_location().may_register() {
        // `cargo run` cannot remove the installed copy's login item: only
        // that copy can. The wish is recorded and the note says so.
        return Outcome {
            open_at_login: Some(false),
            note: Some("this is not the installed copy: turn it off from Coco in Applications".into()),
            ..Outcome::default()
        };
    }
    if !on {
        return match item.disable() {
            Ok(()) => Outcome { open_at_login: Some(false), registered_install: Some(None), ..Outcome::default() },
            Err(error) => Outcome { note: Some(error.to_string()), ..Outcome::default() },
        };
    }
    match bundle::install_fingerprint() {
        Ok(current) => {
            let mut outcome = register(&item, current);
            // The wish is kept even when the app is not installed yet: the
            // installed copy registers itself at its first start.
            outcome.open_at_login = Some(true);
            outcome
        }
        Err(error) => Outcome { open_at_login: Some(true), note: Some(error.to_string()), ..Outcome::default() },
    }
}

fn register(item: &LoginItem, current: String) -> Outcome {
    // The login item first; a launch agent if macOS refuses it.
    match item.enable_with_fallback().map(|(status, _refusal)| status) {
        Ok(LoginStatus::Enabled(_)) => Outcome { registered_install: Some(Some(current)), ..Outcome::default() },
        Ok(LoginStatus::RequiresApproval(_)) => {
            LoginItem::open_system_settings();
            Outcome { note: Some("allow Coco in System Settings, Login Items".into()), ..Outcome::default() }
        }
        Ok(LoginStatus::Disabled) => Outcome { note: Some("macOS accepted the login item but does not list it yet".into()), ..Outcome::default() },
        Err(error) => Outcome { note: Some(error.to_string()), ..Outcome::default() },
    }
}

/// `coco --login-item status|enable|disable`, for the install scripts.
pub fn command_line(argument: &str) -> i32 {
    match argument {
        "status" => {
            let status = item().map(|item| item.status());
            println!("{status:?} (install: {:?})", bundle::install_location());
            0
        }
        "enable" | "disable" => {
            let outcome = set(argument == "enable");
            println!("{outcome:?}");
            i32::from(outcome.note.is_some())
        }
        other => {
            eprintln!("unknown --login-item argument: {other} (status, enable or disable)");
            2
        }
    }
}
