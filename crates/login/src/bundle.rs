//! Where the running program is: inside `Name.app` or not, where its resources are,
//! and whether that place is a sane one to register a login item from.
//!
//! Pure `std` on purpose: the same answers under `cargo run` and inside the bundle,
//! and unit tests need no bundle.

use std::path::{Path, PathBuf};

/// The `Name.app` directory that holds `exe`, when `exe` is `Name.app/Contents/MacOS/<bin>`.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension().is_some_and(|ext| ext == "app")
        && contents.join("Info.plist").is_file();
    is_bundle.then(|| app.to_path_buf())
}

/// The running executable with symlinks resolved.
pub fn current_exe() -> std::io::Result<PathBuf> {
    std::env::current_exe()?.canonicalize()
}

/// The running `Name.app`, or `None` under `cargo run` / `target/release/<bin>`.
pub fn current_bundle() -> Option<PathBuf> {
    bundle_of(&current_exe().ok()?)
}

/// Where read-only assets ship (mascot images, fonts, a model if it is bundled).
///
/// Order:
/// 1. `$<ENV_VAR>` when set (tests, odd setups);
/// 2. `Name.app/Contents/Resources` when running from a bundle;
/// 3. `dev_dir`, normally `concat!(env!("CARGO_MANIFEST_DIR"), "/resources")`,
///    so plain `cargo run` finds the same files in the source tree.
pub fn resources_dir(env_var: &str, dev_dir: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os(env_var).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    resources_dir_for(current_exe().ok().as_deref(), dev_dir)
}

/// [`resources_dir`] without the environment, for tests.
pub fn resources_dir_for(exe: Option<&Path>, dev_dir: &Path) -> PathBuf {
    match exe.and_then(bundle_of) {
        Some(app) => app.join("Contents/Resources"),
        None => dev_dir.to_path_buf(),
    }
}

/// `~/Library/Application Support/<app_dir_name>`: writable per-user data
/// (models, settings, the instance lock). Not created here.
pub fn app_support_dir(app_dir_name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|v| !v.is_empty())?;
    Some(
        PathBuf::from(home)
            .join("Library/Application Support")
            .join(app_dir_name),
    )
}

/// Where the running bundle sits, as far as a login item cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallLocation {
    /// `/Applications/Name.app` (or a sub-folder of it).
    SystemApplications,
    /// `~/Applications/Name.app` (or a sub-folder of it).
    UserApplications,
    /// A randomised read-only copy macOS runs a quarantined app from. Never register here.
    Translocated,
    /// A bundle somewhere else: a build folder, Downloads, a disk image.
    OtherBundle(PathBuf),
    /// No bundle at all: `cargo run`.
    Unbundled,
}

impl InstallLocation {
    /// Only an installed copy may register: Background Task Management records the
    /// path, so a build folder or a translocated copy would become the login item.
    pub fn may_register(&self) -> bool {
        matches!(
            self,
            InstallLocation::SystemApplications | InstallLocation::UserApplications
        )
    }
}

pub fn install_location() -> InstallLocation {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    install_location_of(current_bundle().as_deref(), home.as_deref())
}

/// [`install_location`] on explicit inputs, for tests.
pub fn install_location_of(bundle: Option<&Path>, home: Option<&Path>) -> InstallLocation {
    let Some(bundle) = bundle else {
        return InstallLocation::Unbundled;
    };
    if bundle
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return InstallLocation::Translocated;
    }
    if bundle.starts_with("/Applications") {
        return InstallLocation::SystemApplications;
    }
    if let Some(home) = home {
        // `current_exe` is canonical, so compare against the canonical home as well.
        let user_apps = home.join("Applications");
        let canonical = user_apps
            .canonicalize()
            .unwrap_or_else(|_| user_apps.clone());
        if bundle.starts_with(&user_apps) || bundle.starts_with(&canonical) {
            return InstallLocation::UserApplications;
        }
    }
    InstallLocation::OtherBundle(bundle.to_path_buf())
}

/// A fingerprint of "this installed copy": the executable's path, inode, size and
/// modification time. A reinstall (the script deletes and copies the bundle) changes
/// it; an untouched install keeps it.
///
/// Use: store it next to the "launch at login" preference when registration succeeds.
/// At a later launch, if macOS no longer reports the item as enabled:
/// same fingerprint -> the user removed it in System Settings, so turn the preference off;
/// different fingerprint -> a reinstall lost it, so register again.
pub fn install_fingerprint() -> std::io::Result<String> {
    install_fingerprint_of(&current_exe()?)
}

pub fn install_fingerprint_of(exe: &Path) -> std::io::Result<String> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(exe)?;
    Ok(format!(
        "{}|ino={}|len={}|mtime={}.{:09}",
        exe.display(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_bundle(root: &Path, name: &str) -> PathBuf {
        let app = root.join(format!("{name}.app"));
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(app.join("Contents/Resources")).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), "<plist/>").unwrap();
        let exe = app.join("Contents/MacOS").join(name.to_lowercase());
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
        exe
    }

    fn scratch(tag: &str) -> PathBuf {
        crate::test_scratch(tag)
    }

    #[test]
    fn finds_the_bundle_and_its_resources() {
        let root = scratch("bundle");
        let exe = fake_bundle(&root, "Mascot");
        let app = root.join("Mascot.app");
        assert_eq!(bundle_of(&exe), Some(app.clone()));
        assert_eq!(
            resources_dir_for(Some(&exe), Path::new("/dev/resources")),
            app.join("Contents/Resources")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_cargo_binary_is_unbundled_and_uses_the_dev_dir() {
        let exe = Path::new("/work/app/target/release/mascot");
        assert_eq!(bundle_of(exe), None);
        assert_eq!(
            resources_dir_for(Some(exe), Path::new("/work/app/resources")),
            PathBuf::from("/work/app/resources")
        );
        assert_eq!(install_location_of(None, None), InstallLocation::Unbundled);
    }

    #[test]
    fn a_folder_named_app_without_info_plist_is_not_a_bundle() {
        let root = scratch("nobundle");
        let macos = root.join("X.app/Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        assert_eq!(bundle_of(&macos.join("x")), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn classifies_install_locations() {
        let home = Path::new("/Users/someone");
        let loc = |p: &str| install_location_of(Some(Path::new(p)), Some(home));
        assert_eq!(
            loc("/Applications/Mascot.app"),
            InstallLocation::SystemApplications
        );
        assert_eq!(
            loc("/Users/someone/Applications/Mascot.app"),
            InstallLocation::UserApplications
        );
        assert_eq!(
            loc("/private/var/folders/ab/T/AppTranslocation/1234/d/Mascot.app"),
            InstallLocation::Translocated
        );
        assert_eq!(
            loc("/Users/someone/code/app/dist/Mascot.app"),
            InstallLocation::OtherBundle(PathBuf::from("/Users/someone/code/app/dist/Mascot.app"))
        );
        assert!(loc("/Applications/Mascot.app").may_register());
        assert!(loc("/Users/someone/Applications/Mascot.app").may_register());
        assert!(!loc("/Users/someone/code/app/dist/Mascot.app").may_register());
        assert!(!InstallLocation::Unbundled.may_register());
        assert!(!InstallLocation::Translocated.may_register());
    }

    #[test]
    fn a_reinstall_changes_the_fingerprint() {
        let root = scratch("fingerprint");
        let exe = fake_bundle(&root, "Mascot");
        let first = install_fingerprint_of(&exe).unwrap();
        assert_eq!(
            first,
            install_fingerprint_of(&exe).unwrap(),
            "stable while untouched"
        );
        // What an install script does: delete the bundle, copy a fresh one to the same path.
        std::fs::remove_dir_all(root.join("Mascot.app")).unwrap();
        // Hold a new file so the old inode number is not handed straight back.
        let _spacer = std::fs::File::create(root.join("spacer")).unwrap();
        let exe = fake_bundle(&root, "Mascot");
        std::fs::write(&exe, b"#!/bin/sh\n# rebuilt\n").unwrap();
        assert_ne!(first, install_fingerprint_of(&exe).unwrap());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
