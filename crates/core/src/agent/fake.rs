//! A scripted agent for tests and for working on a front end without spending tokens.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;
use tokio::sync::mpsc;

use super::{
    AgentBackend, AgentCommand, AgentEvent, AgentHandle, PLAN_TOOL, SessionSpec, Supervision,
};
use crate::error::Result;

/// A stand-in agent whose replies depend only on the user's message.
///
/// - plain text: replies `echo: <text>` and ends the turn;
/// - `run: <command>`: asks permission for a `Bash` tool call, then reports the outcome;
/// - `slow`: starts a turn that only ends when interrupted;
/// - in plan mode, anything: presents a plan and waits for it to be approved.
#[derive(Debug, Default)]
pub struct FakeBackend {
    sessions: AtomicU64,
}

impl AgentBackend for FakeBackend {
    fn start(&self, spec: SessionSpec) -> Result<AgentHandle> {
        let (event_tx, events) = mpsc::unbounded_channel();
        let (commands, command_rx) = mpsc::unbounded_channel();
        let number = self.sessions.fetch_add(1, Ordering::Relaxed) + 1;
        let script = Script {
            spec,
            number,
            events: event_tx,
            commands: command_rx,
            requests: 0,
            initialised: false,
        };
        drop(tokio::spawn(script.run()));
        Ok(AgentHandle { events, commands })
    }
}

/// Whether the session should keep going after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    Continue,
    /// The command channel closed: the front end is gone.
    Stop,
}

struct Script {
    spec: SessionSpec,
    number: u64,
    events: mpsc::UnboundedSender<AgentEvent>,
    commands: mpsc::UnboundedReceiver<AgentCommand>,
    requests: u64,
    initialised: bool,
}

impl Script {
    fn emit(&self, event: AgentEvent) {
        self.events.send(event).ok();
    }

    fn text(&self, text: impl Into<String>) {
        self.emit(AgentEvent::Text { text: text.into() });
    }

    async fn run(mut self) {
        while self.step().await == Next::Continue {}
    }

    /// Handles one command; anything but a user message is ignored between turns.
    async fn step(&mut self) -> Next {
        let Some(command) = self.commands.recv().await else {
            self.emit(AgentEvent::Exited {
                code: Some(0),
                stderr: String::new(),
            });
            return Next::Stop;
        };
        let message = match command {
            AgentCommand::UserMessage(message) => message,
            AgentCommand::SetSupervision(supervision) => {
                self.spec.supervision = supervision;
                return Next::Continue;
            }
            _ => return Next::Continue,
        };
        if !self.initialised {
            self.initialised = true;
            self.emit(self.init());
        }
        self.turn(&message).await
    }

    fn init(&self) -> AgentEvent {
        let fresh = || format!("fake-{}", self.number);
        AgentEvent::Init {
            agent_session_id: self.spec.resume.clone().unwrap_or_else(fresh),
            model: self
                .spec
                .model
                .clone()
                .unwrap_or_else(|| "fake".to_string()),
            permission_mode: self.spec.supervision.as_str().to_string(),
            api_key_source: "none".to_string(),
            mcp_servers: vec![],
            skills: 0,
            slash_commands: vec!["context".to_string(), "review".to_string()],
        }
    }

    async fn turn(&mut self, message: &str) -> Next {
        let done = |is_error, result: Option<&str>| AgentEvent::TurnDone {
            is_error,
            result: result.map(String::from),
            cost_usd: Some(0.0),
        };
        if message == "instructions" {
            let told = self.spec.append_system_prompt.clone();
            self.text(told.unwrap_or_else(|| "none".to_string()));
            self.emit(done(false, None));
            return Next::Continue;
        }
        if message == "slow" {
            self.text("working…");
            let interrupt = wait_for(&mut self.commands, |c| matches!(c, AgentCommand::Interrupt));
            let next = interrupt.await.map_or(Next::Stop, |_| Next::Continue);
            self.emit(done(true, Some("interrupted")));
            return next;
        }
        let next = match message.strip_prefix("run:") {
            _ if self.spec.supervision == Supervision::Plan => self.plan(message).await,
            Some(shell) => self.run_tool(shell.trim()).await,
            None => self.echo(message),
        };
        if next == Next::Continue {
            self.emit(done(false, None));
        }
        next
    }

    fn echo(&self, message: &str) -> Next {
        self.text(format!("echo: {message}"));
        Next::Continue
    }

    /// Asks permission for a `Bash` call and reports what the decision led to.
    async fn run_tool(&mut self, shell: &str) -> Next {
        let input = json!({ "command": shell });
        let Some(allowed) = self.ask("Bash", input).await else {
            return Next::Stop;
        };
        self.text(if allowed { "ran it" } else { "not allowed" });
        Next::Continue
    }

    /// Presents a plan for what was asked and reports whether it was approved.
    async fn plan(&mut self, message: &str) -> Next {
        let plan = format!("1. Look into: {message}\n2. Do it.");
        let Some(approved) = self.ask(PLAN_TOOL, json!({ "plan": plan })).await else {
            return Next::Stop;
        };
        self.text(if approved {
            "carrying out the plan"
        } else {
            "still planning"
        });
        Next::Continue
    }

    /// Calls a tool after asking for permission. `None` if the front end went away meanwhile.
    async fn ask(&mut self, tool: &str, input: serde_json::Value) -> Option<bool> {
        self.requests += 1;
        let request_id = format!("req-{}", self.requests);
        let tool_use_id = format!("tool-{}", self.requests);
        self.emit(AgentEvent::ToolUse {
            id: tool_use_id.clone(),
            name: tool.to_string(),
            input: input.clone(),
        });
        self.emit(AgentEvent::PermissionRequest {
            request_id: request_id.clone(),
            tool_name: tool.to_string(),
            input,
            description: None,
        });
        let decision = wait_for(&mut self.commands, |c| decides(c, &request_id)).await?;
        let allowed = matches!(decision, AgentCommand::Allow { .. });
        self.emit(AgentEvent::ToolResult {
            tool_use_id,
            content: if allowed { "ok" } else { "denied" }.to_string(),
            is_error: !allowed,
        });
        Some(allowed)
    }
}

/// Whether `command` answers the permission request `request_id`.
fn decides(command: &AgentCommand, request_id: &str) -> bool {
    match command {
        AgentCommand::Allow { request_id: id } | AgentCommand::Deny { request_id: id, .. } => {
            id == request_id
        }
        _ => false,
    }
}

/// The next command `wanted` accepts, skipping the others; `None` if the channel closed.
async fn wait_for(
    commands: &mut mpsc::UnboundedReceiver<AgentCommand>,
    wanted: impl Fn(&AgentCommand) -> bool,
) -> Option<AgentCommand> {
    loop {
        let command = commands.recv().await?;
        if wanted(&command) {
            return Some(command);
        }
    }
}
