//! `SMAppService.mainApp`: the app registers itself as a login item (macOS 13+).
//!
//! Every function builds its own `SMAppService` handle, so nothing here has to be
//! `Send`: call these from a background thread (they talk to `smd` over XPC and can
//! block), never from the UI thread.

use objc2::rc::Retained;
use objc2_foundation::{NSError, NSURL};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use std::path::Path;

/// `SMAppService.Status`, as a plain value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmStatus {
    /// 0. Registered once, then unregistered (by the app, or removed in System Settings).
    NotRegistered,
    /// 1. Registered and allowed to run.
    Enabled,
    /// 2. Registered, but the user must allow it in System Settings.
    RequiresApproval,
    /// 3. macOS has no record. This is what a never-registered app reports,
    ///    and what an unbundled binary (`cargo run`) reports.
    NotFound,
    /// A value this code does not know.
    Other(isize),
}

impl SmStatus {
    fn from_raw(raw: SMAppServiceStatus) -> Self {
        match raw {
            SMAppServiceStatus::NotRegistered => SmStatus::NotRegistered,
            SMAppServiceStatus::Enabled => SmStatus::Enabled,
            SMAppServiceStatus::RequiresApproval => SmStatus::RequiresApproval,
            SMAppServiceStatus::NotFound => SmStatus::NotFound,
            other => SmStatus::Other(other.0),
        }
    }

    pub fn raw(self) -> isize {
        match self {
            SmStatus::NotRegistered => 0,
            SmStatus::Enabled => 1,
            SmStatus::RequiresApproval => 2,
            SmStatus::NotFound => 3,
            SmStatus::Other(n) => n,
        }
    }
}

/// An `NSError` from ServiceManagement, kept whole.
///
/// Keep the domain: recent macOS versions answer in `SMAppServiceErrorDomain` with raw
/// errno values (1 = EPERM "Operation not permitted", 22 = EINVAL), not with the
/// `kSMError*` constants of `SMErrors.h`, so a code alone is ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmError {
    pub domain: String,
    pub code: isize,
    pub description: String,
}

impl std::fmt::Display for SmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({} {})", self.description, self.domain, self.code)
    }
}

impl std::error::Error for SmError {}

impl SmError {
    fn from_ns(error: &NSError) -> Self {
        SmError {
            domain: error.domain().to_string(),
            code: error.code(),
            description: error.localizedDescription().to_string(),
        }
    }
}

/// `kSMErrorJobNotFound` in `SMErrors.h`.
const SM_ERROR_JOB_NOT_FOUND: isize = 6;
/// `kSMErrorAlreadyRegistered` in `SMErrors.h`.
const SM_ERROR_ALREADY_REGISTERED: isize = 12;

fn main_app() -> Retained<SMAppService> {
    // SAFETY: a class property with no arguments; it only builds a handle.
    unsafe { SMAppService::mainAppService() }
}

/// Reads the login item's state. Read-only.
pub fn status() -> SmStatus {
    // SAFETY: `status` takes no arguments and returns an integer enum.
    SmStatus::from_raw(unsafe { main_app().status() })
}

/// Registers the running app bundle as a login item.
///
/// Returns the status read back after the call: `Enabled` on success,
/// `RequiresApproval` when macOS accepted the request but the user must allow it.
/// Call it only from the installed bundle (see `bundle::InstallLocation`).
pub fn register() -> Result<SmStatus, SmError> {
    let service = main_app();
    // SAFETY: no arguments; the error comes back as an NSError.
    match unsafe { service.registerAndReturnError() } {
        Ok(()) => {}
        Err(error) => {
            let error = SmError::from_ns(&error);
            if error.code != SM_ERROR_ALREADY_REGISTERED {
                return Err(error);
            }
        }
    }
    // SAFETY: as in `status`.
    Ok(SmStatus::from_raw(unsafe { service.status() }))
}

/// Removes the login item. The running app keeps running.
///
/// "There was nothing to remove" is success: the header promises
/// `kSMErrorJobNotFound`, and recent macOS versions answer EPERM for an absent record.
pub fn unregister() -> Result<SmStatus, SmError> {
    let service = main_app();
    // SAFETY: as in `status`.
    let before = SmStatus::from_raw(unsafe { service.status() });
    // SAFETY: no arguments; the error comes back as an NSError.
    if let Err(error) = unsafe { service.unregisterAndReturnError() } {
        let error = SmError::from_ns(&error);
        let nothing_to_remove = error.code == SM_ERROR_JOB_NOT_FOUND
            || matches!(before, SmStatus::NotRegistered | SmStatus::NotFound);
        if !nothing_to_remove {
            return Err(error);
        }
    }
    // SAFETY: as in `status`.
    Ok(SmStatus::from_raw(unsafe { service.status() }))
}

/// What macOS says about a legacy LaunchAgent plist in `~/Library/LaunchAgents`.
/// `RequiresApproval` means the user switched it off in System Settings. Read-only.
pub fn status_for_legacy_plist(plist: &Path) -> SmStatus {
    let Some(url) = NSURL::from_file_path(plist) else {
        return SmStatus::NotFound;
    };
    // SAFETY: a class method that takes a valid file URL.
    SmStatus::from_raw(unsafe { SMAppService::statusForLegacyURL(&url) })
}

/// Opens System Settings > General > Login Items & Extensions.
pub fn open_login_items_settings() {
    // SAFETY: a class method with no arguments.
    unsafe { SMAppService::openSystemSettingsLoginItems() }
}
