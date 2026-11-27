//! Forking a session: a new one that starts where an earlier conversation stood.
//!
//! No agent can be handed another's memory, so the fork is given the conversation as text. What
//! was said is copied into the new session, for the user to read, and goes to its agent with
//! the first message, whatever agent that is.

use crate::agent::{AgentEvent, Supervision};
use crate::agents::CLAUDE;
use crate::rewind;
use crate::sessions::{Session, SessionOptions};
use crate::{Core, Result};

/// The most of an earlier conversation an agent is handed, in characters. Its end matters
/// most, so that is what is kept.
const MAX_CARRIED: usize = 60_000;

/// Where a fork that starts from message number `from` begins in a conversation, counting
/// from 0 every message sent into it: at that message, or at the very start if no message
/// is named or there is no such message.
///
/// A message is counted, not an event's place: a front end's list of events also holds ones
/// that are never stored, so a place in it says nothing of a place here.
fn start_of(events: &[AgentEvent], from: Option<usize>) -> usize {
    let messages = events
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, AgentEvent::UserMessage { .. }));
    let at = from.and_then(|from| messages.map(|(at, _)| at).nth(from));
    at.unwrap_or(0)
}

/// What of a conversation a fork starts with: what was said and shown from the message it is
/// forked from to the end, not how it came about, and nothing from before that message. What
/// a rewind took the conversation back from is no part of it.
fn kept(events: &[AgentEvent], from: Option<usize>) -> Vec<AgentEvent> {
    let start = start_of(events, from);
    let dropped = rewind::dropped(events);
    events
        .iter()
        .enumerate()
        .skip(start)
        .filter(|(at, _)| !dropped.iter().any(|range| range.contains(at)))
        .map(|(_, event)| event)
        .filter(|event| {
            matches!(
                event,
                AgentEvent::UserMessage { .. } | AgentEvent::Text { .. } | AgentEvent::View { .. }
            )
        })
        .cloned()
        .collect()
}

/// A conversation as text, each turn under who said it.
fn transcript<'a>(events: impl IntoIterator<Item = &'a AgentEvent>) -> String {
    let turns: Vec<String> = events.into_iter().filter_map(turn).collect();
    joined(&turns, MAX_CARRIED)
}

/// What an event says in a conversation written out as text, under who said it.
pub(crate) fn turn(event: &AgentEvent) -> Option<String> {
    match event {
        AgentEvent::UserMessage { text, .. } => Some(format!("User:\n{text}")),
        AgentEvent::Text { text } => Some(format!("Assistant:\n{text}")),
        _ => None,
    }
}

/// `turns` as one text of at most `most` characters. The end of a conversation matters most,
/// so of one that is longer the end is kept, and the text says its beginning is left out.
pub(crate) fn joined(turns: &[String], most: usize) -> String {
    let whole = turns.join("\n\n");
    let length = whole.chars().count();
    if length <= most {
        return whole;
    }
    let end: String = whole.chars().skip(length - most).collect();
    format!("[The beginning is left out.]\n…{end}")
}

/// What a fork's agent has to be told with the next message: the conversation it was forked
/// from, until a turn in the fork itself has ended well. A message alone is not enough: an
/// agent that died on the first one never heard the conversation, and the next try needs it.
/// So is the agent of a session taken back to an earlier message that started a new
/// conversation there ([`rewind::starts_anew`]): what was said up to that message.
pub(crate) fn carried(events: &[AgentEvent]) -> Option<String> {
    let forked = events.iter().rposition(rewind::starts_anew)?;
    let ended_well = |event: &AgentEvent| {
        matches!(
            event,
            AgentEvent::TurnDone {
                is_error: false,
                ..
            }
        )
    };
    if events[forked..].iter().any(ended_well) {
        return None;
    }
    // Up to and with the mark, so that what a rewind there dropped is left out.
    let earlier = transcript(rewind::counted(&events[..=forked]));
    (!earlier.is_empty()).then_some(earlier)
}

/// `text` with the earlier conversation under it, the way the agent is to read it.
pub(crate) fn with_carried(text: &str, earlier: &str) -> String {
    format!(
        "{text}\n\n<earlier-conversation>\nThis session continues an earlier conversation, \
         which you do not otherwise remember. What was said in it:\n\n{earlier}\n\
         </earlier-conversation>"
    )
}

/// The supervision a clone on `agent` starts with: the source's, except that only Claude Code
/// plans before it acts, so a planning session asks instead on any other agent.
fn supervision_on(agent: &str, supervision: Supervision) -> Supervision {
    if supervision == Supervision::Plan && agent != CLAUDE {
        Supervision::Ask
    } else {
        supervision
    }
}

impl Core {
    /// A new session in the same task and on the same agent, starting with what was said in
    /// session `id`: all of it, or its message number `from` and everything after it,
    /// counting from 0 every message sent into it. What came before that message stays
    /// behind. The first session is not changed. Nothing runs until the new one is started,
    /// and its agent is given the conversation with the first message.
    ///
    /// With `agent`, the new session runs on that agent instead: a clone of the conversation
    /// for another agent to carry on. It gets what was said, as text, and not what the first
    /// agent did or read to say it. Model, effort and fast mode are that agent's own to start
    /// with, since they mean something else on every agent.
    pub fn session_fork(
        &self,
        id: i64,
        from: Option<usize>,
        agent: Option<&str>,
    ) -> Result<Session> {
        let source = self.session(id)?;
        let title = if source.title.is_empty() {
            "Fork".to_string()
        } else {
            format!("{} (fork)", source.title)
        };
        let task = source.task_id;
        self.fork_to(&source, &Fork { from, agent, task }, title)
    }

    /// A new session in task `task` of `project`, on the same agent and under the same title,
    /// starting with all that was said in session `id`, as [`Core::session_fork`] starts one.
    /// For a chat whose work belongs in a repository: it carries on in a task there, and the
    /// chat stays as it is.
    pub fn session_fork_into(&self, id: i64, project: &str, task: &str) -> Result<Session> {
        let source = self.session(id)?;
        let (_, task) = self.task(project, task)?;
        let title = source.title.clone();
        let fork = Fork {
            from: None,
            agent: None,
            task: task.id,
        };
        self.fork_to(&source, &fork, title)
    }

    fn fork_to(&self, source: &Session, fork: &Fork<'_>, title: String) -> Result<Session> {
        let mut earlier = kept(&self.session_events(source.id)?, fork.from);
        let forked = match fork.agent.filter(|agent| *agent != source.agent) {
            None => {
                let options = SessionOptions {
                    agent: Some(source.agent.clone()),
                    title: Some(title),
                    model: source.model.clone(),
                    supervision: source.supervision,
                    effort: source.effort,
                    fast: source.fast,
                    // A fork keeps what its session is for, and starts working for nobody.
                    role: source.role.clone(),
                };
                // As it is, with what its role took the place of: a supervision the user chose
                // in the first session still wins over the role's in the fork.
                let before = self.store().session_supervision_before_role(source.id)?;
                let store = self.store();
                let forked = store.session_insert(fork.task, &options)?;
                store.session_set_supervision_before_role(forked.id, before)?;
                forked
            }
            Some(other) => {
                self.agents_now().backend(other)?;
                // A new start on another agent: as the first would be without its role, and
                // then under the supervision its role holds, as any session with it starts.
                let without = self.supervision_without_role(source)?;
                let options = SessionOptions {
                    agent: Some(other.to_string()),
                    title: Some(title),
                    model: None,
                    supervision: supervision_on(other, without),
                    effort: None,
                    fast: false,
                    role: source.role.clone(),
                };
                self.session_insert(fork.task, &options)?
            }
        };
        // The images pasted into the conversation are the fork's own from the start.
        let copied = self
            .images_forked(source.id, forked.id, &mut earlier)
            .and_then(|()| {
                self.store()
                    .session_copy_events(forked.id, &earlier, source.id)
            });
        if let Err(error) = copied {
            // Half a fork is none: nothing of it is left.
            self.forget_images(forked.id);
            self.store().session_delete(forked.id).ok();
            return Err(error);
        }
        Ok(forked)
    }
}

/// Where a fork goes and what it takes: from which message, on which agent, in which task.
struct Fork<'a> {
    from: Option<usize>,
    agent: Option<&'a str>,
    task: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> AgentEvent {
        AgentEvent::UserMessage {
            text: text.to_string(),
            remote: false,
            at_reset: false,
        }
    }

    fn reply(text: &str) -> AgentEvent {
        AgentEvent::Text {
            text: text.to_string(),
        }
    }

    fn done() -> AgentEvent {
        AgentEvent::TurnDone {
            is_error: false,
            result: None,
            cost_usd: None,
            duration_ms: None,
            tokens: None,
        }
    }

    #[test]
    fn a_clone_on_another_agent_asks_where_the_first_one_planned() {
        assert_eq!(supervision_on("codex", Supervision::Plan), Supervision::Ask);
        assert_eq!(supervision_on(CLAUDE, Supervision::Plan), Supervision::Plan);
        assert_eq!(
            supervision_on("codex", Supervision::Auto),
            Supervision::Auto
        );
    }

    #[test]
    fn a_fork_keeps_what_was_said_from_the_message_asked_for() {
        let events = [
            user("one"),
            reply("two"),
            done(),
            user("three"),
            reply("four"),
        ];
        assert_eq!(
            kept(&events, None),
            [user("one"), reply("two"), user("three"), reply("four")]
        );
        // From the second message: it and its answer, and nothing from before.
        assert_eq!(kept(&events, Some(1)), [user("three"), reply("four")]);
        assert_eq!(kept(&events, Some(0)).len(), 4);
        // No such message: all of it.
        assert_eq!(kept(&events, Some(99)).len(), 4);
    }

    #[test]
    fn the_earlier_conversation_is_carried_until_a_turn_in_the_fork_ends_well() {
        let forked = AgentEvent::Forked { from: 7 };
        let earlier = Some("User:\none\n\nAssistant:\ntwo");
        let fresh = [user("one"), reply("two"), forked.clone()];
        assert_eq!(carried(&fresh).as_deref(), earlier);
        // The agent died on the first message, or its turn failed: the next try carries it too.
        let died = AgentEvent::Exited {
            code: Some(1),
            stderr: String::new(),
        };
        let failed = AgentEvent::TurnDone {
            is_error: true,
            result: None,
            cost_usd: None,
            duration_ms: None,
            tokens: None,
        };
        for ending in [died, failed] {
            let retried = [
                user("one"),
                reply("two"),
                forked.clone(),
                user("three"),
                ending,
            ];
            assert_eq!(carried(&retried).as_deref(), earlier);
        }
        let continued = [
            user("one"),
            reply("two"),
            forked.clone(),
            user("three"),
            done(),
        ];
        assert_eq!(carried(&continued), None);
        assert_eq!(carried(&[user("one")]), None);
        // A fork of nothing has nothing to hand on.
        assert_eq!(carried(&[forked]), None);
    }

    #[test]
    fn a_long_conversation_keeps_its_end() {
        let long = [user(&"a".repeat(MAX_CARRIED)), reply("the end")];
        let text = transcript(&long);
        assert!(text.starts_with("[The beginning is left out.]"));
        assert!(text.ends_with("the end"));
        assert!(text.chars().count() < MAX_CARRIED + 60);
    }

    #[test]
    fn the_agent_reads_the_message_first_and_the_history_marked_as_such() {
        let message = with_carried("Go on.", "User:\nhi");
        assert!(message.starts_with("Go on.\n\n<earlier-conversation>"));
        assert!(message.ends_with("User:\nhi\n</earlier-conversation>"));
    }
}
