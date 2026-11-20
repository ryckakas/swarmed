//! The `swarmed` command: a thin front end over `swarmed-core`.

mod action;
mod layout;
mod session;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use action::ActionCommand;
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use layout::LayoutCommand;
use session::SessionCommand;
use swarmed_core::agent::FakeBackend;
use swarmed_core::{
    AgentSetup, ApiKeyInfo, CloudOrigin, CloudProvider, CloudTask, Core, McpServerView,
    NewTaskOptions, Paths, RemoveOptions, Settings, SetupStatus, Task,
};

/// Swarmed: parallel Claude Code sessions in isolated git worktrees.
#[derive(Debug, Parser)]
#[command(name = "swarmed", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage registered repositories.
    #[command(subcommand)]
    Project(ProjectCommand),
    /// Manage tasks: one worktree, one branch and one stack each.
    #[command(subcommand)]
    Task(TaskCommand),
    /// Run agent sessions in a task's worktree.
    #[command(subcommand)]
    Session(SessionCommand),
    /// List the MCP servers agents are given: those set up in Swarmed and Claude Code's own.
    Mcp,
    /// Show what Swarmed takes up on disk, and in memory and processor time right now.
    Usage,
    /// Keep what a task's setup produced, for new tasks of its environment to start from.
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
    /// Print what the agents report their work has cost: by day and by task, at API prices.
    Costs,
    /// Print the sessions' turns by day, task, agent and model, as tab-separated values.
    Analytics,
    /// List the agents sessions can run on.
    Agents,
    /// Set up an agent other than Claude Code: install it, or choose the models it offers.
    #[command(subcommand)]
    Agent(AgentCommand),
    /// Serve Swarmed's own tools to an agent over standard input and output. Agents start
    /// this themselves.
    #[command(name = swarmed_core::TOOL_SERVER_ARG, hide = true)]
    ToolServer,
    /// Show the command that opens a folder in your editor, or set it.
    Editor {
        /// The new command, such as `code`, `zed` or `cursor`.
        command: Option<String>,
    },
    /// List a project's environments: the ways a task can be set up in it. They are edited
    /// in the app.
    Environment { project: String },
    /// List the skills Swarmed knows of, or those a session on AGENT would be offered: the
    /// ones that agent does not have by itself.
    Skills {
        /// An agent's id, such as `claude` or `codex`.
        #[arg(long)]
        agent: Option<String>,
    },
    /// Hand work to an agent in the cloud, or see what was handed over.
    #[command(subcommand)]
    Cloud(CloudCommand),
    /// Print the note kept on a project or on one of its tasks, or replace it.
    Note {
        project: String,
        /// The task the note is on; without it, the project's own note.
        task: Option<String>,
        /// Replace the note with what is read from standard input, so it stays out of the
        /// shell history. Nothing but blank space removes it.
        #[arg(long)]
        set: bool,
    },
    /// Manage actions: buttons of your own that send a prompt or run a command.
    #[command(subcommand)]
    Action(ActionCommand),
    /// Show or replace your custom layout: sizes, spacing, fonts and colours of your own.
    #[command(subcommand)]
    Layout(LayoutCommand),
    /// Find the sessions in which something was said, closed ones included.
    Search {
        /// The words to look for, as they were written.
        query: String,
    },
    /// Print the rules every agent is given, whatever the agent or model, or replace them.
    Rules {
        /// Replace the rules with what is read from standard input. Nothing but blank space
        /// removes them. Sessions started from then on get them.
        #[arg(long)]
        set: bool,
    },
    /// Store, remove or list the API keys given to agents other than Claude Code.
    #[command(subcommand)]
    Key(KeyCommand),
    /// Show whether new sessions are named by a model after their first message, or set it.
    AutoTitle {
        #[arg(value_enum)]
        state: Option<Switch>,
    },
}

#[derive(Debug, Subcommand)]
enum AgentCommand {
    /// List the agents that can be set up, and whether each is installed and configured.
    Status,
    /// Install an agent with Homebrew and configure it.
    Install { id: String },
    /// Configure an installed agent; with MODELS, offer those in its model menu.
    Configure { id: String, models: Vec<String> },
    /// Ask an agent that knows its own models, such as Codex, which ones it offers, and
    /// offer those.
    Models { id: String },
}

#[derive(Debug, Subcommand)]
enum CloudCommand {
    /// Hand PROMPT to a Codex cloud agent working on the project's repository as it is on
    /// GitHub. Refused if the repository has no cloud environment at the provider.
    Delegate {
        project: String,
        prompt: String,
        /// Start from this task's branch, if it has been pushed; else from the base branch.
        #[arg(long)]
        task: Option<String>,
        /// Whose cloud: `codex` or `claude`.
        #[arg(long, default_value = "codex")]
        provider: String,
    },
    /// List the cloud tasks delegated from a project, with their statuses as of now.
    List { project: String },
    /// Print what a Codex cloud task changed, by its id from `cloud list`.
    Diff { id: i64 },
    /// Apply what a Codex cloud task changed to a local task's worktree, uncommitted.
    Apply {
        id: i64,
        project: String,
        task: String,
    },
}

#[derive(Debug, Subcommand)]
enum KeyCommand {
    /// List the providers a key can be stored for, and which have one.
    List,
    /// Store a provider's key, read from standard input so it stays out of the shell history.
    Set {
        /// The environment variable the key is for, such as `OPENROUTER_API_KEY`.
        name: String,
    },
    /// Remove a provider's key.
    Rm { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Switch {
    On,
    Off,
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Register the git repository at PATH.
    Add {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Project name; defaults to the repository's directory name.
        #[arg(long)]
        name: Option<String>,
    },
    /// List registered projects.
    List,
    /// Take a project out of Swarmed: its tasks' worktrees, its sessions' history and its notes
    /// go; the repository and its branches stay.
    Rm {
        project: String,
        /// Also remove tasks that hold uncommitted or unpushed work.
        #[arg(long)]
        force: bool,
    },
    /// Print the path of a project's local config file.
    Config { project: String },
}

#[derive(Debug, Subcommand)]
enum TaskCommand {
    /// Create a task: branch, worktree, then the `setup` hook.
    New {
        project: String,
        name: String,
        /// Check out this existing branch instead of creating a new one.
        #[arg(long)]
        branch: Option<String>,
        /// Set the task up in this environment of the project; see `swarmed environment`.
        #[arg(long)]
        environment: Option<String>,
    },
    /// Suggest a name for a task that starts with PROMPT, as the app names new tasks.
    Name { project: String, prompt: String },
    /// List a project's tasks.
    List {
        project: String,
        /// Include removed tasks.
        #[arg(long)]
        all: bool,
    },
    /// Run the `setup` hook again.
    Setup { project: String, name: String },
    /// Run the task's `start` command in the foreground until it ends or is interrupted, the
    /// way the app keeps it running in the background.
    Start { project: String, name: String },
    /// Run `teardown`, remove the worktree and archive the task.
    Rm {
        project: String,
        name: String,
        /// Remove despite uncommitted changes, unpushed commits or a failing teardown.
        #[arg(long)]
        force: bool,
        /// Also delete the task's local branch.
        #[arg(long)]
        delete_branch: bool,
    },
    /// Show the pull request of the task's branch and how its checks stand.
    Pull { project: String, name: String },
    /// List what the task's agents left running in the background, and how it ended.
    Background { project: String, name: String },
    /// Commit everything in the task's worktree.
    Commit {
        project: String,
        name: String,
        /// The commit message.
        #[arg(short, long)]
        message: String,
    },
    /// Print a commit message written for the task's uncommitted work. Commits nothing.
    CommitMessage {
        project: String,
        name: String,
        /// Have this session's agent write it, on the session's model. Without it, and for a
        /// session on Claude Code, Claude Code's smallest model writes.
        #[arg(long)]
        session: Option<i64>,
    },
    /// Print a pull request title and description written for the task's branch. Opens
    /// nothing.
    PrMessage {
        project: String,
        name: String,
        /// Have this session's agent write it, on the session's model. Without it, and for a
        /// session on Claude Code, Claude Code's smallest model writes.
        #[arg(long)]
        session: Option<i64>,
    },
    /// Push the task's branch to its remote.
    Push { project: String, name: String },
    /// Open a pull request for the task's branch. Without a title, the task's commits give
    /// the title and the description.
    Pr {
        project: String,
        name: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        body: Option<String>,
        /// Open it as a draft.
        #[arg(long)]
        draft: bool,
    },
    /// List the files the task has changed, or print one file's diff.
    Changes {
        project: String,
        name: String,
        /// Print this file's diff instead of the list.
        path: Option<String>,
    },
    /// Open the task's worktree in your editor, or its running stack in the browser.
    Open {
        project: String,
        name: String,
        /// Open the project's `open_url` in the browser instead of the editor.
        #[arg(long)]
        browser: bool,
    },
}

/// Writes a line to stdout. A closed pipe (`swarmed task list | head`) is not an error worth
/// reporting, which is why this is not `println!`: that panics instead.
pub(crate) fn say(line: &str) {
    writeln!(std::io::stdout().lock(), "{line}").ok();
}

/// Shows hook output, indented so it reads apart from Swarmed's own lines.
fn hook_line(line: &str) {
    say(&format!("  | {line}"));
}

/// `SWARMED_FAKE_AGENT` swaps Claude for the scripted agent, to try the CLI without tokens.
fn open_core() -> Result<Core> {
    let paths = Paths::from_env()?;
    if std::env::var_os("SWARMED_FAKE_AGENT").is_some() {
        return Ok(Core::with_backend(paths, Arc::new(FakeBackend::default()))?);
    }
    let mut core = Core::open(paths)?;
    // An agent starts this same executable to reach Swarmed's own tools.
    if let Ok(executable) = std::env::current_exe() {
        core.set_tool_server(executable);
    }
    Ok(core)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    // Needs no data directory, and must not print anything but the protocol.
    if matches!(cli.command, Command::ToolServer) {
        let served = swarmed_core::serve_tools(std::io::stdin().lock(), std::io::stdout().lock());
        return Ok(served?);
    }
    let core = Arc::new(open_core()?);
    // A session run from here gets the same tools for reaching other sessions as in the app.
    if matches!(cli.command, Command::Session(_)) {
        drop(tokio::spawn(swarmed_core::serve_session_tools(Arc::clone(
            &core,
        ))));
    }
    let done = run(&core, cli.command).await;
    // A stack a command started lives only as long as this process: it is not left behind.
    core.stacks_stop_all().await;
    // The socket is this process's own; nothing is left to answer on it.
    std::fs::remove_file(core.paths().tools_socket()).ok();
    done
}

async fn run(core: &Core, command: Command) -> Result<()> {
    match command {
        Command::Project(command) => project(core, command).await,
        Command::Task(command) => task(core, command).await,
        Command::Session(command) => session::session(core, command).await,
        // Answered above, before anything was opened.
        Command::ToolServer => Ok(()),
        Command::Agent(command) => agent(core, command).await,
        Command::Mcp => mcp(core),
        Command::Usage => usage(core).await,
        Command::Snapshot { command } => snapshot(core, command).await,
        Command::Costs => costs(core),
        Command::Analytics => analytics(core),
        Command::Agents => {
            agents(core);
            Ok(())
        }
        Command::Editor { command } => editor(core, command),
        Command::AutoTitle { state } => auto_title(core, state),
        Command::Key(command) => key(core, command),
        Command::Note { project, task, set } => note(core, &project, task.as_deref(), set),
        Command::Environment { project } => environments(core, &project),
        Command::Cloud(command) => cloud(core, command).await,
        Command::Skills { agent } => {
            skills(core, agent.as_deref());
            Ok(())
        }
        Command::Rules { set } => rules(core, set),
        Command::Action(command) => action::run(core, command),
        Command::Layout(command) => layout::run(core, &command),
        Command::Search { query } => search(core, &query),
    }
}

/// Prints what the agents report their work has cost.
fn costs(core: &Core) -> Result<()> {
    let costs = core.costs()?;
    say(&format!(
        "today ${:.2}\tlast 7 days ${:.2}\tin all ${:.2}",
        costs.today, costs.week, costs.total
    ));
    for task in &costs.tasks {
        say(&format!(
            "${:.2}\t{} / {}",
            task.usd, task.project, task.task
        ));
    }
    Ok(())
}

#[derive(Debug, Subcommand)]
enum SnapshotCommand {
    /// Keep the files git ignores in the task's worktree as the snapshot of its environment.
    Save { project: String, task: String },
    /// List the project's snapshots.
    List { project: String },
    /// Forget a snapshot: the one of tasks without an environment, unless one is named.
    Remove {
        project: String,
        #[arg(long)]
        environment: Option<String>,
    },
}

async fn snapshot(core: &Core, command: SnapshotCommand) -> Result<()> {
    let describe = |snapshot: &swarmed_core::Snapshot| {
        let environment = snapshot.environment.as_deref().unwrap_or("(none)");
        format!(
            "{environment}\tfrom {}\t{} entries",
            snapshot.task, snapshot.entries
        )
    };
    match command {
        SnapshotCommand::Save { project, task } => {
            say(&describe(&core.snapshot_save(&project, &task).await?));
        }
        SnapshotCommand::List { project } => {
            for snapshot in core.snapshots(&project)? {
                say(&describe(&snapshot));
            }
        }
        SnapshotCommand::Remove {
            project,
            environment,
        } => core.snapshot_remove(&project, environment.as_deref())?,
    }
    Ok(())
}

/// Prints the sessions' turns added up, one day, task, agent and model to a line.
fn analytics(core: &Core) -> Result<()> {
    say("day\tproject\ttask\tagent\tmodel\tturns\tfailed\tusd\tseconds");
    for stats in core.analytics()? {
        say(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{}",
            stats.day,
            stats.project,
            stats.task,
            stats.agent,
            stats.model,
            stats.turns,
            stats.failed,
            stats.usd,
            stats.duration_ms / 1000
        ));
    }
    Ok(())
}

/// Prints the sessions in which `query` was said.
fn search(core: &Core, query: &str) -> Result<()> {
    for hit in core.sessions_search(query)? {
        let closed = if hit.closed { ", closed" } else { "" };
        say(&format!(
            "{}\t{} / {}{closed}\t{}\n\t{}",
            hit.session_id, hit.project, hit.task, hit.title, hit.snippet
        ));
    }
    Ok(())
}

fn editor(core: &Core, command: Option<String>) -> Result<()> {
    if let Some(editor) = command {
        let settings = Settings {
            editor,
            ..core.settings()?
        };
        core.set_settings(&settings)?;
    }
    say(&core.settings()?.editor);
    Ok(())
}

fn key(core: &Core, command: KeyCommand) -> Result<()> {
    match command {
        KeyCommand::List => {
            core.api_keys()?.iter().for_each(|key| say(&key_row(key)));
        }
        KeyCommand::Set { name } => {
            let mut value = String::new();
            std::io::stdin().read_line(&mut value)?;
            core.set_api_key(&name, &value)?;
            say(&format!("stored {name}"));
        }
        KeyCommand::Rm { name } => core.delete_api_key(&name)?,
    }
    Ok(())
}

fn note(core: &Core, project: &str, task: Option<&str>, set: bool) -> Result<()> {
    if set {
        let text = std::io::read_to_string(std::io::stdin())?;
        return Ok(core.set_note(project, task, &text)?);
    }
    say(core.note(project, task)?.trim_end());
    Ok(())
}

/// One line per skill: whose it is, its name, whether it is shared, and when to use it.
fn skills(core: &Core, agent: Option<&str>) {
    let skills = agent.map_or_else(|| core.skills(), |agent| core.skills_offered(agent, None));
    for skill in skills {
        let shared = ["kept back", "shared"][usize::from(skill.shared)];
        say(&format!(
            "{}\t{}\t{shared}\t{}",
            skill.origin, skill.name, skill.description
        ));
    }
}

async fn cloud(core: &Core, command: CloudCommand) -> Result<()> {
    let row = |task: &CloudTask| {
        say(&format!(
            "{}\t{}\t{}\t{}\t{}",
            task.provider,
            task.status,
            task.branch,
            task.url,
            task.prompt.lines().next().unwrap_or_default()
        ));
    };
    match command {
        CloudCommand::Delegate {
            project,
            prompt,
            task,
            provider,
        } => {
            let provider = CloudProvider::parse(&provider)
                .ok_or_else(|| anyhow::anyhow!("there is no cloud provider `{provider}`"))?;
            let origin = CloudOrigin {
                task: task.as_deref(),
                session: None,
                provider,
            };
            row(&core.cloud_delegate(&project, &prompt, origin).await?);
        }
        CloudCommand::List { project } => {
            core.cloud_tasks(&project, true).await?.iter().for_each(row);
        }
        CloudCommand::Diff { id } => say(&core.cloud_task_diff(id).await?),
        CloudCommand::Apply { id, project, task } => {
            core.cloud_task_apply(id, &project, &task).await?;
            say(&format!("applied to `{task}`, uncommitted"));
        }
    }
    Ok(())
}

/// Commits a task's work and says what is left to push.
async fn commit(core: &Core, project: &str, name: &str, message: &str) -> Result<()> {
    core.task_commit(project, name, message).await?;
    let left = core.task_unpublished(project, name).await?;
    say(&format!("committed; {} commit(s) to push", left.unpushed));
    Ok(())
}

/// Opens a task's pull request, from its commits unless a title or a body is given.
async fn open_pull(
    core: &Core,
    (project, name): (&str, &str),
    title: Option<String>,
    body: Option<String>,
    draft: bool,
) -> Result<()> {
    let mut text = core.task_pull_draft(project, name).await?;
    text.title = title.unwrap_or(text.title);
    text.body = body.unwrap_or(text.body);
    let opened = core.task_pull_create(project, name, &text, draft).await?;
    say(&format!("opened #{} {}", opened.number, opened.url));
    Ok(())
}

/// Prints a task's pull request and its checks.
async fn pull(core: &Core, project: &str, name: &str) -> Result<()> {
    let Some(pull) = core.task_pull(project, name).await? else {
        say("no pull request");
        return Ok(());
    };
    let draft = if pull.draft { ", draft" } else { "" };
    say(&format!(
        "#{} {:?}{draft}  {}\n{}",
        pull.number, pull.state, pull.title, pull.url
    ));
    for check in &pull.checks {
        say(&format!("{:?}\t{}", check.outcome, check.name));
    }
    Ok(())
}

/// Prints what the task's agents left running, one to a line.
fn background(core: &Core, project: &str, name: &str) -> Result<()> {
    let tasks = core.task_background(project, name)?;
    if tasks.is_empty() {
        say("nothing was left running");
    }
    for task in tasks {
        say(&format!(
            "{:?}\t{}\t{}\tsession {}",
            task.state, task.task_id, task.description, task.session_id
        ));
    }
    Ok(())
}

/// Runs a task's stack until Ctrl-C, printing what it prints, then stops it.
async fn stack(core: &Core, project: &str, name: &str) -> Result<()> {
    if !core.stack_start(project, name)? {
        say("this task's project has no `start` command");
        return Ok(());
    }
    say("started; Ctrl-C stops it");
    let mut printed = 0;
    let mut interrupted = Box::pin(tokio::signal::ctrl_c());
    loop {
        let log = core.stack_log(project, name)?;
        if let Some(new) = log.get(printed..).filter(|new| !new.is_empty()) {
            say(new.trim_end());
            printed = log.len();
        }
        let wait = tokio::time::sleep(std::time::Duration::from_millis(500));
        tokio::select! {
            _ = &mut interrupted => break,
            () = wait => {}
        }
    }
    core.stack_stop(project, name, &hook_line).await?;
    say("stopped");
    Ok(())
}

/// One line per environment: its name, what it sets up with, and the names of its variables.
fn environments(core: &Core, project: &str) -> Result<()> {
    for (name, environment) in core.environments(project)? {
        let variables: Vec<&str> = environment.env.keys().map(String::as_str).collect();
        say(&format!(
            "{name}\t{}\t{}",
            environment.setup,
            variables.join(", ")
        ));
    }
    Ok(())
}

fn rules(core: &Core, set: bool) -> Result<()> {
    if set {
        let rules = std::io::read_to_string(std::io::stdin())?
            .trim()
            .to_string();
        let settings = Settings {
            rules,
            ..core.settings()?
        };
        return Ok(core.set_settings(&settings)?);
    }
    say(core.settings()?.rules.trim_end());
    Ok(())
}

fn key_row(key: &ApiKeyInfo) -> String {
    let state = if key.stored { "stored" } else { "not set" };
    format!("{}\t{}\t{state}", key.name, key.provider)
}

fn auto_title(core: &Core, state: Option<Switch>) -> Result<()> {
    if let Some(state) = state {
        let settings = Settings {
            auto_title: state == Switch::On,
            ..core.settings()?
        };
        core.set_settings(&settings)?;
    }
    say(if core.settings()?.auto_title {
        "on"
    } else {
        "off"
    });
    Ok(())
}

async fn agent(core: &Core, command: AgentCommand) -> Result<()> {
    let row = |setup: &AgentSetup| {
        let installed = setup.installed.as_ref().map_or_else(
            || format!("not installed ({})", setup.install_command),
            |path| path.display().to_string(),
        );
        let configured = if setup.configured {
            "configured"
        } else {
            "not configured"
        };
        say(&format!(
            "{}\t{installed}\t{configured}\t{}",
            setup.id,
            setup.models.join(", ")
        ));
    };
    match command {
        AgentCommand::Status => core.agent_setups()?.iter().for_each(row),
        AgentCommand::Install { id } => row(&core.agent_install(&id, &hook_line).await?),
        AgentCommand::Models { id } => row(&core.agent_models_refresh(&id).await?),
        AgentCommand::Configure { id, models } => {
            let models = (!models.is_empty()).then_some(models);
            row(&core.agent_configure(&id, models)?);
        }
    }
    Ok(())
}

fn mcp(core: &Core) -> Result<()> {
    let row = |server: &McpServerView| {
        let state = if server.enabled { "on" } else { "off" };
        let status = server.status.as_deref().unwrap_or("-");
        say(&format!(
            "{}\t{}\t{state}\t{status}\t{}",
            server.name, server.origin, server.target
        ));
    };
    core.mcp_servers()?.iter().for_each(row);
    Ok(())
}

/// A size as people say it: 512 B, 3.4 MB, 1.2 GB.
fn bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    let mut unit = "B";
    #[allow(
        clippy::cast_precision_loss,
        reason = "a size for reading, not for arithmetic"
    )]
    let mut value = bytes as f64;
    for next in UNITS {
        if value < 1000.0 {
            break;
        }
        value /= 1000.0;
        unit = next;
    }
    if unit == "B" {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {unit}")
    }
}

async fn usage(core: &Core) -> Result<()> {
    let usage = core.usage().await?;
    let database = bytes(usage.database_bytes);
    say(&format!("database\t{database}\t{} events", usage.events));
    say(&format!("notes\t{}", bytes(usage.notes_bytes)));
    for worktree in &usage.worktrees {
        let size = worktree
            .bytes
            .map_or_else(|| "not measured".to_string(), bytes);
        say(&format!(
            "worktree\t{size}\t{}/{}",
            worktree.project, worktree.task
        ));
    }
    let running = usage.running;
    say(&format!(
        "running\t{}\t{:.1}% of a core\t{} processes",
        bytes(running.memory_bytes),
        running.cpu_percent,
        running.processes
    ));
    Ok(())
}

fn agents(core: &Core) {
    for agent in core.agents() {
        say(&format!(
            "{}\t{}\t{}",
            agent.id,
            agent.name,
            agent.models.join(", ")
        ));
    }
    say(&format!(
        "configure more in {}",
        core.paths().agents_config().display()
    ));
}

async fn project(core: &Core, command: ProjectCommand) -> Result<()> {
    match command {
        ProjectCommand::Add { path, name } => {
            let project = core.project_add(&path, name.as_deref()).await?;
            say(&format!(
                "added project `{}` at {}",
                project.name,
                project.repo_path.display()
            ));
            say(&format!(
                "config: {}",
                core.paths().project_config(&project.name).display()
            ));
        }
        ProjectCommand::List => {
            for project in core.projects()? {
                say(&format!(
                    "{}\t{}",
                    project.name,
                    project.repo_path.display()
                ));
            }
        }
        ProjectCommand::Rm { project, force } => {
            core.project_remove(&project, force, &hook_line).await?;
            say(&format!("removed project `{project}`"));
        }
        ProjectCommand::Config { project } => {
            let project = core.project(&project)?;
            say(&core
                .paths()
                .project_config(&project.name)
                .display()
                .to_string());
        }
    }
    Ok(())
}

/// Prints a commit message or a pull request written for a task, if that is what `command`
/// asks for; else hands the command back.
async fn written(core: &Core, command: TaskCommand) -> Result<Option<TaskCommand>> {
    match command {
        TaskCommand::CommitMessage {
            project,
            name,
            session,
        } => say(&core.task_commit_message(&project, &name, session).await?),
        TaskCommand::PrMessage {
            project,
            name,
            session,
        } => {
            let draft = core.task_pull_message(&project, &name, session).await?;
            say(&format!("{}\n\n{}", draft.title, draft.body));
        }
        other => return Ok(Some(other)),
    }
    Ok(None)
}

async fn task(core: &Core, command: TaskCommand) -> Result<()> {
    let Some(command) = written(core, command).await? else {
        return Ok(());
    };
    match command {
        TaskCommand::Name { project, prompt } => {
            let naming = core.task_name_for(&project, &prompt).await?;
            say(&naming.map_or_else(
                || "no name: naming is off, or no answer came in time".to_string(),
                |naming| format!("{}\t{}", naming.name, naming.title),
            ));
        }
        TaskCommand::New {
            project,
            name,
            branch,
            environment,
        } => {
            let options = NewTaskOptions {
                existing_branch: branch.as_deref(),
                environment: environment.as_deref(),
            };
            let task = core.task_new(&project, &name, options, &hook_line).await?;
            say(&format!(
                "created task `{}` on branch `{}` (slot {}) at {}",
                task.name,
                task.branch,
                task.slot,
                task.worktree_path.display()
            ));
        }
        TaskCommand::List { project, all } => {
            core.tasks(&project, all)?
                .iter()
                .for_each(|task| say(&task_row(task)));
        }
        TaskCommand::Setup { project, name } => {
            core.task_setup(&project, &name, &hook_line).await?;
            say(&format!("setup finished for `{name}`"));
        }
        TaskCommand::Start { project, name } => stack(core, &project, &name).await?,
        TaskCommand::Rm {
            project,
            name,
            force,
            delete_branch,
        } => {
            let options = RemoveOptions {
                force,
                delete_branch,
            };
            core.task_remove(&project, &name, options, &hook_line)
                .await?;
            say(&format!("removed task `{name}`"));
        }
        TaskCommand::Pull { project, name } => pull(core, &project, &name).await?,
        TaskCommand::Background { project, name } => background(core, &project, &name)?,
        TaskCommand::Commit {
            project,
            name,
            message,
        } => commit(core, &project, &name, &message).await?,
        TaskCommand::CommitMessage { .. } | TaskCommand::PrMessage { .. } => {}
        TaskCommand::Push { project, name } => {
            core.task_push(&project, &name).await?;
            say("pushed");
        }
        TaskCommand::Pr {
            project,
            name,
            title,
            body,
            draft,
        } => open_pull(core, (&project, &name), title, body, draft).await?,
        TaskCommand::Changes {
            project,
            name,
            path: Some(path),
        } => say(&core.task_diff(&project, &name, &path).await?.text),
        TaskCommand::Changes { project, name, .. } => {
            for file in core.task_changes(&project, &name).await? {
                let count = |n: Option<u32>| n.map_or_else(|| "-".to_string(), |n| n.to_string());
                let (added, removed) = (count(file.added), count(file.removed));
                say(&format!("+{added}\t-{removed}\t{}", file.path));
            }
        }
        TaskCommand::Open {
            project,
            name,
            browser: true,
        } => core.task_open_url(&project, &name)?,
        TaskCommand::Open { project, name, .. } => core.task_open_editor(&project, &name)?,
    }
    Ok(())
}

fn task_row(task: &Task) -> String {
    let state = match (task.archived, task.setup) {
        (true, _) => "archived",
        (false, SetupStatus::Ok) => "ready",
        (false, SetupStatus::Failed) => "setup failed",
        (false, SetupStatus::Pending) => "setup pending",
    };
    format!(
        "{}\tslot {}\t{state}\t{}\t{}",
        task.name,
        task.slot,
        task.branch,
        task.worktree_path.display()
    )
}
