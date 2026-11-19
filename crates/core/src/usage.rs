//! What Swarmed takes up: on disk, and in memory and processor time right now.
//!
//! Sizes on disk are read with `du`, and what is running with `ps`: both are on every Mac and
//! every Linux, and asking them is cheaper than carrying a library that does the same.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

use crate::error::Result;
use crate::{Core, lock};

/// How long one folder's size is waited for. A worktree with a large build can take a while.
const SIZE_TIMEOUT: Duration = Duration::from_secs(20);

/// What Swarmed takes up, as of when it was asked.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Usage {
    /// The database with everything written beside it, in bytes.
    pub database_bytes: u64,
    /// How many session events the database holds: nearly all of its size.
    pub events: u64,
    /// The user's notes, in bytes.
    pub notes_bytes: u64,
    /// Every active task's worktree, largest first.
    pub worktrees: Vec<WorktreeUsage>,
    /// Swarmed's own process with everything it started: agents, their tools, terminals.
    pub running: ProcessUsage,
    /// The share of that taken by each session whose agent is running.
    pub sessions: Vec<SessionUsage>,
}

/// One task's worktree on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorktreeUsage {
    /// The task's project.
    pub project: String,
    /// The task.
    pub task: String,
    /// Where the worktree is.
    pub path: PathBuf,
    /// Its size in bytes, or `None` if it could not be measured in time.
    pub bytes: Option<u64>,
}

/// What a group of processes takes right now.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct ProcessUsage {
    /// How many processes are in the group.
    pub processes: usize,
    /// The memory they hold, in bytes.
    pub memory_bytes: u64,
    /// The processor time they are using, in percent of one core.
    pub cpu_percent: f64,
}

/// What one session's agent, with everything it started, takes right now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionUsage {
    /// The session.
    pub session_id: i64,
    /// Its title.
    pub title: String,
    /// What its agent and the agent's own processes take.
    #[serde(flatten)]
    pub usage: ProcessUsage,
}

/// One line of `ps`: a process, its parent, its memory in kilobytes and its processor share.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Process {
    pid: u32,
    parent: u32,
    memory_kb: u64,
    cpu: f64,
}

fn parse_processes(listing: &str) -> Vec<Process> {
    let process = |line: &str| {
        let mut fields = line.split_whitespace();
        Some(Process {
            pid: fields.next()?.parse().ok()?,
            parent: fields.next()?.parse().ok()?,
            memory_kb: fields.next()?.parse().ok()?,
            // Some locales write the fraction with a comma.
            cpu: fields.next()?.replace(',', ".").parse().ok()?,
        })
    };
    listing.lines().filter_map(process).collect()
}

/// What `root` and every process started from it take together. Nothing, if `root` is not
/// among `processes`.
fn tree_usage(processes: &[Process], root: u32) -> ProcessUsage {
    let mut usage = ProcessUsage::default();
    let mut waiting = vec![root];
    let mut seen = Vec::new();
    while let Some(pid) = waiting.pop() {
        let Some(process) = processes.iter().find(|process| process.pid == pid) else {
            continue;
        };
        // A parent that was reused as a child's id must not send this round in circles.
        if seen.contains(&pid) {
            continue;
        }
        seen.push(pid);
        usage.processes += 1;
        usage.memory_bytes += process.memory_kb * 1024;
        usage.cpu_percent += process.cpu;
        let children = processes.iter().filter(|child| child.parent == pid);
        waiting.extend(children.map(|child| child.pid));
    }
    usage
}

async fn processes() -> Vec<Process> {
    let listing = Command::new("ps")
        .args(["-axo", "pid=,ppid=,rss=,%cpu="])
        .output()
        .await;
    listing.map_or_else(
        |_| Vec::new(),
        |output| parse_processes(&String::from_utf8_lossy(&output.stdout)),
    )
}

/// The size of everything under `path`, or `None` if it is not there or took too long.
async fn folder_bytes(path: &Path) -> Option<u64> {
    let measuring = Command::new("du").arg("-sk").arg(path).output();
    let output = tokio::time::timeout(SIZE_TIMEOUT, measuring)
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let kilobytes: u64 = text.split_whitespace().next()?.parse().ok()?;
    Some(kilobytes * 1024)
}

/// [`folder_bytes`] for a path of its own, to measure on a task of its own.
async fn owned_folder_bytes(path: PathBuf) -> Option<u64> {
    folder_bytes(&path).await
}

/// The size of the file at `path`; nothing if it is not there.
fn file_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |file| file.len())
}

impl Core {
    /// What Swarmed takes up on disk and what it is using right now. Measuring the worktrees
    /// reads through them, so this can take a few seconds.
    pub async fn usage(&self) -> Result<Usage> {
        let database = self.paths.db_file();
        let beside = |suffix: &str| PathBuf::from(format!("{}{suffix}", database.display()));
        let database_bytes =
            file_bytes(&database) + file_bytes(&beside("-wal")) + file_bytes(&beside("-shm"));
        let events = self.store().event_count()?;
        let notes = self.paths.data_dir.join("notes");
        let (notes_bytes, worktrees, processes) =
            tokio::join!(folder_bytes(&notes), self.worktree_sizes(), processes());
        Ok(Usage {
            database_bytes,
            events,
            notes_bytes: notes_bytes.unwrap_or(0),
            worktrees: worktrees?,
            running: tree_usage(&processes, std::process::id()),
            sessions: self.session_usage(&processes)?,
        })
    }

    /// Every active task's worktree with its size, all measured at once, largest first.
    async fn worktree_sizes(&self) -> Result<Vec<WorktreeUsage>> {
        let mut tasks = Vec::new();
        for project in self.projects()? {
            let own = self.tasks(&project.name, false)?;
            tasks.extend(own.into_iter().map(|task| (project.name.clone(), task)));
        }
        let measure = |(project, task): (String, crate::store::Task)| {
            let path = task.worktree_path.clone();
            let size = tokio::spawn(owned_folder_bytes(path));
            (project, task, size)
        };
        let measuring: Vec<_> = tasks.into_iter().map(measure).collect();
        let mut worktrees = Vec::new();
        for (project, task, size) in measuring {
            worktrees.push(WorktreeUsage {
                project,
                task: task.name,
                path: task.worktree_path,
                bytes: size.await.ok().flatten(),
            });
        }
        worktrees.sort_by_key(|worktree| std::cmp::Reverse(worktree.bytes));
        Ok(worktrees)
    }

    /// What each running session's agent takes, the hungriest for memory first.
    fn session_usage(&self, processes: &[Process]) -> Result<Vec<SessionUsage>> {
        let running = self.running_sessions();
        let pids = lock(&self.live.pids).clone();
        let mut sessions = Vec::new();
        for (session_id, pid) in pids.into_iter().filter(|(id, _)| running.contains(id)) {
            sessions.push(SessionUsage {
                session_id,
                title: self.session(session_id)?.title,
                usage: tree_usage(processes, pid),
            });
        }
        sessions.sort_by_key(|session| std::cmp::Reverse(session.usage.memory_bytes));
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "    1     0   9000   0.0\n  100     1  20000   1.5\n  101   100  \
                           30000  12,5\n  102   101   1000   0.0\n  200     1  50000   3.0\n\
                           garbage line\n";

    #[test]
    fn a_process_listing_is_read_whatever_its_spacing_and_decimal_mark() {
        let processes = parse_processes(LISTING);
        assert_eq!(processes.len(), 5);
        assert_eq!(processes[2].pid, 101);
        assert!((processes[2].cpu - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn a_process_is_counted_with_everything_started_from_it() {
        let processes = parse_processes(LISTING);
        let app = tree_usage(&processes, 100);
        assert_eq!(app.processes, 3);
        assert_eq!(app.memory_bytes, 51_000 * 1024);
        assert!((app.cpu_percent - 14.0).abs() < 1e-9);
        // Its neighbour is not part of it, and a process that is gone takes nothing.
        assert_eq!(tree_usage(&processes, 200).processes, 1);
        assert_eq!(tree_usage(&processes, 999), ProcessUsage::default());
    }

    #[tokio::test]
    async fn a_folders_size_is_measured_and_a_missing_one_is_not() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("file"), vec![0_u8; 300_000]).expect("writes");
        let bytes = folder_bytes(dir.path()).await.expect("measures");
        assert!(bytes >= 300_000, "{bytes}");
        assert_eq!(folder_bytes(&dir.path().join("nothing")).await, None);
    }
}
