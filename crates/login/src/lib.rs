//! Launch at login for an ad-hoc-signed macOS app, plus the small pieces around it:
//! where the bundle is, where its resources are, and a single-instance lock.
//!
//! Primary mechanism: `SMAppService.mainApp` ([`smapp`]).
//! Fallback: a per-user LaunchAgent plist ([`launch_agent`]).
//! Front door: [`LoginItem`].
//!
//! macOS only (the dependencies are declared for `cfg(target_os = "macos")`).

pub mod bundle;
pub mod launch_agent;
pub mod login_item;
pub mod single_instance;
pub mod smapp;

pub use login_item::{
    Backend, LoginItem, LoginItemError, LoginStatus, StartupAction, startup_action,
};

/// A fresh, empty folder under this crate's `target/` for one test.
/// Tests never write anywhere else.
#[cfg(test)]
pub(crate) fn test_scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-scratch")
        .join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}
