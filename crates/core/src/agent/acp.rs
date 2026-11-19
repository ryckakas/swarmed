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

use super::{AgentBackend, AgentCommand, AgentEvent, AgentHandle, SessionSpec, Supervision};
use crate::error::{Error, Result};

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

impl AgentBackend for AcpBackend {
    fn start(&self, spec: SessionSpec) -> Result<AgentHandle> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .envs(self.env.iter().map(|(name, value)| (name, value)))
            .current_dir(&spec.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take().ok_or_else(missing_pipe)?;
        let stdout = child.stdout.take().ok_or_else(missing_pipe)?;
        let stderr = child.stderr.take().ok_or_else(missing_pipe)?;

        let (event_tx, events) = mpsc::unbounded_channel();
        let (commands, command_rx) = mpsc::unbounded_channel();
        let session = Conversation::new(spec, stdin, event_tx);
        drop(tokio::spawn(drive(
            session, child, stdout, stderr, command_rx,
        )));
        Ok(AgentHandle { events, commands })
    }
}

/// The requests Swarmed makes, remembered by id until the agent answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Initialize,
    OpenSession,
    /// Its answer is not needed: an agent without that model keeps its own.
    SetModel,
    Prompt,
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
    permissions: HashMap<String, PendingPermission>,
    /// Tool calls announced without their arguments yet, by id; some agents send those later.
    unannounced: HashMap<String, Value>,
    /// While an old conversation is loaded the agent replays it; Swarmed has it stored already.
    replaying: bool,
    /// Set when the agent could not be set up; the reason is what the user is shown.
    failure: Option<String>,
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
            permissions: HashMap::new(),
            unannounced: HashMap::new(),
            replaying: false,
            failure: None,
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
        let Some(earlier) = self.spec.resume.clone() else {
            let params = json!({ "cwd": cwd, "mcpServers": [] });
            return self.call(Call::OpenSession, "session/new", params).await;
        };
        let params = json!({ "sessionId": earlier, "cwd": cwd, "mcpServers": [] });
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
            mcp_servers: vec![],
            skills: 0,
        });
        if let Some(model) = self.spec.model.clone() {
            let params = json!({ "sessionId": session_id, "configId": "model", "value": model });
            self.call(Call::SetModel, "session/set_config_option", params)
                .await;
        }
        while let Some(text) = self.queued.pop_front() {
            self.prompt(text).await;
        }
    }

    async fn prompt(&mut self, text: String) {
        let Some(session_id) = self.session_id.clone().filter(|_| !self.replaying) else {
            return self.queued.push_back(text);
        };
        if self.calls.values().any(|call| *call == Call::OpenSession) {
            return self.queued.push_back(text);
        }
        let params =
            json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": text }] });
        self.call(Call::Prompt, "session/prompt", params).await;
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
            (Call::SetModel, _) => {}
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
        let finished = error.is_none() && str_of(result, "stopReason") == Some("end_turn");
        self.emit(AgentEvent::TurnDone {
            is_error: !finished,
            result: error,
            cost_usd: self.cost,
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
        if self.spec.supervision != Supervision::Ask {
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

    /// One `session/update` notification: a piece of what the agent is doing.
    fn on_update(&mut self, update: &Value) {
        match str_of(update, "sessionUpdate").unwrap_or_default() {
            "agent_message_chunk" => {
                self.text.push_str(
                    update
                        .pointer("/content/text")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                );
            }
            "tool_call" | "tool_call_update" => self.on_tool(update),
            "usage_update" => {
                self.cost = update
                    .pointer("/cost/amount")
                    .and_then(Value::as_f64)
                    .or(self.cost);
            }
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
    fn the_model_is_read_from_the_config_options() {
        let result = json!({ "configOptions": [
            { "id": "mode", "currentValue": "build" },
            { "id": "model", "currentValue": "openrouter/some/model" },
        ]});
        assert_eq!(current_model(&result), "openrouter/some/model");
        assert_eq!(current_model(&json!({})), "");
    }
}
