//! What sessions can do about each other, against the scripted agent.

mod common;

use std::sync::Arc;

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    CHATS, Core, Delivery, Environment, Error, Layout, LimitWarnings, MAX_SPAWNED, MathShown,
    NewTaskOptions, Palette, Session, SessionEvent, SessionOptions, Settings, ToolCalls, hooks,
    serve_session_tools,
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

/// Waits until something listens on `socket`, for as long as a busy test runner may take.
async fn listening(socket: &std::path::Path) {
    for _ in 0..1000 {
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

    let digest = core
        .peer_digest(first.id, second.id, None, None)
        .expect("reads");
    assert_eq!(digest, "User: hello\n\nAgent: echo: hello");
    // Where that is not all of it, it says which messages these are, and how to read others.
    assert_eq!(
        core.peer_digest(first.id, second.id, Some(1), None)
            .expect("reads"),
        "(Messages 2 to 2 of 2; earlier ones are read with `from`.)\n\nAgent: echo: hello"
    );
    assert!(
        core.peer_digest(second.id, first.id, None, None)
            .expect("reads")
            .starts_with("Nothing")
    );
    assert!(matches!(
        core.peer_sessions(404),
        Err(Error::SessionNotFound(404))
    ));
}

#[tokio::test]
async fn a_long_conversation_is_read_from_where_the_reader_asks() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let (reader, long) = (new_session(&core), new_session(&core));
    core.session_start(long.id, Auth::Subscription)
        .expect("starts");
    for said in ["one", "two", "three"] {
        core.session_send(long.id, said).expect("sends");
        turn(&mut bus, long.id).await;
    }

    // From its start, a few at a time, saying where to read on.
    let start = core
        .peer_digest(reader.id, long.id, Some(2), Some(1))
        .expect("reads");
    assert_eq!(
        start,
        "(Messages 1 to 2 of 6; read on with `from` 3.)\n\nUser: one\n\nAgent: echo: one"
    );
    let middle = core
        .peer_digest(reader.id, long.id, Some(2), Some(3))
        .expect("reads");
    assert!(
        middle.starts_with(
            "(Messages 3 to 4 of 6; earlier ones are read with `from`; read on with `from` 5.)"
        ),
        "{middle}"
    );
    assert!(
        middle.ends_with("User: two\n\nAgent: echo: two"),
        "{middle}"
    );
    // To its end, all of it from there.
    let end = core
        .peer_digest(reader.id, long.id, None, Some(5))
        .expect("reads");
    assert!(end.ends_with("User: three\n\nAgent: echo: three"), "{end}");
    // Past its end, it says so.
    assert_eq!(
        core.peer_digest(reader.id, long.id, None, Some(9))
            .expect("reads"),
        "That session has fewer than 9 messages."
    );
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
    let Some(AgentEvent::UserMessage { text: got, .. }) = events.first() else {
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
        .peer_spawn(parent.id, "write the tests", Some("Tests"), None)
        .expect("spawns");
    assert_eq!(child.task_id, parent.task_id);
    assert_eq!((child.parent, child.spawned), (Some(parent.id), true));
    assert_eq!(child.title, "Tests");
    // It is running on its prompt, and knows whom to report to.
    let events = turn(&mut bus, child.id).await;
    let Some(AgentEvent::UserMessage { text: prompt, .. }) = events.first() else {
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
        core.peer_spawn(child.id, "more", None, None),
        Err(Error::Peers(reason)) if reason.contains("cannot start sessions")
    ));
    for _ in 1..MAX_SPAWNED {
        core.peer_spawn(parent.id, "another part", None, None)
            .expect("spawns");
    }
    assert!(matches!(
        core.peer_spawn(parent.id, "one too many", None, None),
        Err(Error::Peers(reason)) if reason.contains("most allowed")
    ));
    // Closing one makes room again.
    core.session_close(child.id, false).expect("closes");
    core.peer_spawn(parent.id, "room again", None, None)
        .expect("spawns");
}

/// Sets how many open sessions may work for one session.
fn workers(core: &Core, max_workers: usize) {
    let settings = Settings {
        max_workers,
        ..core.settings().expect("reads")
    };
    core.set_settings(&settings).expect("saves");
}

/// Whether `result` is the refusal of one more session working for another, naming the
/// setting and how many it lets work for one.
fn refused_for_workers<T: std::fmt::Debug>(result: Result<T, Error>, most: usize) -> bool {
    let reason = match result {
        Err(Error::Peers(reason)) => reason,
        other => panic!("not refused: {other:?}"),
    };
    reason.contains("the most allowed")
        && reason.contains(&format!("let {most} work for one session"))
        && reason.contains("`max_workers`")
        && reason.contains("Workers per session")
}

#[tokio::test]
async fn how_many_may_work_for_one_session_is_the_users_setting() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let parent = new_session(&core);
    assert_eq!(core.settings().expect("reads").max_workers, MAX_SPAWNED);

    // Fewer than the default.
    workers(&core, 2);
    for _ in 0..2 {
        core.peer_spawn(parent.id, "a part", None, None)
            .expect("spawns");
    }
    assert!(refused_for_workers(
        core.peer_spawn(parent.id, "one too many", None, None),
        2
    ));

    // More than the default: those already open count, and the rest go up to the new limit.
    workers(&core, MAX_SPAWNED + 2);
    for _ in 2..MAX_SPAWNED + 2 {
        core.peer_spawn(parent.id, "another part", None, None)
            .expect("spawns");
    }
    assert_eq!(core.sessions("demo", "one").expect("lists").len(), 9);
    assert!(refused_for_workers(
        core.peer_spawn(parent.id, "one too many", None, None),
        MAX_SPAWNED + 2
    ));

    // Lowered below those already open: none is closed, and none more starts.
    workers(&core, 1);
    assert!(refused_for_workers(
        core.peer_spawn(parent.id, "still too many", None, None),
        1
    ));
    assert_eq!(core.sessions("demo", "one").expect("lists").len(), 9);

    // Out of range is refused, and the limit stays as it was.
    for out_of_range in [0, 21] {
        let settings = Settings {
            max_workers: out_of_range,
            ..core.settings().expect("reads")
        };
        assert!(matches!(
            core.set_settings(&settings),
            Err(Error::WorkersOutOfRange(n)) if n == out_of_range
        ));
    }
    assert_eq!(core.settings().expect("reads").max_workers, 1);

    // Written by hand out of range, it is the default, said with the other problems.
    std::fs::write(fx.paths.settings(), "max_workers = 50\n").expect("writes");
    assert_eq!(core.settings().expect("reads").max_workers, MAX_SPAWNED);
    let problems = core.config_problems().expect("reads");
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("`max_workers` is 50")),
        "{problems:?}"
    );
}

/// Whether `result` is the refusal an agent gets for a session it may not reach.
/// The end of the next turn of session `id`.
async fn turn_end(bus: &mut Receiver<SessionEvent>, id: i64) -> SessionEvent {
    loop {
        let event = next(bus).await;
        if event.session_id == id && matches!(event.event, AgentEvent::TurnDone { .. }) {
            return event;
        }
    }
}

#[tokio::test]
async fn a_started_session_tells_the_user_it_finished_once_nobody_else_hears_it() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let parent = new_session(&core);
    let child = core
        .peer_spawn(parent.id, "write the tests", None, None)
        .expect("spawns");
    // It reports to the session that started it, so the user is not interrupted.
    let done = turn_end(&mut bus, child.id).await;
    assert_eq!(core.notice_for(&done).expect("reads"), None);

    // With that one closed, its agent is told to report to the user, who hears of it.
    core.session_close(parent.id, false).expect("closes");
    core.session_send(child.id, "carry on").expect("sends");
    let done = turn_end(&mut bus, child.id).await;
    let notice = core.notice_for(&done).expect("reads").expect("a notice");
    assert!(notice.title.starts_with("Finished"), "{notice:?}");

    // And with it deleted, there is nobody else to hear.
    core.session_delete(parent.id).expect("deletes");
    core.session_send(child.id, "and on").expect("sends");
    let done = turn_end(&mut bus, child.id).await;
    let notice = core.notice_for(&done).expect("reads").expect("a notice");
    assert!(notice.title.starts_with("Finished"), "{notice:?}");
}

fn out_of_reach<T>(result: Result<T, Error>) -> bool {
    matches!(result, Err(Error::Peers(reason)) if reason.contains("no open session"))
}

#[tokio::test]
async fn a_closed_session_is_neither_read_nor_brought_back_by_a_message() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let parent = new_session(&core);
    let child = core
        .peer_spawn(parent.id, "a part", None, None)
        .expect("spawns");
    turn(&mut bus, child.id).await;
    core.session_close(child.id, false).expect("closes");
    while !matches!(next(&mut bus).await.event, AgentEvent::Exited { .. }) {}

    // Its agent stays stopped, whoever asks: a message, or a plain start.
    assert!(out_of_reach(
        core.peer_message(parent.id, child.id, "wake up")
    ));
    assert!(out_of_reach(
        core.peer_digest(parent.id, child.id, None, None)
    ));
    assert!(matches!(
        core.session_start(child.id, Auth::Subscription),
        Err(Error::SessionClosed(id)) if id == child.id
    ));
    assert_eq!(core.running_sessions(), Vec::<i64>::new());
    assert_eq!(core.session_queue(child.id).expect("reads"), vec![]);

    // So closing sessions and writing to them is no way past the limit.
    let mut spawned = vec![];
    for _ in 0..MAX_SPAWNED {
        let another = core.peer_spawn(parent.id, "another part", None, None);
        spawned.push(another.expect("spawns").id);
    }
    for id in &spawned {
        core.session_close(*id, false).expect("closes");
        assert!(out_of_reach(core.peer_message(parent.id, *id, "carry on")));
    }
    assert_eq!(core.running_sessions(), Vec::<i64>::new());

    // Reopened by the user, it is a session like any other again.
    core.session_reopen(child.id).expect("reopens");
    let sent = core.peer_message(parent.id, child.id, "now you may");
    assert_eq!(sent.expect("delivers"), Delivery::Sent);
    assert_eq!(core.running_sessions(), [child.id]);
    core.session_stop(child.id);
    core.session_start(child.id, Auth::Subscription)
        .expect("starts again");
}

#[tokio::test]
async fn sessions_of_another_project_are_out_of_reach() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let here = new_session(&core);
    // A second repository beside the first, with a task and a session of its own.
    let elsewhere = fx.repo.with_file_name("other");
    std::fs::create_dir(&elsewhere).expect("test setup");
    common::git(&elsewhere, &["init", "--quiet", "-b", "main"]);
    std::fs::write(elsewhere.join("README.md"), "other\n").expect("test setup");
    common::git(&elsewhere, &["add", "."]);
    common::git(&elsewhere, &["commit", "--quiet", "-m", "init"]);
    core.project_add(&elsewhere, None).await.expect("adds");
    core.task_new("other", "two", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let there = core
        .session_new("other", "two", &SessionOptions::default())
        .expect("creates");
    core.session_start(there.id, Auth::Subscription)
        .expect("starts");
    let mut bus = core.subscribe();
    core.session_send(there.id, "the secret is swordfish")
        .expect("sends");
    turn(&mut bus, there.id).await;

    // It is not in the list, and neither reading it nor writing to it gets around that.
    let listed = core.peer_sessions(here.id).expect("lists");
    assert_eq!(listed.lines().count(), 2, "{listed}");
    assert!(out_of_reach(
        core.peer_digest(here.id, there.id, None, None)
    ));
    assert!(out_of_reach(core.peer_message(here.id, there.id, "hello")));
    assert!(out_of_reach(core.peer_digest(here.id, 404, None, None)));
    let said = core.session_events(there.id).expect("reads").len();
    assert_eq!(core.session_queue(there.id).expect("reads"), vec![]);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(core.session_events(there.id).expect("reads").len(), said);
    // Within its own project it is read like any other.
    let own = core
        .peer_digest(there.id, there.id, None, None)
        .expect("reads");
    assert!(own.contains("swordfish"), "{own}");
}

#[tokio::test]
async fn a_chat_reaches_every_repository_and_each_can_answer_but_not_read_it() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let in_repository = new_session(&core);
    let free = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    core.task_new(CHATS, "question", free, &hooks::discard)
        .await
        .expect("makes a chat");
    let chat = core
        .session_new(CHATS, "question", &SessionOptions::default())
        .expect("creates");

    // The chat lists the repository's sessions, with their repository, and itself.
    let listed = core.peer_sessions(chat.id).expect("lists");
    assert!(listed.contains("task demo/one"), "{listed}");
    assert!(listed.contains("chat question"), "{listed}");
    core.peer_digest(chat.id, in_repository.id, None, None)
        .expect("reads a repository's session");
    // A session in a repository can answer the chat that wrote to it.
    let answered = core
        .peer_message(in_repository.id, chat.id, "Here is what you asked for.")
        .expect("writes to the chat");
    assert_eq!(answered, Delivery::Sent);
    // It can neither list nor read one: a chat may hold what it read of another repository.
    let back = core.peer_sessions(in_repository.id).expect("lists");
    assert!(!back.contains("chat question"), "{back}");
    assert!(back.contains("task one"), "{back}");
    let refusal = core
        .peer_digest(in_repository.id, chat.id, None, None)
        .expect_err("a chat is not read from a repository");
    assert!(matches!(refusal, Error::Peers(_)), "{refusal}");
}

#[tokio::test]
async fn a_session_closes_only_what_it_started_and_only_when_that_is_done() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let (parent, stranger) = (new_session(&core), new_session(&core));
    let child = core
        .peer_spawn(parent.id, "slow", None, None)
        .expect("spawns");
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
    assert_eq!(back.parent, Some(parent.id));
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
async fn an_agent_changes_how_often_usage_warnings_are_said_at_once() {
    let fx = fixture();
    let core = Arc::new(core_with_task(&fx).await);
    let session = new_session(&core);
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    listening(&socket).await;

    // Fewer warnings is not proposed: a limit reached is said whatever the setting is.
    let rare = serde_json::json!({ "kind": "appearance", "op": "save", "limit_warnings": "Rare" });
    let (ok, said) = configure(&socket, session.id, rare).await;
    assert!(ok, "{said}");
    assert!(said.contains("limit_warnings rare"), "{said}");
    let chosen = || core.settings().expect("reads").limit_warnings;
    assert_eq!(chosen(), LimitWarnings::Rare);

    let unknown =
        serde_json::json!({ "kind": "appearance", "op": "save", "limit_warnings": "hourly" });
    let (ok, said) = configure(&socket, session.id, unknown).await;
    assert!(!ok, "{said}");
    assert!(said.contains("`limit_warnings`"), "{said}");
    assert_eq!(chosen(), LimitWarnings::Rare);
}

#[tokio::test]
async fn an_agent_cannot_change_how_many_sessions_may_work_for_one() {
    let fx = fixture();
    let core = Arc::new(core_with_task(&fx).await);
    let session = new_session(&core);
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    listening(&socket).await;
    let limit = || core.settings().expect("reads").max_workers;

    // Given among the looks, it is not one of them: the rest is changed, this is not.
    let raised = serde_json::json!({ "kind": "appearance", "op": "save", "palette": "Forest",
        "max_workers": 20 });
    let (ok, said) = configure(&socket, session.id, raised).await;
    assert!(ok, "{said}");
    assert!(!said.contains("max_workers"), "{said}");
    assert_eq!(core.settings().expect("reads").palette, Palette::Forest);
    assert_eq!(limit(), MAX_SPAWNED);

    // Nor is there a kind of its own, and the refusal says it is out of reach.
    for kind in ["settings", "sessions", "max_workers"] {
        let asked = serde_json::json!({ "kind": kind, "op": "save", "max_workers": 20 });
        let (ok, said) = configure(&socket, session.id, asked).await;
        assert!(!ok, "{said}");
        assert!(
            said.contains("how many sessions may work for one, the roles' models"),
            "{said}"
        );
    }
    assert_eq!(limit(), MAX_SPAWNED);
}

#[tokio::test]
async fn an_agent_cannot_change_the_models_and_efforts_of_roles() {
    let fx = fixture();
    let core = Arc::new(core_with_task(&fx).await);
    let session = new_session(&core);
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    listening(&socket).await;
    let defaults = serde_json::json!({ "reviewer": { "model": "opus", "effort": "max" } });

    // Given among the looks, it is not one of them.
    let among = serde_json::json!({ "kind": "appearance", "op": "save", "palette": "Forest",
        "role_defaults": defaults });
    let (ok, said) = configure(&socket, session.id, among).await;
    assert!(ok, "{said}");
    assert!(!said.contains("role_defaults"), "{said}");
    assert_eq!(core.settings().expect("reads").palette, Palette::Forest);
    assert!(core.settings().expect("reads").role_defaults.is_empty());

    // Nor is there a kind of its own.
    for kind in ["roles", "role_defaults", "models"] {
        let asked = serde_json::json!({ "kind": kind, "op": "save", "role_defaults": defaults });
        let (ok, said) = configure(&socket, session.id, asked).await;
        assert!(!ok, "{said}");
        assert!(
            said.contains("the roles' models and efforts and the user's own stylesheet cannot"),
            "{said}"
        );
    }
    assert!(core.settings().expect("reads").role_defaults.is_empty());
}

#[tokio::test]
async fn an_agent_can_neither_write_nor_propose_the_users_stylesheet() {
    let fx = fixture();
    let core = Arc::new(core_with_task(&fx).await);
    let mut bus = core.subscribe();
    let session = new_session(&core);
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    listening(&socket).await;
    let hiding = ".deny { display: none; }";

    // Given among the looks, it is not one of them: the rest is changed, this is not.
    let among = serde_json::json!({ "kind": "appearance", "op": "save", "palette": "Forest",
        "user_stylesheet": true, "stylesheet": hiding, "css": hiding });
    let (ok, said) = configure(&socket, session.id, among).await;
    assert!(ok, "{said}");
    assert!(!said.contains("stylesheet"), "{said}");
    assert_eq!(core.settings().expect("reads").palette, Palette::Forest);

    // Nor is there a kind of its own, to save or to propose, and the refusal says so.
    let asked = ["stylesheet", "style", "css", "user_stylesheet"]
        .into_iter()
        .flat_map(|kind| ["save", "list"].map(|op| (kind, op)));
    for (kind, op) in asked {
        let asked = serde_json::json!({ "kind": kind, "op": op, "css": hiding,
            "text": hiding, "json": hiding, "on": true });
        let (ok, said) = configure(&socket, session.id, asked).await;
        assert!(!ok, "{said}");
        assert!(said.contains("stylesheet cannot be changed"), "{said}");
        // It says where an agent can write one, for the user to see and accept.
        let path = core.paths().stylesheet();
        assert!(said.contains(&path.display().to_string()), "{said}");
        assert!(said.contains("Apply"), "{said}");
    }
    // A kind it does not know says only that the stylesheet is out of reach.
    let (_, said) = configure(
        &socket,
        session.id,
        serde_json::json!({ "kind": "theme",
        "op": "save" }),
    )
    .await;
    assert!(said.contains("the user's own stylesheet cannot"), "{said}");
    // Nor does a layout carry it.
    let layout = serde_json::json!({ "kind": "layout", "op": "save",
        "json": { "base": "classic", "css": hiding } });
    let (ok, said) = configure(&socket, session.id, layout).await;
    assert!(!ok, "{said}");

    let sheet = core.stylesheet().expect("reads");
    assert_eq!(
        (sheet.on, sheet.file, sheet.accepted.as_str()),
        (false, None, "")
    );
    assert!(!core.paths().stylesheet().exists());
    // Nothing was put to the user either.
    while let Ok(event) = bus.try_recv() {
        let proposed = matches!(event.event, AgentEvent::ConfigProposed { .. });
        assert!(!proposed, "{event:?}");
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
        json(serde_json::json!({ "kind": "appearance", "op": "save", "palette": "Forest", "layout": "senior", "text_size": 110, "tool_calls": "Folded", "turn_end_lines": false, "math": "with_single_dollars" })),
    )
    .await;
    assert!(ok, "{said}");
    assert!(said.contains("tool_calls folded"), "{said}");
    assert!(said.contains("turn_end_lines false"), "{said}");
    assert!(said.contains("math with_single_dollars"), "{said}");
    let settings = core.settings().expect("reads");
    assert_eq!(
        (
            settings.palette,
            settings.layout,
            settings.text_scale,
            settings.tool_calls,
            settings.turn_end_lines
        ),
        (
            Palette::Forest,
            Layout::Senior,
            110,
            ToolCalls::Folded,
            false
        )
    );
    assert_eq!(settings.math, MathShown::WithSingleDollars);
    let layout = serde_json::json!({ "kind": "layout", "op": "save", "json": { "base": "senior", "icons": 140 } });
    let (ok, said) = configure(&socket, session.id, json(layout)).await;
    assert!(ok, "{said}");
    assert_eq!(core.settings().expect("reads").layout, Layout::Custom);
    assert_eq!(core.custom_layout().expect("reads").icons, Some(140));
    let terminal =
        serde_json::json!({ "kind": "terminal", "op": "save", "json": { "cursor": "bar" } });
    let (ok, said) = configure(&socket, session.id, json(terminal)).await;
    assert!(ok, "{said}");
    assert_eq!(
        core.terminal_config().expect("reads").cursor,
        Some(swarmed_core::Cursor::Bar)
    );
    let listed = serde_json::json!({ "kind": "terminal", "op": "list" });
    let (_, said) = configure(&socket, session.id, json(listed)).await;
    assert!(
        said.contains("\"cursor\":\"bar\"") && said.contains("scrollback"),
        "{said}"
    );
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

    // Hiding tool calls would hide what every agent does: the rest of the looks is changed
    // at once, and that part waits.
    let hidden = serde_json::json!({
        "kind": "appearance", "op": "save", "palette": "plum", "tool_calls": "hidden"
    });
    let (ok, said) = configure(&socket, session.id, hidden).await;
    assert!(ok, "{said}");
    assert!(said.contains("NOT in effect"), "{said}");
    let (hides, title, _) = proposed(&mut bus).await;
    assert_eq!(title, "Hide tool calls in every conversation?");
    let settings = core.settings().expect("reads");
    assert_eq!(
        (settings.palette, settings.tool_calls),
        (Palette::Plum, ToolCalls::Folded)
    );
    core.config_answer(&hides, true).expect("accepts");
    assert_eq!(
        core.settings().expect("reads").tool_calls,
        ToolCalls::Hidden
    );

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
async fn an_agent_sets_an_environments_ports_and_variables_one_by_one() {
    let fx = fixture();
    let core = Arc::new(core_with_task(&fx).await);
    let mut bus = core.subscribe();
    let session = new_session(&core);
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    listening(&socket).await;
    let before = Environment {
        ports: [("db".to_string(), 5432), ("old".to_string(), 9000)].into(),
        env: [
            ("API_TOKEN".to_string(), "s3cret-value".to_string()),
            ("STALE".to_string(), "x".to_string()),
        ]
        .into(),
        ..Environment::default()
    };
    core.environment_save("demo", "local", &before)
        .expect("test setup");

    let list = serde_json::json!({ "kind": "environment", "op": "list" });
    let (_, listed) = configure(&socket, session.id, list).await;
    assert!(listed.contains("ports: db=5432, old=9000"), "{listed}");
    assert!(listed.contains("{{port.<name>}}"), "{listed}");
    assert!(!listed.contains("s3cret-value"));

    let change = serde_json::json!({
        "kind": "environment", "op": "save", "name": "local",
        "start": "pnpm dev --port {{port.web}}", "open_url": "http://localhost:{{port.web}}",
        "ports": { "web": 3000, "old": 0 },
        "variables": { "DATABASE_URL": "postgres://localhost:{{port.db}}/app", "STALE": "" },
    });
    let (ok, said) = configure(&socket, session.id, change).await;
    assert!(ok, "{said}");
    let (request, _, detail) = proposed(&mut bus).await;
    assert!(detail.contains("ports: db=5432, web=3000"), "{detail}");
    assert!(
        detail.contains(
            "variables: DATABASE_URL=postgres://localhost:{{port.db}}/app, STALE removed"
        ),
        "{detail}"
    );
    // A variable the environment already had is no part of what the conversation shows.
    assert!(!detail.contains("API_TOKEN") && !detail.contains("s3cret-value"));
    core.config_answer(&request, true).expect("accepts");
    let kept = &core.environments("demo").expect("reads")["local"];
    assert_eq!(
        kept.ports,
        [("db".to_string(), 5432), ("web".to_string(), 3000)].into()
    );
    let names: Vec<&str> = kept.env.keys().map(String::as_str).collect();
    assert_eq!(names, ["API_TOKEN", "DATABASE_URL"]);

    for unusable in [
        serde_json::json!({ "ports": { "web": 70000 } }),
        serde_json::json!({ "ports": "3000" }),
        serde_json::json!({ "variables": { "A": 1 } }),
    ] {
        let mut change =
            serde_json::json!({ "kind": "environment", "op": "save", "name": "local" });
        change
            .as_object_mut()
            .expect("an object")
            .extend(unusable.as_object().expect("an object").clone());
        assert!(!configure(&socket, session.id, change).await.0);
    }
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
