//! Drives any agent that speaks the Agent Client Protocol (ACP) over stdio.
//!
//! ACP is JSON-RPC 2.0, one message per line: <https://agentclientprotocol.com>. Only the part
//! of protocol version 1 a session needs is implemented here, and, as with the Claude backend,
//! parsing is loose: unknown updates are skipped and missing fields fall back to defaults.
//!
//! The agent keeps its own credentials and configuration. Swarmed passes it a working
//! directory and, optionally, a model and extra environment variables.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use self::children::Children;
use self::printed::Printed;
use self::shells::Shells;
use super::{
    AgentBackend, AgentCommand, AgentEvent, AgentHandle, Effort, McpServerInfo, SessionSpec,
    Supervision,
};
use crate::error::{Error, Result};
use crate::mcp::{McpServer, McpTransport};
use crate::returned::Taking;

mod children;
mod printed;
mod shells;

/// How much of the process's stderr is kept for [`AgentEvent::Exited`].
const STDERR_TAIL_CHARS: usize = 2000;
/// JSON-RPC's code for a method the receiver does not implement.
const METHOD_NOT_FOUND: i64 = -32601;
/// The notification OpenCode tells a client that asked for it about a sub-agent with.
const CHILD_UPDATE: &str = "opencode/session/child_update";

/// Runs sessions with an ACP agent, such as `opencode acp` or `goose acp`.
#[derive(Debug, Clone)]
pub struct AcpBackend {
    /// The agent's executable; a bare name is looked up on `PATH`.
    pub program: PathBuf,
    /// The arguments that put it into ACP mode.
    pub args: Vec<String>,
    /// Extra environment variables for the agent, on top of Swarmed's own.
    pub env: Vec<(String, String)>,
    /// What a session that leaves the model to the agent runs on, for an agent whose own
    /// default is not one to go by: `None` leaves it to the agent.
    pub default_model: Option<String>,
}

fn missing_pipe() -> Error {
    Error::Io(std::io::Error::other("the agent's pipes are missing"))
}

impl AcpBackend {
    /// Why the agent's program could not be started, said so that the user can act on it: a
    /// bare "No such file or directory" does not say which file, or what to do.
    fn not_started(&self, error: std::io::Error) -> Error {
        if error.kind() == std::io::ErrorKind::NotFound {
            return Error::AgentMissing(self.program.to_string_lossy().into_owned());
        }
        error.into()
    }
}

impl AgentBackend for AcpBackend {
    fn start(&self, spec: SessionSpec) -> Result<AgentHandle> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .env("PATH", crate::shellenv::path())
            .envs(spec.env.0.iter().map(|(name, value)| (name, value)))
            // What the user wrote for this agent in agents.toml wins over a stored key.
            .envs(self.env.iter().map(|(name, value)| (name, value)))
            .current_dir(&spec.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| self.not_started(error))?;
        let pid = child.id();
        let stdin = child.stdin.take().ok_or_else(missing_pipe)?;
        let stdout = child.stdout.take().ok_or_else(missing_pipe)?;
        let stderr = child.stderr.take().ok_or_else(missing_pipe)?;

        let (event_tx, events) = mpsc::unbounded_channel();
        let (commands, command_rx) = mpsc::unbounded_channel();
        let session = Conversation::new(spec, self.default_model.clone(), stdin, event_tx);
        // The task holds the process, which is killed when the task goes.
        let driving = tokio::spawn(drive(session, child, stdout, stderr, command_rx));
        Ok(AgentHandle {
            events,
            commands,
            pid,
            tasks: vec![driving.abort_handle()],
        })
    }
}

/// The requests Swarmed makes, remembered by id until the agent answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Initialize,
    OpenSession,
    /// Its answer is not needed: an agent without that model or mode keeps its own.
    SetOption,
    /// Putting an earlier conversation back on its own model; see `restoring`.
    RestoreModel,
    Prompt,
}

/// The agent's own mode that comes closest to `supervision`, among the ones it `offered`.
/// The names are the ones agents use for these three: asking before anything is changed,
/// deciding for itself what needs a person, and never asking.
fn agent_mode(supervision: Supervision, offered: &[String]) -> Option<&str> {
    let names: &[&str] = match supervision {
        Supervision::Ask => &["read-only", "ask", "default"],
        // A planning mode of its own if it has one; else it asks, which keeps it from acting.
        Supervision::Plan => &["plan", "read-only", "ask", "default"],
        Supervision::Auto => &["agent", "auto"],
        Supervision::Bypass => &["agent-full-access", "full-access"],
    };
    // The first of the names it has, in the order above: the closest match first.
    let has = |name: &&&str| offered.iter().any(|mode| mode == **name);
    let found = names.iter().find(has)?;
    offered
        .iter()
        .find(|mode| mode == found)
        .map(String::as_str)
}

/// The efforts from least to most, as agents name them. Some offer less than these, and
/// some a step below the lowest Swarmed has.
const EFFORTS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];

/// The agent's own effort that comes closest to `effort`, among the ones it `offered`: the
/// same one if it has it, else the nearest below, else the nearest above.
fn agent_effort(effort: Effort, offered: &[String]) -> Option<&str> {
    let rank = |name: &str| EFFORTS.iter().position(|known| *known == name);
    let wanted = rank(effort.as_str())?;
    let known = || offered.iter().filter_map(|name| Some((rank(name)?, name)));
    let below = known()
        .filter(|(at, _)| *at <= wanted)
        .max_by_key(|(at, _)| *at);
    let above = known()
        .filter(|(at, _)| *at > wanted)
        .min_by_key(|(at, _)| *at);
    below.or(above).map(|(_, name)| name.as_str())
}

/// The option Codex offers for quicker answers at a higher price, and the value that turns it on.
const FAST_MODE: &str = "fast-mode";
const FAST_ON: &str = "on";

/// What the agent's fast mode is set to: on, if the session asks for it and the agent has
/// such a mode; otherwise nothing is set and the agent stays as it is.
fn fast_value(wanted: bool, offered: &[String]) -> Option<&'static str> {
    (wanted && offered.iter().any(|value| value == FAST_ON)).then_some(FAST_ON)
}

/// The values the agent offers for its option `id`, from what it answers to opening a session.
fn option_values(opened: &Value, id: &str) -> Vec<String> {
    let options = opened.get("configOptions").and_then(Value::as_array);
    let named = |option: &&Value| str_of(option, "id") == Some(id);
    let option = options.and_then(|options| options.iter().find(named));
    let values = option.and_then(|option| option.get("options")?.as_array());
    let value = |choice: &Value| str_of(choice, "value").map(String::from);
    values.map_or_else(Vec::new, |values| values.iter().filter_map(value).collect())
}

/// A permission request the agent is waiting on.
#[derive(Debug)]
struct PendingPermission {
    /// The JSON-RPC id the answer has to carry.
    id: Value,
    allow: Option<String>,
    reject: Option<String>,
}

/// What one speaker in a conversation has said and done and not yet passed on: the agent
/// itself, or one of its sub-agents.
#[derive(Debug, Default)]
struct Voice {
    /// The reply so far; ACP streams it in pieces and Swarmed shows whole blocks.
    text: String,
    /// The last whole reply passed on: what a sub-agent hands back when it is done.
    said: String,
    /// Tool calls announced without their arguments yet, by id; some agents send those later.
    unannounced: HashMap<String, Value>,
    /// The arguments of the calls shown and not yet ended, by id.
    open: HashMap<String, Value>,
}

impl Voice {
    /// The reply so far, which is passed on whole, if there is any.
    fn take_text(&mut self) -> Option<String> {
        if self.text.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.text);
        self.said.clone_from(&text);
        Some(text)
    }
}

/// One ACP conversation: what has been asked, what is waiting, and what was heard so far.
struct Conversation {
    spec: SessionSpec,
    /// `None` once Swarmed has hung up, which is what tells the agent to exit.
    stdin: Option<ChildStdin>,
    events: mpsc::UnboundedSender<AgentEvent>,
    next_id: u64,
    calls: HashMap<u64, Call>,
    /// The agent's id for the conversation, known once it is open.
    session_id: Option<String>,
    /// Messages sent before the conversation was open.
    queued: VecDeque<String>,
    /// What the agent itself has said and done and not yet passed on.
    own: Voice,
    /// The sub-agents the agent started, and what they have said and done.
    children: Children,
    /// What the commands the agent runs have printed, for an agent that says so apart.
    printed: Printed,
    /// The shells the agent left running, for an agent that reports them.
    shells: Shells,
    cost: Option<f64>,
    /// Tokens used and the window's size, as last reported and not yet passed on.
    context: Option<(u64, u64)>,
    permissions: HashMap<String, PendingPermission>,
    /// The MCP servers given to the agent, by name, in ACP's shape.
    passed: Vec<(String, Value)>,
    /// While an old conversation is loaded the agent replays it; Swarmed has it stored already.
    replaying: bool,
    /// While an earlier conversation, opened on the agent's default model, is put back on its
    /// own. What the agent says in that time is about the model it opened with, which is
    /// already no longer so, and messages wait so that they run on the right one.
    restoring: bool,
    /// The agent is at work without having been asked just now: something it left running
    /// ended, or a timer of its own went off, and it carries on after its turn was over.
    /// Nothing in the protocol says when it is done with that; see `end_own_turn`.
    woken: bool,
    /// Set when the agent could not be set up; the reason is what the user is shown.
    failure: Option<String>,
    /// The modes of its own the agent offers, by id. Empty if it has none.
    modes: Vec<String>,
    /// See [`AcpBackend::default_model`].
    default_model: Option<String>,
}

impl Conversation {
    fn new(
        mut spec: SessionSpec,
        default_model: Option<String>,
        stdin: ChildStdin,
        events: mpsc::UnboundedSender<AgentEvent>,
    ) -> Self {
        spec.model = spec.model.or_else(|| default_model.clone());
        Self {
            spec,
            stdin: Some(stdin),
            events,
            next_id: 0,
            calls: HashMap::new(),
            session_id: None,
            queued: VecDeque::new(),
            own: Voice::default(),
            children: Children::default(),
            printed: Printed::default(),
            shells: Shells::default(),
            cost: None,
            context: None,
            permissions: HashMap::new(),
            passed: Vec::new(),
            replaying: false,
            restoring: false,
            woken: false,
            failure: None,
            modes: vec![],
            default_model,
        }
    }

    fn emit(&self, event: AgentEvent) {
        self.events.send(event).ok();
    }

    fn emit_all(&self, events: Vec<AgentEvent>) {
        for event in events {
            self.emit(event);
        }
    }

    async fn write(&mut self, message: &Value) {
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let mut bytes = message.to_string().into_bytes();
        bytes.push(b'\n');
        // A failed write means the agent is gone; its exit is reported when stdout closes.
        if stdin.write_all(&bytes).await.is_ok() {
            stdin.flush().await.ok();
        }
    }

    async fn call(&mut self, call: Call, method: &str, params: Value) {
        self.next_id += 1;
        self.calls.insert(self.next_id, call);
        let request =
            json!({ "jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params });
        self.write(&request).await;
    }

    async fn initialize(&mut self) {
        let params = json!({
            "protocolVersion": 1,
            // Swarmed offers no file or terminal services; the agent uses its own tools. It
            // follows sub-agents, as the protocol has them and as OpenCode tells of them. Codex
            // reports work left running only to a client that declares JetBrains' AIR, which
            // changes the shape of all it sends besides, so that is not declared.
            "clientCapabilities": {
                "fs": { "readTextFile": false, "writeTextFile": false },
                "terminal": false,
                "subagents": {},
                "_meta": { "opencode/child-session-updates": true },
            },
        });
        self.call(Call::Initialize, "initialize", params).await;
    }

    /// Opens the conversation: a new one, or the earlier one by whichever way the agent offers.
    async fn open_session(&mut self, capabilities: &Value) {
        let cwd = self.spec.cwd.to_string_lossy().into_owned();
        let takes_http = capabilities
            .pointer("/mcpCapabilities/http")
            .and_then(Value::as_bool);
        self.passed = acp_servers(&self.spec.mcp_servers, takes_http.unwrap_or(false));
        let servers: Vec<&Value> = self
            .passed
            .iter()
            .map(|(_, definition)| definition)
            .collect();
        let Some(earlier) = self.spec.resume.clone() else {
            let params = json!({ "cwd": cwd, "mcpServers": servers });
            return self.call(Call::OpenSession, "session/new", params).await;
        };
        let params = json!({ "sessionId": earlier, "cwd": cwd, "mcpServers": servers });
        let can_resume = capabilities
            .pointer("/sessionCapabilities/resume")
            .is_some();
        let can_load = capabilities
            .get("loadSession")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.session_id = Some(earlier);
        if can_resume {
            self.call(Call::OpenSession, "session/resume", params).await;
        } else if can_load {
            self.replaying = true;
            self.call(Call::OpenSession, "session/load", params).await;
        } else {
            self.failure = Some("this agent cannot continue an earlier conversation".to_string());
        }
    }

    async fn on_session_open(&mut self, result: &Value) {
        self.replaying = false;
        self.modes = option_values(result, "mode");
        if self.cannot_be_held_back() {
            // Nothing has been sent to it yet, and nothing is: the conversation ends here.
            self.failure = Some(
                "this agent has no mode that keeps it from acting, so it is not asked".to_string(),
            );
            return;
        }
        if let Some(id) = str_of(result, "sessionId") {
            self.session_id = Some(id.to_string());
        }
        let session_id = self.session_id.clone().unwrap_or_default();
        self.emit(AgentEvent::Init {
            agent_session_id: session_id.clone(),
            model: self
                .spec
                .model
                .clone()
                .unwrap_or_else(|| current_model(result)),
            permission_mode: self.spec.supervision.as_str().to_string(),
            api_key_source: "agent".to_string(),
            mcp_servers: self.passed_servers(),
            skills: self.spec.skills,
            slash_commands: vec![],
        });
        if let Some(applied) = self.applied(result) {
            self.emit(applied);
        }
        self.set_mode().await;
        self.set_effort(result).await;
        self.set_fast(&option_values(result, FAST_MODE)).await;
        if self.opened_on_another_model(result) {
            self.restore_model().await;
        } else {
            self.set_model(self.spec.model.clone()).await;
        }
        self.send_queued().await;
    }

    /// Whether an earlier conversation was opened on a model other than the session's own.
    fn opened_on_another_model(&self, result: &Value) -> bool {
        let wanted = self.spec.model.as_deref();
        self.spec.resume.is_some() && wanted.is_some_and(|model| model != current_model(result))
    }

    /// Puts an earlier conversation back on the session's model, holding everything else
    /// until the agent has answered.
    async fn restore_model(&mut self) {
        let (Some(model), Some(session_id)) = (self.spec.model.clone(), self.session_id.clone())
        else {
            return;
        };
        self.restoring = true;
        let params = json!({ "sessionId": session_id, "configId": "model", "value": model });
        self.call(Call::RestoreModel, "session/set_config_option", params)
            .await;
    }

    async fn on_model_restored(&mut self) {
        self.restoring = false;
        self.own.text.clear();
        self.send_queued().await;
    }

    async fn send_queued(&mut self) {
        if self.restoring {
            return;
        }
        while let Some(text) = self.queued.pop_front() {
            self.prompt(text).await;
        }
    }

    /// What the conversation runs with once it is set up: the session's model and effort where
    /// it chose them, else what the agent opened with. Nothing for an agent that has no effort.
    fn applied(&self, opened: &Value) -> Option<AgentEvent> {
        let option = effort_option(opened)?;
        let offered = option_values(opened, option);
        let chosen = self
            .spec
            .effort
            .and_then(|effort| agent_effort(effort, &offered));
        let effort = chosen.or_else(|| current_value(opened, option))?;
        let model = self.spec.model.clone();
        Some(AgentEvent::Applied {
            model: model.unwrap_or_else(|| current_model(opened)),
            effort: Some(effort.to_string()),
        })
    }

    /// Switches the open session to `model`. The protocol has no way back to the agent's own
    /// default, so `None` only applies when the session is next started, unless Swarmed has a
    /// default of its own for the agent, which it switches to.
    async fn set_model(&mut self, model: Option<String>) {
        let model = model.or_else(|| self.default_model.clone());
        self.spec.model.clone_from(&model);
        let (Some(model), Some(session_id)) = (model, self.session_id.clone()) else {
            return;
        };
        let params = json!({ "sessionId": session_id, "configId": "model", "value": model });
        self.call(Call::SetOption, "session/set_config_option", params)
            .await;
    }

    /// Sets how hard the agent's model thinks, to the level of its own nearest the one chosen
    /// for the session. An agent that offers no such choice, or a session without one, is left
    /// at the agent's default.
    async fn set_effort(&mut self, opened: &Value) {
        let Some(option) = effort_option(opened) else {
            return;
        };
        let offered = option_values(opened, option);
        let effort = self.spec.effort;
        let effort = effort.and_then(|effort| agent_effort(effort, &offered));
        let (Some(effort), Some(session_id)) = (effort, self.session_id.clone()) else {
            return;
        };
        let params = json!({ "sessionId": session_id, "configId": option, "value": effort });
        self.call(Call::SetOption, "session/set_config_option", params)
            .await;
    }

    /// Turns the agent's fast mode on, if the session asks for it and the agent offers one.
    /// A session that does not ask leaves the agent as its own configuration has it.
    async fn set_fast(&mut self, offered: &[String]) {
        let value = fast_value(self.spec.fast, offered);
        let (Some(value), Some(session_id)) = (value, self.session_id.clone()) else {
            return;
        };
        let params = json!({ "sessionId": session_id, "configId": FAST_MODE, "value": value });
        self.call(Call::SetOption, "session/set_config_option", params)
            .await;
    }

    /// Puts the agent into the mode of its own that matches the session's supervision, if it
    /// has modes and one of them matches. An agent without modes is supervised from here alone.
    async fn set_mode(&mut self) {
        let mode = agent_mode(self.spec.supervision, &self.modes).map(String::from);
        let (Some(mode), Some(session_id)) = (mode, self.session_id.clone()) else {
            return;
        };
        let params = json!({ "sessionId": session_id, "configId": "mode", "value": mode });
        self.call(Call::SetOption, "session/set_config_option", params)
            .await;
    }

    /// Whether the agent is wanted for words only and would act without asking all the same:
    /// it has modes of its own and none of them is one that plans, reads or asks. Swarmed
    /// refuses what such an agent asks for, but in the mode it is in it may never ask.
    fn cannot_be_held_back(&self) -> bool {
        let held = agent_mode(self.spec.supervision, &self.modes).is_some();
        self.spec.words_only && !self.modes.is_empty() && !held
    }

    /// Whether what the agent asks permission for is allowed without showing it to the user.
    /// An agent with modes of its own has already let through what it finds safe, so in auto
    /// mode what still reaches Swarmed is what it wants a person to see.
    fn allows_unasked(&self) -> bool {
        match self.spec.supervision {
            Supervision::Bypass => true,
            Supervision::Auto => agent_mode(Supervision::Auto, &self.modes).is_none(),
            Supervision::Ask | Supervision::Plan => false,
        }
    }

    async fn prompt(&mut self, text: String) {
        let Some(session_id) = self.session_id.clone().filter(|_| !self.replaying) else {
            return self.queued.push_back(text);
        };
        let opening = |call: &Call| matches!(call, Call::OpenSession | Call::RestoreModel);
        if self.restoring || self.calls.values().any(opening) {
            return self.queued.push_back(text);
        }
        // Whatever it was doing by itself is part of the turn that starts now.
        self.woken = false;
        let mut prompt = vec![json!({ "type": "text", "text": text })];
        // The protocol has nowhere to put instructions of the client's own, so what Claude Code
        // is given as part of its system prompt goes along with the first message instead:
        // after the user's own words, once for each time the agent is started.
        if let Some(guidance) = self.spec.append_system_prompt.take() {
            prompt.push(json!({ "type": "text", "text": guidance_block(&guidance) }));
        }
        let params = json!({ "sessionId": session_id, "prompt": prompt });
        self.call(Call::Prompt, "session/prompt", params).await;
    }

    /// What the session reports about its MCP servers. Whether the agent connected to them is
    /// not something ACP tells the client, so they are reported as passed, not connected.
    fn passed_servers(&self) -> Vec<McpServerInfo> {
        let info = |(name, _): &(String, Value)| McpServerInfo {
            name: name.clone(),
            status: "passed".into(),
        };
        self.passed.iter().map(info).collect()
    }

    /// The voice of the agent itself, for `None`, or of its sub-agent by that session id.
    fn voice(&mut self, child: Option<&str>) -> Option<&mut Voice> {
        match child {
            None => Some(&mut self.own),
            Some(child) => self.children.voice(child),
        }
    }

    /// Passes on what the agent itself did, for `None`, or what its sub-agent by that session
    /// id did, filed under the call that sub-agent is shown by.
    fn emit_by(&self, child: Option<&str>, event: AgentEvent) {
        let Some(child) = child else {
            return self.emit(event);
        };
        if let Some(parent) = self.children.call_of(child) {
            self.emit(AgentEvent::Sub {
                parent: parent.to_string(),
                event: Box::new(event),
            });
        }
    }

    /// Passes on what the agent itself, for `None`, or its sub-agent by that session id has
    /// said since it last moved on.
    fn flush_text(&mut self, child: Option<&str>) {
        let said = match child {
            None => self.own.take_text().map(|text| AgentEvent::Text { text }),
            Some(child) => self.children.flush(child),
        };
        if let Some(said) = said {
            self.emit(said);
        }
    }

    /// Passes on what follows from something a sub-agent did, such as the start or end of
    /// its work. Outside a turn the agent is then at work by itself, as far as anyone can
    /// tell: that ends when it goes quiet, so that the session does not stay busy.
    fn emit_for_children(&mut self, events: Vec<AgentEvent>) {
        if events.is_empty() {
            return;
        }
        if self.session_id.is_some() && !self.in_turn() {
            self.woken = true;
        }
        self.emit_all(events);
    }

    /// The sub-agent a request or update on the agent's stream is from, by its session id:
    /// the session it names, if that is not the agent's own, else the one OpenCode marks it as
    /// coming from. OpenCode's is followed from then on.
    fn child_of(&mut self, params: &Value, marked_in: &Value) -> Option<String> {
        let session = str_of(params, "sessionId");
        let own = self.session_id.as_deref();
        if let Some(session) = session.filter(|session| own.is_some_and(|own| own != *session)) {
            return Some(session.to_string());
        }
        let (child, title) = children::marked(marked_in)?;
        let started = self.children.follow(&child, Some(&title));
        self.emit_for_children(started);
        Some(child)
    }

    async fn on_response(&mut self, id: u64, message: &Value) {
        let Some(call) = self.calls.remove(&id) else {
            return;
        };
        let error = message.get("error").map(error_text);
        let result = message.get("result").unwrap_or(&Value::Null);
        match (call, error) {
            (Call::SetOption, _) => {}
            (Call::RestoreModel, _) => self.on_model_restored().await,
            (Call::Prompt, error) => self.on_turn_end(result, error),
            (Call::Initialize | Call::OpenSession, Some(error)) => self.failure = Some(error),
            (Call::Initialize, None) => {
                let capabilities = result
                    .get("agentCapabilities")
                    .cloned()
                    .unwrap_or(Value::Null);
                self.open_session(&capabilities).await;
            }
            (Call::OpenSession, None) => self.on_session_open(result).await,
        }
    }

    fn on_turn_end(&mut self, result: &Value, error: Option<String>) {
        let finished = error.is_none() && str_of(result, "stopReason") == Some("end_turn");
        self.finish_turn(!finished, error);
    }

    /// Reports a turn as over: what was said in it whole, how full the window is, and how it
    /// ended.
    fn finish_turn(&mut self, is_error: bool, error: Option<String>) {
        self.flush_text(None);
        let said = self.children.flush_all();
        self.emit_all(said);
        if let Some(error) = &error {
            self.emit(AgentEvent::Text {
                text: format!("The agent reported an error: {error}"),
            });
        }
        if let Some((used, max)) = self.context.take() {
            self.emit(AgentEvent::ContextUsage {
                used,
                max,
                categories: vec![],
            });
        }
        self.emit(AgentEvent::TurnDone {
            is_error,
            result: error,
            cost_usd: self.cost,
            duration_ms: None,
        });
    }

    /// The agent asks for something. Only permission requests are answered with a decision.
    async fn on_request(&mut self, id: &Value, method: &str, params: &Value) {
        if method != "session/request_permission" {
            let error = json!({ "code": METHOD_NOT_FOUND, "message": "not supported by Swarmed" });
            return self
                .write(&json!({ "jsonrpc": "2.0", "id": id, "error": error }))
                .await;
        }
        let pending = pending_permission(id, params);
        // Plan mode is Claude Code's own; here it asks, which is the closest there is.
        if self.allows_unasked() {
            return self.answer(&pending, true).await;
        }
        // A sub-agent's request is answered where its starter's are, naming the sub-agent.
        let tool = params.get("toolCall").unwrap_or(&Value::Null);
        let child = self.child_of(params, tool);
        let child = child.as_deref();
        if let Some(child) = child.filter(|child| self.children.knows(child)) {
            // What it asks is something it does: it is shown at work if it was not yet.
            let started = self.children.follow(child, None);
            self.emit_for_children(started);
        }
        let description = child.and_then(|child| self.children.asked_by(child));
        if child.is_some() && !self.in_turn() {
            // Nothing else would end the work it makes the session do after its turn.
            self.woken = true;
        }
        let titled = child.and_then(|child| self.children.title(child));
        let tool = &titled.map_or_else(|| tool.clone(), |title| children::untitled(tool, &title));
        let tool_id = str_of(tool, "toolCallId").unwrap_or_default().to_string();
        let voice = self.voice(child);
        let known = voice.and_then(|voice| voice.unannounced.get_mut(&tool_id));
        // Named from all that is known of the call: its announcement may say what kind of
        // call it is where the request does not.
        let tool_name = known.map_or_else(
            || tool_name(tool).to_string(),
            |known| {
                merge_tool(known, tool);
                tool_name(known).to_string()
            },
        );
        self.announce_tool(&tool_id, child);
        self.flush_text(None);
        self.flush_text(child);
        let request_id = id.to_string();
        self.emit(AgentEvent::PermissionRequest {
            request_id: request_id.clone(),
            tool_name,
            input: tool.get("rawInput").cloned().unwrap_or_else(|| json!({})),
            description,
        });
        self.permissions.insert(request_id, pending);
    }

    async fn answer(&mut self, pending: &PendingPermission, allow: bool) {
        let option = if allow {
            &pending.allow
        } else {
            &pending.reject
        };
        let outcome = option.as_ref().map_or_else(
            || json!({ "outcome": "cancelled" }),
            |option| json!({ "outcome": "selected", "optionId": option }),
        );
        let response =
            json!({ "jsonrpc": "2.0", "id": pending.id, "result": { "outcome": outcome } });
        self.write(&response).await;
    }

    async fn decide(&mut self, request_id: &str, allow: bool) {
        if let Some(pending) = self.permissions.remove(request_id) {
            self.answer(&pending, allow).await;
        }
    }

    /// Stops the turn. The protocol wants every waiting permission request answered as cancelled.
    async fn cancel(&mut self) {
        let waiting: Vec<PendingPermission> = self.permissions.drain().map(|(_, p)| p).collect();
        for pending in waiting {
            let cancelled = json!({ "outcome": { "outcome": "cancelled" } });
            self.write(&json!({ "jsonrpc": "2.0", "id": pending.id, "result": cancelled }))
                .await;
        }
        let Some(session_id) = self.session_id.clone() else {
            return;
        };
        let params = json!({ "sessionId": session_id });
        self.write(&json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": params }))
            .await;
        // With no turn of the user's running, the agent has nothing to answer the
        // cancellation with: the stop is reported here, or the session would work forever.
        if !self.in_turn() {
            self.end_own_turn(true);
        }
    }

    /// Whether a message of the user's is being answered.
    fn in_turn(&self) -> bool {
        self.calls.values().any(|call| *call == Call::Prompt)
    }

    /// Ends what the agent did by itself, the way a turn ends: its words whole, then that it
    /// is done, so that the session stops counting as working.
    fn end_own_turn(&mut self, stopped: bool) {
        self.woken = false;
        self.finish_turn(stopped, None);
    }

    /// Takes note of something the agent did after its turn was over. It is at work again
    /// from its first word or tool, and done once it has said something and reports its
    /// usage, which is what agents send last. One that says nothing is done when it has been
    /// quiet for a while; see `QUIET`.
    fn on_own_activity(&mut self, kind: &str) {
        let open = self.session_id.is_some() && !self.restoring;
        if !open || self.in_turn() {
            return;
        }
        match kind {
            "agent_message_chunk" | "tool_call" | "tool_call_update" if !self.woken => {
                self.woken = true;
                // What wakes it is something it left running that ended, and a shell does
                // not say which it was: every one is taken to have ended.
                let ended = self.shells.ended();
                self.emit_all(ended);
            }
            "usage_update" if self.woken && !self.own.text.is_empty() => self.end_own_turn(false),
            _ => {}
        }
    }

    /// A piece of the reply: shown at once, and kept to be sent whole when the agent moves on.
    /// A sub-agent's is only kept: a piece filed under it would be stored.
    fn on_chunk(&mut self, update: &Value, child: Option<&str>) {
        if self.restoring {
            return;
        }
        let chunk = update.pointer("/content/text").and_then(Value::as_str);
        let chunk = chunk.unwrap_or_default();
        let Some(voice) = self.voice(child) else {
            return;
        };
        voice.text.push_str(chunk);
        if child.is_none() && !chunk.is_empty() {
            self.emit(AgentEvent::TextDelta {
                text: chunk.to_string(),
            });
        }
    }

    /// What the turn has cost and how full the context window is, reported when the turn ends.
    fn on_usage(&mut self, update: &Value) {
        self.cost = update
            .pointer("/cost/amount")
            .and_then(Value::as_f64)
            .or(self.cost);
        let tokens = |key: &str| update.get(key).and_then(Value::as_u64);
        self.context = tokens("used").zip(tokens("size")).or(self.context);
    }

    /// One `session/update` notification: a piece of what the agent is doing, or of what one
    /// of its sub-agents is doing, which is kept apart.
    fn on_update(&mut self, params: &Value) {
        let update = params.get("update").unwrap_or(&Value::Null);
        if let Some(child) = self.child_of(params, update) {
            return self.on_child_update(&child, update);
        }
        let kind = str_of(update, "sessionUpdate").unwrap_or_default();
        match kind {
            "agent_message_chunk" => self.on_chunk(update, None),
            "tool_call" | "tool_call_update" => self.on_tool(update, None),
            "usage_update" => self.on_usage(update),
            "subagent_update" | "session_message" => self.on_children(update),
            // Thoughts, plans, mode and command lists are not shown yet.
            _ => {}
        }
        self.on_own_activity(kind);
    }

    /// What the agent, or one of its sub-agents, says of a sub-agent: that it is there and
    /// how its work stands, or what it was sent.
    fn on_children(&mut self, update: &Value) {
        let events = if str_of(update, "sessionUpdate") == Some("subagent_update") {
            self.children.update(update)
        } else {
            self.children.message(update)
        };
        self.emit_for_children(events);
    }

    /// One update of sub-agent `child`'s own: kept apart under the call it is shown by, and
    /// never taken for the agent's own. What it uses is not the agent's own usage either.
    fn on_child_update(&mut self, child: &str, update: &Value) {
        if !self.children.knows(child) {
            // A session nobody announced is no sub-agent this conversation knows of.
            return;
        }
        let started = self.children.follow(child, None);
        self.emit_for_children(started);
        let title = self.children.title(child).unwrap_or_default();
        let update = children::untitled(update, &title);
        let child = Some(child);
        match str_of(&update, "sessionUpdate").unwrap_or_default() {
            "agent_message_chunk" => self.on_chunk(&update, child),
            "tool_call" | "tool_call_update" => self.on_tool(&update, child),
            // A sub-agent of its own, or what it sent one, as Codex tells it.
            "subagent_update" => self.on_children(&update),
            "session_message" if str_of(&update, "recipientSessionId") != child => {
                self.on_children(&update);
            }
            _ => {}
        }
    }

    /// What OpenCode tells of a sub-agent, to a client that asked for it: how its work stands,
    /// or one of its own updates.
    fn on_child_notice(&mut self, params: &Value) {
        let Some(child) = str_of(params, "childSessionId") else {
            return;
        };
        let title = str_of(params, "title").unwrap_or_default();
        if str_of(params, "type") == Some("status") {
            let events = self.children.status(child, title, params);
            return self.emit_for_children(events);
        }
        let started = self.children.follow_noticed(child, title);
        self.emit_for_children(started);
        if let Some(update) = params.get("update") {
            self.on_child_update(child, update);
        }
    }

    /// A tool call or a change to one. The call is shown once its arguments are known, or when
    /// it ends or needs permission, whichever comes first.
    fn on_tool(&mut self, update: &Value, child: Option<&str>) {
        let id = str_of(update, "toolCallId").unwrap_or_default().to_string();
        let status = str_of(update, "status").unwrap_or_default();
        let ended = status == "completed" || status == "failed";
        self.printed.heard(&id, update);
        let Some(voice) = self.voice(child) else {
            return;
        };
        if str_of(update, "sessionUpdate") == Some("tool_call") {
            voice.unannounced.insert(id.clone(), update.clone());
        }
        if let Some(known) = voice.unannounced.get_mut(&id) {
            merge_tool(known, update);
        }
        let has_input = voice.unannounced.get(&id).is_some_and(has_arguments);
        if ended || has_input {
            self.announce_tool(&id, child);
        }
        if ended {
            self.end_tool(&id, update, child);
        }
    }

    /// Passes on how a tool call ended, and what it left running: a sub-agent it started, or
    /// a shell it put in the background.
    fn end_tool(&mut self, id: &str, update: &Value, child: Option<&str>) {
        let printed = self.printed.take(id);
        let input = self.voice(child).and_then(|voice| voice.open.remove(id));
        let mut taking = Taking::default();
        let content = tool_output(update, printed, &mut taking);
        let result = AgentEvent::ToolResult {
            tool_use_id: id.to_string(),
            content,
            is_error: str_of(update, "status") == Some("failed"),
            images: Vec::new(),
            returned: taking.images(),
        };
        self.emit_by(child, result);
        if child.is_none() {
            self.children.answered(id, update);
        }
        if let Some(shell) = self.shells.left_running(id, update, input.as_ref()) {
            self.emit(shell);
        }
    }

    /// Shows a tool call that has not been shown yet. One of the agent's own that starts a
    /// sub-agent is shown by the name front ends know such a call by.
    fn announce_tool(&mut self, id: &str, child: Option<&str>) {
        let Some(voice) = self.voice(child) else {
            return;
        };
        let Some(known) = voice.unannounced.remove(id) else {
            return;
        };
        let input = known.get("rawInput").cloned().unwrap_or_else(|| json!({}));
        voice.open.insert(id.to_string(), input.clone());
        let shown = if child.is_none() && children::starts_one(&known) {
            let input = children::starting_input(&input);
            self.children.starting(id, &input);
            AgentEvent::ToolUse {
                id: id.to_string(),
                name: children::STARTING_TOOL.to_string(),
                input,
            }
        } else {
            tool_use(&known)
        };
        self.flush_text(child);
        self.emit_by(child, shown);
    }

    async fn on_command(&mut self, command: AgentCommand) {
        match command {
            AgentCommand::UserMessage(text) => self.prompt(text).await,
            // No agent that speaks ACP puts questions this way; one that did is let go ahead.
            AgentCommand::Allow { request_id } | AgentCommand::Answer { request_id, .. } => {
                self.decide(&request_id, true).await;
            }
            AgentCommand::Deny { request_id, .. } => self.decide(&request_id, false).await,
            AgentCommand::Interrupt => self.cancel().await,
            AgentCommand::SetModel(model) => self.set_model(model).await,
            // Applied here, when the agent asks, and by the agent itself if it has modes.
            AgentCommand::SetSupervision(supervision) => {
                self.spec.supervision = supervision;
                self.set_mode().await;
            }
            // The protocol has no way to let a call return while its work goes on.
            AgentCommand::SendToBackground { tool_use_id } => {
                self.emit(AgentEvent::SentToBackground {
                    tool_use_id,
                    moved: false,
                    refused: Some("this agent cannot send work to the background".to_string()),
                });
            }
            AgentCommand::StopBackground { task_id } => self.stop_child(&task_id).await,
            // These agents describe no screens of their own, and cannot be reached from
            // claude.ai.
            AgentCommand::Show(_) | AgentCommand::AskUsage | AgentCommand::RemoteControl { .. } => {
            }
        }
    }

    /// Stops the sub-agent whose work is reported under `task_id`, by cancelling its session,
    /// which only stops what it is doing. Nothing else an ACP agent reports can be stopped.
    async fn stop_child(&mut self, task_id: &str) {
        let Some(child) = self.children.to_stop(task_id).map(String::from) else {
            return;
        };
        let params = json!({ "sessionId": child });
        self.write(&json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": params }))
            .await;
    }

    async fn on_line(&mut self, line: &str) {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let id = message.get("id");
        let method = str_of(&message, "method");
        let params = message.get("params").unwrap_or(&Value::Null);
        match (id, method) {
            (Some(id), Some(method)) => self.on_request(id, method, params).await,
            (Some(id), None) => {
                self.on_response(id.as_u64().unwrap_or_default(), &message)
                    .await;
            }
            (None, Some("session/update")) if !self.replaying => self.on_update(params),
            (None, Some(CHILD_UPDATE)) if !self.replaying => self.on_child_notice(params),
            _ => {}
        }
    }
}

/// How long an agent that took something up by itself may say nothing at all, not even what
/// it is thinking, before it counts as done with it.
const QUIET: Duration = Duration::from_secs(20);

/// Runs a session until the agent exits or cannot be set up, then reports how it ended.
async fn drive(
    mut session: Conversation,
    mut child: Child,
    stdout: ChildStdout,
    mut stderr: ChildStderr,
    mut commands: mpsc::UnboundedReceiver<AgentCommand>,
) {
    let stderr_text = tokio::spawn(async move {
        let mut text = String::new();
        stderr.read_to_string(&mut text).await.ok();
        text
    });
    let mut lines = BufReader::new(stdout).lines();
    session.initialize().await;
    while session.failure.is_none() && step(&mut session, &mut lines, &mut commands).await {}

    if let Some(reason) = session.failure.take() {
        // Dropping `child` kills the agent, which never got as far as a conversation.
        return session.emit(AgentEvent::Exited {
            code: Some(1),
            stderr: reason,
        });
    }
    let code = child.wait().await.ok().and_then(|status| status.code());
    let stderr = tail(&stderr_text.await.unwrap_or_default(), STDERR_TAIL_CHARS);
    session.emit(AgentEvent::Exited { code, stderr });
}

/// Handles one line from the agent or one command from Swarmed; `false` once the agent's
/// output has ended.
async fn step(
    session: &mut Conversation,
    lines: &mut Lines<BufReader<ChildStdout>>,
    commands: &mut mpsc::UnboundedReceiver<AgentCommand>,
) -> bool {
    tokio::select! {
        line = lines.next_line() => match line {
            Ok(Some(line)) => session.on_line(&line).await,
            _ => return false,
        },
        command = commands.recv(), if session.stdin.is_some() => match command {
            Some(command) => session.on_command(command).await,
            // Swarmed hung up: closing the agent's input is what asks it to exit.
            None => session.stdin = None,
        },
        // Nothing has come from it for a while, and it is not waiting on the user: whatever
        // it took up by itself, it is done with.
        () = tokio::time::sleep(QUIET), if session.woken && session.permissions.is_empty() => {
            session.end_own_turn(false);
        }
    }
    true
}

/// The servers in ACP's shape, by name. Every ACP agent takes stdio servers; HTTP ones only go
/// to an agent that says it takes them.
fn acp_servers(servers: &[McpServer], takes_http: bool) -> Vec<(String, Value)> {
    let pairs = |map: &std::collections::BTreeMap<String, String>| -> Vec<Value> {
        map.iter()
            .map(|(name, value)| json!({ "name": name, "value": value }))
            .collect()
    };
    let definition = |server: &McpServer| match &server.transport {
        McpTransport::Stdio { command, args, env } => Some(
            json!({ "name": server.name, "command": command, "args": args, "env": pairs(env) }),
        ),
        McpTransport::Http { url, headers } if takes_http => Some(
            json!({ "type": "http", "name": server.name, "url": url, "headers": pairs(headers) }),
        ),
        McpTransport::Http { .. } => None,
    };
    servers
        .iter()
        .filter_map(|server| Some((server.name.clone(), definition(server)?)))
        .collect()
}

/// Swarmed's guidance as it is handed to an agent with a message: marked as coming from the
/// app the conversation is shown in, and not from the user.
fn guidance_block(guidance: &str) -> String {
    format!(
        "<swarmed-guidance>\nThe following is from Swarmed, the app this conversation is shown \
         in, not from the user. It holds for the whole conversation.\n\n{guidance}\n\
         </swarmed-guidance>"
    )
}

fn str_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn error_text(error: &Value) -> String {
    str_of(error, "message").map_or_else(|| error.to_string(), String::from)
}

/// The options agents name their effort by: Codex's, and Antigravity's.
const EFFORT_OPTIONS: [&str; 2] = ["reasoning_effort", "thought_level"];

/// The one of those a conversation has, if any.
fn effort_option(opened: &Value) -> Option<&'static str> {
    let has = |id: &&str| current_value(opened, id).is_some();
    EFFORT_OPTIONS.into_iter().find(has)
}

/// What one of a conversation's options stands at when it opens, if the agent says.
fn current_value<'a>(opened: &'a Value, id: &str) -> Option<&'a str> {
    let options = opened.get("configOptions").and_then(Value::as_array);
    let named = |option: &&Value| str_of(option, "id") == Some(id);
    let option = options.and_then(|options| options.iter().find(named));
    option.and_then(|option| str_of(option, "currentValue"))
}

/// The model a new conversation reports it runs on, if it says.
fn current_model(result: &Value) -> String {
    current_value(result, "model")
        .unwrap_or_default()
        .to_string()
}

/// The answers a permission request offers, reduced to "go ahead once" and "not this time".
fn pending_permission(id: &Value, params: &Value) -> PendingPermission {
    let options = params.get("options").and_then(Value::as_array);
    let option_of = |kinds: [&str; 2]| {
        let options = options.into_iter().flatten();
        let by_kind = |kind: &str| {
            options
                .clone()
                .find(|option| str_of(option, "kind") == Some(kind))
        };
        let found = kinds.iter().find_map(|kind| by_kind(kind));
        found
            .and_then(|option| str_of(option, "optionId"))
            .map(String::from)
    };
    PendingPermission {
        id: id.clone(),
        allow: option_of(["allow_once", "allow_always"]),
        reject: option_of(["reject_once", "reject_always"]),
    }
}

fn has_arguments(tool: &Value) -> bool {
    tool.get("rawInput")
        .and_then(Value::as_object)
        .is_some_and(|input| !input.is_empty())
}

/// Copies what a later message says about a tool call onto what is known of it.
fn merge_tool(known: &mut Value, later: &Value) {
    for key in ["title", "kind", "rawInput"] {
        let value = later.get(key).filter(|value| !value.is_null());
        if let (Some(value), Some(known)) = (value, known.as_object_mut()) {
            known.insert(key.to_string(), value.clone());
        }
    }
}

/// What a tool call is named by, wherever it is shown: its title, or failing that its kind.
///
/// Some agents title a shell call with the command itself. That is an argument, not a name:
/// the arguments already say it, and a name goes where arguments must not, such as into a
/// notification. Such a call is named by its kind, and never by the title.
fn tool_name(tool: &Value) -> &str {
    let title = str_of(tool, "title");
    let command = tool
        .get("rawInput")
        .and_then(|input| str_of(input, "command"));
    let repeats_command = title.is_some() && title == command;
    let kind = str_of(tool, "kind");
    let name = if repeats_command {
        kind
    } else {
        title.or(kind)
    };
    name.unwrap_or("tool")
}

fn tool_use(update: &Value) -> AgentEvent {
    AgentEvent::ToolUse {
        id: str_of(update, "toolCallId").unwrap_or_default().to_string(),
        name: tool_name(update).to_string(),
        input: update.get("rawInput").cloned().unwrap_or_else(|| json!({})),
    }
}

/// What a finished tool call produced: its text and image content, or else what it printed,
/// as some agents send apart, or else its raw output. An image is taken to be kept apart, with
/// `[image]` in its place.
fn tool_output(update: &Value, printed: Option<String>, taking: &mut Taking) -> String {
    let blocks = update.get("content").and_then(Value::as_array);
    let said: Vec<String> = blocks
        .into_iter()
        .flatten()
        .filter_map(|block| block_said(block.get("content")?, taking))
        .collect();
    if !said.is_empty() {
        return said.join("\n");
    }
    if let Some(printed) = printed {
        return printed;
    }
    match update.get("rawOutput") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// What one content block of a tool call says: its text, or for an image,
/// `{"type": "image", "data": …, "mimeType": …}`, what stands in its place. Anything else, such
/// as a diff, says nothing here.
fn block_said(content: &Value, taking: &mut Taking) -> Option<String> {
    if let Some(text) = str_of(content, "text") {
        return Some(text.to_string());
    }
    let data = str_of(content, "data").filter(|_| str_of(content, "type") == Some("image"))?;
    Some(taking.image(str_of(content, "mimeType").unwrap_or_default(), data))
}

fn tail(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(max_chars)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_mode_is_turned_on_only_when_asked_for_and_offered() {
        let opened = json!({ "configOptions": [
            { "id": "fast-mode", "options": [{ "value": "off" }, { "value": "on" }] },
        ]});
        let offered = option_values(&opened, FAST_MODE);
        assert_eq!(fast_value(true, &offered), Some("on"));
        assert_eq!(fast_value(false, &offered), None);
        // An agent without the mode is not asked for it.
        assert_eq!(fast_value(true, &option_values(&opened, "mode")), None);
    }

    #[test]
    fn supervision_is_matched_to_a_mode_the_agent_offers() {
        let opened = json!({ "configOptions": [
            { "id": "model", "options": [{ "value": "gpt-a" }] },
            { "id": "mode", "options": [
                { "value": "read-only" }, { "value": "workspace-write" },
                { "value": "agent" }, { "value": "agent-full-access" },
            ]},
        ]});
        let modes = option_values(&opened, "mode");
        assert_eq!(modes.len(), 4);
        assert_eq!(agent_mode(Supervision::Ask, &modes), Some("read-only"));
        assert_eq!(agent_mode(Supervision::Plan, &modes), Some("read-only"));
        assert_eq!(agent_mode(Supervision::Auto, &modes), Some("agent"));
        assert_eq!(
            agent_mode(Supervision::Bypass, &modes),
            Some("agent-full-access")
        );
        // An agent with no modes, or none by these names, is left as it is.
        assert_eq!(option_values(&json!({}), "mode"), Vec::<String>::new());
        assert_eq!(agent_mode(Supervision::Auto, &[]), None);
        assert_eq!(agent_mode(Supervision::Ask, &["build".to_string()]), None);
        // Antigravity's: asking is its default, and it has a planning mode of its own.
        let modes = ["default", "accept-edits", "auto", "plan"].map(String::from);
        assert_eq!(agent_mode(Supervision::Ask, &modes), Some("default"));
        assert_eq!(agent_mode(Supervision::Plan, &modes), Some("plan"));
        assert_eq!(agent_mode(Supervision::Auto, &modes), Some("auto"));
        assert_eq!(agent_mode(Supervision::Bypass, &modes), None);
    }

    #[test]
    fn effort_is_matched_to_the_nearest_level_the_agent_offers() {
        let levels = |names: &[&str]| names.iter().map(ToString::to_string).collect::<Vec<_>>();
        let codex = levels(&["minimal", "low", "medium", "high"]);
        assert_eq!(agent_effort(Effort::Medium, &codex), Some("medium"));
        // Nothing higher on offer: the highest there is.
        assert_eq!(agent_effort(Effort::Max, &codex), Some("high"));
        assert_eq!(agent_effort(Effort::Xhigh, &codex), Some("high"));
        // Nothing as low on offer: the nearest above.
        assert_eq!(
            agent_effort(Effort::Low, &levels(&["medium", "high"])),
            Some("medium")
        );
        // An agent with no such choice, or with names nobody knows, is left alone.
        assert_eq!(agent_effort(Effort::High, &[]), None);
        assert_eq!(agent_effort(Effort::High, &levels(&["turbo"])), None);
    }

    #[test]
    fn permission_options_are_reduced_to_allow_and_reject() {
        let params = json!({ "options": [
            { "optionId": "always", "kind": "allow_always" },
            { "optionId": "once", "kind": "allow_once" },
            { "optionId": "no", "kind": "reject_once" },
        ]});
        let pending = pending_permission(&json!(7), &params);
        assert_eq!(pending.allow.as_deref(), Some("once"));
        assert_eq!(pending.reject.as_deref(), Some("no"));

        let only_always = json!({ "options": [{ "optionId": "always", "kind": "allow_always" }] });
        let pending = pending_permission(&json!(8), &only_always);
        assert_eq!(
            (pending.allow.as_deref(), pending.reject),
            (Some("always"), None)
        );
    }

    #[test]
    fn tool_output_prefers_text_content_over_raw_output() {
        let with_content = json!({
            "content": [{ "type": "content", "content": { "type": "text", "text": "42 passed" } }],
            "rawOutput": { "exit": 0 },
        });
        assert_eq!(output(&with_content, None), "42 passed");
        assert_eq!(output(&json!({ "rawOutput": "plain" }), None), "plain");
        assert_eq!(
            output(&json!({ "rawOutput": { "exit": 1 } }), None),
            r#"{"exit":1}"#
        );
        assert_eq!(output(&json!({}), None), "");
        // What a command printed apart, as Codex sends it, comes before its exit code.
        let printed = Some("total 8\n".to_string());
        assert_eq!(
            output(&json!({ "rawOutput": { "exit_code": 0 } }), printed),
            "total 8\n"
        );
    }

    #[test]
    fn an_image_a_tool_call_returned_is_taken_with_a_word_in_its_place() {
        let png = "iVBORw0KGgoAAAANSUhEUg==";
        let update = json!({
            "content": [
                { "type": "content", "content": { "type": "text", "text": "Took a screenshot" } },
                { "type": "content",
                  "content": { "type": "image", "data": png, "mimeType": "image/png" } },
                { "type": "content",
                  "content": { "type": "image", "data": "PHN2Zz4=", "mimeType": "image/svg+xml" } },
                { "type": "diff", "path": "a.txt", "oldText": "a", "newText": "b" },
            ],
            // Never read for content once there is some: it can hold the image again, whole.
            "rawOutput": { "content": [{ "type": "image", "data": png }] },
        });
        let mut taking = Taking::default();
        assert_eq!(
            tool_output(&update, None, &mut taking),
            "Took a screenshot\n[image]\n[image not shown: image/svg+xml]"
        );
        assert_eq!(taking.images().len(), 1);
        // An image alone is content too.
        let alone = json!({ "content": [{ "type": "content",
            "content": { "type": "image", "data": png, "mimeType": "image/png" } }],
            "rawOutput": "ignored" });
        assert_eq!(output(&alone, None), "[image]");
    }

    /// What a finished call produced, with what was taken of its images left aside.
    fn output(update: &Value, printed: Option<String>) -> String {
        tool_output(update, printed, &mut Taking::default())
    }

    #[test]
    fn a_tool_titled_with_its_own_command_is_named_by_its_kind() {
        let named = |update: Value| match tool_use(&update) {
            AgentEvent::ToolUse { name, .. } => name,
            other => panic!("not a tool use: {other:?}"),
        };
        let shell =
            json!({ "title": "ls -la", "kind": "execute", "rawInput": { "command": "ls -la" } });
        assert_eq!(named(shell), "execute");
        let read = json!({ "title": "Read file", "kind": "read", "rawInput": { "path": "a" } });
        assert_eq!(named(read), "Read file");
        assert_eq!(named(json!({})), "tool");
        // With no kind to go by, the command still does not stand in for a name.
        let bare = json!({ "title": "rm -rf build", "rawInput": { "command": "rm -rf build" } });
        assert_eq!(tool_name(&bare), "tool");
    }

    #[test]
    fn a_later_message_fills_in_a_tool_calls_arguments() {
        let mut known = json!({ "toolCallId": "c", "title": "Terminal", "rawInput": {} });
        assert!(!has_arguments(&known));
        merge_tool(
            &mut known,
            &json!({ "status": "in_progress", "rawInput": { "command": "ls" }, "title": null }),
        );
        assert!(has_arguments(&known));
        assert_eq!(known["title"], "Terminal");
    }

    #[test]
    fn http_servers_only_go_to_agents_that_take_them() {
        let pair = |name: &str, value: &str| {
            std::collections::BTreeMap::from([(name.to_string(), value.to_string())])
        };
        let servers = [
            McpServer {
                name: "db".into(),
                transport: McpTransport::Stdio {
                    command: "npx".into(),
                    args: vec!["db".into()],
                    env: pair("A", "1"),
                },
            },
            McpServer {
                name: "docs".into(),
                transport: McpTransport::Http {
                    url: "https://x/mcp".into(),
                    headers: pair("Key", "secret"),
                },
            },
        ];
        let all = acp_servers(&servers, true);
        assert_eq!(
            all[0].1,
            json!({ "name": "db", "command": "npx", "args": ["db"], "env": [{ "name": "A", "value": "1" }] })
        );
        let http = json!({ "type": "http", "name": "docs", "url": "https://x/mcp", "headers": [{ "name": "Key", "value": "secret" }] });
        assert_eq!(all[1].1, http);
        let stdio_only: Vec<String> = acp_servers(&servers, false)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(stdio_only, ["db"]);
    }

    #[test]
    fn the_model_is_read_from_the_config_options() {
        let result = json!({ "configOptions": [
            { "id": "mode", "currentValue": "build" },
            { "id": "model", "currentValue": "openrouter/some/model" },
        ]});
        assert_eq!(current_model(&result), "openrouter/some/model");
        assert_eq!(current_model(&json!({})), "");
    }
}
