//! A scripted agent for tests and for working on a front end without spending tokens.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;
use tokio::sync::mpsc;

use super::{
    AgentBackend, AgentCommand, AgentEvent, AgentHandle, BackgroundState, PLAN_TOOL, ScreenKind,
    ScreenRow, ScreenSection, SessionSpec, Supervision, UsageLimit,
};
use crate::error::Result;

/// A stand-in agent whose replies depend only on the user's message.
///
/// - plain text: replies `echo: <text>` and ends the turn;
/// - `run: <command>`: asks permission for a `Bash` tool call, then reports the outcome; a stop
///   meanwhile takes the request back and ends the turn;
/// - `env: <NAME>`: replies with the value of that variable as given for the session, or `unset`;
/// - `slow`: starts a turn that only ends when interrupted;
/// - `fail`: ends the turn with an error;
/// - `suggest: <text>`: replies as to plain text, then offers `<text>` as the next message;
/// - `background agent`: starts a sub-agent in the background and ends the turn; the sub-agent
///   then writes, reports its end, and the agent takes a turn of its own to read its report;
///   `background agent left` leaves it at work, with no end;
/// - `background bash`: leaves a command running in the background and ends the turn;
/// - in plan mode, anything: presents a plan and waits for it to be approved.
#[derive(Debug, Default)]
pub struct FakeBackend {
    sessions: AtomicU64,
}

impl AgentBackend for FakeBackend {
    /// `echo`, as a stand-in for a screen: it prints the command it was opened on, and ends.
    fn interactive(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/bin/echo"))
    }

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
        let script = tokio::spawn(script.run());
        Ok(AgentHandle {
            events,
            commands,
            pid: None,
            tasks: vec![script.abort_handle()],
        })
    }
}

/// Whether the session should keep going after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    Continue,
    /// The turn was stopped and has said so. The session goes on.
    Interrupted,
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

    /// Leaves something running behind the turn, as Claude Code reports it: a sub-agent
    /// (`agent`), which works on after the turn and is read in a turn of the agent's own, or a
    /// command (`bash`), which runs on.
    fn background(&self, what: &str) {
        let report = |state, kind: &str, summary: Option<&str>| AgentEvent::Background {
            task_id: format!("bg-{what}"),
            description: format!("background {what}"),
            kind: kind.to_string(),
            state,
            summary: summary.map(str::to_string),
            tool_use_id: what.starts_with("agent").then(|| "tool-bg".to_string()),
            // Named as Claude Code names it, and never there: the fake writes nothing.
            output_file: (!what.starts_with("agent"))
                .then(|| format!("/tmp/swarmed-fake-agent/bg-{what}.output")),
        };
        if !what.starts_with("agent") {
            self.emit(report(BackgroundState::Running, "local_bash", None));
            self.emit(done(false, None));
            return;
        }
        self.emit(AgentEvent::ToolUse {
            id: "tool-bg".to_string(),
            name: "Agent".to_string(),
            input: json!({ "description": "look around", "run_in_background": true }),
        });
        self.emit(report(BackgroundState::Running, "local_agent", None));
        self.emit(AgentEvent::ToolResult {
            tool_use_id: "tool-bg".to_string(),
            content: "Async agent launched successfully.".to_string(),
            is_error: false,
        });
        self.emit(done(false, Some("It works in the background.")));
        if what != "agent" {
            return;
        }
        let said = AgentEvent::Text {
            text: "still at it".to_string(),
        };
        self.emit(AgentEvent::Sub {
            parent: "tool-bg".to_string(),
            event: Box::new(said),
        });
        self.emit(report(BackgroundState::Completed, "", Some("found it")));
        self.text("It is back: found it.");
        self.emit(done(false, Some("It is back.")));
    }

    async fn run(mut self) {
        while self.step().await != Next::Stop {}
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
            AgentCommand::Show(kind) => {
                self.emit(screen(kind));
                return Next::Continue;
            }
            AgentCommand::AskUsage => {
                self.emit(usage_limits());
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
        if let Some(name) = message.strip_prefix("env:") {
            let found = self
                .spec
                .env
                .0
                .iter()
                .find(|(known, _)| known == name.trim());
            self.text(
                found
                    .map_or("unset", |(_, value)| value.as_str())
                    .to_string(),
            );
            self.emit(done(false, None));
            return Next::Continue;
        }
        if message == "fail" {
            self.text("trying…");
            self.emit(done(true, Some("it went wrong")));
            return Next::Continue;
        }
        if let Some(what) = message.strip_prefix("background ") {
            self.background(what);
            return Next::Continue;
        }
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
            self.suggest(message);
        }
        next
    }

    /// Offers what follows `suggest:` as the user's next message.
    fn suggest(&self, message: &str) {
        if let Some(text) = message.strip_prefix("suggest:") {
            self.emit(AgentEvent::Suggestion {
                text: text.trim().to_string(),
            });
        }
    }

    fn echo(&self, message: &str) -> Next {
        self.text(format!("echo: {message}"));
        Next::Continue
    }

    /// Asks permission for a `Bash` call and reports what the decision led to.
    async fn run_tool(&mut self, shell: &str) -> Next {
        let input = json!({ "command": shell });
        let allowed = match self.ask("Bash", input).await {
            Ok(allowed) => allowed,
            Err(next) => return next,
        };
        self.text(if allowed { "ran it" } else { "not allowed" });
        Next::Continue
    }

    /// Presents a plan for what was asked and reports whether it was approved.
    async fn plan(&mut self, message: &str) -> Next {
        let plan = format!("1. Look into: {message}\n2. Do it.");
        let approved = match self.ask(PLAN_TOOL, json!({ "plan": plan })).await {
            Ok(approved) => approved,
            Err(next) => return next,
        };
        self.text(if approved {
            "carrying out the plan"
        } else {
            "still planning"
        });
        Next::Continue
    }

    /// Calls a tool after asking for permission, and says whether it was allowed. If it never
    /// was, what follows instead: the turn was stopped, which takes the request back as Claude
    /// Code does, or the front end went away.
    async fn ask(
        &mut self,
        tool: &str,
        input: serde_json::Value,
    ) -> std::result::Result<bool, Next> {
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
        let answer =
            |c: &AgentCommand| decides(c, &request_id) || matches!(c, AgentCommand::Interrupt);
        let decision = wait_for(&mut self.commands, answer)
            .await
            .ok_or(Next::Stop)?;
        if matches!(decision, AgentCommand::Interrupt) {
            self.emit(AgentEvent::PermissionWithdrawn { request_id });
            self.emit(done(true, Some("interrupted")));
            return Err(Next::Interrupted);
        }
        let allowed = !matches!(decision, AgentCommand::Deny { .. });
        let content = match decision {
            AgentCommand::Answer { answers, .. } => format!("answered: {answers:?}"),
            AgentCommand::Allow { .. } => "ok".to_string(),
            _ => "denied".to_string(),
        };
        self.emit(AgentEvent::ToolResult {
            tool_use_id,
            content,
            is_error: !allowed,
        });
        Ok(allowed)
    }
}

/// The end of a turn, as the scripted agent reports it.
fn done(is_error: bool, result: Option<&str>) -> AgentEvent {
    AgentEvent::TurnDone {
        is_error,
        result: result.map(String::from),
        cost_usd: Some(0.0),
        duration_ms: None,
    }
}

/// The usage the scripted agent reports when asked: one limit.
fn usage_limits() -> AgentEvent {
    AgentEvent::UsageLimits {
        limits: vec![UsageLimit {
            kind: "session".to_string(),
            model: None,
            percent: 36,
            resets_at: None,
            severity: "normal".to_string(),
        }],
    }
}

/// The screen the scripted agent has for every command: one row that names it.
fn screen(kind: ScreenKind) -> AgentEvent {
    AgentEvent::Screen {
        command: kind.command().to_string(),
        title: kind.title().to_string(),
        note: String::new(),
        sections: vec![ScreenSection {
            title: "Scripted".to_string(),
            rows: vec![ScreenRow {
                label: "Screen".to_string(),
                value: kind.command().to_string(),
                ..ScreenRow::default()
            }],
        }],
    }
}

/// Whether `command` answers the permission request `request_id`.
fn decides(command: &AgentCommand, request_id: &str) -> bool {
    match command {
        AgentCommand::Allow { request_id: id }
        | AgentCommand::Answer { request_id: id, .. }
        | AgentCommand::Deny { request_id: id, .. } => id == request_id,
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
