//! What a task's agents left running behind their turns.
//!
//! An agent can put a command in the background and carry on, or end its turn with it still
//! running. It says when such a thing starts and when it ends; this reads those two reports
//! back out of the sessions' histories, so a front end can list them and stop the ones that
//! run.

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
}

/// A report from a session's history: whose it is, and the event itself.
type Reported = (i64, String, AgentEvent);

impl Core {
    /// Everything the task's sessions left running, the running ones first, then the ended
    /// ones with the newest first. Closed sessions are left out.
    pub fn task_background(&self, project: &str, task: &str) -> Result<Vec<BackgroundTask>> {
        let (_, task) = self.task(project, task)?;
        let reported = self.store().background_reports(task.id)?;
        let alive = self.running_sessions();
        Ok(listed(reported, &alive))
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

/// Folds the reports into one entry per thing, by the last word on each. What a session's
/// agent left running does not outlive that agent, so it is stopped if the agent is gone.
fn listed(reported: Vec<Reported>, alive: &[i64]) -> Vec<BackgroundTask> {
    let mut tasks: Vec<BackgroundTask> = Vec::new();
    for (session_id, session_title, event) in reported {
        let AgentEvent::Background {
            task_id,
            description,
            kind,
            state,
            summary,
            tool_use_id,
        } = event
        else {
            continue;
        };
        let known = tasks
            .iter()
            .position(|task| task.session_id == session_id && task.task_id == task_id);
        let Some(at) = known else {
            tasks.push(BackgroundTask {
                session_id,
                session_title,
                task_id,
                description,
                kind,
                state,
                summary,
                tool_use_id,
            });
            continue;
        };
        // An end is reported without saying again what the thing was.
        let mut task = tasks.remove(at);
        task.state = state;
        task.summary = summary.or(task.summary);
        task.tool_use_id = task.tool_use_id.or(tool_use_id);
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
