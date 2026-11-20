//! The `PATH` the user's own terminal has, for what Swarmed starts on their behalf.
//!
//! An app started from the Finder or the Dock is given the system's bare `PATH`, without what
//! the user's shell adds to it: Homebrew, Nix, version managers. A setup command, a stack or
//! an agent's tools would then not find programs that work in any terminal. So a front end
//! started that way asks the user's shell once, and everything started afterwards is given
//! what it said, ahead of the process's own.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// What the shell's answer stands between, since a shell's start-up files may print too.
const MARK: &str = "__SWARMED_PATH__";
/// How long the shell may take: its start-up files are the user's, and can be slow.
const SHELL_TIMEOUT: Duration = Duration::from_secs(8);
const POLL: Duration = Duration::from_millis(20);

static LEARNED: OnceLock<OsString> = OnceLock::new();

/// The `PATH` for whatever Swarmed starts: the shell's, if it was asked, then this process's.
pub(crate) fn path() -> OsString {
    let own = || std::env::var_os("PATH").unwrap_or_default();
    LEARNED.get().cloned().unwrap_or_else(own)
}

/// The `PATH` line of what the shell printed, from between the marks.
fn path_in(printed: &str) -> Option<&str> {
    let after = printed.split_once(MARK)?.1;
    let between = after.split_once(MARK)?.0.trim();
    (!between.is_empty()).then_some(between)
}

/// `first`, then the folders of `second` that are not in it already.
fn joined(first: &OsStr, second: &OsStr) -> OsString {
    let mut dirs: Vec<PathBuf> = std::env::split_paths(first).collect();
    for dir in std::env::split_paths(second) {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs).unwrap_or_else(|_| first.to_os_string())
}

/// Asks the user's shell, started the way a terminal starts it, what its `PATH` is. `None`
/// if it cannot be started, takes too long or says nothing usable.
fn ask_shell() -> Option<String> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    // `printenv` and not `$PATH`: not every shell writes that as one colon-separated line.
    let script = format!("echo {MARK}; printenv PATH; echo {MARK}");
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let until = Instant::now() + SHELL_TIMEOUT;
    while matches!(child.try_wait(), Ok(None)) {
        if Instant::now() >= until {
            child.kill().ok();
            child.wait().ok();
            return None;
        }
        std::thread::sleep(POLL);
    }
    let output = child.wait_with_output().ok()?;
    let printed = String::from_utf8_lossy(&output.stdout);
    path_in(&printed).map(String::from)
}

/// Learns the `PATH` of the user's shell, for everything started from now on.
///
/// For a front end that was not started from a terminal; one that was already has it. Blocks
/// for as long as the shell takes to start, and changes nothing if the shell cannot say.
pub fn learn_shell_path() {
    let Some(shell) = ask_shell() else {
        return;
    };
    let own = std::env::var_os("PATH").unwrap_or_default();
    LEARNED.set(joined(OsStr::new(&shell), &own)).ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run by hand (`cargo test -p swarmed-core shell -- --ignored --nocapture`): what this
    /// machine's own shell says, started with the system's bare `PATH`.
    #[test]
    #[ignore = "starts the user's own shell"]
    fn this_machines_shell_says_where_its_programs_are() {
        let found = ask_shell().expect("the shell answers");
        let has =
            |program: &str| std::env::split_paths(&found).any(|dir| dir.join(program).is_file());
        eprintln!(
            "devenv: {}, brew: {}, folders: {}",
            has("devenv"),
            has("brew"),
            std::env::split_paths(&found).count()
        );
    }

    #[test]
    fn the_path_is_read_from_between_the_marks_whatever_else_was_printed() {
        let printed = format!("Welcome back!\n{MARK}\n/opt/x/bin:/usr/bin\n{MARK}\nbye\n");
        assert_eq!(path_in(&printed), Some("/opt/x/bin:/usr/bin"));
        assert_eq!(path_in("nothing of the kind"), None);
        assert_eq!(path_in(&format!("{MARK}\n\n{MARK}")), None);
    }

    #[test]
    fn the_shells_folders_come_first_and_none_twice() {
        let both = joined(OsStr::new("/nix/bin:/usr/bin"), OsStr::new("/usr/bin:/bin"));
        assert_eq!(both, OsString::from("/nix/bin:/usr/bin:/bin"));
    }
}
