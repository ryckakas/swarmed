//! Who works for whom: a session's parent, as the user sets it, and what the agents hear of it.

mod common;

use std::path::PathBuf;

use common::{core_with_project, fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{Core, Error, MAX_SPAWNED, NewTaskOptions, Session, SessionOptions, hooks};

/// A core with a repository `demo` and two tasks in it, `one` and `two`.
async fn core_with_tasks(fx: &common::Fixture) -> Core {
    let core = core_with_project(fx).await;
    for name in ["one", "two"] {
        core.task_new("demo", name, NewTaskOptions::default(), &hooks::discard)
            .await
            .expect("test setup");
    }
    core
}

fn new_session(core: &Core, task: &str) -> Session {
    core.session_new("demo", task, &SessionOptions::default())
        .expect("creates")
}

/// The changes of whom a session works for recorded in session `id`.
fn regroupings(core: &Core, id: i64) -> Vec<AgentEvent> {
    let events = core.session_events(id).expect("reads");
    let changed = |event: &AgentEvent| matches!(event, AgentEvent::ParentChanged { .. });
    events.into_iter().filter(changed).collect()
}

/// What the agent of session `id` answers `message` with.
async fn answer(
    core: &Core,
    bus: &mut tokio::sync::broadcast::Receiver<swarmed_core::SessionEvent>,
    id: i64,
    message: &str,
) -> String {
    core.session_send(id, message).expect("sends");
    let events = turn(bus, id).await;
    let reply = events.iter().find_map(|event| match event {
        AgentEvent::Text { text } => Some(text.clone()),
        _ => None,
    });
    reply.expect("the agent replied")
}

#[tokio::test]
async fn a_session_works_for_another_of_its_repository_in_any_task() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let mut told = core.subscribe();
    let (planner, implementer) = (new_session(&core, "one"), new_session(&core, "two"));

    let set = core
        .session_set_parent(implementer.id, Some(planner.id))
        .expect("works for it");
    assert_eq!((set.parent, set.spawned), (Some(planner.id), false));
    assert_eq!(
        core.session(implementer.id).expect("reads").parent,
        Some(planner.id)
    );
    // Recorded where it applies, on both sides, and the window reads the tree again.
    let change = AgentEvent::ParentChanged {
        child: implementer.id,
        parent: Some(planner.id),
        previous: None,
    };
    assert_eq!(
        regroupings(&core, planner.id),
        std::slice::from_ref(&change)
    );
    assert_eq!(regroupings(&core, implementer.id), [change]);
    let heard = std::iter::from_fn(|| told.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        heard
            .iter()
            .any(|heard| heard.session_id == 0 && heard.event == AgentEvent::TreeChanged),
        "{heard:?}"
    );

    // The agents can find each other again: who works for whom is in what they read.
    let listed = core.peer_sessions(planner.id).expect("lists");
    assert!(
        listed.starts_with("id\tstate\ttask\tworks for\trole\tmodel\teffort\ttitle"),
        "{listed}"
    );
    let line = format!("{}\tidle\ttask two\tyou\t", implementer.id);
    assert!(listed.contains(&line), "{listed}");
    let read = core
        .peer_digest(implementer.id, planner.id, None, None)
        .expect("reads");
    let working = format!(
        "(Working for it: session {} (\"untitled\").)",
        implementer.id
    );
    assert!(read.starts_with(&working), "{read}");
    let read = core
        .peer_digest(planner.id, implementer.id, None, None)
        .expect("reads");
    let works_for = format!(
        "(Session {} works for session {}",
        implementer.id, planner.id
    );
    assert!(read.starts_with(&works_for), "{read}");

    // Set again to the same, nothing changes; set to nobody, the planner hears of it too.
    core.session_set_parent(implementer.id, Some(planner.id))
        .expect("the same");
    assert_eq!(regroupings(&core, planner.id).len(), 1);
    let unset = core
        .session_set_parent(implementer.id, None)
        .expect("works for nobody");
    assert_eq!(unset.parent, None);
    assert_eq!(regroupings(&core, planner.id).len(), 2);
    assert_eq!(regroupings(&core, implementer.id).len(), 2);
}

#[tokio::test]
async fn who_works_for_whom_stays_a_tree() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let (top, middle, bottom) = (
        new_session(&core, "one"),
        new_session(&core, "one"),
        new_session(&core, "two"),
    );
    core.session_set_parent(middle.id, Some(top.id))
        .expect("works for it");
    core.session_set_parent(bottom.id, Some(middle.id))
        .expect("works for it");
    let refused = |result: Result<Session, Error>, why: &str| {
        assert!(
            matches!(&result, Err(Error::CannotWorkFor { why: said, .. }) if said.contains(why)),
            "{result:?}"
        );
    };
    refused(core.session_set_parent(top.id, Some(top.id)), "itself");
    refused(
        core.session_set_parent(top.id, Some(bottom.id)),
        "works for this session",
    );
    refused(
        core.session_set_parent(middle.id, Some(bottom.id)),
        "works for this session",
    );
    refused(
        core.session_set_parent(top.id, Some(999)),
        "no such session",
    );

    // Not for a closed one, and a closed one is not given a parent either.
    let closed = new_session(&core, "one");
    core.session_close(closed.id, false).expect("closes");
    refused(core.session_set_parent(top.id, Some(closed.id)), "closed");
    assert!(matches!(
        core.session_set_parent(closed.id, Some(top.id)),
        Err(Error::SessionClosed(id)) if id == closed.id
    ));
    // Moving one further down to another parent is no cycle.
    core.session_set_parent(bottom.id, Some(top.id))
        .expect("moves up");
    assert!(regroupings(&core, top.id).len() >= 2);
}

#[tokio::test]
async fn a_session_does_not_work_for_one_of_another_repository() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let here = new_session(&core, "one");
    let elsewhere = fx.repo.with_file_name("other");
    std::fs::create_dir(&elsewhere).expect("test setup");
    common::git(&elsewhere, &["init", "--quiet", "-b", "main"]);
    std::fs::write(elsewhere.join("README.md"), "other\n").expect("test setup");
    common::git(&elsewhere, &["add", "."]);
    common::git(&elsewhere, &["commit", "--quiet", "-m", "first"]);
    core.project_add(&elsewhere, None).await.expect("adds");
    core.task_new("other", "there", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let there = core
        .session_new("other", "there", &SessionOptions::default())
        .expect("creates");
    let result = core.session_set_parent(there.id, Some(here.id));
    assert!(
        matches!(&result, Err(Error::CannotWorkFor { why, .. }) if why.contains("another repository")),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_session_another_starts_works_for_it_from_its_start_without_a_change_to_tell() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let (architect, planner) = (new_session(&core, "one"), new_session(&core, "two"));
    let set = core
        .session_parent_from_start(planner.id, architect.id)
        .expect("works for it");
    assert_eq!(set.parent, Some(architect.id));
    assert_eq!(regroupings(&core, planner.id), []);
    assert_eq!(regroupings(&core, architect.id), []);
    // Checked all the same.
    assert!(matches!(
        core.session_parent_from_start(architect.id, planner.id),
        Err(Error::CannotWorkFor { .. })
    ));
}

#[tokio::test]
async fn a_session_started_from_a_shell_is_held_to_what_its_starter_may_start() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let with_role = |role: &str| {
        let options = SessionOptions {
            role: Some(role.into()),
            ..SessionOptions::default()
        };
        core.session_new("demo", "one", &options).expect("creates")
    };
    let refused = |result: Result<Session, Error>, why: &str| {
        assert!(
            matches!(result, Err(Error::Peers(ref reason)) if reason.contains(why)),
            "{result:?}"
        );
    };
    // An implementer starts nothing, whatever the new session is for.
    let (implementer, architect) = (with_role("implementer"), with_role("architect"));
    refused(
        core.session_parent_from_start(architect.id, implementer.id),
        "starts no sessions",
    );
    assert_eq!(core.session(architect.id).expect("reads").parent, None);
    // A planner starts implementers, and no architect.
    let planner = with_role("planner");
    refused(
        core.session_parent_from_start(architect.id, planner.id),
        "may start only",
    );
    let set = core.session_parent_from_start(implementer.id, planner.id);
    assert_eq!(set.expect("works for it").parent, Some(planner.id));

    // And no more than may work for one session.
    let busy = new_session(&core, "one");
    for _ in 0..MAX_SPAWNED {
        let another = new_session(&core, "two");
        core.session_set_parent(another.id, Some(busy.id))
            .expect("works for it");
    }
    refused(
        core.session_parent_from_start(new_session(&core, "two").id, busy.id),
        "the most allowed",
    );
}

#[tokio::test]
async fn a_session_started_from_a_shell_is_its_starter_s_to_close_and_one_the_user_set_is_not() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let not_yours = |result: Result<(), Error>| {
        assert!(
            matches!(result, Err(Error::Peers(ref reason)) if reason.contains("not yours")),
            "{result:?}"
        );
    };
    let agent = new_session(&core, "one");
    // One the user started and set to work for the agent stays the user's.
    let users = new_session(&core, "two");
    core.session_set_parent(users.id, Some(agent.id))
        .expect("works for it");
    not_yours(core.peer_close(agent.id, users.id));

    // One its agent started from its shell is its own, moved away and back or not.
    let started = new_session(&core, "two");
    core.session_parent_from_start(started.id, agent.id)
        .expect("works for it");
    let other = new_session(&core, "one");
    core.session_set_parent(started.id, Some(other.id))
        .expect("moved");
    not_yours(core.peer_close(other.id, started.id));
    not_yours(core.peer_close(agent.id, started.id));
    core.session_set_parent(started.id, Some(agent.id))
        .expect("moved back");
    core.peer_close(agent.id, started.id).expect("closes it");
    assert_eq!(
        core.sessions_closed("demo", "two")
            .expect("lists")
            .iter()
            .map(|session| session.id)
            .collect::<Vec<_>>(),
        [started.id]
    );
}

#[tokio::test]
async fn a_deleted_parent_leaves_those_that_worked_for_it_with_none() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let (parent, child) = (new_session(&core, "one"), new_session(&core, "two"));
    core.session_set_parent(child.id, Some(parent.id))
        .expect("works for it");
    // Closed, it is still the one they worked for.
    core.session_close(parent.id, false).expect("closes");
    assert_eq!(
        core.session(child.id).expect("reads").parent,
        Some(parent.id)
    );
    core.session_delete(parent.id).expect("deletes");
    assert_eq!(core.session(child.id).expect("reads").parent, None);
}

#[tokio::test]
async fn agents_hear_who_works_with_them_at_their_start_and_with_their_next_message() {
    let fx = fixture();
    let mut core = core_with_tasks(&fx).await;
    core.set_tool_server(PathBuf::from("/usr/local/bin/swarmed"));
    let mut bus = core.subscribe();
    let (planner, implementer) = (new_session(&core, "one"), new_session(&core, "two"));
    core.session_set_parent(implementer.id, Some(planner.id))
        .expect("works for it");

    // Started after, its agent is told in its guidance, and its first message says so too.
    core.session_start(implementer.id, Auth::Subscription)
        .expect("starts");
    let first = answer(&core, &mut bus, implementer.id, "hello").await;
    assert!(first.starts_with("echo: hello\n\n<sessions>\n"), "{first}");
    let works_for = format!("You work for session {} (\"untitled\")", planner.id);
    assert!(first.contains(&works_for), "{first}");
    let guidance = answer(&core, &mut bus, implementer.id, "instructions").await;
    assert!(guidance.contains("# Who you work with"), "{guidance}");
    assert!(guidance.contains(&works_for), "{guidance}");
    // And knows which session it is, for one it starts from its shell.
    let id = answer(&core, &mut bus, implementer.id, "env:SWARMED_SESSION").await;
    assert_eq!(id, implementer.id.to_string());

    // The planner, already at work, hears of it with its next message, once.
    core.session_start(planner.id, Auth::Subscription)
        .expect("starts");
    let heard = answer(&core, &mut bus, planner.id, "anything new?").await;
    let working = format!("Working for you: session {}", implementer.id);
    assert!(heard.contains(&working), "{heard}");
    let again = answer(&core, &mut bus, planner.id, "and now?").await;
    assert_eq!(again, "echo: and now?");

    // Let go, it is told it works for nobody.
    core.session_set_parent(implementer.id, None)
        .expect("works for nobody");
    let free = answer(&core, &mut bus, implementer.id, "carry on").await;
    assert!(
        free.contains("You work for no other session now, and none works for you."),
        "{free}"
    );
}
