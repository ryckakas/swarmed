//! Finished work put away, by hand or by the user's own rules (#375).
//!
//! A task whose pull request merged, or one the user stopped working on, used to stay in the
//! sidebar with its agents and its stack running until it was removed, and removal takes the
//! worktree and the branch too. Putting a task away is the step between:
//!
//! - **What it does.** The task's agents are stopped, its stack and its browser, and it is
//!   marked done (`tasks.done_at`, with why), which front ends list apart. Its sessions stay open unless
//!   `Settings::put_away_closes_sessions` says to close them: the user often keeps them for
//!   what they say. Remote Control is turned off and continuations are cancelled, since
//!   either would start an agent again by itself. Nothing is deleted: the worktree, the
//!   branch, the notes and every conversation stay.
//! - **Sessions of other tasks** that work for its sessions are left alone, unless the user
//!   asks to close them too (`close_workers`, #470). Only those sessions are closed then;
//!   their tasks stay. A rule never asks.
//! - **Coming back.** [`Core::task_undone`] brings a task back, and so does what works on it
//!   again: reopening one of its sessions, starting one of its agents (which a message to one
//!   of its sessions does), or starting its stack. When it came back is kept
//!   (`tasks.back_at`), and the rules count from then.
//! - **By itself, only where the user says so.** Two rules, each off until turned on and with
//!   its own number of days: a task whose pull request merged is put away that long after the
//!   merge (GitHub's `mergedAt`), and a task none of whose sessions recorded anything for that
//!   long. A terminal or a running stack is not activity. A task brought back after its merge
//!   is not put away again for that merge. Agents cannot turn a rule on through `configure`:
//!   a rule stops agents by itself.
//! - **Never while work goes on:** a session at work or waiting for the user, work an agent
//!   left running, a session another Swarmed runs, a session that goes on by itself when a
//!   usage limit resets, a session snoozed until a time to come (the user asked to be reminded
//!   of it: `snooze.rs`), or a setup under way. The next look tries again.
//! - **One Swarmed looks**, the one that keeps house ([`follow_put_away`]), once a minute.
//! - **Every front end follows:** a change is published as [`AgentEvent::TreeChanged`], and
//!   another Swarmed sees the row change (`elsewhere.rs`).
//! - **Removed only where the user says so** (`Settings::put_away_remove_days`, off at 0, up
//!   to 365 days). The days count from `tasks.done_at`, whether the task was put away by hand
//!   or by a rule; a task brought back has none, so its count starts again the next time it is
//!   put away. The removal is the user's forced Remove… with the branch
//!   ([`Core::task_remove`]): uncommitted changes and untracked files are lost, on purpose,
//!   since that is what the user turned on; a branch with commits found nowhere else is kept,
//!   and that is said; a remote branch is never touched. Only the house looks, never while
//!   work goes on, and each removal is said to the user ([`AgentEvent::TaskRemoved`], with
//!   what the removal passed over). Agents cannot turn it on through `configure`: it deletes
//!   work by itself.
//! - **To the clean-up a done task is still a task like any other**: its worktree and its
//!   branch are never offered there.

use std::sync::Weak;
use std::time::Duration;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::account_usage::seconds_now;
use crate::agent::AgentEvent;
use crate::error::{Error, Result};
use crate::hooks::OutputSink;
use crate::parents::deepest_first;
use crate::pulls::PullState;
use crate::sessions::{Session, is_busy_status};
use crate::settings::Settings;
use crate::setups::SettingUp;
use crate::snooze::snoozed_now;
use crate::store::{Project, Store, Task};
use crate::{Core, RemoveOptions, continuation, lock};

/// How often [`follow_put_away`] looks for tasks the user's settings put away or remove.
const LOOK_EVERY: Duration = Duration::from_mins(1);

/// A day, in seconds.
const DAY: i64 = 86_400;

/// The most days a rule waits.
pub(crate) const MAX_DAYS: u32 = 365;

/// Why a task was put away.
///
/// Kept with the task as text, so its kinds are only added, never renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoneWhy {
    /// The user put it away.
    Hand,
    /// Its pull request merged, and the user's rule put it away.
    Merged,
    /// Nothing happened in its sessions for long, and the user's rule put it away.
    Idle,
}

impl DoneWhy {
    /// As it is kept.
    fn as_str(self) -> &'static str {
        match self {
            Self::Hand => "hand",
            Self::Merged => "merged",
            Self::Idle => "idle",
        }
    }

    /// As it was kept; by hand for anything this version does not know.
    pub(crate) fn parse(kept: &str) -> Self {
        match kept {
            "merged" => Self::Merged,
            "idle" => Self::Idle,
            _ => Self::Hand,
        }
    }
}

/// How a task is put away: why, and whether the sessions of other tasks that work for its
/// sessions are closed with it.
#[derive(Debug, Clone, Copy)]
struct PutAway {
    why: DoneWhy,
    close_workers: bool,
}

/// What putting a task away did ([`Core::task_done`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskDone {
    /// The sessions of other tasks that worked for its sessions and were closed with it, the
    /// deepest first. None unless asked for, and none for a task that was done already.
    pub workers_closed: Vec<i64>,
    /// What went wrong on the way, to be said: a `stop` that failed, or a stack that would not
    /// end.
    pub notes: Vec<String>,
}

/// The user's rules for putting tasks away by themselves: after how many days, for each rule
/// that is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Rules {
    /// Days after its pull request merged.
    merged: Option<u32>,
    /// Days without anything in its sessions.
    idle: Option<u32>,
}

impl Rules {
    /// The rules the settings turn on.
    pub(crate) fn of(settings: &Settings) -> Self {
        Self {
            merged: settings
                .put_away_merged
                .then_some(settings.put_away_merged_days),
            idle: settings
                .put_away_idle
                .then_some(settings.put_away_idle_days),
        }
    }

    /// Whether any rule is on.
    fn any(self) -> bool {
        self.merged.is_some() || self.idle.is_some()
    }
}

/// What the rules look at in a task, in seconds since 1970.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Standing {
    /// When its pull request merged, if it did and GitHub said when.
    merged: Option<i64>,
    /// The latest of when it was made, when one of its sessions last recorded anything, and
    /// when it was brought back.
    active: i64,
    /// When it was last brought back, if ever.
    back: Option<i64>,
}

/// Whether a rule's days are up for a task now, and which rule's. The merge first: it says
/// more. Whether work goes on in the task is asked apart, and only of one that is due.
pub(crate) fn due(rules: Rules, standing: &Standing, now: i64) -> Option<DoneWhy> {
    let after = |days: u32| i64::from(days) * DAY;
    // Brought back since the merge, it was kept on purpose: that merge puts it away no more.
    let merged = standing
        .merged
        .filter(|at| standing.back.is_none_or(|back| back < *at));
    if let (Some(days), Some(at)) = (rules.merged, merged)
        && now >= at + after(days)
    {
        return Some(DoneWhy::Merged);
    }
    let idle = rules.idle?;
    (now >= standing.active + after(idle)).then_some(DoneWhy::Idle)
}

/// Whether a task put away at `done_at` has been put away for `days` by `now`, all in seconds
/// since 1970 but the days. Never at 0 days, which is how the settings say never.
pub(crate) fn removal_due(days: u32, done_at: i64, now: i64) -> bool {
    days > 0 && now >= done_at + i64::from(days) * DAY
}

/// Whether `task`, read again just before its removal, is still the one a look listed as
/// `listed` and still put away for `days` by `now`: one brought back in the meantime, or a
/// setting turned off, is left.
pub(crate) fn still_due(listed: i64, task: &Task, days: u32, now: i64) -> bool {
    task.id == listed
        && !task.archived
        && task
            .done_at
            .is_some_and(|done_at| removal_due(days, done_at, now))
}

/// Seconds since 1970 of a time as GitHub writes it, `2026-11-20T10:00:00Z`, with or without
/// fractions of a second; `None` for anything else, and for a time not after 1970, which is
/// how GitHub says "never".
pub(crate) fn seconds_of(utc: &str) -> Option<i64> {
    let (date, time) = utc.strip_suffix('Z')?.split_once('T')?;
    let numbers = |text: &str, sep: char| -> Option<[i64; 3]> {
        let mut parts = text.splitn(3, sep).map(|part| part.parse::<i64>().ok());
        Some([parts.next()??, parts.next()??, parts.next()??])
    };
    let [year, month, day] = numbers(date, '-')?;
    let whole = time.split('.').next()?;
    let [hour, minute, second] = numbers(whole, ':')?;
    let in_range = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..=60).contains(&second);
    if !in_range {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * DAY + hour * 3600 + minute * 60 + second;
    (seconds > 0).then_some(seconds)
}

/// Days from 1970-01-01 to a date of the Gregorian calendar (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    // Counted from March, so that the leap day is the last of the year.
    let from_march = (month + 9) % 12;
    let of_year = (153 * from_march + 2) / 5 + day - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_cycle - 719_468
}

impl Store {
    /// Marks a task done, and says whether it was not already: one done, or removed, is left.
    fn task_set_done(&self, task: i64, at: i64, why: DoneWhy) -> Result<bool> {
        let sql = "UPDATE tasks SET done_at = ?2, done_why = ?3 \
                   WHERE id = ?1 AND done_at IS NULL AND archived_at IS NULL";
        Ok(self.conn.execute(sql, params![task, at, why.as_str()])? > 0)
    }

    /// Brings a done task back, noting when, and says whether it was done.
    fn task_bring_back(&self, task: i64, at: i64) -> Result<bool> {
        let sql = "UPDATE tasks SET done_at = NULL, done_why = NULL, back_at = ?2 \
                   WHERE id = ?1 AND done_at IS NOT NULL";
        Ok(self.conn.execute(sql, params![task, at])? > 0)
    }

    /// When a task was last brought back, and the latest of that, when it was made and when
    /// one of its sessions last recorded anything, closed ones included.
    fn task_activity(&self, task: i64) -> Result<(Option<i64>, i64)> {
        // Each session's latest event is its last by id, which its index finds at once.
        let sql = "SELECT t.back_at, MAX(\
                     CAST(strftime('%s', t.created_at) AS INTEGER), \
                     COALESCE(t.back_at, 0), \
                     COALESCE((SELECT MAX(CAST(strftime('%s', e.created_at) AS INTEGER)) \
                       FROM session_events e WHERE e.id IN (\
                         SELECT MAX(id) FROM session_events WHERE session_id IN \
                           (SELECT id FROM sessions WHERE task_id = t.id) \
                         GROUP BY session_id)), 0)) \
                   FROM tasks t WHERE t.id = ?1";
        let found = self
            .conn
            .query_row(sql, params![task], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        Ok(found.unwrap_or((None, 0)))
    }
}

impl Core {
    /// Puts a task away: stops its agents and its stack, closes its sessions if the settings
    /// say to, and lists it with the done ones. Nothing is deleted, and it comes back with
    /// [`Core::task_undone`] or once something works on it again. Returns what went wrong on
    /// the way, to be said, and the sessions it closed ([`TaskDone`]). Refused for a chat,
    /// while another Swarmed runs one of its agents ([`Error::RunsElsewhere`]) and while it is
    /// being set up. A task done already is left as it is, and nothing is closed for it.
    ///
    /// With `close_workers`, the open sessions of other tasks that work for its sessions,
    /// however far down ([`Core::task_descendants_elsewhere`]), are closed first, the deepest
    /// first; their tasks stay as they are. Refused, with nothing closed or stopped, while
    /// another Swarmed runs one of those too.
    pub async fn task_done(
        &self,
        project: &str,
        name: &str,
        close_workers: bool,
        sink: OutputSink<'_>,
    ) -> Result<TaskDone> {
        let (project, task) = self.repo_task(project, name)?;
        let how = PutAway {
            why: DoneWhy::Hand,
            close_workers,
        };
        self.put_away(&project, &task, how, sink).await
    }

    /// Brings a done task back to the others. Nothing is started: its agents start with the
    /// next message, and its stack when asked. A task that is not done is left as it is.
    pub fn task_undone(&self, project: &str, name: &str) -> Result<()> {
        let (_, task) = self.task(project, name)?;
        self.bring_back(task.id)
    }

    /// Brings task `id` back if it is done, and tells every front end.
    pub(crate) fn bring_back(&self, id: i64) -> Result<()> {
        if self.store().task_bring_back(id, seconds_now())? {
            self.live.publish_apart(AgentEvent::TreeChanged);
        }
        Ok(())
    }

    /// Brings back the task of session `id`, if it is done: something works on it again.
    pub(crate) fn bring_back_session(&self, id: i64) -> Result<()> {
        let task = self.session(id)?.task_id;
        self.bring_back(task)
    }

    /// Puts `task` away as `how` says, as [`Core::task_done`] does.
    async fn put_away(
        &self,
        project: &Project,
        task: &Task,
        how: PutAway,
        sink: OutputSink<'_>,
    ) -> Result<TaskDone> {
        if task.done_at.is_some() {
            return Ok(TaskDone::default());
        }
        let workers = if how.close_workers {
            deepest_first(&self.descendants_elsewhere(task)?)
        } else {
            Vec::new()
        };
        let mut ids = self.store().session_ids(task.id)?;
        ids.extend(&workers);
        self.none_run_elsewhere(&ids)?;
        if matches!(self.setting_up(task), Some(SettingUp::Here { .. })) {
            return Err(Error::SetupRunning(task.name.clone()));
        }
        for worker in &workers {
            self.close_one(*worker)?;
        }
        let close = self.settings()?.put_away_closes_sessions;
        // Read first: the store is let go before the loop, which records in it.
        let open = self.store().sessions(task.id)?;
        for session in open {
            self.session_put_away(session.id, close)?;
        }
        let (config, notes) = self.config_for_removal(project, task);
        let mut notes = notes;
        notes.extend(self.stop_stack(project, &config, task, sink).await);
        notes.extend(self.stop_browser(task.id).await);
        if self
            .store()
            .task_set_done(task.id, seconds_now(), how.why)?
        {
            self.live.publish_apart(AgentEvent::TreeChanged);
        }
        Ok(TaskDone {
            workers_closed: workers,
            notes,
        })
    }

    /// Stops session `id`'s agent for good, as a put away task's: closed with `close`, else
    /// left open with nothing that would start its agent again by itself.
    fn session_put_away(&self, id: i64, close: bool) -> Result<()> {
        if close {
            return self.close_one(id);
        }
        self.remote_control_closing(id)?;
        continuation::cancel(&self.store, &self.live, id)?;
        self.session_stop(id);
        Ok(())
    }

    /// What the rules look at in `task`.
    fn standing(&self, task: &Task) -> Result<Standing> {
        let merged_at = self
            .pull_kept(task.id)?
            .map(|seen| seen.pull)
            .filter(|pull| pull.state == PullState::Merged)
            .and_then(|pull| pull.merged_at.as_deref().and_then(seconds_of));
        let (back_at, active_at) = self.store().task_activity(task.id)?;
        Ok(Standing {
            merged: merged_at,
            active: active_at,
            back: back_at,
        })
    }

    /// Whether work goes on in `task`: a session at work, waiting for the user, run by another
    /// Swarmed (those are `elsewhere`), going on by itself at a reset or snoozed, work an agent
    /// left running, or its setup.
    fn works_on(&self, task: &Task, elsewhere: &[i64]) -> Result<bool> {
        let sessions = self.store().sessions(task.id)?;
        let busy = sessions.iter().any(|session| works(session, elsewhere));
        let setting_up = matches!(
            self.setting_up(task),
            Some(SettingUp::Here { .. } | SettingUp::Elsewhere)
        );
        Ok(busy || setting_up || self.background_running(task.id)?)
    }

    /// One look of the user's rules: puts away each task they say is due, where no work goes
    /// on, and returns those put away with why. Only the Swarmed that keeps house looks, and
    /// nothing is looked at while both rules are off. A task that could not be put away is
    /// tried again at the next look.
    pub async fn tasks_put_away_due(&self) -> Result<Vec<(String, String, DoneWhy)>> {
        if lock(&self.house).is_none() {
            return Ok(Vec::new());
        }
        let rules = Rules::of(&self.settings()?);
        if !rules.any() {
            return Ok(Vec::new());
        }
        let elsewhere = self.sessions_run_elsewhere();
        let mut put = Vec::new();
        for (project, task) in self.repository_tasks(false)? {
            let why = self
                .put_away_if_due(rules, &project, &task, &elsewhere)
                .await?;
            put.extend(why.map(|why| (project.name, task.name, why)));
        }
        Ok(put)
    }

    /// Puts `task` away if a rule says it is due and no work goes on in it, and says why if
    /// it went. One that could not go is not an error: the next look tries again. The days
    /// are looked at first: whether work goes on reads the sessions' histories, and nearly
    /// every task of a look is not due.
    async fn put_away_if_due(
        &self,
        rules: Rules,
        project: &Project,
        task: &Task,
        elsewhere: &[i64],
    ) -> Result<Option<DoneWhy>> {
        let Some(why) = due(rules, &self.standing(task)?, seconds_now()) else {
            return Ok(None);
        };
        if self.works_on(task, elsewhere)? {
            return Ok(None);
        }
        // A rule closes nothing outside the task: only the user can ask for that.
        let how = PutAway {
            why,
            close_workers: false,
        };
        let done = self.put_away(project, task, how, &unsaid).await;
        Ok(done.ok().map(|_| why))
    }

    /// One look of the user's retention: removes each task put away for as many days as the
    /// settings say, where no work goes on, as a forced Remove… does with its branch, tells
    /// every front end, and returns those removed with what the removal passed over. Only the
    /// Swarmed that keeps house looks, and nothing is looked at while the days are 0. A task
    /// that could not be removed is tried again at the next look.
    pub async fn tasks_removed_due(&self) -> Result<Vec<(String, String, Vec<String>)>> {
        if lock(&self.house).is_none() {
            return Ok(Vec::new());
        }
        let days = self.settings()?.put_away_remove_days;
        if days == 0 {
            return Ok(Vec::new());
        }
        let elsewhere = self.sessions_run_elsewhere();
        let mut removed = Vec::new();
        for (project, task) in self.repository_tasks(true)? {
            let notes = self
                .remove_if_due(days, &project, &task, &elsewhere)
                .await?;
            removed.extend(notes.map(|notes| (project.name, task.name, notes)));
        }
        Ok(removed)
    }

    /// Removes `listed`, put away, if it has been for `days` and no work goes on in it, and
    /// says what the removal passed over if it went. One that could not go is not an error:
    /// the next look tries again.
    async fn remove_if_due(
        &self,
        days: u32,
        project: &Project,
        listed: &Task,
        elsewhere: &[i64],
    ) -> Result<Option<Vec<String>>> {
        let due = listed
            .done_at
            .is_some_and(|done_at| removal_due(days, done_at, seconds_now()));
        if !due || self.works_on(listed, elsewhere)? {
            return Ok(None);
        }
        let Some((task, days)) = self.removal_still_due(project, listed)? else {
            return Ok(None);
        };
        // What the window's forced Remove… sends.
        let options = RemoveOptions {
            force: true,
            delete_branch: true,
        };
        let removal = self.task_remove(&project.name, &task.name, options, &unsaid);
        let notes = match removal.await {
            Ok(notes) => notes,
            // Archived, it is gone all the same: only what comes after failed, its branch's
            // deletion with it, and that is said.
            Err(error) if self.removed_anyway(task.id, &error)? => vec![format!(
                "not everything of it could be forgotten, and its branch was not deleted: {error}"
            )],
            // Tried again at the next look, `teardown` included, for as long as it fails:
            // accepted, as for a task a rule could not put away.
            Err(_) => return Ok(None),
        };
        self.live.publish_apart(AgentEvent::TaskRemoved {
            project: project.name.clone(),
            task: task.name.clone(),
            days,
            notes: notes.clone(),
        });
        self.live.publish_apart(AgentEvent::TreeChanged);
        Ok(Some(notes))
    }

    /// `listed` as it is now, with the days the settings say now, if it is still to be
    /// removed. Read again just before it goes: a removal before it in the same look may have
    /// taken minutes, in which it was brought back or the setting was turned off.
    fn removal_still_due(&self, project: &Project, listed: &Task) -> Result<Option<(Task, u32)>> {
        let days = self.settings()?.put_away_remove_days;
        let again = self.store().task_by_name(project.id, &listed.name)?;
        let now = seconds_now();
        let task = again.filter(|task| still_due(listed.id, task, days, now));
        Ok(task.map(|task| (task, days)))
    }

    /// Whether task `id`, whose removal failed with `error`, is gone all the same: archived
    /// before what failed. One that was not there to remove was not removed here.
    fn removed_anyway(&self, id: i64, error: &Error) -> Result<bool> {
        if matches!(error, Error::TaskNotFound { .. }) {
            return Ok(false);
        }
        let row = self.store().task_by_id(id)?;
        Ok(row.is_none_or(|task| task.archived))
    }

    /// Every task in a repository that is done, or every one that is not, as `done` says, with
    /// its repository. Chats are never put away.
    fn repository_tasks(&self, done: bool) -> Result<Vec<(Project, Task)>> {
        let store = self.store();
        let projects = store.projects()?;
        let mut found = Vec::new();
        for project in projects.into_iter().filter(|project| !project.chats) {
            let tasks = store.tasks(project.id, false)?;
            found.extend(done_or_not_of(&project, tasks, done));
        }
        Ok(found)
    }
}

/// Whether `session` works: at work, waiting for the user, going on by itself at a reset,
/// run by another Swarmed, whose sessions are `elsewhere`, or snoozed until a time to come,
/// when the user asked to be reminded of it (`snooze.rs`).
fn works(session: &Session, elsewhere: &[i64]) -> bool {
    is_busy_status(session.status)
        || session.continue_at.is_some()
        || snoozed_now(session)
        || elsewhere.contains(&session.id)
}

/// Where what a rule's putting away, or a removal by the settings, prints goes: nobody asked,
/// so nowhere.
fn unsaid(_: &str) {}

/// Those of `tasks` that are done, or those that are not, as `done` says, each with `project`.
fn done_or_not_of(project: &Project, tasks: Vec<Task>, done: bool) -> Vec<(Project, Task)> {
    let wanted = tasks
        .into_iter()
        .filter(|task| task.done_at.is_some() == done);
    wanted.map(|task| (project.clone(), task)).collect()
}

/// Puts tasks away as the user's rules say, and removes those put away long enough, for a
/// front end that stays.
///
/// It calls [`Core::tasks_put_away_due`] and then [`Core::tasks_removed_due`] once a minute,
/// each of which does nothing unless this is the Swarmed that keeps house and its setting is
/// on. It holds the core only while it looks, and ends once nobody else holds it.
pub async fn follow_put_away(core: Weak<Core>) {
    let mut looks = tokio::time::interval(LOOK_EVERY);
    looks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        looks.tick().await;
        let Some(core) = core.upgrade() else {
            return;
        };
        // A look that failed is made again at the next.
        core.tasks_put_away_due().await.ok();
        core.tasks_removed_due().await.ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn standing(merged_at: Option<i64>, active_at: i64, back_at: Option<i64>) -> Standing {
        Standing {
            merged: merged_at,
            active: active_at,
            back: back_at,
        }
    }

    const BOTH: Rules = Rules {
        merged: Some(1),
        idle: Some(14),
    };

    #[test]
    fn nothing_is_put_away_while_no_rule_is_on() {
        let long_ago = standing(Some(NOW - 100 * DAY), NOW - 100 * DAY, None);
        assert_eq!(due(Rules::default(), &long_ago, NOW), None);
        assert_eq!(due(Rules::of(&Settings::default()), &long_ago, NOW), None);
    }

    #[test]
    fn a_merged_task_goes_its_days_after_the_merge() {
        let merged = |at| standing(Some(at), NOW, None);
        assert_eq!(due(BOTH, &merged(NOW - DAY + 60), NOW), None);
        assert_eq!(due(BOTH, &merged(NOW - DAY), NOW), Some(DoneWhy::Merged));
        let at_once = Rules {
            merged: Some(0),
            idle: None,
        };
        assert_eq!(due(at_once, &merged(NOW), NOW), Some(DoneWhy::Merged));
        // Without the merged rule, a merge alone puts nothing away.
        let idle_only = Rules {
            merged: None,
            idle: Some(14),
        };
        assert_eq!(due(idle_only, &merged(NOW - 30 * DAY), NOW), None);
    }

    #[test]
    fn a_task_brought_back_after_its_merge_stays() {
        let kept = standing(Some(NOW - 10 * DAY), NOW - DAY, Some(NOW - DAY));
        assert_eq!(due(BOTH, &kept, NOW), None);
        // Brought back before a later merge, that merge counts.
        let merged_since = standing(Some(NOW - 2 * DAY), NOW, Some(NOW - 5 * DAY));
        assert_eq!(due(BOTH, &merged_since, NOW), Some(DoneWhy::Merged));
    }

    #[test]
    fn an_idle_task_goes_its_days_after_anything_last_happened() {
        assert_eq!(due(BOTH, &standing(None, NOW - 13 * DAY, None), NOW), None);
        assert_eq!(
            due(BOTH, &standing(None, NOW - 14 * DAY, None), NOW),
            Some(DoneWhy::Idle)
        );
    }

    #[test]
    fn nothing_is_removed_at_0_days() {
        assert!(!removal_due(0, NOW - 1000 * DAY, NOW));
    }

    #[test]
    fn a_task_put_away_is_removed_its_days_after() {
        assert!(!removal_due(30, NOW - 30 * DAY + 60, NOW));
        assert!(removal_due(30, NOW - 30 * DAY, NOW));
        assert!(removal_due(30, NOW - 31 * DAY, NOW));
    }

    fn put_away_at(done_at: Option<i64>) -> Task {
        Task {
            id: 1,
            project_id: 1,
            name: "one".to_string(),
            branch: "one".to_string(),
            base_ref: "main".to_string(),
            worktree_path: std::path::PathBuf::from("/nowhere"),
            slot: 1,
            setup: crate::store::SetupStatus::Ok,
            setup_reason: None,
            archived: false,
            environment: None,
            pinned: false,
            done_at,
            done_why: done_at.map(|_| DoneWhy::Hand),
            issue: None,
            highlight: None,
        }
    }

    #[test]
    fn a_task_read_again_is_removed_only_if_it_is_still_due() {
        let long_put_away = put_away_at(Some(NOW - 40 * DAY));
        assert!(still_due(1, &long_put_away, 30, NOW));
        // Brought back while an earlier removal of the look went on.
        assert!(!still_due(1, &put_away_at(None), 30, NOW));
        // The setting turned off, or raised past it, in the meantime.
        assert!(!still_due(1, &long_put_away, 0, NOW));
        assert!(!still_due(1, &long_put_away, 60, NOW));
        // Another task of that name since.
        assert!(!still_due(2, &long_put_away, 30, NOW));
        let archived = Task {
            archived: true,
            ..long_put_away
        };
        assert!(!still_due(1, &archived, 30, NOW));
    }

    #[test]
    fn githubs_times_are_read_as_seconds_since_1970() {
        assert_eq!(seconds_of("1970-01-02T00:00:00Z"), Some(DAY));
        assert_eq!(seconds_of("2026-11-20T10:00:00Z"), Some(1_795_168_800));
        assert_eq!(seconds_of("2024-02-29T23:59:59.123Z"), Some(1_709_251_199));
        assert_eq!(seconds_of("0001-01-01T00:00:00Z"), None);
        assert_eq!(seconds_of("2026-11-20 10:00:00"), None);
        assert_eq!(seconds_of("2026-13-20T10:00:00Z"), None);
        assert_eq!(seconds_of(""), None);
    }

    #[test]
    fn why_is_kept_as_text_and_read_back() {
        for why in [DoneWhy::Hand, DoneWhy::Merged, DoneWhy::Idle] {
            assert_eq!(DoneWhy::parse(why.as_str()), why);
        }
        assert_eq!(DoneWhy::parse("later"), DoneWhy::Hand);
    }
}
