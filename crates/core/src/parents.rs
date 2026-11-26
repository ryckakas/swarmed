//! Who works for whom. Each session can work for one other, its parent: the session whose agent
//! started it (`spawn_session`, `start_task`, or `swarmed session run` in an agent's shell), or
//! one the user chose. The parents make a tree, across the tasks of one repository, which the
//! sidebar draws and the agents are told of.
//!
//! What the user chooses is checked here: no session works for itself or for one that works for
//! it, however far up, for a closed one, or for one of another repository. Each change is
//! recorded as [`AgentEvent::ParentChanged`] in the sessions it concerns, and their agents are
//! told who works with them now along with their next message, so that one given a new parent,
//! or one that lost a session working for it, does not carry on as before.

use crate::Core;
use crate::agent::AgentEvent;
use crate::error::{Error, Result};
use crate::sessions::Session;
use crate::store::Store;

/// How far up a chain of parents is followed at most. No cycle can be made, but one written by
/// hand into the database must not keep a check from ending: a chain this long is taken for one.
const MAX_CHAIN: usize = 64;

/// The heading of what an agent is told at its start of who works with it.
const HEADING: &str = "# Who you work with";

/// Who works with a session: the session it works for, and the open ones that work for it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Relations {
    /// The session it works for, if any, and whether that one is closed.
    pub(crate) parent: Option<(Session, bool)>,
    /// The open sessions that work for it, oldest first.
    pub(crate) children: Vec<Session>,
}

/// A session as an agent is told of it: its number, and its title in quotes.
fn named(session: &Session) -> String {
    let title = if session.title.is_empty() {
        "untitled"
    } else {
        &session.title
    };
    format!("session {} (\"{title}\")", session.id)
}

/// Who works with a session, in sentences for its agent; `None` for nobody.
fn sentences(relations: &Relations) -> Option<String> {
    let parent = relations.parent.as_ref().map(|(parent, closed)| {
        if *closed {
            format!(
                "You work for {}, which is closed now: report to the user instead.",
                named(parent)
            )
        } else {
            format!(
                "You work for {}, which handed you your work. When you are done, report what \
                 you did and found to it with `send_message`.",
                named(parent)
            )
        }
    });
    let children = match relations.children.as_slice() {
        [] => None,
        children => {
            let listed: Vec<String> = children.iter().map(named).collect();
            Some(format!(
                "Working for you: {}. They report back to you with messages, and \
                 `read_session` shows what they are at.",
                listed.join(", ")
            ))
        }
    };
    let lines: Vec<String> = [parent, children].into_iter().flatten().collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// What an agent is told of who works with it when it starts, as part of Swarmed's guidance;
/// `None` when nobody does.
pub(crate) fn guidance(relations: &Relations) -> Option<String> {
    sentences(relations).map(|said| format!("{HEADING}\n\n{said}"))
}

/// What an agent is told along with its next message when who works with it changed since its
/// last: who does now, nobody included.
pub(crate) fn note(relations: &Relations) -> String {
    let now = sentences(relations)
        .unwrap_or_else(|| "You work for no other session now, and none works for you.".into());
    format!(
        "<sessions>\nWho you work with among the user's agent sessions has changed. As it \
         stands now:\n{now}\n</sessions>"
    )
}

/// Who works with session `id`, in a line for another session's agent that reads it; `None`
/// for nobody.
pub(crate) fn about(id: i64, relations: &Relations) -> Option<String> {
    let parent = relations.parent.as_ref().map(|(parent, closed)| {
        let closed = if *closed { ", which is closed" } else { "" };
        format!("Session {id} works for {}{closed}.", named(parent))
    });
    let children = (!relations.children.is_empty()).then(|| {
        let listed: Vec<String> = relations.children.iter().map(named).collect();
        format!("Working for it: {}.", listed.join(", "))
    });
    let parts: Vec<String> = [parent, children].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| format!("({})", parts.join(" ")))
}

/// Whether who works with a session changed since the last message its agent was sent, by
/// `events`, its history so far: then it is told along with the next one.
pub(crate) fn regrouped(events: &[AgentEvent]) -> bool {
    events
        .iter()
        .rev()
        .take_while(|event| !matches!(event, AgentEvent::UserMessage { .. }))
        .any(|event| matches!(event, AgentEvent::ParentChanged { .. }))
}

/// Whether following `parent_of` up from `start` reaches `target`, `start` itself included. A
/// chain longer than [`MAX_CHAIN`] is taken for a cycle, and so as reaching it.
fn reaches(start: i64, target: i64, parent_of: impl Fn(i64) -> Option<i64>) -> bool {
    let mut at = Some(start);
    for _ in 0..MAX_CHAIN {
        match at {
            None => return false,
            Some(now) if now == target => return true,
            Some(now) => at = parent_of(now),
        }
    }
    true
}

/// A task's sessions in the order the sidebar lists them, each with how deep it is nested.
///
/// A session that works for another of them comes right after that one and the others that
/// work for it, one step deeper; one whose parent is not among them stays in line. `sessions`
/// are in the task's own order, which each level keeps.
#[must_use]
pub fn nested_sessions(sessions: &[Session]) -> Vec<(&Session, usize)> {
    let here = |id: i64| sessions.iter().any(|session| session.id == id);
    let under = |session: &Session, parent: Option<i64>| match session.parent {
        Some(by) if by != session.id && here(by) => Some(by) == parent,
        _ => parent.is_none(),
    };
    let mut listed = Vec::with_capacity(sessions.len());
    // Depth first, the next level of each session right under it. A cycle written by hand into
    // the database has no root to start from, and is left out rather than followed for ever.
    let mut stack: Vec<(&Session, usize)> = sessions
        .iter()
        .rev()
        .filter(|session| under(session, None))
        .map(|session| (session, 0))
        .collect();
    while let Some((session, depth)) = stack.pop() {
        if listed
            .iter()
            .any(|(seen, _): &(&Session, usize)| seen.id == session.id)
        {
            continue;
        }
        listed.push((session, depth));
        let children = sessions
            .iter()
            .rev()
            .filter(|child| under(child, Some(session.id)));
        stack.extend(children.map(|child| (child, depth + 1)));
    }
    listed
}

impl Store {
    /// Who works with session `id`: the session it works for, and the open ones that work for
    /// it. A parent that was deleted is none.
    pub(crate) fn relations(&self, id: i64) -> Result<Relations> {
        let session = self.session_by_id(id)?;
        let parent = match session.parent.filter(|parent| *parent != id) {
            Some(parent) => match self.session_by_id(parent) {
                Ok(found) => Some((found, self.session_is_closed(parent)?)),
                Err(Error::SessionNotFound(_)) => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        let children = self.session_children(id)?;
        Ok(Relations { parent, children })
    }
}

impl Core {
    /// Has session `id` work for session `parent`, or with `None` for nobody, as the user
    /// chooses: its agent reports to that one, and the sidebar lists it under it. Refused for a
    /// closed session, and for a parent that is not another open session of the same
    /// repository, in a task that is not archived, or that works for `id` itself, however far
    /// up ([`Error::CannotWorkFor`]): who works for whom stays a tree.
    ///
    /// The change is recorded as [`AgentEvent::ParentChanged`] in session `id` and in the ones
    /// it worked for and works for now, whose agents are told who works with them along with
    /// their next message, and front ends are told to read the tree again.
    pub fn session_set_parent(&self, id: i64, parent: Option<i64>) -> Result<Session> {
        let child = self.session(id)?;
        if self.store().session_is_closed(id)? {
            return Err(Error::SessionClosed(id));
        }
        if child.parent == parent {
            return Ok(child);
        }
        if let Some(parent) = parent {
            self.may_work_for(&child, parent)?;
        }
        self.store().session_set_parent(id, parent)?;
        let previous = child.parent;
        let event = AgentEvent::ParentChanged {
            child: id,
            parent,
            previous,
        };
        let mut concerned = vec![id];
        concerned.extend(previous.filter(|previous| *previous != id));
        concerned.extend(parent);
        // One it worked for may have been deleted since.
        concerned.retain(|session| self.session(*session).is_ok());
        for session in concerned {
            self.live.note(&self.store, session, event.clone())?;
        }
        self.live.publish_apart(AgentEvent::TreeChanged);
        self.session(id)
    }

    /// Has session `id` work for session `parent` from its start, as one that `parent`'s agent
    /// starts from its shell with the command line does: checked as
    /// [`Core::session_set_parent`] checks it, and front ends are told, but no change is
    /// recorded. The agent of `parent` started it, and its own learns whom it works for as it
    /// starts. Once something was said in it, this is [`Core::session_set_parent`].
    pub fn session_parent_from_start(&self, id: i64, parent: i64) -> Result<Session> {
        let said = self.session_events(id)?;
        if said
            .iter()
            .any(|event| matches!(event, AgentEvent::UserMessage { .. }))
        {
            return self.session_set_parent(id, Some(parent));
        }
        let child = self.session(id)?;
        if self.store().session_is_closed(id)? {
            return Err(Error::SessionClosed(id));
        }
        self.may_work_for(&child, parent)?;
        self.store().session_set_parent(id, Some(parent))?;
        self.live.publish_apart(AgentEvent::TreeChanged);
        self.session(id)
    }

    /// Refuses `parent` for `child` unless it is another open session of the same repository,
    /// in a task that is not archived, that does not work for `child`, however far up.
    fn may_work_for(&self, child: &Session, parent: i64) -> Result<()> {
        let refuse = |why: &str| Error::CannotWorkFor {
            child: child.id,
            parent,
            why: why.to_string(),
        };
        if parent == child.id {
            return Err(refuse("a session cannot work for itself"));
        }
        let Ok(boss) = self.session(parent) else {
            return Err(refuse("there is no such session"));
        };
        if self.store().session_is_closed(parent)? {
            return Err(refuse("it is closed"));
        }
        let (own, theirs) = (self.task_of(child)?, self.task_of(&boss)?);
        if own.project_id != theirs.project_id {
            return Err(refuse("it works in another repository"));
        }
        if theirs.archived {
            return Err(refuse("its task is archived"));
        }
        let store = self.store();
        let parent_of = |id: i64| store.session_by_id(id).ok().and_then(|found| found.parent);
        if reaches(parent, child.id, parent_of) {
            return Err(refuse(
                "it works for this session, directly or through others",
            ));
        }
        Ok(())
    }

    /// What the agent of session `id` is told at its start of who works with it, if anyone
    /// does.
    pub(crate) fn relations_guidance(&self, id: i64) -> Result<Option<String>> {
        Ok(guidance(&self.store().relations(id)?))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn session(id: i64, title: &str) -> Session {
        Session {
            id,
            task_id: 1,
            agent: "claude".into(),
            title: title.into(),
            agent_session_id: None,
            model: None,
            supervision: crate::agent::Supervision::default(),
            effort: None,
            status: crate::sessions::SessionStatus::Idle,
            unread: false,
            parent: None,
            spawned: false,
            fast: false,
            mark: None,
            waits_for_setup: false,
            pinned: false,
            continue_at: None,
            background: crate::BackgroundCount::default(),
            highlight: None,
        }
    }

    #[test]
    fn a_chain_of_parents_is_followed_up_and_a_cycle_ends() {
        // 4 works for 3, 3 for 2, 2 for 1.
        let parents = HashMap::from([(4, 3), (3, 2), (2, 1)]);
        let parent_of = |id: i64| parents.get(&id).copied();
        assert!(reaches(4, 1, parent_of), "its parent's parent's parent");
        assert!(reaches(2, 2, parent_of), "itself");
        assert!(!reaches(2, 4, parent_of), "not down");
        let round = HashMap::from([(1, 2), (2, 1)]);
        assert!(
            reaches(1, 9, |id| round.get(&id).copied()),
            "a cycle written by hand is taken as reaching anything"
        );
    }

    #[test]
    fn an_agent_is_told_whom_it_works_for_and_who_works_for_it() {
        let nobody = Relations::default();
        assert_eq!(guidance(&nobody), None);
        assert!(note(&nobody).contains("You work for no other session now"));

        let relations = Relations {
            parent: Some((session(5, "Plan the login"), false)),
            children: vec![session(7, "Build the form"), session(8, "")],
        };
        let said = guidance(&relations).expect("someone works with it");
        assert!(said.starts_with(HEADING));
        assert!(said.contains("You work for session 5 (\"Plan the login\")"));
        assert!(said.contains("`send_message`"));
        assert!(said.contains(
            "Working for you: session 7 (\"Build the form\"), session 8 (\"untitled\")."
        ));

        assert_eq!(
            about(9, &relations).as_deref(),
            Some(
                "(Session 9 works for session 5 (\"Plan the login\"). Working for it: session 7 \
                 (\"Build the form\"), session 8 (\"untitled\").)"
            )
        );
        assert_eq!(about(9, &nobody), None);

        let orphan = Relations {
            parent: Some((session(5, "Plan the login"), true)),
            children: vec![],
        };
        let said = note(&orphan);
        assert!(said.starts_with("<sessions>\n") && said.ends_with("\n</sessions>"));
        assert!(said.contains("which is closed now: report to the user instead"));
        assert!(!said.contains("Working for you"));
    }

    #[test]
    fn a_task_s_sessions_are_listed_each_under_the_one_it_works_for() {
        let with = |id: i64, parent: Option<i64>| Session {
            parent,
            ..session(id, "")
        };
        // 3 and 4 work for 1, 5 for 3, 6 for one elsewhere, and 7 for itself.
        let sessions = [
            with(1, None),
            with(2, None),
            with(3, Some(1)),
            with(4, Some(1)),
            with(5, Some(3)),
            with(6, Some(99)),
            with(7, Some(7)),
        ];
        let listed: Vec<(i64, usize)> = nested_sessions(&sessions)
            .into_iter()
            .map(|(session, depth)| (session.id, depth))
            .collect();
        assert_eq!(
            listed,
            [(1, 0), (3, 1), (5, 2), (4, 1), (2, 0), (6, 0), (7, 0)]
        );
    }

    #[test]
    fn a_change_is_told_once_with_the_next_message() {
        let changed = AgentEvent::ParentChanged {
            child: 2,
            parent: Some(1),
            previous: None,
        };
        let message = AgentEvent::UserMessage {
            text: "go on".into(),
            remote: false,
            at_reset: false,
        };
        let renamed = AgentEvent::Renamed {
            title: "Fix it".into(),
        };
        assert!(
            regrouped(std::slice::from_ref(&changed)),
            "before the first message"
        );
        assert!(regrouped(&[message.clone(), changed.clone(), renamed]));
        assert!(!regrouped(&[changed, message]), "told with that message");
        assert!(!regrouped(&[]));
    }
}
