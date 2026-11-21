//! What still runs in a folder that is about to go, and ending it.
//!
//! A task's stack is stopped by what Swarmed knows of it, but a stack can outlive that: one
//! started by hand in the task's terminal, or one whose services set themselves apart and
//! survived an earlier stop. Deleting the worktree under them leaves them restarting on files
//! that are gone, and recreating some as the deletion runs, so that it fails halfway.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

/// Shells a person types into. One open in a terminal is somebody's own, and is left alone.
const SHELLS: [&str; 11] = [
    "sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "elvish", "xonsh",
];

/// A process found running in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Running {
    pub(crate) pid: u32,
    /// The name of its program, for saying what it was.
    pub(crate) name: String,
}

/// One line of `ps`: a process, its terminal (`??` or `?` for none) and its program.
struct Process {
    pid: u32,
    tty: String,
    program: String,
}

/// The processes working in `dir` or running a program from it. Left out: this process, a
/// shell open in a terminal, and the processes of an application (an editor's helpers, say),
/// which see to a folder that went away themselves.
pub(crate) async fn running_in(dir: &Path) -> Vec<Running> {
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let cwds = working_dirs().await;
    let me = std::process::id();
    let is_inside = |process: &Process| {
        Path::new(&process.program).starts_with(&dir)
            || cwds
                .get(&process.pid)
                .is_some_and(|cwd| cwd.starts_with(&dir))
    };
    processes()
        .await
        .into_iter()
        .filter(|process| process.pid != me && is_inside(process) && !spared(process))
        .map(|process| Running {
            pid: process.pid,
            name: program_name(&process.program),
        })
        .collect()
}

/// Ends what still runs in `dir`: asked to stop, then killed. Says what it ended, and what
/// outlived even being killed, which is left running.
pub(crate) async fn end_in(dir: &Path, sink: crate::hooks::OutputSink<'_>) -> Option<String> {
    let running = running_in(dir).await;
    if running.is_empty() {
        return None;
    }
    sink(&format!(
        "ending what still runs in the worktree: {}",
        named(&running)
    ));
    let pids = running.iter().map(|process| process.pid).collect();
    let left = crate::stacks::end_leftovers(pids).await;
    let outlived: Vec<Running> = running
        .into_iter()
        .filter(|process| left.contains(&process.pid))
        .collect();
    (!outlived.is_empty())
        .then(|| format!("left running, as it would not end: {}", named(&outlived)))
}

/// `name (pid)`, one after the other.
fn named(running: &[Running]) -> String {
    let each: Vec<String> = running
        .iter()
        .map(|process| format!("{} ({})", process.name, process.pid))
        .collect();
    each.join(", ")
}

fn spared(process: &Process) -> bool {
    let in_app = process.program.contains(".app/Contents/");
    let has_terminal = !matches!(process.tty.as_str(), "" | "?" | "??");
    let shell = SHELLS.contains(&program_name(&process.program).trim_start_matches('-'));
    in_app || (has_terminal && shell)
}

/// The last part of a program's path: `ps` gives the whole path on a Mac, a name on Linux.
fn program_name(program: &str) -> String {
    Path::new(program).file_name().map_or_else(
        || program.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Every process, with its terminal and its program.
async fn processes() -> Vec<Process> {
    let listing = Command::new("ps")
        .args(["-axo", "pid=,tty=,comm="])
        .stderr(Stdio::null())
        .output()
        .await;
    let Ok(listing) = listing else {
        return vec![];
    };
    String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter_map(parse_process)
        .collect()
}

/// A program's path may hold spaces, so it is the rest of the line after the first two words.
fn parse_process(line: &str) -> Option<Process> {
    let line = line.trim_start();
    let (pid, rest) = line.split_once(char::is_whitespace)?;
    let rest = rest.trim_start();
    let (tty, program) = rest.split_once(char::is_whitespace)?;
    Some(Process {
        pid: pid.parse().ok()?,
        tty: tty.to_string(),
        program: program.trim().to_string(),
    })
}

/// Where each process works, as far as this user may see. Read from `/proc`, one link for each
/// process, on a thread that may wait on files.
#[cfg(target_os = "linux")]
async fn working_dirs() -> HashMap<u32, PathBuf> {
    tokio::task::spawn_blocking(proc_working_dirs)
        .await
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn proc_working_dirs() -> HashMap<u32, PathBuf> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return HashMap::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse().ok()?;
            Some((pid, std::fs::read_link(entry.path().join("cwd")).ok()?))
        })
        .collect()
}

/// Where each process works, as far as this user may see: `lsof` lists them all at once.
#[cfg(not(target_os = "linux"))]
async fn working_dirs() -> HashMap<u32, PathBuf> {
    let listing = Command::new("lsof")
        .args(["-a", "-d", "cwd", "-Fpn", "-w", "-n", "-P"])
        .stderr(Stdio::null())
        .output()
        .await;
    let Ok(listing) = listing else {
        return HashMap::new();
    };
    parse_lsof(&String::from_utf8_lossy(&listing.stdout))
}

/// `lsof -Fpn` writes a `p<pid>` line for each process, then an `n<path>` line for the file.
#[cfg_attr(
    target_os = "linux",
    allow(dead_code, reason = "Linux reads /proc instead")
)]
fn parse_lsof(listing: &str) -> HashMap<u32, PathBuf> {
    let mut cwds = HashMap::new();
    let mut pid = None;
    for line in listing.lines() {
        if let Some(number) = line.strip_prefix('p') {
            pid = number.parse().ok();
        } else if let (Some(path), Some(pid)) = (line.strip_prefix('n'), pid) {
            cwds.insert(pid, PathBuf::from(path));
        }
    }
    cwds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_of_ps_keeps_a_program_path_with_spaces() {
        let process = parse_process(
            "  812 ??       /Applications/Code.app/Contents/MacOS/Code Helper (Plugin)",
        )
        .expect("a process");
        assert_eq!(process.pid, 812);
        assert_eq!(process.tty, "??");
        assert_eq!(
            process.program,
            "/Applications/Code.app/Contents/MacOS/Code Helper (Plugin)"
        );
    }

    #[test]
    fn shells_in_a_terminal_and_applications_are_spared() {
        let process = |tty: &str, program: &str| Process {
            pid: 1,
            tty: tty.to_string(),
            program: program.to_string(),
        };
        assert!(spared(&process("ttys001", "-zsh")));
        assert!(spared(&process("ttys002", "/bin/bash")));
        assert!(spared(&process(
            "??",
            "/Applications/Code.app/Contents/MacOS/Code Helper"
        )));
        // A program started from a terminal is not a shell, and a shell without one is a
        // script's.
        assert!(!spared(&process("ttys001", "devenv")));
        assert!(!spared(&process("??", "/bin/sh")));
    }

    #[test]
    fn lsof_output_gives_each_process_its_folder() {
        let cwds = parse_lsof("p12\nfcwd\nn/Users/me/swarmed/demo/one\np13\nfcwd\nn/\n");
        assert_eq!(
            cwds.get(&12),
            Some(&PathBuf::from("/Users/me/swarmed/demo/one"))
        );
        assert_eq!(cwds.get(&13), Some(&PathBuf::from("/")));
    }
}
