//! Remote Control: a Claude Code session reached from claude.ai and the Claude app.
//!
//! Claude Code can connect the conversation it runs to claude.ai, where the user opens it in a
//! browser or in the Claude app on their phone and carries on. What they type there reaches
//! the same process and is answered in the same worktree; Claude Code says it back as it takes
//! it in, and Swarmed records it as the user's, marked as typed elsewhere. Swarmed asks for it
//! per session and handles no sign-in: Claude Code uses its own, which has to be a claude.ai
//! subscription. While it is on, the conversation is kept on Anthropic's servers as well.
//!
//! It lasts as long as the agent's process; the user's wish outlives that. The session's
//! latest [`AgentEvent::RemoteControl`] says whether it is wanted, and an agent that starts
//! for a session that wants it turns it on, attached to the same session on claude.ai: after
//! a change of settings, when the next message starts it, or when Swarmed itself starts again
//! ([`Core::remote_controls_resume`]).

use std::sync::Mutex;

use crate::agent::{AgentCommand, AgentEvent, Auth, RemoteState};
use crate::agents::CLAUDE;
use crate::error::{Error, Result};
use crate::sessions::Live;
use crate::store::Store;
use crate::{Core, lock};

/// The name Remote Control's events are stored under.
const KIND: &str = "remote_control";

impl Core {
    /// Turns Remote Control of a Claude Code session on or off. On starts the session's agent
    /// with `auth` if it is not running; how it went arrives as
    /// [`AgentEvent::RemoteControl`] events, the first of which is recorded before this
    /// returns. Off ends it for this run of the agent and every later one. Must be called from
    /// within a tokio runtime.
    pub fn session_remote_control(&self, id: i64, on: bool, auth: Auth) -> Result<()> {
        let session = self.session(id)?;
        if session.agent != CLAUDE {
            return Err(Error::NoRemoteControl(session.agent));
        }
        if !on {
            return turn_off(&self.store, &self.live, id);
        }
        // An agent that starts for a session that wants it turns it on by itself.
        self.session_start(id, auth)?;
        if self.live.is_remote(id) {
            return Ok(());
        }
        turn_on(&self.store, &self.live, id)
    }

    /// Starts the agents of the open sessions that had Remote Control on when Swarmed last
    /// ran, so that they can be reached from claude.ai again without anyone writing to them
    /// here first. Only the Swarmed that keeps house does this, the one that arrived first
    /// ([`Core::arrive`]): a second one beside it would start agents the first one runs. Says
    /// how many it started. One that cannot be started has its Remote Control recorded as
    /// failed, with why. Must be called from within a tokio runtime.
    pub fn remote_controls_resume(&self, auth: &Auth) -> Result<usize> {
        if lock(&self.house).is_none() {
            return Ok(0);
        }
        let sessions = self.store().sessions_all()?;
        let mut started = 0;
        for session in sessions.iter().filter(|session| session.agent == CLAUDE) {
            started += usize::from(self.resume_remote_control(session.id, auth)?);
        }
        Ok(started)
    }

    /// Starts the agent of session `id` if it wants Remote Control and is not running, and
    /// says whether it did.
    fn resume_remote_control(&self, id: i64, auth: &Auth) -> Result<bool> {
        if !wanted(&self.store, id)? || self.live.is_running(id) {
            return Ok(false);
        }
        let Err(error) = self.session_start(id, auth.clone()) else {
            return Ok(true);
        };
        let failed = remote(RemoteState::Failed, Some(error.to_string()));
        self.live
            .note(&self.store, id, stamped(&self.store, id, failed))?;
        Ok(false)
    }

    /// Ends Remote Control of a session that is being closed: a closed session is put away,
    /// and is not to be reached from anywhere until it is reopened and turned on again.
    pub(crate) fn remote_control_closing(&self, id: i64) -> Result<()> {
        turn_off(&self.store, &self.live, id)
    }
}

/// Whether session `id` wants Remote Control on, by its latest record of it.
fn wanted(store: &Mutex<Store>, id: i64) -> Result<bool> {
    let latest = lock(store).latest_event(id, KIND)?;
    Ok(matches!(latest, Some(AgentEvent::RemoteControl { state, .. }) if state.wanted()))
}

/// An event that says how Remote Control stands, with nothing else known.
fn remote(state: RemoteState, detail: Option<String>) -> AgentEvent {
    AgentEvent::RemoteControl {
        state,
        url: None,
        remote_id: None,
        detail,
    }
}

/// Asks the running agent of session `id` to turn Remote Control on, attached again to the
/// session on claude.ai it last had, unless that failed: a failure is most likely that
/// session's, which may be gone. That it was asked is recorded first, so that the answer,
/// however soon it comes, follows it.
fn turn_on(store: &Mutex<Store>, live: &Live, id: i64) -> Result<()> {
    let (name, latest) = {
        let store = lock(store);
        let session = store.session_by_id(id)?;
        let task = store.task_by_id(session.task_id)?;
        let name = Some(session.title)
            .filter(|title| !title.trim().is_empty())
            .or_else(|| task.map(|task| task.name))
            .unwrap_or_else(|| "Swarmed".to_string());
        (name, store.latest_event(id, KIND)?)
    };
    let (remote_id, url) = match latest {
        Some(AgentEvent::RemoteControl {
            state,
            remote_id,
            url,
            ..
        }) if state != RemoteState::Failed => (remote_id, url),
        _ => (None, None),
    };
    live.remote_begins(id);
    let starting = AgentEvent::RemoteControl {
        state: RemoteState::Starting,
        url,
        remote_id: remote_id.clone(),
        detail: None,
    };
    live.note(store, id, starting)?;
    let asked = AgentCommand::RemoteControl {
        on: true,
        name,
        reattach: remote_id,
    };
    live.command(id, asked).inspect_err(|error| {
        live.remote_ends(id);
        let failed = remote(RemoteState::Failed, Some(error.to_string()));
        live.note(store, id, failed).ok();
    })
}

/// Turns Remote Control of session `id` off, if it is wanted: its agent is told if it runs,
/// and the session on claude.ai is remembered, to be attached to again when it is turned on.
fn turn_off(store: &Mutex<Store>, live: &Live, id: i64) -> Result<()> {
    let running = live.remote_ends(id);
    if !(running || wanted(store, id)?) {
        return Ok(());
    }
    if running {
        let off = AgentCommand::RemoteControl {
            on: false,
            name: String::new(),
            reattach: None,
        };
        // An agent that has just gone has nothing left to turn off.
        live.command(id, off).ok();
    }
    let off = stamped(store, id, remote(RemoteState::Off, None));
    live.note(store, id, off)
}

/// Turns Remote Control on for an agent that has just started for session `id`, if the
/// session wants it.
pub(crate) fn resume(store: &Mutex<Store>, live: &Live, id: i64) {
    if wanted(store, id).unwrap_or(false) {
        turn_on(store, live, id).ok();
    }
}

/// What the agent of session `id` reports about Remote Control, as it is recorded: with where
/// the session is, from what was recorded before, when the report does not say. Nothing while
/// it is not on for this run: what the agent says after the user turned it off, or when it
/// was never asked, is about nothing the user wants. Nothing, either, for a report that says
/// only what the latest one said, as `connected` after `ready` does. A failure ends it.
pub(crate) fn reported(
    store: &Mutex<Store>,
    live: &Live,
    id: i64,
    event: AgentEvent,
) -> Option<AgentEvent> {
    let AgentEvent::RemoteControl { state, .. } = &event else {
        return Some(event);
    };
    if !live.is_remote(id) {
        return None;
    }
    if *state == RemoteState::Failed {
        live.remote_ends(id);
    }
    let latest = latest(store, id);
    let event = stamped_from(latest.as_ref(), event);
    (latest.as_ref() != Some(&event)).then_some(event)
}

/// Records that the agent of session `id`, which had Remote Control on, has gone: it is
/// turned on again when the agent starts again.
pub(crate) fn paused(store: &Mutex<Store>, live: &Live, id: i64) {
    if live.remote_ends(id) {
        let paused = stamped(store, id, remote(RemoteState::Paused, None));
        live.note(store, id, paused).ok();
    }
}

/// `event` with the session on claude.ai and its address filled in from the session's latest
/// record of Remote Control, where it does not say them itself. Claude Code names them only
/// when it is turned on; its later reports of the connection do not.
fn stamped(store: &Mutex<Store>, id: i64, event: AgentEvent) -> AgentEvent {
    stamped_from(latest(store, id).as_ref(), event)
}

/// The session's latest record of Remote Control, if it has one.
fn latest(store: &Mutex<Store>, id: i64) -> Option<AgentEvent> {
    lock(store).latest_event(id, KIND).ok().flatten()
}

/// `event` with what `latest` knew of the session on claude.ai, where it does not say.
fn stamped_from(latest: Option<&AgentEvent>, event: AgentEvent) -> AgentEvent {
    let AgentEvent::RemoteControl {
        state,
        url,
        remote_id,
        detail,
    } = event
    else {
        return event;
    };
    let (known_url, known_id) = match latest {
        Some(AgentEvent::RemoteControl { url, remote_id, .. }) => (url.clone(), remote_id.clone()),
        _ => (None, None),
    };
    AgentEvent::RemoteControl {
        state,
        url: url.or(known_url),
        remote_id: remote_id.or(known_id),
        detail,
    }
}

/// Whether `text`, said back by Claude Code as a message it took in, is what Claude Code
/// keeps of a command rather than words of the user's: `<command-name>/compact</command-name>`
/// and the like, which it says back for a command typed on claude.ai.
pub(crate) fn is_command_record(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("<command-") || text.starts_with("<local-command-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_claude_code_keeps_of_a_command_is_not_a_message() {
        assert!(is_command_record(
            "<command-name>/compact</command-name>\n<command-message>compact</command-message>"
        ));
        assert!(is_command_record(
            "<local-command-stdout>Set model to opus</local-command-stdout>"
        ));
        assert!(!is_command_record("look at <command-name> in the docs"));
        assert!(!is_command_record("/compact"));
    }

    #[test]
    fn it_is_wanted_until_it_is_turned_off_or_fails() {
        for state in [
            RemoteState::Starting,
            RemoteState::On,
            RemoteState::Reconnecting,
            RemoteState::Paused,
        ] {
            assert!(state.wanted(), "{state:?}");
        }
        assert!(!RemoteState::Off.wanted());
        assert!(!RemoteState::Failed.wanted());
    }
}
