//! A session that goes on by itself when the usage limit that stopped it resets.
//!
//! A turn a usage limit refused leaves the work where it stopped until someone writes to the
//! session again. Claude Code's report of the refusal says when the limit resets
//! ([`AgentEvent::RateLimit`]), so the session can be told to go on then: a continuation, set
//! when the user asks, or for every refused turn with `Settings::continue_after_limit`. At its
//! time, [`CONTINUE_MESSAGE`] is put first in the session's queue, ahead of what the user
//! queued meanwhile, and sent as if typed; it is recorded with `at_reset`.
//!
//! - **Kept on the session** (`continue_at`), not in the queue's memory: a reset can be hours
//!   or days away, and Swarmed may quit and start again meanwhile.
//! - **Cancelled by whatever says the user is back:** a message to the session, Stop, closing
//!   or deleting it, removing its task, or cancelling it.
//! - **Never for a reset that has passed:** a refusal whose reset is not later than now sets
//!   nothing, so a stale figure cannot send the message again and again.
//! - **Sent by one Swarmed:** the one that keeps house (`presence.rs`), a few seconds apart
//!   when one limit stopped several sessions ([`follow_resets`]). One that came due while no
//!   Swarmed ran goes when the house is next kept, unless what was last heard of the account
//!   says a limit refuses requests still: it then waits for that limit's reset instead.
//! - **Asks nothing:** the time comes from a report already heard, and nothing asks Claude
//!   Code for its usage first. A limit that had not reset after all refuses the turn again,
//!   which says so, with the new reset.

use std::sync::{Mutex, Weak};
use std::time::Duration;

use rusqlite::params;
use serde_json::Value;

use crate::account_usage::{on_extra_usage, one_models, reached, seconds_now, window_reached};
use crate::agent::{AgentEvent, Auth};
use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::sessions::{Live, is_busy_status};
use crate::settings::Settings;
use crate::store::Store;
use crate::{AccountUsage, Core, lock, queue};

/// What a session is sent when the usage limit that stopped it resets.
pub const CONTINUE_MESSAGE: &str = "Continue where you stopped.";

/// How long the house waits after continuing one session before it continues the next: one
/// limit stops every session it refuses, and they are not all to start again at once.
const APART: Duration = Duration::from_secs(5);

/// How often [`follow_resets`] looks for a continuation that is due.
const LOOK_EVERY: Duration = Duration::from_secs(1);

/// Whether a usage report says a limit refused the turn: reached, with no extra usage paying.
fn refuses(status: &str, raw: &Value) -> bool {
    reached(status) && !on_extra_usage(raw)
}

/// When the limit a usage report says refused the turn resets, in seconds since 1970; `None`
/// for a report of anything else, or one that does not say.
pub(crate) fn refused_until_reset(event: &AgentEvent) -> Option<i64> {
    match event {
        AgentEvent::RateLimit {
            status,
            resets_at,
            raw,
        } if refuses(status, raw) => *resets_at,
        _ => None,
    }
}

/// When every limit that refused a turn has reset, by the turn's usage reports: the latest of
/// their resets, if it is later than `now`. One that has passed is nothing to wait for.
fn reset_of(reports: &[AgentEvent], now: i64) -> Option<i64> {
    let resets = reports.iter().filter_map(refused_until_reset);
    resets.max().filter(|at| *at > now)
}

/// Until when the account's usage, as last heard, says requests are refused: the latest reset
/// still to come of a window used up, unless extra usage can pay past it. `None` for not.
fn refused_until(usage: &AccountUsage, now: i64) -> Option<i64> {
    if usage.extra_can_pay(now) {
        return None;
    }
    // One model's weekly limit refuses that model alone: a session on another goes on.
    let windows = usage
        .windows
        .iter()
        .filter(|window| !one_models(&window.kind));
    windows
        .filter(|window| window_reached(window))
        .filter_map(|window| window.resets_at)
        .filter(|at| *at > now)
        .max()
}

impl Store {
    /// Sets when session `id` goes on by itself, or that it does not.
    fn session_set_continue_at(&self, id: i64, at: Option<i64>) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET continue_at = ?2 WHERE id = ?1",
            params![id, at],
        )?;
        Ok(())
    }

    /// Sets the continuation of session `id` to `to` if it is still due at `now`, and says
    /// whether it was: one cancelled since it was listed, or whose session was closed, is left.
    fn continuation_move_if_due(&self, id: i64, now: i64, to: Option<i64>) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE sessions SET continue_at = ?3 \
             WHERE id = ?1 AND continue_at <= ?2 AND closed_at IS NULL",
            params![id, now, to],
        )?;
        Ok(changed > 0)
    }

    /// The usage reports session `id` recorded since its latest message: those of the turn
    /// that message started, and of the turns its agent took up by itself after it.
    fn reports_since_message(&self, id: i64) -> Result<Vec<AgentEvent>> {
        // The event's type is the first thing in its JSON, which keeps these prefix matches.
        let sql = "SELECT event FROM session_events WHERE session_id = ?1 AND event LIKE ?2 \
                   AND id > COALESCE((SELECT MAX(id) FROM session_events \
                   WHERE session_id = ?1 AND event LIKE ?3), 0) ORDER BY id";
        let reports = "{\"type\":\"rate_limit\"%";
        let messages = "{\"type\":\"user_message\"%";
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params![id, reports, messages], |row| {
            row.get::<_, String>(0)
        })?;
        let texts = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(texts
            .iter()
            .map(|text| crate::sessions::event_from_json(text))
            .collect())
    }

    /// The open sessions of current tasks whose continuation is due at `now`, the one due
    /// first first.
    fn continuations_due(&self, now: i64) -> Result<Vec<i64>> {
        let sql = "SELECT s.id FROM sessions s JOIN tasks t ON t.id = s.task_id \
                   WHERE s.continue_at <= ?1 AND s.closed_at IS NULL \
                   AND t.archived_at IS NULL ORDER BY s.continue_at, s.id";
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params![now], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// Sets when session `id` goes on by itself, or that it does not, and tells every front end.
fn set(store: &Mutex<Store>, live: &Live, id: i64, at: Option<i64>) -> Result<()> {
    lock(store).session_set_continue_at(id, at)?;
    live.note(store, id, AgentEvent::Continuation { at })
}

/// Moves the continuation of session `id` to `to` and tells every front end, if it is still
/// due at `now`; says whether it was. A cancel, a message or a close that came since the due
/// ones were listed, in this Swarmed or another, is not undone.
fn move_if_due(
    store: &Mutex<Store>,
    live: &Live,
    id: i64,
    now: i64,
    to: Option<i64>,
) -> Result<bool> {
    let moved = lock(store).continuation_move_if_due(id, now, to)?;
    if moved {
        live.note(store, id, AgentEvent::Continuation { at: to })?;
    }
    Ok(moved)
}

/// Cancels session `id`'s continuation, if it has one: the user is back. A session that is
/// not there has none, and what was asked of it says so in its own words.
pub(crate) fn cancel(store: &Mutex<Store>, live: &Live, id: i64) -> Result<()> {
    let session = lock(store).session_by_id(id);
    let waits = session.is_ok_and(|session| session.continue_at.is_some());
    if waits {
        set(store, live, id, None)?;
    }
    Ok(())
}

/// Sets session `id` to go on when the limit that refused its turn resets, at `reset`, if the
/// user's settings say so for every refused turn. A reset that has passed sets nothing. One
/// later than the session waits for already takes its place: the session goes on only once
/// every limit that refused it has reset.
pub(crate) fn after_refusal(store: &Mutex<Store>, live: &Live, paths: &Paths, id: i64, reset: i64) {
    if reset <= seconds_now() {
        return;
    }
    let wanted = Settings::load(&paths.settings()).is_ok_and(|s| s.continue_after_limit);
    let waits = lock(store)
        .session_by_id(id)
        .ok()
        .and_then(|session| session.continue_at);
    if wanted && waits.is_none_or(|at| at < reset) {
        // An error only means the session is gone.
        set(store, live, id, Some(reset)).ok();
    }
}

impl Core {
    /// Sets a session to go on by itself when the usage limit that stopped its latest turn
    /// resets, and says when: [`CONTINUE_MESSAGE`] is sent to it then, as if typed, ahead of
    /// what waits in its queue. With several limits, the latest of their resets. Refused when
    /// no limit that is still to reset stopped that turn ([`Error::NoLimitToWaitFor`]), and for
    /// a closed session. Only the Swarmed that keeps house sends it: the app, not the command
    /// line.
    pub fn session_continue_at_reset(&self, id: i64) -> Result<i64> {
        if self.store().session_is_closed(id)? {
            return Err(Error::SessionClosed(id));
        }
        let reports = self.store().reports_since_message(id)?;
        let at = reset_of(&reports, seconds_now()).ok_or(Error::NoLimitToWaitFor(id))?;
        set(&self.store, &self.live, id, Some(at))?;
        Ok(at)
    }

    /// Cancels a session's continuation, if it has one: it no longer goes on by itself.
    pub fn session_continue_cancel(&self, id: i64) -> Result<()> {
        self.session(id)?;
        cancel(&self.store, &self.live, id)
    }

    /// Continues the session whose continuation came due first, if this is the Swarmed that
    /// keeps house, and says which. If what was last heard of the account says a limit
    /// refuses requests still, every continuation that is due waits for its reset instead,
    /// and none goes now. Must be called from within a tokio runtime.
    pub fn continue_due(&self) -> Result<Option<i64>> {
        if lock(&self.house).is_none() {
            return Ok(None);
        }
        let now = seconds_now();
        let due = self.store().continuations_due(now)?;
        let Some(&first) = due.first() else {
            return Ok(None);
        };
        let usage = self.live.usage.now(&self.store)?;
        if let Some(until) = refused_until(&usage, now) {
            self.wait_until(&due, now, until)?;
            return Ok(None);
        }
        // Taken off before anything else, and only if it still stands.
        if !move_if_due(&self.store, &self.live, first, now, None)? {
            return Ok(None);
        }
        self.continue_now(first)?;
        Ok(Some(first))
    }

    /// Has the continuations of `sessions` that are still due at `now` wait until `until`,
    /// when the limit that refuses requests now resets.
    fn wait_until(&self, sessions: &[i64], now: i64, until: i64) -> Result<()> {
        for id in sessions {
            move_if_due(&self.store, &self.live, *id, now, Some(until))?;
        }
        Ok(())
    }

    /// Sends session `id` [`CONTINUE_MESSAGE`] through its queue, first in it, starting its
    /// agent if it is not running. Its continuation was taken off before, so that it never
    /// goes twice: a message that cannot go waits in the queue, where the user sees it. One
    /// whose agent another Swarmed runs goes once that run is over; one whose agent is busy
    /// goes when its turn ends.
    fn continue_now(&self, id: i64) -> Result<()> {
        self.live.queues.push_front_at_reset(id, CONTINUE_MESSAGE);
        queue::publish(&self.store, &self.live, id)?;
        if self.runs_elsewhere(id) {
            self.queued_for(id);
            return Ok(());
        }
        self.session_start(id, Auth::default())?;
        // Held until the message has gone: a turn's end meanwhile cannot send it too.
        let _pressing = self.live.queues.pressing();
        let busy = self
            .session(id)
            .is_ok_and(|session| is_busy_status(session.status));
        if busy {
            return Ok(());
        }
        queue::send_next_by_itself(&self.outbox(), id)
    }
}

/// Continues sessions as their continuations come due, for a front end that stays.
///
/// It calls [`Core::continue_due`] every second, and pauses a few seconds after each session
/// it continued, so that sessions one limit stopped start again one by one. It holds the core
/// only while it looks, and ends once nobody else holds it.
pub async fn follow_resets(core: Weak<Core>) {
    let mut looks = tokio::time::interval(LOOK_EVERY);
    looks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        looks.tick().await;
        let Some(core) = core.upgrade() else {
            return;
        };
        // A look that failed is made again next time. A message that could not go waits in
        // its session's queue, where the user sees it, and counts as sent for the pause.
        let tried = core.continue_due();
        drop(core);
        if !matches!(tried, Ok(None)) {
            tokio::time::sleep(APART).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::account_usage::{Billed, ExtraPays, UsageWindow};

    fn report(status: &str, resets_at: i64, raw: Value) -> AgentEvent {
        AgentEvent::RateLimit {
            status: status.to_string(),
            resets_at: Some(resets_at),
            raw,
        }
    }

    #[test]
    fn a_turn_waits_for_the_latest_reset_of_the_limits_that_refused_it() {
        let now = 1_000;
        let reports = [
            report("allowed", 9_000, json!({})),
            report("rejected", 2_000, json!({ "rateLimitType": "five_hour" })),
            report("rejected", 5_000, json!({ "rateLimitType": "seven_day" })),
            // Extra usage pays past this one: it refused nothing.
            report("rejected", 7_000, json!({ "isUsingOverage": true })),
        ];
        assert_eq!(reset_of(&reports, now), Some(5_000));
        assert_eq!(reset_of(&reports[..1], now), None);
    }

    #[test]
    fn a_reset_that_has_passed_is_nothing_to_wait_for() {
        let reports = [report("rejected", 2_000, json!({}))];
        assert_eq!(reset_of(&reports, 2_000), None);
        assert_eq!(reset_of(&reports, 3_000), None);
        assert_eq!(reset_of(&reports, 1_999), Some(2_000));
    }

    fn window(kind: &str, used: f64, status: &str, resets_at: i64) -> UsageWindow {
        UsageWindow {
            kind: kind.to_string(),
            name: kind.to_string(),
            used,
            resets_at: Some(resets_at),
            status: status.to_string(),
            heard_at: 0,
            asked: false,
        }
    }

    #[test]
    fn the_account_refuses_until_a_window_used_up_resets() {
        let now = 1_000;
        let mut usage = AccountUsage {
            windows: vec![
                // Reset already: it counts for nothing.
                window("five_hour", 1.0, "rejected", 900),
                window("seven_day", 0.4, "allowed", 50_000),
            ],
            ..AccountUsage::default()
        };
        assert_eq!(refused_until(&usage, now), None);
        // One model's weekly limit stops that model, not the account.
        usage
            .windows
            .push(window("seven_day_opus", 1.0, "rejected", 90_000));
        assert_eq!(refused_until(&usage, now), None);
        // So is the one a report after a turn gives without naming its model.
        usage.windows.push(window(
            "seven_day_overage_included",
            1.0,
            "rejected",
            30_000,
        ));
        assert_eq!(refused_until(&usage, now), None);
        usage.windows[1] = window("seven_day", 1.0, "allowed", 20_000);
        assert_eq!(refused_until(&usage, now), Some(20_000));
        // Extra usage pays past it until it resets: nothing is refused.
        usage.billed = Some(Billed {
            on: true,
            resets_at: Some(20_000),
            heard_at: 0,
        });
        assert_eq!(refused_until(&usage, now), None);
        // And so it does when the newest word on it says it can, before any turn it paid for.
        usage.billed = None;
        let pays = |can| ExtraPays {
            can,
            heard_at: 0,
            asked: false,
        };
        usage.pays = Some(pays(true));
        assert_eq!(refused_until(&usage, now), None);
        usage.pays = Some(pays(false));
        assert_eq!(refused_until(&usage, now), Some(20_000));
    }

    #[test]
    fn an_answers_rounded_hundred_is_no_limit_reached() {
        let now = 1_000;
        // Asked at the second a report after a turn gave it as 99%, `allowed_warning`.
        let mut weekly = window("seven_day", 1.0, "critical", 20_000);
        weekly.asked = true;
        let usage = AccountUsage {
            windows: vec![weekly],
            ..AccountUsage::default()
        };
        assert_eq!(refused_until(&usage, now), None);
    }
}
