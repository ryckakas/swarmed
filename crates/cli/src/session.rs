//! The `swarmed session` commands: run a turn in the terminal and read back history.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::{Subcommand, ValueEnum};
use serde_json::Value;
use swarmed_core::agent::{
    AgentEvent, Asked, Auth, Effort, McpServerInfo, PLAN_TOOL, ScreenRow, ScreenSection,
    Supervision,
};
use swarmed_core::{Core, Session, SessionEvent, SessionOptions, SessionSettings};
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
        /// Ask the agent for its fast mode, where it has one: quicker, at a higher price.
        #[arg(long)]
        fast: bool,
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
    /// Start a new session from what was said in this one. Its agent is given that
    /// conversation with the first message sent to it.
    Fork {
        id: i64,
        /// Take only this message and what was said after it, counting the messages sent into
        /// the session from 0.
        #[arg(long)]
        from: Option<usize>,
        /// Run the new session on this agent instead: a clone of the conversation for
        /// another agent to carry on.
        #[arg(long)]
        agent: Option<String>,
    },
    /// Change the model and supervision a session runs with from now on. Without `--auto`,
    /// `--ask` or `--plan` the session stays supervised as it is.
    Set {
        id: i64,
        /// The model to run on; without it, the agent's own default.
        #[arg(long)]
        model: Option<String>,
        /// How hard the model thinks; without it, the agent's own default.
        #[arg(long, value_parser = effort)]
        effort: Option<Effort>,
        /// Ask the agent for its fast mode, where it has one: quicker, at a higher price.
        #[arg(long)]
        fast: bool,
        /// Let Claude Code's auto mode decide which actions are safe, instead of asking.
        #[arg(long, conflicts_with = "ask")]
        auto: bool,
        /// Ask before commands.
        #[arg(long, conflicts_with = "plan")]
        ask: bool,
        /// Plan mode: the agent only reads, then presents a plan to approve before it acts.
        #[arg(long, conflicts_with = "auto")]
        plan: bool,
    },
    /// Put a session that has not begun on another agent, with that agent's default model.
    Agent { id: i64, agent: String },
    /// Print what the status line script shows for a session.
    Status { id: i64 },
    /// Give a session a new title.
    Rename { id: i64, title: String },
    /// Put a mark on a session: a colour and a few words shown in the sidebar.
    Mark {
        id: i64,
        /// `green`, `blue`, `yellow`, `red` or `purple`; `none` takes the mark off.
        colour: String,
        /// A few words beside the colour.
        #[arg(default_value = "")]
        label: String,
    },
    /// Take a session out of the lists. Its history is kept.
    Close { id: i64 },
    /// Put a closed session back in the lists.
    Reopen { id: i64 },
    /// Delete a session and its conversation for good, open or closed. The task's files stay.
    Delete { id: i64 },
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

/// Puts a mark on a session, or takes it off, and prints what it then has.
fn mark(core: &Core, id: i64, colour: &str, label: &str) -> Result<()> {
    let marked = core.session_mark_named(id, colour, label)?;
    say(&marked.mark.map_or_else(
        || "no mark".to_string(),
        |mark| format!("{}\t{}", mark.colour.as_str(), mark.label),
    ));
    Ok(())
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
            fast,
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
                fast,
            };
            let session = core.session_new(&project, &task, &options)?;
            say(&format!("session {}", session.id));
            run_turn(core, session.id, &prompt, &[], auth.resolve()?).await
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
            run_turn(core, id, &prompt, &files, auth.resolve()?).await
        }
        SessionCommand::List { project, task } => {
            for s in core.sessions(&project, &task)? {
                let mode = s.supervision.as_str();
                let state = state_of(&s);
                say(&format!("{}\t{state}\t{mode}\t{}", s.id, s.title));
            }
            Ok(())
        }
        SessionCommand::Fork { id, from, agent } => {
            let fork = core.session_fork(id, from, agent.as_deref())?;
            say(&format!("session {}\t{}", fork.id, fork.title));
            Ok(())
        }
        SessionCommand::Show { id } => show(core, id),
        SessionCommand::Set {
            id,
            model,
            effort,
            fast,
            auto,
            ask,
            plan,
        } => {
            let chosen = (auto || ask || plan).then(|| supervision(auto, plan));
            let supervision = chosen_or_kept(core, id, chosen)?;
            let settings = SessionSettings {
                model,
                supervision,
                effort,
                fast,
            };
            core.session_configure(id, &settings)?;
            Ok(())
        }
        SessionCommand::Status { id } => {
            let line = core.status_line(id).await?;
            say(line.as_deref().unwrap_or("no status_command is set"));
            Ok(())
        }
        SessionCommand::Agent { id, agent } => {
            say(&core.session_set_agent(id, &agent)?.agent);
            Ok(())
        }
        SessionCommand::Rename { id, title } => renamed(core, id, &title),
        SessionCommand::Mark { id, colour, label } => mark(core, id, &colour, &label),
        SessionCommand::Close { id } => Ok(core.session_close(id)?),
        SessionCommand::Delete { id } => Ok(core.session_delete(id)?),
        SessionCommand::Reopen { id } => {
            say(&core.session_reopen(id)?.title);
            Ok(())
        }
    }
}

/// Prints everything a session has said and been told, which counts as having read it.
fn show(core: &Core, id: i64) -> Result<()> {
    core.session_mark_read(id)?;
    core.session_events(id)?
        .iter()
        .filter_map(describe)
        .for_each(|line| say(&line));
    Ok(())
}

/// How a session is supervised from now on: as `chosen` on the command line, or, with none
/// of `--auto`, `--ask` and `--plan` given, as it is. A change of model or effort alone must
/// not turn a session that decides for itself into one that asks.
fn chosen_or_kept(core: &Core, id: i64, chosen: Option<Supervision>) -> Result<Supervision> {
    match chosen {
        Some(supervision) => Ok(supervision),
        None => Ok(core.session(id)?.supervision),
    }
}

fn renamed(core: &Core, id: i64, title: &str) -> Result<()> {
    say(&core.session_rename(id, title)?.title);
    Ok(())
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

/// How the turn ended, if `event` ends it. A turn that was stopped or failed, and an agent
/// that exited with a failure, are errors, so that the command's exit code says so to
/// whatever script ran it.
fn ending(event: &AgentEvent) -> Option<Result<()>> {
    match event {
        AgentEvent::TurnDone { is_error: true, .. } => {
            Some(Err(anyhow!("the turn was stopped or failed")))
        }
        AgentEvent::Exited {
            code: Some(code), ..
        } if *code != 0 => Some(Err(anyhow!("the agent exited with code {code}"))),
        // The last three are a command of the agent's that Swarmed answered itself: there is
        // no turn to wait for.
        AgentEvent::TurnDone { .. }
        | AgentEvent::Exited { .. }
        | AgentEvent::Screen { .. }
        | AgentEvent::CommandAsked { .. }
        | AgentEvent::CommandAnswered { .. } => Some(Ok(())),
        _ => None,
    }
}

/// Sends `prompt`, with `files` listed under it, and shows the session's events until the
/// turn ends. Permission requests are answered from stdin; Ctrl-C interrupts the turn instead
/// of killing the session.
async fn run_turn(core: &Core, id: i64, prompt: &str, files: &[PathBuf], auth: Auth) -> Result<()> {
    let mut bus = core.subscribe();
    core.session_message(id, prompt, files, auth)?;
    let mut answers = BufReader::new(tokio::io::stdin()).lines();
    // The user's own message is the first event published; it is on screen already. A
    // command Swarmed answers itself is no message, and what comes first is its answer.
    let mut first = true;
    while let Some(event) = next_or_interrupt(core, &mut bus, id).await? {
        let own = first && matches!(event, AgentEvent::UserMessage { .. });
        first = false;
        if let Some(line) = describe(&event).filter(|_| !own) {
            say(&line);
        }
        if let AgentEvent::PermissionRequest { request_id, .. } = &event {
            decide(core, id, &mut answers, request_id).await?;
        }
        // The line above has said how it went; the error only carries it to the exit code.
        if let Some(ended) = ending(&event) {
            return ended;
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
        AgentEvent::Screen {
            title,
            note,
            sections,
            ..
        } => Some(screen(title, note, sections)),
        AgentEvent::CommandAsked {
            command, questions, ..
        } => Some(asked(command, questions)),
        AgentEvent::CommandAnswered { said, .. } => Some(said.clone()),
        _ => None,
    }
}

/// One of the agent's screens as lines of text: its heading, then each part's rows.
fn screen(title: &str, note: &str, sections: &[ScreenSection]) -> String {
    let row = |row: &ScreenRow| {
        let named = [row.label.as_str(), row.value.as_str()];
        let line = named.into_iter().filter(|part| !part.is_empty());
        let line = line.collect::<Vec<_>>().join(": ");
        let resets = row.resets_at.as_ref();
        let resets = resets.map_or_else(String::new, |at| format!(" (resets {at})"));
        format!("  {line}{resets}")
    };
    let part = |section: &ScreenSection| {
        let heading = (!section.title.is_empty()).then(|| section.title.clone());
        let rows = section.rows.iter().map(row);
        heading.into_iter().chain(rows).collect::<Vec<_>>()
    };
    let head = [title, note].into_iter().filter(|line| !line.is_empty());
    let head = head.map(String::from);
    let lines = head.chain(sections.iter().flat_map(part));
    lines.collect::<Vec<_>>().join("\n")
}

/// What a command asks, with its choices. There is nothing to choose with here, so it says
/// how to name the choice with the command.
fn asked(command: &str, questions: &[Asked]) -> String {
    let one = |asked: &Asked| {
        let labels = asked.options.iter().map(|option| option.label.as_str());
        let labels = labels.collect::<Vec<_>>().join(", ");
        format!("{} ({labels})", asked.question)
    };
    let asked = questions.iter().map(one).collect::<Vec<_>>().join("\n");
    format!("{asked}\nName the choice with the command, as in `/{command} <choice>`.")
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
    fn a_turn_that_went_wrong_ends_the_command_with_an_error() {
        let done = |is_error| AgentEvent::TurnDone {
            is_error,
            result: None,
            cost_usd: None,
            duration_ms: None,
        };
        let exited = |code| AgentEvent::Exited {
            code,
            stderr: String::new(),
        };
        assert!(matches!(ending(&done(false)), Some(Ok(()))));
        assert!(matches!(ending(&done(true)), Some(Err(_))));
        assert!(matches!(ending(&exited(Some(0))), Some(Ok(()))));
        assert!(matches!(ending(&exited(None)), Some(Ok(()))));
        let failed = ending(&exited(Some(2))).expect("ends the turn");
        let said = failed.expect_err("an error").to_string();
        assert_eq!(said, "the agent exited with code 2");
        // Anything else is the turn still going.
        let text = AgentEvent::Text { text: "hi".into() };
        assert!(ending(&text).is_none());
    }

    #[test]
    fn a_finished_turn_shows_the_cost() {
        let done = AgentEvent::TurnDone {
            is_error: false,
            result: None,
            cost_usd: Some(0.0123),
            duration_ms: None,
        };
        assert_eq!(
            describe(&done).as_deref(),
            Some("— done (cost so far $0.0123)")
        );
    }
}
