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
//! Nothing asks on its own, on a timer or when a figure is looked at.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{AgentCommand, AgentEvent, Auth, UsageLimit};
use crate::agents::CLAUDE;
use crate::error::{Error, Result};
use crate::sessions::event_from_json;
use crate::store::Store;
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
}

/// One of the plan's usage windows, as the newest report that gave it said.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    /// What tells it apart, in the words of Claude Code's report after a turn: `five_hour`,
    /// `seven_day`, `seven_day_overage_included`, or `seven_day_` and the model for one
    /// model's weekly limit. A limit only an answer lists that is none of these keeps its kind.
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

    /// Puts `windows` in place of those of the same kind and keeps the rest: a report after a
    /// turn leaves a model's own weekly limit out. An answer to asking lists every limit, so
    /// one an earlier answer gave that it does not list is gone.
    fn take(&mut self, windows: Vec<UsageWindow>, asked: bool) {
        if asked {
            let listed = |kind: &str| windows.iter().any(|window| window.kind == kind);
            self.windows
                .retain(|window| !window.asked || listed(&window.kind));
        }
        for window in windows {
            match self.windows.iter_mut().find(|old| old.kind == window.kind) {
                Some(old) => *old = window,
                None => self.windows.push(window),
            }
        }
        // By kind, which puts the 5-hour window first and the weekly ones after it.
        self.windows.sort_by(|a, b| a.kind.cmp(&b.kind));
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

/// Whether a report's own status says a limit is reached, and not only that a window fills up.
fn reached(status: &str) -> bool {
    !matches!(status, "allowed" | "allowed_warning")
}

/// Whether a report says extra usage pays for the turn: the plan's limit is reached, and the
/// work goes on, billed.
fn on_extra_usage(raw: &Value) -> bool {
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
        "seven_day_overage_included" => "weekly, extra usage included".to_string(),
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
fn seconds_now() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
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

    /// Asks the session's Claude Code for the account's usage now, starting it with `auth` if
    /// it is not running. The answer arrives as the session's events,
    /// [`crate::agent::AgentEvent::UsageLimits`] and [`crate::agent::AgentEvent::ExtraUsage`],
    /// and changes [`Core::account_usage`] for every session.
    /// Only a Claude Code session can ask: no other agent reports usage. Must be called from
    /// within a tokio runtime.
    pub fn session_usage_refresh(&self, id: i64, auth: Auth) -> Result<()> {
        let session = self.session(id)?;
        if session.agent != CLAUDE {
            return Err(Error::UsageNotReported(session.agent));
        }
        self.session_start(id, auth)?;
        self.live
            .sender(id)?
            .send(AgentCommand::AskUsage)
            .map_err(|_| Error::SessionNotRunning(id))
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
        assert!(usage.hear(&turn_report(0.88, 2_000_000_000, false), 200));
        assert_eq!(
            shown(&usage),
            vec![
                row("five_hour", "5-hour", "88", false),
                row("seven_day", "weekly", "88", false),
                row("seven_day_fable", "weekly Fable", "100", true),
                row(
                    "seven_day_overage_included",
                    "weekly, extra usage included",
                    "88",
                    false
                ),
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
        // The 5-hour and weekly windows are the answer's; the one only reports list stays.
        assert_eq!(shares, ["36", "36", "100", "50"]);
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
        // The weekly window was heard in the answer before, and is not listed now either; the
        // one only reports list was never asked for.
        assert_eq!(kinds, ["five_hour", "seven_day_overage_included"]);
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
    fn a_windows_name_follows_its_kind() {
        assert_eq!(name_of("seven_day_opus"), "weekly Opus");
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
