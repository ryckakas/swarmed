//! Session lifecycle against the scripted fake agent.

mod common;

use std::time::Duration;

use common::{core_with_project, fixture};
use swarmed_core::agent::{AgentEvent, Auth, Supervision};
use swarmed_core::{
    Core, Error, NewTaskOptions, Session, SessionEvent, SessionOptions, SessionStatus, hooks,
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

/// The next published event, failing the test instead of hanging if the agent goes quiet.
async fn next(bus: &mut Receiver<SessionEvent>) -> SessionEvent {
    tokio::time::timeout(Duration::from_secs(5), bus.recv())
        .await
        .expect("the agent went quiet")
        .expect("the event stream closed")
}

/// One session's events up to and including the end of the turn.
async fn turn(bus: &mut Receiver<SessionEvent>, session_id: i64) -> Vec<AgentEvent> {
    let mut events = vec![];
    loop {
        let published = next(bus).await;
        if published.session_id != session_id {
            continue;
        }
        let done = matches!(published.event, AgentEvent::TurnDone { .. });
        events.push(published.event);
        if done {
            return events;
        }
    }
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
