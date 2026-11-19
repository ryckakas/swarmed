//! Drives the installed Claude Code CLI over its line-delimited JSON protocol.
//!
//! The protocol has no published specification, so this is the only module that knows its
//! shapes. Parsing is deliberately loose: unknown message types become [`AgentEvent::Other`]
//! and missing fields fall back to defaults, because the CLI changes often.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use super::{
    AgentBackend, AgentCommand, AgentEvent, AgentHandle, Auth, ContextCategory, McpServerInfo,
    Namer, Naming, SessionSpec, Supervision,
};
use crate::error::{Error, Result};
use crate::mcp::{McpServer, McpTransport};

/// How much of the process's stderr is kept for [`AgentEvent::Exited`].
const STDERR_TAIL_CHARS: usize = 2000;

/// Runs sessions with the Claude Code CLI.
#[derive(Debug, Clone)]
pub struct ClaudeBackend {
    /// The `claude` executable; a bare name is looked up on `PATH`.
    pub program: PathBuf,
}

impl Default for ClaudeBackend {
    /// Uses `claude` from `PATH`, or the executable named by `SWARMED_CLAUDE_BIN`.
    fn default() -> Self {
        let program = std::env::var_os("SWARMED_CLAUDE_BIN");
        Self {
            program: program.map_or_else(|| PathBuf::from("claude"), PathBuf::from),
        }
    }
}

/// How long naming a conversation may take before the title from the prompt is kept.
const NAMING_TIMEOUT: Duration = Duration::from_secs(30);

const NAMING_INSTRUCTIONS: &str = "You name conversations. Reply with a title of three to six \
    words for the request you are given: plain text, no quotes, no full stop, in the language \
    of the request. Never answer the request itself.";

/// Names conversations with one short call to Claude Code's smallest model.
///
/// The call uses the CLI's stored login and nothing else of the user's setup: no settings, no
/// MCP servers, no tools, and no session is saved.
#[derive(Debug, Clone, Default)]
pub struct ClaudeNamer {
    backend: ClaudeBackend,
}

impl ClaudeNamer {
    fn command(&self, prompt: &str) -> Command {
        let mut command = Command::new(&self.backend.program);
        command
            .args(["-p", "--model", "haiku", "--output-format", "text"])
            .args(["--setting-sources", "", "--strict-mcp-config"])
            .args(["--no-session-persistence", "--tools", ""])
            .args(["--system-prompt", NAMING_INSTRUCTIONS])
            .arg(prompt)
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        apply_auth(&mut command, &Auth::Subscription);
        command
    }
}

impl Namer for ClaudeNamer {
    fn name(&self, prompt: String) -> Naming {
        let mut command = self.command(&prompt);
        Box::pin(async move {
            let output = tokio::time::timeout(NAMING_TIMEOUT, command.output()).await;
            let output = output.ok()?.ok().filter(|o| o.status.success())?;
            Some(String::from_utf8_lossy(&output.stdout).into_owned())
        })
    }
}

/// Inputs of pending permission requests, which have to be echoed back when allowing.
#[derive(Debug, Clone, Default)]
struct Pending(Arc<Mutex<HashMap<String, Value>>>);

impl Pending {
    fn insert(&self, request_id: String, input: Value) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(request_id, input);
    }

    fn take(&self, request_id: &str) -> Option<Value> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(request_id)
    }
}

fn permission_mode(supervision: Supervision) -> &'static str {
    match supervision {
        Supervision::Ask => "acceptEdits",
        Supervision::Auto => "auto",
        Supervision::Bypass => "bypassPermissions",
        Supervision::Plan => "plan",
    }
}

fn args(spec: &SessionSpec) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // Replies arrive piece by piece, as well as whole once finished.
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
        // Always explicit: the user's own default mode must not override the task's setting.
        "--permission-mode",
        permission_mode(spec.supervision),
    ]
    .map(String::from)
    .to_vec();
    let mut flag = |name: &str, value: Option<String>| {
        if let Some(value) = value {
            args.extend([name.to_string(), value]);
        }
    };
    let joined = |rules: &[String]| (!rules.is_empty()).then(|| rules.join(","));
    flag("--model", spec.model.clone());
    flag(
        "--effort",
        spec.effort.map(|effort| effort.as_str().to_string()),
    );
    flag("--resume", spec.resume.clone());
    flag("--allowedTools", joined(&spec.allowed_tools));
    flag("--disallowedTools", joined(&spec.disallowed_tools));
    flag("--append-system-prompt", spec.append_system_prompt.clone());
    flag("--mcp-config", mcp_config(&spec.mcp_servers));
    args
}

/// The servers as the `--mcp-config` JSON Claude Code takes, or `None` if there are none.
fn mcp_config(servers: &[McpServer]) -> Option<String> {
    let definition = |server: &McpServer| match &server.transport {
        McpTransport::Stdio { command, args, env } => {
            json!({ "type": "stdio", "command": command, "args": args, "env": env })
        }
        McpTransport::Http { url, headers } => {
            json!({ "type": "http", "url": url, "headers": headers })
        }
    };
    let by_name: serde_json::Map<String, Value> = servers
        .iter()
        .map(|server| (server.name.clone(), definition(server)))
        .collect();
    (!by_name.is_empty()).then(|| json!({ "mcpServers": by_name }).to_string())
}

/// Shapes the child's environment for the chosen credentials.
fn apply_auth(command: &mut Command, auth: &Auth) {
    if matches!(auth, Auth::Inherit) {
        return;
    }
    // Remove credentials and the markers of an enclosing Claude Code session, so the child
    // behaves the same whether Swarmed was started from a shell or from inside an agent.
    let inherited = std::env::vars_os().map(|(name, _)| name);
    for name in inherited.filter(|name| {
        let name = name.to_string_lossy();
        name.starts_with("ANTHROPIC_") || name.starts_with("CLAUDE")
    }) {
        command.env_remove(name);
    }
    if let Auth::ApiKey(key) = auth {
        command.env("ANTHROPIC_API_KEY", key);
    }
}

fn missing_pipe() -> Error {
    Error::Io(std::io::Error::other("the agent's pipes are missing"))
}

impl AgentBackend for ClaudeBackend {
    fn start(&self, spec: SessionSpec) -> Result<AgentHandle> {
        let mut command = Command::new(&self.program);
        command
            .args(args(&spec))
            .current_dir(&spec.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        apply_auth(&mut command, &spec.auth);

        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or_else(missing_pipe)?;
        let stdout = child.stdout.take().ok_or_else(missing_pipe)?;
        let stderr = child.stderr.take().ok_or_else(missing_pipe)?;

        let (event_tx, events) = mpsc::unbounded_channel();
        let (commands, command_rx) = mpsc::unbounded_channel();
        let pending = Pending::default();
        // Requests of Swarmed's own, which the reading side raises and the writing side sends.
        let (asks, ask_rx) = mpsc::unbounded_channel();
        drop(tokio::spawn(write_commands(
            stdin,
            (command_rx, ask_rx),
            pending.clone(),
        )));
        drop(tokio::spawn(read_events(
            child,
            (stdout, stderr),
            event_tx,
            pending,
            asks,
        )));
        Ok(AgentHandle { events, commands })
    }
}

/// Forwards the process's output as events until it exits, then reports the exit.
async fn read_events(
    mut child: Child,
    (stdout, mut stderr): (ChildStdout, ChildStderr),
    events: mpsc::UnboundedSender<AgentEvent>,
    pending: Pending,
    asks: mpsc::UnboundedSender<Value>,
) {
    let stderr_text = tokio::spawn(async move {
        let mut text = String::new();
        stderr.read_to_string(&mut text).await.ok();
        text
    });
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let parsed = serde_json::from_str::<Value>(&line).ok();
        for question in parsed.as_ref().map_or_else(Vec::new, questions_after) {
            asks.send(question).ok();
        }
        let new_events = parsed.map_or_else(Vec::new, |message| parse_message(&message, &pending));
        if new_events
            .into_iter()
            .any(|event| events.send(event).is_err())
        {
            // Nobody is listening any more; dropping `child` kills the process.
            return;
        }
    }
    let code = child.wait().await.ok().and_then(|status| status.code());
    let stderr = tail(&stderr_text.await.unwrap_or_default(), STDERR_TAIL_CHARS);
    events.send(AgentEvent::Exited { code, stderr }).ok();
}

/// Turns commands into protocol lines. Ends, closing stdin and so the process, when the
/// command channel is dropped.
async fn write_commands(
    mut stdin: ChildStdin,
    (mut commands, mut asks): (
        mpsc::UnboundedReceiver<AgentCommand>,
        mpsc::UnboundedReceiver<Value>,
    ),
    pending: Pending,
) {
    let mut requests = 0_u64;
    loop {
        let line = tokio::select! {
            command = commands.recv() => command.map(|c| command_line(c, &pending, &mut requests)),
            Some(ask) = asks.recv() => Some(ask),
        };
        let Some(line) = line else {
            break;
        };
        let mut bytes = line.to_string().into_bytes();
        bytes.push(b'\n');
        if stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err() {
            break;
        }
    }
}

/// What to ask the CLI once it has sent `message`. When it starts, which model and effort it
/// really runs with, since a default says neither. After a turn, that again, as either can have
/// changed, and how full the context window has become.
fn questions_after(message: &Value) -> Vec<Value> {
    match (kind_of(message, "type"), kind_of(message, "subtype")) {
        (Some("system"), Some("init")) => vec![settings_request()],
        (Some("result"), _) => vec![context_request(), settings_request()],
        _ => vec![],
    }
}

/// The id of the request that asks which settings the CLI runs with.
const SETTINGS_REQUEST: &str = "swarmed-settings";

fn settings_request() -> Value {
    json!({
        "type": "control_request",
        "request_id": SETTINGS_REQUEST,
        "request": { "subtype": "get_settings" },
    })
}

/// The answer to [`settings_request`], if `message` is one.
///
/// Only the model and the effort are read. The rest of the answer is the user's whole Claude
/// Code configuration, environment included: it is not kept, passed on or logged.
fn parse_applied(message: &Value) -> Option<AgentEvent> {
    let response = message.get("response")?;
    if kind_of(response, "request_id") != Some(SETTINGS_REQUEST) {
        return None;
    }
    let applied = response.pointer("/response/applied")?;
    Some(AgentEvent::Applied {
        model: kind_of(applied, "model")?.to_string(),
        effort: kind_of(applied, "effort").map(String::from),
    })
}

/// The id of the request that asks how full the context window is.
const CONTEXT_REQUEST: &str = "swarmed-context";

fn context_request() -> Value {
    json!({
        "type": "control_request",
        "request_id": CONTEXT_REQUEST,
        "request": { "subtype": "get_context_usage" },
    })
}

/// The answer to [`context_request`], if `message` is one.
fn parse_context(message: &Value) -> Option<AgentEvent> {
    let response = message.get("response")?;
    if kind_of(response, "request_id") != Some(CONTEXT_REQUEST) {
        return None;
    }
    let usage = response.get("response")?;
    let categories = usage.get("categories").and_then(Value::as_array);
    let category = |c: &Value| ContextCategory {
        name: str_of(c, "name"),
        tokens: c.get("tokens").and_then(Value::as_u64).unwrap_or(0),
        kind: str_of(c, "kind"),
    };
    Some(AgentEvent::ContextUsage {
        used: usage.get("totalTokens").and_then(Value::as_u64)?,
        max: usage.get("maxTokens").and_then(Value::as_u64)?,
        categories: categories.map_or_else(Vec::new, |c| c.iter().map(category).collect()),
    })
}

fn command_line(command: AgentCommand, pending: &Pending, requests: &mut u64) -> Value {
    match command {
        AgentCommand::UserMessage(text) => json!({
            "type": "user",
            "message": { "role": "user", "content": text },
        }),
        AgentCommand::Allow { request_id } => {
            let input = pending.take(&request_id).unwrap_or_else(|| json!({}));
            control_response(
                &request_id,
                &json!({ "behavior": "allow", "updatedInput": input }),
            )
        }
        AgentCommand::Deny { request_id, reason } => {
            pending.take(&request_id);
            control_response(
                &request_id,
                &json!({ "behavior": "deny", "message": reason }),
            )
        }
        AgentCommand::Interrupt => control_request(requests, &json!({ "subtype": "interrupt" })),
        // A null model puts the CLI back on its own default.
        AgentCommand::SetModel(model) => {
            control_request(requests, &json!({ "subtype": "set_model", "model": model }))
        }
        AgentCommand::SetSupervision(supervision) => {
            let mode = permission_mode(supervision);
            control_request(
                requests,
                &json!({ "subtype": "set_permission_mode", "mode": mode }),
            )
        }
    }
}

/// A request of our own to the CLI; each needs an id it has not seen.
fn control_request(requests: &mut u64, request: &Value) -> Value {
    *requests += 1;
    json!({
        "type": "control_request",
        "request_id": format!("swarmed-{requests}"),
        "request": request,
    })
}

fn control_response(request_id: &str, response: &Value) -> Value {
    json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": request_id, "response": response },
    })
}

fn str_of(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn kind_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Maps one protocol message to zero or more events.
fn parse_message(message: &Value, pending: &Pending) -> Vec<AgentEvent> {
    match kind_of(message, "type").unwrap_or_default() {
        // Hook activity, thinking-token counters and the like are not shown yet.
        "system" if kind_of(message, "subtype") == Some("init") => vec![parse_init(message)],
        "assistant" => content_blocks(message)
            .filter_map(parse_assistant_block)
            .collect(),
        "user" => content_blocks(message)
            .filter_map(parse_tool_result)
            .collect(),
        "control_request" => vec![parse_control_request(message, pending)],
        "control_cancel_request" => withdraw(message, pending),
        "stream_event" => text_delta(message).into_iter().collect(),
        "rate_limit_event" => vec![parse_rate_limit(message)],
        "result" => vec![AgentEvent::TurnDone {
            is_error: message
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            result: kind_of(message, "result").map(String::from),
            cost_usd: message.get("total_cost_usd").and_then(Value::as_f64),
        }],
        // Of the replies to our own control requests, these two are of use.
        "control_response" => parse_context(message)
            .or_else(|| parse_applied(message))
            .into_iter()
            .collect(),
        "system" => vec![],
        _ => vec![AgentEvent::Other {
            raw: message.clone(),
        }],
    }
}

fn parse_init(message: &Value) -> AgentEvent {
    let servers = message.get("mcp_servers").and_then(Value::as_array);
    let server_info = |s: &Value| McpServerInfo {
        name: str_of(s, "name"),
        status: str_of(s, "status"),
    };
    AgentEvent::Init {
        agent_session_id: str_of(message, "session_id"),
        model: str_of(message, "model"),
        permission_mode: str_of(message, "permissionMode"),
        api_key_source: str_of(message, "apiKeySource"),
        mcp_servers: servers.map_or_else(Vec::new, |s| s.iter().map(server_info).collect()),
        skills: message
            .get("skills")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        slash_commands: strings(message.get("slash_commands")),
    }
}

/// The strings of a JSON array; anything else in it is skipped.
fn strings(value: Option<&Value>) -> Vec<String> {
    let items = value.and_then(Value::as_array).into_iter().flatten();
    items.filter_map(Value::as_str).map(String::from).collect()
}

fn parse_assistant_block(block: &Value) -> Option<AgentEvent> {
    match kind_of(block, "type")? {
        "text" => Some(AgentEvent::Text {
            text: str_of(block, "text"),
        }),
        "tool_use" => Some(AgentEvent::ToolUse {
            id: str_of(block, "id"),
            name: str_of(block, "name"),
            input: block.get("input").cloned().unwrap_or(Value::Null),
        }),
        _ => None,
    }
}

fn parse_tool_result(block: &Value) -> Option<AgentEvent> {
    (kind_of(block, "type") == Some("tool_result")).then(|| AgentEvent::ToolResult {
        tool_use_id: str_of(block, "tool_use_id"),
        content: flatten_content(block.get("content")),
        is_error: block
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn parse_control_request(message: &Value, pending: &Pending) -> AgentEvent {
    let request = message.get("request").unwrap_or(&Value::Null);
    if kind_of(request, "subtype") != Some("can_use_tool") {
        return AgentEvent::Other {
            raw: message.clone(),
        };
    }
    let request_id = str_of(message, "request_id");
    let input = request.get("input").cloned().unwrap_or_else(|| json!({}));
    pending.insert(request_id.clone(), input.clone());
    AgentEvent::PermissionRequest {
        request_id,
        tool_name: str_of(request, "tool_name"),
        input,
        description: kind_of(request, "description").map(String::from),
    }
}

/// The text in a piece of a message that is still being written. Pieces of thinking, of tool
/// arguments and of a sub-agent's messages are not shown.
fn text_delta(message: &Value) -> Option<AgentEvent> {
    let own = message.get("parent_tool_use_id").is_none_or(Value::is_null);
    let delta = message.pointer("/event/delta")?;
    let text = (own && kind_of(delta, "type") == Some("text_delta")).then(|| str_of(delta, "text"));
    text.map(|text| AgentEvent::TextDelta { text })
}

/// The CLI takes back a permission request, for example because the turn was interrupted.
fn withdraw(message: &Value, pending: &Pending) -> Vec<AgentEvent> {
    let request_id = str_of(message, "request_id");
    let known = pending.take(&request_id).is_some();
    let withdrawn = AgentEvent::PermissionWithdrawn { request_id };
    known.then_some(withdrawn).into_iter().collect()
}

fn parse_rate_limit(message: &Value) -> AgentEvent {
    let info = message
        .get("rate_limit_info")
        .cloned()
        .unwrap_or(Value::Null);
    AgentEvent::RateLimit {
        status: str_of(&info, "status"),
        resets_at: info.get("resetsAt").and_then(Value::as_i64),
        raw: info,
    }
}

fn content_blocks(message: &Value) -> impl Iterator<Item = &Value> {
    message
        .pointer("/message/content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// Tool results are either a string or a list of blocks; only text is kept.
fn flatten_content(content: Option<&Value>) -> String {
    let block_text = |block: &Value| {
        kind_of(block, "text").map_or_else(
            || format!("[{}]", kind_of(block, "type").unwrap_or("content")),
            String::from,
        )
    };
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks.iter().map(block_text).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn tail(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(max_chars)).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn parse(value: &Value) -> Vec<AgentEvent> {
        parse_message(value, &Pending::default())
    }

    #[test]
    fn init_lists_what_the_session_loaded() {
        let events = parse(&json!({
            "type": "system", "subtype": "init", "session_id": "abc", "model": "claude-haiku-5-5",
            "permissionMode": "acceptEdits", "apiKeySource": "none",
            "mcp_servers": [{ "name": "context7", "status": "connected" }],
            "skills": ["a", "b"], "slash_commands": ["review", "context"],
        }));
        let expected = AgentEvent::Init {
            agent_session_id: "abc".into(),
            model: "claude-haiku-5-5".into(),
            permission_mode: "acceptEdits".into(),
            api_key_source: "none".into(),
            mcp_servers: vec![McpServerInfo {
                name: "context7".into(),
                status: "connected".into(),
            }],
            skills: 2,
            slash_commands: vec!["review".into(), "context".into()],
        };
        assert_eq!(events, vec![expected]);
    }

    #[test]
    fn assistant_blocks_become_text_and_tool_use() {
        let events = parse(&json!({
            "type": "assistant",
            "message": { "content": [
                { "type": "thinking", "thinking": "" },
                { "type": "text", "text": "On it." },
                { "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "ls" } },
            ]},
        }));
        let tool_use = AgentEvent::ToolUse {
            id: "t1".into(),
            name: "Bash".into(),
            input: json!({ "command": "ls" }),
        };
        assert_eq!(
            events,
            vec![
                AgentEvent::Text {
                    text: "On it.".into()
                },
                tool_use
            ]
        );
    }

    #[test]
    fn tool_results_are_flattened_to_text() {
        let events = parse(&json!({
            "type": "user",
            "message": { "content": [
                { "type": "tool_result", "tool_use_id": "t1",
                  "content": [{ "type": "text", "text": "a" }, { "type": "image" }] },
                { "type": "tool_result", "tool_use_id": "t2", "content": "boom", "is_error": true },
            ]},
        }));
        let result = |id: &str, content: &str, is_error| AgentEvent::ToolResult {
            tool_use_id: id.into(),
            content: content.into(),
            is_error,
        };
        assert_eq!(
            events,
            vec![
                result("t1", "a\n[image]", false),
                result("t2", "boom", true)
            ]
        );
    }

    #[test]
    fn allowing_a_request_echoes_its_input() {
        let pending = Pending::default();
        let events = parse_message(
            &json!({
                "type": "control_request", "request_id": "r1",
                "request": { "subtype": "can_use_tool", "tool_name": "Bash",
                             "input": { "command": "git push" }, "description": "Push" },
            }),
            &pending,
        );
        assert!(matches!(
            events.first(),
            Some(AgentEvent::PermissionRequest { request_id, tool_name, .. })
                if request_id == "r1" && tool_name == "Bash"
        ));

        let allow = AgentCommand::Allow {
            request_id: "r1".into(),
        };
        let line = command_line(allow, &pending, &mut 0);
        assert_eq!(line.pointer("/response/request_id"), Some(&json!("r1")));
        assert_eq!(
            line.pointer("/response/response"),
            Some(&json!({ "behavior": "allow", "updatedInput": { "command": "git push" } }))
        );
        assert_eq!(pending.take("r1"), None);
    }

    #[test]
    fn naming_uses_the_small_model_and_none_of_the_users_setup() {
        let namer = ClaudeNamer::default();
        let command = namer.command("fix the login");
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let line = args.join(" ");
        assert!(line.starts_with("-p --model haiku"), "{line}");
        assert!(
            line.contains("--strict-mcp-config --no-session-persistence"),
            "{line}"
        );
        assert_eq!(args.last().map(String::as_str), Some("fix the login"));
    }

    #[test]
    fn settings_change_through_control_requests() {
        let mut requests = 0;
        let mut line = |command| command_line(command, &Pending::default(), &mut requests);
        let model = line(AgentCommand::SetModel(Some("opus".into())));
        assert_eq!(
            model.get("request"),
            Some(&json!({ "subtype": "set_model", "model": "opus" }))
        );
        let default_model = line(AgentCommand::SetModel(None));
        assert_eq!(default_model.pointer("/request/model"), Some(&Value::Null));
        let mode = line(AgentCommand::SetSupervision(Supervision::Auto));
        assert_eq!(
            mode.get("request"),
            Some(&json!({ "subtype": "set_permission_mode", "mode": "auto" }))
        );
        let plan = line(AgentCommand::SetSupervision(Supervision::Plan));
        assert_eq!(plan["request"]["mode"], "plan");
        assert_eq!(mode.get("request_id"), Some(&json!("swarmed-3")));
    }

    #[test]
    fn only_the_main_agents_text_is_streamed() {
        let delta = |delta: Value, parent: Value| {
            parse(&json!({
                "type": "stream_event", "parent_tool_use_id": parent,
                "event": { "type": "content_block_delta", "index": 0, "delta": delta },
            }))
        };
        let text = json!({ "type": "text_delta", "text": "Rain fal" });
        let piece = AgentEvent::TextDelta {
            text: "Rain fal".into(),
        };
        assert_eq!(delta(text.clone(), Value::Null), vec![piece]);
        assert_eq!(delta(text, json!("toolu_1")), vec![]);
        let arguments = json!({ "type": "input_json_delta", "partial_json": "{" });
        assert_eq!(delta(arguments, Value::Null), vec![]);
        let start = json!({ "type": "stream_event", "event": { "type": "message_start" } });
        assert_eq!(parse(&start), vec![]);
    }

    #[test]
    fn the_model_and_effort_in_use_are_read_from_the_settings_answer_and_nothing_else() {
        let answer = |request_id: &str| {
            json!({ "type": "control_response", "response": {
                "subtype": "success", "request_id": request_id,
                "response": {
                    "effective": { "env": { "SOME_TOKEN": "secret" }, "effortLevel": "xhigh" },
                    "applied": { "model": "claude-opus-5-5", "effort": "xhigh", "advisor": null },
                },
            }})
        };
        let applied = AgentEvent::Applied {
            model: "claude-opus-5-5".into(),
            effort: Some("xhigh".into()),
        };
        let events = parse(&answer(SETTINGS_REQUEST));
        assert_eq!(events, vec![applied]);
        assert!(!format!("{events:?}").contains("secret"));
        // Somebody else's answer, and one without the part we read, are nothing.
        assert_eq!(parse(&answer("other")), vec![]);
        let bare = json!({ "type": "control_response", "response": {
            "subtype": "success", "request_id": SETTINGS_REQUEST, "response": {},
        }});
        assert_eq!(parse(&bare), vec![]);

        let asked = |message: Value| questions_after(&message).len();
        assert_eq!(asked(json!({ "type": "system", "subtype": "init" })), 1);
        assert_eq!(asked(json!({ "type": "result" })), 2);
        assert_eq!(asked(json!({ "type": "assistant" })), 0);
    }

    #[test]
    fn the_context_windows_state_is_read_from_the_answer_to_our_own_request() {
        let answer = |request_id: &str| {
            json!({ "type": "control_response", "response": {
                "subtype": "success", "request_id": request_id,
                "response": {
                    "totalTokens": 29_629, "maxTokens": 1_000_000, "percentage": 3,
                    "categories": [
                        { "id": "messages", "name": "Messages", "tokens": 9171, "kind": "used" },
                        { "id": "free_space", "name": "Free space", "tokens": 937_371, "kind": "free" },
                    ],
                },
            }})
        };
        let category = |name: &str, tokens, kind: &str| ContextCategory {
            name: name.into(),
            tokens,
            kind: kind.into(),
        };
        let usage = AgentEvent::ContextUsage {
            used: 29_629,
            max: 1_000_000,
            categories: vec![
                category("Messages", 9171, "used"),
                category("Free space", 937_371, "free"),
            ],
        };
        assert_eq!(parse(&answer(CONTEXT_REQUEST)), vec![usage]);
        // The answer to an interrupt, or to anything else, is not shown.
        assert_eq!(parse(&answer("swarmed-1")), vec![]);
        assert_eq!(
            context_request().pointer("/request/subtype"),
            Some(&json!("get_context_usage"))
        );
    }

    #[test]
    fn a_cancelled_request_is_withdrawn_once() {
        let pending = Pending::default();
        pending.insert("r1".into(), json!({}));
        let cancel = json!({ "type": "control_cancel_request", "request_id": "r1" });
        let withdrawn = AgentEvent::PermissionWithdrawn {
            request_id: "r1".into(),
        };
        assert_eq!(parse_message(&cancel, &pending), vec![withdrawn]);
        assert_eq!(parse_message(&cancel, &pending), vec![]);
    }

    #[test]
    fn unknown_messages_are_kept_not_dropped() {
        let raw = json!({ "type": "brand_new_thing", "x": 1 });
        assert_eq!(parse(&raw), vec![AgentEvent::Other { raw: raw.clone() }]);
        assert_eq!(
            parse(&json!({ "type": "system", "subtype": "hook_started" })),
            vec![]
        );
    }

    #[test]
    fn ask_supervision_maps_to_accept_edits_and_passes_rules() {
        let spec = SessionSpec {
            model: Some("haiku".into()),
            effort: Some(crate::agent::Effort::High),
            resume: Some("abc".into()),
            allowed_tools: vec!["Bash(git add:*)".into(), "Bash(git commit:*)".into()],
            disallowed_tools: vec!["Bash(git push --force:*)".into()],
            mcp_servers: vec![McpServer {
                name: "swarmed".into(),
                transport: McpTransport::Http {
                    url: "http://127.0.0.1:1".into(),
                    headers: BTreeMap::new(),
                },
            }],
            ..SessionSpec::default()
        };
        let args = args(&spec).join(" ");
        assert!(args.contains("--permission-mode acceptEdits"), "{args}");
        assert!(
            args.contains("--model haiku --effort high --resume abc"),
            "{args}"
        );
        assert!(
            args.contains("--allowedTools Bash(git add:*),Bash(git commit:*)"),
            "{args}"
        );
        assert!(
            args.contains("--disallowedTools Bash(git push --force:*)"),
            "{args}"
        );
        assert!(
            args.contains(r#"--mcp-config {"mcpServers":{"swarmed":"#),
            "{args}"
        );
        assert!(!args.contains("--append-system-prompt"), "{args}");
    }
}
