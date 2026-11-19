//! The `swarmed session` commands: run a turn in the terminal and read back history.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Subcommand, ValueEnum};
use serde_json::Value;
use swarmed_core::agent::{AgentEvent, Auth, Effort, McpServerInfo, PLAN_TOOL, Supervision};
use swarmed_core::{Core, Session, SessionEvent, SessionOptions, SessionSettings, with_files};
use tokio::io::{AsyncBufReadExt, BufReader, Lines, Stdin};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;

use crate::say;

/// How long a tool call's arguments may be when shown on one line.
const SUMMARY_CHARS: usize = 120;

#[derive(Debug, Subcommand)]
pub(crate) enum SessionCommand {
    /// Start a session in a task and run one turn.
    Run {
        project: String,
        task: String,
        prompt: String,
        /// The agent to run on, by its id from `swarmed agents`; defaults to Claude Code.
        #[arg(long)]
        agent: Option<String>,
        /// The model to run on; defaults to the agent's own default.
        #[arg(long)]
        model: Option<String>,
        /// How hard the model thinks: low, medium, high, xhigh or max.
        #[arg(long, value_parser = effort)]
        effort: Option<Effort>,
        /// Let Claude Code's auto mode decide which actions are safe, instead of asking.
        #[arg(long, conflicts_with = "ask")]
        auto: bool,
        /// Ask before commands. Without one of these flags, the default from the settings applies.
        #[arg(long, conflicts_with = "plan")]
        ask: bool,
        /// Plan mode: the agent only reads, then presents a plan to approve before it acts.
        #[arg(long, conflicts_with = "auto")]
        plan: bool,
        #[arg(long, value_enum, default_value_t)]
        auth: AuthMode,
    },
    /// Continue a session with another turn.
    Send {
        id: i64,
        prompt: String,
        /// A file or folder to point the agent at; may be given more than once.
        #[arg(long = "file")]
        files: Vec<PathBuf>,
        #[arg(long, value_enum, default_value_t)]
        auth: AuthMode,
    },
    /// List a task's sessions.
    List { project: String, task: String },
    /// Print everything a session has said and been told.
    Show { id: i64 },
    /// Change the model and supervision a session runs with from now on.
    Set {
        id: i64,
        /// The model to run on; without it, the agent's own default.
        #[arg(long)]
        model: Option<String>,
        /// How hard the model thinks; without it, the agent's own default.
        #[arg(long, value_parser = effort)]
        effort: Option<Effort>,
        /// Let Claude Code's auto mode decide which actions are safe, instead of asking.
        #[arg(long)]
        auto: bool,
        /// Plan mode: the agent only reads, then presents a plan to approve before it acts.
        #[arg(long, conflicts_with = "auto")]
        plan: bool,
    },
    /// Print what the status line script shows for a session.
    Status { id: i64 },
    /// Give a session a new title.
    Rename { id: i64, title: String },
    /// Take a session out of the lists. Its history is kept.
    Close { id: i64 },
    /// Put a closed session back in the lists.
    Reopen { id: i64 },
}

/// Where the agent's credentials come from.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub(crate) enum AuthMode {
    /// Claude Code's own stored login.
    #[default]
    Subscription,
    /// The key in `ANTHROPIC_API_KEY`.
    ApiKey,
    /// Pass the environment through unchanged (Bedrock, Vertex).
    Inherit,
}

impl AuthMode {
    fn resolve(self) -> Result<Auth> {
        Ok(match self {
            Self::Subscription => Auth::Subscription,
            Self::Inherit => Auth::Inherit,
            Self::ApiKey => Auth::ApiKey(
                std::env::var("ANTHROPIC_API_KEY")
                    .context("--auth api-key needs ANTHROPIC_API_KEY to be set")?,
            ),
        })
    }
}

pub(crate) async fn session(core: &Core, command: SessionCommand) -> Result<()> {
    match command {
        SessionCommand::Run {
            project,
            task,
            prompt,
            agent,
            model,
            effort,
            auto,
            ask,
            plan,
            auth,
        } => {
            let chosen = (auto || ask || plan).then(|| supervision(auto, plan));
            let options = SessionOptions {
                agent,
                title: None,
                model,
                supervision: chosen.unwrap_or(core.settings()?.default_supervision),
                effort,
            };
            let session = core.session_new(&project, &task, &options)?;
            say(&format!("session {}", session.id));
            run_turn(core, session.id, &prompt, auth.resolve()?).await
        }
        SessionCommand::Send {
            id,
            prompt,
            files,
            auth,
        } => {
            // A path typed in a terminal is meant from where the terminal is.
            let files: Vec<PathBuf> = files
                .iter()
                .map(std::path::absolute)
                .collect::<std::io::Result<_>>()?;
            let prompt = with_files(&prompt, &files)?;
            run_turn(core, id, &prompt, auth.resolve()?).await
        }
        SessionCommand::List { project, task } => {
            for s in core.sessions(&project, &task)? {
                let mode = s.supervision.as_str();
                let state = state_of(&s);
                say(&format!("{}\t{state}\t{mode}\t{}", s.id, s.title));
            }
            Ok(())
        }
        SessionCommand::Show { id } => {
            core.session_mark_read(id)?;
            core.session_events(id)?
                .iter()
                .filter_map(describe)
                .for_each(|line| say(&line));
            Ok(())
        }
        SessionCommand::Set {
            id,
            model,
            effort,
            auto,
            plan,
        } => {
            let settings = SessionSettings {
                model,
                supervision: supervision(auto, plan),
                effort,
            };
            core.session_configure(id, &settings)?;
            Ok(())
        }
        SessionCommand::Status { id } => {
            let line = core.status_line(id).await?;
            say(line.as_deref().unwrap_or("no status_command is set"));
            Ok(())
        }
        SessionCommand::Rename { id, title } => {
            say(&core.session_rename(id, &title)?.title);
            Ok(())
        }
        SessionCommand::Close { id } => Ok(core.session_close(id)?),
        SessionCommand::Reopen { id } => {
            say(&core.session_reopen(id)?.title);
            Ok(())
        }
    }
}

/// What the list says a session is doing. An answer nobody has read yet says more than "idle".
fn state_of(session: &Session) -> &'static str {
    if session.unread {
        "unread"
    } else {
        session.status.as_str()
    }
}

/// Reads `--effort`.
fn effort(value: &str) -> std::result::Result<Effort, String> {
    Effort::parse(value).ok_or_else(|| "use low, medium, high, xhigh or max".to_string())
}

fn supervision(auto: bool, plan: bool) -> Supervision {
    match (auto, plan) {
        (true, _) => Supervision::Auto,
        (_, true) => Supervision::Plan,
        _ => Supervision::Ask,
    }
}

/// Sends `prompt` and shows the session's events until the turn ends. Permission requests are
/// answered from stdin; Ctrl-C interrupts the turn instead of killing the session.
async fn run_turn(core: &Core, id: i64, prompt: &str, auth: Auth) -> Result<()> {
    let mut bus = core.subscribe();
    core.session_start(id, auth)?;
    core.session_send(id, prompt)?;
    let mut answers = BufReader::new(tokio::io::stdin()).lines();
    // The user's own message is the first event published; it is on screen already.
    next_or_interrupt(core, &mut bus, id).await?;
    while let Some(event) = next_or_interrupt(core, &mut bus, id).await? {
        if let Some(line) = describe(&event) {
            say(&line);
        }
        match event {
            AgentEvent::PermissionRequest { request_id, .. } => {
                decide(core, id, &mut answers, &request_id).await?;
            }
            AgentEvent::TurnDone { .. } | AgentEvent::Exited { .. } => break,
            _ => {}
        }
    }
    Ok(())
}

/// The session's next event, or `None` if the event stream ended. Ctrl-C interrupts the turn
/// and keeps waiting.
async fn next_or_interrupt(
    core: &Core,
    bus: &mut Receiver<SessionEvent>,
    id: i64,
) -> Result<Option<AgentEvent>> {
    loop {
        tokio::select! {
            published = bus.recv() => match published {
                Ok(published) if published.session_id == id => return Ok(Some(published.event)),
                // Another session's event, or events lost to a full buffer: keep reading.
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(None),
            },
            _ = tokio::signal::ctrl_c() => core.session_interrupt(id)?,
        }
    }
}

/// Reads one line; anything but `y` or `yes` refuses, and so does a closed stdin.
async fn decide(
    core: &Core,
    id: i64,
    answers: &mut Lines<BufReader<Stdin>>,
    request_id: &str,
) -> Result<()> {
    let answer = answers.next_line().await?.unwrap_or_default();
    if matches!(answer.trim(), "y" | "yes") {
        core.session_allow(id, request_id)?;
    } else {
        core.session_deny(id, request_id, "The user declined this action.")?;
    }
    Ok(())
}

/// The line an event is shown as, or `None` for events not worth a line.
fn describe(event: &AgentEvent) -> Option<String> {
    match event {
        AgentEvent::UserMessage { text } => Some(format!("> {text}")),
        AgentEvent::Init {
            model,
            permission_mode,
            api_key_source,
            mcp_servers,
            skills,
            ..
        } => {
            let login = if api_key_source == "none" {
                "subscription"
            } else {
                api_key_source
            };
            let servers = mcp_summary(mcp_servers);
            Some(format!(
                "[{model} · {permission_mode} · login: {login} · MCP servers: {servers} · \
                 skills: {skills}]"
            ))
        }
        AgentEvent::Text { text } => Some(text.clone()),
        AgentEvent::Suggestion { text } => Some(format!("↳ suggested next: {text}")),
        // The plan is printed in full with the question that follows.
        AgentEvent::ToolUse { name, .. } if name == PLAN_TOOL => None,
        AgentEvent::ToolUse { name, input, .. } => Some(format!("▸ {name} {}", summary(input))),
        AgentEvent::ToolResult {
            content,
            is_error: true,
            ..
        } => Some(format!(
            "  ✗ {}",
            content.lines().next().unwrap_or_default()
        )),
        AgentEvent::PermissionRequest {
            tool_name, input, ..
        } if tool_name == PLAN_TOOL => {
            let plan = input
                .get("plan")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Some(format!("{}\n? approve this plan [y/N]", plan.trim_end()))
        }
        AgentEvent::PermissionRequest {
            tool_name, input, ..
        } => Some(format!("? allow {tool_name}: {} [y/N]", summary(input))),
        AgentEvent::RateLimit {
            status, resets_at, ..
        } if status != "allowed" => Some(format!(
            "usage limit: {status}, resets at {}",
            resets_at.unwrap_or_default()
        )),
        AgentEvent::TurnDone {
            is_error, cost_usd, ..
        } => {
            let outcome = if *is_error { "stopped" } else { "done" };
            Some(cost_usd.map_or_else(
                || format!("— {outcome}"),
                |cost| format!("— {outcome} (cost so far ${cost:.4})"),
            ))
        }
        AgentEvent::Exited {
            code: Some(code),
            stderr,
        } if *code != 0 => Some(format!("agent exited with code {code}: {}", stderr.trim())),
        _ => None,
    }
}

/// How many MCP servers a session has. Claude Code reports which connected; other agents are
/// only known to have been given them.
fn mcp_summary(servers: &[McpServerInfo]) -> String {
    let count = |status: &str| {
        servers
            .iter()
            .filter(|server| server.status == status)
            .count()
    };
    match count("passed") {
        0 => format!("{} of {} connected", count("connected"), servers.len()),
        passed => format!("{passed} passed to the agent"),
    }
}

/// The one argument that says what a tool call does, or the arguments as compact JSON.
fn summary(input: &Value) -> String {
    let named = ["command", "file_path", "pattern", "url", "query"]
        .iter()
        .find_map(|key| input.get(key).and_then(Value::as_str));
    let text = named.map_or_else(|| input.to_string(), String::from);
    if text.chars().count() <= SUMMARY_CHARS {
        return text;
    }
    let mut short: String = text.chars().take(SUMMARY_CHARS - 1).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tool_calls_are_summarised_by_their_telling_argument() {
        assert_eq!(
            summary(&json!({ "command": "ls", "description": "List" })),
            "ls"
        );
        assert_eq!(summary(&json!({ "a": 1 })), r#"{"a":1}"#);
        assert_eq!(
            summary(&json!({ "command": "x".repeat(500) }))
                .chars()
                .count(),
            SUMMARY_CHARS
        );
    }

    #[test]
    fn quiet_events_get_no_line() {
        let ok = AgentEvent::ToolResult {
            tool_use_id: "t".into(),
            content: "fine".into(),
            is_error: false,
        };
        assert_eq!(describe(&ok), None);
        let allowed = AgentEvent::RateLimit {
            status: "allowed".into(),
            resets_at: None,
            raw: json!({}),
        };
        assert_eq!(describe(&allowed), None);
    }

    #[test]
    fn mcp_servers_are_counted_as_connected_or_as_passed() {
        let server = |status: &str| McpServerInfo {
            name: "x".into(),
            status: status.into(),
        };
        assert_eq!(
            mcp_summary(&[server("connected"), server("failed")]),
            "1 of 2 connected"
        );
        assert_eq!(
            mcp_summary(&[server("passed"), server("passed")]),
            "2 passed to the agent"
        );
        assert_eq!(mcp_summary(&[]), "0 of 0 connected");
    }

    #[test]
    fn a_finished_turn_shows_the_cost() {
        let done = AgentEvent::TurnDone {
            is_error: false,
            result: None,
            cost_usd: Some(0.0123),
        };
        assert_eq!(
            describe(&done).as_deref(),
            Some("— done (cost so far $0.0123)")
        );
    }
}
