//! The account's usage as Claude Code reports it now, asked for by the user.
//!
//! Swarmed hears the plan's usage limits and the extra usage while Claude Code works for a
//! session, so between turns they can be out of date. Only the user asking brings them up to
//! date: the session's own Claude Code is asked, and its answer is recorded in the session,
//! with nothing in the conversation. Nothing asks on its own, on a timer or when a figure is
//! looked at.

use crate::Core;
use crate::agent::{AgentCommand, Auth};
use crate::agents::CLAUDE;
use crate::error::{Error, Result};

impl Core {
    /// Asks the session's Claude Code for the account's usage now, starting it with `auth` if
    /// it is not running. The answer arrives as the session's events,
    /// [`crate::agent::AgentEvent::UsageLimits`] and [`crate::agent::AgentEvent::ExtraUsage`].
    /// Only a Claude Code session can ask: no other agent reports usage. Must be called from
    /// within a tokio runtime.
    pub fn session_usage_refresh(&self, id: i64, auth: Auth) -> Result<()> {
        let session = self.session(id)?;
        if session.agent != CLAUDE {
            return Err(Error::UsageNotReported(session.agent));
        }
        self.session_start(id, auth)?;
        self.live
            .sender(id)?
            .send(AgentCommand::AskUsage)
            .map_err(|_| Error::SessionNotRunning(id))
    }
}
