//! The `limits` command: the plan's usage limits and the extra usage, as any Claude Code
//! session last heard them. Nothing is asked of Claude Code: `/usage` in a session asks.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use swarmed_core::{Core, ExtraUsageHeard, UsageWindow};

use crate::say;

pub(crate) fn run(core: &Core) -> Result<()> {
    let usage = core.account_usage()?;
    let now = seconds_now();
    if usage.windows.is_empty() && usage.extra.is_none() {
        say("no usage heard yet: Claude Code reports it after a turn, and `/usage` asks for it");
        return Ok(());
    }
    for window in &usage.windows {
        say(&window_line(window, now));
    }
    if let Some(extra) = &usage.extra {
        say(&extra_line(extra, now));
    }
    if let Some(billed) = usage.billed.filter(|billed| billed.on) {
        say(&format!(
            "billed\tthe latest turn ran on extra usage\theard {}",
            ago(billed.heard_at, now)
        ));
    }
    Ok(())
}

/// A window: how full it is, when it resets and how old the figure is. One past its reset
/// counts as reset until a report says otherwise, as in the window.
fn window_line(window: &UsageWindow, now: i64) -> String {
    let reset = window.resets_at.is_some_and(|at| at <= now);
    let used = if reset { 0.0 } else { window.used };
    let full = !reset && (window.used >= 1.0 || window.status == "rejected");
    let mut parts = vec![window.name.clone(), format!("{:.0}%", used * 100.0)];
    if full {
        parts.push("used up".to_string());
    }
    if let Some(at) = window.resets_at {
        parts.push(if reset {
            format!("reset {}", ago(at, now))
        } else {
            format!("resets in {}", span(at - now))
        });
    }
    parts.push(format!("heard {}", ago(window.heard_at, now)));
    parts.join("\t")
}

fn extra_line(extra: &ExtraUsageHeard, now: i64) -> String {
    let spent = money(extra.used, &extra.currency, extra.decimals);
    let cap = extra.limit.map_or_else(
        || "with no cap".to_string(),
        |limit| format!("of {}", money(limit, &extra.currency, extra.decimals)),
    );
    let state = if extra.limit_reached {
        "\tcap reached"
    } else if extra.enabled {
        ""
    } else {
        "\toff"
    };
    format!(
        "extra usage\t{spent} {cap} this month{state}\theard {}",
        ago(extra.heard_at, now)
    )
}

/// An amount in minor units, in its currency: 3871 cents of USD is `38.71 USD`.
fn money(minor: u64, currency: &str, decimals: u32) -> String {
    let Some(unit) = 10_u64.checked_pow(decimals).filter(|_| decimals > 0) else {
        return format!("{minor} {currency}");
    };
    let width = usize::try_from(decimals).unwrap_or(2);
    format!("{}.{:0width$} {currency}", minor / unit, minor % unit)
}

/// How long ago `at` was: `just now`, or `4m ago`, `2h 13m ago`, `5d 3h ago`.
fn ago(at: i64, now: i64) -> String {
    if now - at < 60 {
        "just now".to_string()
    } else {
        format!("{} ago", span(now - at))
    }
}

/// A span of seconds in its two largest units, to the minute.
pub(crate) fn span(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

pub(crate) fn seconds_now() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(used: f64, resets_at: Option<i64>) -> UsageWindow {
        UsageWindow {
            kind: "seven_day_fable".to_string(),
            name: "weekly Fable".to_string(),
            used,
            resets_at,
            status: "allowed".to_string(),
            heard_at: 1000,
            asked: true,
        }
    }

    #[test]
    fn a_window_says_how_full_it_is_when_it_resets_and_how_old_that_is() {
        let now = 1000 + 9 * 60;
        let full = window(1.0, Some(now + 5 * 86_400 + 3 * 3600));
        assert_eq!(
            window_line(&full, now),
            "weekly Fable\t100%\tused up\tresets in 5d 3h\theard 9m ago"
        );
        let past = window(1.0, Some(now - 2 * 3600));
        assert_eq!(
            window_line(&past, now),
            "weekly Fable\t0%\treset 2h 0m ago\theard 9m ago"
        );
        assert_eq!(
            window_line(&window(0.36, None), 1010),
            "weekly Fable\t36%\theard just now"
        );
    }

    #[test]
    fn money_is_shown_in_its_currency() {
        assert_eq!(money(3871, "USD", 2), "38.71 USD");
        assert_eq!(money(40_005, "EUR", 2), "400.05 EUR");
        assert_eq!(money(1200, "JPY", 0), "1200 JPY");
    }
}
