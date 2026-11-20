//! What the agents say their work has cost, summed by day and by task.
//!
//! An agent reports a running total since its process started, and starts again from nothing
//! when a session is resumed. What a turn cost is therefore the step from the total before it,
//! within one run. The figures are the agents' own, at API prices: on a subscription they say
//! what the work would have cost, not what was paid.

use rusqlite::params;
use serde::Serialize;

use crate::agent::AgentEvent;
use crate::{Core, Result};

/// How many tasks are listed, the most expensive first.
const LISTED_TASKS: usize = 12;
/// How many days the daily figures go back.
const LISTED_DAYS: usize = 14;

/// What one task's sessions have cost.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskCost {
    /// The task's project.
    pub project: String,
    /// The task.
    pub task: String,
    /// In US dollars.
    pub usd: f64,
}

/// What was spent on one day.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayCost {
    /// The day, as `YYYY-MM-DD` in local time.
    pub day: String,
    /// In US dollars.
    pub usd: f64,
}

/// What the agents report their work has cost, in US dollars at API prices.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Costs {
    /// Today, in local time.
    pub today: f64,
    /// The last seven days, today included.
    pub week: f64,
    /// Since Swarmed started keeping conversations.
    pub total: f64,
    /// The most expensive tasks, the most expensive first.
    pub tasks: Vec<TaskCost>,
    /// The last days on which something was spent, the newest first.
    pub days: Vec<DayCost>,
}

/// One stored event that matters here, in the order it was stored.
struct Reported {
    session: i64,
    day: String,
    project: String,
    task: String,
    mark: Mark,
}

/// What an event says about an agent's running total.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mark {
    /// The agent's process started: it counts from nothing again.
    Started,
    /// The total it reported at the end of a turn.
    Total(f64),
}

/// One turn's cost: where and when it was spent.
struct Spent {
    day: String,
    project: String,
    task: String,
    usd: f64,
}

/// What each turn cost: the step from the total reported before it in the same run.
fn spent(reported: Vec<Reported>) -> Vec<Spent> {
    let mut before: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
    let mut steps = Vec::new();
    for event in reported {
        let Mark::Total(total) = event.mark else {
            before.remove(&event.session);
            continue;
        };
        let last = before.insert(event.session, total).unwrap_or(0.0);
        // A total below the last one means a run started unnoticed: it counts whole.
        let usd = if total >= last { total - last } else { total };
        if usd > 0.0 {
            steps.push(Spent {
                day: event.day,
                project: event.project,
                task: event.task,
                usd,
            });
        }
    }
    steps
}

/// The steps summed up, with `today` and the day seven days back as `YYYY-MM-DD`.
fn summed(steps: &[Spent], today: &str, week_start: &str) -> Costs {
    let mut costs = Costs::default();
    for step in steps {
        costs.total += step.usd;
        if step.day.as_str() == today {
            costs.today += step.usd;
        }
        if step.day.as_str() >= week_start {
            costs.week += step.usd;
        }
        add_to(
            &mut costs.days,
            step,
            |day| day.day == step.day,
            || DayCost {
                day: step.day.clone(),
                usd: 0.0,
            },
        )
        .usd += step.usd;
        add_to(
            &mut costs.tasks,
            step,
            |task| task.project == step.project && task.task == step.task,
            || TaskCost {
                project: step.project.clone(),
                task: step.task.clone(),
                usd: 0.0,
            },
        )
        .usd += step.usd;
    }
    costs.days.sort_by(|a, b| b.day.cmp(&a.day));
    costs.days.truncate(LISTED_DAYS);
    costs.tasks.sort_by(|a, b| b.usd.total_cmp(&a.usd));
    costs.tasks.truncate(LISTED_TASKS);
    costs
}

/// The entry of `list` that `is` picks out, made first if it is not there yet.
fn add_to<'a, T>(
    list: &'a mut Vec<T>,
    _step: &Spent,
    is: impl Fn(&T) -> bool,
    make: impl FnOnce() -> T,
) -> &'a mut T {
    let at = list.iter().position(is).unwrap_or_else(|| {
        list.push(make());
        list.len() - 1
    });
    &mut list[at]
}

/// A stored event with where it belongs: session, day, project, task, and the event itself.
type Row = (i64, String, String, String, String);

/// A row as something reported, if its event is one that matters here.
fn reported((session, day, project, task, event): Row) -> Option<Reported> {
    Some(Reported {
        session,
        day,
        project,
        task,
        mark: mark_in(&event)?,
    })
}

/// What a stored event says about the running total, if anything.
fn mark_in(event: &str) -> Option<Mark> {
    match serde_json::from_str::<AgentEvent>(event).ok()? {
        AgentEvent::Init { .. } => Some(Mark::Started),
        AgentEvent::TurnDone { cost_usd, .. } => cost_usd.map(Mark::Total),
        _ => None,
    }
}

impl Core {
    /// What the agents report their work has cost: today, this week, in all, by task and by
    /// day. Closed sessions and removed tasks' sessions that are still stored count too.
    pub fn costs(&self) -> Result<Costs> {
        let store = self.store();
        let (today, week_start): (String, String) = store.conn.query_row(
            "SELECT date('now', 'localtime'), date('now', 'localtime', '-6 days')",
            params![],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut statement = store.conn.prepare(
            "SELECT e.session_id, date(e.created_at, 'localtime'), p.name, t.name, e.event
             FROM session_events e
             JOIN sessions s ON s.id = e.session_id
             JOIN tasks t ON t.id = s.task_id
             JOIN projects p ON p.id = t.project_id
             WHERE e.event LIKE '{\"type\":\"turn_done\"%' OR e.event LIKE '{\"type\":\"init\"%'
             ORDER BY e.id",
        )?;
        let rows = statement.query_map(params![], |row| {
            let event: String = row.get(4)?;
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, event))
        })?;
        let rows = rows.collect::<rusqlite::Result<Vec<Row>>>()?;
        let reported = rows.into_iter().filter_map(reported).collect();
        Ok(summed(&spent(reported), &today, &week_start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(session: i64, day: &str, task: &str, total: Option<f64>) -> Reported {
        Reported {
            session,
            day: day.to_string(),
            project: "demo".to_string(),
            task: task.to_string(),
            mark: total.map_or(Mark::Started, Mark::Total),
        }
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_turn_costs_the_step_from_the_total_before_it() {
        let steps = spent(vec![
            at(1, "2026-11-19", "one", None),
            at(1, "2026-11-19", "one", Some(0.10)),
            at(1, "2026-11-20", "one", Some(0.25)),
            // The session was resumed: its agent counts from nothing again.
            at(1, "2026-11-20", "one", None),
            at(1, "2026-11-20", "one", Some(0.05)),
            // Another session's totals are its own.
            at(2, "2026-11-20", "two", Some(1.00)),
            // A turn that cost nothing is not a step.
            at(2, "2026-11-20", "two", Some(1.00)),
        ]);
        let usd: Vec<f64> = steps.iter().map(|step| step.usd).collect();
        assert_eq!(usd.len(), 4);
        assert!(near(usd[0], 0.10) && near(usd[1], 0.15) && near(usd[2], 0.05));
        assert!(near(usd[3], 1.00));
    }

    #[test]
    fn a_total_that_went_down_counts_whole() {
        let steps = spent(vec![
            at(1, "d", "one", Some(0.50)),
            at(1, "d", "one", Some(0.20)),
        ]);
        assert!(near(steps[1].usd, 0.20));
    }

    #[test]
    fn steps_are_summed_by_day_and_by_task() {
        let steps = spent(vec![
            at(1, "2026-11-10", "one", Some(2.0)),
            at(2, "2026-11-19", "two", Some(0.5)),
            at(3, "2026-11-20", "one", Some(0.25)),
        ]);
        let costs = summed(&steps, "2026-11-20", "2026-11-14");
        assert!(near(costs.today, 0.25));
        assert!(near(costs.week, 0.75));
        assert!(near(costs.total, 2.75));
        assert_eq!(costs.days[0].day, "2026-11-20");
        assert_eq!(costs.days.len(), 3);
        assert_eq!(costs.tasks[0].task, "one");
        assert!(near(costs.tasks[0].usd, 2.25));
        assert_eq!(costs.tasks[1].task, "two");
    }

    #[test]
    fn only_starts_and_ended_turns_with_a_cost_are_read() {
        assert_eq!(mark_in(r#"{"type":"text","text":"hi"}"#), None);
        let done = |cost: &str| {
            format!(r#"{{"type":"turn_done","is_error":false,"result":null,"cost_usd":{cost}}}"#)
        };
        assert_eq!(mark_in(&done("0.5")), Some(Mark::Total(0.5)));
        assert_eq!(mark_in(&done("null")), None);
    }
}
