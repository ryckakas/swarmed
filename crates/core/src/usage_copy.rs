//! Claude Code's own copy of the account's usage, and when a question for the usage goes so
//! that its answer is not that copy.
//!
//! Claude Code keeps the figures it last fetched from claude.ai in its config file
//! (`cachedUsageUtilization`, with `fetchedAtMs`), which every Claude Code on the machine
//! shares. For a minute after a fetch, `get_usage` is answered from that copy and claude.ai is
//! not asked, in whichever process asks: as 2.1.332 does it, unless the process has had a
//! turn since. A question asked within that minute therefore gets figures from before it was
//! asked, and pressing the coins again and again changes nothing, even right after the usage
//! was reset. So a question the user asks for goes once the copy is a minute old, and the
//! answer is fetched after they asked. A copy fetched after they asked would do, and one
//! cannot be: it was read when they asked.
//!
//! Only `fetchedAtMs` is read. The rest of the copy, and of the file, is the user's, holds
//! secrets, and is not kept, passed on or logged.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

/// How long Claude Code answers from its copy after fetching it, in milliseconds.
const COPY_KEPT_MS: i64 = 60_000;

/// How long after the copy is a minute old a question goes, so that it surely is by then:
/// Claude Code's clock and this one are read at different moments.
const AFTER_COPY_MS: i64 = 1_000;

/// When a question for the account's usage goes to Claude Code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct UsageAsk {
    /// When it goes, in milliseconds since 1970, if it waits for Claude Code's copy of the
    /// figures to be old enough; `None` when it went at once.
    pub waits_until: Option<i64>,
    /// When the copy it waits out was fetched, in milliseconds since 1970.
    pub copy_fetched_at: Option<i64>,
}

impl UsageAsk {
    /// When a question asked at `now` goes, by when Claude Code fetched its copy
    /// (`fetched_at`, `None` if it has none): at once, unless Claude Code would answer it
    /// from a copy fetched before. A copy said to be fetched later than now is a clock gone
    /// wrong, and Claude Code asks claude.ai again for it too.
    pub(crate) fn at(fetched_at: Option<i64>, now: i64) -> Self {
        let Some(fetched) = fetched_at.filter(|fetched| *fetched <= now) else {
            return Self::default();
        };
        let fresh_from = fetched + COPY_KEPT_MS + AFTER_COPY_MS;
        if fresh_from <= now {
            return Self::default();
        }
        Self {
            waits_until: Some(fresh_from),
            copy_fetched_at: Some(fetched),
        }
    }
}

/// When Claude Code fetched the copy of the usage it answers from, in milliseconds since
/// 1970, as its config file at `claude_config` says; `None` if it says nothing of one.
pub(crate) fn copy_fetched_at(claude_config: &Path) -> Option<i64> {
    let text = std::fs::read_to_string(claude_config).ok()?;
    let config: Value = serde_json::from_str(&text).ok()?;
    config
        .pointer("/cachedUsageUtilization/fetchedAtMs")?
        .as_i64()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const NOW: i64 = 1_795_000_000_000;

    #[test]
    fn a_copy_under_a_minute_old_is_waited_out() {
        let ask = UsageAsk::at(Some(NOW - 17_000), NOW);
        assert_eq!(ask.waits_until, Some(NOW + 44_000));
        assert_eq!(ask.copy_fetched_at, Some(NOW - 17_000));
        // Fetched this very moment: a whole minute to wait.
        assert_eq!(UsageAsk::at(Some(NOW), NOW).waits_until, Some(NOW + 61_000));
    }

    #[test]
    fn a_question_goes_at_once_with_no_copy_to_wait_out() {
        // None at all, or one old enough that Claude Code asks claude.ai again.
        assert_eq!(UsageAsk::at(None, NOW), UsageAsk::default());
        assert_eq!(UsageAsk::at(Some(NOW - 61_000), NOW), UsageAsk::default());
        assert_eq!(
            UsageAsk::at(Some(NOW - 3_600_000), NOW),
            UsageAsk::default()
        );
        // One said to be fetched later than now: Claude Code does not answer from it either.
        assert_eq!(UsageAsk::at(Some(NOW + 5_000), NOW), UsageAsk::default());
    }

    #[test]
    fn only_when_the_copy_was_fetched_is_read_from_claude_codes_config() {
        let dir = tempfile::tempdir().expect("makes a folder");
        let config = dir.path().join("claude.json");
        assert_eq!(copy_fetched_at(&config), None, "no file");

        let file = json!({
            "mcpServers": {},
            "cachedUsageUtilization": {
                "fetchedAtMs": NOW,
                "accountUuid": "account",
                "utilization": { "five_hour": { "utilization": 19.0 } },
            },
        });
        std::fs::write(&config, file.to_string()).expect("writes");
        assert_eq!(copy_fetched_at(&config), Some(NOW));

        std::fs::write(&config, json!({ "mcpServers": {} }).to_string()).expect("writes");
        assert_eq!(copy_fetched_at(&config), None, "no copy");
        std::fs::write(&config, "{ not json").expect("writes");
        assert_eq!(copy_fetched_at(&config), None, "unreadable");
    }
}
