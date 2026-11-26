//! A scripted agent for tests and for working on a front end without spending tokens.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;
use tokio::sync::mpsc;

use super::{
    AgentBackend, AgentCommand, AgentEvent, AgentHandle, Auth, BackgroundState, PLAN_TOOL,
    RemoteState, ScreenKind, ScreenRow, ScreenSection, SessionSpec, Supervision, UsageLimit,
};
use crate::error::Result;
use crate::mcp::McpTransport;
use crate::returned::Taking;

/// The app `app` shows: a heading.
const FAKE_APP: &str = "<!doctype html><h1>Chart</h1>";

/// What `screenshot` returns: a PNG of two by two pixels.
const SCREENSHOT: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAE0lEQVR42mPQiDoBRAxA/OHDBwAlUgav0sDuFwAAAABJRU5ErkJggg==";

/// What an MCP server runs, or where it is reached: never what its variables hold.
fn run_by(server: &crate::mcp::McpServer) -> String {
    match &server.transport {
        McpTransport::Stdio { command, args, .. } => {
            let words: Vec<&str> = std::iter::once(command)
                .chain(args)
                .map(String::as_str)
                .collect();
            words.join(" ")
        }
        McpTransport::Http { url, .. } => url.clone(),
    }
}

/// A stand-in agent whose replies depend only on the user's message.
///
/// - plain text: replies `echo: <text>` and ends the turn;
/// - `run: <command>`: asks permission for a `Bash` tool call, then reports the outcome; a stop
///   meanwhile takes the request back and ends the turn;
/// - `env: <NAME>`: replies with the value of that variable as given for the session, or `unset`;
/// - `mcp: <name>`: replies with the command and arguments of the MCP server of that name it
///   was given, or `none`; never what the server's variables hold;
/// - `write: <path> = <text>`: writes `<text>` and a line end into the file at `<path>` in the
///   session's folder, says so and ends the turn;
/// - `slow`: starts a turn that only ends when interrupted;
/// - `fail`: ends the turn with an error;
/// - `suggest: <text>`: replies as to plain text, then offers `<text>` as the next message;
/// - `limit <percent>`: replies as to plain text, then reports, as Claude Code does after a
///   turn, that the 5-hour and weekly windows are that full;
/// - `refuse <seconds>`: ends the turn refused by the 5-hour limit, as Claude Code does, which
///   resets that many seconds from now (a negative number for one that has passed);
/// - `background agent`: starts a sub-agent in the background and ends the turn; the sub-agent
///   then writes, reports its end, and the agent takes a turn of its own to read its report;
///   `background agent left` leaves it at work, with no end;
/// - `background bash`: leaves a command running in the background and ends the turn;
/// - `foreground bash`, `foreground agent`: calls a tool whose command or sub-agent runs in the
///   foreground until it is sent to the background, then goes on as `background …` does; a
///   stop meanwhile ends the turn;
/// - `remote: <text>`: replies as to plain text; then, with Remote Control on, `<text>` comes
///   as if typed on claude.ai, and is answered in a turn of its own;
/// - `remote wavers`: replies as to plain text; then, with Remote Control on, reports the
///   connection as Claude Code does, without saying where the session is: on once more,
///   reconnecting, and on again;
/// - `resumed`: replies with where it was started to resume its conversation: the session id
///   and the point in it it was given, or `none`;
/// - `app`: calls an MCP tool whose server attached an app to it, which is read once the call
///   has its result; a tool called for the app between turns answers with its name and what
///   it was given;
/// - `screenshot`: calls a browser's screenshot tool, which returns a small PNG with a line of
///   text, as Playwright MCP's does, then replies and ends the turn;
/// - in plan mode, anything: presents a plan and waits for it to be approved.
///
/// Each answered turn ends, as Claude Code's do, with where its record of the conversation
/// stands: `fake-<run>-<turn>`.
///
/// Like Claude Code, it says each message back as it takes it in, and turns Remote Control on
/// unless it was given an API key, which Remote Control does not take.
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
            told: Vec::new(),
            remote: false,
            turns: 0,
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
    /// Messages written while it worked, taken in at its next step as Claude Code does.
    told: Vec<String>,
    spec: SessionSpec,
    number: u64,
    events: mpsc::UnboundedSender<AgentEvent>,
    commands: mpsc::UnboundedReceiver<AgentCommand>,
    requests: u64,
    initialised: bool,
    /// Whether Remote Control is on.
    remote: bool,
    /// How many turns it answered, which names where its conversation stands after each.
    turns: u64,
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
            moved: false,
            unstoppable: false,
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
            images: Vec::new(),
            returned: Vec::new(),
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

    /// Calls a tool whose work runs in the foreground, the call waiting on it, until it is sent
    /// to the background or the turn is stopped: a command (`bash`), which then runs on, or a
    /// sub-agent (`agent`), which then works on past the turn, reports its end, and is read in
    /// a turn of the agent's own. Claude Code tells nothing of such work until it moves.
    async fn foreground(&mut self, what: &str) -> Next {
        let call = format!("tool-fg-{what}");
        let agent = what == "agent";
        let (name, input) = if agent {
            let asked = json!({ "description": "look around", "prompt": "Look around." });
            ("Agent", asked)
        } else {
            (
                "Bash",
                json!({ "command": "sleep 600", "description": "Wait" }),
            )
        };
        self.emit(AgentEvent::ToolUse {
            id: call.clone(),
            name: name.to_string(),
            input,
        });
        if let Err(next) = self.until_sent_away(&call).await {
            return next;
        }
        let report = |state, summary: Option<&str>| AgentEvent::Background {
            task_id: format!("bg-fg-{what}"),
            description: format!("foreground {what}"),
            kind: if agent { "local_agent" } else { "local_bash" }.to_string(),
            state,
            summary: summary.map(str::to_string),
            tool_use_id: Some(call.clone()),
            output_file: (!agent).then(|| format!("/tmp/swarmed-fake-agent/bg-fg-{what}.output")),
            moved: true,
            unstoppable: false,
        };
        self.emit(report(BackgroundState::Running, None));
        self.emit(AgentEvent::ToolResult {
            tool_use_id: call.clone(),
            content: format!("Sent to the background with ID: bg-fg-{what}."),
            is_error: false,
            images: Vec::new(),
            returned: Vec::new(),
        });
        self.text("It goes on in the background.");
        self.emit(done(false, None));
        if agent {
            self.emit(report(BackgroundState::Completed, Some("found it")));
            self.text("It is back: found it.");
            self.emit(done(false, Some("It is back.")));
        }
        Next::Continue
    }

    /// Waits until the work of call `call` is sent to the background, answering each request
    /// to send work there as Claude Code does: one for another call does not move it. If it
    /// never is, what follows instead: the turn was stopped, or the front end went away.
    async fn until_sent_away(&mut self, call: &str) -> std::result::Result<(), Next> {
        let mut moved = false;
        while !moved {
            let command = wait_for(&mut self.commands, &mut self.told, sends_or_stops).await;
            let tool_use_id = self.sending(command)?;
            moved = tool_use_id.as_deref().is_none_or(|id| id == call);
            self.emit(sent_to_background(tool_use_id, moved));
        }
        Ok(())
    }

    /// The call whose work `command` sends to the background, or `None` for every call's; if
    /// it sends none, what follows: a stop ends the turn, and no command at all ends the agent.
    fn sending(&self, command: Option<AgentCommand>) -> std::result::Result<Option<String>, Next> {
        match command {
            Some(AgentCommand::SendToBackground { tool_use_id }) => Ok(tool_use_id),
            Some(_) => {
                self.emit(done(true, Some("interrupted")));
                Err(Next::Interrupted)
            }
            None => Err(Next::Stop),
        }
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
            AgentCommand::RemoteControl { on, reattach, .. } => {
                self.remote_control(on, reattach);
                return Next::Continue;
            }
            // Between turns no call waits on anything. Asked for every call, Claude Code takes
            // the request all the same.
            AgentCommand::SendToBackground { tool_use_id } => {
                let every = tool_use_id.is_none();
                self.emit(sent_to_background(tool_use_id, every));
                return Next::Continue;
            }
            // A tool an app calls answers with its own name and what it was given.
            AgentCommand::CallTool {
                call,
                tool,
                arguments,
                ..
            } => {
                self.emit(AgentEvent::AppCalled {
                    call,
                    result: Some(json!({
                        "content": [{ "type": "text", "text": format!("called {tool}") }],
                        "structuredContent": arguments,
                    })),
                    error: None,
                });
                return Next::Continue;
            }
            _ => return Next::Continue,
        };
        if !self.initialised {
            self.initialised = true;
            self.emit(self.init());
        }
        self.emit(AgentEvent::Heard {
            text: message.clone(),
        });
        let next = self.turn(&message).await;
        if next != Next::Continue || !self.remote {
            return next;
        }
        if message == "remote wavers" {
            self.wavers();
        }
        match message.strip_prefix("remote:") {
            Some(typed) => self.typed_on_claude_ai(typed.trim()),
            None => next,
        }
    }

    /// The connection to claude.ai reported as Claude Code reports it after it is turned on,
    /// by its state alone: on, which it was already, then lost and found again.
    fn wavers(&self) {
        let report = |state, detail: Option<&str>| AgentEvent::RemoteControl {
            state,
            url: None,
            remote_id: None,
            detail: detail.map(String::from),
        };
        self.emit(report(RemoteState::On, None));
        self.emit(report(RemoteState::Reconnecting, Some("network")));
        self.emit(report(RemoteState::On, None));
    }

    /// Turns Remote Control on, at the session on claude.ai it is to attach to again or at a
    /// new one, or off, which is answered with nothing as Claude Code answers it.
    fn remote_control(&mut self, on: bool, reattach: Option<String>) {
        self.remote = on && !matches!(self.spec.auth, Auth::ApiKey(_));
        if !on {
            return;
        }
        if !self.remote {
            self.emit(AgentEvent::RemoteControl {
                state: RemoteState::Failed,
                url: None,
                remote_id: None,
                detail: Some("Remote Control needs a claude.ai subscription".to_string()),
            });
            return;
        }
        let id = reattach.unwrap_or_else(|| format!("remote-{}", self.number));
        self.emit(AgentEvent::RemoteControl {
            state: RemoteState::On,
            url: Some(format!("https://claude.ai/code/{id}")),
            remote_id: Some(id),
            detail: None,
        });
    }

    /// A message typed on claude.ai, as Claude Code takes it in: said back, then answered.
    fn typed_on_claude_ai(&self, typed: &str) -> Next {
        self.emit(AgentEvent::Heard {
            text: typed.to_string(),
        });
        self.text(format!("echo: {typed}"));
        self.emit(done(false, None));
        Next::Continue
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
            capabilities: vec![],
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
        if let Some(name) = message.strip_prefix("mcp:") {
            let given = self
                .spec
                .mcp_servers
                .iter()
                .find(|server| server.name == name.trim());
            self.text(given.map_or_else(|| "none".to_string(), run_by));
            self.emit(done(false, None));
            return Next::Continue;
        }
        if let Some(what) = message.strip_prefix("write:") {
            self.write(what);
            return Next::Continue;
        }
        if let Some(seconds) = message.strip_prefix("refuse ") {
            self.refuse(seconds.trim().parse().unwrap_or(0));
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
        if let Some(what) = message.strip_prefix("foreground ") {
            return self.foreground(what).await;
        }
        if message == "resumed" {
            let at = |what: &Option<String>| what.clone().unwrap_or_else(|| "none".to_string());
            self.text(format!(
                "resumed {} at {}",
                at(&self.spec.resume),
                at(&self.spec.resume_at)
            ));
            self.end_answered();
            return Next::Continue;
        }
        if message == "screenshot" {
            self.screenshot();
            return Next::Continue;
        }
        if message == "app" {
            self.app();
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
            let interrupt = wait_for(&mut self.commands, &mut self.told, is_interrupt);
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
            self.end_answered();
            self.suggest(message);
            self.report_usage(message);
        }
        next
    }

    /// Calls a tool whose MCP server attached an app to it, as Claude Code reports one: the
    /// call, its result, and the app read for it, which may call the server's `chart` and
    /// `refresh` tools.
    fn app(&mut self) {
        let call = "tool-app".to_string();
        self.emit(AgentEvent::ToolUse {
            id: call.clone(),
            name: "mcp__demo__chart".to_string(),
            input: json!({ "bars": 3 }),
        });
        let text = "Drew 3 bars.";
        self.emit(AgentEvent::ToolResult {
            tool_use_id: call.clone(),
            content: text.to_string(),
            is_error: false,
            images: Vec::new(),
            returned: Vec::new(),
        });
        self.emit(AgentEvent::App {
            tool_use_id: call,
            server: "demo".to_string(),
            tool: "chart".to_string(),
            uri: "ui://demo/chart".to_string(),
            document: None,
            note: None,
            csp: crate::apps::AppCsp::default(),
            permissions: vec![],
            prefers_border: Some(true),
            callable: vec!["chart".to_string(), "refresh".to_string()],
            result: Some(json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": { "bars": [1, 2, 3] },
            })),
            html: Some(FAKE_APP.to_string()),
        });
        self.text("Here is the chart.");
        self.end_answered();
    }

    /// Takes a screenshot as a browser's tool does: a line of text and the image, in base64.
    fn screenshot(&mut self) {
        let call = "tool-shot".to_string();
        self.emit(AgentEvent::ToolUse {
            id: call.clone(),
            name: "mcp__playwright__browser_take_screenshot".to_string(),
            input: json!({}),
        });
        let mut taking = Taking::default();
        let image = taking.image("image/png", SCREENSHOT);
        self.emit(AgentEvent::ToolResult {
            tool_use_id: call,
            content: format!("Took the viewport screenshot\n{image}"),
            is_error: false,
            images: Vec::new(),
            returned: taking.images(),
        });
        self.text("The page looks right.");
        self.end_answered();
    }

    /// Reports the windows as full as `limit <percent>` says, the way Claude Code reports
    /// them after a turn.
    fn report_usage(&self, message: &str) {
        let percent = message.strip_prefix("limit ").map(str::trim);
        let Some(used) = percent.and_then(|percent| percent.parse::<f64>().ok()) else {
            return;
        };
        let window =
            json!({ "status": "allowed", "utilization": used / 100.0, "resetsAt": 2_000_000_000 });
        self.emit(AgentEvent::RateLimit {
            status: "allowed".to_string(),
            resets_at: Some(2_000_000_000),
            raw: json!({ "unifiedWindows": { "five_hour": window, "seven_day": window } }),
        });
    }

    /// Ends the turn refused by the 5-hour limit, which resets `seconds` from now, as Claude
    /// Code reports it: used up, and nothing paying past it.
    fn refuse(&self, seconds: i64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0));
        let resets_at = now + seconds;
        let window = json!({ "status": "rejected", "utilization": 1.0, "resetsAt": resets_at });
        self.emit(AgentEvent::RateLimit {
            status: "rejected".to_string(),
            resets_at: Some(resets_at),
            raw: json!({
                "status": "rejected",
                "rateLimitType": "five_hour",
                "resetsAt": resets_at,
                "unifiedWindows": { "five_hour": window },
            }),
        });
        self.text("You've hit your limit.");
        self.emit(done(true, Some("usage limit reached")));
    }

    /// Writes the file `write: <path> = <text>` names in the session's folder, and ends the
    /// turn, with an error if it could not.
    fn write(&mut self, what: &str) {
        let (path, text) = what.split_once('=').unwrap_or((what, ""));
        let file = self.spec.cwd.join(path.trim());
        let written = std::fs::write(&file, format!("{}\n", text.trim()));
        self.text(format!("wrote {}", path.trim()));
        if written.is_ok() {
            self.end_answered();
        } else {
            self.emit(done(true, None));
        }
    }

    /// Ends a turn it answered well, saying first where its conversation stands, as Claude
    /// Code does.
    fn end_answered(&mut self) {
        self.turns += 1;
        self.emit(AgentEvent::ResumePoint {
            uuid: format!("fake-{}-{}", self.number, self.turns),
        });
        self.emit(done(false, None));
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
        let decision = wait_for(&mut self.commands, &mut self.told, answer)
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
            images: Vec::new(),
            returned: Vec::new(),
        });
        // What the user wrote meanwhile is taken in now, said back as Claude Code says it.
        for text in std::mem::take(&mut self.told) {
            self.emit(AgentEvent::Heard { text: text.clone() });
            self.text(format!("heard: {text}"));
        }
        Ok(allowed)
    }
}

fn is_interrupt(command: &AgentCommand) -> bool {
    matches!(command, AgentCommand::Interrupt)
}

/// Whether `command` sends work to the background or stops the turn.
fn sends_or_stops(command: &AgentCommand) -> bool {
    matches!(
        command,
        AgentCommand::SendToBackground { .. } | AgentCommand::Interrupt
    )
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

/// The scripted agent's answer to sending work to the background.
fn sent_to_background(tool_use_id: Option<String>, moved: bool) -> AgentEvent {
    AgentEvent::SentToBackground {
        tool_use_id,
        moved,
        refused: None,
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
    told: &mut Vec<String>,
    wanted: impl Fn(&AgentCommand) -> bool,
) -> Option<AgentCommand> {
    loop {
        let command = commands.recv().await?;
        if wanted(&command) {
            return Some(command);
        }
        if let AgentCommand::UserMessage(text) = command {
            told.push(text);
        }
    }
}
