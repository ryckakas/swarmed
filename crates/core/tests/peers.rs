//! What sessions can do about each other, against the scripted agent.

mod common;

use std::sync::Arc;

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    Core, Delivery, Environment, Error, Layout, MAX_SPAWNED, NewTaskOptions, Palette, Session,
    SessionEvent, SessionOptions, hooks, serve_session_tools,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::broadcast::Receiver;

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

/// Calls the `configure` tool for `session` and returns whether it worked and what it said.
async fn configure(
    socket: &std::path::Path,
    session: i64,
    arguments: serde_json::Value,
) -> (bool, String) {
    let request =
        serde_json::json!({ "session": session, "tool": "configure", "arguments": arguments });
    let reply = asked(socket.to_path_buf(), request).await;
    let text = reply["text"].as_str().unwrap_or_default().to_string();
    (reply["ok"] == true, text)
}

/// Skips ahead to the next proposed change; returns its id, its title and its detail.
async fn proposed(bus: &mut Receiver<SessionEvent>) -> (String, String, String) {
    loop {
        if let AgentEvent::ConfigProposed {
            request_id,
            title,
            detail,
        } = next(bus).await.event
        {
            return (request_id, title, detail);
        }
    }
}

#[tokio::test]
async fn an_agent_changes_what_is_harmless_at_once() {
    let fx = fixture();
    let core = Arc::new(core_with_task(&fx).await);
    let session = new_session(&core);
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    listening(&socket).await;
    let json = |value: serde_json::Value| value;

    let (ok, _) = configure(
        &socket,
        session.id,
        json(serde_json::json!({ "kind": "prompt", "op": "save", "name": "review", "text": "Review it." })),
    )
    .await;
    assert!(ok);
    assert_eq!(core.templates().expect("reads")[0].text, "Review it.");
    let (_, listed) = configure(
        &socket,
        session.id,
        json(serde_json::json!({ "kind": "prompt", "op": "list" })),
    )
    .await;
    assert_eq!(listed, "/review: Review it.");

    let (ok, _) = configure(
        &socket,
        session.id,
        json(serde_json::json!({ "kind": "action", "op": "save", "name": "Review", "icon": "🔍", "text": "Review the changes." })),
    )
    .await;
    assert!(ok);
    let own = core.actions(Some("demo")).expect("reads");
    assert_eq!((own[0].name.as_str(), own[0].of_project), ("Review", true));

    let (ok, said) = configure(
        &socket,
        session.id,
        json(serde_json::json!({ "kind": "appearance", "op": "save", "palette": "Forest", "layout": "senior", "text_size": 110 })),
    )
    .await;
    assert!(ok, "{said}");
    let settings = core.settings().expect("reads");
    assert_eq!(
        (settings.palette, settings.layout, settings.text_scale),
        (Palette::Forest, Layout::Senior, 110)
    );
    let layout = serde_json::json!({ "kind": "layout", "op": "save", "json": { "base": "senior", "icons": 140 } });
    let (ok, said) = configure(&socket, session.id, json(layout)).await;
    assert!(ok, "{said}");
    assert_eq!(core.settings().expect("reads").layout, Layout::Custom);
    assert_eq!(core.custom_layout().expect("reads").icons, Some(140));
    let unreadable = serde_json::json!({ "kind": "layout", "op": "save",
        "json": { "dark": { "dusk": "#202020", "text": "#303030" } } });
    let (ok, said) = configure(&socket, session.id, json(unreadable)).await;
    assert!(!ok && said.contains("cannot be read"), "{said}");
    assert_eq!(core.custom_layout().expect("reads").icons, Some(140));
    let (ok, _) = configure(
        &socket,
        session.id,
        json(
            serde_json::json!({ "kind": "status_line", "op": "save", "items": ["model", "cost"] }),
        ),
    )
    .await;
    assert!(ok);
    assert_eq!(core.settings().expect("reads").status_items.len(), 2);

    for refused in [
        serde_json::json!({ "kind": "api_key", "op": "list" }),
        serde_json::json!({ "kind": "appearance", "op": "save", "palette": "pink" }),
        serde_json::json!({ "kind": "prompt", "op": "rename" }),
    ] {
        assert!(!configure(&socket, session.id, refused).await.0);
    }
}

#[tokio::test]
async fn what_outlives_the_session_is_only_proposed_until_the_user_accepts_it() {
    let fx = fixture();
    let core = Arc::new(core_with_task(&fx).await);
    let mut bus = core.subscribe();
    let session = new_session(&core);
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    listening(&socket).await;

    let rule = serde_json::json!({ "kind": "rule", "op": "save", "text": "never use em dashes" });
    let (ok, said) = configure(&socket, session.id, rule.clone()).await;
    assert!(ok);
    assert!(said.contains("NOT in effect"));
    let (declined, title, detail) = proposed(&mut bus).await;
    assert_eq!(title, "Add this rule for every agent?");
    assert_eq!(detail, "never use em dashes");
    assert_eq!(core.settings().expect("reads").rules, "");

    core.config_answer(&declined, false).expect("declines");
    assert_eq!(core.settings().expect("reads").rules, "");
    // An answer is given once.
    assert!(core.config_answer(&declined, true).is_err());

    configure(&socket, session.id, rule).await;
    let (accepted, ..) = proposed(&mut bus).await;
    core.config_answer(&accepted, true).expect("accepts");
    assert_eq!(core.settings().expect("reads").rules, "never use em dashes");

    let command = serde_json::json!({
        "kind": "action", "op": "save", "name": "Nix env", "command": "nix develop", "everywhere": true
    });
    assert!(configure(&socket, session.id, command).await.0);
    let (button, title, detail) = proposed(&mut bus).await;
    assert!(title.contains("every repository"));
    assert_eq!(detail, "nix develop");
    assert_eq!(core.actions(None).expect("reads"), []);
    core.config_answer(&button, true).expect("accepts");
    assert_eq!(core.actions(None).expect("reads")[0].command, "nix develop");

    let script = serde_json::json!({ "kind": "status_line", "op": "save", "command": "echo hi" });
    assert!(configure(&socket, session.id, script).await.0);
    let (script, ..) = proposed(&mut bus).await;
    assert_eq!(core.settings().expect("reads").status_command, "");
    core.config_answer(&script, true).expect("accepts");
    assert_eq!(core.settings().expect("reads").status_command, "echo hi");

    let gone = serde_json::json!({ "kind": "rule", "op": "remove", "text": "no such rule" });
    assert!(!configure(&socket, session.id, gone).await.0);
}

#[tokio::test]
async fn an_environment_is_proposed_and_its_variables_stay_out_of_what_an_agent_reads() {
    let fx = fixture();
    let core = Arc::new(core_with_task(&fx).await);
    let mut bus = core.subscribe();
    let session = new_session(&core);
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    listening(&socket).await;
    let secret = Environment {
        start: "make up".to_string(),
        env: [("API_TOKEN".to_string(), "s3cret-value".to_string())].into(),
        ..Environment::default()
    };
    core.environment_save("demo", "local", &secret)
        .expect("test setup");

    let list = serde_json::json!({ "kind": "environment", "op": "list" });
    let (_, listed) = configure(&socket, session.id, list).await;
    assert!(listed.contains("start: make up"));
    assert!(listed.contains("API_TOKEN"));
    assert!(!listed.contains("s3cret-value"));

    let change = serde_json::json!({
        "kind": "environment", "op": "save", "name": "local", "setup": "make deps"
    });
    assert!(configure(&socket, session.id, change).await.0);
    let (request, _, detail) = proposed(&mut bus).await;
    assert!(detail.contains("setup: make deps"));
    assert!(!detail.contains("s3cret-value"));
    core.config_answer(&request, true).expect("accepts");
    let kept = &core.environments("demo").expect("reads")["local"];
    // What the call did not name is as it was, the variables included.
    assert_eq!(
        (kept.setup.as_str(), kept.start.as_str()),
        ("make deps", "make up")
    );
    assert_eq!(kept.env["API_TOKEN"], "s3cret-value");
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
    let marked = ask(serde_json::json!({
        "session": first.id, "tool": "set_mark",
        "arguments": { "colour": "Green", "label": "e2e passed" }
    }))
    .await;
    assert_eq!(marked["ok"], true);
    let mark = core
        .session(first.id)
        .expect("exists")
        .mark
        .expect("marked");
    assert_eq!(
        (mark.colour.as_str(), mark.label.as_str()),
        ("green", "e2e passed")
    );
    assert_eq!(core.session(second.id).expect("exists").mark, None);
    let pink = ask(serde_json::json!({
        "session": first.id, "tool": "set_mark", "arguments": { "colour": "pink" }
    }))
    .await;
    assert_eq!(pink["ok"], false);
    let off = ask(serde_json::json!({
        "session": first.id, "tool": "set_mark", "arguments": { "color": "none" }
    }))
    .await;
    assert_eq!(off["ok"], true);
    assert_eq!(core.session(first.id).expect("exists").mark, None);
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
