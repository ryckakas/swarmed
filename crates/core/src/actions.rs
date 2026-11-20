//! Actions: buttons of the user's own making, each a prompt for the agent or a command for the
//! task's terminal.
//!
//! Swarmed cannot know what every user wants one click away, so it does not try: an action is
//! a name, a sign to show it by, and either a prompt or a command. Those kept for every
//! repository are in one file in the data directory, a repository's own in a file beside its
//! configuration. The files are plain TOML, meant to be written by hand or by an agent asked to
//! add a button. The core only keeps them: a front end sends the prompt, or types the command
//! into the session's terminal, when the button is pressed.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::{Core, Result};

/// How long an action's name may be, in characters.
const MAX_NAME_CHARS: usize = 40;
/// How long the sign an action is shown by may be, in characters: an emoji or a letter or two.
const MAX_ICON_CHARS: usize = 8;
/// How long an action's prompt or command may be, in bytes.
const MAX_TEXT_BYTES: usize = 50_000;

/// A button of the user's own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Action {
    /// What the button says.
    pub name: String,
    /// A sign shown before the name, such as an emoji. May be empty.
    pub icon: String,
    /// Sent to the agent of the session on screen. Empty for an action that runs a command.
    pub prompt: String,
    /// Typed into the terminal of the session on screen, in the task's worktree. Empty for an
    /// action that sends a prompt.
    pub command: String,
    /// Whether it belongs to one repository and not to all. Which file it is in says so:
    /// the files do not hold it, and what a front end sends for it is not gone by.
    pub of_project: bool,
}

/// A file of actions, as read.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Kept {
    action: Vec<Action>,
}

/// An action as it is written to a file: only what the user gave it.
#[derive(Serialize)]
struct Written<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    icon: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    prompt: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    command: &'a str,
}

/// A file of actions, as written.
#[derive(Serialize)]
struct Writing<'a> {
    action: Vec<Written<'a>>,
}

fn refused(why: impl Into<String>) -> Error {
    Error::Action(why.into())
}

/// `action` as it is kept, if it is one that can be: named, and with a prompt or a command.
pub(crate) fn checked(action: &Action) -> Result<Action> {
    let name = action.name.trim().to_string();
    if name.is_empty() || name.contains('\n') {
        return Err(refused("an action needs a name of one line"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(refused(format!(
            "an action's name can be at most {MAX_NAME_CHARS} characters"
        )));
    }
    let icon = action.icon.trim().to_string();
    if icon.chars().count() > MAX_ICON_CHARS {
        return Err(refused(
            "an action's icon is an emoji or a letter or two, not a text",
        ));
    }
    let (prompt, command) = (action.prompt.trim(), action.command.trim());
    if prompt.is_empty() == command.is_empty() {
        return Err(refused(
            "an action has either a prompt for the agent or a command for the terminal",
        ));
    }
    if prompt.len().max(command.len()) > MAX_TEXT_BYTES {
        return Err(refused(format!(
            "an action can hold at most {MAX_TEXT_BYTES} bytes"
        )));
    }
    Ok(Action {
        name,
        icon,
        prompt: prompt.to_string(),
        command: command.to_string(),
        of_project: action.of_project,
    })
}

impl Core {
    /// The file with the actions of `project`, or with those for every repository.
    fn actions_file(&self, project: Option<&str>) -> PathBuf {
        match project {
            Some(project) => self.paths.project_actions(project),
            None => self.paths.data_dir.join("actions.toml"),
        }
    }

    /// The actions in one file, as written. None if the file is not there.
    fn actions_in(&self, project: Option<&str>) -> Result<Vec<Action>> {
        let file = self.actions_file(project);
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let kept: Kept = toml::from_str(&text).map_err(|error| Error::Config {
            path: file,
            message: error.to_string(),
        })?;
        let of_project = project.is_some();
        Ok(kept
            .action
            .into_iter()
            .map(|action| Action {
                of_project,
                ..action
            })
            .collect())
    }

    fn actions_write(&self, project: Option<&str>, actions: &[Action]) -> Result<()> {
        let file = self.actions_file(project);
        let action = actions
            .iter()
            .map(|action| Written {
                name: &action.name,
                icon: &action.icon,
                prompt: &action.prompt,
                command: &action.command,
            })
            .collect();
        let text = toml::to_string_pretty(&Writing { action }).map_err(|error| Error::Config {
            path: file.clone(),
            message: error.to_string(),
        })?;
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        Ok(std::fs::write(file, text)?)
    }

    /// The actions offered in `project`: those for every repository, then its own. One of its
    /// own takes the place of a general one of the same name. Without a project, the general
    /// ones alone.
    ///
    /// One that was written by hand and is not a usable action is left out, so one slip in a
    /// file does not take every button away.
    pub fn actions(&self, project: Option<&str>) -> Result<Vec<Action>> {
        let own = match project {
            Some(project) => {
                self.project(project)?;
                self.actions_in(Some(project))?
            }
            None => Vec::new(),
        };
        let general = self.actions_in(None)?;
        let general = general
            .into_iter()
            .filter(|action| !own.iter().any(|mine| mine.name == action.name));
        Ok(general
            .chain(own.clone())
            .filter_map(|action| checked(&action).ok())
            .collect())
    }

    /// Keeps an action, in place of the one of the same name in the same file: `project`'s
    /// own, or the general ones without a project. Returns what is then offered there.
    pub fn action_save(&self, project: Option<&str>, action: &Action) -> Result<Vec<Action>> {
        if let Some(project) = project {
            self.project(project)?;
        }
        let action = checked(action)?;
        let mut kept = self.actions_in(project)?;
        match kept.iter_mut().find(|other| other.name == action.name) {
            Some(other) => *other = action,
            None => kept.push(action),
        }
        self.actions_write(project, &kept)?;
        self.actions(project)
    }

    /// Drops the action called `name` from `project`'s own, or from the general ones without
    /// a project, and returns what is then offered there. Dropping one that is not there is
    /// not an error.
    pub fn action_remove(&self, project: Option<&str>, name: &str) -> Result<Vec<Action>> {
        let mut kept = self.actions_in(project)?;
        kept.retain(|action| action.name != name.trim());
        self.actions_write(project, &kept)?;
        self.actions(project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_action_is_taken_as_a_front_end_sends_it() {
        // A front end sends back what it was given, `of_project` and all.
        let sent =
            r#"{"name":"Tests","icon":"","prompt":"","command":"make test","of_project":true}"#;
        let action: Action = serde_json::from_str(sent).unwrap_or_default();
        assert_eq!(action.name, "Tests");
        assert_eq!(action.command, "make test");
    }
}
