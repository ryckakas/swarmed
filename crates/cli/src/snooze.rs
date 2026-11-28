//! `swarmed session snooze` and `wake`: a session out of sight until a time, read as people
//! write one.
//!
//! The core is handed a time in seconds since 1970 (`Core::session_snooze`); what a person
//! writes is read here, in the local time zone: a duration (`--for 90m`, `2h`, `1d`, `1h30m`,
//! by jiff's friendly format, where a day is the calendar's, so the same time tomorrow even
//! across a change of clocks), or a time (`--until 09:00`, the next such time, today or
//! tomorrow; `2026-12-01 09:00`, local; or RFC 3339 with its offset).

use anyhow::{Result, anyhow};
use clap::Args;
use jiff::civil::{DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, Zoned};
use swarmed_core::{Core, Session};

use crate::limits::span;
use crate::say;

/// Until when `swarmed session snooze` puts a session out of sight: one of the two.
#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub(crate) struct SnoozeWhen {
    /// A time: `HH:MM` for the next such time, `YYYY-MM-DD HH:MM` in the local time zone, or
    /// RFC 3339 such as `2026-12-01T09:00:00+01:00`.
    #[arg(long, value_name = "TIME")]
    until: Option<String>,
    /// How long from now: `90m`, `2h`, `1d`, `1h30m`.
    #[arg(long = "for", value_name = "DURATION")]
    lasting: Option<String>,
}

/// The time `text` names, read as `--until` takes it, with `now` for the day and time zone it
/// leaves out.
fn until_time(text: &str, now: &Zoned) -> Result<Timestamp> {
    let text = text.trim();
    if let Ok(exact) = text.parse::<Timestamp>() {
        return Ok(exact);
    }
    if let Ok(local) = text.parse::<DateTime>() {
        return Ok(local.to_zoned(now.time_zone().clone())?.timestamp());
    }
    let time = text
        .parse::<Time>()
        .map_err(|_| anyhow!("`{text}` is not a time: use HH:MM, YYYY-MM-DD HH:MM or RFC 3339"))?;
    let today = now.with().time(time).build()?;
    if today.timestamp() > now.timestamp() {
        return Ok(today.timestamp());
    }
    let tomorrow = today.date().tomorrow()?.to_datetime(time);
    Ok(tomorrow.to_zoned(now.time_zone().clone())?.timestamp())
}

/// The time `text` from `now` is, read as `--for` takes it.
fn after(text: &str, now: &Zoned) -> Result<Timestamp> {
    let text = text.trim();
    let lasting = text
        .parse::<Span>()
        .map_err(|_| anyhow!("`{text}` is not a duration: use 90m, 2h, 1d or 1h30m"))?;
    Ok(now.checked_add(lasting)?.timestamp())
}

/// Until when `when` asks, from `now`, in seconds since 1970.
fn asked(when: &SnoozeWhen, now: &Zoned) -> Result<i64> {
    let time = match (&when.until, &when.lasting) {
        (Some(until), _) => until_time(until, now)?,
        (None, Some(lasting)) => after(lasting, now)?,
        (None, None) => return Err(anyhow!("say until when: --until or --for")),
    };
    Ok(time.as_second())
}

/// `at`, in seconds since 1970, as the local clock shows it: day, date and time.
fn local(at: i64, zone: TimeZone) -> String {
    Timestamp::from_second(at).map_or_else(
        |_| at.to_string(),
        |at| at.to_zoned(zone).strftime("%a %d %b %H:%M").to_string(),
    )
}

/// What is said of `session` while it is snoozed at `now`, in seconds since 1970: until when,
/// and how long that is from now. Nothing for one awake, or one whose time has come.
pub(crate) fn snoozed_said(session: &Session, now: i64) -> Option<String> {
    let until = session.snoozed_until.filter(|until| *until > now)?;
    Some(format!(
        "snoozed until {} (in {})",
        local(until, TimeZone::system()),
        span(until - now)
    ))
}

/// Snoozes session `id` until `when`, and says until when.
pub(crate) fn snooze(core: &Core, id: i64, when: &SnoozeWhen) -> Result<()> {
    let now = Zoned::now();
    let session = core.session_snooze(id, Some(asked(when, &now)?))?;
    let said = snoozed_said(&session, now.timestamp().as_second());
    say(&said.unwrap_or_else(|| "awake".to_string()));
    Ok(())
}

/// Wakes session `id`.
pub(crate) fn wake(core: &Core, id: i64) -> Result<()> {
    core.session_snooze(id, None)?;
    say("awake");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Now, as a clock in Berlin shows it at `at`.
    fn berlin(at: &str) -> Zoned {
        let zone = TimeZone::get("Europe/Berlin").expect("a known zone");
        at.parse::<DateTime>()
            .expect("a date and time")
            .to_zoned(zone)
            .expect("in the zone")
    }

    fn exact(at: &str) -> Timestamp {
        at.parse().expect("a timestamp")
    }

    fn when(until: Option<&str>, lasting: Option<&str>) -> SnoozeWhen {
        SnoozeWhen {
            until: until.map(str::to_string),
            lasting: lasting.map(str::to_string),
        }
    }

    #[test]
    fn a_time_of_day_is_its_next_coming() {
        let now = berlin("2026-11-28 08:30");
        let later = until_time("09:00", &now).expect("reads");
        assert_eq!(later, exact("2026-11-28T08:00:00Z"));
        let passed = until_time("08:00", &now).expect("reads");
        assert_eq!(passed, exact("2026-11-29T07:00:00Z"));
        // The very minute it is now has come already.
        let now = berlin("2026-11-28 09:00");
        let same = until_time("09:00", &now).expect("reads");
        assert_eq!(same, exact("2026-11-29T08:00:00Z"));
    }

    #[test]
    fn a_date_and_time_is_local_and_an_offset_is_kept() {
        let now = berlin("2026-11-28 08:30");
        let local = until_time("2026-12-01 09:00", &now).expect("reads");
        assert_eq!(local, exact("2026-12-01T08:00:00Z"));
        let iso = until_time("2026-12-01T09:00", &now).expect("reads");
        assert_eq!(iso, local);
        let offset = until_time("2026-12-01T09:00:00-05:00", &now).expect("reads");
        assert_eq!(offset, exact("2026-12-01T14:00:00Z"));
        assert!(until_time("tomorrow", &now).is_err());
        assert!(until_time("25:00", &now).is_err());
    }

    #[test]
    fn a_duration_counts_from_now_and_a_day_keeps_the_clock_across_a_change() {
        let now = berlin("2026-11-28 08:30");
        let from = |text: &str| after(text, &now).expect("reads");
        assert_eq!(from("90m"), exact("2026-11-28T09:00:00Z"));
        assert_eq!(from("2h"), exact("2026-11-28T09:30:00Z"));
        assert_eq!(from("1h30m"), exact("2026-11-28T09:00:00Z"));
        assert_eq!(from("1d"), exact("2026-11-29T07:30:00Z"));
        assert!(after("soon", &now).is_err());
        // The clocks go back an hour in the night to 25 October 2026.
        let before = berlin("2026-10-24 09:00");
        let next_day = after("1d", &before).expect("reads");
        assert_eq!(next_day, exact("2026-10-25T08:00:00Z"));
    }

    #[test]
    fn until_or_for_is_read_into_seconds() {
        let now = berlin("2026-11-28 08:30");
        let at = asked(&when(Some("09:00"), None), &now).expect("reads");
        assert_eq!(at, exact("2026-11-28T08:00:00Z").as_second());
        let at = asked(&when(None, Some("30m")), &now).expect("reads");
        assert_eq!(at, exact("2026-11-28T08:00:00Z").as_second());
        assert!(asked(&when(None, None), &now).is_err());
    }
}
