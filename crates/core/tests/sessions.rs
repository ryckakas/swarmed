//! Session lifecycle against the scripted fake agent.

mod common;

use common::{core_with_project, fixture, next, turn};
use std::sync::Arc;

use swarmed_core::agent::{AgentEvent, Auth, Effort, Namer, Naming, PLAN_TOOL, Supervision};
use swarmed_core::{
    Core, Error, NewTaskOptions, Session, SessionEvent, SessionOptions, SessionSettings,
    SessionStatus, Settings, TerminalOutput, hooks,
};
use tokio::sync::broadcast::Receiver;

async fn core_with_task(fx: &common::Fixture) -> Core {
    let core = core_with_project(fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core
}

/// A new session with its agent running.
fn started_session(core: &Core) -> Session {
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("test setup");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    session
}

/// Skips ahead to the next permission request; returns its id and the tool it is for.
async fn permission_request(bus: &mut Receiver<SessionEvent>) -> (String, String) {
    loop {
        if let AgentEvent::PermissionRequest {
            request_id,
            tool_name,
            ..
        } = next(bus).await.event
        {
            return (request_id, tool_name);
        }
    }
}

fn text(text: &str) -> AgentEvent {
    AgentEvent::Text {
        text: text.to_string(),
    }
}

#[tokio::test]
async fn a_turn_is_stored_published_and_names_the_session() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    assert_eq!(session.status, SessionStatus::Idle);
    assert_eq!(session.supervision, Supervision::Ask);
    assert_eq!(core.running_sessions(), [session.id]);

    core.session_send(session.id, "hello there").expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert_eq!(
        events.first(),
        Some(&AgentEvent::UserMessage {
            text: "hello there".into()
        })
    );
    assert!(
        matches!(events.get(1), Some(AgentEvent::Init { permission_mode, .. }) if permission_mode == "ask")
    );
    assert_eq!(events.get(2), Some(&text("echo: hello there")));

    let stored = core.session(session.id).expect("exists");
    assert_eq!(stored.title, "hello there");
    assert_eq!(stored.status, SessionStatus::Idle);
    assert_eq!(stored.agent_session_id.as_deref(), Some("fake-1"));
    assert_eq!(core.session_events(session.id).expect("exists"), events);
    assert_eq!(core.sessions("demo", "one").expect("lists"), vec![stored]);
}

#[tokio::test]
async fn published_events_carry_the_status_they_lead_to() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "run: ls").expect("sends");
    assert_eq!(next(&mut bus).await.status, Some(SessionStatus::Working));
    let init = next(&mut bus).await;
    assert!(matches!(init.event, AgentEvent::Init { .. }));
    assert_eq!(init.status, None);
    assert_eq!(next(&mut bus).await.status, Some(SessionStatus::Working));
    let request = next(&mut bus).await;
    assert_eq!(request.status, Some(SessionStatus::Waiting));
    assert_eq!(request.session_id, session.id);
}

#[tokio::test]
async fn a_permission_request_waits_for_the_decision() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "run: git push")
        .expect("sends");
    let (request_id, tool_name) = permission_request(&mut bus).await;
    assert_eq!(tool_name, "Bash");
    assert_eq!(
        core.session(session.id).expect("exists").status,
        SessionStatus::Waiting
    );

    core.session_allow(session.id, &request_id).expect("sends");
    // A second answer to the same request, from a double click, changes nothing.
    core.session_allow(session.id, &request_id).expect("sends");
    let answered = next(&mut bus).await;
    let allowed = AgentEvent::PermissionAnswered {
        request_id,
        allowed: true,
    };
    assert_eq!(
        (answered.event, answered.status),
        (allowed, Some(SessionStatus::Working))
    );
    let rest = turn(&mut bus, session.id).await;
    assert!(matches!(
        rest.first(),
        Some(AgentEvent::ToolResult {
            is_error: false,
            ..
        })
    ));
    assert!(rest.contains(&text("ran it")));
    assert_eq!(
        core.session(session.id).expect("exists").status,
        SessionStatus::Idle
    );

    core.session_send(session.id, "run: rm -rf /")
        .expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    core.session_deny(session.id, &request_id, "no")
        .expect("sends");
    let rest = turn(&mut bus, session.id).await;
    assert!(matches!(
        rest.first(),
        Some(AgentEvent::PermissionAnswered { allowed: false, .. })
    ));
    assert!(matches!(
        rest.get(1),
        Some(AgentEvent::ToolResult { is_error: true, .. })
    ));
    assert!(rest.contains(&text("not allowed")));
}

#[tokio::test]
async fn an_interrupt_ends_the_turn_and_the_session_stays_usable() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "slow").expect("sends");
    while next(&mut bus).await.event != text("working…") {}
    assert_eq!(
        core.session(session.id).expect("exists").status,
        SessionStatus::Working
    );
    core.session_interrupt(session.id).expect("sends");
    let rest = turn(&mut bus, session.id).await;
    assert!(matches!(
        rest.last(),
        Some(AgentEvent::TurnDone { is_error: true, .. })
    ));

    core.session_send(session.id, "still there?")
        .expect("sends");
    assert!(
        turn(&mut bus, session.id)
            .await
            .contains(&text("echo: still there?"))
    );
}

#[tokio::test]
async fn two_sessions_run_side_by_side() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let (first, second) = (started_session(&core), started_session(&core));
    assert_eq!(core.running_sessions(), [first.id, second.id]);

    core.session_send(first.id, "slow").expect("sends");
    core.session_send(second.id, "quick").expect("sends");
    assert!(
        turn(&mut bus, second.id)
            .await
            .contains(&text("echo: quick"))
    );
    assert_eq!(
        core.session(first.id).expect("exists").status,
        SessionStatus::Working
    );
    assert_eq!(
        core.session(second.id).expect("exists").status,
        SessionStatus::Idle
    );
}

/// Names every conversation the same, after a moment, as a model would.
#[derive(Debug)]
struct FixedNamer(&'static str);

impl Namer for FixedNamer {
    fn name(&self, _prompt: String) -> Naming {
        let title = self.0.to_string();
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Some(title)
        })
    }
}

/// Skips ahead to the next change of title.
async fn renamed(bus: &mut Receiver<SessionEvent>) -> String {
    loop {
        if let AgentEvent::Renamed { title } = next(bus).await.event {
            return title;
        }
    }
}

#[tokio::test]
async fn a_session_is_named_after_its_first_message_and_can_be_renamed() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    core.set_namer(Arc::new(FixedNamer("\"Greeting the agent\"")));
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "hello there").expect("sends");
    assert_eq!(
        core.session(session.id).expect("exists").title,
        "hello there"
    );
    assert_eq!(renamed(&mut bus).await, "Greeting the agent");
    assert_eq!(
        core.session(session.id).expect("exists").title,
        "Greeting the agent"
    );

    let by_hand = core
        .session_rename(session.id, "  My own name\nsecond line ")
        .expect("renames");
    assert_eq!(by_hand.title, "My own name");
    assert_eq!(renamed(&mut bus).await, "My own name");
    // Only the first message names a session.
    core.session_send(session.id, "another").expect("sends");
    turn(&mut bus, session.id).await;
    assert_eq!(
        core.session(session.id).expect("exists").title,
        "My own name"
    );
    assert!(matches!(
        core.session_rename(session.id, "  "),
        Err(Error::EmptyTitle)
    ));
}

#[tokio::test]
async fn a_title_chosen_before_the_suggestion_arrives_is_kept_and_naming_can_be_off() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    core.set_namer(Arc::new(FixedNamer("Suggested")));
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "hello").expect("sends");
    core.session_rename(session.id, "Mine").expect("renames");
    turn(&mut bus, session.id).await;
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    assert_eq!(core.session(session.id).expect("exists").title, "Mine");

    let off = Settings {
        auto_title: false,
        ..Settings::default()
    };
    core.set_settings(&off).expect("saves");
    let quiet = started_session(&core);
    core.session_send(quiet.id, "hello again").expect("sends");
    turn(&mut bus, quiet.id).await;
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    assert_eq!(core.session(quiet.id).expect("exists").title, "hello again");
}

#[tokio::test]
async fn a_status_line_script_is_told_about_the_session_it_is_for() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;
    assert_eq!(core.status_line(session.id).await.expect("no script"), None);

    // Runs in the worktree and reads the model the agent reported from its input.
    let script = r#"printf '%s on ' "$(basename "$PWD")"; sed -E 's/.*"model":"([^"]*)".*/\1/'"#;
    let settings = Settings {
        status_command: script.to_string(),
        ..Settings::default()
    };
    core.set_settings(&settings).expect("saves");
    let shown = core.status_line(session.id).await.expect("runs");
    assert_eq!(shown.as_deref(), Some("one on fake"));
    assert!(matches!(
        core.status_line(404).await,
        Err(Error::SessionNotFound(404))
    ));
}

/// Waits until the session's terminal has printed `wanted`.
async fn printed(output: &mut Receiver<TerminalOutput>, wanted: &str) {
    let mut seen = String::new();
    let wait = async {
        while !seen.contains(wanted) {
            seen.push_str(&output.recv().await.expect("the terminal is open").data);
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(20), wait)
        .await
        .expect("the terminal never printed what was typed");
}

#[tokio::test]
async fn what_happens_in_the_terminal_travels_with_the_next_message() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let mut output = core.subscribe_terminals();
    let session = started_session(&core);

    assert_eq!(core.terminal_open(session.id, 80, 24).expect("opens"), "");
    // The sum shows the command ran: its text alone would also match what the terminal echoes.
    core.terminal_write(session.id, "echo term-$((40 + 2))\n")
        .expect("types");
    printed(&mut output, "term-42").await;
    assert!(
        core.terminal_open(session.id, 100, 30)
            .expect("reopens")
            .contains("term-42")
    );

    core.session_send(session.id, "what happened?")
        .expect("sends");
    let events = turn(&mut bus, session.id).await;
    let shared = events.iter().find_map(|event| match event {
        AgentEvent::TerminalShared { text } => Some(text.as_str()),
        _ => None,
    });
    assert!(
        shared.is_some_and(|text| text.contains("term-42")),
        "{events:?}"
    );
    let reply = events.iter().find_map(|event| match event {
        AgentEvent::Text { text } => Some(text.as_str()),
        _ => None,
    });
    let reply = reply.expect("the agent replied");
    assert!(
        reply.starts_with("echo: what happened?\n\n<terminal>"),
        "{reply}"
    );
    assert!(
        reply.contains("term-42") && reply.ends_with("</terminal>"),
        "{reply}"
    );

    // Nothing new in the terminal: the next message goes out as typed.
    core.session_send(session.id, "and now?").expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(events.contains(&text("echo: and now?")), "{events:?}");

    core.terminal_close(session.id);
    assert!(matches!(
        core.terminal_write(session.id, "ls\n"),
        Err(Error::TerminalNotOpen(id)) if id == session.id
    ));
}

#[tokio::test]
async fn agents_are_told_about_html_views_unless_the_user_turned_them_off() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "instructions")
        .expect("sends");
    let told = turn(&mut bus, session.id).await;
    assert!(
        told.iter()
            .any(|e| matches!(e, AgentEvent::Text { text } if text.contains("`html-view`"))),
        "{told:?}"
    );

    let off = Settings {
        html_views: false,
        ..Settings::default()
    };
    core.set_settings(&off).expect("saves");
    let quiet = started_session(&core);
    core.session_send(quiet.id, "instructions").expect("sends");
    assert!(turn(&mut bus, quiet.id).await.contains(&text("none")));

    let id = core.view_put("<p>hi</p>".to_string());
    assert_eq!(core.view(&id).as_deref(), Some("<p>hi</p>"));
    assert_eq!(core.view("nope"), None);
}

#[tokio::test]
async fn slash_commands_come_from_the_last_start_of_the_session_or_of_another() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    assert_eq!(core.slash_commands(None).expect("reads"), [""; 0]);

    let session = started_session(&core);
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;
    let known = ["context", "review"];
    assert_eq!(core.slash_commands(Some(session.id)).expect("reads"), known);

    let fresh = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    assert_eq!(core.slash_commands(Some(fresh.id)).expect("reads"), known);
    assert_eq!(core.slash_commands(None).expect("reads"), known);
    assert!(matches!(
        core.slash_commands(Some(404)),
        Err(Error::SessionNotFound(404))
    ));
}

#[tokio::test]
async fn settings_can_change_while_running_and_apply_to_the_next_start() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "slow").expect("sends");

    let settings = SessionSettings {
        model: Some("opus".into()),
        supervision: Supervision::Auto,
        effort: None,
    };
    let changed = core
        .session_configure(session.id, &settings)
        .expect("changes while a turn runs");
    assert_eq!(
        (changed.model.as_deref(), changed.supervision),
        (Some("opus"), Supervision::Auto)
    );
    core.session_interrupt(session.id).expect("interrupts");
    turn(&mut bus, session.id).await;

    core.session_stop(session.id);
    next(&mut bus).await;
    core.session_start(session.id, Auth::Subscription)
        .expect("restarts");
    core.session_send(session.id, "again").expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(matches!(
        events.get(1),
        Some(AgentEvent::Init { model, permission_mode, .. })
            if model == "opus" && permission_mode == "auto"
    ));
    let unknown = core.session_configure(404, &settings);
    assert!(matches!(unknown, Err(Error::SessionNotFound(404))));
}

#[tokio::test]
async fn a_new_effort_restarts_the_agent_between_turns_and_keeps_the_conversation() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "first").expect("sends");
    turn(&mut bus, session.id).await;

    // Between turns: the agent is stopped at once.
    let high = SessionSettings {
        effort: Some(Effort::High),
        ..SessionSettings::default()
    };
    let changed = core.session_configure(session.id, &high).expect("changes");
    assert_eq!(changed.effort, Some(Effort::High));
    assert!(matches!(
        next(&mut bus).await.event,
        AgentEvent::Exited { .. }
    ));
    assert_eq!(core.running_sessions(), [0_i64; 0]);
    core.session_start(session.id, Auth::Subscription)
        .expect("restarts");

    // During a turn: it is stopped when the turn is over, not before.
    core.session_send(session.id, "slow").expect("sends");
    let max = SessionSettings {
        effort: Some(Effort::Max),
        ..SessionSettings::default()
    };
    core.session_configure(session.id, &max).expect("changes");
    assert_eq!(core.running_sessions(), [session.id]);
    core.session_interrupt(session.id).expect("interrupts");
    turn(&mut bus, session.id).await;
    assert!(matches!(
        next(&mut bus).await.event,
        AgentEvent::Exited { .. }
    ));
    assert_eq!(
        core.session(session.id).expect("exists").effort,
        Some(Effort::Max)
    );
}

#[tokio::test]
async fn a_closed_session_stops_and_leaves_the_lists_but_keeps_its_history() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;

    core.session_close(session.id).expect("closes");
    assert!(matches!(
        next(&mut bus).await.event,
        AgentEvent::Exited { .. }
    ));
    assert_eq!(core.running_sessions(), [0_i64; 0]);
    assert_eq!(core.sessions("demo", "one").expect("lists"), vec![]);
    let overview = core.overview().expect("reads");
    assert_eq!(overview[0].tasks[0].sessions, vec![]);
    assert_eq!(core.session_events(session.id).expect("kept").len(), 5);
    assert!(matches!(
        core.session_close(404),
        Err(Error::SessionNotFound(404))
    ));
}

#[tokio::test]
async fn a_stopped_session_exits_and_restarting_continues_the_conversation() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "first").expect("sends");
    turn(&mut bus, session.id).await;

    core.session_stop(session.id);
    assert!(matches!(
        next(&mut bus).await.event,
        AgentEvent::Exited { code: Some(0), .. }
    ));
    assert_eq!(core.running_sessions(), [0_i64; 0]);
    let not_running = core.session_send(session.id, "nobody home");
    assert!(matches!(not_running, Err(Error::SessionNotRunning(id)) if id == session.id));

    core.session_start(session.id, Auth::Subscription)
        .expect("restarts");
    core.session_start(session.id, Auth::Subscription)
        .expect("starting twice is a no-op");
    core.session_send(session.id, "second").expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(matches!(
        events.get(1),
        Some(AgentEvent::Init { agent_session_id, .. }) if agent_session_id == "fake-1"
    ));

    let history = core.session_events(session.id).expect("exists");
    let messages: Vec<_> = history
        .iter()
        .filter_map(|event| match event {
            AgentEvent::UserMessage { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(messages, ["first", "second"]);
    assert_eq!(core.session(session.id).expect("exists").title, "first");
}

#[tokio::test]
async fn unknown_sessions_and_tasks_are_errors() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    assert!(matches!(core.session(99), Err(Error::SessionNotFound(99))));
    assert!(matches!(
        core.session_events(99),
        Err(Error::SessionNotFound(99))
    ));
    assert!(matches!(
        core.session_start(99, Auth::Subscription),
        Err(Error::SessionNotFound(99))
    ));
    assert!(matches!(
        core.session_interrupt(99),
        Err(Error::SessionNotRunning(99))
    ));
    let missing = core.session_new("demo", "nope", &SessionOptions::default());
    assert!(matches!(missing, Err(Error::TaskNotFound { .. })));
}

#[tokio::test]
async fn the_overview_nests_tasks_and_sessions_and_marks_the_running_ones() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    core.task_new("demo", "two", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let running = started_session(&core);
    let idle = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("test setup");

    let overview = core.overview().expect("reads");
    let [project] = overview.as_slice() else {
        panic!("one project expected: {overview:?}")
    };
    assert_eq!(project.project.name, "demo");
    let [one, two] = project.tasks.as_slice() else {
        panic!("two tasks expected")
    };
    assert_eq!(
        (one.task.name.as_str(), two.task.name.as_str()),
        ("one", "two")
    );
    assert_eq!(
        one.sessions.iter().map(|s| s.id).collect::<Vec<_>>(),
        [running.id, idle.id]
    );
    assert_eq!(one.running, [running.id]);
    assert_eq!(two.sessions, []);

    let json = serde_json::to_value(project).expect("serialises");
    assert_eq!(json["name"], "demo");
    assert_eq!(json["tasks"][0]["setup"], "ok");
    assert_eq!(json["tasks"][0]["sessions"][0]["status"], "idle");
    assert_eq!(json["tasks"][0]["sessions"][0]["supervision"], "ask");
}

/// A new session in plan mode, with its agent running.
fn planning_session(core: &Core) -> Session {
    let options = SessionOptions {
        supervision: Supervision::Plan,
        ..SessionOptions::default()
    };
    let session = core
        .session_new("demo", "one", &options)
        .expect("test setup");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    session
}

#[tokio::test]
async fn an_approved_plan_ends_plan_mode_and_a_refused_one_keeps_it() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = planning_session(&core);

    // Refused: the agent keeps planning.
    core.session_send(session.id, "add a login page")
        .expect("sends");
    let (request_id, tool_name) = permission_request(&mut bus).await;
    assert_eq!(tool_name, PLAN_TOOL);
    core.session_deny(session.id, &request_id, "Not yet.")
        .expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(events.contains(&text("still planning")));
    let still = core.session(session.id).expect("exists");
    assert_eq!(still.supervision, Supervision::Plan);

    // Approved: the session goes on the way new sessions start, and every front end is told.
    core.session_send(session.id, "add a login page")
        .expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    core.session_allow(session.id, &request_id).expect("sends");
    let events = turn(&mut bus, session.id).await;
    let default = Settings::default().default_supervision;
    assert!(events.contains(&AgentEvent::SupervisionChanged {
        supervision: default
    }));
    assert!(events.contains(&text("carrying out the plan")));
    let after = core.session(session.id).expect("exists");
    assert_eq!(after.supervision, default);

    // Out of plan mode, the agent acts again instead of planning.
    core.session_send(session.id, "hello").expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(events.contains(&text("echo: hello")));
}

#[tokio::test]
async fn allowing_an_ordinary_request_leaves_plan_mode_alone() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    core.session_allow(session.id, &request_id).expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::SupervisionChanged { .. }))
    );
}
