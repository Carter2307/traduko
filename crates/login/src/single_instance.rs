//! One mascot per user: an exclusive lock on a file, held for the life of the process.
//!
//! Why the app needs its own guard: LaunchServices refuses a second copy only when
//! both launches go through it (Finder, Dock, `open`, a login item). A LaunchAgent,
//! `cargo run`, or running `Name.app/Contents/MacOS/<bin>` directly bypasses it.
//!
//! The kernel drops the lock when the process ends, however it ends, so a crash
//! leaves nothing stale. `std::fs::File::try_lock` is stable since Rust 1.89.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Keep this value alive for as long as the app runs (bind it in `main`).
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
    path: PathBuf,
}

impl InstanceLock {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug)]
pub enum LockError {
    /// Another process holds the lock: exit quietly, with status 0.
    AlreadyRunning,
    Io(io::Error),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::AlreadyRunning => write!(f, "another instance is already running"),
            LockError::Io(error) => write!(f, "cannot take the instance lock: {error}"),
        }
    }
}

impl std::error::Error for LockError {}

/// Takes `<dir>/instance.lock`. `dir` is normally
/// `~/Library/Application Support/<App>`; it is created when missing.
pub fn acquire(dir: &Path) -> Result<InstanceLock, LockError> {
    std::fs::create_dir_all(dir).map_err(LockError::Io)?;
    let path = dir.join("instance.lock");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(LockError::Io)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err(LockError::AlreadyRunning),
        Err(TryLockError::Error(error)) => return Err(LockError::Io(error)),
    }
    // The PID is for a human reading the file; nothing depends on it.
    let _ = file.set_len(0);
    let _ = writeln!(file, "{}", std::process::id());
    Ok(InstanceLock { _file: file, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_taker_is_refused_until_the_first_lets_go() {
        let dir = crate::test_scratch("lock");
        let first = acquire(&dir).expect("first lock");
        assert!(matches!(acquire(&dir), Err(LockError::AlreadyRunning)));
        assert_eq!(
            std::fs::read_to_string(first.path()).unwrap().trim(),
            std::process::id().to_string()
        );
        drop(first);
        // The lock belongs to the open file, and a child process holds a copy of
        // every descriptor between fork and exec. Other tests spawn `plutil` and
        // `launchctl` in parallel, so the lock can outlive `drop` by an instant
        // (measured: 6 failures in 60 parallel runs without this wait, 0 in 60
        // single-threaded). It can only last longer, never shorter, which is the
        // safe direction for a single-instance guard.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let again = loop {
            match acquire(&dir) {
                Ok(lock) => break lock,
                Err(LockError::AlreadyRunning) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => panic!("lock is free again: {error}"),
            }
        };
        drop(again);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
