//! Naming a new task after what it is for, instead of after the first words of its prompt.
//!
//! A task's name becomes its worktree's folder and its branch, and neither can sensibly be
//! renamed once an agent works in them. So the name is asked for before the task exists, from
//! the same small model that titles sessions, and one answer serves as both.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::Core;
use crate::error::Result;
use crate::sessions::title_of;

/// How long a task waits to be named. Past that, whoever asked falls back on the prompt.
const NAMING_TIMEOUT: Duration = Duration::from_secs(8);
/// How long a task name made from a title may be, and of how many words.
const NAME_CHARS: usize = 40;
const NAME_WORDS: usize = 5;
/// How much of a skill's description the namer is given: enough to say what it is for.
const DESCRIPTION_CHARS: usize = 300;

/// What a new task and its first session are called.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskNaming {
    /// The task's name: lower-case words joined by dashes, not taken by another task.
    pub name: String,
    /// The title for the task's first session, which the name was made from.
    pub title: String,
}

/// `title` as a task name: its first few words in lower case, joined by dashes. Letters and
/// digits outside ASCII are left out, since the name becomes a branch and a folder.
fn name_from(title: &str) -> String {
    let lowered = title.to_ascii_lowercase();
    let words = lowered
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty());
    let mut name = String::new();
    for word in words.take(NAME_WORDS) {
        let longer = name.len() + 1 + word.len();
        if !name.is_empty() && longer > NAME_CHARS {
            break;
        }
        if !name.is_empty() {
            name.push('-');
        }
        name.push_str(word);
    }
    name.chars().take(NAME_CHARS).collect()
}

/// The skill or command a message calls, `/name` and whatever follows: the name and the rest.
/// `None` for any other message, a path such as `/Users/me/notes.txt` among them.
pub(crate) fn command_called(message: &str) -> Option<(&str, &str)> {
    let called = message.trim_start().strip_prefix('/')?;
    let (name, rest) = called
        .split_once(char::is_whitespace)
        .unwrap_or((called, ""));
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.');
    let named = !name.is_empty() && name.chars().all(allowed);
    named.then(|| (name, rest.trim()))
}

/// What the namer is asked to name for a message that calls `name` with `rest` after it: the
/// request it stands for, with what the skill says it is for when that is known. The text as
/// typed would be read as a command, and a bare command's name says little of the request.
pub(crate) fn naming_request(name: &str, rest: &str, description: Option<&str>) -> String {
    let called = match description {
        Some(description) => {
            let flat = description.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut said: String = flat.chars().take(DESCRIPTION_CHARS).collect();
            // A prefix of `flat`, so shorter in bytes exactly when something was cut.
            if said.len() < flat.len() {
                said.push('…');
            }
            format!("The user calls the skill `{name}`, which is for: {said}")
        }
        None => format!("The user calls the command `{name}`."),
    };
    let after = if rest.is_empty() {
        "They give nothing after it.".to_string()
    } else {
        format!("What they give after it: {rest}")
    };
    format!("{called}\n{after}")
}

/// `name`, or `name-2`, `name-3` and so on if it is among `taken`.
fn unique(name: &str, taken: &[String]) -> String {
    let free = |candidate: &String| !taken.contains(candidate);
    // One of these is free: there are more of them than names taken.
    std::iter::once(name.to_string())
        .chain((2..taken.len() + 3).map(|n| format!("{name}-{n}")))
        .find(free)
        .unwrap_or_else(|| name.to_string())
}

impl Core {
    /// What the namer is asked to name for `message`, sent to `agent` working in `worktree`
    /// where those are known: the message itself, or for a message that calls a skill or
    /// command, the request that stands for.
    pub(crate) fn naming_prompt(
        &self,
        message: &str,
        agent: Option<&str>,
        worktree: Option<&Path>,
    ) -> String {
        let Some((name, rest)) = command_called(message) else {
            return message.to_string();
        };
        let description = self.paths.skill_description(agent, worktree, name);
        naming_request(name, rest, description.as_deref())
    }

    /// A name for a task in `project` that starts with `prompt`, and the title its first
    /// session gets. `None` if the user has naming switched off, nothing here can name, or no
    /// usable answer came in time: the caller then names the task itself.
    pub async fn task_name_for(&self, project: &str, prompt: &str) -> Result<Option<TaskNaming>> {
        // A removed task's name too: its branch may still be there.
        let taken: Vec<String> = self
            .tasks(project, true)?
            .into_iter()
            .map(|task| task.name)
            .collect();
        let wanted = self.settings()?.auto_title;
        let Some(namer) = self.namer.clone().filter(|_| wanted) else {
            return Ok(None);
        };
        // No task yet, so no worktree: the repository's own skills are the same.
        let repo = self.project(project)?.repo_path;
        let asked = self.naming_prompt(prompt, None, Some(&repo));
        let answer = tokio::time::timeout(NAMING_TIMEOUT, namer.name(asked)).await;
        let title = answer.ok().flatten();
        let title = title.and_then(|answer| title_of(&answer, prompt));
        let naming = title.and_then(|title| {
            let name = name_from(&title);
            (!name.is_empty()).then(|| TaskNaming {
                name: unique(&name, &taken),
                title,
            })
        });
        Ok(naming)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_becomes_a_few_lower_case_words_joined_by_dashes() {
        assert_eq!(
            name_from("Fix Login Redirect Loop"),
            "fix-login-redirect-loop"
        );
        assert_eq!(
            name_from("  CSV export: keep the VAT column!  "),
            "csv-export-keep-the-vat"
        );
        assert_eq!(name_from("Žalia žolė 2"), "alia-ol-2");
        assert_eq!(name_from("…"), "");
        // Whole words only, until the limit; one endless word is cut.
        let long = name_from("Internationalisation of the administrative configuration");
        assert_eq!(long, "internationalisation-of-the");
        assert_eq!(name_from(&"x".repeat(90)).len(), NAME_CHARS);
    }

    #[test]
    fn a_message_that_calls_a_skill_or_command_is_told_apart_from_a_path() {
        assert_eq!(
            command_called("/code-review 123"),
            Some(("code-review", "123"))
        );
        assert_eq!(command_called("  /simplify\n"), Some(("simplify", "")));
        assert_eq!(
            command_called("/repolore:search why"),
            Some(("repolore:search", "why"))
        );
        assert_eq!(command_called("/Users/me/notes.txt fix this"), None);
        assert_eq!(command_called("/ nothing"), None);
        assert_eq!(command_called("fix /login"), None);
    }

    #[test]
    fn the_namer_is_given_what_a_skill_is_for_and_what_follows_it() {
        assert_eq!(
            naming_request("code-review", "123", Some("Review code\n  for quality.")),
            "The user calls the skill `code-review`, which is for: Review code for quality.\n\
             What they give after it: 123"
        );
        assert_eq!(
            naming_request("init", "", None),
            "The user calls the command `init`.\nThey give nothing after it."
        );
        let long = "word ".repeat(100);
        let request = naming_request("long", "", Some(&long));
        let said = request.lines().next().unwrap_or_default();
        assert!(said.ends_with('…'), "{said}");
        assert!(said.chars().count() < 360, "{said}");
    }

    #[test]
    fn a_tag_a_command_or_nothing_is_no_title() {
        let skill = "/code-review 123";
        assert_eq!(
            title_of("\"Review PR 123\"\nmore", skill).as_deref(),
            Some("Review PR 123")
        );
        assert_eq!(title_of("<invoke_skill_or_bash_placeholder>", skill), None);
        assert_eq!(title_of("  \n", skill), None);
        assert_eq!(title_of("/code-review 123", skill), None);
        assert_eq!(title_of("Code-Review 123", skill), None);
        // An answer that repeats an ordinary message is a title like any other.
        assert_eq!(
            title_of("Fix login", "fix login").as_deref(),
            Some("Fix login")
        );
    }

    #[test]
    fn a_name_that_is_taken_gets_a_number() {
        let taken = ["fix-login".to_string(), "fix-login-2".to_string()];
        assert_eq!(unique("fix-login", &taken), "fix-login-3");
        assert_eq!(unique("other", &taken), "other");
    }
}
