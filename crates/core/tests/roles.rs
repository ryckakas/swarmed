//! What a session is for: its role, as the user chooses and changes it, what its agent hears of
//! it, and what a session with a role may start.

mod common;

use std::path::PathBuf;

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Auth, PLAN_TOOL, Supervision};
use swarmed_core::{
    Core, Error, MAX_DEPTH, NewTaskOptions, Session, SessionOptions, SessionSettings, hooks,
};

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
    // A reviewer plans first; the user would rather it asked, and their choice wins.
    assert_eq!(set.supervision, Supervision::Plan);
    core.session_configure(session.id, &by_hand(&set, Supervision::Ask))
        .expect("chooses");
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
    core.session_set_role(session.id, Some("implementer"))
        .expect("changes");
    core.session_stop(session.id);
    while !matches!(next(&mut bus).await.event, AgentEvent::Exited { .. }) {}
    core.session_start(session.id, Auth::Subscription)
        .expect("starts again");
    let told = answer(&core, &mut bus, session.id, "back").await;
    assert!(
        told.contains("Your role in this session is implementer."),
        "{told}"
    );
    let guidance = answer(&core, &mut bus, session.id, "instructions").await;
    assert!(guidance.contains("# Your role: implementer"), "{guidance}");

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
    // A reviewer plans first, and waits for its plan to be approved: no turn ends.
    let reviewer = core
        .peer_spawn(planner.id, "review it", None, Some("reviewer"))
        .expect("spawns");
    assert_eq!(reviewer.role.as_deref(), Some("reviewer"));
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

    // A reviewer hands the fixes it was asked for to implementers, and starts nothing else.
    let reviewing = new_session(&core, Some("reviewer"));
    let fixer = core
        .peer_spawn(reviewing.id, "fix the finding", None, None)
        .expect("spawns");
    assert_eq!(fixer.role.as_deref(), Some("implementer"));
    turn(&mut bus, fixer.id).await;
    let refused = core.peer_spawn(reviewing.id, "plan it", None, Some("planner"));
    assert!(
        matches!(&refused, Err(Error::Peers(why)) if why.contains("may start only implementer")),
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
}

/// The changes of supervision recorded in session `id`, in order.
fn supervision_changes(core: &Core, id: i64) -> Vec<Supervision> {
    let events = core.session_events(id).expect("reads");
    let changed = |event: AgentEvent| match event {
        AgentEvent::SupervisionChanged { supervision } => Some(supervision),
        _ => None,
    };
    events.into_iter().filter_map(changed).collect()
}

/// A new session of task `one` with the role called `role`, asked to start under `supervision`.
fn supervised(core: &Core, role: Option<&str>, supervision: Supervision) -> Session {
    let options = SessionOptions {
        role: role.map(String::from),
        supervision,
        ..SessionOptions::default()
    };
    core.session_new("demo", "one", &options).expect("creates")
}

/// The settings of `session` with `supervision` in place of its own, as the user changes them.
fn by_hand(session: &Session, supervision: Supervision) -> SessionSettings {
    SessionSettings {
        model: session.model.clone(),
        supervision,
        effort: session.effort,
        fast: session.fast,
    }
}

fn supervision_of(core: &Core, id: i64) -> Supervision {
    core.session(id).expect("reads").supervision
}

#[tokio::test]
async fn a_role_that_changes_no_files_holds_its_session_in_plan_mode() {
    let fx = fixture();
    let core = core_with_task(&fx).await;

    // Started with one: in plan mode, and back as asked once the role is taken off.
    let planner = supervised(&core, Some("planner"), Supervision::Auto);
    assert_eq!(planner.supervision, Supervision::Plan);
    core.session_set_role(planner.id, None)
        .expect("takes it off");
    assert_eq!(supervision_of(&core, planner.id), Supervision::Auto);
    assert_eq!(supervision_changes(&core, planner.id), [Supervision::Auto]);

    // A role that changes files holds nothing.
    let implementer = supervised(&core, Some("implementer"), Supervision::Auto);
    assert_eq!(implementer.supervision, Supervision::Auto);

    // Set while it runs: held, through a change to another role that holds the same, and put
    // back when one that holds none takes its place.
    let session = supervised(&core, None, Supervision::Ask);
    core.session_set_role(session.id, Some("reviewer"))
        .expect("sets");
    assert_eq!(supervision_of(&core, session.id), Supervision::Plan);
    core.session_set_role(session.id, Some("architect"))
        .expect("changes");
    assert_eq!(supervision_of(&core, session.id), Supervision::Plan);
    core.session_set_role(session.id, Some("implementer"))
        .expect("changes");
    assert_eq!(supervision_of(&core, session.id), Supervision::Ask);
    assert_eq!(
        supervision_changes(&core, session.id),
        [Supervision::Plan, Supervision::Ask]
    );

    // The user's own choice wins: taking the role off later leaves it.
    core.session_set_role(session.id, Some("planner"))
        .expect("sets");
    let now = core.session(session.id).expect("reads");
    core.session_configure(session.id, &by_hand(&now, Supervision::Auto))
        .expect("chooses");
    core.session_set_role(session.id, None)
        .expect("takes it off");
    assert_eq!(supervision_of(&core, session.id), Supervision::Auto);

    // Already in plan mode by the user's choice, it stays there without the role.
    let planning = supervised(&core, None, Supervision::Plan);
    core.session_set_role(planning.id, Some("planner"))
        .expect("sets");
    core.session_set_role(planning.id, None)
        .expect("takes it off");
    assert_eq!(supervision_of(&core, planning.id), Supervision::Plan);
    assert_eq!(supervision_changes(&core, planning.id), []);

    // A fork keeps the role, and what to put back.
    let planner = supervised(&core, Some("planner"), Supervision::Auto);
    let fork = core.session_fork(planner.id, None, None).expect("forks");
    assert_eq!(fork.supervision, Supervision::Plan);
    core.session_set_role(fork.id, None).expect("takes it off");
    assert_eq!(supervision_of(&core, fork.id), Supervision::Auto);
}

#[tokio::test]
async fn a_running_agent_is_put_in_its_role_s_plan_mode_and_out_of_it() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = new_session(&core, None);
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    assert_eq!(
        answer(&core, &mut bus, session.id, "hello").await,
        "echo: hello"
    );

    core.session_set_role(session.id, Some("planner"))
        .expect("sets");
    core.session_send(session.id, "add a login page")
        .expect("sends");
    let asked = requested(&mut bus).await;
    assert_eq!(asked.1, PLAN_TOOL, "the agent plans now");
    core.session_deny(session.id, &asked.0, "Not yet.")
        .expect("sends");
    turn(&mut bus, session.id).await;

    core.session_set_role(session.id, None)
        .expect("takes it off");
    assert_eq!(
        answer(&core, &mut bus, session.id, "and now?").await,
        "echo: and now?\n\n<role>\nThe user changed your role in this session. The rules of \
         your role until now, the planner's, no longer apply. You have no role now: work as \
         the user asks.\n</role>"
    );
}

#[tokio::test]
async fn a_session_with_a_role_uses_swarmed_s_session_tools_unasked_in_plan_mode_too() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let reviewer = new_session(&core, Some("reviewer"));
    assert_eq!(reviewer.supervision, Supervision::Plan);
    core.session_start(reviewer.id, Auth::Subscription)
        .expect("starts");
    for tool in [
        "send_message",
        "list_sessions",
        "spawn_session",
        "close_session",
    ] {
        let call = format!("call: mcp__swarmed__{tool}");
        assert_eq!(
            answer(&core, &mut bus, reviewer.id, &call).await,
            "called it"
        );
    }
    let events = core.session_events(reviewer.id).expect("reads");
    let asked = |event: &AgentEvent| matches!(event, AgentEvent::PermissionRequest { .. });
    assert!(!events.iter().any(asked), "never put to the user");

    // Anything else is put to the user as before.
    core.session_send(reviewer.id, "call: Bash").expect("sends");
    let (request, tool) = requested(&mut bus).await;
    assert_eq!(tool, "Bash");
    core.session_deny(reviewer.id, &request, "Not now.")
        .expect("denies");
    turn(&mut bus, reviewer.id).await;

    // And so are the session tools, in a session with no role.
    let plain = new_session(&core, None);
    core.session_start(plain.id, Auth::Subscription)
        .expect("starts");
    core.session_send(plain.id, "call: mcp__swarmed__send_message")
        .expect("sends");
    let (request, tool) = requested(&mut bus).await;
    assert_eq!(tool, "mcp__swarmed__send_message");
    core.session_allow(plain.id, &request).expect("allows");
    turn(&mut bus, plain.id).await;
}

/// The permission request the agent of a session makes next: its id and the tool it is for.
async fn requested(
    bus: &mut tokio::sync::broadcast::Receiver<swarmed_core::SessionEvent>,
) -> (String, String) {
    loop {
        let event = next(bus).await.event;
        if let AgentEvent::PermissionRequest {
            request_id,
            tool_name,
            ..
        } = event
        {
            return (request_id, tool_name);
        }
    }
}

/// What the agent of session `id`, presenting its plan for `message`, is told once the user
/// approves it, with the events of that turn.
async fn approved(
    core: &Core,
    bus: &mut tokio::sync::broadcast::Receiver<swarmed_core::SessionEvent>,
    id: i64,
    message: &str,
) -> (String, Vec<AgentEvent>) {
    core.session_start(id, Auth::Subscription).expect("starts");
    core.session_send(id, message).expect("sends");
    let (request, tool) = requested(bus).await;
    assert_eq!(tool, PLAN_TOOL);
    core.session_allow(id, &request).expect("approves");
    let events = turn(bus, id).await;
    // Approved, as every front end shows it.
    assert!(events.contains(&AgentEvent::PermissionAnswered {
        request_id: request,
        allowed: true,
    }));
    let told = events.iter().find_map(|event| match event {
        AgentEvent::ToolResult { content, .. } => Some(content.clone()),
        _ => None,
    });
    (told.expect("the plan's call ended"), events)
}

#[tokio::test]
async fn approving_the_plan_of_a_role_that_changes_no_files_hands_it_on() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();

    // A planner is told to hand it on, not to build it, and leaves plan mode for auto mode,
    // so that starting and following its implementers asks the user nothing.
    let planner = new_session(&core, Some("planner"));
    let (told, events) = approved(&core, &mut bus, planner.id, "build the export").await;
    assert!(
        told.starts_with("The user approved your plan. Your role, the planner's, changes no files"),
        "{told}"
    );
    assert!(told.contains("You are out of plan mode now"));
    assert!(told.contains("Hand each change to an implementer now with `spawn_session`"));
    assert!(events.contains(&AgentEvent::Text {
        text: "still planning".into()
    }));
    assert_eq!(supervision_of(&core, planner.id), Supervision::Auto);
    assert_eq!(supervision_changes(&core, planner.id), [Supervision::Auto]);
    // What its role took the place of is still put back when the role goes.
    core.session_set_role(planner.id, None)
        .expect("takes it off");
    assert_eq!(supervision_of(&core, planner.id), Supervision::Ask);

    // A reviewer has nothing to follow: it stays in plan mode and reports.
    let reviewer = new_session(&core, Some("reviewer"));
    let (told, _) = approved(&core, &mut bus, reviewer.id, "review the export").await;
    assert!(
        told.contains("You stay in the mode that keeps you from it."),
        "{told}"
    );
    assert!(told.contains("Report your findings now"));
    assert_eq!(supervision_of(&core, reviewer.id), Supervision::Plan);
    assert_eq!(supervision_changes(&core, reviewer.id), []);

    // An implementer the user put in plan mode carries its plan out once approved, as before.
    let implementer = supervised(&core, Some("implementer"), Supervision::Plan);
    let (told, events) = approved(&core, &mut bus, implementer.id, "build the form").await;
    assert_eq!(told, "ok");
    assert!(events.contains(&AgentEvent::Text {
        text: "carrying out the plan".into()
    }));
    assert_ne!(supervision_of(&core, implementer.id), Supervision::Plan);
}

#[tokio::test]
async fn a_role_that_starts_sessions_may_though_it_was_started_by_one_up_to_a_depth() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let user_s = supervised(&core, None, Supervision::Auto);

    // A planner another session started starts implementers, which are not held in its plan
    // mode: they start as it would without its role. It plans first, and waits for approval.
    let planner = core
        .peer_spawn(user_s.id, "plan the export", None, Some("planner"))
        .expect("spawns");
    assert!(planner.spawned);
    assert_eq!(planner.supervision, Supervision::Plan);
    let implementer = core
        .peer_spawn(planner.id, "build the form", None, None)
        .expect("a role that starts sessions may");
    assert_eq!(implementer.role.as_deref(), Some("implementer"));
    assert_eq!(implementer.supervision, Supervision::Auto);
    turn(&mut bus, implementer.id).await;
    let reviewer = core
        .peer_spawn(planner.id, "review the form", None, Some("reviewer"))
        .expect("spawns");
    assert_eq!(reviewer.supervision, Supervision::Plan);

    // Without a role, a session another one started still starts none.
    let helper = core
        .peer_spawn(user_s.id, "a part", None, None)
        .expect("spawns");
    turn(&mut bus, helper.id).await;
    let refused = core.peer_spawn(helper.id, "more", None, None);
    assert!(
        matches!(&refused, Err(Error::Peers(why)) if why.contains("cannot start sessions itself")),
        "{refused:?}"
    );

    // Sessions with a role work one for another at most MAX_DEPTH in a row.
    let top = new_session(&core, Some("architect"));
    let middle = new_session(&core, Some("planner"));
    let low = new_session(&core, Some("architect"));
    core.session_set_parent(middle.id, Some(top.id))
        .expect("works for it");
    core.session_set_parent(low.id, Some(middle.id))
        .expect("works for it");
    assert_eq!(MAX_DEPTH, 3);
    let refused = core.peer_spawn(low.id, "plan a part", None, None);
    assert!(
        matches!(&refused, Err(Error::Peers(why)) if why.contains("at most 3 in a row")),
        "{refused:?}"
    );
    // One with no role above does not count.
    core.session_set_parent(top.id, Some(user_s.id))
        .expect("works for it");
    core.session_set_role(top.id, None).expect("takes it off");
    let planner = core
        .peer_spawn(low.id, "plan a part", None, None)
        .expect("third in a row");
    assert_eq!(planner.role.as_deref(), Some("planner"));
}
