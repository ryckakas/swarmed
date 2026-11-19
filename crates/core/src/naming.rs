//! Naming a new task after what it is for, instead of after the first words of its prompt.
//!
//! A task's name becomes its worktree's folder and its branch, and neither can sensibly be
//! renamed once an agent works in them. So the name is asked for before the task exists, from
//! the same small model that titles sessions, and one answer serves as both.

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
    /// A name for a task in `project` that starts with `prompt`, and the title its first
    /// session gets. `None` if the user has naming switched off, nothing here can name, or no
    /// usable answer came in time: the caller then names the task itself.
    pub async fn task_name_for(&self, project: &str, prompt: &str) -> Result<Option<TaskNaming>> {
        let taken: Vec<String> = self
            .tasks(project, false)?
            .into_iter()
            .map(|task| task.name)
            .collect();
        let wanted = self.settings()?.auto_title;
        let Some(namer) = self.namer.clone().filter(|_| wanted) else {
            return Ok(None);
        };
        let answer = tokio::time::timeout(NAMING_TIMEOUT, namer.name(prompt.to_string())).await;
        let title = answer.ok().flatten().map(|answer| title_of(&answer));
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
    fn a_name_that_is_taken_gets_a_number() {
        let taken = ["fix-login".to_string(), "fix-login-2".to_string()];
        assert_eq!(unique("fix-login", &taken), "fix-login-3");
        assert_eq!(unique("other", &taken), "other");
    }
}
