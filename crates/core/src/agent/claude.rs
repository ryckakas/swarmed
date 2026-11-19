//! Drives the installed Claude Code CLI over its line-delimited JSON protocol.
//!
//! The protocol has no published specification, so this is the only module that knows its
//! shapes. Parsing is deliberately loose: unknown message types become [`AgentEvent::Other`]
//! and missing fields fall back to defaults, because the CLI changes often.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use super::{
    AgentBackend, AgentCommand, AgentEvent, AgentHandle, Auth, McpServerInfo, SessionSpec,
    Supervision,
};
use crate::error::{Error, Result};

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
    flag("--resume", spec.resume.clone());
    flag("--allowedTools", joined(&spec.allowed_tools));
    flag("--disallowedTools", joined(&spec.disallowed_tools));
    flag("--append-system-prompt", spec.append_system_prompt.clone());
    flag(
        "--mcp-config",
        spec.mcp_servers
            .as_ref()
            .map(|s| json!({ "mcpServers": s }).to_string()),
    );
    args
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
        drop(tokio::spawn(write_commands(
            stdin,
            command_rx,
            pending.clone(),
        )));
        drop(tokio::spawn(read_events(
            child, stdout, stderr, event_tx, pending,
        )));
        Ok(AgentHandle { events, commands })
    }
}

/// Forwards the process's output as events until it exits, then reports the exit.
async fn read_events(
    mut child: Child,
    stdout: ChildStdout,
    mut stderr: ChildStderr,
    events: mpsc::UnboundedSender<AgentEvent>,
    pending: Pending,
) {
    let stderr_text = tokio::spawn(async move {
        let mut text = String::new();
        stderr.read_to_string(&mut text).await.ok();
        text
    });
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let parsed = serde_json::from_str::<Value>(&line).ok();
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
    mut commands: mpsc::UnboundedReceiver<AgentCommand>,
    pending: Pending,
) {
    let mut interrupts = 0_u64;
    while let Some(command) = commands.recv().await {
        let mut bytes = command_line(command, &pending, &mut interrupts)
            .to_string()
            .into_bytes();
        bytes.push(b'\n');
        if stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err() {
            break;
        }
    }
}

fn command_line(command: AgentCommand, pending: &Pending, interrupts: &mut u64) -> Value {
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
        AgentCommand::Interrupt => {
            *interrupts += 1;
            json!({
                "type": "control_request",
                "request_id": format!("swarmed-{interrupts}"),
                "request": { "subtype": "interrupt" },
            })
        }
    }
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
        "rate_limit_event" => vec![parse_rate_limit(message)],
        "result" => vec![AgentEvent::TurnDone {
            is_error: message
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            result: kind_of(message, "result").map(String::from),
            cost_usd: message.get("total_cost_usd").and_then(Value::as_f64),
        }],
        // Replies to our own control requests (interrupt) carry nothing we need.
        "system" | "control_response" => vec![],
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
    }
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
            "skills": ["a", "b"],
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
            resume: Some("abc".into()),
            allowed_tools: vec!["Bash(git add:*)".into(), "Bash(git commit:*)".into()],
            disallowed_tools: vec!["Bash(git push --force:*)".into()],
            mcp_servers: Some(
                json!({ "swarmed": { "type": "http", "url": "http://127.0.0.1:1" } }),
            ),
            ..SessionSpec::default()
        };
        let args = args(&spec).join(" ");
        assert!(args.contains("--permission-mode acceptEdits"), "{args}");
        assert!(args.contains("--model haiku --resume abc"), "{args}");
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
