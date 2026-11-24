//! What a command an agent ran printed, as some agents send it beside the protocol's tool
//! calls.
//!
//! Codex streams a command's output in the `_meta` of the call's updates, under
//! `terminal_output_delta` (or `terminal_output`, to a client that asks for that), and to a
//! client that is not its own kind of terminal it sends the output nowhere else: the call ends
//! with no content. So the pieces are kept by call until it ends, and are its result then.

use std::collections::HashMap;

use serde_json::Value;

/// The keys of `_meta` a piece of a command's output comes under.
const OUTPUT_KEYS: [&str; 2] = ["terminal_output_delta", "terminal_output"];

/// How much of what one call printed is kept: its latest part, when there is more. A command's
/// result is stored with the conversation.
const KEPT_BYTES: usize = 64 * 1024;

/// What each call that has not ended has printed so far.
#[derive(Debug, Default)]
pub(super) struct Printed {
    by_call: HashMap<String, String>,
}

impl Printed {
    /// Keeps what `update`, about call `call`, says the command printed, if anything.
    pub(super) fn heard(&mut self, call: &str, update: &Value) {
        let Some(meta) = update.get("_meta") else {
            return;
        };
        let piece = OUTPUT_KEYS
            .iter()
            .find_map(|key| meta.get(*key)?.get("data")?.as_str());
        let Some(piece) = piece.filter(|piece| !piece.is_empty()) else {
            return;
        };
        let kept = self.by_call.entry(call.to_string()).or_default();
        kept.push_str(piece);
        keep_latest(kept);
    }

    /// What call `call` printed, once it has ended: nothing if it printed nothing this way.
    pub(super) fn take(&mut self, call: &str) -> Option<String> {
        self.by_call.remove(call)
    }
}

/// Cuts `kept` to its latest [`KEPT_BYTES`], at a character's start.
fn keep_latest(kept: &mut String) {
    let Some(over) = kept.len().checked_sub(KEPT_BYTES) else {
        return;
    };
    let from = (over..=kept.len())
        .find(|at| kept.is_char_boundary(*at))
        .unwrap_or(kept.len());
    kept.drain(..from);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_pieces_codex_streams_in_meta_are_what_the_call_printed() {
        let mut printed = Printed::default();
        let piece = |data: &str| {
            json!({ "sessionUpdate": "tool_call_update", "toolCallId": "call_1",
                    "_meta": { "terminal_output_delta": { "data": data, "terminal_id": "call_1" } } })
        };
        printed.heard("call_1", &piece("total 8\n"));
        printed.heard("call_1", &json!({ "status": "in_progress" }));
        printed.heard("call_1", &piece("README.md\n"));
        assert_eq!(
            printed.take("call_1").as_deref(),
            Some("total 8\nREADME.md\n")
        );
        assert_eq!(printed.take("call_1"), None, "taken once");
        let other = json!({ "_meta": { "terminal_output": { "data": "ok", "terminal_id": "c" } } });
        printed.heard("c", &other);
        assert_eq!(printed.take("c").as_deref(), Some("ok"));
    }

    #[test]
    fn a_long_output_keeps_its_latest_part_whole_characters_only() {
        let mut printed = Printed::default();
        let long = "é".repeat(KEPT_BYTES);
        printed.heard(
            "c",
            &json!({ "_meta": { "terminal_output_delta": { "data": long } } }),
        );
        let kept = printed.take("c").expect("kept");
        assert!(kept.len() <= KEPT_BYTES);
        assert!(kept.chars().all(|c| c == 'é'));
    }
}
