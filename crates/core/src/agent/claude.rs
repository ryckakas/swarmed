//! Drives the installed Claude Code CLI over its line-delimited JSON protocol.
//!
//! The protocol has no published specification, so this is the only module that knows its
//! shapes. Parsing is deliberately loose: unknown message types become [`AgentEvent::Other`]
//! and missing fields fall back to defaults, because the CLI changes often.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use super::{
    AgentBackend, AgentCommand, AgentEvent, AgentHandle, Auth, BackgroundState,
    COMMIT_INSTRUCTIONS, ContextCategory, McpServerInfo, Namer, Naming, PULL_INSTRUCTIONS,
    RemoteState, SessionSpec, Supervision, UsageLimit,
};
use crate::agent_setup::{default_tool_dirs, find_program};
use crate::error::{Error, Result};
use crate::mcp::{McpServer, McpTransport};

mod screens;

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
    of the request. Never answer the request itself. A request may be a call of a skill or \
    command, given with what it is for: name what the user asks of it.";

/// How long writing a commit message may take: the changes take longer to read than a prompt.
const COMMIT_TIMEOUT: Duration = Duration::from_secs(60);

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
        self.asking(NAMING_INSTRUCTIONS, prompt)
    }

    /// One call to the smallest model under `instructions`, with nothing of the user's setup.
    /// No skill or command runs either: Claude Code reads a prompt that starts with `/` as one,
    /// even with no settings, and the model then answers with a stand-in for a tool call.
    fn asking(&self, instructions: &str, prompt: &str) -> Command {
        let mut command = Command::new(self.backend.resolved());
        command
            .args(["-p", "--model", "haiku", "--output-format", "text"])
            .args(["--setting-sources", "", "--strict-mcp-config"])
            .args(["--no-session-persistence", "--tools", ""])
            .arg("--disable-slash-commands")
            .args(["--system-prompt", instructions])
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
        let command = self.command(&prompt);
        Box::pin(answer(command, NAMING_TIMEOUT))
    }

    fn commit_message(&self, changes: String) -> Naming {
        let command = self.asking(COMMIT_INSTRUCTIONS, &changes);
        Box::pin(answer(command, COMMIT_TIMEOUT))
    }

    fn pull_message(&self, branch: String) -> Naming {
        let command = self.asking(PULL_INSTRUCTIONS, &branch);
        Box::pin(answer(command, COMMIT_TIMEOUT))
    }
}

/// What `command` printed, if it ended well within `limit`.
async fn answer(mut command: Command, limit: Duration) -> Option<String> {
    let output = tokio::time::timeout(limit, command.output()).await;
    let output = output.ok()?.ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// How many commands and sub-agents in the foreground are kept, the latest: a turn waits on a
/// few at most, and Claude Code may say nothing of one that ends there.
const FOREGROUND_KEPT: usize = 256;

/// What is pending between messages: the inputs of permission requests, which have to be
/// echoed back when allowing, and the commands and sub-agents a call waits on, by task id,
/// each with the report of its start, which is told only if it moves to the background.
#[derive(Debug, Clone, Default)]
struct Pending {
    requests: Arc<Mutex<HashMap<String, Value>>>,
    foreground: Arc<Mutex<VecDeque<(String, AgentEvent)>>>,
}

impl Pending {
    fn insert(&self, request_id: String, input: Value) {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(request_id, input);
    }

    fn take(&self, request_id: &str) -> Option<Value> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(request_id)
    }

    /// Keeps the report of a task that started in the foreground.
    fn in_foreground(&self, task_id: &str, started: AgentEvent) {
        let mut kept = self
            .foreground
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        kept.push_back((task_id.to_string(), started));
        while kept.len() > FOREGROUND_KEPT {
            kept.pop_front();
        }
    }

    /// The report of task `task_id`'s start, if it started in the foreground and has not left
    /// it until now. It has now: it is forgotten.
    fn out_of_foreground(&self, task_id: &str) -> Option<AgentEvent> {
        let mut kept = self
            .foreground
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let at = kept.iter().position(|(id, _)| id == task_id)?;
        kept.remove(at).map(|(_, started)| started)
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

/// The arguments Claude Code is started with. `mcp_config` is the file its MCP servers are
/// written in: never the servers themselves, since anyone on the machine can read a
/// process's arguments and the servers come with their secrets.
fn args(spec: &SessionSpec, mcp_config: Option<&Path>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // Replies arrive piece by piece, as well as whole once finished.
        "--include-partial-messages",
        // After a turn, what the user is likely to say next, if Claude Code has a guess.
        "--prompt-suggestions",
        // Each message of the user's said back as Claude Code takes it in, which for one
        // written in the middle of a turn is where it was heard.
        "--replay-user-messages",
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
    flag(
        "--mcp-config",
        mcp_config.map(|file| file.to_string_lossy().into_owned()),
    );
    args
}

/// What the files written for an agent are called, before a number that tells them apart.
const PRIVATE_FILE_PREFIX: &str = "swarmed-mcp-";

/// A file written for an agent to read, which holds secrets: only the user can read it, and
/// it is removed when this is dropped, which is when the agent's process is.
#[derive(Debug)]
struct PrivateFile(PathBuf);

impl PrivateFile {
    /// Writes `contents` to a new file for one run of an agent. In `folder`, which is the
    /// session's own, the files earlier runs left behind are removed first; without one the
    /// file goes among the system's temporary ones.
    fn write(folder: Option<&Path>, contents: &str) -> std::io::Result<Self> {
        static WRITTEN: AtomicU64 = AtomicU64::new(0);
        if let Some(folder) = folder {
            let mut private = std::fs::DirBuilder::new();
            private.recursive(true).mode(0o700).create(folder)?;
            remove_private_files(folder);
        }
        let number = WRITTEN.fetch_add(1, Ordering::Relaxed);
        let name = format!("{PRIVATE_FILE_PREFIX}{}-{number}.json", std::process::id());
        let file = Self(
            folder
                .map_or_else(std::env::temp_dir, Path::to_path_buf)
                .join(name),
        );
        // One of this name is left from a process long gone. Created anew, so that it is
        // never readable by anyone else, not even until its mode is set.
        std::fs::remove_file(&file.0).ok();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        options.open(&file.0)?.write_all(contents.as_bytes())?;
        Ok(file)
    }
}

impl Drop for PrivateFile {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

/// Removes the files written for earlier runs of an agent from a session's own `folder`.
fn remove_private_files(folder: &Path) {
    let entries = std::fs::read_dir(folder).into_iter().flatten().flatten();
    let ours = |name: &std::ffi::OsStr| name.to_string_lossy().starts_with(PRIVATE_FILE_PREFIX);
    for entry in entries.filter(|entry| ours(&entry.file_name())) {
        std::fs::remove_file(entry.path()).ok();
    }
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

impl ClaudeBackend {
    /// The executable to start. A bare name is looked for where installers put programs as
    /// well as on the `PATH`, each time: an app started from the Dock has a shorter `PATH`
    /// than a shell, and Claude Code may have been installed since Swarmed was started.
    fn resolved(&self) -> PathBuf {
        let bare = self.program.components().count() == 1;
        let name = self.program.to_string_lossy();
        bare.then(|| find_program(&name, &default_tool_dirs()))
            .flatten()
            .unwrap_or_else(|| self.program.clone())
    }

    /// Why Claude Code could not be started, said so that someone who never installed it knows
    /// what is missing and what to do.
    fn not_started(&self, error: std::io::Error) -> Error {
        if error.kind() == std::io::ErrorKind::NotFound {
            return Error::ClaudeMissing(self.program.to_string_lossy().into_owned());
        }
        error.into()
    }
}

impl AgentBackend for ClaudeBackend {
    fn interactive(&self) -> Option<PathBuf> {
        Some(self.resolved())
    }

    fn start(&self, spec: SessionSpec) -> Result<AgentHandle> {
        let servers = mcp_config(&spec.mcp_servers)
            .map(|servers| PrivateFile::write(spec.private_dir.as_deref(), &servers))
            .transpose()?;
        let mut command = Command::new(self.resolved());
        command
            .args(args(&spec, servers.as_ref().map(|file| file.0.as_path())))
            .current_dir(&spec.cwd)
            .env("PATH", crate::shellenv::path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // The task's own variables, except any that would decide what Claude Code signs in
        // with or how it behaves: those stay as `auth` and the user's own setup have them.
        let reserved = |name: &str| name.starts_with("ANTHROPIC_") || name.starts_with("CLAUDE");
        let given = spec.env.0.iter().filter(|(name, _)| !reserved(name));
        command.envs(given.map(|(name, value)| (name, value)));
        apply_auth(&mut command, &spec.auth);

        let mut child = command.spawn().map_err(|error| self.not_started(error))?;
        let pid = child.id();
        let stdin = child.stdin.take().ok_or_else(missing_pipe)?;
        let stdout = child.stdout.take().ok_or_else(missing_pipe)?;
        let stderr = child.stderr.take().ok_or_else(missing_pipe)?;

        let (event_tx, events) = mpsc::unbounded_channel();
        let (commands, command_rx) = mpsc::unbounded_channel();
        let pending = Pending::default();
        // Requests of Swarmed's own, which the reading side raises and the writing side sends.
        let (asks, ask_rx) = mpsc::unbounded_channel();
        let writing = tokio::spawn(write_commands(stdin, (command_rx, ask_rx), pending.clone()));
        let reading = tokio::spawn(read_events(
            (child, servers),
            (stdout, stderr),
            event_tx,
            pending,
            asks,
        ));
        Ok(AgentHandle {
            events,
            commands,
            pid,
            tasks: vec![reading.abort_handle(), writing.abort_handle()],
        })
    }
}

/// Forwards the process's output as events until it exits, then reports the exit.
///
/// The process and the file its MCP servers are written in are held here, so both go when
/// this ends, however it ends: the process is killed if it still runs, the file removed.
async fn read_events(
    (mut child, _servers): (Child, Option<PrivateFile>),
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
/// really runs with, since a default says neither, and what extra usage stands at. After a
/// turn, the model and effort again, as either can have changed, and how full the context
/// window has become. Extra usage again whenever a usage report says it pays for this turn,
/// which is when its amount moves.
fn questions_after(message: &Value) -> Vec<Value> {
    match (kind_of(message, "type"), kind_of(message, "subtype")) {
        (Some("system"), Some("init")) => vec![settings_request(), usage_request()],
        (Some("result"), _) => vec![context_request(), settings_request()],
        (Some("rate_limit_event"), _) if on_extra_usage(message) => vec![usage_request()],
        _ => vec![],
    }
}

/// Whether a usage report says the turn is paid for by extra usage.
fn on_extra_usage(message: &Value) -> bool {
    let flag = |name: &str| {
        message
            .pointer(&format!("/rate_limit_info/{name}"))
            .and_then(Value::as_bool)
            == Some(true)
    };
    flag("isUsingOverage") || flag("overageInUse")
}

/// The id of the request that asks what the account's usage stands at.
const USAGE_REQUEST: &str = "swarmed-usage";

/// Asks for the account's usage without the scan of local transcripts that would come with
/// it otherwise: only the plan's figures are wanted, and the scan reads a week of them.
fn usage_request() -> Value {
    json!({
        "type": "control_request",
        "request_id": USAGE_REQUEST,
        "request": { "subtype": "get_usage", "skip_behaviors": true },
    })
}

/// What Claude Code answered to one of Swarmed's own requests: the state of the context
/// window, the settings in use, the account's usage, or a screen. The `/usage` screen carries
/// the account's usage too, which is read from it as well, so that what the user last saw
/// is what the context card shows.
fn parse_answer(message: &Value) -> Vec<AgentEvent> {
    let known = parse_context(message)
        .or_else(|| parse_applied(message))
        .or_else(|| parse_remote_answer(message))
        .or_else(|| parse_background_answer(message));
    if let Some(event) = known {
        return vec![event];
    }
    if answers(message, USAGE_REQUEST) {
        return usage_figures(message);
    }
    let Some(screen) = screens::parse(message) else {
        return vec![];
    };
    let is_usage = matches!(&screen, AgentEvent::Screen { command, .. } if command == "usage");
    let figures = if is_usage {
        usage_figures(message)
    } else {
        vec![]
    };
    std::iter::once(screen).chain(figures).collect()
}

/// Whether `message` is the answer to the request `id`.
fn answers(message: &Value, id: &str) -> bool {
    message
        .get("response")
        .is_some_and(|response| kind_of(response, "request_id") == Some(id))
}

/// The plan's usage limits and the extra usage in an answer to `get_usage`, those it gives in
/// a shape known here. An answer that gives neither, such as an error, gives nothing.
///
/// Claude Code marks this request experimental, so a shape it does not have is no answer.
/// Only these figures are read: the rest of the answer, such as the plan and what the
/// account's sessions cost, is not kept, passed on or logged.
fn usage_figures(message: &Value) -> Vec<AgentEvent> {
    let Some(rate_limits) = message.pointer("/response/response/rate_limits") else {
        return vec![];
    };
    [parse_limits(rate_limits), parse_extra_usage(rate_limits)]
        .into_iter()
        .flatten()
        .collect()
}

/// The plan's usage limits, as `/usage` lists them; `None` without one in a known shape.
fn parse_limits(rate_limits: &Value) -> Option<AgentEvent> {
    let limits: Vec<UsageLimit> = rate_limits
        .get("limits")?
        .as_array()?
        .iter()
        .filter_map(parse_limit)
        .collect();
    (!limits.is_empty()).then_some(AgentEvent::UsageLimits { limits })
}

fn parse_limit(row: &Value) -> Option<UsageLimit> {
    Some(UsageLimit {
        kind: kind_of(row, "kind")?.to_string(),
        model: row
            .pointer("/scope/model/display_name")
            .and_then(Value::as_str)
            .map(str::to_string),
        percent: u32::from(screens::percent(row.get("percent"))?),
        resets_at: kind_of(row, "resets_at").map(str::to_string),
        severity: kind_of(row, "severity").unwrap_or("normal").to_string(),
    })
}

/// The extra usage, if the plan has it and it is in a known shape.
fn parse_extra_usage(rate_limits: &Value) -> Option<AgentEvent> {
    let extra = rate_limits.get("extra_usage")?;
    Some(AgentEvent::ExtraUsage {
        enabled: extra.get("is_enabled").and_then(Value::as_bool)?,
        used: extra.get("used_credits").and_then(Value::as_u64)?,
        limit: extra.get("monthly_limit").and_then(Value::as_u64),
        currency: kind_of(extra, "currency")?.to_string(),
        decimals: extra
            .get("decimal_places")
            .and_then(Value::as_u64)
            .and_then(|digits| u32::try_from(digits).ok())
            .unwrap_or(2),
        limit_reached: extra.get("spend_limit_reached").and_then(Value::as_bool) == Some(true),
    })
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

/// The next message Claude Code suggests, unless it is blank.
fn parse_suggestion(message: &Value) -> Option<AgentEvent> {
    let text = kind_of(message, "suggestion")?.trim();
    (!text.is_empty()).then(|| AgentEvent::Suggestion {
        text: text.to_string(),
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
        AgentCommand::Answer {
            request_id,
            answers,
        } => {
            // The questions go back as they came, with the answers beside them.
            let mut input = pending.take(&request_id).unwrap_or_else(|| json!({}));
            if let Some(input) = input.as_object_mut() {
                input.insert("answers".to_string(), json!(answers));
            }
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
        AgentCommand::StopBackground { task_id } => control_request(
            requests,
            &json!({ "subtype": "stop_task", "task_id": task_id }),
        ),
        AgentCommand::SendToBackground { tool_use_id } => {
            background_request(tool_use_id.as_deref())
        }
        AgentCommand::Show(kind) => screens::request(kind),
        AgentCommand::AskUsage => usage_request(),
        AgentCommand::RemoteControl { on, name, reattach } => {
            remote_control_request(on, &name, reattach.as_deref())
        }
    }
}

/// The id of a request that sends work to the background. One for a single call is followed
/// by `:` and the call's id, so that its answer says which call it is about.
const BACKGROUND_REQUEST: &str = "swarmed-background";

/// Sends the command or sub-agent that call `tool_use_id` waits on to the background, or with
/// `None` every one that a call waits on, as Ctrl+B does in Claude Code's terminal.
fn background_request(tool_use_id: Option<&str>) -> Value {
    let (request_id, request) = match tool_use_id {
        Some(id) => (
            format!("{BACKGROUND_REQUEST}:{id}"),
            json!({ "subtype": "background_tasks", "tool_use_id": id }),
        ),
        None => (
            BACKGROUND_REQUEST.to_string(),
            json!({ "subtype": "background_tasks" }),
        ),
    };
    json!({
        "type": "control_request",
        "request_id": request_id,
        "request": request,
    })
}

/// Claude Code's answer to [`background_request`], if `message` is one. Asked about one call,
/// it says whether that call's work went; asked about every call, it only takes the request.
/// What it says when it refuses is passed on, except where only its version is to blame.
fn parse_background_answer(message: &Value) -> Option<AgentEvent> {
    let response = message.get("response")?;
    let rest = kind_of(response, "request_id")?.strip_prefix(BACKGROUND_REQUEST)?;
    let tool_use_id = match rest {
        "" => None,
        call => Some(call.strip_prefix(':')?.to_string()),
    };
    if kind_of(response, "subtype") != Some("success") {
        let said = kind_of(response, "error").unwrap_or("Claude Code gave no reason");
        let refused = if said.contains("Unsupported control request subtype") {
            "this version of Claude Code cannot send work to the background: update it".to_string()
        } else {
            said.to_string()
        };
        return Some(AgentEvent::SentToBackground {
            tool_use_id,
            moved: false,
            refused: Some(refused),
        });
    }
    let went = response
        .pointer("/response/backgrounded")
        .and_then(Value::as_bool);
    Some(AgentEvent::SentToBackground {
        moved: went.unwrap_or_else(|| tool_use_id.is_none()),
        tool_use_id,
        refused: None,
    })
}

/// The id of the request that turns Remote Control on, which its answer is known by.
const REMOTE_ON_REQUEST: &str = "swarmed-remote-control";

/// The id of the request that turns it off, whose answer says nothing worth hearing.
const REMOTE_OFF_REQUEST: &str = "swarmed-remote-control-off";

/// Turns Remote Control on or off, which Claude Code offers a host that runs it without its
/// screen, as the Agent SDK's `enableRemoteControl` does. Its session on claude.ai is kept when
/// the process ends (`keep_session_on_exit`), so that the next run can attach to it again
/// (`reattach_session_id`) instead of listing the same session there twice.
fn remote_control_request(on: bool, name: &str, reattach: Option<&str>) -> Value {
    if !on {
        return json!({
            "type": "control_request",
            "request_id": REMOTE_OFF_REQUEST,
            "request": { "subtype": "remote_control", "enabled": false },
        });
    }
    let mut request = json!({
        "subtype": "remote_control",
        "enabled": true,
        "name": name,
        "keep_session_on_exit": true,
    });
    if let (Some(id), Some(fields)) = (reattach, request.as_object_mut()) {
        fields.insert("reattach_session_id".to_string(), json!(id));
    }
    json!({
        "type": "control_request",
        "request_id": REMOTE_ON_REQUEST,
        "request": request,
    })
}

/// Claude Code's answer to turning Remote Control on, if `message` is one: where the session
/// is on claude.ai, or why it could not be turned on.
fn parse_remote_answer(message: &Value) -> Option<AgentEvent> {
    let response = message.get("response")?;
    if kind_of(response, "request_id") != Some(REMOTE_ON_REQUEST) {
        return None;
    }
    if kind_of(response, "subtype") != Some("success") {
        let said = kind_of(response, "error").unwrap_or("Claude Code gave no reason");
        return Some(remote(RemoteState::Failed, Some(said)));
    }
    let answer = response.get("response");
    let field = |name: &str| {
        answer
            .and_then(|answer| kind_of(answer, name))
            .map(String::from)
    };
    Some(AgentEvent::RemoteControl {
        state: RemoteState::On,
        url: field("session_url"),
        remote_id: field("bridge_session_id"),
        detail: None,
    })
}

/// How Remote Control stands, as Claude Code reports each change: `ready` and `connected`
/// when it can be reached; `failed`, which is for good unless its kind says it is not and it
/// tries again; `policy_disabled` when the organisation's policy turned it off; anything else
/// is a connection it is trying to get back.
fn parse_bridge_state(message: &Value) -> Option<AgentEvent> {
    let detail = kind_of(message, "detail").filter(|detail| !detail.is_empty());
    let for_good = message
        .get("failure_kind")
        .and_then(Value::as_str)
        .is_none_or(|kind| kind == "terminal");
    let state = match kind_of(message, "state")? {
        "ready" | "connected" => RemoteState::On,
        "failed" if for_good => RemoteState::Failed,
        "policy_disabled" => RemoteState::Failed,
        _ => RemoteState::Reconnecting,
    };
    Some(remote(state, detail))
}

fn remote(state: RemoteState, detail: Option<&str>) -> AgentEvent {
    AgentEvent::RemoteControl {
        state,
        url: None,
        remote_id: None,
        detail: detail.map(String::from),
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
        "assistant" => terminal_only(message).map_or_else(
            || {
                let said = content_blocks(message).filter_map(parse_assistant_block);
                said.map(|event| of_sub_agent(message, event)).collect()
            },
            |refused| vec![refused],
        ),
        "user" if message.get("isReplay").and_then(Value::as_bool) == Some(true) => {
            heard(message).into_iter().collect()
        }
        "user" => {
            let results = content_blocks(message).filter_map(parse_tool_result);
            let mut events = Vec::new();
            for result in results {
                let output = started_in_background(&result);
                events.push(of_sub_agent(message, result));
                events.extend(output);
            }
            events
        }
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
            duration_ms: None,
        }],
        "prompt_suggestion" => parse_suggestion(message).into_iter().collect(),
        // The replies to our own control requests that are of use.
        "control_response" => parse_answer(message),
        "system" if kind_of(message, "subtype") == Some("bridge_state") => {
            parse_bridge_state(message).into_iter().collect()
        }
        "system" => parse_background(message, pending).into_iter().collect(),
        _ => vec![AgentEvent::Other {
            raw: message.clone(),
        }],
    }
}

/// A command sent to Claude Code that only opens a screen in its terminal, such as `/vim`.
/// Run without one, Claude Code answers for the command itself, with no model, and marks the
/// answer `unavailable_headless`. Its words start with the command as it was typed, so that
/// any such command is known, and none needs naming here. `None` for any other message.
fn terminal_only(message: &Value) -> Option<AgentEvent> {
    let outcome = message.pointer("/local_command_outcome/kind");
    if outcome.and_then(Value::as_str) != Some("unavailable_headless") {
        return None;
    }
    let said = content_blocks(message).find_map(|block| kind_of(block, "text"))?;
    let command = said.strip_prefix('/')?.split_whitespace().next()?;
    Some(AgentEvent::TerminalOnly {
        command: command.to_string(),
    })
}

/// `event` as it is, or marked as a sub-agent's if the message it came in belongs to one:
/// Claude Code names the tool call that started the sub-agent on each of its messages.
fn of_sub_agent(message: &Value, event: AgentEvent) -> AgentEvent {
    match kind_of(message, "parent_tool_use_id") {
        Some(parent) => AgentEvent::Sub {
            parent: parent.to_string(),
            event: Box::new(event),
        },
        None => event,
    }
}

/// What Claude Code says of the commands and sub-agents it runs as tasks: that one started,
/// moved to the background, or ended. One that a call waits on, in the foreground, is no work
/// left running: its start is kept here and told only if it moves to the background, and its
/// end in the foreground, which that call's result says, is not told at all. Its other system
/// messages, hooks and counters, are not shown.
fn parse_background(message: &Value, pending: &Pending) -> Option<AgentEvent> {
    let task_id = kind_of(message, "task_id")?;
    match kind_of(message, "subtype")? {
        "task_started" => {
            let started = task_report(message, BackgroundState::Running);
            let waited_on = message.get("is_backgrounded").and_then(Value::as_bool) == Some(false);
            if !waited_on {
                return Some(started);
            }
            pending.in_foreground(task_id, started);
            None
        }
        "task_updated" => moved_to_background(task_id, message.get("patch")?, pending),
        "task_notification" => {
            let state = match kind_of(message, "status")? {
                "completed" => BackgroundState::Completed,
                "stopped" | "killed" => BackgroundState::Stopped,
                _ => BackgroundState::Failed,
            };
            let in_foreground = pending.out_of_foreground(task_id).is_some();
            (!in_foreground).then(|| task_report(message, state))
        }
        _ => None,
    }
}

/// A report of a task as Claude Code describes it in `message`.
fn task_report(message: &Value, state: BackgroundState) -> AgentEvent {
    AgentEvent::Background {
        task_id: str_of(message, "task_id"),
        description: str_of(message, "description"),
        kind: str_of(message, "task_type"),
        state,
        summary: kind_of(message, "summary").map(String::from),
        tool_use_id: kind_of(message, "tool_use_id").map(String::from),
        // Claude Code names an empty file for a task that writes none.
        output_file: kind_of(message, "output_file")
            .filter(|file| !file.is_empty())
            .map(String::from),
        moved: false,
        unstoppable: false,
    }
}

/// A task that ran in the foreground and now runs in the background, if `patch`, a change to
/// task `task_id`, says it moved there: reported as it started, now running in the
/// background. Any other change is not told.
fn moved_to_background(task_id: &str, patch: &Value, pending: &Pending) -> Option<AgentEvent> {
    if patch.get("is_backgrounded").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let started = pending.out_of_foreground(task_id);
    let bare = || AgentEvent::Background {
        task_id: task_id.to_string(),
        description: str_of(patch, "description"),
        kind: String::new(),
        state: BackgroundState::Running,
        summary: None,
        tool_use_id: None,
        output_file: None,
        moved: true,
        unstoppable: false,
    };
    let mut report = started.unwrap_or_else(bare);
    if let AgentEvent::Background { moved, .. } = &mut report {
        *moved = true;
    }
    Some(report)
}

/// How Claude Code starts a `Bash` call's result when the command goes on in the background,
/// each with whether it ran in the foreground first and was moved there.
const IN_BACKGROUND: [(&str, bool); 2] = [
    ("Command running in background with ID: ", false),
    ("Command was manually backgrounded by user with ID: ", true),
];

/// Claude Code's word, in a `Bash` call's result, that the command goes on in the background
/// and where what it prints goes: "Command running in background with ID: bzufoxos3. Output
/// is being written to: /private/tmp/claude-501/.../bzufoxos3.output", or the same after
/// "Command was manually backgrounded by user with ID: " for one sent there while it ran. Read
/// as a report of it running, since its own report of the start does not say where its output
/// goes. Other wording is passed over: such a command shows what it printed only once it has
/// ended. So is a result that only holds those words further in: a file or a page that quotes
/// them would otherwise report a command that never ran, writing wherever the quote says.
fn started_in_background(result: &AgentEvent) -> Option<AgentEvent> {
    let AgentEvent::ToolResult {
        tool_use_id,
        content,
        ..
    } = result
    else {
        return None;
    };
    let said = content.trim_start();
    let (rest, moved) = IN_BACKGROUND
        .iter()
        .find_map(|(words, moved)| Some((said.strip_prefix(words)?, *moved)))?;
    let (task_id, rest) = rest.split_once(". Output is being written to: ")?;
    let named = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    if task_id.is_empty() || !task_id.chars().all(named) {
        return None;
    }
    let file = rest.lines().next()?.trim().trim_end_matches('.');
    (!file.is_empty()).then(|| AgentEvent::Background {
        task_id: task_id.to_string(),
        description: String::new(),
        kind: "local_bash".to_string(),
        state: BackgroundState::Running,
        summary: None,
        tool_use_id: Some(tool_use_id.clone()),
        output_file: Some(file.to_string()),
        moved,
        unstoppable: false,
    })
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

/// A message of the user's that Claude Code repeats as it takes it in: its text as sent, which
/// is a string or, for a message with more than text, its text blocks. One it made up itself
/// (`isSynthetic`), such as a reminder or a summary, or keeps for itself beside the
/// conversation (`isMeta`), such as what a skill of its own says, is nobody's message.
fn heard(message: &Value) -> Option<AgentEvent> {
    let marked = |key: &str| message.get(key).and_then(Value::as_bool) == Some(true);
    if marked("isSynthetic") || marked("isMeta") {
        return None;
    }
    let content = message.pointer("/message/content")?;
    let text = content.as_str().map_or_else(
        || {
            let texts =
                content_blocks(message).filter(|block| kind_of(block, "type") == Some("text"));
            texts
                .map(|block| str_of(block, "text"))
                .collect::<Vec<_>>()
                .join("\n")
        },
        String::from,
    );
    (!text.is_empty()).then_some(AgentEvent::Heard { text })
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
    fn what_a_sub_agent_says_and_does_is_kept_apart_under_the_call_that_started_it() {
        let pending = Pending::default();
        let said = parse_message(
            &json!({
                "type": "assistant", "parent_tool_use_id": "toolu_agent",
                "message": { "content": [
                    { "type": "text", "text": "Looking." },
                    { "type": "tool_use", "id": "t1", "name": "Grep", "input": {} },
                ] },
            }),
            &pending,
        );
        let inner: Vec<&AgentEvent> = said
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Sub { parent, event } if parent == "toolu_agent" => Some(&**event),
                _ => None,
            })
            .collect();
        assert!(matches!(
            inner.as_slice(),
            [AgentEvent::Text { text }, AgentEvent::ToolUse { name, .. }]
                if text == "Looking." && name == "Grep"
        ));
        let result = parse_message(
            &json!({
                "type": "user", "parent_tool_use_id": "toolu_agent",
                "message": { "content": [
                    { "type": "tool_result", "tool_use_id": "t1", "content": "found" },
                ] },
            }),
            &pending,
        );
        assert!(matches!(result.as_slice(), [AgentEvent::Sub { .. }]));
        // The agent's own messages name no such call, or name none, and stay as they are.
        let own = parse_message(
            &json!({
                "type": "assistant", "parent_tool_use_id": null,
                "message": { "content": [{ "type": "text", "text": "Mine." }] },
            }),
            &pending,
        );
        assert!(matches!(own.as_slice(), [AgentEvent::Text { text }] if text == "Mine."));
    }

    #[test]
    fn answering_questions_hands_them_back_with_what_was_chosen() {
        let pending = Pending::default();
        let questions = json!([{ "question": "Which one?", "header": "Pick", "multiSelect": false,
            "options": [{ "label": "A", "description": "" }, { "label": "B", "description": "" }] }]);
        parse_message(
            &json!({
                "type": "control_request", "request_id": "q1",
                "request": { "subtype": "can_use_tool", "tool_name": "AskUserQuestion",
                             "input": { "questions": questions } },
            }),
            &pending,
        );
        let answer = AgentCommand::Answer {
            request_id: "q1".into(),
            answers: BTreeMap::from([("Which one?".to_string(), "B".to_string())]),
        };
        let line = command_line(answer, &pending, &mut 0);
        assert_eq!(
            line.pointer("/response/response"),
            Some(&json!({ "behavior": "allow", "updatedInput": {
                "questions": questions, "answers": { "Which one?": "B" } } }))
        );
        assert_eq!(pending.take("q1"), None);
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
        // A prompt such as `/code-review 123` would otherwise run the user's skill.
        assert!(
            args.iter().any(|arg| arg == "--disable-slash-commands"),
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
    fn a_suggested_next_message_is_read_unless_it_is_blank() {
        let suggested = |text: &str| {
            parse(&json!({ "type": "prompt_suggestion", "suggestion": text, "uuid": "u" }))
        };
        let expected = AgentEvent::Suggestion {
            text: "run the tests".into(),
        };
        assert_eq!(suggested(" run the tests\n"), vec![expected]);
        assert_eq!(suggested("  "), vec![]);
        assert_eq!(parse(&json!({ "type": "prompt_suggestion" })), vec![]);
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
        assert_eq!(asked(json!({ "type": "system", "subtype": "init" })), 2);
        assert_eq!(asked(json!({ "type": "result" })), 2);
        assert_eq!(asked(json!({ "type": "assistant" })), 0);
    }

    #[test]
    fn extra_usage_is_asked_at_the_start_and_whenever_it_pays_for_a_turn() {
        let subtypes = |message: Value| -> Vec<Value> {
            questions_after(&message)
                .iter()
                .filter_map(|question| question.pointer("/request/subtype").cloned())
                .collect()
        };
        let report = |info: Value| json!({ "type": "rate_limit_event", "rate_limit_info": info });
        assert!(
            subtypes(json!({ "type": "system", "subtype": "init" })).contains(&json!("get_usage"))
        );
        assert_eq!(
            subtypes(report(
                json!({ "status": "allowed", "isUsingOverage": false })
            )),
            Vec::<Value>::new()
        );
        assert_eq!(
            subtypes(report(
                json!({ "status": "rejected", "isUsingOverage": true })
            )),
            vec![json!("get_usage")]
        );
        assert_eq!(
            subtypes(report(
                json!({ "status": "rejected", "overageInUse": true })
            )),
            vec![json!("get_usage")]
        );
        // Only the plan's figures are wanted: the scan of a week of transcripts is skipped.
        assert_eq!(
            usage_request().pointer("/request/skip_behaviors"),
            Some(&json!(true))
        );
    }

    #[test]
    fn extra_usage_is_read_from_the_answer_and_nothing_else_is_kept() {
        // The shape Claude Code 2.1.330 answers with, cut down; the session and the plan's
        // other figures are there too, and none of them may come through.
        let answer = |request_id: &str, extra: Value| {
            json!({ "type": "control_response", "response": {
                "subtype": "success", "request_id": request_id,
                "response": {
                    "session": { "total_cost_usd": 0 },
                    "subscription_type": "max-secret",
                    "rate_limits": {
                        "five_hour": { "utilization": 36, "resets_at": "2026-11-21T19:59:59Z" },
                        "extra_usage": extra,
                    },
                    "behaviors": null,
                },
            }})
        };
        let extra = json!({
            "is_enabled": true, "monthly_limit": 40_000, "used_credits": 3871,
            "utilization": 9.6775, "currency": "EUR", "decimal_places": 2,
            "spend_limit_reached": false, "user_disabled": false,
        });
        let events = parse(&answer(USAGE_REQUEST, extra.clone()));
        assert_eq!(
            events,
            vec![AgentEvent::ExtraUsage {
                enabled: true,
                used: 3871,
                limit: Some(40_000),
                currency: "EUR".into(),
                decimals: 2,
                limit_reached: false,
            }]
        );
        assert!(!format!("{events:?}").contains("secret"));
        // Somebody else's answer, a plan without extra usage, and a shape it does not know
        // are nothing.
        assert_eq!(parse(&answer("other", extra)), vec![]);
        assert_eq!(parse(&answer(USAGE_REQUEST, Value::Null)), vec![]);
        assert_eq!(
            parse(&answer(USAGE_REQUEST, json!({ "used_credits": "?" }))),
            vec![]
        );
        // No cap, and a cap reached.
        let uncapped = json!({ "is_enabled": false, "used_credits": 0, "currency": "USD", "spend_limit_reached": true });
        assert_eq!(
            parse(&answer(USAGE_REQUEST, uncapped)),
            vec![AgentEvent::ExtraUsage {
                enabled: false,
                used: 0,
                limit: None,
                currency: "USD".into(),
                decimals: 2,
                limit_reached: true,
            }]
        );
    }

    /// Claude Code's answer to `get_usage`, as 2.1.330 gives it, cut down: three limits, one
    /// of them a single model's, one row it cannot have, extra usage, and figures that must
    /// not come through.
    fn usage_answer(request_id: &str) -> Value {
        json!({ "type": "control_response", "response": {
            "subtype": "success", "request_id": request_id,
            "response": {
                "session": { "total_cost_usd": 0 },
                "subscription_type": "max-secret",
                "rate_limits": {
                    "limits": [
                        { "kind": "session", "group": "session", "percent": 36,
                          "severity": "normal", "resets_at": "2026-11-21T19:59:59.723480+00:00",
                          "scope": null, "is_active": false },
                        { "kind": "weekly_all", "group": "weekly", "percent": 30,
                          "severity": "normal", "resets_at": "2026-11-27T20:59:59+00:00" },
                        { "kind": "weekly_scoped", "group": "weekly", "percent": 92,
                          "severity": "warning", "resets_at": null,
                          "scope": { "model": { "id": null, "display_name": "Fable" } } },
                        { "kind": "broken", "percent": "lots" },
                    ],
                    "extra_usage": { "is_enabled": true, "monthly_limit": 50_000,
                        "used_credits": 1000, "currency": "USD", "decimal_places": 2,
                        "spend_limit_reached": false },
                    "spend": { "used": { "amount_minor": 1000, "currency": "USD" } },
                },
            },
        }})
    }

    /// Claude Code 2.1.330's answer to a command that only opens a screen in its terminal.
    fn refused(text: &str, outcome: Option<&str>) -> Value {
        let mut message = json!({
            "type": "assistant", "parent_tool_use_id": null,
            "message": { "model": "<synthetic>", "role": "assistant",
                "content": [{ "type": "text", "text": text }] },
            "local_command_source": format!("<local-command-stdout>{text}</local-command-stdout>"),
        });
        if let Some(kind) = outcome {
            message["local_command_outcome"] = json!({ "kind": kind });
        }
        message
    }

    #[test]
    fn a_command_with_only_a_terminal_screen_is_told_apart_from_a_reply() {
        let theme = refused(
            "/theme isn't available in this environment.",
            Some("unavailable_headless"),
        );
        let only = |command: &str| AgentEvent::TerminalOnly {
            command: command.to_string(),
        };
        assert_eq!(parse(&theme), vec![only("theme")]);
        // Claude Code's other wording for it, with the command as typed, alias and all.
        let panel = refused(
            "/allowed-tools opens an interactive panel and isn't available in this environment. \
             Run it from the Claude Code terminal instead.",
            Some("unavailable_headless"),
        );
        assert_eq!(parse(&panel), vec![only("allowed-tools")]);
        // Without the mark, or with another, the same words are a reply like any other.
        let said = "/theme isn't available in this environment.";
        for outcome in [None, Some("failed")] {
            let text = AgentEvent::Text {
                text: said.to_string(),
            };
            assert_eq!(parse(&refused(said, outcome)), vec![text]);
        }
        // Marked words that name no command are kept as they are.
        let unnamed = refused("Not here.", Some("unavailable_headless"));
        let text = AgentEvent::Text {
            text: "Not here.".to_string(),
        };
        assert_eq!(parse(&unnamed), vec![text]);
    }

    #[test]
    fn a_limit_whose_share_is_not_a_whole_number_is_still_read() {
        let limit = parse_limit(&json!({ "kind": "session", "percent": 46.4 })).expect("is read");
        assert_eq!(limit.percent, 46);
    }

    /// The figures [`usage_answer`] gives.
    fn usage_read() -> Vec<AgentEvent> {
        let limit = |kind: &str, model: Option<&str>, percent, resets: Option<&str>| UsageLimit {
            kind: kind.into(),
            model: model.map(Into::into),
            percent,
            resets_at: resets.map(Into::into),
            severity: if percent > 90 { "warning" } else { "normal" }.into(),
        };
        vec![
            AgentEvent::UsageLimits {
                limits: vec![
                    limit(
                        "session",
                        None,
                        36,
                        Some("2026-11-21T19:59:59.723480+00:00"),
                    ),
                    limit("weekly_all", None, 30, Some("2026-11-27T20:59:59+00:00")),
                    limit("weekly_scoped", Some("Fable"), 92, None),
                ],
            },
            AgentEvent::ExtraUsage {
                enabled: true,
                used: 1000,
                limit: Some(50_000),
                currency: "USD".into(),
                decimals: 2,
                limit_reached: false,
            },
        ]
    }

    #[test]
    fn usage_asked_for_is_read_as_figures_and_put_nowhere_in_the_conversation() {
        assert_eq!(
            command_line(AgentCommand::AskUsage, &Pending::default(), &mut 0),
            usage_request()
        );
        let events = parse(&usage_answer(USAGE_REQUEST));
        assert_eq!(events, usage_read());
        assert!(!format!("{events:?}").contains("secret"));
        // An error is no figures.
        let error = json!({ "type": "control_response", "response": {
            "subtype": "error", "request_id": USAGE_REQUEST, "error": "not signed in",
        }});
        assert_eq!(parse(&error), vec![]);
    }

    #[test]
    fn the_usage_screen_brings_the_figures_up_to_date_as_well() {
        let screen = screens::request(crate::agent::ScreenKind::Usage);
        let id = screen["request_id"].as_str().expect("an id");
        let events = parse(&usage_answer(id));
        assert!(matches!(&events[0], AgentEvent::Screen { command, .. } if command == "usage"));
        assert_eq!(events[1..], usage_read());
        // Other screens are only themselves.
        let status = screens::request(crate::agent::ScreenKind::Status);
        let id = status["request_id"].as_str().expect("an id");
        assert_eq!(parse(&usage_answer(id)).len(), 1);
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
        let args = args(&spec, Some(Path::new("/private/servers.json"))).join(" ");
        assert!(args.contains("--permission-mode acceptEdits"), "{args}");
        assert!(args.contains("--prompt-suggestions"), "{args}");
        assert!(args.contains("--replay-user-messages"), "{args}");
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
        // The servers are named by the file they are in, never written out.
        assert!(
            args.contains("--mcp-config /private/servers.json"),
            "{args}"
        );
        assert!(!args.contains("mcpServers"), "{args}");
        assert!(!args.contains("--append-system-prompt"), "{args}");
    }

    /// A stand-in for `claude` in `folder`: it notes its arguments, copies the file it is
    /// given for its MCP servers and notes who may read that, then waits for its input to end.
    fn noting_program(folder: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let program = folder.join("claude");
        let script = "#!/bin/sh\nout=$(dirname \"$0\")\nprintf '%s\\n' \"$@\" > \"$out/args\"\n\
            while [ $# -gt 0 ]; do\n  if [ \"$1\" = --mcp-config ]; then\n    \
            cp \"$2\" \"$out/servers\"\n    ls -l \"$2\" | cut -c1-10 > \"$out/mode\"\n  fi\n  \
            shift\ndone\ncat > /dev/null\n";
        std::fs::write(&program, script).expect("writes");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .expect("makes it a program");
        program
    }

    /// Starts `backend`, trying again while the system still counts its program as being
    /// written, which it can for a moment when another test starts a process meanwhile.
    async fn start_noting(backend: &ClaudeBackend, spec: &SessionSpec) -> AgentHandle {
        let mut started = backend.start(spec.clone());
        let mut tries = 0;
        while started.is_err() && tries < 50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            started = backend.start(spec.clone());
            tries += 1;
        }
        started.expect("starts")
    }

    /// Whether `folder` is empty, or becomes so within a couple of seconds.
    async fn emptied(folder: &Path) -> bool {
        let left = || std::fs::read_dir(folder).expect("reads").count();
        let mut waited = 0;
        while left() != 0 && waited < 100 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            waited += 1;
        }
        left() == 0
    }

    #[tokio::test]
    async fn mcp_servers_reach_claude_code_in_a_private_file_that_goes_with_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let private = dir.path().join("run").join("7");
        let secret = BTreeMap::from([("TOKEN".to_string(), "hunter2".to_string())]);
        let spec = SessionSpec {
            cwd: dir.path().to_path_buf(),
            auth: Auth::Inherit,
            mcp_servers: vec![McpServer {
                name: "db".into(),
                transport: McpTransport::Stdio {
                    command: "npx".into(),
                    args: vec![],
                    env: secret,
                },
            }],
            private_dir: Some(private.clone()),
            ..SessionSpec::default()
        };
        // What a run that never cleaned up left in the session's folder goes at the start.
        std::fs::create_dir_all(&private).expect("creates");
        let stale = private.join(format!("{PRIVATE_FILE_PREFIX}1-1.json"));
        std::fs::write(&stale, "{}").expect("writes");
        let backend = ClaudeBackend {
            program: noting_program(dir.path()),
        };
        let mut handle = start_noting(&backend, &spec).await;
        drop(handle.commands);
        let ended = tokio::time::timeout(Duration::from_secs(10), handle.events.recv()).await;
        assert!(
            matches!(ended, Ok(Some(AgentEvent::Exited { code: Some(0), .. }))),
            "{ended:?}"
        );

        let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).expect("noted");
        let args = read("args");
        assert!(!args.contains("hunter2"), "{args}");
        assert!(args.contains(&*private.to_string_lossy()), "{args}");
        assert!(read("servers").contains(r#""TOKEN":"hunter2""#));
        assert_eq!(read("mode").trim(), "-rw-------");
        assert!(!stale.exists());
        // The agent has gone, and its file with it.
        assert!(emptied(&private).await, "the file was left behind");
    }

    #[tokio::test]
    async fn a_missing_claude_code_says_what_to_install() {
        let backend = ClaudeBackend {
            program: "/nonexistent/claude".into(),
        };
        let error = backend
            .start(SessionSpec::default())
            .expect_err("started without a program");
        assert!(
            matches!(&error, Error::ClaudeMissing(program) if program == "/nonexistent/claude")
        );
        let said = error.to_string();
        assert!(
            said.contains("Settings, under Agents") && said.contains("sign in"),
            "{said}"
        );
    }

    #[test]
    fn a_sub_agent_in_the_background_is_tied_to_the_call_that_started_it() {
        let pending = Pending::default();
        let started = json!({
            "type": "system", "subtype": "task_started", "task_id": "a1",
            "description": "Code review of PR 258", "task_type": "local_agent",
            "tool_use_id": "toolu_agent",
        });
        let ended = json!({
            "type": "system", "subtype": "task_notification", "task_id": "a1",
            "status": "completed", "summary": "Agent completed", "tool_use_id": "toolu_agent",
        });
        for (message, state) in [
            (started, BackgroundState::Running),
            (ended, BackgroundState::Completed),
        ] {
            let parsed = parse_message(&message, &pending);
            let tied = matches!(
                parsed.as_slice(),
                [AgentEvent::Background { tool_use_id: Some(id), state: read, .. }]
                    if id == "toolu_agent" && *read == state
            );
            assert!(tied, "{parsed:?}");
        }
        // One recorded before the field was there still reads, as tied to nothing.
        let old = r#"{"type":"background","task_id":"b1","description":"d","kind":"local_bash","state":"running","summary":null}"#;
        let read: AgentEvent = serde_json::from_str(old).expect("reads");
        assert!(matches!(
            read,
            AgentEvent::Background {
                tool_use_id: None,
                ..
            }
        ));
    }

    #[test]
    fn where_a_background_command_writes_is_read_from_its_start_and_its_end() {
        let pending = Pending::default();
        let file = "/private/tmp/claude-501/p/s/tasks/bzufoxos3.output";
        let result = json!({
            "type": "user",
            "message": { "content": [{
                "type": "tool_result", "tool_use_id": "toolu_1",
                "content": format!("Command running in background with ID: bzufoxos3. Output is being written to: {file}."),
            }]},
        });
        let events = parse_message(&result, &pending);
        let running = matches!(
            events.as_slice(),
            [AgentEvent::ToolResult { .. }, AgentEvent::Background { task_id, state: BackgroundState::Running, output_file: Some(named), tool_use_id: Some(call), .. }]
                if task_id == "bzufoxos3" && named == file && call == "toolu_1"
        );
        assert!(running, "{events:?}");
        let ended = json!({
            "type": "system", "subtype": "task_notification", "task_id": "bzufoxos3",
            "status": "completed", "output_file": file,
        });
        let events = parse_message(&ended, &pending);
        let named = matches!(events.as_slice(), [AgentEvent::Background { output_file: Some(named), .. }] if named == file);
        assert!(named, "{events:?}");
        // A command sent there while its call waited on it is worded as Claude Code 2.1.330
        // words it, and marked as moved.
        let manual = json!({
            "type": "user",
            "message": { "content": [{
                "type": "tool_result", "tool_use_id": "toolu_2",
                "content": format!("Command was manually backgrounded by user with ID: b7. Output is being written to: {file}."),
            }]},
        });
        let events = parse_message(&manual, &pending);
        let moved = matches!(
            events.as_slice(),
            [AgentEvent::ToolResult { .. }, AgentEvent::Background { task_id, moved: true, output_file: Some(named), .. }]
                if task_id == "b7" && named == file
        );
        assert!(moved, "{events:?}");
        // Other wording says nothing about where output goes.
        let other = json!({
            "type": "user",
            "message": { "content": [{ "type": "tool_result", "tool_use_id": "t", "content": "Running." }]},
        });
        assert_eq!(parse_message(&other, &pending).len(), 1);
        // Nor does a result that quotes the words, as a file read back or a page would.
        let quoted = json!({
            "type": "user",
            "message": { "content": [{
                "type": "tool_result", "tool_use_id": "t",
                "content": format!("12\tCommand running in background with ID: made-up. Output is being written to: {file}."),
            }]},
        });
        assert_eq!(parse_message(&quoted, &pending).len(), 1);
    }

    #[test]
    fn a_message_said_back_as_claude_code_takes_it_in_is_heard() {
        let pending = Pending::default();
        // As Claude Code 2.1.330 says back a message written in the middle of a turn.
        let replayed = json!({
            "type": "user", "isReplay": true,
            "message": { "role": "user", "content": "Also, at the very end, add the word SECOND." },
        });
        assert_eq!(
            parse_message(&replayed, &pending),
            [AgentEvent::Heard {
                text: "Also, at the very end, add the word SECOND.".into()
            }]
        );
        let blocks = json!({
            "type": "user", "isReplay": true,
            "message": { "content": [{ "type": "text", "text": "a hint" }] },
        });
        let heard = parse_message(&blocks, &pending);
        assert_eq!(
            heard,
            [AgentEvent::Heard {
                text: "a hint".into()
            }]
        );
        // A tool's result is not the user's: it is read as before.
        let result = json!({
            "type": "user",
            "message": { "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": "ok" }] },
        });
        assert!(matches!(
            parse_message(&result, &pending).as_slice(),
            [AgentEvent::ToolResult { .. }]
        ));
        // One Claude Code made up itself is nobody's message.
        let made_up = json!({
            "type": "user", "isReplay": true, "isSynthetic": true,
            "message": { "role": "user", "content": "This session is being continued." },
        });
        assert_eq!(parse_message(&made_up, &pending), []);
        // Nor is one it keeps beside the conversation, such as what a skill of its own says.
        let beside = json!({
            "type": "user", "isReplay": true, "isMeta": true,
            "message": { "role": "user", "content": "Base directory for this skill: /skills/x" },
        });
        assert_eq!(parse_message(&beside, &pending), []);
    }

    #[test]
    fn remote_control_is_asked_for_as_the_agent_sdk_asks_and_its_session_is_kept() {
        let line = |on, reattach: Option<&str>| {
            let command = AgentCommand::RemoteControl {
                on,
                name: "Fix the login".into(),
                reattach: reattach.map(String::from),
            };
            command_line(command, &Pending::default(), &mut 0)
        };
        let fresh = line(true, None);
        assert_eq!(fresh.get("request_id"), Some(&json!(REMOTE_ON_REQUEST)));
        assert_eq!(
            fresh.get("request"),
            Some(&json!({ "subtype": "remote_control", "enabled": true,
                "name": "Fix the login", "keep_session_on_exit": true }))
        );
        let again = line(true, Some("cse_1"));
        assert_eq!(
            again.pointer("/request/reattach_session_id"),
            Some(&json!("cse_1"))
        );
        let off = line(false, Some("cse_1"));
        assert_eq!(
            off.get("request"),
            Some(&json!({ "subtype": "remote_control", "enabled": false }))
        );
        assert_ne!(off.get("request_id"), Some(&json!(REMOTE_ON_REQUEST)));
    }

    #[test]
    fn claude_code_says_where_the_session_is_or_why_it_could_not_be_reached() {
        let pending = Pending::default();
        // As Claude Code 2.1.330 answers: the addresses, and its ids for the session.
        let on = json!({ "type": "control_response", "response": {
            "subtype": "success", "request_id": REMOTE_ON_REQUEST, "response": {
                "session_url": "https://claude.ai/code/cse_1",
                "connect_url": "https://claude.ai/code?environment=env_1",
                "environment_id": "env_1", "bridge_epoch": 1, "bridge_session_id": "cse_1" } } });
        assert_eq!(
            parse_message(&on, &pending),
            [AgentEvent::RemoteControl {
                state: RemoteState::On,
                url: Some("https://claude.ai/code/cse_1".into()),
                remote_id: Some("cse_1".into()),
                detail: None,
            }]
        );
        let refused = json!({ "type": "control_response", "response": {
            "subtype": "error", "request_id": REMOTE_ON_REQUEST,
            "error": "Remote Control requires a claude.ai subscription",
            "error_code": "remote_control_auth" } });
        assert_eq!(
            parse_message(&refused, &pending),
            [remote(
                RemoteState::Failed,
                Some("Remote Control requires a claude.ai subscription")
            )]
        );
        // Turning it off is answered with nothing worth hearing.
        let off = json!({ "type": "control_response", "response": {
            "subtype": "success", "request_id": REMOTE_OFF_REQUEST } });
        assert_eq!(parse_message(&off, &pending), []);
    }

    #[test]
    fn each_change_of_the_connection_to_claude_ai_is_reported() {
        let state = |fields: Value| {
            let message = with_fields(
                json!({ "type": "system", "subtype": "bridge_state",
                    "bridge_epoch": 1, "uuid": "u", "session_id": "s" }),
                fields,
            );
            match parse_message(&message, &Pending::default()).as_slice() {
                [AgentEvent::RemoteControl { state, detail, .. }] => (*state, detail.clone()),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(state(json!({ "state": "ready" })).0, RemoteState::On);
        assert_eq!(
            state(json!({ "state": "connected", "recovered": true })).0,
            RemoteState::On
        );
        assert_eq!(
            state(json!({ "state": "reconnecting", "detail": "network" })),
            (RemoteState::Reconnecting, Some("network".to_string()))
        );
        // A failure is for good unless its kind says otherwise.
        assert_eq!(
            state(json!({ "state": "failed", "detail": "gone", "failure_kind": "terminal" })),
            (RemoteState::Failed, Some("gone".to_string()))
        );
        assert_eq!(
            state(json!({ "state": "failed", "failure_kind": "transient" })).0,
            RemoteState::Reconnecting
        );
        assert_eq!(
            state(json!({ "state": "policy_disabled", "detail": "" })),
            (RemoteState::Failed, None)
        );
    }

    /// `message` with the fields of `more` added.
    fn with_fields(mut message: Value, more: Value) -> Value {
        if let (Some(message), Value::Object(more)) = (message.as_object_mut(), more) {
            message.extend(more);
        }
        message
    }

    /// How Claude Code's report of an end with this status is read.
    fn ended_as(status: &str) -> Option<BackgroundState> {
        let ended = json!({
            "type": "system", "subtype": "task_notification", "task_id": "b1",
            "status": status, "summary": "it ended",
        });
        match parse_message(&ended, &Pending::default()).first() {
            Some(AgentEvent::Background { state, summary, .. }) if summary.is_some() => {
                Some(*state)
            }
            _ => None,
        }
    }

    #[test]
    fn what_claude_code_leaves_running_is_reported_at_its_start_and_its_end() {
        let pending = Pending::default();
        let started = json!({
            "type": "system", "subtype": "task_started", "task_id": "b1",
            "description": "Run the dev server", "task_type": "local_bash",
        });
        assert_eq!(
            parse_message(&started, &pending),
            [AgentEvent::Background {
                task_id: "b1".into(),
                description: "Run the dev server".into(),
                kind: "local_bash".into(),
                state: BackgroundState::Running,
                summary: None,
                tool_use_id: None,
                output_file: None,
                moved: false,
                unstoppable: false,
            }]
        );
        assert_eq!(ended_as("completed"), Some(BackgroundState::Completed));
        assert_eq!(ended_as("stopped"), Some(BackgroundState::Stopped));
        assert_eq!(ended_as("failed"), Some(BackgroundState::Failed));
        // The other things it says about them are not events of their own.
        let changed = json!({ "type": "system", "subtype": "background_tasks_changed" });
        assert_eq!(parse_message(&changed, &pending), []);
    }

    /// Claude Code's report of the start of task `id`, which call `call` started; in the
    /// foreground, its call waiting on it, unless `background`.
    fn task_started(id: &str, call: &str, kind: &str, background: bool) -> Value {
        json!({
            "type": "system", "subtype": "task_started", "task_id": id,
            "description": format!("run {id}"), "task_type": kind, "tool_use_id": call,
            "is_backgrounded": background,
        })
    }

    /// Claude Code's report of a change to task `id`, as Claude Code 2.1.330 sends it.
    fn task_updated(id: &str, patch: &Value) -> Value {
        json!({ "type": "system", "subtype": "task_updated", "task_id": id, "patch": patch })
    }

    #[test]
    fn work_a_call_waits_on_is_reported_only_once_it_moves_to_the_background() {
        let pending = Pending::default();
        let started = task_started("a1", "toolu_agent", "local_agent", false);
        assert_eq!(parse_message(&started, &pending), []);
        // A change that is no move says nothing either.
        let running = task_updated("a1", &json!({ "status": "running" }));
        assert_eq!(parse_message(&running, &pending), []);
        let moved = task_updated("a1", &json!({ "is_backgrounded": true }));
        assert_eq!(
            parse_message(&moved, &pending),
            [AgentEvent::Background {
                task_id: "a1".into(),
                description: "run a1".into(),
                kind: "local_agent".into(),
                state: BackgroundState::Running,
                summary: None,
                tool_use_id: Some("toolu_agent".into()),
                output_file: None,
                moved: true,
                unstoppable: false,
            }]
        );
        // From there on it is work left running, whose end is told.
        let ended = json!({
            "type": "system", "subtype": "task_notification", "task_id": "a1",
            "status": "completed", "summary": "done", "output_file": "",
        });
        let told = parse_message(&ended, &pending);
        let end = matches!(
            told.as_slice(),
            [AgentEvent::Background {
                state: BackgroundState::Completed,
                output_file: None,
                ..
            }]
        );
        assert!(end, "{told:?}");
    }

    #[test]
    fn work_that_ends_where_its_call_waited_is_never_listed_as_left_running() {
        let pending = Pending::default();
        let started = task_started("b1", "toolu_bash", "local_bash", false);
        let finished = task_updated("b1", &json!({ "status": "completed" }));
        let ended = json!({
            "type": "system", "subtype": "task_notification", "task_id": "b1",
            "status": "completed", "summary": "exit code 0",
        });
        for message in [started, finished, ended] {
            assert_eq!(parse_message(&message, &pending), [], "{message}");
        }
        // One that started in the background is reported as before, end and all.
        let background = task_started("b2", "toolu_2", "local_bash", true);
        assert_eq!(parse_message(&background, &pending).len(), 1);
        // A move nobody saw start is still a move, if a bare one.
        let unseen = task_updated("b3", &json!({ "is_backgrounded": true }));
        let bare = matches!(
            parse_message(&unseen, &pending).as_slice(),
            [AgentEvent::Background { task_id, moved: true, state: BackgroundState::Running, .. }]
                if task_id == "b3"
        );
        assert!(bare);
    }

    #[test]
    fn a_call_is_sent_to_the_background_as_the_agent_sdk_sends_it_and_the_answer_is_read() {
        let one = AgentCommand::SendToBackground {
            tool_use_id: Some("toolu_1".into()),
        };
        let line = command_line(one, &Pending::default(), &mut 0);
        assert_eq!(
            line,
            json!({
                "type": "control_request", "request_id": "swarmed-background:toolu_1",
                "request": { "subtype": "background_tasks", "tool_use_id": "toolu_1" },
            })
        );
        let every = AgentCommand::SendToBackground { tool_use_id: None };
        let line = command_line(every, &Pending::default(), &mut 0);
        assert_eq!(line["request"], json!({ "subtype": "background_tasks" }));

        let answer = |request_id: &str, response: Value| {
            let message = json!({ "type": "control_response", "response": with_fields(
                json!({ "request_id": request_id }), response) });
            parse_message(&message, &Pending::default())
        };
        let sent = |call: Option<&str>, moved: bool, refused: Option<&str>| {
            vec![AgentEvent::SentToBackground {
                tool_use_id: call.map(String::from),
                moved,
                refused: refused.map(String::from),
            }]
        };
        let success = |response: Value| json!({ "subtype": "success", "response": response });
        let call = "swarmed-background:toolu_1";
        let went = success(json!({ "backgrounded": true }));
        assert_eq!(answer(call, went), sent(Some("toolu_1"), true, None));
        // Ended already, or in the background already: it did not go.
        let stayed = success(json!({ "backgrounded": false }));
        assert_eq!(answer(call, stayed), sent(Some("toolu_1"), false, None));
        // Asked for every call, Claude Code answers with nothing but that it took the request.
        let taken = answer("swarmed-background", success(json!({})));
        assert_eq!(taken, sent(None, true, None));

        let error = |said: &str| json!({ "subtype": "error", "error": said });
        let disabled = "Background tasks are disabled in this session.";
        assert_eq!(
            answer(call, error(disabled)),
            sent(Some("toolu_1"), false, Some(disabled))
        );
        let older = answer(
            call,
            error("Unsupported control request subtype: background_tasks"),
        );
        let update = matches!(
            older.as_slice(),
            [AgentEvent::SentToBackground { refused: Some(said), .. }] if said.contains("update it")
        );
        assert!(update, "{older:?}");
        // An answer to another request of Swarmed's own is not taken for one.
        assert_eq!(answer("swarmed-backgroundx", success(json!({}))), []);
    }

    #[test]
    fn stopping_what_runs_in_the_background_names_it_to_claude_code() {
        let stop = AgentCommand::StopBackground {
            task_id: "b1".into(),
        };
        let line = command_line(stop, &Pending::default(), &mut 0);
        assert_eq!(
            line.get("request"),
            Some(&json!({ "subtype": "stop_task", "task_id": "b1" }))
        );
    }
}
