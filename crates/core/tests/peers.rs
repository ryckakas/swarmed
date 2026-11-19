//! What sessions can do about each other, against the scripted agent.

mod common;

use std::sync::Arc;

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    Core, Delivery, Error, MAX_SPAWNED, NewTaskOptions, Session, SessionOptions, hooks,
    serve_session_tools,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

async fn core_with_task(fx: &common::Fixture) -> Core {
    let core = core_with_project(fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core
}

fn new_session(core: &Core) -> Session {
    core.session_new("demo", "one", &SessionOptions::default())
        .expect("creates")
}

/// Waits until something listens on `socket`.
async fn listening(socket: &std::path::Path) {
    for _ in 0..200 {
        if socket.exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("nothing listens on {}", socket.display());
}

/// One request to the core as the tool server makes it, and the core's answer.
async fn asked(socket: std::path::PathBuf, request: serde_json::Value) -> serde_json::Value {
    let mut stream = UnixStream::connect(&socket).await.expect("connects");
    stream
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("writes");
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .await
        .expect("reads");
    serde_json::from_str(&line).expect("json")
}

#[tokio::test]
async fn sessions_see_each_other_and_read_what_was_said() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let (first, second) = (new_session(&core), new_session(&core));
    core.session_start(second.id, Auth::Subscription)
        .expect("starts");
    core.session_send(second.id, "hello").expect("sends");
    turn(&mut bus, second.id).await;

    let listed = core.peer_sessions(first.id).expect("lists");
    let lines: Vec<&str> = listed.lines().collect();
    assert_eq!(lines.len(), 3, "{listed}");
    assert!(
        lines[1].starts_with(&format!("{} (you)\tidle\ttask one", first.id)),
        "{listed}"
    );
    // The other one has answered and nobody has looked.
    assert!(
        lines[2].contains("finished, not yet read by the user"),
        "{listed}"
    );
    assert!(lines[2].ends_with("hello"), "{listed}");

    let digest = core.peer_digest(second.id, None).expect("reads");
    assert_eq!(digest, "User: hello\n\nAgent: echo: hello");
    assert_eq!(
        core.peer_digest(second.id, Some(1)).expect("reads"),
        "Agent: echo: hello"
    );
    assert!(
        core.peer_digest(first.id, None)
            .expect("reads")
            .starts_with("Nothing")
    );
    assert!(matches!(
        core.peer_sessions(404),
        Err(Error::SessionNotFound(404))
    ));
}

#[tokio::test]
async fn a_message_reaches_an_idle_session_at_once_and_a_busy_one_when_it_is_done() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let (first, second) = (new_session(&core), new_session(&core));
    core.session_start(first.id, Auth::Subscription)
        .expect("starts");
    core.session_send(first.id, "my own work").expect("sends");
    turn(&mut bus, first.id).await;

    // Idle, and not even running: it is started and gets the message, marked with its sender.
    let sent = core.peer_message(first.id, second.id, "the API changed");
    assert_eq!(sent.expect("delivers"), Delivery::Sent);
    let events = turn(&mut bus, second.id).await;
    let Some(AgentEvent::UserMessage { text: got }) = events.first() else {
        panic!("no message: {events:?}");
    };
    assert!(got.starts_with(&format!(
        "Message from session {} (\"my own work\")",
        first.id
    )));
    assert!(got.ends_with("the API changed"), "{got}");

    // Busy: the message waits in its queue.
    core.session_send(second.id, "slow").expect("sends");
    next(&mut bus).await;
    let queued = core.peer_message(first.id, second.id, "one more thing");
    assert_eq!(queued.expect("queues"), Delivery::Queued);
    assert_eq!(core.session_queue(second.id).expect("reads").len(), 1);

    assert!(matches!(
        core.peer_message(first.id, first.id, "hi"),
        Err(Error::Peers(_))
    ));
    assert!(matches!(
        core.peer_message(first.id, second.id, "  "),
        Err(Error::EmptyMessage)
    ));
}

#[tokio::test]
async fn a_session_starts_others_beside_itself_within_limits() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let parent = new_session(&core);

    let child = core
        .peer_spawn(parent.id, "write the tests", Some("Tests"))
        .expect("spawns");
    assert_eq!(child.task_id, parent.task_id);
    assert_eq!(child.spawned_by, Some(parent.id));
    assert_eq!(child.title, "Tests");
    // It is running on its prompt, and knows whom to report to.
    let events = turn(&mut bus, child.id).await;
    let Some(AgentEvent::UserMessage { text: prompt }) = events.first() else {
        panic!("no prompt: {events:?}");
    };
    assert!(prompt.starts_with("write the tests\n\n(You were started by session"));
    assert!(
        prompt.contains(&format!("to session {}", parent.id)),
        "{prompt}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Text { .. }))
    );

    // Not a tree that grows by itself: one level, and a handful at a time.
    assert!(matches!(
        core.peer_spawn(child.id, "more", None),
        Err(Error::Peers(reason)) if reason.contains("cannot start sessions")
    ));
    for _ in 1..MAX_SPAWNED {
        core.peer_spawn(parent.id, "another part", None)
            .expect("spawns");
    }
    assert!(matches!(
        core.peer_spawn(parent.id, "one too many", None),
        Err(Error::Peers(reason)) if reason.contains("most allowed")
    ));
    // Closing one makes room again.
    core.session_close(child.id).expect("closes");
    core.peer_spawn(parent.id, "room again", None)
        .expect("spawns");
}

#[tokio::test]
async fn a_session_closes_only_what_it_started_and_only_when_that_is_done() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let (parent, stranger) = (new_session(&core), new_session(&core));
    let child = core.peer_spawn(parent.id, "slow", None).expect("spawns");
    // The child's turn only ends when it is interrupted: it is at work.
    next(&mut bus).await;
    assert!(matches!(
        core.peer_close(parent.id, child.id),
        Err(Error::Peers(reason)) if reason.contains("still at work")
    ));
    core.session_interrupt(child.id).expect("interrupts");
    turn(&mut bus, child.id).await;

    // Not the user's sessions, not another agent's, not its own starter.
    for (from, target) in [
        (parent.id, stranger.id),
        (stranger.id, child.id),
        (child.id, parent.id),
    ] {
        assert!(matches!(
            core.peer_close(from, target),
            Err(Error::Peers(reason)) if reason.contains("not yours")
        ));
    }

    core.peer_close(parent.id, child.id).expect("closes");
    let open: Vec<i64> = core
        .sessions("demo", "one")
        .expect("lists")
        .iter()
        .map(|session| session.id)
        .collect();
    assert_eq!(open, [parent.id, stranger.id]);
    // Put away, not thrown away: it is listed as closed, counted, and comes back whole.
    let closed = core.sessions_closed("demo", "one").expect("lists");
    assert_eq!(closed.iter().map(|s| s.id).collect::<Vec<_>>(), [child.id]);
    assert_eq!(core.overview().expect("reads")[0].tasks[0].closed, 1);
    let back = core.session_reopen(child.id).expect("reopens");
    assert_eq!(back.spawned_by, Some(parent.id));
    assert_eq!(core.sessions("demo", "one").expect("lists").len(), 3);
    assert_eq!(core.overview().expect("reads")[0].tasks[0].closed, 0);
    assert_ne!(core.session_events(child.id).expect("reads").len(), 0);
}

#[tokio::test]
async fn the_tool_server_reaches_the_core_over_its_socket() {
    let fx = fixture();
    let core = Arc::new(core_with_task(&fx).await);
    let mut bus = core.subscribe();
    let (first, second) = (new_session(&core), new_session(&core));
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    listening(&socket).await;
    let ask = |request: serde_json::Value| asked(socket.clone(), request);
    let listed =
        ask(serde_json::json!({ "session": first.id, "tool": "list_sessions", "arguments": {} }))
            .await;
    assert_eq!(listed["ok"], true);
    assert!(
        listed["text"]
            .as_str()
            .is_some_and(|text| text.contains("(you)"))
    );

    let sent = ask(serde_json::json!({
        "session": first.id, "tool": "send_message",
        "arguments": { "session_id": second.id, "text": "over the wire" },
    }))
    .await;
    assert_eq!(sent["ok"], true, "{sent}");
    let events = turn(&mut bus, second.id).await;
    let echoed = |event: &AgentEvent| matches!(event, AgentEvent::Text { text } if text.ends_with("over the wire"));
    assert!(events.iter().any(echoed), "{events:?}");

    // A refusal comes back as text for the agent, not as a dropped connection.
    let refused = ask(serde_json::json!({ "session": first.id, "tool": "send_message", "arguments": { "text": "x" } })).await;
    assert_eq!(refused["ok"], false);
    assert!(
        refused["text"]
            .as_str()
            .is_some_and(|text| text.contains("session_id"))
    );
    let unknown =
        ask(serde_json::json!({ "session": first.id, "tool": "nope", "arguments": {} })).await;
    assert_eq!(unknown["ok"], false);
}
