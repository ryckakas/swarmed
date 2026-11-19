//! The `swarmed` command: a thin front end over `swarmed-core`.

mod session;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use session::SessionCommand;
use swarmed_core::agent::FakeBackend;
use swarmed_core::{Core, NewTaskOptions, Paths, RemoveOptions, SetupStatus, Task};

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
    /// List the agents sessions can run on.
    Agents,
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
    },
    /// List a project's tasks.
    List {
        project: String,
        /// Include removed tasks.
        #[arg(long)]
        all: bool,
    },
    /// Run the `setup` hook again.
    Setup { project: String, name: String },
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
    Ok(Core::open(paths)?)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let core = open_core()?;
    match cli.command {
        Command::Project(command) => project(&core, command).await,
        Command::Task(command) => task(&core, command).await,
        Command::Session(command) => session::session(&core, command).await,
        Command::Agents => {
            agents(&core);
            Ok(())
        }
    }
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

async fn task(core: &Core, command: TaskCommand) -> Result<()> {
    match command {
        TaskCommand::New {
            project,
            name,
            branch,
        } => {
            let options = NewTaskOptions {
                existing_branch: branch.as_deref(),
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
