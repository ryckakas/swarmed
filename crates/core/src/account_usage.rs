//! The account's usage as Claude Code reports it: shared by every session, and asked for by
//! the user.
//!
//! Usage limits belong to the account, not to a session. Claude Code reports them after each
//! turn (`rate_limit`, only some windows) and when asked (`usage_limits`, every limit, with the
//! extra usage). Every Claude Code session runs on the built-in `claude` agent with the user's
//! own sign-in (`agents.toml` cannot redefine it), so all of them report one account, and what
//! any of them heard last is what every session shows: [`AccountUsage`]. Each window keeps the
//! newest figure any report gave for it, since a report after a turn leaves out a model's own
//! weekly limit. It is read from what the sessions recorded when first needed, and published
//! apart, under session id 0, whenever it changes.
//!
//! Only the user asking brings the figures up to date between turns: the session's own Claude
//! Code is asked, and its answer is recorded in the session, with nothing in the conversation.
//! Nothing asks on its own, on a timer or when a figure is looked at: the user's choice, so
//! that looking costs nothing.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{AgentCommand, AgentEvent, Auth, UsageLimit};
use crate::agents::CLAUDE;
use crate::error::{Error, Result};
use crate::sessions::{Live, event_from_json};
use crate::store::Store;
use crate::usage_copy::{UsageAsk, copy_fetched_at};
use crate::{Core, lock};

/// How many of the latest reports after a turn are read at start. Each lists the same few
/// windows, so the latest of them hold the newest figure of each.
const REPORTS_READ: i64 = 20;

/// The account's usage as Swarmed last heard it, from whichever Claude Code session heard it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AccountUsage {
    /// Every usage window heard of, each as the newest report that gave it said, by `kind`.
    pub windows: Vec<UsageWindow>,
    /// The extra usage as Claude Code last gave it when asked, if it ever did.
    pub extra: Option<ExtraUsageHeard>,
    /// What the newest report after a turn that said anything of extra usage said of it, if
    /// there was one. A report of a window still within, while extra usage pays for another
    /// window's limit until it resets, says nothing of it.
    pub billed: Option<Billed>,
    /// Whether extra usage can pay past a limit reached, as the newest word on it said, if
    /// any did. Not whether it does: a limit within needs nothing paid.
    #[serde(default)]
    pub pays: Option<ExtraPays>,
}

/// Whether extra usage can pay past a limit reached.
///
/// It is heard from a report after a turn (`overageStatus`, else `overageEnabled`, or a turn it
/// paid for), or from the extra usage as asked for (turned on, its cap not reached), whichever
/// came last, except that an answer never overrules a report's refusal: its `is_enabled` is the
/// setting, not known to turn off when credits run out (`out_of_credits`), and Claude Code is
/// asked at every start. A later report, or a turn it paid for, does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraPays {
    /// Whether it can.
    pub can: bool,
    /// When Swarmed heard it, in seconds since 1970.
    pub heard_at: i64,
    /// Whether it came from an answer to asking, and not from a report after a turn.
    #[serde(default)]
    pub asked: bool,
}

/// One of the plan's usage windows, as the newest report that gave it said.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    /// What tells it apart, in the words of Claude Code's report after a turn: `five_hour`,
    /// `seven_day`, or `seven_day_` and the model for one model's weekly limit. A report's
    /// `seven_day_overage_included` is one model's too, folded into that model's window once it
    /// is known. A limit only an answer lists that is none of these keeps its kind.
    pub kind: String,
    /// What it is called here, such as `5-hour` or `weekly Fable`.
    pub name: String,
    /// How much of it is used, from 0 to 1.
    pub used: f64,
    /// When it resets, in seconds since 1970, if said.
    pub resets_at: Option<i64>,
    /// `allowed`, or Claude Code's word for how near it is or what stops the next request.
    pub status: String,
    /// When Swarmed heard it, in seconds since 1970.
    pub heard_at: i64,
    /// Whether it came from an answer to asking, which lists every limit, and not from a
    /// report after a turn, which lists only some.
    pub asked: bool,
}

/// What extra usage stood at when Claude Code was last asked: what is spent beyond the plan's
/// limits this billing period, in minor units of `currency`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraUsageHeard {
    /// Whether extra usage can pay for requests now.
    pub enabled: bool,
    /// Spent this period.
    pub used: u64,
    /// The monthly cap; `None` when there is none.
    pub limit: Option<u64>,
    /// ISO 4217, such as `USD` or `EUR`.
    pub currency: String,
    /// How many digits of `used` and `limit` come after the decimal point.
    pub decimals: u32,
    /// Whether the cap is reached, so that extra usage pays for nothing more.
    pub limit_reached: bool,
    /// When Swarmed heard it, in seconds since 1970.
    pub heard_at: i64,
}

/// What the newest report after a turn said of extra usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Billed {
    /// Whether extra usage paid for that turn: a limit was reached and the work went on.
    pub on: bool,
    /// When the window the report was about resets, in seconds since 1970, if said.
    pub resets_at: Option<i64>,
    /// When Swarmed heard the report, in seconds since 1970.
    pub heard_at: i64,
}

impl AccountUsage {
    /// Takes in a report of Claude Code's heard at `at`, and says whether that changed
    /// anything. Any other event changes nothing.
    pub(crate) fn hear(&mut self, event: &AgentEvent, at: i64) -> bool {
        let before = self.clone();
        match event {
            AgentEvent::RateLimit {
                status,
                resets_at,
                raw,
            } => {
                self.hear_billed(status, *resets_at, raw, at);
                let said = extra_can_pay(raw).map(|can| ExtraPays {
                    can,
                    heard_at: at,
                    asked: false,
                });
                self.pays = said.or(self.pays);
                self.take(reported(raw, at), false);
            }
            AgentEvent::UsageLimits { limits } => {
                let windows = limits.iter().map(|limit| asked(limit, at)).collect();
                self.take(windows, true);
            }
            AgentEvent::ExtraUsage {
                enabled,
                used,
                limit,
                currency,
                decimals,
                limit_reached,
            } => {
                self.extra = Some(ExtraUsageHeard {
                    enabled: *enabled,
                    used: *used,
                    limit: *limit,
                    currency: currency.clone(),
                    decimals: *decimals,
                    limit_reached: *limit_reached,
                    heard_at: at,
                });
                self.hear_pays_asked(*enabled && !*limit_reached, at);
            }
            _ => return false,
        }
        *self != before
    }

    /// Takes in what a report heard at `at` says of extra usage, if it says anything of it.
    fn hear_billed(&mut self, status: &str, resets_at: Option<i64>, raw: &Value, at: i64) {
        let on = on_extra_usage(raw);
        if on || reached(status) || !self.billed_until_reset(at) {
            self.billed = Some(Billed {
                on,
                resets_at,
                heard_at: at,
            });
        }
    }

    /// Whether extra usage was heard to pay for the turns until a reset still to come at `at`.
    /// Claude Code sends a report of each window in turn, and one of a window still within
    /// says the turn was not billed while another window's limit is reached and billed: it
    /// says nothing of that limit, which stays billed until it resets.
    fn billed_until_reset(&self, at: i64) -> bool {
        self.billed
            .is_some_and(|billed| billed.on && billed.resets_at.is_some_and(|reset| reset > at))
    }

    /// Takes in what an answer heard at `at` says of whether extra usage can pay: anything but
    /// raising a report's refusal, which only a later report, or a turn it paid for, undoes.
    fn hear_pays_asked(&mut self, can: bool, at: i64) {
        let refused_by_report = self.pays.is_some_and(|pays| !pays.can && !pays.asked);
        if can && refused_by_report {
            return;
        }
        self.pays = Some(ExtraPays {
            can,
            heard_at: at,
            asked: true,
        });
    }

    /// Whether extra usage can pay past a limit reached at `at`: as the newest word on it
    /// said, else as long as a turn it paid for said, until that limit resets.
    pub(crate) fn extra_can_pay(&self, at: i64) -> bool {
        self.pays
            .map_or_else(|| self.billed_until_reset(at), |pays| pays.can)
    }

    /// Puts `windows` in place of those of the same kind and keeps the rest: a report after a
    /// turn leaves a model's own weekly limit out. An answer to asking lists every limit, so
    /// one an earlier answer gave that it does not list is gone.
    fn take(&mut self, windows: Vec<UsageWindow>, asked: bool) {
        if asked {
            let listed = |kind: &str| windows.iter().any(|window| window.kind == kind);
            self.windows
                .retain(|window| !window.asked || listed(&window.kind));
        }
        let came: Vec<String> = windows.iter().map(|window| window.kind.clone()).collect();
        for window in windows {
            self.put(window);
        }
        self.fold_model_weekly(&came);
        // By kind, which puts the 5-hour window first and the weekly ones after it.
        self.windows.sort_by(|a, b| a.kind.cmp(&b.kind));
    }

    /// Puts `window` in place of the one of its kind, if there is one, keeping what a report
    /// said of that limit reached ([`keep_refusal`]).
    fn put(&mut self, mut window: UsageWindow) {
        let Some(old) = self.windows.iter_mut().find(|old| old.kind == window.kind) else {
            self.windows.push(window);
            return;
        };
        keep_refusal(old, &mut window);
        *old = window;
    }

    /// Folds the report's [`MODEL_WEEKLY`] window into the one model's weekly window known,
    /// so that one limit is one window, with the newer figure: the report's if it is what
    /// just `came` and the model's own did not, else the model's. With no model's window
    /// known, or several, there is no telling which it is, and it stays apart.
    fn fold_model_weekly(&mut self, came: &[String]) {
        let is_model =
            |window: &UsageWindow| one_models(&window.kind) && window.kind != MODEL_WEEKLY;
        let models = self.windows.iter().filter(|window| is_model(window));
        if models.count() != 1 {
            return;
        }
        let Some(at) = self.windows.iter().position(|w| w.kind == MODEL_WEEKLY) else {
            return;
        };
        let reported = self.windows.remove(at);
        let Some(model) = self.windows.iter_mut().find(|window| is_model(window)) else {
            return;
        };
        let has_come = |kind: &str| came.iter().any(|one| one == kind);
        let newer = match (has_come(MODEL_WEEKLY), has_come(&model.kind)) {
            (true, false) => true,
            (false, false) => reported.heard_at > model.heard_at,
            _ => false,
        };
        if newer {
            *model = UsageWindow {
                kind: model.kind.clone(),
                name: model.name.clone(),
                ..reported
            };
        }
    }

    /// The usage as the sessions' records leave it: the newest answer to asking, the newest
    /// extra usage and the latest reports after a turn, taken in as they came.
    fn read(store: &Store) -> Result<Self> {
        let mut usage = Self::default();
        for (event, at) in store.usage_reports()? {
            usage.hear(&event, at);
        }
        Ok(usage)
    }
}

/// What Claude Code's report after a turn calls one model's own weekly limit, without naming
/// the model. Claude Code takes it for the limit of the plan's one model with a weekly limit of
/// its own, and shows it under that model's name when it cannot ask claude.ai; its figures
/// follow that model's as an answer to asking gives them (`weekly_scoped`), to the percent.
/// Despite its name, it is not extra usage, which is money against a monthly cap.
pub(crate) const MODEL_WEEKLY: &str = "seven_day_overage_included";

/// Whether a usage window of this kind is one model's weekly limit, and not the account's: it
/// stops requests to that model alone.
pub(crate) fn one_models(kind: &str) -> bool {
    kind.starts_with("seven_day_")
}

/// Whether a window's limit is reached, so that requests to it are refused unless extra usage
/// pays: a report said so, or a report after a turn gave it at 100% or more. An answer to
/// asking gives a whole percent, and its 100 may be 99.5: a report after a turn said 99%,
/// `allowed_warning`, of the weekly limit at the very second an answer gave it as 100%.
pub(crate) fn window_reached(window: &UsageWindow) -> bool {
    window.status == "rejected" || (!window.asked && window.used >= 1.0)
}

/// Keeps what a report said of a limit reached when an answer to asking gives the same window
/// at 100%, which may be rounded: the answer has no word for a limit reached, and the window
/// stays reached until it resets.
fn keep_refusal(old: &UsageWindow, new: &mut UsageWindow) {
    let same_reset = match (old.resets_at, new.resets_at) {
        (Some(old), Some(new)) => (old - new).abs() <= 60,
        _ => false,
    };
    if new.asked && new.used >= 1.0 && old.status == "rejected" && same_reset {
        new.status.clone_from(&old.status);
    }
}

/// Whether a report after a turn says extra usage can pay past a limit reached, if it says:
/// Claude Code's `overageStatus`, which is `rejected` when it cannot (`out_of_credits`, say),
/// else `overageEnabled`, else a turn it paid for. Older reports have no `overageEnabled`.
fn extra_can_pay(raw: &Value) -> Option<bool> {
    if on_extra_usage(raw) {
        return Some(true);
    }
    if let Some(status) = raw.get("overageStatus").and_then(Value::as_str) {
        return Some(status != "rejected");
    }
    raw.get("overageEnabled").and_then(Value::as_bool)
}

/// Whether a report's own status says a limit is reached, and not only that a window fills up.
pub(crate) fn reached(status: &str) -> bool {
    !matches!(status, "allowed" | "allowed_warning")
}

/// Whether a report says extra usage pays for the turn: the plan's limit is reached, and the
/// work goes on, billed.
pub(crate) fn on_extra_usage(raw: &Value) -> bool {
    let set = |key: &str| raw.get(key).and_then(Value::as_bool) == Some(true);
    set("isUsingOverage") || set("overageInUse")
}

/// The windows a report after a turn lists, those in a known shape.
fn reported(raw: &Value, at: i64) -> Vec<UsageWindow> {
    let Some(windows) = raw.get("unifiedWindows").and_then(Value::as_object) else {
        return vec![];
    };
    let window = |(kind, window): (&String, &Value)| {
        Some(UsageWindow {
            name: name_of(kind),
            kind: kind.clone(),
            used: window.get("utilization")?.as_f64()?,
            resets_at: window.get("resetsAt").and_then(Value::as_i64),
            status: window
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("allowed")
                .to_string(),
            heard_at: at,
            asked: false,
        })
    };
    windows.iter().filter_map(window).collect()
}

/// A limit from an answer to asking, as a window of the same kind as a report after a turn
/// gives it: the session limit is the 5-hour window, and so on.
fn asked(limit: &UsageLimit, at: i64) -> UsageWindow {
    let (kind, name) = match (limit.kind.as_str(), limit.model.as_deref()) {
        ("session", _) => ("five_hour".to_string(), None),
        ("weekly_all", _) => ("seven_day".to_string(), None),
        ("weekly_scoped", Some(model)) => {
            let kind = format!("seven_day_{}", slug(model));
            (kind, Some(format!("weekly {model}")))
        }
        (kind, _) => (kind.to_string(), None),
    };
    let status = if limit.severity == "normal" {
        "allowed"
    } else {
        limit.severity.as_str()
    };
    UsageWindow {
        name: name.unwrap_or_else(|| name_of(&kind)),
        kind,
        used: f64::from(limit.percent) / 100.0,
        resets_at: limit.resets_at.as_deref().and_then(iso_seconds),
        status: status.to_string(),
        heard_at: at,
        asked: true,
    }
}

/// What a window is called here, from its kind.
fn name_of(kind: &str) -> String {
    match kind {
        "five_hour" => "5-hour".to_string(),
        "seven_day" => "weekly".to_string(),
        MODEL_WEEKLY => "weekly model".to_string(),
        _ => match kind.strip_prefix("seven_day_") {
            Some(model) => format!("weekly {}", capitalised(&model.replace('_', " "))),
            None => kind.replace('_', " "),
        },
    }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A model's display name as part of a window's kind: `Opus 4` is `opus_4`.
fn slug(model: &str) -> String {
    let word = |c: char| {
        if c.is_alphanumeric() {
            c.to_ascii_lowercase()
        } else {
            '_'
        }
    };
    model.chars().map(word).collect()
}

/// An ISO 8601 time, as Claude Code gives a limit's reset, in seconds since 1970: a date, a
/// time with or without fractions of a second, and `Z` or an offset. `None` in another form.
fn iso_seconds(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once('T')?;
    let mut parts = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, day) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    let zone = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (time, offset) = rest.split_at(zone);
    let mut parts = time.splitn(3, ':');
    let hour: i64 = parts.next()?.parse().ok()?;
    let minute: i64 = parts.next()?.parse().ok()?;
    let second: i64 = parts.next().map_or(Some(0), |second| {
        second
            .split('.')
            .next()
            .and_then(|whole| whole.parse().ok())
    })?;
    let valid = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && hour < 24
        && minute < 60
        && second < 61;
    let clock = hour * 3600 + minute * 60 + second;
    valid.then(|| days_from_civil(year, month, day) * 86_400 + clock - offset_seconds(offset))
}

/// How far ahead of UTC an offset such as `+02:00` is, in seconds; `Z` and none are UTC.
fn offset_seconds(offset: &str) -> i64 {
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let digits: String = offset.chars().filter(char::is_ascii_digit).collect();
    let hours = digits.get(..2).and_then(|hours| hours.parse::<i64>().ok());
    let minutes = digits
        .get(2..4)
        .and_then(|minutes| minutes.parse::<i64>().ok());
    sign * (hours.unwrap_or(0) * 3600 + minutes.unwrap_or(0) * 60)
}

/// The days from 1970-01-01 to a date of the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // Counted from March, so that the leap day ends the year.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Now, in seconds since 1970.
pub(crate) fn seconds_now() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    })
}

/// Now, in milliseconds since 1970, as Claude Code keeps time in its config file.
fn millis_now() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |since| {
        i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
    })
}

/// Whether an event is one of Claude Code's reports of the account's usage.
fn is_report(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::RateLimit { .. }
            | AgentEvent::UsageLimits { .. }
            | AgentEvent::ExtraUsage { .. }
    )
}

/// A recorded report: its row's id, the event as stored, and when it was recorded.
fn report_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, String, i64)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

impl Store {
    /// Claude Code's reports of the account's usage that [`AccountUsage::read`] takes in, as
    /// they came, each with when it was heard: the newest answer to asking, the newest extra
    /// usage, and the latest reports after a turn.
    fn usage_reports(&self) -> Result<Vec<(AgentEvent, i64)>> {
        // The event's type is the first thing in its JSON, which keeps this a prefix match.
        let mut statement = self.conn.prepare(
            "SELECT e.id, e.event, CAST(strftime('%s', e.created_at) AS INTEGER)
             FROM session_events e
             JOIN sessions s ON s.id = e.session_id
             WHERE s.agent = ?1 AND e.event LIKE ?2
             ORDER BY e.id DESC
             LIMIT ?3",
        )?;
        let mut found = Vec::new();
        let wanted = [
            ("usage_limits", 1),
            ("extra_usage", 1),
            ("rate_limit", REPORTS_READ),
        ];
        for (kind, newest) in wanted {
            let prefix = format!("{{\"type\":\"{kind}\"%");
            let rows = statement.query_map(params![CLAUDE, prefix, newest], report_row)?;
            found.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        found.sort_unstable_by_key(|(id, ..)| *id);
        let reports = found.into_iter();
        Ok(reports
            .map(|(_, event, at)| (event_from_json(&event), at))
            .collect())
    }
}

/// The account's usage as last heard, which every session shares: `None` until it is first
/// needed, when it is read from what the sessions recorded.
#[derive(Debug, Default)]
pub(crate) struct UsageHeard(Mutex<Option<AccountUsage>>);

impl UsageHeard {
    /// The usage as last heard.
    pub(crate) fn now(&self, store: &Mutex<Store>) -> Result<AccountUsage> {
        let mut heard = lock(&self.0);
        if let Some(usage) = heard.as_ref() {
            return Ok(usage.clone());
        }
        let usage = AccountUsage::read(&lock(store))?;
        *heard = Some(usage.clone());
        Ok(usage)
    }

    /// Takes in an event session `session_id` recorded, and returns the usage now if the
    /// event changed it. Only Claude Code reports usage: the Agent Client Protocol has no
    /// message for an account's limits, and another agent's would be another account's.
    /// Reports are told apart by agent, not by sign-in: a session the command line runs with
    /// `--auth inherit` on another account's sign-in counts as the same account.
    pub(crate) fn hear(
        &self,
        store: &Mutex<Store>,
        session_id: i64,
        event: &AgentEvent,
    ) -> Option<AccountUsage> {
        if !is_report(event) {
            return None;
        }
        let agent = lock(store).session_by_id(session_id).ok()?.agent;
        if agent != CLAUDE {
            return None;
        }
        let mut heard = lock(&self.0);
        if let Some(usage) = heard.as_mut() {
            return usage.hear(event, seconds_now()).then(|| usage.clone());
        }
        // Read for the first time: what was recorded holds this report already.
        let usage = AccountUsage::read(&lock(store)).ok()?;
        *heard = Some(usage.clone());
        Some(usage)
    }
}

impl Core {
    /// The account's usage as any Claude Code session last heard it: each window's newest
    /// figure, the extra usage, and whether the latest turn ran on it. Nothing is asked of
    /// Claude Code for it; [`Core::session_usage_refresh`] does that, when the user asks.
    pub fn account_usage(&self) -> Result<AccountUsage> {
        self.live.usage.now(&self.store)
    }

    /// Asks the session's Claude Code for the account's usage as it is now, starting it with
    /// `auth` if it is not running, and says when the question goes. The answer arrives as
    /// the session's events, [`crate::agent::AgentEvent::UsageLimits`] and
    /// [`crate::agent::AgentEvent::ExtraUsage`], and changes [`Core::account_usage`] for every
    /// session.
    ///
    /// Claude Code answers from its own copy of the figures for a minute after it fetched them
    /// (`usage_copy`), so while that copy is younger, the question waits until it is not, and
    /// the answer is fetched after this call. A question that waits is sent by the session's
    /// agent of that moment, and by none if it no longer runs.
    ///
    /// Only a Claude Code session can ask: no other agent reports usage. Must be called from
    /// within a tokio runtime.
    pub fn session_usage_refresh(&self, id: i64, auth: Auth) -> Result<UsageAsk> {
        let session = self.session(id)?;
        if session.agent != CLAUDE {
            return Err(Error::UsageNotReported(session.agent));
        }
        self.session_start(id, auth)?;
        let now = millis_now();
        let ask = UsageAsk::at(copy_fetched_at(&self.paths.claude_config), now);
        let Some(until) = ask.waits_until else {
            self.live
                .sender(id)?
                .send(AgentCommand::AskUsage)
                .map_err(|_| Error::SessionNotRunning(id))?;
            return Ok(ask);
        };
        let wait = Duration::from_millis(u64::try_from(until - now).unwrap_or(0));
        tokio::spawn(ask_after(Arc::clone(&self.live), id, wait));
        Ok(ask)
    }
}

/// Asks session `id`'s agent for the account's usage after `wait`, the agent that runs it
/// then. Its run is looked up when the question goes, not held meanwhile: a sender held would
/// keep the agent's input open after it was stopped. An agent that ended meanwhile is asked
/// nothing.
async fn ask_after(live: Arc<Live>, id: i64, wait: Duration) {
    tokio::time::sleep(wait).await;
    if let Ok(sender) = live.sender(id) {
        sender.send(AgentCommand::AskUsage).ok();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A report after a turn, with every window at `used` and resetting at `resets`.
    fn turn_report(used: f64, resets: i64, billed: bool) -> AgentEvent {
        let window = json!({ "status": "allowed", "utilization": used, "resetsAt": resets });
        AgentEvent::RateLimit {
            status: "allowed".to_string(),
            resets_at: Some(resets),
            raw: json!({
                "isUsingOverage": billed,
                "unifiedWindows": {
                    "five_hour": window,
                    "seven_day": window,
                    "seven_day_overage_included": window,
                },
            }),
        }
    }

    /// A report after a turn of only the account's windows, with no word of a model's own.
    fn plan_report(used: f64, resets: i64) -> AgentEvent {
        let window = json!({ "status": "allowed", "utilization": used, "resetsAt": resets });
        AgentEvent::RateLimit {
            status: "allowed".to_string(),
            resets_at: Some(resets),
            raw: json!({ "unifiedWindows": { "five_hour": window, "seven_day": window } }),
        }
    }

    /// A report after a turn of one model's weekly limit, without naming the model, as Claude
    /// Code gives it, at `used`.
    fn model_report(used: f64) -> AgentEvent {
        let window = json!({ "status": "allowed", "utilization": used, "resetsAt": 1_796_061_600 });
        AgentEvent::RateLimit {
            status: "allowed".to_string(),
            resets_at: Some(1_796_061_600),
            raw: json!({ "unifiedWindows": { MODEL_WEEKLY: window } }),
        }
    }

    fn limit(kind: &str, model: Option<&str>, percent: u32, severity: &str) -> UsageLimit {
        UsageLimit {
            kind: kind.to_string(),
            model: model.map(str::to_string),
            percent,
            resets_at: Some("2026-11-30T18:00:00.123456+00:00".to_string()),
            severity: severity.to_string(),
        }
    }

    /// An answer to asking: the 5-hour and weekly limits at `percent`, and Fable's at 100%.
    fn answer(percent: u32) -> AgentEvent {
        AgentEvent::UsageLimits {
            limits: vec![
                limit("session", None, percent, "normal"),
                limit("weekly_all", None, percent, "normal"),
                limit("weekly_scoped", Some("Fable"), 100, "critical"),
            ],
        }
    }

    /// Each window's kind, name, share in percent and whether it was asked for.
    fn shown(usage: &AccountUsage) -> Vec<(String, String, String, bool)> {
        let row = |window: &UsageWindow| {
            let share = format!("{:.0}", window.used * 100.0);
            (
                window.kind.clone(),
                window.name.clone(),
                share,
                window.asked,
            )
        };
        usage.windows.iter().map(row).collect()
    }

    fn row(kind: &str, name: &str, share: &str, asked: bool) -> (String, String, String, bool) {
        (kind.to_string(), name.to_string(), share.to_string(), asked)
    }

    #[test]
    fn a_report_after_a_turn_keeps_a_models_limit_heard_when_asked() {
        let mut usage = AccountUsage::default();
        assert!(usage.hear(&answer(80), 100));
        // The report after the next turn has no word of Fable: its limit stays as it was.
        assert!(usage.hear(&plan_report(0.88, 2_000_000_000), 200));
        assert_eq!(
            shown(&usage),
            vec![
                row("five_hour", "5-hour", "88", false),
                row("seven_day", "weekly", "88", false),
                row("seven_day_fable", "weekly Fable", "100", true),
            ]
        );
        let fable = &usage.windows[2];
        assert_eq!((fable.status.as_str(), fable.heard_at), ("critical", 100));
        assert_eq!(fable.resets_at, Some(1_796_061_600));
        assert_eq!(usage.windows[0].heard_at, 200);
    }

    #[test]
    fn an_answer_takes_the_place_of_older_figures_of_the_same_windows() {
        let mut usage = AccountUsage::default();
        usage.hear(&turn_report(0.5, 2_000_000_000, false), 100);
        usage.hear(&answer(36), 200);
        let shares: Vec<_> = shown(&usage).into_iter().map(|row| row.2).collect();
        // The 5-hour and weekly windows are the answer's, and so is Fable's, which the report
        // gave without naming it.
        assert_eq!(shares, ["36", "36", "100"]);
        assert_eq!(usage.windows[0].status, "allowed");
    }

    #[test]
    fn a_later_answer_without_a_models_limit_drops_it() {
        let mut usage = AccountUsage::default();
        usage.hear(&turn_report(0.5, 2_000_000_000, false), 100);
        usage.hear(&answer(36), 200);
        let without_fable = AgentEvent::UsageLimits {
            limits: vec![limit("session", None, 40, "normal")],
        };
        usage.hear(&without_fable, 300);
        let kinds: Vec<_> = shown(&usage).into_iter().map(|row| row.0).collect();
        // The weekly and Fable's windows were heard in the answer before, and are not listed
        // now either.
        assert_eq!(kinds, ["five_hour"]);
    }

    #[test]
    fn a_reports_unnamed_model_limit_is_the_one_models_limit_known() {
        // As Claude Code gave them: Fable's weekly limit at 39% when asked, then the report
        // after a turn of a model's own limit at 39%, then 40%. One limit, one window.
        let mut usage = AccountUsage::default();
        let fable = |percent| AgentEvent::UsageLimits {
            limits: vec![limit("weekly_scoped", Some("Fable"), percent, "normal")],
        };
        usage.hear(&fable(39), 100);
        usage.hear(&model_report(0.39), 200);
        assert_eq!(
            shown(&usage),
            [row("seven_day_fable", "weekly Fable", "39", false)]
        );
        assert_eq!(usage.windows[0].heard_at, 200);
        usage.hear(&model_report(0.40), 300);
        assert_eq!(
            shown(&usage),
            [row("seven_day_fable", "weekly Fable", "40", false)]
        );
        // An answer is newer again.
        usage.hear(&fable(41), 400);
        assert_eq!(
            shown(&usage),
            [row("seven_day_fable", "weekly Fable", "41", true)]
        );
    }

    #[test]
    fn a_reports_unnamed_model_limit_stays_apart_until_its_model_is_known() {
        let mut usage = AccountUsage::default();
        usage.hear(&model_report(0.39), 100);
        assert_eq!(
            shown(&usage),
            [row(MODEL_WEEKLY, "weekly model", "39", false)]
        );
        // The answer that names the model is newer, and takes its place.
        let both = AgentEvent::UsageLimits {
            limits: vec![
                limit("weekly_scoped", Some("Fable"), 40, "normal"),
                limit("weekly_scoped", Some("Opus"), 10, "normal"),
            ],
        };
        let fable = AgentEvent::UsageLimits {
            limits: vec![limit("weekly_scoped", Some("Fable"), 40, "normal")],
        };
        usage.hear(&fable, 200);
        assert_eq!(
            shown(&usage),
            [row("seven_day_fable", "weekly Fable", "40", true)]
        );
        // With two models' limits there is no telling which it is.
        usage.hear(&both, 300);
        usage.hear(&model_report(0.45), 400);
        let kinds: Vec<_> = shown(&usage).into_iter().map(|row| row.0).collect();
        assert_eq!(kinds, ["seven_day_fable", "seven_day_opus", MODEL_WEEKLY]);
    }

    #[test]
    fn extra_usage_and_billing_are_the_newest_heard() {
        let mut usage = AccountUsage::default();
        let extra = |used| AgentEvent::ExtraUsage {
            enabled: true,
            used,
            limit: Some(40_000),
            currency: "USD".to_string(),
            decimals: 2,
            limit_reached: false,
        };
        assert!(usage.hear(&extra(3871), 100));
        assert!(usage.hear(&extra(4126), 200));
        let heard = usage
            .extra
            .as_ref()
            .map(|extra| (extra.used, extra.heard_at));
        assert_eq!(heard, Some((4126, 200)));
        usage.hear(&turn_report(1.0, 2_000_000_000, true), 300);
        let billed = Billed {
            on: true,
            resets_at: Some(2_000_000_000),
            heard_at: 300,
        };
        assert_eq!(usage.billed, Some(billed));
        // What is no report changes nothing, and the same report heard again neither.
        assert!(!usage.hear(&AgentEvent::Closed, 400));
        assert!(!usage.hear(&turn_report(1.0, 2_000_000_000, true), 300));
    }

    #[test]
    fn a_report_of_a_window_within_leaves_a_limit_billed_until_it_resets() {
        let resets = 2_000_000_000;
        let report = |status: &str, kind: &str, billed: bool| AgentEvent::RateLimit {
            status: status.to_string(),
            resets_at: Some(resets),
            raw: json!({
                "isUsingOverage": billed,
                "rateLimitType": kind,
                "unifiedWindows": {
                    kind: { "status": status, "utilization": 0.99, "resetsAt": resets },
                },
            }),
        };
        let billed = report("rejected", "seven_day_overage_included", true);
        let warning = report("allowed_warning", "seven_day", false);
        let mut usage = AccountUsage::default();
        // As Claude Code sends them in a turn: one window, then the other, and again.
        for (at, event) in [
            (100, &billed),
            (110, &warning),
            (200, &billed),
            (210, &warning),
        ] {
            usage.hear(event, at);
            assert!(usage.billed.is_some_and(|billed| billed.on), "at {at}");
        }
        // Once the limit has reset, a turn that was not billed says it is not.
        usage.hear(&warning, resets + 10);
        assert!(usage.billed.is_some_and(|billed| !billed.on));
        // A limit reached and nothing paying says so at once.
        usage.hear(&billed, resets + 20);
        usage.hear(&report("rejected", "five_hour", false), resets + 30);
        assert!(usage.billed.is_some_and(|billed| !billed.on));
    }

    #[test]
    fn whether_extra_usage_can_pay_is_the_newest_word_on_it() {
        let report = |fields: Value| AgentEvent::RateLimit {
            status: "allowed_warning".to_string(),
            resets_at: Some(1_795_813_200),
            raw: fields,
        };
        let mut usage = AccountUsage::default();
        // Nothing said yet, and no turn it paid for.
        assert!(!usage.extra_can_pay(100));
        // As Claude Code reported on 25 Nov, at 99% of the weekly limit: not paying yet, but able.
        usage.hear(
            &report(json!({ "isUsingOverage": false, "overageEnabled": true, "overageStatus": "allowed" })),
            100,
        );
        assert!(usage.extra_can_pay(100));
        // As on 24 Nov, out of credits.
        let out = json!({ "overageStatus": "rejected", "overageDisabledReason": "out_of_credits" });
        usage.hear(&report(out), 200);
        assert!(!usage.extra_can_pay(200));
        // An answer to asking, turned on and under its cap, does not overrule that: its
        // `is_enabled` is the setting, and Claude Code is asked at every start.
        let extra = |enabled, limit_reached| AgentEvent::ExtraUsage {
            enabled,
            used: 15_314,
            limit: Some(40_000),
            currency: "EUR".to_string(),
            decimals: 2,
            limit_reached,
        };
        usage.hear(&extra(true, false), 300);
        assert!(!usage.extra_can_pay(300));
        // A later report does, as when credits were added.
        usage.hear(&report(json!({ "overageStatus": "allowed" })), 350);
        assert!(usage.extra_can_pay(350));
        // An answer lowers it, its cap reached, and raises what an answer lowered.
        usage.hear(&extra(true, true), 400);
        assert!(!usage.extra_can_pay(400));
        usage.hear(&extra(true, false), 450);
        assert!(usage.extra_can_pay(450));
        // A report that says nothing of it changes nothing; older ones had no `overageEnabled`.
        usage.hear(&extra(false, false), 480);
        usage.hear(&plan_report(0.5, 1_795_813_200), 500);
        assert!(!usage.extra_can_pay(500));
        usage.hear(&report(json!({ "overageStatus": "allowed" })), 600);
        assert!(usage.extra_can_pay(600));
    }

    #[test]
    fn an_answers_hundred_is_a_limit_reached_only_if_a_report_said_so() {
        let resets = 1_795_813_200;
        let weekly = |status: &str, used: f64| AgentEvent::RateLimit {
            status: status.to_string(),
            resets_at: Some(resets),
            raw: json!({ "unifiedWindows": {
                "seven_day": { "status": status, "utilization": used, "resetsAt": resets },
            } }),
        };
        let asked_at_hundred = AgentEvent::UsageLimits {
            limits: vec![UsageLimit {
                kind: "weekly_all".to_string(),
                model: None,
                percent: 100,
                resets_at: Some("2026-11-27T21:00:00.260758+00:00".to_string()),
                severity: "critical".to_string(),
            }],
        };
        // On 25 Nov: 99% and `allowed_warning` after a turn, 100 when asked that second.
        let mut usage = AccountUsage::default();
        usage.hear(&weekly("allowed_warning", 0.99), 100);
        usage.hear(&asked_at_hundred, 100);
        assert!(!window_reached(&usage.windows[0]), "only rounded up");
        // Reached and said so after a turn: an answer at 100 keeps it reached.
        usage.hear(&weekly("rejected", 1.02), 200);
        assert!(window_reached(&usage.windows[0]));
        usage.hear(&asked_at_hundred, 300);
        assert_eq!(usage.windows[0].status, "rejected");
        assert!(window_reached(&usage.windows[0]));
    }

    #[test]
    fn a_windows_name_follows_its_kind() {
        assert_eq!(name_of("seven_day_opus"), "weekly Opus");
        assert_eq!(name_of(MODEL_WEEKLY), "weekly model");
        assert_eq!(name_of("weekly_scoped"), "weekly scoped");
        let opus = asked(&limit("weekly_scoped", Some("Opus 4"), 10, "normal"), 0);
        assert_eq!(
            (opus.kind.as_str(), opus.name.as_str()),
            ("seven_day_opus_4", "weekly Opus 4")
        );
    }

    #[test]
    fn reset_times_are_read_as_claude_code_writes_them() {
        assert_eq!(iso_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            iso_seconds("2026-11-23T20:00:00.654321+00:00"),
            Some(1_795_464_000)
        );
        assert_eq!(
            iso_seconds("2026-11-23T22:00:00+02:00"),
            Some(1_795_464_000)
        );
        assert_eq!(
            iso_seconds("2026-11-23T15:30:00-04:30"),
            Some(1_795_464_000)
        );
        assert_eq!(iso_seconds("2024-02-29T00:00:00"), Some(1_709_164_800));
        assert_eq!(iso_seconds("tomorrow"), None);
        assert_eq!(iso_seconds("2026-13-01T00:00:00Z"), None);
    }
}
