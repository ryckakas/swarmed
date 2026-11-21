//! One question to an agent, outside every session: a conversation of its own that Swarmed
//! does not keep, for the small things an agent can write for the user.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;

use crate::agent::{AgentBackend, AgentCommand, AgentEvent, SessionSpec, Supervision};

/// What the agent is told when it wants to use a tool: it was asked for words only.
const NO_TOOLS: &str = "Answer from what you were given, without using tools.";

/// Reads the agent's reply to the end of its turn, refusing every tool on the way. `None` if
/// the turn failed or the agent went away.
async fn reply(
    events: &mut UnboundedReceiver<AgentEvent>,
    commands: &tokio::sync::mpsc::UnboundedSender<AgentCommand>,
) -> Option<String> {
    let mut said = String::new();
    while let Some(event) = events.recv().await {
        match event {
            AgentEvent::Text { text } => said.push_str(&text),
            AgentEvent::PermissionRequest { request_id, .. } => {
                let reason = NO_TOOLS.to_string();
                commands
                    .send(AgentCommand::Deny { request_id, reason })
                    .ok();
            }
            AgentEvent::TurnDone { is_error, .. } => return (!is_error).then_some(said),
            AgentEvent::Exited { .. } => return None,
            _ => {}
        }
    }
    None
}

/// Starts `backend` as `spec` says, asks it `question` and returns what it answered, if it
/// did within `limit`. The agent is gone again when this returns.
///
/// What it is asked about may be anything, a diff somebody else wrote for one, so it is held
/// to words whatever `spec` says of supervision: put into the mode of its own that plans or
/// only reads, and refused every tool it asks for. An agent with modes none of which holds it
/// back would run tools without asking; it is not started, and there is no answer.
pub(crate) async fn ask(
    backend: Arc<dyn AgentBackend>,
    spec: SessionSpec,
    question: String,
    limit: Duration,
) -> Option<String> {
    let spec = SessionSpec {
        supervision: Supervision::Plan,
        words_only: true,
        ..spec
    };
    let mut handle = backend.start(spec).ok()?;
    handle
        .commands
        .send(AgentCommand::UserMessage(question))
        .ok()?;
    let answer = tokio::time::timeout(limit, reply(&mut handle.events, &handle.commands)).await;
    // Nobody listens to it any more. An agent that took too long, or was refused a tool and
    // waits, would otherwise stay: it is ended for certain.
    for task in &handle.tasks {
        task.abort();
    }
    answer.ok().flatten()
}
