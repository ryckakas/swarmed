//! What the sessions did, counted: turns, how long they took and what they cost, by day,
//! agent and model, for a front end to draw.
//!
//! Everything here is read back from the stored conversations, so it covers the time before
//! this was written too, as far as those say: a turn's length has only been kept since turns
//! were timed. Costs are the agents' own figures at API prices, as in [`crate::Costs`].

use std::collections::HashMap;

use rusqlite::params;
use serde::Serialize;

use crate::agent::AgentEvent;
use crate::sessions::event_from_json;
use crate::store::Store;
use crate::{Core, Result};

/// What stands for the model of a turn whose agent never said which it ran on.
const UNNAMED_MODEL: &str = "default";

/// The turns of one day on one model of one agent in one task, added up.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurnStats {
    /// The day, as `YYYY-MM-DD` in local time.
    pub day: String,
    /// The task's project.
    pub project: String,
    /// The task.
    pub task: String,
    /// The id of the agent that ran them.
    pub agent: String,
    /// The model they ran on, as the agent named it.
    pub model: String,
    /// How many turns ended.
    pub turns: u32,
    /// How many of those failed or were interrupted.
    pub failed: u32,
    /// What they cost, in US dollars at API prices.
    pub usd: f64,
    /// How long they took in all, in milliseconds, as far as that was measured.
    pub duration_ms: u64,
}

/// One stored event with where it belongs.
struct Row {
    session: i64,
    day: String,
    project: String,
    task: String,
    agent: String,
    /// The model the session was set to, for an agent that does not say what it runs on.
    chosen: Option<String>,
    event: AgentEvent,
}

/// What is remembered of each session while its events go by.
#[derive(Default)]
struct Run {
    /// The agent's running total as last reported in this run of its process.
    total: f64,
    /// The model it last said it runs on.
    model: Option<String>,
}

impl Core {
    /// The sessions' turns added up by day, task, agent and model, the oldest day first.
    /// Closed sessions and the sessions of removed tasks count as long as they are stored.
    pub fn analytics(&self) -> Result<Vec<TurnStats>> {
        let rows = self.store().analytics_rows()?;
        Ok(counted(rows))
    }
}

impl Store {
    fn analytics_rows(&self) -> Result<Vec<Row>> {
        // The event's type is the first thing in its JSON, which keeps these prefix matches.
        let mut statement = self.conn.prepare(
            "SELECT e.session_id, date(e.created_at, 'localtime'), p.name, t.name, s.agent,
                    s.model, e.event
             FROM session_events e
             JOIN sessions s ON s.id = e.session_id
             JOIN tasks t ON t.id = s.task_id
             JOIN projects p ON p.id = t.project_id
             WHERE e.event LIKE '{\"type\":\"turn_done\"%' OR e.event LIKE '{\"type\":\"init\"%'
                OR e.event LIKE '{\"type\":\"applied\"%'
             ORDER BY e.id",
        )?;
        let rows = statement.query_map(params![], |row| {
            let event: String = row.get(6)?;
            Ok(Row {
                session: row.get(0)?,
                day: row.get(1)?,
                project: row.get(2)?,
                task: row.get(3)?,
                agent: row.get(4)?,
                chosen: row.get(5)?,
                event: event_from_json(&event),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// Adds the turns up. What a turn cost is the step from the total its agent reported before
/// it in the same run of its process, as that total starts again whenever the agent does.
fn counted(rows: Vec<Row>) -> Vec<TurnStats> {
    let mut runs: HashMap<i64, Run> = HashMap::new();
    let mut stats: Vec<TurnStats> = Vec::new();
    for row in rows {
        let run = runs.entry(row.session).or_default();
        let (failed, cost, took) = match row.event {
            AgentEvent::Init { model, .. } => {
                run.total = 0.0;
                run.model = named(model).or_else(|| run.model.take());
                continue;
            }
            AgentEvent::Applied { model, .. } => {
                run.model = named(model).or_else(|| run.model.take());
                continue;
            }
            AgentEvent::TurnDone {
                is_error,
                cost_usd,
                duration_ms,
                ..
            } => (is_error, cost_usd, duration_ms),
            _ => continue,
        };
        let usd = cost.map_or(0.0, |total| step(run, total));
        let model = run.model.clone().or(row.chosen);
        let model = model.unwrap_or_else(|| UNNAMED_MODEL.to_string());
        let bucket = bucket(
            &mut stats,
            (row.day, row.project, row.task, row.agent, model),
        );
        bucket.turns += 1;
        bucket.failed += u32::from(failed);
        bucket.usd += usd;
        bucket.duration_ms += took.unwrap_or(0);
    }
    stats.sort_by(|a, b| a.day.cmp(&b.day));
    stats
}

/// A model's name, unless it is empty.
fn named(model: String) -> Option<String> {
    (!model.trim().is_empty()).then_some(model)
}

/// What a turn cost, given the total its agent reports after it.
fn step(run: &mut Run, total: f64) -> f64 {
    let last = std::mem::replace(&mut run.total, total);
    // A total below the last one means a run started unnoticed: it counts whole.
    if total >= last { total - last } else { total }
}

/// Where a turn is counted, made first if nothing was counted there yet.
type Place = (String, String, String, String, String);

fn bucket(stats: &mut Vec<TurnStats>, (day, project, task, agent, model): Place) -> &mut TurnStats {
    let found = stats.iter().position(|s| {
        s.day == day
            && s.project == project
            && s.task == task
            && s.agent == agent
            && s.model == model
    });
    let at = found.unwrap_or_else(|| {
        stats.push(TurnStats {
            day,
            project,
            task,
            agent,
            model,
            turns: 0,
            failed: 0,
            usd: 0.0,
            duration_ms: 0,
        });
        stats.len() - 1
    });
    &mut stats[at]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(session: i64, day: &str, event: AgentEvent) -> Row {
        Row {
            session,
            day: day.to_string(),
            project: "demo".to_string(),
            task: "one".to_string(),
            agent: "claude".to_string(),
            chosen: None,
            event,
        }
    }

    fn init(model: &str) -> AgentEvent {
        AgentEvent::Init {
            agent_session_id: String::new(),
            model: model.to_string(),
            permission_mode: String::new(),
            api_key_source: String::new(),
            mcp_servers: vec![],
            skills: 0,
            slash_commands: vec![],
        }
    }

    fn done(total: Option<f64>, ms: Option<u64>, is_error: bool) -> AgentEvent {
        AgentEvent::TurnDone {
            is_error,
            result: None,
            cost_usd: total,
            duration_ms: ms,
        }
    }

    #[test]
    fn turns_are_added_up_by_day_and_model_with_each_turns_own_cost() {
        let stats = counted(vec![
            row(1, "2026-11-19", init("opus")),
            row(1, "2026-11-19", done(Some(0.5), Some(1000), false)),
            row(1, "2026-11-19", done(Some(0.75), Some(2000), true)),
            // The agent started again, so its total did too.
            row(1, "2026-11-20", init("opus")),
            row(1, "2026-11-20", done(Some(0.25), None, false)),
            row(2, "2026-11-19", init("haiku")),
            row(2, "2026-11-19", done(None, Some(500), false)),
        ]);
        let seen: Vec<_> = stats
            .iter()
            .map(|s| {
                (
                    s.day.as_str(),
                    s.model.as_str(),
                    s.turns,
                    s.failed,
                    s.usd,
                    s.duration_ms,
                )
            })
            .collect();
        assert_eq!(
            seen,
            [
                ("2026-11-19", "opus", 2, 1, 0.75, 3000),
                ("2026-11-19", "haiku", 1, 0, 0.0, 500),
                ("2026-11-20", "opus", 1, 0, 0.25, 0),
            ]
        );
    }

    #[test]
    fn a_turn_without_a_named_model_counts_under_the_one_the_session_was_set_to() {
        let mut chosen = row(1, "2026-11-20", done(None, None, false));
        chosen.chosen = Some("gpt-5".to_string());
        let stats = counted(vec![chosen, row(2, "2026-11-20", done(None, None, false))]);
        let models: Vec<_> = stats.iter().map(|s| s.model.as_str()).collect();
        assert_eq!(models, ["gpt-5", UNNAMED_MODEL]);
    }
}
