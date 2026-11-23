//! Shells OpenCode leaves running in the background.
//!
//! OpenCode runs a command in the background when asked to, or when it outlasts its timeout,
//! and ends the call at once with its shell's id and `status: "running"` in the call's
//! metadata. Nothing over ACP says when such a shell ends: OpenCode only wakes the agent, which
//! takes it up in a turn of its own, without saying which shell it was. So a shell is reported
//! running until the agent starts a turn by itself or goes, and there is no stopping one: the
//! protocol has no way to stop a single shell.

use serde_json::Value;

use super::str_of;
use crate::agent::{AgentEvent, BackgroundState};

/// What an end inferred, not heard of, is said to be.
const INFERRED: &str = "OpenCode does not say when a shell ends, or how: it was taken to have \
ended when the agent next took something up by itself.";

/// The shells reported running, by their ids.
#[derive(Debug, Default)]
pub(super) struct Shells {
    running: Vec<Running>,
}

#[derive(Debug)]
struct Running {
    id: String,
    call: String,
    description: String,
}

impl Shells {
    /// A report of a shell left running by call `call`, which ended as `update` says with the
    /// arguments `input`, if it left one running; it is followed from then on.
    pub(super) fn left_running(
        &mut self,
        call: &str,
        update: &Value,
        input: Option<&Value>,
    ) -> Option<AgentEvent> {
        let metadata = update.pointer("/rawOutput/metadata")?;
        if str_of(metadata, "status") != Some("running") {
            return None;
        }
        let id = str_of(metadata, "shellID")?;
        let said = |key: &str| input.and_then(|input| str_of(input, key));
        let description = said("description").or_else(|| said("command"));
        let shell = Running {
            id: id.to_string(),
            call: call.to_string(),
            description: description.unwrap_or(id).to_string(),
        };
        let report = shell.report(BackgroundState::Running, None);
        self.running.retain(|one| one.id != id);
        self.running.push(shell);
        Some(report)
    }

    /// Reports every shell that was running as ended: the agent took something up by itself,
    /// which is what the end of one wakes it for.
    pub(super) fn ended(&mut self) -> Vec<AgentEvent> {
        let ended = std::mem::take(&mut self.running);
        let report =
            |shell: Running| shell.report(BackgroundState::Completed, Some(INFERRED.to_string()));
        ended.into_iter().map(report).collect()
    }
}

impl Running {
    fn report(&self, state: BackgroundState, summary: Option<String>) -> AgentEvent {
        AgentEvent::Background {
            task_id: self.id.clone(),
            description: self.description.clone(),
            kind: "local_bash".to_string(),
            state,
            summary,
            tool_use_id: Some(self.call.clone()),
            output_file: None,
            moved: false,
            unstoppable: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_shell_left_running_is_reported_until_the_agent_takes_something_up_by_itself() {
        let mut shells = Shells::default();
        let ended = json!({ "status": "completed",
            "rawOutput": { "metadata": { "status": "running", "truncated": false, "shellID": "sh_X", "pid": 1 } } });
        let input = json!({ "command": "pnpm dev", "background": true });
        let report = shells.left_running("call_2", &ended, Some(&input));
        assert_eq!(
            report,
            Some(AgentEvent::Background {
                task_id: "sh_X".into(),
                description: "pnpm dev".into(),
                kind: "local_bash".into(),
                state: BackgroundState::Running,
                summary: None,
                tool_use_id: Some("call_2".into()),
                output_file: None,
                moved: false,
                unstoppable: true,
            })
        );
        let over = shells.ended();
        assert!(
            matches!(&over[..], [AgentEvent::Background { task_id, state: BackgroundState::Completed, .. }] if task_id == "sh_X")
        );
        assert!(shells.ended().is_empty(), "each end is told once");
    }

    #[test]
    fn a_command_that_ended_leaves_no_shell() {
        let mut shells = Shells::default();
        let done = json!({ "rawOutput": { "metadata": { "status": "completed", "exit": 0 } } });
        assert_eq!(shells.left_running("c", &done, None), None);
        assert_eq!(
            shells.left_running("c", &json!({ "rawOutput": "x" }), None),
            None
        );
    }
}
