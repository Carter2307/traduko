//! Fallback: a per-user LaunchAgent plist in `~/Library/LaunchAgents`.
//!
//! launchd loads every plist in that folder when the user logs in, so writing the
//! file is enough to start at the next login, and deleting it is enough to stop.
//! `launchctl bootstrap` is only needed to load the job in the *current* session;
//! with `RunAtLoad` that starts a second copy of the app at once, so the app itself
//! never calls it. The helpers at the bottom are for an install / uninstall script.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The definition of the agent. Building one touches nothing on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchAgent {
    /// launchd label and plist file name, e.g. `dev.example.mascot.login`.
    pub label: String,
    /// Absolute path of the executable: `/…/Name.app/Contents/MacOS/<bin>`.
    pub program: PathBuf,
    /// Extra arguments, e.g. `--launched-at-login`.
    pub args: Vec<String>,
    /// `CFBundleIdentifier` of the app, written as `AssociatedBundleIdentifiers`.
    /// macOS only honours it when the executable has a Team ID, so on an ad-hoc
    /// build it is harmless and ignored.
    pub bundle_id: Option<String>,
    /// Restart after a crash (non-zero exit) but not after a normal quit.
    pub restart_on_crash: bool,
    /// Normally `~/Library/LaunchAgents`. Tests point it at a scratch folder.
    pub agents_dir: PathBuf,
}

impl LaunchAgent {
    /// An agent in the real `~/Library/LaunchAgents` for `program`.
    pub fn for_current_user(label: &str, program: &Path) -> io::Result<Self> {
        let home = std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        Ok(LaunchAgent {
            label: label.to_string(),
            program: program.to_path_buf(),
            args: Vec::new(),
            bundle_id: None,
            restart_on_crash: false,
            agents_dir: PathBuf::from(home).join("Library/LaunchAgents"),
        })
    }

    pub fn plist_path(&self) -> PathBuf {
        self.agents_dir.join(format!("{}.plist", self.label))
    }

    /// The exact file contents.
    ///
    /// * `RunAtLoad`: start when launchd loads the job, that is at login.
    /// * `ProcessType = Interactive`: without it launchd throttles the job's CPU and I/O
    ///   ("light resource limits", `man launchd.plist`), which would slow a model down.
    /// * `LimitLoadToSessionType = Aqua`: only in a graphical login session.
    /// * `KeepAlive { SuccessfulExit = false }` (optional): restart on a crash only.
    ///   The single-instance guard must therefore exit with status 0.
    pub fn plist_xml(&self) -> String {
        let mut arguments = format!(
            "        <string>{}</string>\n",
            xml_escape(&self.program.to_string_lossy())
        );
        for arg in &self.args {
            arguments.push_str(&format!("        <string>{}</string>\n", xml_escape(arg)));
        }
        let mut body = format!(
            "    <key>Label</key>\n    <string>{label}</string>\n    \
             <key>ProgramArguments</key>\n    <array>\n{arguments}    </array>\n    \
             <key>RunAtLoad</key>\n    <true/>\n    \
             <key>ProcessType</key>\n    <string>Interactive</string>\n    \
             <key>LimitLoadToSessionType</key>\n    <string>Aqua</string>\n",
            label = xml_escape(&self.label),
        );
        if let Some(bundle_id) = &self.bundle_id {
            body.push_str(&format!(
                "    <key>AssociatedBundleIdentifiers</key>\n    <array>\n        <string>{}</string>\n    </array>\n",
                xml_escape(bundle_id)
            ));
        }
        if self.restart_on_crash {
            body.push_str(
                "    <key>KeepAlive</key>\n    <dict>\n        <key>SuccessfulExit</key>\n        <false/>\n    </dict>\n",
            );
        }
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n<dict>\n{body}</dict>\n</plist>\n"
        )
    }

    /// Writes the plist (atomically: temp file, then rename). Starts at the next login.
    pub fn install(&self) -> io::Result<()> {
        if !self.program.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the program path must be absolute",
            ));
        }
        std::fs::create_dir_all(&self.agents_dir)?;
        let path = self.plist_path();
        let temp = self.agents_dir.join(format!(".{}.plist.tmp", self.label));
        std::fs::write(&temp, self.plist_xml())?;
        std::fs::rename(&temp, &path)
    }

    /// Deletes the plist. A missing file is success.
    ///
    /// It does not `bootout` the job: when launchd started this very process as the
    /// job, `bootout` would kill the app the user is looking at. A job that stays
    /// loaded until logout does nothing more; it is not loaded again at the next login.
    pub fn remove(&self) -> io::Result<()> {
        match std::fs::remove_file(self.plist_path()) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// A plist with this label is on disk (whatever it points at).
    pub fn is_installed(&self) -> bool {
        self.plist_path().is_file()
    }

    /// The plist on disk is byte-for-byte what [`install`](Self::install) would write.
    /// False after the app moved or the arguments changed: call `install` again.
    pub fn is_current(&self) -> bool {
        std::fs::read_to_string(self.plist_path()).is_ok_and(|on_disk| on_disk == self.plist_xml())
    }
}

fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

// ---- launchctl: for an install / uninstall script, not for the running app ----

unsafe extern "C" {
    safe fn getuid() -> u32;
}

/// `gui/<uid>`: the launchd domain of the logged-in user's graphical session.
pub fn gui_domain() -> String {
    format!("gui/{}", getuid())
}

/// `launchctl print gui/<uid>/<label>` succeeded: the job is loaded in this session.
/// Read-only. (Exit status 113 means "could not find service".)
pub fn is_loaded(label: &str) -> bool {
    Command::new("/bin/launchctl")
        .args(["print", &format!("{}/{label}", gui_domain())])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// `launchctl bootstrap gui/<uid> <plist>`: loads the job now. With `RunAtLoad` it
/// starts the program at once. NOT called by the app; the spike never ran it.
pub fn bootstrap(plist: &Path) -> io::Result<Output> {
    Command::new("/bin/launchctl")
        .arg("bootstrap")
        .arg(gui_domain())
        .arg(plist)
        .output()
}

/// `launchctl bootout gui/<uid>/<label>`: unloads the job and kills its process.
/// NOT called by the app; the spike never ran it.
pub fn bootout(label: &str) -> io::Result<Output> {
    Command::new("/bin/launchctl")
        .args(["bootout", &format!("{}/{label}", gui_domain())])
        .output()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(dir: &Path) -> LaunchAgent {
        LaunchAgent {
            label: "dev.spike.mascot.login".into(),
            program: PathBuf::from("/Users/someone/Applications/Mascot.app/Contents/MacOS/mascot"),
            args: vec!["--launched-at-login".into()],
            bundle_id: Some("dev.spike.mascot".into()),
            restart_on_crash: true,
            agents_dir: dir.to_path_buf(),
        }
    }

    #[test]
    fn install_then_remove_in_a_scratch_folder() {
        let dir = crate::test_scratch("agent-install");
        let agent = agent(&dir);
        assert!(!agent.is_installed());
        agent.install().unwrap();
        assert!(agent.is_installed() && agent.is_current());
        assert_eq!(agent.plist_path(), dir.join("dev.spike.mascot.login.plist"));
        // No temp file is left behind.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        agent.remove().unwrap();
        assert!(!agent.is_installed());
        agent.remove().unwrap(); // a second remove is fine
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_moved_app_makes_the_plist_stale() {
        let dir = crate::test_scratch("agent-stale");
        let mut agent = agent(&dir);
        agent.install().unwrap();
        agent.program = PathBuf::from("/Applications/Mascot.app/Contents/MacOS/mascot");
        assert!(agent.is_installed());
        assert!(!agent.is_current());
        agent.install().unwrap();
        assert!(agent.is_current());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_plist_passes_plutil_and_holds_the_keys() {
        let dir = crate::test_scratch("agent-lint");
        let mut agent = agent(&dir);
        agent.program = PathBuf::from("/Users/a&b/My <Apps>/Mascot's.app/Contents/MacOS/mascot");
        agent.install().unwrap();
        let lint = Command::new("/usr/bin/plutil")
            .arg("-lint")
            .arg(agent.plist_path())
            .output()
            .unwrap();
        assert!(
            lint.status.success(),
            "{}",
            String::from_utf8_lossy(&lint.stdout)
        );
        let extract = |key: &str| {
            let out = Command::new("/usr/bin/plutil")
                .args(["-extract", key, "raw", "-o", "-"])
                .arg(agent.plist_path())
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        assert_eq!(extract("Label"), "dev.spike.mascot.login");
        assert_eq!(
            extract("ProgramArguments.0"),
            "/Users/a&b/My <Apps>/Mascot's.app/Contents/MacOS/mascot"
        );
        assert_eq!(extract("ProgramArguments.1"), "--launched-at-login");
        assert_eq!(extract("RunAtLoad"), "true");
        assert_eq!(extract("ProcessType"), "Interactive");
        assert_eq!(extract("LimitLoadToSessionType"), "Aqua");
        assert_eq!(extract("AssociatedBundleIdentifiers.0"), "dev.spike.mascot");
        assert_eq!(extract("KeepAlive.SuccessfulExit"), "false");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_relative_program_is_refused() {
        let dir = crate::test_scratch("agent-relative");
        let mut agent = agent(&dir);
        agent.program = PathBuf::from("target/release/mascot");
        assert!(agent.install().is_err());
        assert!(!agent.is_installed());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unknown_label_is_not_loaded() {
        assert!(gui_domain().starts_with("gui/"));
        assert!(!is_loaded("dev.spike.this.label.does.not.exist"));
    }
}
