//! The `swarmed session` commands: run a turn in the terminal and read back history.

use std::collections::VecDeque;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::Value;
use swarmed_core::agent::{
    AgentEvent, Asked, Auth, Effort, McpServerInfo, PLAN_TOOL, RemoteState, ScreenRow,
    ScreenSection, Supervision, Tokens,
};
use swarmed_core::{
    Attached, BackgroundCount, Core, SESSION_ENV, Session, SessionEvent, SessionOptions,
    SessionSettings, SettingsReach, nested_sessions,
};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::limits::{seconds_now, span};
use crate::{aside, say};

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
        /// The session the new one works for, by id, or `none`: it reports to that one, and is
        /// listed under it. Without it, the session whose agent runs this command, if it may.
        #[arg(long, value_parser = parent)]
        parent: Option<Parent>,
        /// What the new session is for: architect, planner, implementer or reviewer. Its
        /// agent is told the role's rules, and an architect, a planner or a reviewer starts in
        /// plan mode whatever `--ask` or `--auto` says. Without it, it has no role.
        #[arg(long)]
        role: Option<String>,
        #[arg(long, value_enum, default_value_t)]
        auth: AuthMode,
    },
    /// Continue a session with another turn.
    Send(SendArgs),
    /// List a task's sessions, each after the one it works for, with `for <id>` at the end of
    /// the line of each that works for another, and then `role <role>` for each that has one.
    List { project: String, task: String },
    /// Print everything a session has said and been told.
    Show { id: i64 },
    /// List the turns of a session whose checkpoints are kept, each with the files it changed:
    /// the worktree as it was before its message went, against when it ended.
    Turns { id: i64 },
    /// Print what one turn changed as a diff: of one file, or of every file it changed.
    TurnDiff {
        id: i64,
        /// The message that began the turn, counting the messages sent into the session from 0.
        message: usize,
        /// The file, relative to the worktree; without it, every file the turn changed.
        path: Option<String>,
    },
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
        /// Start it in this task of this project instead of the session's own, with all that
        /// was said: for a chat whose work belongs in a repository.
        #[arg(
            long,
            num_args = 2,
            value_names = ["PROJECT", "TASK"],
            conflicts_with_all = ["from", "agent"]
        )]
        into: Option<Vec<String>>,
    },
    /// Take a session back to before one of its messages: that message and what followed no
    /// longer count, and its agent goes on from there with the next message. With `--files`,
    /// the worktree's files go back to how they were before it too; commits stay.
    Rewind(crate::rewind::RewindArgs),
    /// Put back the files a session's latest rewind took back, as they were before it.
    PutBack { id: i64 },
    /// Change the model and supervision a session runs with from now on. Without `--auto`,
    /// `--ask` or `--plan` the session stays supervised as it is. Says what became of the
    /// change: passed on to the agent the app runs, or taken when the agent next starts.
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
    /// Have a session work for another, of any task of the same repository: it reports to that
    /// one, and is listed under it. `none` has it work for nobody. Both agents are told with
    /// their next message.
    Parent {
        id: i64,
        /// The session it works for, by id, or `none`.
        #[arg(value_parser = parent)]
        parent: Parent,
    },
    /// Give a session a role: architect, planner, implementer or reviewer, or `none` for no
    /// role. Once something was said in it, its agent is told with its next message. An
    /// architect, a planner or a reviewer puts it in plan mode, and taking the role off puts
    /// back the supervision it had, unless another was chosen since.
    Role {
        id: i64,
        /// The role, or `none`.
        role: String,
    },
    /// Move a session among the other open sessions of its task, as the app lists them: one
    /// another session started, among those that one started, and a pinned one among the
    /// pinned.
    Move {
        id: i64,
        /// How many places: negative for earlier, as in `--by=-1`.
        #[arg(long, allow_hyphen_values = true)]
        by: i32,
    },
    /// Pin a session above its task's other open sessions that are not pinned: one another
    /// session started, among those that one started. It keeps its pin while it is closed.
    Pin { id: i64 },
    /// Unpin a session: it goes first among those that are not pinned.
    Unpin { id: i64 },
    /// Highlight a session in a colour of your own, in the sidebar, where it is: writing to it
    /// leaves it, and nothing but you takes it off. Agents cannot set one; a mark is theirs.
    Highlight {
        id: i64,
        /// `green`, `blue`, `yellow`, `red` or `purple`.
        #[arg(required_unless_present = "off")]
        colour: Option<String>,
        /// Take the highlight off instead.
        #[arg(long, conflicts_with = "colour")]
        off: bool,
    },
    /// Put a mark on a session: a colour and a few words shown in the sidebar.
    Mark {
        id: i64,
        /// `green`, `blue`, `yellow`, `red` or `purple`; `none` takes the mark off.
        colour: String,
        /// A few words beside the colour.
        #[arg(default_value = "")]
        label: String,
    },
    /// Let a Claude Code session be reached from claude.ai and the Claude app while this runs,
    /// printing what is said there and here. Ctrl-C ends it; the next agent started for the
    /// session is reached at the same place on claude.ai again.
    RemoteControl {
        id: i64,
        /// Turn it off instead, for good: no later run of the agent turns it on again.
        #[arg(long)]
        off: bool,
        #[arg(long, value_enum, default_value_t)]
        auth: AuthMode,
    },
    /// Let a session a usage limit stopped go on by itself when the limit resets: a message is
    /// sent to it then, as if typed. The app sends it, so it has to be open by then.
    ContinueAtReset {
        id: i64,
        /// Cancel it instead: the session no longer goes on by itself.
        #[arg(long)]
        cancel: bool,
    },
    /// Take a session out of the lists. Its history is kept. The sessions that work for it stay
    /// open, which is said, unless asked to close them too.
    Close {
        id: i64,
        /// Also close the open sessions that work for it, however far down and in whichever
        /// task, the deepest first. If another Swarmed runs any of them, nothing is closed.
        #[arg(long)]
        with_workers: bool,
    },
    /// Put a closed session back in the lists.
    Reopen { id: i64 },
    /// Delete a session and its conversation for good, open or closed. The task's files stay.
    Delete { id: i64 },
}

/// What `swarmed session send` is given.
#[derive(Debug, Args)]
pub(crate) struct SendArgs {
    id: i64,
    prompt: String,
    /// A file or folder to point the agent at; may be given more than once.
    #[arg(long = "file")]
    files: Vec<PathBuf>,
    /// Another session for the agent to read, by id: one this session's agent can reach
    /// with `read_session`. May be given more than once.
    #[arg(long = "session")]
    sessions: Vec<i64>,
    /// A pull request of the session's repository for the agent to read, by number; may
    /// be given more than once. It is read through your `gh` and listed under the message
    /// with its title, state, branches and address.
    #[arg(long = "pull")]
    pulls: Vec<u64>,
    /// An issue of the session's repository, as `--pull` takes a pull request. GitHub
    /// counts both alike: the list under the message says which a number is.
    #[arg(long = "issue")]
    issues: Vec<u64>,
    /// An element picked for this session in its task's browser, by its number; may be given
    /// more than once. It is listed under the message with its address, selector, role, text,
    /// HTML and picture.
    #[arg(long = "element")]
    elements: Vec<u64>,
    #[arg(long, value_enum, default_value_t)]
    auth: AuthMode,
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

/// The session one works for, as `--parent` and `swarmed session parent` take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Parent {
    /// This session.
    Session(i64),
    /// None.
    Nobody,
}

impl Parent {
    fn id(self) -> Option<i64> {
        match self {
            Self::Session(id) => Some(id),
            Self::Nobody => None,
        }
    }
}

/// Reads a session's id, or `none`.
fn parent(value: &str) -> std::result::Result<Parent, String> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(Parent::Nobody);
    }
    value
        .parse()
        .map(Parent::Session)
        .map_err(|_| "use a session's id, or none".to_string())
}

/// Prints a task's sessions, each after the one it works for, which a column names after the
/// columns there always were, and then its role.
fn list(core: &Core, project: &str, task: &str) -> Result<()> {
    let sessions = core.sessions(project, task)?;
    for (s, _) in nested_sessions(&sessions) {
        let mode = s.supervision.as_str();
        let state = state_of(s);
        let parent = s
            .parent
            .map_or_else(String::new, |id| format!("\tfor {id}"));
        let role = s
            .role
            .as_deref()
            .map_or_else(String::new, |role| format!("\trole {role}"));
        say(&format!(
            "{}\t{state}\t{mode}\t{}{parent}{role}",
            s.id, s.title
        ));
    }
    Ok(())
}

/// Reads a role's name, or `none` for no role.
fn role_asked(value: &str) -> Option<&str> {
    (!value.eq_ignore_ascii_case("none")).then_some(value)
}

/// Gives session `id` the role `role`, or `none`, and prints what it has then.
fn role_set(core: &Core, id: i64, role: &str) -> Result<()> {
    let given = core.session_set_role(id, role_asked(role))?;
    say(given.role.as_deref().unwrap_or("no role"));
    Ok(())
}

/// What became of a change of session `id`'s settings, for its agent.
fn reach_said(id: i64, reach: SettingsReach) -> String {
    match reach {
        SettingsReach::Told => format!("told session {id}'s agent"),
        SettingsReach::PassedOn => format!(
            "passed on to session {id}'s agent, which another Swarmed runs: a new model from \
             its next turn, a new effort once its turn is over"
        ),
        SettingsReach::NextStart => {
            format!("session {id}'s agent takes it when it next starts")
        }
    }
}

/// What a session works for, said after it was changed.
fn parent_said(parent: Option<i64>) -> String {
    parent.map_or_else(
        || "works for nobody".to_string(),
        |id| format!("works for session {id}"),
    )
}

/// Has new session `id` work for `asked`, or without that for the session whose agent runs
/// this command (`SWARMED_SESSION`), if any. One asked for and refused fails the command; the
/// calling session refused, such as one of another repository, is said aside and passed over.
/// One asked for is told so with its next message, as a change in the window is; the calling
/// session started it itself, and has nothing to be told.
fn works_for(core: &Core, id: i64, asked: Option<Parent>) -> Result<()> {
    if let Some(asked) = asked {
        core.session_set_parent(id, asked.id())?;
        return Ok(());
    }
    let calling = std::env::var(SESSION_ENV).ok();
    let Some(calling) = calling.and_then(|calling| calling.trim().parse::<i64>().ok()) else {
        return Ok(());
    };
    if let Err(error) = core.session_parent_from_start(id, calling) {
        aside(&format!(
            "not working for session {calling}, which runs this: {error}"
        ));
    }
    Ok(())
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

/// Forks session `id`: from message `from` and on `agent`, or whole into another task.
fn fork(
    core: &Core,
    id: i64,
    (from, agent): (Option<usize>, Option<&str>),
    into: Option<&[String]>,
) -> Result<()> {
    let fork = match into {
        Some([project, task]) => core.session_fork_into(id, project, task)?,
        _ => core.session_fork(id, from, agent)?,
    };
    say(&format!("session {}\t{}", fork.id, fork.title));
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
            parent,
            role,
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
                role: role.as_deref().and_then(role_asked).map(String::from),
            };
            let session = core.session_new(&project, &task, &options)?;
            say(&format!("session {}", session.id));
            works_for(core, session.id, parent)?;
            let nothing = Attached::default();
            run_turn(core, session.id, &prompt, &nothing, auth.resolve()?).await
        }
        SessionCommand::Send(args) => send(core, args).await,
        SessionCommand::List { project, task } => list(core, &project, &task),
        SessionCommand::Fork {
            id,
            from,
            agent,
            into,
        } => fork(core, id, (from, agent.as_deref()), into.as_deref()),
        SessionCommand::Show { id } => show(core, id),
        SessionCommand::Turns { id } => turns(core, id).await,
        SessionCommand::TurnDiff { id, message, path } => turn_diff(core, id, message, path).await,
        SessionCommand::Rewind(args) => crate::rewind::rewind(core, args).await,
        SessionCommand::PutBack { id } => crate::rewind::put_back(core, id).await,
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
            say(&reach_said(id, core.session_settings_reach(id)?));
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
        SessionCommand::Parent { id, parent } => {
            say(&parent_said(
                core.session_set_parent(id, parent.id())?.parent,
            ));
            Ok(())
        }
        SessionCommand::Role { id, role } => role_set(core, id, &role),
        SessionCommand::Move { id, by } => moved(core, id, by),
        SessionCommand::Pin { id } => pinned(core, id, true),
        SessionCommand::Unpin { id } => pinned(core, id, false),
        SessionCommand::Highlight { id, colour, .. } => highlighted(core, id, colour.as_deref()),
        SessionCommand::Mark { id, colour, label } => mark(core, id, &colour, &label),
        SessionCommand::RemoteControl { id, off, auth } => {
            remote_control(core, id, off, auth).await
        }
        SessionCommand::ContinueAtReset { id, cancel } => continue_at_reset(core, id, cancel),
        SessionCommand::Close { id, with_workers } => closed(core, id, with_workers),
        SessionCommand::Delete { id } => Ok(core.session_delete(id)?),
        SessionCommand::Reopen { id } => reopened(core, id),
    }
}

/// Prints each kept turn of a session, oldest first: its message's number and first words,
/// whether its end was recorded, who else was at work meanwhile, and the files it changed.
async fn turns(core: &Core, id: i64) -> Result<()> {
    for turn in core.session_turns(id)? {
        let open = if turn.ended {
            ""
        } else {
            "\t(no end yet: up to now)"
        };
        say(&format!("{}\t{}{open}", turn.message, turn.words));
        if !turn.beside.is_empty() {
            say(&format!("\talso at work: {}", turn.beside.join(", ")));
        }
        let changes = core.session_turn_changes(id, turn.message).await?;
        for file in &changes.files {
            let counts = file.added.zip(file.removed).map_or_else(
                || "binary".to_string(),
                |(added, removed)| format!("+{added} -{removed}"),
            );
            say(&format!(
                "\t{}\t{counts}\t{}",
                kind_mark(file.kind),
                file.path
            ));
        }
        for file in &changes.skipped {
            say(&format!(
                "\tnot kept, {} bytes, over the limit for files git does not track\t{}",
                file.bytes, file.path
            ));
        }
    }
    Ok(())
}

pub(crate) fn kind_mark(kind: swarmed_core::ChangeKind) -> &'static str {
    match kind {
        swarmed_core::ChangeKind::Added => "A",
        swarmed_core::ChangeKind::Modified => "M",
        swarmed_core::ChangeKind::Deleted => "D",
    }
}

/// Prints the diff of one turn: of `path`, or of every file the turn changed.
async fn turn_diff(core: &Core, id: i64, message: usize, path: Option<String>) -> Result<()> {
    let paths = if let Some(path) = path {
        vec![path]
    } else {
        let changes = core.session_turn_changes(id, message).await?;
        changes.files.into_iter().map(|file| file.path).collect()
    };
    for path in paths {
        let diff = core.session_turn_diff(id, message, &path).await?;
        say(diff.text.trim_end());
        if diff.truncated {
            say("[the diff is longer; the rest is not shown]");
        }
    }
    Ok(())
}

/// Closes a session, with the ones that work for it if asked, and says aside which of those
/// it left open.
fn closed(core: &Core, id: i64, with_workers: bool) -> Result<()> {
    let closed = core.session_close(id, with_workers)?;
    if closed.len() > 1 {
        say(&format!(
            "closed {} sessions: {}",
            closed.len(),
            ids_said(&closed)
        ));
    }
    let left: Vec<i64> = core
        .session_descendants(id, false)?
        .iter()
        .map(|one| one.session.id)
        .collect();
    if let Some(said) = left_open(id, &left) {
        aside(&said);
    }
    Ok(())
}

/// Session ids as a list in words: `4, 7 and 9`.
pub(crate) fn ids_said(ids: &[i64]) -> String {
    let words: Vec<String> = ids.iter().map(ToString::to_string).collect();
    match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => words.join(""),
    }
}

/// What is said aside when closing session `id` left the sessions `left` open, that work for
/// it; `None` for none.
fn left_open(id: i64, left: &[i64]) -> Option<String> {
    let (count, verb) = match left.len() {
        0 => return None,
        1 => ("1 session".to_string(), "works"),
        n => (format!("{n} sessions"), "work"),
    };
    Some(format!(
        "{count} still {verb} for session {id}, open: {}. `swarmed session close {id} \
         --with-workers` closes them too.",
        ids_said(left)
    ))
}

/// Puts a closed session back in the lists and prints its title.
fn reopened(core: &Core, id: i64) -> Result<()> {
    say(&core.session_reopen(id)?.title);
    Ok(())
}

/// Prints everything a session has said and been told, which counts as having read it, and
/// when it goes on by itself, if a usage limit stopped it and it is to.
fn show(core: &Core, id: i64) -> Result<()> {
    core.session_mark_read(id)?;
    for line in shown(&core.session_events(id)?) {
        say(&line);
    }
    let session = core.session(id)?;
    if let Some(at) = session.continue_at {
        say(&continues(at, seconds_now()));
    }
    if let Some(running) = in_background(session.background) {
        say(&running);
    }
    Ok(())
}

/// What is said of what a session's agent has running in the background, if anything.
fn in_background(count: BackgroundCount) -> Option<String> {
    let counted = |n: u32, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    let parts: Vec<String> = [
        counted(count.agents, "sub-agent", "sub-agents"),
        counted(count.commands, "command", "commands"),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| format!("{} in the background", parts.join(", ")))
}

/// Sets a session to go on by itself when the usage limit that stopped it resets, or cancels
/// that, and says how it stands.
fn continue_at_reset(core: &Core, id: i64, cancel: bool) -> Result<()> {
    if cancel {
        core.session_continue_cancel(id)?;
        say("does not go on by itself");
        return Ok(());
    }
    let at = core.session_continue_at_reset(id)?;
    say(&continues(at, seconds_now()));
    Ok(())
}

/// What is said of a session that goes on by itself at `at`.
fn continues(at: i64, now: i64) -> String {
    let when = if at > now {
        format!("in {}", span(at - now))
    } else {
        "as soon as the app looks".to_string()
    };
    format!("goes on by itself {when}, when the usage limit resets: the app sends it")
}

/// The lines a session's history is shown as. Claude Code says what it loaded at the start of
/// every turn, and only the first is shown, as in the window.
fn shown(events: &[AgentEvent]) -> Vec<String> {
    let mut started = false;
    let first = |event: &&AgentEvent| {
        let init = matches!(event, AgentEvent::Init { .. });
        let again = init && started;
        started |= init;
        !again
    };
    events.iter().filter(first).filter_map(describe).collect()
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

fn moved(core: &Core, id: i64, by: i32) -> Result<()> {
    core.session_move(id, by)?;
    say("moved");
    Ok(())
}

fn pinned(core: &Core, id: i64, pinned: bool) -> Result<()> {
    core.session_pin(id, pinned)?;
    say(if pinned { "pinned" } else { "unpinned" });
    Ok(())
}

/// Highlights session `id` in `colour`, or takes its highlight off for none.
fn highlighted(core: &Core, id: i64, colour: Option<&str>) -> Result<()> {
    let session = core.session_highlight_named(id, colour)?;
    say(&crate::highlight_said(session.highlight));
    Ok(())
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

/// What, typed on its own line while a turn runs, sends the work the agent waits on to the
/// background.
const TO_BACKGROUND: &str = "bg";

/// The lines typed on the terminal while a turn runs. [`TO_BACKGROUND`] is acted on as it is
/// typed; any other line answers the next question, as it did when nothing was read until a
/// question came.
///
/// Stdin is read on a thread of its own, never by tokio: a read of a terminal cannot be given
/// up, and tokio waits for one it started before the program may end. The command would then
/// stay, its turn over, until Enter was pressed. A thread left reading ends with the program.
struct Typed {
    lines: UnboundedReceiver<std::io::Result<String>>,
    /// Lines typed before a question came, first typed first.
    ahead: VecDeque<String>,
    /// Whether stdin may still have more: a closed one would be read for ever.
    open: bool,
}

impl Typed {
    fn new() -> Self {
        let (typed, lines) = unbounded_channel();
        // Not waited for: it ends when stdin does, when nobody listens, or with the program.
        drop(std::thread::spawn(move || pass_on_stdin(&typed)));
        Self {
            lines,
            ahead: VecDeque::new(),
            open: true,
        }
    }

    /// The next line read from stdin; `None` once it is closed.
    async fn read(&mut self) -> Result<Option<String>> {
        Ok(self.lines.recv().await.transpose()?)
    }

    /// The next line typed, those typed ahead first; `None` once stdin is closed.
    async fn next(&mut self) -> Result<Option<String>> {
        if let Some(line) = self.ahead.pop_front() {
            return Ok(Some(line));
        }
        if !self.open {
            return Ok(None);
        }
        let line = self.read().await?;
        self.open = line.is_some();
        Ok(line)
    }
}

/// Hands each line of stdin to `typed`, until stdin ends or nobody is left to hear.
fn pass_on_stdin(typed: &UnboundedSender<std::io::Result<String>>) {
    for line in std::io::stdin().lines() {
        if typed.send(line).is_err() {
            return;
        }
    }
}

/// `files` as typed in a terminal, which means them from where the terminal is.
fn from_here(files: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let files = files.iter().map(std::path::absolute);
    Ok(files.collect::<std::io::Result<_>>()?)
}

/// Sends a message as [`run_turn`] does, the pull requests and issues it mentions read first:
/// the message then waits on nothing.
async fn send(core: &Core, args: SendArgs) -> Result<()> {
    let SendArgs {
        id,
        prompt,
        files,
        sessions,
        pulls,
        issues,
        elements,
        auth,
    } = args;
    let attached = Attached {
        files: from_here(&files)?,
        sessions,
        github: [pulls, issues].concat(),
        elements,
    };
    if !attached.github.is_empty() {
        core.session_github_read(id, &attached.github).await?;
    }
    run_turn(core, id, &prompt, &attached, auth.resolve()?).await
}

/// Sends `prompt`, with what is `attached` listed under it, and shows the session's events
/// until the turn ends. Permission requests are answered from stdin, and `bg` there sends what
/// the agent waits on to the background; Ctrl-C interrupts the turn instead of killing the
/// session.
async fn run_turn(
    core: &Core,
    id: i64,
    prompt: &str,
    attached: &Attached,
    auth: Auth,
) -> Result<()> {
    let mut bus = core.subscribe();
    core.session_message(id, prompt, attached, auth)?;
    follow_turn(core, id, &mut bus).await
}

/// Shows the events of session `id` heard on `bus` until its turn ends, answering what the
/// agent asks from stdin as [`run_turn`] does. The user's message that starts the turn is
/// left out when it comes first: it is on screen already.
pub(crate) async fn follow_turn(
    core: &Core,
    id: i64,
    bus: &mut Receiver<SessionEvent>,
) -> Result<()> {
    let mut typed = Typed::new();
    // The user's own message is the first event published; it is on screen already. A
    // command Swarmed answers itself is no message, and what comes first is its answer.
    let mut first = true;
    while let Some(event) = next_or_interrupt(core, bus, id, &mut typed).await? {
        let own = first && matches!(event, AgentEvent::UserMessage { .. });
        first = false;
        if let Some(line) = describe(&event).filter(|_| !own) {
            say(&line);
        }
        if let AgentEvent::PermissionRequest { request_id, .. } = &event {
            decide(core, id, &mut typed, request_id).await?;
        }
        // The line above has said how it went; the error only carries it to the exit code.
        if let Some(ended) = ending(&event) {
            return ended;
        }
    }
    Ok(())
}

/// The session's next event, or `None` if the event stream ended. Ctrl-C interrupts the turn
/// and keeps waiting, and so does [`TO_BACKGROUND`], which sends what the agent waits on to
/// the background.
async fn next_or_interrupt(
    core: &Core,
    bus: &mut Receiver<SessionEvent>,
    id: i64,
    typed: &mut Typed,
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
            line = typed.read(), if typed.open => match line? {
                Some(line) if line.trim() == TO_BACKGROUND => to_background(core, id).await,
                Some(line) => typed.ahead.push_back(line),
                None => typed.open = false,
            },
        }
    }
}

/// Sends what the session's agent waits on to the background, as Ctrl+B does in Claude Code's
/// own terminal, and says what came of it.
async fn to_background(core: &Core, id: i64) {
    match core.session_send_to_background(id, None).await {
        Ok(_) => say("↳ anything the agent waited on now goes on in the background"),
        Err(error) => say(&format!("↳ {error}")),
    }
}

/// What turning Remote Control off from the command line leaves as it is.
const OFF_ELSEWHERE: &str = "No later run of the session's agent turns it on. An agent that the \
                             Swarmed app runs for the session now stays reachable until it is \
                             turned off there, or stopped.";

/// Turns Remote Control off, or on and shows the session's events until Ctrl-C, the agent's
/// end, or Remote Control's. A request for permission is left to be answered on claude.ai,
/// where it is shown as well.
async fn remote_control(core: &Core, id: i64, off: bool, auth: AuthMode) -> Result<()> {
    let auth = auth.resolve()?;
    if off {
        // This command runs no agent of the session's: one a running Swarmed has is not
        // reached from here, and saying only "off" would be untrue of it.
        let here = core.running_sessions().contains(&id);
        core.session_remote_control(id, false, auth)?;
        say("Remote Control is off");
        if !here {
            say(OFF_ELSEWHERE);
        }
        return Ok(());
    }
    let mut bus = core.subscribe();
    core.session_remote_control(id, true, auth)?;
    while let Some(event) = next_until_stopped(&mut bus, id).await {
        match &event {
            AgentEvent::PermissionRequest { tool_name, .. } => {
                say(&format!("? {tool_name} waits for an answer on claude.ai"));
            }
            _ => describe(&event).into_iter().for_each(|line| say(&line)),
        }
        if let Some(ended) = remote_ending(&event) {
            return ended;
        }
    }
    Ok(())
}

/// How following a session reached from claude.ai ends, if `event` ends it.
fn remote_ending(event: &AgentEvent) -> Option<Result<()>> {
    match event {
        AgentEvent::RemoteControl {
            state: RemoteState::Failed,
            ..
        } => Some(Err(anyhow!("Remote Control is not on"))),
        AgentEvent::Exited {
            code: Some(code), ..
        } if *code != 0 => Some(Err(anyhow!("the agent exited with code {code}"))),
        AgentEvent::Exited { .. } => Some(Ok(())),
        _ => None,
    }
}

/// The session's next event, or `None` once Ctrl-C is pressed or the event stream ended.
async fn next_until_stopped(bus: &mut Receiver<SessionEvent>, id: i64) -> Option<AgentEvent> {
    loop {
        tokio::select! {
            published = bus.recv() => match published {
                Ok(published) if published.session_id == id => return Some(published.event),
                // Another session's event, or events lost to a full buffer: keep reading.
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return None,
            },
            _ = tokio::signal::ctrl_c() => return None,
        }
    }
}

/// What a report of Remote Control says, if it says something worth a line: not that it was
/// asked for, nor that it is on before Claude Code has said where.
fn remote_line(state: RemoteState, url: Option<&str>, detail: Option<&str>) -> Option<String> {
    let because = detail.map_or_else(String::new, |detail| format!(": {detail}"));
    match state {
        RemoteState::Starting => None,
        RemoteState::On => url.map(|url| format!("Remote Control is on: {url}")),
        RemoteState::Reconnecting => Some(format!("Remote Control is reconnecting{because}")),
        RemoteState::Paused => Some("Remote Control waits for the agent to run again".into()),
        RemoteState::Off => Some("Remote Control is off".into()),
        RemoteState::Failed => Some(format!("Remote Control failed{because}")),
    }
}

/// Reads one line; anything but `y` or `yes` refuses, and so does a closed stdin.
async fn decide(core: &Core, id: i64, typed: &mut Typed, request_id: &str) -> Result<()> {
    let answer = typed.next().await?.unwrap_or_default();
    if matches!(answer.trim(), "y" | "yes") {
        core.session_allow(id, request_id)?;
    } else {
        core.session_deny(id, request_id, "The user declined this action.")?;
    }
    Ok(())
}

/// Whether a usage report says a limit refused the turn: reached, with no extra usage paying.
fn refused(status: &str, raw: &Value) -> bool {
    let set = |key: &str| raw.get(key).and_then(Value::as_bool) == Some(true);
    !matches!(status, "allowed" | "allowed_warning")
        && !set("isUsingOverage")
        && !set("overageInUse")
}

/// The line a message to the session is shown as, with where it came from if not from here:
/// typed on claude.ai, or sent by Swarmed when the usage limit that stopped the session reset.
fn said(text: &str, remote: bool, at_reset: bool) -> String {
    let from = match (remote, at_reset) {
        (true, _) => "[claude.ai] ",
        (_, true) => "[sent at the limit's reset] ",
        _ => "",
    };
    format!("> {from}{text}")
}

/// The line an event is shown as, or `None` for events not worth a line.
pub(crate) fn describe(event: &AgentEvent) -> Option<String> {
    match event {
        AgentEvent::UserMessage {
            text,
            remote,
            at_reset,
        } => Some(said(text, *remote, *at_reset)),
        AgentEvent::RemoteControl {
            state, url, detail, ..
        } => remote_line(*state, url.as_deref(), detail.as_deref()),
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
        // A sub-agent's request names the sub-agent; another agent's says what it is for.
        AgentEvent::PermissionRequest {
            tool_name,
            input,
            description,
            ..
        } => {
            let why = description
                .as_deref()
                .map_or_else(String::new, |why| format!(" ({why})"));
            Some(format!(
                "? allow {tool_name}: {}{why} [y/N]",
                summary(input)
            ))
        }
        // Only a turn a limit refused, as in the window: a warning, or a limit extra usage
        // pays past, is for `swarmed limits`, and Claude Code reports them with every turn.
        AgentEvent::RateLimit {
            status,
            resets_at,
            raw,
        } if refused(status, raw) => Some(format!(
            "usage limit: {status}, resets at {}",
            resets_at.unwrap_or_default()
        )),
        AgentEvent::TurnDone {
            is_error,
            cost_usd,
            tokens,
            ..
        } => {
            let outcome = if *is_error { "stopped" } else { "done" };
            let used = tokens.map_or_else(String::new, |tokens| format!(", {}", used(tokens)));
            Some(cost_usd.map_or_else(
                || format!("— {outcome}{used}"),
                |cost| format!("— {outcome}{used} (cost so far ${cost:.4})"),
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
        _ => described_by_core(event),
    }
}

/// The line of an event the core records by itself or answers for, or `None` for events not
/// worth a line.
/// What a turn used, as the window says it: its tokens in all, then what of them it read anew
/// and what it wrote.
fn used(tokens: Tokens) -> String {
    let all = tokens.input + tokens.output;
    format!(
        "{} tokens ({} in, {} out)",
        short(all),
        short(tokens.input),
        short(tokens.output)
    )
}

/// A token count as people say it, as the window's `formatTokens` says it: 937, 29.6k, 123k,
/// 1.2M.
fn short(tokens: u64) -> String {
    let unit = |size: u64, unit: &str| {
        // Whole units from a hundred on, else tenths: rounded in integers, as no float can
        // say a count wrongly.
        if tokens >= 100 * size {
            return format!("{}{unit}", (tokens + size / 2) / size);
        }
        let tenths = (tokens * 10 + size / 2) / size;
        if tenths.is_multiple_of(10) {
            format!("{}{unit}", tenths / 10)
        } else {
            format!("{}.{}{unit}", tenths / 10, tenths % 10)
        }
    };
    match tokens {
        0..1_000 => tokens.to_string(),
        1_000..1_000_000 => unit(1_000, "k"),
        _ => unit(1_000_000, "M"),
    }
}

fn described_by_core(event: &AgentEvent) -> Option<String> {
    match event {
        AgentEvent::CommandAnswered { said, .. } => Some(said.clone()),
        AgentEvent::TerminalOnly { command } => Some(format!(
            "/{command} only works in Claude Code's own terminal"
        )),
        AgentEvent::PullChanged { number, change, .. } => {
            Some(crate::pull::changed(*number, change))
        }
        AgentEvent::Rewound { to, files, .. } => Some(crate::rewind::rewound(*to, *files)),
        AgentEvent::FilesPutBack { .. } => Some("— the files were put back".to_string()),
        AgentEvent::ParentChanged {
            child,
            parent,
            previous,
        } => Some(regrouped(*child, *parent, *previous)),
        AgentEvent::RoleChanged { role, previous } => {
            let now = role.as_deref().unwrap_or("none");
            let before = previous
                .as_deref()
                .map_or_else(String::new, |before| format!(", no longer {before}"));
            Some(format!("role: {now} from here on{before}"))
        }
        // An app is HTML for a sandboxed frame, which only the window has.
        AgentEvent::App { server, .. } => Some(format!(
            "— the {server} server showed an app, which the Swarmed window draws"
        )),
        _ => None,
    }
}

/// The line of a change of whom session `child` works for, the same in its history and in
/// those of the sessions it worked for and works for now.
fn regrouped(child: i64, parent: Option<i64>, previous: Option<i64>) -> String {
    let now = parent.map_or_else(|| "nobody".to_string(), |id| format!("session {id}"));
    let before = previous.map_or_else(String::new, |id| format!(", no longer for session {id}"));
    format!("— session {child} works for {now} now{before}")
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
    fn sessions_left_open_under_a_closed_one_are_said_with_the_way_to_close_them() {
        assert_eq!(left_open(7, &[]), None);
        assert_eq!(
            left_open(7, &[9]).as_deref(),
            Some(
                "1 session still works for session 7, open: 9. `swarmed session close 7 \
                 --with-workers` closes them too."
            )
        );
        let said = left_open(7, &[9, 11, 12]).expect("said");
        assert!(said.starts_with("3 sessions still work for session 7, open: 9, 11 and 12."));
        assert_eq!(ids_said(&[4, 7]), "4 and 7");
    }

    #[test]
    fn what_runs_in_the_background_is_said_by_kind_and_nothing_is_not() {
        let said = |agents, commands| in_background(BackgroundCount { agents, commands });
        assert_eq!(
            said(2, 1).as_deref(),
            Some("2 sub-agents, 1 command in the background")
        );
        assert_eq!(said(0, 3).as_deref(), Some("3 commands in the background"));
        assert_eq!(said(0, 0), None);
    }

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
    fn a_request_says_who_asks_when_it_is_a_sub_agent() {
        let asked = AgentEvent::PermissionRequest {
            request_id: "1".into(),
            tool_name: "Terminal".into(),
            input: json!({ "command": "cat README.md" }),
            description: Some("Asked by the sub-agent Explorer 1".into()),
        };
        assert_eq!(
            describe(&asked).as_deref(),
            Some("? allow Terminal: cat README.md (Asked by the sub-agent Explorer 1) [y/N]")
        );
    }

    #[test]
    fn a_session_shows_what_its_agent_loaded_once() {
        let init = |model: &str| AgentEvent::Init {
            agent_session_id: "abc".into(),
            model: model.into(),
            permission_mode: "default".into(),
            api_key_source: "none".into(),
            mcp_servers: vec![],
            skills: 3,
            slash_commands: vec![],
            capabilities: vec![],
        };
        let done = AgentEvent::TurnDone {
            is_error: false,
            result: None,
            cost_usd: None,
            duration_ms: None,
            tokens: None,
        };
        let lines = shown(&[
            init("opus"),
            done.clone(),
            init("opus"),
            done.clone(),
            init("sonnet"),
        ]);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].contains("opus"), "{lines:?}");
        assert!(
            lines.iter().skip(1).all(|line| line.starts_with("—")),
            "{lines:?}"
        );
    }

    #[test]
    fn quiet_events_get_no_line() {
        let ok = AgentEvent::ToolResult {
            tool_use_id: "t".into(),
            content: "fine".into(),
            is_error: false,
            images: Vec::new(),
            returned: Vec::new(),
        };
        assert_eq!(describe(&ok), None);
        let allowed = AgentEvent::RateLimit {
            status: "allowed".into(),
            resets_at: None,
            raw: json!({}),
        };
        assert_eq!(describe(&allowed), None);
        let report = |status: &str, raw: Value| AgentEvent::RateLimit {
            status: status.into(),
            resets_at: Some(1_795_813_200),
            raw,
        };
        let warning = report("allowed_warning", json!({ "utilization": 0.99 }));
        assert_eq!(describe(&warning), None);
        let billed = report("rejected", json!({ "isUsingOverage": true }));
        assert_eq!(describe(&billed), None);
        let refused = report("rejected", json!({}));
        assert_eq!(
            describe(&refused).as_deref(),
            Some("usage limit: rejected, resets at 1795813200")
        );
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
            tokens: None,
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
            tokens: None,
        };
        assert_eq!(
            describe(&done).as_deref(),
            Some("— done (cost so far $0.0123)")
        );
    }

    #[test]
    fn a_finished_turn_says_the_tokens_it_used() {
        let done = AgentEvent::TurnDone {
            is_error: false,
            result: None,
            cost_usd: Some(0.0094),
            duration_ms: None,
            tokens: Some(Tokens {
                input: 43_790,
                output: 97,
            }),
        };
        assert_eq!(
            describe(&done).as_deref(),
            Some("— done, 43.9k tokens (43.8k in, 97 out) (cost so far $0.0094)")
        );
    }

    #[test]
    fn a_token_count_is_said_as_people_say_it() {
        let said = [
            (0, "0"),
            (937, "937"),
            (1_000, "1k"),
            (1_049, "1k"),
            (1_250, "1.3k"),
            (29_640, "29.6k"),
            (99_960, "100k"),
            (123_456, "123k"),
            (1_200_000, "1.2M"),
        ];
        for (tokens, words) in said {
            assert_eq!(short(tokens), words, "{tokens}");
        }
    }
}
