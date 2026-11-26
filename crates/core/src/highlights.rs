//! A highlight: a colour the user gives a task or a session so that it stands out where it is.
//!
//! Pinning moves a row to the top of its list; a highlight leaves it where it is. It is the
//! user's alone: an agent reports how its work stands with a mark (`marks.rs`), which the next
//! message clears, and has no way to set a highlight, which stays until the user takes it off.
//! The colours are the marks' five names, drawn by each palette and layout, so a custom
//! layout's value for one changes both.

use crate::agent::AgentEvent;
use crate::error::Error;
use crate::{Core, MarkColour, Result, Session, Task};

/// What takes a highlight off, where a colour is asked for.
const NO_HIGHLIGHT: &str = "none";

/// The highlight the word `colour` asks for, as a command line or a front end gives it;
/// `None` for `none`, which takes it off.
pub fn highlight_named(colour: &str) -> Result<Option<MarkColour>> {
    if colour.trim().eq_ignore_ascii_case(NO_HIGHLIGHT) {
        return Ok(None);
    }
    MarkColour::parse(colour).map(Some).ok_or_else(|| {
        let known: Vec<&str> = MarkColour::ALL
            .into_iter()
            .map(MarkColour::as_str)
            .collect();
        Error::Highlight(format!(
            "there is no colour `{}`; use one of {}, or `{NO_HIGHLIGHT}` to take the highlight off",
            colour.trim(),
            known.join(", ")
        ))
    })
}

impl Core {
    /// Highlights session `id` in `colour`, in place of the colour it has, or with `None` takes
    /// its highlight off, and has every front end read the tree again. The session keeps its
    /// place, and its highlight while it is closed.
    pub fn session_highlight(&self, id: i64, colour: Option<MarkColour>) -> Result<Session> {
        self.session(id)?;
        self.store().highlight_write("sessions", id, colour)?;
        self.live.publish_apart(AgentEvent::TreeChanged);
        self.session(id)
    }

    /// Highlights task `task` of `project` in `colour`, or with `None` takes its highlight
    /// off, as [`Core::session_highlight`] does a session's.
    pub fn task_highlight(
        &self,
        project: &str,
        task: &str,
        colour: Option<MarkColour>,
    ) -> Result<Task> {
        let (project, task) = self.task(project, task)?;
        self.store().highlight_write("tasks", task.id, colour)?;
        self.live.publish_apart(AgentEvent::TreeChanged);
        Ok(self.task(&project.name, &task.name)?.1)
    }
}

impl crate::store::Store {
    /// Writes the highlight of row `id` of `table`, which is `tasks` or `sessions`.
    pub(crate) fn highlight_write(
        &self,
        table: &'static str,
        id: i64,
        colour: Option<MarkColour>,
    ) -> Result<()> {
        let sql = format!("UPDATE {table} SET highlight = ?2 WHERE id = ?1");
        self.conn
            .execute(&sql, rusqlite::params![id, colour.map(MarkColour::as_str)])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_highlight_is_a_known_colour_or_none() {
        assert_eq!(highlight_named(" Blue ").ok(), Some(Some(MarkColour::Blue)));
        assert_eq!(highlight_named("NONE").ok(), Some(None));
        let refused = highlight_named("pink").err().map(|e| e.to_string());
        assert!(
            refused.is_some_and(|said| said.contains("green, blue, yellow, red, purple")),
            "names the colours there are"
        );
    }
}
