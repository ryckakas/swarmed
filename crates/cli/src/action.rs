//! The `action` commands: the buttons of the user's own.

use anyhow::Result;
use clap::Subcommand;
use swarmed_core::{Action, Core};

use crate::say;

#[derive(Debug, Subcommand)]
pub(crate) enum ActionCommand {
    /// List the actions offered in a project, or those for every repository.
    List {
        /// The project whose actions to list, its own and the general ones.
        #[arg(long)]
        project: Option<String>,
    },
    /// Keep an action, in place of the one of that name. Give it `--prompt` or `--command`.
    Save {
        /// What the button says.
        name: String,
        /// Sent to the agent of the session on screen.
        #[arg(long, conflicts_with = "command")]
        prompt: Option<String>,
        /// Typed into the terminal of the session on screen, in the task's worktree.
        #[arg(long)]
        command: Option<String>,
        /// A sign shown before the name, such as an emoji.
        #[arg(long, default_value = "")]
        icon: String,
        /// Keep it for this project alone; without it, for every repository.
        #[arg(long)]
        project: Option<String>,
    },
    /// Move an action among the others: its button is shown where it is kept.
    Move {
        name: String,
        /// How many places: negative for earlier, as in `--by=-1`.
        #[arg(long, allow_hyphen_values = true)]
        by: i32,
        /// Move it among this project's own; without it, among those for every repository.
        #[arg(long)]
        project: Option<String>,
    },
    /// Drop an action.
    Remove {
        name: String,
        /// Drop it from this project's own; without it, from those for every repository.
        #[arg(long)]
        project: Option<String>,
    },
}

/// One line per action: where it belongs, its name, and what it does.
fn list(actions: &[Action]) {
    for action in actions {
        let scope = if action.of_project { "project" } else { "all" };
        let (kind, text) = if action.command.is_empty() {
            ("prompt", &action.prompt)
        } else {
            ("command", &action.command)
        };
        let first = text.lines().next().unwrap_or_default();
        say(&format!(
            "{scope}\t{}\t{}\t{kind}\t{first}",
            action.icon, action.name
        ));
    }
}

pub(crate) fn run(core: &Core, command: ActionCommand) -> Result<()> {
    let offered = match command {
        ActionCommand::List { project } => core.actions(project.as_deref())?,
        ActionCommand::Save {
            name,
            prompt,
            command,
            icon,
            project,
        } => {
            let action = Action {
                name,
                icon,
                prompt: prompt.unwrap_or_default(),
                command: command.unwrap_or_default(),
                ..Action::default()
            };
            core.action_save(project.as_deref(), &action)?
        }
        ActionCommand::Remove { name, project } => core.action_remove(project.as_deref(), &name)?,
        ActionCommand::Move { name, by, project } => {
            core.action_move(project.as_deref(), &name, by)?
        }
    };
    list(&offered);
    Ok(())
}
