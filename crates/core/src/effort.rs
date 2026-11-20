//! What an agent's default effort comes to, before a session has asked it.
//!
//! An agent only says how hard it thinks once it runs. Until then the best answer is what it
//! said the last time a session left the effort to it, on the same model.

use rusqlite::{OptionalExtension, params};

use crate::agent::AgentEvent;
use crate::sessions::event_from_json;
use crate::store::Store;
use crate::{Core, Result};

impl Store {
    /// What `agent` last said it runs with, in a session that chose no effort and was set to
    /// `model`.
    fn last_applied(&self, agent: &str, model: Option<&str>) -> Result<Option<AgentEvent>> {
        // The event's type is the first thing in its JSON, which keeps this a prefix match.
        let mut statement = self.conn.prepare(
            "SELECT e.event
             FROM session_events e
             JOIN sessions s ON s.id = e.session_id
             WHERE s.agent = ?1 AND s.effort IS NULL AND s.model IS ?2
               AND e.event LIKE '{\"type\":\"applied\"%'
             ORDER BY e.id DESC
             LIMIT 1",
        )?;
        let event: Option<String> = statement
            .query_row(params![agent, model], |row| row.get(0))
            .optional()?;
        Ok(event.map(|event| event_from_json(&event)))
    }
}

impl Core {
    /// The effort `agent` runs `model` with when a session leaves the effort to it, as it last
    /// reported; `model` is `None` for the agent's default model. `None` if no such session has
    /// run yet, or the agent says nothing of effort.
    pub fn effort_default(&self, agent: &str, model: Option<&str>) -> Result<Option<String>> {
        let applied = crate::lock(&self.store).last_applied(agent, model)?;
        Ok(match applied {
            Some(AgentEvent::Applied { effort, .. }) => effort,
            _ => None,
        })
    }
}
