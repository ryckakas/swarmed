//! Session lifecycle against the scripted fake agent.

mod common;

use common::{core_with_project, fixture, next, turn};
use std::sync::Arc;

use swarmed_core::agent::{
    AgentEvent, Auth, Effort, FakeBackend, Namer, Naming, PLAN_TOOL, Supervision,
};
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
async fn a_suggested_next_message_follows_the_turn_and_leaves_the_session_idle() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "suggest: run the tests")
        .expect("sends");
    turn(&mut bus, session.id).await;
    let after = next(&mut bus).await;
    let suggested = AgentEvent::Suggestion {
        text: "run the tests".into(),
    };
    assert_eq!(after.event, suggested);
    // It changes nothing about the session, and is kept with the conversation.
    assert_eq!(after.status, None);
    let stored = core.session_events(session.id).expect("exists");
    assert_eq!(stored.last(), Some(&suggested));
    assert_eq!(
        core.session(session.id).expect("exists").status,
        SessionStatus::Idle
    );
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
async fn a_finished_turn_is_unread_until_the_user_looks_or_writes() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    assert!(!session.unread);

    core.session_send(session.id, "hello").expect("sends");
    assert!(!core.session(session.id).expect("exists").unread);
    turn(&mut bus, session.id).await;
    let done = core.session(session.id).expect("exists");
    assert_eq!((done.status, done.unread), (SessionStatus::Idle, true));
    // The lists say so too.
    assert!(core.overview().expect("reads")[0].tasks[0].sessions[0].unread);

    core.session_mark_read(session.id).expect("marks");
    assert!(!core.session(session.id).expect("exists").unread);

    // Writing to a session is having seen it.
    core.session_send(session.id, "again").expect("sends");
    turn(&mut bus, session.id).await;
    assert!(core.session(session.id).expect("exists").unread);
    core.session_send(session.id, "slow").expect("sends");
    next(&mut bus).await;
    assert!(!core.session(session.id).expect("exists").unread);
    assert!(matches!(
        core.session_mark_read(404),
        Err(Error::SessionNotFound(404))
    ));
}

#[tokio::test]
async fn a_view_handed_to_the_tool_is_shown_when_the_call_returns() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    // What an earlier run left behind is not this conversation's to show.
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    let spool = fx.paths.view_spool(session.id);
    std::fs::create_dir_all(&spool).expect("dir");
    std::fs::write(spool.join("0-stale.html"), "<p>stale</p>").expect("writes");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");

    // The tool server leaves the view while the agent's call is running; however the agent
    // made that call, the view follows its result.
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    std::fs::create_dir_all(&spool).expect("dir");
    std::fs::write(spool.join("1-view.html"), "<p>chart</p>").expect("writes");
    core.session_allow(session.id, &request_id).expect("sends");
    let events = turn(&mut bus, session.id).await;
    let views: Vec<&AgentEvent> = events
        .iter()
        .filter(|event| matches!(event, AgentEvent::View { .. }))
        .collect();
    let chart = AgentEvent::View {
        html: "<p>chart</p>".into(),
    };
    assert_eq!(views, [&chart]);
    // It is part of the conversation, and it is handed over once.
    assert!(
        core.session_events(session.id)
            .expect("reads")
            .contains(&chart)
    );
    assert!(!spool.join("1-view.html").exists());
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

    fn commit_message(&self, changes: String) -> Naming {
        // What it was shown, so that a test can see it.
        Box::pin(async move { Some(format!("```\nfeat: written\n\n{changes}\n```")) })
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

    // The user's own rules are told to every agent, and come first.
    let ruled = Settings {
        html_views: false,
        rules: "  Keep answers short.\nNo em dashes.\n".to_string(),
        ..Settings::default()
    };
    core.set_settings(&ruled).expect("saves");
    let obedient = started_session(&core);
    core.session_send(obedient.id, "instructions")
        .expect("sends");
    let told = turn(&mut bus, obedient.id).await;
    let obeyed = |e: &AgentEvent| matches!(e, AgentEvent::Text { text } if text.starts_with("The user has set these rules.") && text.ends_with(":\nKeep answers short.\nNo em dashes."));
    assert!(told.iter().any(obeyed), "{told:?}");

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
        fast: false,
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
    // Every front end is told, so the session leaves their lists too; then its agent exits.
    assert_eq!(next(&mut bus).await.event, AgentEvent::Closed);
    assert!(matches!(
        next(&mut bus).await.event,
        AgentEvent::Exited { .. }
    ));
    assert_eq!(core.running_sessions(), [0_i64; 0]);
    assert_eq!(core.sessions("demo", "one").expect("lists"), vec![]);
    let overview = core.overview().expect("reads");
    assert_eq!(overview[0].tasks[0].sessions, vec![]);
    assert_eq!(core.session_events(session.id).expect("kept").len(), 6);
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

/// Skips ahead to the next change of the session's queue and returns what it holds.
async fn queue_change(bus: &mut Receiver<SessionEvent>) -> Vec<String> {
    loop {
        if let AgentEvent::Queue { messages } = next(bus).await.event {
            return messages.into_iter().map(|message| message.text).collect();
        }
    }
}

#[tokio::test]
async fn queued_messages_are_sent_one_by_one_as_turns_end() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    // The agent is held up, so what comes next has to wait.
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    core.session_enqueue(session.id, "first").expect("queues");
    let queue = core.session_enqueue(session.id, "2nd").expect("queues");
    assert_eq!(queue.len(), 2);
    // A waiting message can still be corrected, and one emptied is taken out.
    let queue = core
        .session_queue_edit(session.id, queue[1].id, "second")
        .expect("edits");
    assert_eq!(queue[1].text, "second");
    let extra = core.session_enqueue(session.id, "third").expect("queues");
    core.session_queue_edit(session.id, extra[2].id, "  ")
        .expect("removes");
    assert_eq!(core.session_queue(session.id).expect("reads").len(), 2);
    assert!(matches!(
        core.session_enqueue(session.id, " "),
        Err(Error::EmptyMessage)
    ));

    core.session_allow(session.id, &request_id).expect("sends");
    turn(&mut bus, session.id).await;
    // The turn ended: the first goes out, then, when that turn ends, the second.
    let first = turn(&mut bus, session.id).await;
    assert!(first.contains(&text("echo: first")), "{first:?}");
    let second = turn(&mut bus, session.id).await;
    assert!(second.contains(&text("echo: second")), "{second:?}");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);

    // The queue is never part of the stored conversation.
    let stored = core.session_events(session.id).expect("reads");
    assert!(
        !stored
            .iter()
            .any(|event| matches!(event, AgentEvent::Queue { .. }))
    );
}

#[tokio::test]
async fn a_stopped_turn_leaves_the_queue_for_the_user_to_send() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "slow").expect("sends");
    core.session_enqueue(session.id, "after").expect("queues");
    assert_eq!(queue_change(&mut bus).await, ["after"]);
    core.session_interrupt(session.id).expect("interrupts");
    turn(&mut bus, session.id).await;
    assert_eq!(core.session_queue(session.id).expect("reads").len(), 1);

    core.session_send_queued(session.id).expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(events.contains(&text("echo: after")), "{events:?}");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);

    // With the agent stopped the message stays where it was.
    core.session_enqueue(session.id, "later").expect("queues");
    core.session_stop(session.id);
    assert!(matches!(
        core.session_send_queued(session.id),
        Err(Error::SessionNotRunning(_))
    ));
    assert_eq!(core.session_queue(session.id).expect("reads").len(), 1);
}

#[tokio::test]
async fn a_session_left_busy_by_a_swarmed_that_ended_can_be_stopped_and_is_settled_at_start() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "slow").expect("sends");
    while core.session(session.id).expect("exists").status != SessionStatus::Working {
        next(&mut bus).await;
    }
    // That Swarmed ends in the middle of the turn: nothing gets to say the turn is over.
    drop(core);

    let later =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let status = |core: &Core| core.session(session.id).expect("exists").status;
    assert_eq!(status(&later), SessionStatus::Working);
    // Stopping it puts it right, with a turn recorded as stopped, instead of refusing.
    later.session_interrupt(session.id).expect("stops");
    assert_eq!(status(&later), SessionStatus::Idle);
    let events = later.session_events(session.id).expect("reads");
    let last = events.last().expect("recorded");
    assert!(
        matches!(last, AgentEvent::TurnDone { is_error: true, .. }),
        "{last:?}"
    );
    // An idle session that is not running still has nothing to stop.
    assert!(matches!(
        later.session_interrupt(session.id),
        Err(Error::SessionNotRunning(_))
    ));

    // And a front end that starts settles whatever is left busy, all at once.
    let other = later
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    later
        .session_start(other.id, Auth::Subscription)
        .expect("starts");
    later.session_send(other.id, "slow").expect("sends");
    let mut bus = later.subscribe();
    while status_of(&later, other.id) != SessionStatus::Working {
        next(&mut bus).await;
    }
    drop(later);
    let fresh =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    assert_eq!(fresh.sessions_settle().expect("settles"), 1);
    assert_eq!(status_of(&fresh, other.id), SessionStatus::Idle);
}

fn status_of(core: &Core, id: i64) -> SessionStatus {
    core.session(id).expect("exists").status
}

#[tokio::test]
async fn a_commit_message_is_written_from_what_changed_and_commits_nothing() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    let worktree = core.task("demo", "one").expect("exists").1.worktree_path;
    // Nothing changed, then nothing to write about; and nobody to write.
    assert!(core.task_commit_message("demo", "one", None).await.is_err());
    std::fs::write(worktree.join("notes.txt"), "one\n").expect("writes");
    assert!(core.task_commit_message("demo", "one", None).await.is_err());

    core.set_namer(Arc::new(FixedNamer("unused")));
    let message = core
        .task_commit_message("demo", "one", None)
        .await
        .expect("writes");
    // The fence is gone, and the model was shown the new file.
    assert!(
        message.starts_with("feat: written\n\ngit status"),
        "{message}"
    );
    assert!(message.contains("?? notes.txt"), "{message}");
    assert!(message.contains("new file notes.txt:\none"), "{message}");
    let left = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!((left.changed, left.unpushed), (1, 0));
}

#[tokio::test]
async fn a_new_task_is_named_by_the_namer_and_its_session_keeps_that_title() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    // Nothing to ask yet: the caller names the task itself.
    assert_eq!(core.task_name_for("demo", "x").await.expect("asks"), None);

    core.set_namer(Arc::new(FixedNamer("\"Fix Login Redirect\"")));
    let naming = core
        .task_name_for("demo", "users get stuck after logging in, please fix")
        .await
        .expect("asks")
        .expect("names");
    assert_eq!(naming.name, "fix-login-redirect");
    assert_eq!(naming.title, "Fix Login Redirect");

    // A name already taken gets a number.
    core.task_new(
        "demo",
        &naming.name,
        NewTaskOptions::default(),
        &hooks::discard,
    )
    .await
    .expect("creates");
    let again = core
        .task_name_for("demo", "same again")
        .await
        .expect("asks");
    assert_eq!(again.expect("names").name, "fix-login-redirect-2");

    // The session takes the title, and its first message does not ask for another.
    let mut bus = core.subscribe();
    let session = core
        .session_new("demo", &naming.name, &SessionOptions::default())
        .expect("creates");
    core.session_rename(session.id, &naming.title)
        .expect("titles");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;
    let title = core.session(session.id).expect("exists").title;
    assert_eq!(title, "Fix Login Redirect");

    // Switched off, nothing is asked.
    let off = Settings {
        auto_title: false,
        ..Settings::default()
    };
    core.set_settings(&off).expect("saves");
    assert_eq!(core.task_name_for("demo", "x").await.expect("asks"), None);
    assert!(matches!(
        core.task_name_for("nowhere", "x").await,
        Err(Error::ProjectNotFound(_))
    ));
}

#[tokio::test]
async fn what_was_said_is_found_again_in_open_and_closed_sessions() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let first = started_session(&core);
    core.session_send(first.id, "Why is the Zeppelin 100% \"late\"?")
        .expect("sends");
    turn(&mut bus, first.id).await;
    let second = started_session(&core);
    core.session_send(second.id, "Nothing about airships here")
        .expect("sends");
    turn(&mut bus, second.id).await;

    let hits = core.sessions_search("zeppelin").expect("searches");
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!((hit.session_id, hit.closed), (first.id, false));
    assert_eq!((hit.project.as_str(), hit.task.as_str()), ("demo", "one"));
    assert!(hit.snippet.contains("Zeppelin"));
    // What the fake agent answers repeats the message, so both sides may hold the word.
    assert!(hit.matches >= 1);

    // Signs that mean something to the database or to JSON are taken as they are typed.
    assert_eq!(
        core.sessions_search("100% \"late\"")
            .expect("searches")
            .len(),
        1
    );
    assert_eq!(core.sessions_search("100_").expect("searches"), []);
    assert_eq!(core.sessions_search("   ").expect("searches"), []);
    // A word that only stands in an event's own field names is not something anybody said.
    assert_eq!(core.sessions_search("user_message").expect("searches"), []);

    core.session_close(first.id).expect("closes");
    let closed = core.sessions_search("ZEPPELIN").expect("searches");
    assert_eq!(closed.len(), 1);
    assert!(closed[0].closed);
}

#[tokio::test]
async fn a_fork_starts_with_what_was_said_and_tells_its_agent_once() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let source = started_session(&core);
    core.session_send(source.id, "The password is swordfish")
        .expect("sends");
    turn(&mut bus, source.id).await;
    let said = core.session_events(source.id).expect("reads");

    let fork = core.session_fork(source.id, None).expect("forks");
    assert_eq!(fork.task_id, source.task_id);
    assert_eq!(fork.agent, source.agent);
    let fork_row = core.session(fork.id).expect("reads");
    assert!(fork_row.title.ends_with("(fork)"), "{}", fork_row.title);
    assert!(!fork_row.unread);
    assert_eq!(fork_row.status, SessionStatus::Idle);
    let copied = core.session_events(fork.id).expect("reads");
    assert_eq!(copied.first(), said.first());
    assert_eq!(copied.last(), Some(&AgentEvent::Forked { from: source.id }));
    // The source is as it was.
    assert_eq!(core.session_events(source.id).expect("reads"), said);

    // The fake agent repeats what it is sent: the first message carries the conversation,
    // the second does not, and what is recorded is what was typed.
    core.session_start(fork.id, Auth::Subscription)
        .expect("starts");
    core.session_send(fork.id, "echo: what was it?")
        .expect("sends");
    let first = turn(&mut bus, fork.id).await;
    assert_eq!(
        first.first(),
        Some(&AgentEvent::UserMessage {
            text: "echo: what was it?".into()
        })
    );
    let heard = |events: &[AgentEvent]| {
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Text { text } if text.contains("swordfish")))
    };
    assert!(heard(&first), "{first:?}");
    core.session_send(fork.id, "echo: and again?")
        .expect("sends");
    assert!(!heard(&turn(&mut bus, fork.id).await));
}
