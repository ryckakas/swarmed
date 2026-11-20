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

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use super::{
    AgentBackend, AgentCommand, AgentEvent, AgentHandle, Effort, McpServerInfo, SessionSpec,
    Supervision,
};
use crate::error::{Error, Result};
use crate::mcp::{McpServer, McpTransport};

/// How much of the process's stderr is kept for [`AgentEvent::Exited`].
const STDERR_TAIL_CHARS: usize = 2000;
/// JSON-RPC's code for a method the receiver does not implement.
const METHOD_NOT_FOUND: i64 = -32601;

/// Runs sessions with an ACP agent, such as `opencode acp` or `goose acp`.
#[derive(Debug, Clone)]
pub struct AcpBackend {
    /// The agent's executable; a bare name is looked up on `PATH`.
    pub program: PathBuf,
    /// The arguments that put it into ACP mode.
    pub args: Vec<String>,
    /// Extra environment variables for the agent, on top of Swarmed's own.
    pub env: Vec<(String, String)>,
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
        let session = Conversation::new(spec, stdin, event_tx);
        drop(tokio::spawn(drive(
            session, child, stdout, stderr, command_rx,
        )));
        Ok(AgentHandle {
            events,
            commands,
            pid,
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
        Supervision::Ask | Supervision::Plan => &["read-only", "ask"],
        Supervision::Auto => &["agent", "auto"],
        Supervision::Bypass => &["agent-full-access", "full-access"],
    };
    let wanted = |mode: &&String| names.contains(&mode.as_str());
    offered.iter().find(wanted).map(String::as_str)
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
    /// The agent's reply so far; ACP streams it in pieces and Swarmed shows whole blocks.
    text: String,
    cost: Option<f64>,
    /// Tokens used and the window's size, as last reported and not yet passed on.
    context: Option<(u64, u64)>,
    permissions: HashMap<String, PendingPermission>,
    /// Tool calls announced without their arguments yet, by id; some agents send those later.
    unannounced: HashMap<String, Value>,
    /// The MCP servers given to the agent, by name, in ACP's shape.
    passed: Vec<(String, Value)>,
    /// While an old conversation is loaded the agent replays it; Swarmed has it stored already.
    replaying: bool,
    /// While an earlier conversation, opened on the agent's default model, is put back on its
    /// own. What the agent says in that time is about the model it opened with, which is
    /// already no longer so, and messages wait so that they run on the right one.
    restoring: bool,
    /// Set when the agent could not be set up; the reason is what the user is shown.
    failure: Option<String>,
    /// The modes of its own the agent offers, by id. Empty if it has none.
    modes: Vec<String>,
}

impl Conversation {
    fn new(
        spec: SessionSpec,
        stdin: ChildStdin,
        events: mpsc::UnboundedSender<AgentEvent>,
    ) -> Self {
        Self {
            spec,
            stdin: Some(stdin),
            events,
            next_id: 0,
            calls: HashMap::new(),
            session_id: None,
            queued: VecDeque::new(),
            text: String::new(),
            cost: None,
            context: None,
            permissions: HashMap::new(),
            unannounced: HashMap::new(),
            passed: Vec::new(),
            replaying: false,
            restoring: false,
            failure: None,
            modes: vec![],
        }
    }

    fn emit(&self, event: AgentEvent) {
        self.events.send(event).ok();
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
            // Swarmed offers no file or terminal services; the agent uses its own tools.
            "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false },
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
            skills: 0,
            slash_commands: vec![],
        });
        self.modes = option_values(result, "mode");
        self.set_mode().await;
        self.set_effort(&option_values(result, "reasoning_effort"))
            .await;
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
        self.text.clear();
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

    /// Switches the open session to `model`. The protocol has no way back to the agent's own
    /// default, so `None` only applies when the session is next started.
    async fn set_model(&mut self, model: Option<String>) {
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
    async fn set_effort(&mut self, offered: &[String]) {
        let effort = self
            .spec
            .effort
            .and_then(|effort| agent_effort(effort, offered));
        let (Some(effort), Some(session_id)) = (effort, self.session_id.clone()) else {
            return;
        };
        let params =
            json!({ "sessionId": session_id, "configId": "reasoning_effort", "value": effort });
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

    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            self.emit(AgentEvent::Text { text });
        }
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
        self.flush_text();
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
        let finished = error.is_none() && str_of(result, "stopReason") == Some("end_turn");
        self.emit(AgentEvent::TurnDone {
            is_error: !finished,
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
        let tool = params.get("toolCall").unwrap_or(&Value::Null);
        let tool_id = str_of(tool, "toolCallId").unwrap_or_default().to_string();
        if let Some(known) = self.unannounced.get_mut(&tool_id) {
            merge_tool(known, tool);
        }
        self.announce_tool(&tool_id);
        self.flush_text();
        let request_id = id.to_string();
        self.emit(AgentEvent::PermissionRequest {
            request_id: request_id.clone(),
            tool_name: str_of(tool, "title")
                .or_else(|| str_of(tool, "kind"))
                .unwrap_or("tool")
                .to_string(),
            input: tool.get("rawInput").cloned().unwrap_or_else(|| json!({})),
            description: None,
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
    }

    /// A piece of the reply: shown at once, and kept to be sent whole when the agent moves on.
    fn on_chunk(&mut self, update: &Value) {
        if self.restoring {
            return;
        }
        let chunk = update.pointer("/content/text").and_then(Value::as_str);
        let chunk = chunk.unwrap_or_default();
        self.text.push_str(chunk);
        if !chunk.is_empty() {
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

    /// One `session/update` notification: a piece of what the agent is doing.
    fn on_update(&mut self, update: &Value) {
        match str_of(update, "sessionUpdate").unwrap_or_default() {
            "agent_message_chunk" => self.on_chunk(update),
            "tool_call" | "tool_call_update" => self.on_tool(update),
            "usage_update" => self.on_usage(update),
            // Thoughts, plans, mode and command lists are not shown yet.
            _ => {}
        }
    }

    /// A tool call or a change to one. The call is shown once its arguments are known, or when
    /// it ends or needs permission, whichever comes first.
    fn on_tool(&mut self, update: &Value) {
        let id = str_of(update, "toolCallId").unwrap_or_default().to_string();
        let status = str_of(update, "status").unwrap_or_default();
        let ended = status == "completed" || status == "failed";
        if str_of(update, "sessionUpdate") == Some("tool_call") {
            self.unannounced.insert(id.clone(), update.clone());
        }
        if let Some(known) = self.unannounced.get_mut(&id) {
            merge_tool(known, update);
        }
        let has_input = self.unannounced.get(&id).is_some_and(has_arguments);
        if ended || has_input {
            self.announce_tool(&id);
        }
        if ended {
            let content = tool_output(update);
            self.emit(AgentEvent::ToolResult {
                tool_use_id: id,
                content,
                is_error: status == "failed",
            });
        }
    }

    /// Shows a tool call that has not been shown yet.
    fn announce_tool(&mut self, id: &str) {
        if let Some(known) = self.unannounced.remove(id) {
            self.flush_text();
            self.emit(tool_use(&known));
        }
    }

    async fn on_command(&mut self, command: AgentCommand) {
        match command {
            AgentCommand::UserMessage(text) => self.prompt(text).await,
            AgentCommand::Allow { request_id } => self.decide(&request_id, true).await,
            AgentCommand::Deny { request_id, .. } => self.decide(&request_id, false).await,
            AgentCommand::Interrupt => self.cancel().await,
            AgentCommand::SetModel(model) => self.set_model(model).await,
            // Applied here, when the agent asks, and by the agent itself if it has modes.
            AgentCommand::SetSupervision(supervision) => {
                self.spec.supervision = supervision;
                self.set_mode().await;
            }
            // These agents report nothing left running, so there is nothing to stop.
            AgentCommand::StopBackground { .. } => {}
        }
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
            (None, Some("session/update")) if !self.replaying => {
                self.on_update(params.get("update").unwrap_or(&Value::Null));
            }
            _ => {}
        }
    }
}

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

/// The model a new conversation reports it runs on, if it says.
fn current_model(result: &Value) -> String {
    let options = result.get("configOptions").and_then(Value::as_array);
    let model = options
        .into_iter()
        .flatten()
        .find(|option| str_of(option, "id") == Some("model"));
    model
        .and_then(|option| str_of(option, "currentValue"))
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

fn tool_use(update: &Value) -> AgentEvent {
    let input = update.get("rawInput").cloned().unwrap_or_else(|| json!({}));
    let title = str_of(update, "title");
    // Some agents title a shell call with the command itself, which the arguments already say.
    let repeats_command = title.is_some() && title == str_of(&input, "command");
    let kind = str_of(update, "kind");
    let name = if repeats_command {
        kind.or(title)
    } else {
        title.or(kind)
    };
    AgentEvent::ToolUse {
        id: str_of(update, "toolCallId").unwrap_or_default().to_string(),
        name: name.unwrap_or("tool").to_string(),
        input,
    }
}

/// What a finished tool call produced: its text content, or else its raw output.
fn tool_output(update: &Value) -> String {
    let blocks = update.get("content").and_then(Value::as_array);
    let texts: Vec<&str> = blocks
        .into_iter()
        .flatten()
        .filter_map(|block| block.pointer("/content/text").and_then(Value::as_str))
        .collect();
    if !texts.is_empty() {
        return texts.join("\n");
    }
    match update.get("rawOutput") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
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
        assert_eq!(tool_output(&with_content), "42 passed");
        assert_eq!(tool_output(&json!({ "rawOutput": "plain" })), "plain");
        assert_eq!(
            tool_output(&json!({ "rawOutput": { "exit": 1 } })),
            r#"{"exit":1}"#
        );
        assert_eq!(tool_output(&json!({})), "");
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
