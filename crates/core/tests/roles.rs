//! What a session is for: its role, as the user chooses and changes it, what its agent hears of
//! it, and what a session with a role may start.

mod common;

use std::path::PathBuf;

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{Core, Error, NewTaskOptions, Session, SessionOptions, hooks};

/// A core with a repository `demo` and a task `one` in it, whose agents reach each other.
async fn core_with_task(fx: &common::Fixture) -> Core {
    let mut core = core_with_project(fx).await;
    core.set_tool_server(PathBuf::from("/usr/local/bin/swarmed"));
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core
}

/// A new session of task `one` with the role called `role`, or none.
fn new_session(core: &Core, role: Option<&str>) -> Session {
    let options = SessionOptions {
        role: role.map(String::from),
        ..SessionOptions::default()
    };
    core.session_new("demo", "one", &options).expect("creates")
}

/// The changes of role recorded in session `id`.
fn role_changes(core: &Core, id: i64) -> Vec<AgentEvent> {
    let events = core.session_events(id).expect("reads");
    let changed = |event: &AgentEvent| matches!(event, AgentEvent::RoleChanged { .. });
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
async fn a_role_is_chosen_at_the_start_and_one_there_is_not_is_refused() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let planner = new_session(&core, Some("planner"));
    assert_eq!(planner.role.as_deref(), Some("planner"));
    assert_eq!(
        core.session(planner.id).expect("reads").role.as_deref(),
        Some("planner")
    );
    assert_eq!(new_session(&core, None).role, None);
    let options = SessionOptions {
        role: Some("boss".into()),
        ..SessionOptions::default()
    };
    let refused = core.session_new("demo", "one", &options);
    assert!(
        matches!(&refused, Err(Error::UnknownRole { name, .. }) if name == "boss"),
        "{refused:?}"
    );
    // A fork is for the same.
    let fork = core.session_fork(planner.id, None, None).expect("forks");
    assert_eq!(fork.role.as_deref(), Some("planner"));
}

#[tokio::test]
async fn a_role_changed_while_a_session_runs_is_recorded_and_told_with_the_next_message() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let mut told = core.subscribe();
    let session = new_session(&core, None);

    // Before anything was said, nothing is recorded: the agent starts with it.
    let set = core
        .session_set_role(session.id, Some("reviewer"))
        .expect("sets");
    assert_eq!(set.role.as_deref(), Some("reviewer"));
    assert_eq!(role_changes(&core, session.id), []);
    let heard = std::iter::from_fn(|| told.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        heard
            .iter()
            .any(|heard| heard.session_id == 0 && heard.event == AgentEvent::TreeChanged),
        "{heard:?}"
    );
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    let first = answer(&core, &mut bus, session.id, "hello").await;
    assert_eq!(first, "echo: hello");
    let guidance = answer(&core, &mut bus, session.id, "instructions").await;
    assert!(guidance.contains("# Your role: reviewer"), "{guidance}");

    // Once something was said, a change is recorded where it applies and told once.
    core.session_set_role(session.id, Some("implementer"))
        .expect("changes");
    assert_eq!(
        role_changes(&core, session.id),
        [AgentEvent::RoleChanged {
            role: Some("implementer".into()),
            previous: Some("reviewer".into()),
        }]
    );
    let after = answer(&core, &mut bus, session.id, "go on").await;
    assert!(after.starts_with("echo: go on\n\n<role>\n"), "{after}");
    assert!(after.contains("the reviewer's, no longer apply"), "{after}");
    assert!(
        after.contains("Your role in this session is implementer."),
        "{after}"
    );
    let again = answer(&core, &mut bus, session.id, "and now?").await;
    assert_eq!(again, "echo: and now?");

    // The same again changes nothing; taken off, the agent works as the user asks.
    core.session_set_role(session.id, Some("implementer"))
        .expect("the same");
    assert_eq!(role_changes(&core, session.id).len(), 1);
    core.session_set_role(session.id, None)
        .expect("takes it off");
    let free = answer(&core, &mut bus, session.id, "carry on").await;
    assert!(free.contains("You have no role now"), "{free}");
    assert!(
        free.contains("the implementer's, no longer apply"),
        "{free}"
    );

    // A restarted agent has the role it has now in its guidance.
    core.session_set_role(session.id, Some("planner"))
        .expect("changes");
    core.session_stop(session.id);
    while !matches!(next(&mut bus).await.event, AgentEvent::Exited { .. }) {}
    core.session_start(session.id, Auth::Subscription)
        .expect("starts again");
    let told = answer(&core, &mut bus, session.id, "back").await;
    assert!(
        told.contains("Your role in this session is planner."),
        "{told}"
    );
    let guidance = answer(&core, &mut bus, session.id, "instructions").await;
    assert!(guidance.contains("# Your role: planner"), "{guidance}");

    assert!(matches!(
        core.session_set_role(session.id, Some("boss")),
        Err(Error::UnknownRole { .. })
    ));
    core.session_close(session.id).expect("closes");
    assert!(matches!(
        core.session_set_role(session.id, Some("reviewer")),
        Err(Error::SessionClosed(_))
    ));
}

#[tokio::test]
async fn the_guidance_holds_the_role_and_who_works_with_it() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let architect = new_session(&core, Some("architect"));
    let planner = new_session(&core, Some("planner"));
    let implementer = new_session(&core, Some("implementer"));
    core.session_parent_from_start(planner.id, architect.id)
        .expect("works for it");
    core.session_parent_from_start(implementer.id, planner.id)
        .expect("works for it");

    core.session_start(planner.id, Auth::Subscription)
        .expect("starts");
    let guidance = answer(&core, &mut bus, planner.id, "instructions").await;
    assert!(guidance.contains("# Your role: planner"), "{guidance}");
    assert!(guidance.contains("## The roles"), "{guidance}");
    assert!(guidance.contains("| | Architect | Planner |"), "{guidance}");
    assert!(guidance.contains("## Who you work with"), "{guidance}");
    let parent = format!(
        "You work for session {} (\"untitled\", architect)",
        architect.id
    );
    assert!(guidance.contains(&parent), "{guidance}");
    let child = format!(
        "Working for you: session {} (\"untitled\", implementer)",
        implementer.id
    );
    assert!(guidance.contains(&child), "{guidance}");
    // One section, not a role's and another apart that says the same.
    assert!(!guidance.contains("\n# Who you work with"), "{guidance}");

    // The others read the role too.
    let listed = core.peer_sessions(architect.id).expect("lists");
    assert!(
        listed.starts_with("id\tstate\ttask\tworks for\trole\ttitle"),
        "{listed}"
    );
    let line = format!(
        "{}\tidle\ttask one\t{}\timplementer\t",
        implementer.id, planner.id
    );
    assert!(listed.contains(&line), "{listed}");
    let read = core
        .peer_digest(architect.id, planner.id, None, None)
        .expect("reads");
    let about = format!("(Session {}'s role is planner. ", planner.id);
    assert!(read.starts_with(&about), "{read}");
}

#[tokio::test]
async fn a_session_starts_only_the_roles_its_own_may() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let planner = new_session(&core, Some("planner"));

    // Without a role asked for, its default; one it may start; not one it may not.
    let implementer = core
        .peer_spawn(planner.id, "build the form", None, None)
        .expect("spawns");
    assert_eq!(implementer.role.as_deref(), Some("implementer"));
    assert_eq!(implementer.parent, Some(planner.id));
    turn(&mut bus, implementer.id).await;
    let reviewer = core
        .peer_spawn(planner.id, "review it", None, Some("reviewer"))
        .expect("spawns");
    assert_eq!(reviewer.role.as_deref(), Some("reviewer"));
    turn(&mut bus, reviewer.id).await;
    let refused = core.peer_spawn(planner.id, "shape it all", None, Some("architect"));
    assert!(
        matches!(&refused, Err(Error::Peers(why)) if why.contains("may start only implementer or reviewer")),
        "{refused:?}"
    );
    let refused = core.peer_spawn(planner.id, "anything", None, Some("boss"));
    assert!(
        matches!(&refused, Err(Error::UnknownRole { .. })),
        "{refused:?}"
    );

    // A role that starts nobody starts nobody, even one the user started.
    let alone = new_session(&core, Some("implementer"));
    let refused = core.peer_spawn(alone.id, "help me", None, None);
    assert!(
        matches!(&refused, Err(Error::Peers(why)) if why.contains("starts no sessions")),
        "{refused:?}"
    );

    // With no role, as before: none by default, any role asked for.
    let free = new_session(&core, None);
    let plain = core
        .peer_spawn(free.id, "a part", None, None)
        .expect("spawns");
    assert_eq!(plain.role, None);
    turn(&mut bus, plain.id).await;
    let architect = core
        .peer_spawn(free.id, "shape it", None, Some("architect"))
        .expect("spawns");
    assert_eq!(architect.role.as_deref(), Some("architect"));
    turn(&mut bus, architect.id).await;
}
