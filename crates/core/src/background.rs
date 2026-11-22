//! What a task's agents left running behind their turns.
//!
//! An agent can put a command in the background and carry on, or end its turn with it still
//! running. It says when such a thing starts and when it ends; this reads those two reports
//! back out of the sessions' histories, so a front end can list them and stop the ones that
//! run. What a command prints can be read too, from the file Claude Code writes it to: only a
//! file the session's own agent named, and only from Claude Code's temporary folder. It can
//! hold anything, so it goes to the user alone: never logged, stored or handed on.

use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use rusqlite::params;
use serde::Serialize;

use crate::agent::{AgentCommand, AgentEvent, BackgroundState};
use crate::error::Result;
use crate::sessions::event_from_json;
use crate::store::Store;
use crate::{Core, Error};

/// One thing an agent left running, as last heard of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackgroundTask {
    /// The session whose agent started it.
    pub session_id: i64,
    /// That session's title.
    pub session_title: String,
    /// The agent's own id for it.
    pub task_id: String,
    /// What it is, in the agent's words.
    pub description: String,
    /// What kind of thing it is, as the agent names it.
    pub kind: String,
    /// Whether it runs, and how it ended if it did.
    pub state: BackgroundState,
    /// The agent's own line about how it ended.
    pub summary: Option<String>,
    /// The tool call that started it, for a sub-agent: what opens its conversation.
    pub tool_use_id: Option<String>,
    /// Whether what it printed can be read, with [`Core::session_background_output`].
    pub output: bool,
}

impl BackgroundTask {
    /// Takes in a later report on the same thing. An end is reported without saying again
    /// what the thing was, and a report of where a command's output goes says little else.
    fn heard(&mut self, later: Self) {
        self.state = later.state;
        self.summary = later.summary.or_else(|| self.summary.take());
        if self.description.is_empty() {
            self.description = later.description;
        }
        if self.kind.is_empty() {
            self.kind = later.kind;
        }
        self.tool_use_id = self.tool_use_id.take().or(later.tool_use_id);
        self.output |= later.output;
    }
}

/// A report from a session's history: whose it is, and the event itself.
type Reported = (i64, String, AgentEvent);

/// How much of what a command printed is read at once: its latest part, when there is more.
const OUTPUT_LIMIT: u64 = 256 * 1024;

/// What a command an agent left running printed, or the part of it read this time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackgroundOutput {
    /// What it printed, from `start` on.
    pub text: String,
    /// Where `text` starts in what it printed, in bytes: past the beginning when only the
    /// latest part was read.
    pub start: u64,
    /// Where `text` ends: where to read on from.
    pub end: u64,
}

impl Core {
    /// Everything the task's sessions left running, the running ones first, then the ended
    /// ones with the newest first. Closed sessions are left out.
    pub fn task_background(&self, project: &str, task: &str) -> Result<Vec<BackgroundTask>> {
        let (_, task) = self.task(project, task)?;
        let reported = self.store().background_reports(task.id)?;
        let alive = self.running_sessions();
        Ok(listed(reported, &alive))
    }

    /// What a command session `id`'s agent left running printed: from `from` on if that is
    /// no more than is read at once, else its latest part. `None` once the file is gone, which
    /// it is after Claude Code cleared its temporary folder on starting again. Only a file the
    /// session's own agent named is read, and only from Claude Code's temporary folder, links
    /// followed.
    pub fn session_background_output(
        &self,
        id: i64,
        task_id: &str,
        from: Option<u64>,
    ) -> Result<Option<BackgroundOutput>> {
        let named = self.store().output_file(id, task_id)?.ok_or_else(|| {
            Error::BackgroundOutput(format!(
                "this session's agent named no output for `{task_id}`"
            ))
        })?;
        // Claude Code writes it in a folder of the user's own: the user is who owns Swarmed's.
        let owner = std::fs::metadata(&self.paths.data_dir)?.uid();
        let root = PathBuf::from(format!("/tmp/claude-{owner}"));
        let Some(file) = readable_output(&named, &root)? else {
            return Ok(None);
        };
        read_output(&file, from).map(Some)
    }

    /// Asks the session's agent to stop something it left running. Refused if the agent is
    /// not running: what it started went with it.
    pub fn session_stop_background(&self, id: i64, task_id: &str) -> Result<()> {
        if task_id.trim().is_empty() {
            return Err(Error::InvalidName(task_id.to_string()));
        }
        let task_id = task_id.to_string();
        self.live
            .command(id, AgentCommand::StopBackground { task_id })
    }
}

impl Store {
    /// Where session `session`'s agent last said what `task_id` prints is written.
    fn output_file(&self, session: i64, task_id: &str) -> Result<Option<String>> {
        let mut statement = self.conn.prepare(
            "SELECT event FROM session_events
             WHERE session_id = ?1 AND event LIKE '{\"type\":\"background\"%'
             ORDER BY id",
        )?;
        let rows = statement.query_map(params![session], |row| row.get::<_, String>(0))?;
        let named = |event: AgentEvent| match event {
            AgentEvent::Background {
                task_id: id,
                output_file,
                ..
            } if id == task_id => output_file,
            _ => None,
        };
        let events = rows.filter_map(std::result::Result::ok);
        Ok(events
            .filter_map(|text| named(event_from_json(&text)))
            .last())
    }

    /// What the task's open sessions said of the things they left running, oldest first.
    fn background_reports(&self, task: i64) -> Result<Vec<Reported>> {
        // The event's type is the first thing in its JSON, which keeps this a prefix match.
        let mut statement = self.conn.prepare(
            "SELECT s.id, s.title, e.event FROM session_events e
             JOIN sessions s ON s.id = e.session_id
             WHERE s.task_id = ?1 AND s.closed_at IS NULL
               AND e.event LIKE '{\"type\":\"background\"%'
             ORDER BY e.id",
        )?;
        let rows = statement.query_map(params![task], |row| {
            let event: String = row.get(2)?;
            Ok((row.get(0)?, row.get(1)?, event_from_json(&event)))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// One report as an entry of its own, if it is a report of something left running.
fn task_of(session_id: i64, session_title: String, event: AgentEvent) -> Option<BackgroundTask> {
    let AgentEvent::Background {
        task_id,
        description,
        kind,
        state,
        summary,
        tool_use_id,
        output_file,
    } = event
    else {
        return None;
    };
    Some(BackgroundTask {
        session_id,
        session_title,
        task_id,
        description,
        kind,
        state,
        summary,
        tool_use_id,
        output: output_file.is_some(),
    })
}

/// Folds the reports into one entry per thing, by the last word on each. What a session's
/// agent left running does not outlive that agent, so it is stopped if the agent is gone.
fn listed(reported: Vec<Reported>, alive: &[i64]) -> Vec<BackgroundTask> {
    let mut tasks: Vec<BackgroundTask> = Vec::new();
    for (session_id, session_title, event) in reported {
        let Some(new) = task_of(session_id, session_title, event) else {
            continue;
        };
        let same = |task: &BackgroundTask| {
            task.session_id == new.session_id && task.task_id == new.task_id
        };
        let Some(at) = tasks.iter().position(same) else {
            tasks.push(new);
            continue;
        };
        let mut task = tasks.remove(at);
        task.heard(new);
        tasks.push(task);
    }
    for task in &mut tasks {
        let orphaned = task.state == BackgroundState::Running && !alive.contains(&task.session_id);
        if orphaned {
            task.state = BackgroundState::Stopped;
        }
    }
    // Running first; within each, the newest word first.
    tasks.reverse();
    tasks.sort_by_key(|task| task.state != BackgroundState::Running);
    tasks
}

/// The file `named`, links followed, if it is one output is read from: a regular `.output`
/// file inside `root`, Claude Code's temporary folder. `None` if there is no such file.
fn readable_output(named: &str, root: &Path) -> Result<Option<PathBuf>> {
    let refused = || {
        Error::BackgroundOutput(
            "only what Claude Code writes to its own temporary folder is read".to_string(),
        )
    };
    let real = match Path::new(named).canonicalize() {
        Ok(real) => real,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(refused()),
    };
    let root = root.canonicalize().map_err(|_| refused())?;
    let output = real
        .extension()
        .is_some_and(|extension| extension == "output");
    let file = std::fs::metadata(&real).is_ok_and(|metadata| metadata.is_file());
    if real.starts_with(&root) && output && file {
        Ok(Some(real))
    } else {
        Err(refused())
    }
}

/// What `file` holds from `from` on, if that is no more than [`OUTPUT_LIMIT`], else its last
/// part, starting after the first line break in it so that no line is shown cut. A character
/// cut in two at the end is left for the next read.
fn read_output(file: &Path, from: Option<u64>) -> Result<BackgroundOutput> {
    let mut opened = File::open(file)?;
    let length = opened.metadata()?.len();
    let near = |from: &u64| *from <= length && length - from <= OUTPUT_LIMIT;
    let start = from
        .filter(near)
        .unwrap_or_else(|| length.saturating_sub(OUTPUT_LIMIT));
    opened.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    opened.take(length - start).read_to_end(&mut bytes)?;
    let skip = if start > 0 && from != Some(start) {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(0, |at| at + 1)
    } else {
        0
    };
    let whole = match std::str::from_utf8(&bytes) {
        Err(error) if error.error_len().is_none() => error.valid_up_to(),
        _ => bytes.len(),
    };
    let kept = &bytes[skip.min(whole)..whole];
    Ok(BackgroundOutput {
        text: String::from_utf8_lossy(kept).into_owned(),
        start: start + skip.min(whole) as u64,
        end: start + whole as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(session: i64, id: &str, state: BackgroundState) -> Reported {
        let started = state == BackgroundState::Running;
        let event = AgentEvent::Background {
            task_id: id.to_string(),
            description: if started {
                format!("run {id}")
            } else {
                String::new()
            },
            kind: "local_bash".to_string(),
            state,
            summary: (!started).then(|| format!("{id} ended")),
            // Only the start names the call here: the end leaves it out.
            tool_use_id: started.then(|| format!("call-{id}")),
            output_file: (!started).then(|| format!("/tmp/{id}.output")),
        };
        (session, format!("session {session}"), event)
    }

    #[test]
    fn a_task_keeps_the_call_that_started_it_past_its_end() {
        let reported = vec![
            report(1, "a", BackgroundState::Running),
            report(1, "a", BackgroundState::Completed),
        ];
        let tasks = listed(reported, &[1]);
        assert_eq!(tasks[0].tool_use_id.as_deref(), Some("call-a"));
        assert_eq!(tasks[0].state, BackgroundState::Completed);
        // The end named where its output went; the start said what it was.
        assert!(tasks[0].output);
        assert_eq!(tasks[0].description, "run a");
    }

    #[test]
    fn output_is_read_only_from_a_regular_output_file_in_claude_codes_folder() {
        let dir = tempfile::TempDir::new().expect("test setup");
        let root = dir.path().join("claude-501");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(root.join("p/s/tasks")).expect("test setup");
        std::fs::create_dir_all(&elsewhere).expect("test setup");
        let inside = root.join("p/s/tasks/b1.output");
        std::fs::write(&inside, "hello\n").expect("test setup");
        let named = |path: &Path| path.to_string_lossy().into_owned();
        let read = readable_output(&named(&inside), &root).expect("reads");
        assert!(read.is_some_and(|file| file.ends_with("b1.output")));
        // Gone is not an error: Claude Code clears its folder when it starts again.
        let gone = readable_output(&named(&root.join("p/s/tasks/gone.output")), &root);
        assert!(matches!(gone, Ok(None)));
        // Outside the folder, not an output file, or a link out of it: refused.
        let outside = elsewhere.join("x.output");
        std::fs::write(&outside, "secret").expect("test setup");
        let other = root.join("p/s/tasks/notes.txt");
        std::fs::write(&other, "x").expect("test setup");
        let link = root.join("p/s/tasks/link.output");
        std::os::unix::fs::symlink(&outside, &link).expect("test setup");
        for path in [&outside, &other, &link] {
            let refused = readable_output(&named(path), &root);
            assert!(
                matches!(refused, Err(Error::BackgroundOutput(_))),
                "{path:?}"
            );
        }
    }

    #[test]
    fn a_long_output_is_cut_to_its_latest_lines_and_read_on_from_where_it_ended() {
        let dir = tempfile::TempDir::new().expect("test setup");
        let file = dir.path().join("b1.output");
        let line = "a line of output that goes on for a while\n";
        let long = line.repeat(usize::try_from(OUTPUT_LIMIT).expect("small") / line.len() + 10);
        std::fs::write(&file, &long).expect("test setup");
        let first = read_output(&file, None).expect("reads");
        assert!(first.start > 0);
        assert!(first.text.starts_with("a line"), "no line is shown cut");
        assert_eq!(first.end, u64::try_from(long.len()).expect("small"));
        // Read on from where it ended: only what came since.
        let mut opened = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .expect("test setup");
        std::io::Write::write_all(&mut opened, "the last line\n".as_bytes()).expect("test setup");
        let next = read_output(&file, Some(first.end)).expect("reads");
        assert_eq!(
            (next.text.as_str(), next.start),
            ("the last line\n", first.end)
        );
        // A character cut in two at the end waits for the rest of it.
        std::io::Write::write_all(&mut opened, &"é".as_bytes()[..1]).expect("test setup");
        let half = read_output(&file, Some(next.end)).expect("reads");
        assert_eq!((half.text.as_str(), half.end), ("", next.end));
    }

    fn states(tasks: &[BackgroundTask]) -> Vec<(&str, BackgroundState)> {
        tasks
            .iter()
            .map(|t| (t.task_id.as_str(), t.state))
            .collect()
    }

    #[test]
    fn the_last_word_on_each_thing_counts_and_running_ones_come_first() {
        let reported = vec![
            report(1, "a", BackgroundState::Running),
            report(1, "b", BackgroundState::Running),
            report(1, "a", BackgroundState::Completed),
            report(1, "c", BackgroundState::Running),
            report(1, "c", BackgroundState::Failed),
        ];
        let tasks = listed(reported, &[1]);
        assert_eq!(
            states(&tasks),
            [
                ("b", BackgroundState::Running),
                ("c", BackgroundState::Failed),
                ("a", BackgroundState::Completed),
            ]
        );
        // The end keeps what the start said it was, and adds how it ended.
        assert_eq!(tasks[2].description, "run a");
        assert_eq!(tasks[2].summary.as_deref(), Some("a ended"));
    }

    #[test]
    fn what_ran_under_an_agent_that_is_gone_is_stopped() {
        let reported = vec![
            report(1, "a", BackgroundState::Running),
            report(2, "a", BackgroundState::Running),
        ];
        assert_eq!(
            states(&listed(reported, &[2])),
            [
                ("a", BackgroundState::Running),
                ("a", BackgroundState::Stopped)
            ]
        );
    }
}
