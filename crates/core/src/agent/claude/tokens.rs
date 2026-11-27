//! The tokens a running turn uses, counted from what Claude Code streams of each request (#419).
//!
//! A request reports what it was sent when its answer starts (`message_start`) and what it
//! wrote when the answer is done (`message_delta`). [`Counts`] adds them up per turn from each
//! line `read_events` reads and publishes [`AgentEvent::TurnTokens`] with a [`TokenPhase`]:
//! `prompt` from a message said back or the last tool result in, `response` from
//! `message_start`, `between` from `message_stop`. A count is only shown: never stored
//! (`shown_only` in `sessions.rs`), and never passed on from a run another took the place of
//! (`dropped_when_late`).
//!
//! - **Only what is new counts** (the user's choice): `input_tokens` and
//!   `cache_creation_input_tokens`, never `cache_read_input_tokens`. The rest the model read
//!   from the cache, and a turn of ten requests sends it ten times. And the output: an answer
//!   still coming in is estimated at [`CHARS_PER_TOKEN`] from its text, thinking and tool
//!   arguments, and published again only once grown by [`ESTIMATE_STEP`]; its `message_delta`
//!   gives the figure.
//! - **A sub-agent is counted apart** (the user's choice), by the call that started it
//!   (`parent_tool_use_id`), for the life of the agent: one in the background works past the
//!   turn. Its count is [`AgentEvent::Sub`] and shows in its pane only.
//! - **The turn's end records its totals** on `TurnDone::tokens`, the session's own agent's
//!   only, which the window's turn-end line, `swarmed session send` and the status line's
//!   `tokens` piece show.
//! - **ACP agents report none:** the protocol only proposes usage at a turn's end, and leaves
//!   open whether its input counts cache reads, so `acp.rs` says none rather than guess.
//!
//! Tried with Claude Code 2.1.332, whose `result.usage` reports the same sums for the agent's
//! own requests.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::{content_blocks, kind_of};
use crate::agent::{AgentEvent, TokenPhase, Tokens};

/// About how many characters of an answer make a token, for the estimate while it comes in.
const CHARS_PER_TOKEN: u64 = 4;

/// How much an estimate grows before it is published again: about a hundred characters, so
/// that a reply streaming in is told a few times a second rather than with every piece.
const ESTIMATE_STEP: u64 = 25;

/// The counts of the session's own agent and of each of its sub-agents.
#[derive(Debug, Default)]
pub(super) struct Counts {
    own: Count,
    /// By the call that started each sub-agent. Kept while the agent runs: a sub-agent in the
    /// background works on past the turn that started it, and its count with it.
    subs: HashMap<String, Count>,
}

impl Counts {
    /// Takes in what Claude Code said in `message`, whose events are `events`: adds the count
    /// to publish if it changed, and at a turn's end puts the turn's totals on its
    /// [`AgentEvent::TurnDone`] and starts the next turn's count from nothing.
    pub(super) fn read(&mut self, message: &Value, events: &mut Vec<AgentEvent>) {
        if kind_of(message, "type") == Some("result") {
            let totals = std::mem::take(&mut self.own).totals();
            let ends = events.iter_mut().filter_map(totals_of);
            ends.for_each(|tokens| *tokens = totals);
            return;
        }
        let parent = kind_of(message, "parent_tool_use_id");
        let count = match parent {
            Some(parent) => self.subs.entry(parent.to_string()).or_default(),
            None => &mut self.own,
        };
        let Some((tokens, phase)) = count.read(message) else {
            return;
        };
        let event = AgentEvent::TurnTokens { tokens, phase };
        events.push(match parent {
            Some(parent) => AgentEvent::Sub {
                parent: parent.to_string(),
                event: Box::new(event),
            },
            None => event,
        });
    }
}

/// One agent's count: the session's own for the turn under way, or a sub-agent's.
#[derive(Debug, Default)]
struct Count {
    /// What the requests whose answers are done used.
    done: Tokens,
    /// The request whose answer is coming in, if one is.
    answering: Option<Answer>,
    /// Whether any request was counted. A turn that made none has no totals.
    counted: bool,
    /// The tools the agent called whose results have not come back: once the last has, the
    /// next request goes out.
    calls: HashSet<String>,
    /// Which way the tokens go; `None` before anything was heard.
    phase: Option<TokenPhase>,
    /// What was published last, if anything.
    told: Option<(Tokens, TokenPhase)>,
}

impl Count {
    /// Takes in `message`, and says the count if it should be published again.
    fn read(&mut self, message: &Value) -> Option<(Tokens, TokenPhase)> {
        match kind_of(message, "type") {
            Some("stream_event") => self.streamed(message.get("event")?),
            Some("assistant") => self.calls.extend(called(message)),
            Some("user") => self.answered(message),
            _ => {}
        }
        self.worth_telling()
    }

    /// A piece of a request's answer: its start, a bit of it, its usage, or its end.
    fn streamed(&mut self, event: &Value) {
        match kind_of(event, "type") {
            Some("message_start") => self.started(event.pointer("/message/usage")),
            Some("content_block_delta") => self.wrote(event.get("delta").map_or(0, written_chars)),
            Some("message_delta") => self.reported(event.get("usage")),
            Some("message_stop") => self.stopped(),
            _ => {}
        }
    }

    /// A request's answer starts, saying what the request was sent. An answer still coming
    /// in then was cut off, its request started again: what it used stays counted, as far as
    /// it was heard, so the count never falls and the totals hold it.
    fn started(&mut self, usage: Option<&Value>) {
        if let Some(cut_off) = self.answering.take() {
            self.done = sum(self.done, cut_off.tokens());
        }
        self.answering = Some(Answer {
            input: usage.and_then(new_input).unwrap_or_default(),
            at_start: usage.and_then(output).unwrap_or_default(),
            chars: 0,
            written: None,
        });
        self.counted = true;
        self.phase = Some(TokenPhase::Response);
    }

    /// Another `chars` characters of the answer came in.
    fn wrote(&mut self, chars: u64) {
        if let Some(answer) = &mut self.answering {
            answer.chars += chars;
        }
    }

    /// The answer says what its request came to.
    fn reported(&mut self, usage: Option<&Value>) {
        let (Some(answer), Some(usage)) = (&mut self.answering, usage) else {
            return;
        };
        answer.input = new_input(usage).unwrap_or(answer.input);
        answer.written = output(usage).or(answer.written);
    }

    /// The answer is done.
    fn stopped(&mut self) {
        if let Some(answer) = self.answering.take() {
            self.done = sum(self.done, answer.tokens());
        }
        self.phase = Some(TokenPhase::Between);
    }

    /// A message of the user's side: a message of the user's said back, which goes out as the
    /// turn's first request, or results of the agent's tools, the last of which sends the next.
    fn answered(&mut self, message: &Value) {
        let replay = message.get("isReplay").and_then(Value::as_bool) == Some(true);
        let goes_out = replay || self.last_result_in(message);
        if goes_out && self.answering.is_none() {
            self.phase = Some(TokenPhase::Prompt);
        }
    }

    /// Whether `message` holds the result of the last tool called that had not answered.
    fn last_result_in(&mut self, message: &Value) -> bool {
        let mut any = false;
        for id in results(message) {
            any |= self.calls.remove(id);
        }
        any && self.calls.is_empty()
    }

    /// Everything counted so far, the answer coming in included.
    fn total(&self) -> Tokens {
        self.answering
            .map_or(self.done, |answer| sum(self.done, answer.tokens()))
    }

    /// The turn's totals, if it made any request.
    fn totals(&self) -> Option<Tokens> {
        self.counted.then(|| self.total())
    }

    /// The count to publish, if it changed enough since it was last: a new direction, a new
    /// request, a figure the agent reported, or an estimate grown by [`ESTIMATE_STEP`].
    fn worth_telling(&mut self) -> Option<(Tokens, TokenPhase)> {
        let phase = self.phase?;
        let now = (self.total(), phase);
        let exact = self.answering.is_none_or(|answer| answer.written.is_some());
        let worth = self.told.is_none_or(|(told, told_phase)| {
            told_phase != phase
                || told.input != now.0.input
                || told.output.abs_diff(now.0.output) >= ESTIMATE_STEP
                || (exact && told.output != now.0.output)
        });
        worth.then(|| {
            self.told = Some(now);
            now
        })
    }
}

/// A request whose answer is coming in.
#[derive(Debug, Clone, Copy)]
struct Answer {
    /// What it was sent that the model read anew.
    input: u64,
    /// What its start said it had written, which is little.
    at_start: u64,
    /// How many characters of it have come in.
    chars: u64,
    /// What it wrote, once its usage says.
    written: Option<u64>,
}

impl Answer {
    fn tokens(self) -> Tokens {
        let estimate = || self.at_start.max(self.chars / CHARS_PER_TOKEN);
        Tokens {
            input: self.input,
            output: self.written.unwrap_or_else(estimate),
        }
    }
}

fn sum(a: Tokens, b: Tokens) -> Tokens {
    Tokens {
        input: a.input + b.input,
        output: a.output + b.output,
    }
}

/// What a request was sent that the model read anew: what was not cached, and what was
/// written to the cache. `None` if the usage names neither.
fn new_input(usage: &Value) -> Option<u64> {
    let of = |key: &str| usage.get(key).and_then(Value::as_u64);
    match (of("input_tokens"), of("cache_creation_input_tokens")) {
        (None, None) => None,
        (input, created) => Some(input.unwrap_or_default() + created.unwrap_or_default()),
    }
}

fn output(usage: &Value) -> Option<u64> {
    usage.get("output_tokens").and_then(Value::as_u64)
}

/// How many characters a piece of an answer adds: of its text, its thinking or the arguments of
/// a tool call it writes.
fn written_chars(delta: &Value) -> u64 {
    let key = match kind_of(delta, "type") {
        Some("text_delta") => "text",
        Some("thinking_delta") => "thinking",
        Some("input_json_delta") => "partial_json",
        _ => return 0,
    };
    let chars = kind_of(delta, key).map_or(0, |text| text.chars().count());
    u64::try_from(chars).unwrap_or(u64::MAX)
}

/// The tool calls whose results `message` holds, by their ids.
fn results(message: &Value) -> impl Iterator<Item = &str> {
    content_blocks(message)
        .filter(|block| kind_of(block, "type") == Some("tool_result"))
        .filter_map(|block| kind_of(block, "tool_use_id"))
}

/// Where a turn's end keeps its tokens; `None` for any other event.
fn totals_of(event: &mut AgentEvent) -> Option<&mut Option<Tokens>> {
    match event {
        AgentEvent::TurnDone { tokens, .. } => Some(tokens),
        _ => None,
    }
}

/// The tools an answer calls, by the ids their results will name.
fn called(message: &Value) -> impl Iterator<Item = String> + '_ {
    content_blocks(message)
        .filter(|block| kind_of(block, "type") == Some("tool_use"))
        .filter_map(|block| kind_of(block, "id"))
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// What a request's answer starts with, as Claude Code 2.1.332 streams it, cut down.
    fn start(parent: Option<&str>, input: u64, created: u64, read: u64) -> Value {
        let usage = json!({
            "input_tokens": input, "cache_creation_input_tokens": created,
            "cache_read_input_tokens": read, "output_tokens": 8,
        });
        let event = json!({ "type": "message_start", "message": { "usage": usage } });
        json!({ "type": "stream_event", "parent_tool_use_id": parent, "event": event })
    }

    fn piece(parent: Option<&str>, delta: &Value) -> Value {
        let event = json!({ "type": "content_block_delta", "delta": delta });
        json!({ "type": "stream_event", "parent_tool_use_id": parent, "event": event })
    }

    fn text(parent: Option<&str>, chars: usize) -> Value {
        piece(
            parent,
            &json!({ "type": "text_delta", "text": "x".repeat(chars) }),
        )
    }

    /// What a request's usage came to, said as its answer ends.
    fn usage(parent: Option<&str>, input: u64, created: u64, output: u64) -> Value {
        let usage = json!({
            "input_tokens": input, "cache_creation_input_tokens": created,
            "cache_read_input_tokens": 12_328, "output_tokens": output,
        });
        let event = json!({ "type": "message_delta", "usage": usage });
        json!({ "type": "stream_event", "parent_tool_use_id": parent, "event": event })
    }

    fn stop(parent: Option<&str>) -> Value {
        let event = json!({ "type": "message_stop" });
        json!({ "type": "stream_event", "parent_tool_use_id": parent, "event": event })
    }

    fn calls(parent: Option<&str>, id: &str) -> Value {
        let block = json!({ "type": "tool_use", "id": id, "name": "Bash", "input": {} });
        let message = json!({ "content": [block] });
        json!({ "type": "assistant", "parent_tool_use_id": parent, "message": message })
    }

    fn result_of(parent: Option<&str>, id: &str) -> Value {
        let block = json!({ "type": "tool_result", "tool_use_id": id, "content": "hi" });
        let message = json!({ "content": [block] });
        json!({ "type": "user", "parent_tool_use_id": parent, "message": message })
    }

    fn said_back() -> Value {
        let message = json!({ "content": "Fix the login form" });
        json!({ "type": "user", "isReplay": true, "parent_tool_use_id": null, "message": message })
    }

    fn end() -> Value {
        json!({ "type": "result", "is_error": false, "result": "Done" })
    }

    fn turn_done() -> AgentEvent {
        AgentEvent::TurnDone {
            is_error: false,
            result: Some("Done".to_string()),
            cost_usd: None,
            duration_ms: None,
            tokens: None,
        }
    }

    /// What each message adds to the events.
    fn told(counts: &mut Counts, messages: &[Value]) -> Vec<Vec<AgentEvent>> {
        messages
            .iter()
            .map(|message| told_of(counts, message))
            .collect()
    }

    /// What one message adds: a turn's end has its own event, which the count fills in.
    fn told_of(counts: &mut Counts, message: &Value) -> Vec<AgentEvent> {
        let ends = kind_of(message, "type") == Some("result");
        let mut events: Vec<AgentEvent> = ends.then(turn_done).into_iter().collect();
        counts.read(message, &mut events);
        events
    }

    fn running(input: u64, output: u64, phase: TokenPhase) -> AgentEvent {
        AgentEvent::TurnTokens {
            tokens: Tokens { input, output },
            phase,
        }
    }

    #[test]
    fn a_turn_counts_what_each_request_read_anew_and_wrote_and_ends_with_the_sums() {
        let mut counts = Counts::default();
        let own = None;
        let said = told(
            &mut counts,
            &[
                said_back(),
                start(own, 2, 36_710, 12_328),
                calls(own, "toolu_1"),
                usage(own, 2, 36_710, 77),
                stop(own),
                result_of(own, "toolu_1"),
                start(own, 2, 7_076, 49_038),
                usage(own, 2, 7_076, 20),
                stop(own),
                end(),
            ],
        );
        let after_first = 2 + 36_710;
        assert_eq!(
            said,
            vec![
                // The turn's first request goes out with the message.
                vec![running(0, 0, TokenPhase::Prompt)],
                // What it read from the cache does not count.
                vec![running(after_first, 8, TokenPhase::Response)],
                vec![],
                vec![running(after_first, 77, TokenPhase::Response)],
                // The tool runs.
                vec![running(after_first, 77, TokenPhase::Between)],
                // Its result goes out with the next request.
                vec![running(after_first, 77, TokenPhase::Prompt)],
                vec![running(after_first + 7_078, 85, TokenPhase::Response)],
                vec![running(after_first + 7_078, 97, TokenPhase::Response)],
                vec![running(after_first + 7_078, 97, TokenPhase::Between)],
                // The same sums as Claude Code's own `result` says.
                vec![AgentEvent::TurnDone {
                    is_error: false,
                    result: Some("Done".to_string()),
                    cost_usd: None,
                    duration_ms: None,
                    tokens: Some(Tokens {
                        input: 43_790,
                        output: 97,
                    }),
                }],
            ]
        );
    }

    #[test]
    fn an_answer_coming_in_is_estimated_and_told_only_every_so_often() {
        let mut counts = Counts::default();
        let own = None;
        let said = told(
            &mut counts,
            &[
                start(own, 10, 0, 0),
                // 80 characters: 20 tokens, below the 8 the start said and the step.
                text(own, 32),
                text(own, 48),
                // 120 more, 50 tokens in all: told.
                text(own, 120),
                piece(
                    own,
                    &json!({ "type": "thinking_delta", "thinking": "y".repeat(100) }),
                ),
                piece(
                    own,
                    &json!({ "type": "input_json_delta", "partial_json": "z".repeat(4) }),
                ),
                piece(
                    own,
                    &json!({ "type": "signature_delta", "signature": "s".repeat(400) }),
                ),
                // What it came to replaces the estimate.
                usage(own, 10, 0, 64),
            ],
        );
        assert_eq!(
            said,
            vec![
                vec![running(10, 8, TokenPhase::Response)],
                vec![],
                vec![],
                vec![running(10, 50, TokenPhase::Response)],
                vec![running(10, 75, TokenPhase::Response)],
                vec![],
                vec![],
                vec![running(10, 64, TokenPhase::Response)],
            ]
        );
    }

    #[test]
    fn the_next_request_goes_out_once_every_tool_called_has_answered() {
        let mut counts = Counts::default();
        let own = None;
        told(&mut counts, &[start(own, 5, 0, 0)]);
        let said = told(
            &mut counts,
            &[
                calls(own, "toolu_1"),
                calls(own, "toolu_2"),
                stop(own),
                result_of(own, "toolu_2"),
                // Another's result, or one heard twice, sends nothing.
                result_of(own, "toolu_9"),
                result_of(own, "toolu_1"),
            ],
        );
        assert_eq!(
            said,
            vec![
                vec![],
                vec![],
                vec![running(5, 8, TokenPhase::Between)],
                vec![],
                vec![],
                vec![running(5, 8, TokenPhase::Prompt)],
            ]
        );
    }

    #[test]
    fn a_sub_agents_count_is_its_own_and_outlasts_its_starters_turn() {
        let mut counts = Counts::default();
        let sub = Some("toolu_agent");
        let as_sub = |event| AgentEvent::Sub {
            parent: "toolu_agent".to_string(),
            event: Box::new(event),
        };
        let said = told(
            &mut counts,
            &[
                start(None, 3, 100, 0),
                start(sub, 4, 1_000, 0),
                usage(sub, 4, 1_000, 30),
                stop(sub),
                stop(None),
                end(),
                start(sub, 1, 50, 0),
            ],
        );
        assert_eq!(said[0], vec![running(103, 8, TokenPhase::Response)]);
        assert_eq!(
            said[1],
            vec![as_sub(running(1_004, 8, TokenPhase::Response))]
        );
        assert_eq!(
            said[3],
            vec![as_sub(running(1_004, 30, TokenPhase::Between))]
        );
        // The turn's totals are its own agent's alone.
        let totals = match &said[5][..] {
            [AgentEvent::TurnDone { tokens, .. }] => *tokens,
            other => panic!("expected the turn's end, got {other:?}"),
        };
        assert_eq!(
            totals,
            Some(Tokens {
                input: 103,
                output: 8
            })
        );
        // A sub-agent in the background counts on after the turn that started it.
        assert_eq!(
            said[6],
            vec![as_sub(running(1_004 + 51, 38, TokenPhase::Response))]
        );
    }

    #[test]
    fn a_turn_that_made_no_request_has_no_totals_and_the_next_starts_from_nothing() {
        let mut counts = Counts::default();
        let said = told(&mut counts, &[said_back(), end()]);
        assert_eq!(said[1], vec![turn_done()]);
        let said = told(&mut counts, &[start(None, 1, 2, 3), stop(None), end()]);
        assert_eq!(said[0], vec![running(3, 8, TokenPhase::Response)]);
        let next = told(&mut counts, &[said_back()]);
        assert_eq!(next[0], vec![running(0, 0, TokenPhase::Prompt)]);
    }

    #[test]
    fn a_request_started_again_before_its_answer_ended_keeps_what_the_first_used() {
        let mut counts = Counts::default();
        let own = None;
        let said = told(
            &mut counts,
            &[
                start(own, 10, 0, 0),
                // 200 characters, 50 tokens, then the stream breaks: no `message_stop`.
                text(own, 200),
                start(own, 10, 0, 0),
                usage(own, 10, 0, 30),
                stop(own),
                end(),
            ],
        );
        assert_eq!(
            said[..5],
            [
                vec![running(10, 8, TokenPhase::Response)],
                vec![running(10, 50, TokenPhase::Response)],
                // The count goes on from the first attempt's, and never falls.
                vec![running(20, 58, TokenPhase::Response)],
                vec![running(20, 80, TokenPhase::Response)],
                vec![running(20, 80, TokenPhase::Between)],
            ]
        );
        let totals = match &said[5][..] {
            [AgentEvent::TurnDone { tokens, .. }] => *tokens,
            other => panic!("expected the turn's end, got {other:?}"),
        };
        let both = Tokens {
            input: 20,
            output: 80,
        };
        assert_eq!(totals, Some(both));
    }

    #[test]
    fn a_message_said_back_while_an_answer_comes_in_leaves_its_direction_alone() {
        let mut counts = Counts::default();
        told(&mut counts, &[start(None, 1, 0, 0)]);
        assert_eq!(
            told(&mut counts, &[said_back()]),
            vec![Vec::<AgentEvent>::new()]
        );
    }
}
