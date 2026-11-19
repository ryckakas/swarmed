//! Session lifecycle against the scripted fake agent.

mod common;

use std::time::Duration;

use common::{core_with_project, fixture};
use swarmed_core::agent::{AgentEvent, Auth, Supervision};
use swarmed_core::{
    Core, Error, LiveSession, NewTaskOptions, Session, SessionOptions, SessionStatus, hooks,
};

async fn core_with_task(fx: &common::Fixture) -> Core {
    let core = core_with_project(fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core
}

fn new_session(core: &Core) -> Session {
    core.session_new("demo", "one", &SessionOptions::default())
        .expect("test setup")
}

/// The next event, failing the test instead of hanging if the agent goes quiet.
async fn next(live: &mut LiveSession) -> AgentEvent {
    tokio::time::timeout(Duration::from_secs(5), live.next_event())
        .await
        .expect("the agent went quiet")
        .expect("the agent exited early")
}

/// Events up to and including the end of the turn.
async fn turn(live: &mut LiveSession) -> Vec<AgentEvent> {
    let mut events = vec![];
    loop {
        let event = next(live).await;
        let done = matches!(event, AgentEvent::TurnDone { .. });
        events.push(event);
        if done {
            return events;
        }
    }
}

/// Skips ahead to the next permission request; returns its id and the tool it is for.
async fn permission_request(live: &mut LiveSession) -> (String, String) {
    loop {
        if let AgentEvent::PermissionRequest {
            request_id,
            tool_name,
            ..
        } = next(live).await
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
async fn a_turn_is_stored_and_names_the_session() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = new_session(&core);
    assert_eq!(session.status, SessionStatus::Idle);
    assert_eq!(session.supervision, Supervision::Ask);

    let mut live = core
        .session_open(session.id, Auth::Subscription)
        .expect("opens");
    live.send("hello there").expect("sends");
    let events = turn(&mut live).await;
    assert!(
        matches!(events.first(), Some(AgentEvent::Init { permission_mode, .. }) if permission_mode == "ask")
    );
    assert_eq!(events.get(1), Some(&text("echo: hello there")));

    let stored = core.session(session.id).expect("exists");
    assert_eq!(stored.title, "hello there");
    assert_eq!(stored.status, SessionStatus::Idle);
    assert_eq!(stored.agent_session_id.as_deref(), Some("fake-1"));

    let history = core.session_events(session.id).expect("exists");
    assert_eq!(
        history.first(),
        Some(&AgentEvent::UserMessage {
            text: "hello there".into()
        })
    );
    assert_eq!(history.get(1..), Some(events.as_slice()));
    assert_eq!(core.sessions("demo", "one").expect("lists"), vec![stored]);
}

#[tokio::test]
async fn a_permission_request_waits_for_the_decision() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = new_session(&core);
    let mut live = core
        .session_open(session.id, Auth::Subscription)
        .expect("opens");

    live.send("run: git push").expect("sends");
    let (request_id, tool_name) = permission_request(&mut live).await;
    assert_eq!(tool_name, "Bash");
    assert_eq!(
        core.session(session.id).expect("exists").status,
        SessionStatus::Waiting
    );

    live.allow(&request_id).expect("sends");
    let rest = turn(&mut live).await;
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

    live.send("run: rm -rf /").expect("sends");
    let (request_id, _) = permission_request(&mut live).await;
    live.deny(&request_id, "no").expect("sends");
    let rest = turn(&mut live).await;
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
    let session = new_session(&core);
    let mut live = core
        .session_open(session.id, Auth::Subscription)
        .expect("opens");

    live.send("slow").expect("sends");
    while next(&mut live).await != text("working…") {}
    assert_eq!(
        core.session(session.id).expect("exists").status,
        SessionStatus::Working
    );
    live.interrupt().expect("sends");
    let rest = turn(&mut live).await;
    assert!(matches!(
        rest.last(),
        Some(AgentEvent::TurnDone { is_error: true, .. })
    ));

    live.send("still there?").expect("sends");
    assert!(turn(&mut live).await.contains(&text("echo: still there?")));
}

#[tokio::test]
async fn reopening_continues_the_same_conversation() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = new_session(&core);
    let mut live = core
        .session_open(session.id, Auth::Subscription)
        .expect("opens");
    live.send("first").expect("sends");
    turn(&mut live).await;
    drop(live);

    let mut live = core
        .session_open(session.id, Auth::Subscription)
        .expect("opens");
    live.send("second").expect("sends");
    let events = turn(&mut live).await;
    assert!(matches!(
        events.first(),
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
        core.session_open(99, Auth::Subscription),
        Err(Error::SessionNotFound(99))
    ));
    let missing = core.session_new("demo", "nope", &SessionOptions::default());
    assert!(matches!(missing, Err(Error::TaskNotFound { .. })));
}
