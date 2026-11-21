//! Handing what a session arrived at to a cloud agent, with the conversation it came from.
//!
//! A cloud agent starts with no knowledge of a session here, and a brief written for it loses
//! whatever its writer thought unimportant. So the conversation itself can go along: what the
//! user and the agent said, and the plans the agent proposed, as text under what the cloud
//! agent is asked to do. Not what the agent read or did on the way, and no file: the cloud
//! agent has the repository as it is on GitHub, and nothing that is only on this machine.

use crate::agent::{AgentEvent, PLAN_TOOL};
use crate::cloud::{CloudOrigin, CloudProvider, CloudTask};
use crate::error::Error;
use crate::{Core, Result, fork};

/// The most of a conversation a cloud agent is handed, in characters. The prompt goes to the
/// provider's program as one argument, and this stays far below what a system allows there.
pub const MAX_HANDED: usize = 60_000;

/// What the cloud agent is asked to do when the user says nothing more.
const CARRY_ON: &str = "Carry on with the work this conversation arrived at. If it ends in a \
                        plan, carry that plan out.";

/// A plan the agent proposed, which it presents by asking to use [`PLAN_TOOL`]: it is in no
/// reply, and it is the very thing a cloud agent is most often to carry out.
fn plan(event: &AgentEvent) -> Option<String> {
    let AgentEvent::PermissionRequest {
        tool_name, input, ..
    } = event
    else {
        return None;
    };
    let plan = input.get("plan")?.as_str()?.trim();
    (tool_name == PLAN_TOOL && !plan.is_empty())
        .then(|| format!("Plan proposed by the assistant:\n{plan}"))
}

/// The conversation as a cloud agent is handed it, or `None` if nothing was said in it.
fn conversation(events: &[AgentEvent]) -> Option<String> {
    let turns: Vec<String> = events
        .iter()
        .filter_map(|event| fork::turn(event).or_else(|| plan(event)))
        .collect();
    (!turns.is_empty()).then(|| fork::joined(&turns, MAX_HANDED))
}

/// What the cloud agent is sent: what to do first, then the conversation, marked as such.
fn prompt(instruction: &str, conversation: &str) -> String {
    let instruction = match instruction.trim() {
        "" => CARRY_ON,
        given => given,
    };
    format!(
        "{instruction}\n\n<earlier-conversation>\nThis work was prepared in a conversation \
         between the user and an agent on the user's own machine, which you do not otherwise \
         know. You have the repository as it is on GitHub: files and changes the conversation \
         mentions that were never pushed are not there. What was said:\n\n{conversation}\n\
         </earlier-conversation>"
    )
}

impl Core {
    /// Hands the conversation of `session` to a cloud agent, under `instruction`, which says
    /// what to do with it; left empty, the agent is told to carry on where it ended. The
    /// agent starts from the branch of the session's task if GitHub has it.
    ///
    /// The user does this, not the session's agent, so it does not count towards what a
    /// session may delegate. Refused like [`Core::cloud_delegate`], and for a session in
    /// which nothing was said.
    pub async fn cloud_delegate_conversation(
        &self,
        session: i64,
        instruction: &str,
        provider: CloudProvider,
    ) -> Result<CloudTask> {
        let events = self.session_events(session)?;
        let conversation = conversation(&events).ok_or_else(|| {
            Error::Cloud(
                "nothing was said in this session yet, so there is nothing to hand on".into(),
            )
        })?;
        let session = self.session(session)?;
        let (task, project) = {
            let store = self.store();
            let missing = || Error::SessionNotFound(session.id);
            let task = store.task_by_id(session.task_id)?.ok_or_else(missing)?;
            let project = store.project_by_id(task.project_id)?.ok_or_else(missing)?;
            (task, project)
        };
        let origin = CloudOrigin {
            task: Some(&task.name),
            session: None,
            provider,
        };
        let prompt = prompt(instruction, &conversation);
        self.cloud_delegate(&project.name, &prompt, origin).await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn user(text: &str) -> AgentEvent {
        AgentEvent::UserMessage { text: text.into() }
    }

    fn reply(text: &str) -> AgentEvent {
        AgentEvent::Text { text: text.into() }
    }

    fn asks(tool: &str, input: serde_json::Value) -> AgentEvent {
        AgentEvent::PermissionRequest {
            request_id: "r1".into(),
            tool_name: tool.into(),
            input,
            description: None,
        }
    }

    #[test]
    fn what_was_said_and_planned_goes_along_and_what_was_done_does_not() {
        let events = [
            user("plan the move"),
            reply("Looking."),
            asks("Bash", json!({ "command": "cat .env" })),
            asks(PLAN_TOOL, json!({ "plan": "1. Move it." })),
        ];
        let said = conversation(&events).expect("something was said");
        assert_eq!(
            said,
            "User:\nplan the move\n\nAssistant:\nLooking.\n\n\
             Plan proposed by the assistant:\n1. Move it."
        );
        assert_eq!(conversation(&[asks("Bash", json!({}))]), None);
    }

    #[test]
    fn a_long_conversation_keeps_its_end() {
        let said = conversation(&[user(&"a".repeat(MAX_HANDED)), reply("the plan")]);
        let said = said.expect("something was said");
        assert!(said.starts_with("[The beginning is left out.]"), "{said}");
        assert!(said.ends_with("Assistant:\nthe plan"), "{said}");
    }

    #[test]
    fn the_agent_reads_what_to_do_first_and_the_conversation_marked_as_such() {
        let asked = prompt("  do step 2 only ", "User:\nhello");
        assert!(
            asked.starts_with("do step 2 only\n\n<earlier-conversation>"),
            "{asked}"
        );
        assert!(
            asked.ends_with("User:\nhello\n</earlier-conversation>"),
            "{asked}"
        );
        assert!(prompt(" ", "User:\nhello").starts_with(CARRY_ON));
    }
}
