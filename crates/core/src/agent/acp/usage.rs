//! The tokens a turn used, read from the `usage` an ACP agent puts on its `session/prompt`
//! response (#528).
//!
//! They count as Claude Code's do (`claude/tokens.rs`): what the model read anew, cache writes
//! with it, and what it wrote, reasoning with it. The protocol's `usage` is unstable, and agents
//! fill it in differently, so two things have to be read per answer:
//!
//! - **Cache reads:** codex-acp, OpenCode, Antigravity (`agy`) and claude-agent-acp leave them
//!   out of `inputTokens`, and Gemini CLI counts them in.
//! - **Thoughts:** codex-acp, agy and claude-agent-acp count them in `outputTokens`, and
//!   OpenCode leaves them out of it but in its total. Gemini CLI sends them apart and outside
//!   its total too, where the total cannot say so, and so its thoughts go uncounted.
//!
//! Swarmed cannot tell agents apart (they are the user's own, in `agents.toml`), so
//! `totalTokens` decides both, as far as it can. A total under the categories added up says the
//! cache reads are inside the input; one that reaches the categories and the thoughts besides
//! says the thoughts are apart, and any other says they are inside the output. Without a total,
//! the categories are read as apart.
//!
//! - **Only the turn's end says them:** no agent sends counts while it works, so nothing is
//!   published while an ACP turn runs. A turn the agent took up by itself has no response, and
//!   so no totals (`end_own_turn`).
//! - **A sub-agent's are its own** where the agent says them on the child's session (codex-acp;
//!   `on_child_update` reads no usage) or leaves them out (OpenCode). An agent that folds them
//!   into its own session's usage (agy from 1.5.15, by its changelog) has them counted there.
//!
//! Tried with codex-acp 2.7.0, OpenCode 2.3.2 and agy 1.5.13, whose `usage` is the turn's sum.

use serde_json::Value;

use crate::agent::Tokens;

/// What a turn used, read from the `usage` of its `session/prompt` response. `None` when there
/// is none, it lacks input or output, or it counts nothing.
pub(super) fn turn_tokens(usage: &Value) -> Option<Tokens> {
    let count = |key: &str| usage.get(key).and_then(Value::as_u64);
    let input = count("inputTokens")?;
    let output = count("outputTokens")?;
    let read = count("cachedReadTokens").unwrap_or(0);
    let written = count("cachedWriteTokens").unwrap_or(0);
    let thoughts = count("thoughtTokens").unwrap_or(0);
    let apart = input
        .saturating_add(read)
        .saturating_add(written)
        .saturating_add(output);
    let total = count("totalTokens");
    let reads_inside = total.is_some_and(|total| total < apart);
    let thoughts_apart =
        thoughts > 0 && total.is_some_and(|total| total >= apart.saturating_add(thoughts));
    let fresh = if reads_inside {
        input.saturating_sub(read)
    } else {
        input
    };
    let tokens = Tokens {
        input: fresh.saturating_add(written),
        output: if thoughts_apart {
            output.saturating_add(thoughts)
        } else {
            output
        },
    };
    (tokens != Tokens::default()).then_some(tokens)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn tokens(input: u64, output: u64) -> Tokens {
        Tokens { input, output }
    }

    #[test]
    fn codex_on_a_cold_cache_says_its_input_and_output() {
        let usage = json!({
            "totalTokens": 24022, "inputTokens": 24017, "cachedReadTokens": 0,
            "cachedWriteTokens": 0, "outputTokens": 5, "thoughtTokens": 0
        });
        assert_eq!(turn_tokens(&usage), Some(tokens(24_017, 5)));
    }

    #[test]
    fn codex_on_a_warm_cache_leaves_out_what_it_read_from_the_cache() {
        // codex-acp takes the cache reads out of its input itself: the total adds them back.
        let usage = json!({
            "totalTokens": 24268, "inputTokens": 199, "cachedReadTokens": 24064,
            "cachedWriteTokens": 0, "outputTokens": 5, "thoughtTokens": 0
        });
        assert_eq!(turn_tokens(&usage), Some(tokens(199, 5)));
    }

    #[test]
    fn reasoning_inside_the_output_is_not_added_twice() {
        // agy, as codex-acp: the thoughts are part of the output, and the total has them once.
        let usage = json!({
            "inputTokens": 2151, "outputTokens": 14, "cachedReadTokens": 7803,
            "cachedWriteTokens": 0, "thoughtTokens": 9, "totalTokens": 9968
        });
        assert_eq!(turn_tokens(&usage), Some(tokens(2151, 14)));
    }

    #[test]
    fn opencode_s_thoughts_are_added_to_what_it_wrote() {
        let usage = json!({
            "inputTokens": 94, "outputTokens": 3, "totalTokens": 12455,
            "thoughtTokens": 70, "cachedReadTokens": 12288
        });
        assert_eq!(turn_tokens(&usage), Some(tokens(94, 73)));
    }

    #[test]
    fn cache_writes_count_as_read_anew() {
        // OpenCode's first turn on a cold cache, had it written some.
        let usage = json!({
            "inputTokens": 8512, "outputTokens": 3, "totalTokens": 13366, "thoughtTokens": 11,
            "cachedReadTokens": 3840, "cachedWriteTokens": 1000
        });
        assert_eq!(turn_tokens(&usage), Some(tokens(9512, 14)));
    }

    #[test]
    fn claude_agent_acp_counts_as_claude_code_does() {
        // The same request as Claude Code's own count in `claude/tokens.rs`.
        let usage = json!({
            "inputTokens": 2, "cachedWriteTokens": 36710, "cachedReadTokens": 12328,
            "outputTokens": 77, "totalTokens": 49117
        });
        assert_eq!(turn_tokens(&usage), Some(tokens(36_712, 77)));
    }

    #[test]
    fn cache_reads_inside_the_input_are_taken_out_of_it() {
        // Gemini CLI: the input holds the cache reads, and the total is input and output. Its
        // thoughts are apart and outside the total, which so cannot tell them from reasoning
        // inside the output: they are lost.
        let usage = json!({
            "inputTokens": 120, "outputTokens": 45, "cachedReadTokens": 80,
            "thoughtTokens": 15, "totalTokens": 165
        });
        assert_eq!(turn_tokens(&usage), Some(tokens(40, 45)));
    }

    #[test]
    fn without_a_total_the_categories_are_apart() {
        let usage = json!({
            "inputTokens": 120, "cachedReadTokens": 9000, "cachedWriteTokens": 30,
            "outputTokens": 40, "thoughtTokens": 20
        });
        assert_eq!(turn_tokens(&usage), Some(tokens(150, 40)));
    }

    #[test]
    fn missing_or_empty_usage_says_none() {
        assert_eq!(turn_tokens(&Value::Null), None);
        assert_eq!(turn_tokens(&json!({})), None);
        assert_eq!(
            turn_tokens(&json!({ "inputTokens": 12, "totalTokens": 12 })),
            None
        );
        assert_eq!(turn_tokens(&json!({ "outputTokens": 3 })), None);
        let nothing = json!({
            "totalTokens": 0, "inputTokens": 0, "cachedReadTokens": 0,
            "cachedWriteTokens": 0, "outputTokens": 0, "thoughtTokens": 0
        });
        assert_eq!(turn_tokens(&nothing), None);
    }

    #[test]
    fn counts_beyond_reason_saturate() {
        let usage = json!({
            "inputTokens": u64::MAX, "cachedWriteTokens": 5, "outputTokens": u64::MAX,
            "thoughtTokens": 5, "totalTokens": u64::MAX
        });
        assert_eq!(turn_tokens(&usage), Some(tokens(u64::MAX, u64::MAX)));
    }
}
