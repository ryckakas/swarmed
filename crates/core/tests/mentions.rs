//! Sessions mentioned in a message, against the scripted agent.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{core_with_project, fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    Attached, CHATS, Core, Error, FirstSession, NewTaskOptions, Reachable, Session, SessionOptions,
    hooks,
};

/// A core whose agents are given Swarmed's tools, as the app's and the command line's are, with
/// a task `one` in the project.
async fn core_with_tools(fx: &common::Fixture) -> Core {
    let mut core = core_with_project(fx).await;
    core.set_tool_server(PathBuf::from("/usr/local/bin/swarmed"));
    task(&core, "one").await;
    core
}

async fn task(core: &Core, name: &str) {
    core.task_new("demo", name, NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
}

fn session(core: &Core, task: &str, title: &str) -> Session {
    let options = SessionOptions {
        title: Some(title.to_string()),
        ..SessionOptions::default()
    };
    core.session_new("demo", task, &options).expect("creates")
}

fn mentioning(sessions: &[i64]) -> Attached {
    Attached {
        files: vec![],
        sessions: sessions.to_vec(),
    }
}

fn messages(core: &Core, id: i64) -> Vec<String> {
    let events = core.session_events(id).expect("reads");
    let said = events.into_iter().filter_map(|event| match event {
        AgentEvent::UserMessage { text, .. } => Some(text),
        _ => None,
    });
    said.collect()
}

fn ids(listed: &[Reachable]) -> Vec<i64> {
    listed.iter().map(|session| session.id).collect()
}

fn refused(result: &swarmed_core::Result<impl std::fmt::Debug>, id: i64) -> bool {
    matches!(result, Err(Error::CannotMention(refused)) if *refused == id)
}

/// A second repository beside the first, called `other`, with a task `two` and a session in it.
async fn other_repository(fx: &common::Fixture, core: &Core) -> Session {
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
    core.session_new("other", "two", &SessionOptions::default())
        .expect("creates")
}

#[tokio::test]
async fn the_sessions_a_session_can_read_are_listed_its_own_tasks_first() {
    let fx = fixture();
    let core = core_with_tools(&fx).await;
    task(&core, "two").await;
    let here = session(&core, "one", "Here");
    let beside = session(&core, "one", "Beside");
    let apart = session(&core, "two", "Apart");
    let elsewhere = other_repository(&fx, &core).await;
    // The one apart was at work last, which puts it first among the others, never ahead of
    // its own task's.
    let mut bus = core.subscribe();
    core.session_message(apart.id, "hello", &Attached::default(), Auth::Subscription)
        .expect("sends");
    turn(&mut bus, apart.id).await;

    let listed = core.session_reachable(here.id).expect("lists");
    assert_eq!(ids(&listed), [beside.id, apart.id]);
    let first = &listed[0];
    assert!(first.same_task && !first.chat, "{first:?}");
    assert_eq!(
        (
            first.title.as_str(),
            first.task.as_str(),
            first.project.as_str()
        ),
        ("Beside", "one", "demo")
    );
    assert!(!listed[1].same_task);
    assert!(!ids(&listed).contains(&elsewhere.id));
    // A new task's first session would read the same, latest active first.
    let new_task = core.project_reachable("demo").expect("lists");
    assert_eq!(ids(&new_task)[0], apart.id);
    assert_eq!(new_task.len(), 3);

    // A closed one is not listed.
    core.session_close(beside.id).expect("closes");
    assert_eq!(
        ids(&core.session_reachable(here.id).expect("lists")),
        [apart.id]
    );
}

#[tokio::test]
async fn a_mention_is_sent_as_a_reference_listed_under_the_message() {
    let fx = fixture();
    let core = core_with_tools(&fx).await;
    let here = session(&core, "one", "Here");
    let found = session(&core, "one", "Fix the login form");
    let readme = fx.repo.join("README.md");
    let mut bus = core.subscribe();

    let attached = Attached {
        files: vec![readme.clone()],
        sessions: vec![found.id, found.id],
    };
    let typed = format!("do what [@{} Fix the login form] did", found.id);
    core.session_message(here.id, &typed, &attached, Auth::Subscription)
        .expect("sends");
    turn(&mut bus, here.id).await;
    let expected = format!(
        "{typed}\n\nSessions mentioned, to read with the read_session tool:\n\
         - session {} (\"Fix the login form\")\n\nAttached files:\n- {}",
        found.id,
        readme.display()
    );
    assert_eq!(messages(&core, here.id), [expected]);
    // The session mentioned is only read, not written to.
    assert_eq!(messages(&core, found.id), Vec::<String>::new());

    // A mention alone is a message too, and it waits in the queue as typed.
    let queue = core
        .session_enqueue_message(here.id, "", &mentioning(&[found.id]))
        .expect("queues");
    let waiting = queue.first().map(|message| message.text.clone());
    assert_eq!(
        waiting.as_deref(),
        Some(&*format!(
            "Sessions mentioned, to read with the read_session tool:\n\
             - session {} (\"Fix the login form\")",
            found.id
        ))
    );
}

#[tokio::test]
async fn a_session_out_of_reach_is_refused_before_anything_is_sent() {
    let fx = fixture();
    let core = core_with_tools(&fx).await;
    let here = session(&core, "one", "Here");
    let closed = session(&core, "one", "Closed");
    core.session_close(closed.id).expect("closes");
    let elsewhere = other_repository(&fx, &core).await;

    for id in [elsewhere.id, closed.id, here.id, 404] {
        let sent = core.session_message(here.id, "look", &mentioning(&[id]), Auth::Subscription);
        assert!(refused(&sent, id), "session {id}");
        let now = core.session_message_now(here.id, "look", &mentioning(&[id]), Auth::Subscription);
        assert!(refused(&now, id), "session {id}");
        let queued = core.session_enqueue_message(here.id, "later", &mentioning(&[id]));
        assert!(refused(&queued, id), "session {id}");
    }
    // Nothing was started, sent or queued.
    assert_eq!(core.running_sessions(), Vec::<i64>::new());
    assert_eq!(messages(&core, here.id), Vec::<String>::new());
    assert_eq!(core.session_queue(here.id).expect("reads"), vec![]);
}

#[tokio::test]
async fn a_chat_mentions_any_session_and_a_repository_never_a_chat() {
    let fx = fixture();
    let core = core_with_tools(&fx).await;
    let in_repository = session(&core, "one", "In the repository");
    let free = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    // Before there is any chat, a new one would read every open session.
    let before = core.project_reachable(CHATS).expect("lists");
    assert_eq!(ids(&before), [in_repository.id]);
    core.task_new(CHATS, "question", free, &hooks::discard)
        .await
        .expect("makes a chat");
    let chat = core
        .session_new(CHATS, "question", &SessionOptions::default())
        .expect("creates");

    let from_chat = core.session_reachable(chat.id).expect("lists");
    assert_eq!(ids(&from_chat), [in_repository.id]);
    let mut bus = core.subscribe();
    core.session_message(
        chat.id,
        "what did it find?",
        &mentioning(&[in_repository.id]),
        Auth::Subscription,
    )
    .expect("sends");
    turn(&mut bus, chat.id).await;

    // A chat may hold what it read of another repository: a repository's session cannot
    // point its agent at one.
    let back = core.session_reachable(in_repository.id).expect("lists");
    assert!(!ids(&back).contains(&chat.id), "{back:?}");
    let sent = core.session_message(
        in_repository.id,
        "read the chat",
        &mentioning(&[chat.id]),
        Auth::Subscription,
    );
    assert!(refused(&sent, chat.id));
}

#[tokio::test]
async fn an_agent_without_swarmeds_tools_is_given_what_was_said_instead() {
    let fx = fixture();
    // No front end named a tool server: the agent has nothing to read a session with.
    let core = core_with_project(&fx).await;
    task(&core, "one").await;
    let here = session(&core, "one", "Here");
    let found = session(&core, "one", "Greeting");
    let silent = session(&core, "one", "Silent");
    let mut bus = core.subscribe();
    core.session_message(found.id, "hello", &Attached::default(), Auth::Subscription)
        .expect("sends");
    turn(&mut bus, found.id).await;

    let attached = mentioning(&[found.id, silent.id]);
    core.session_message(here.id, "see these", &attached, Auth::Subscription)
        .expect("sends");
    turn(&mut bus, here.id).await;
    let (found, silent) = (found.id, silent.id);
    let expected = format!(
        "see these\n\nSessions mentioned, with what was last said in each:\n\
         - session {found} (\"Greeting\"):\n<session_{found}>\nUser: hello\n\n\
         Agent: echo: hello\n</session_{found}>\n\
         - session {silent} (\"Silent\"):\n<session_{silent}>\n\
         Nothing has been said there yet.\n</session_{silent}>"
    );
    assert_eq!(messages(&core, here.id), [expected]);
}

#[tokio::test]
async fn a_new_task_mentions_the_sessions_of_its_repository() {
    let fx = fixture();
    let core = Arc::new(core_with_tools(&fx).await);
    let found = session(&core, "one", "Found it");
    let elsewhere = other_repository(&fx, &core).await;
    let first = |sessions: Vec<i64>| FirstSession {
        prompt: "carry on from there".to_string(),
        sessions,
        ..FirstSession::default()
    };

    // One of another repository is refused before anything is made.
    let start = core.task_start(
        "demo",
        "refused",
        NewTaskOptions::default(),
        first(vec![elsewhere.id]),
        Auth::Subscription,
        &hooks::discard,
    );
    assert!(refused(&start.await, elsewhere.id));
    assert!(core.task("demo", "refused").is_err());

    let mut bus = core.subscribe();
    let (started, going) = core
        .task_start(
            "demo",
            "carry",
            NewTaskOptions::default(),
            first(vec![found.id]),
            Auth::Subscription,
            &hooks::discard,
        )
        .await
        .expect("starts");
    going.finished().await.expect("set up");
    turn(&mut bus, started.session.id).await;
    let expected = format!(
        "carry on from there\n\nSessions mentioned, to read with the read_session tool:\n\
         - session {} (\"Found it\")",
        found.id
    );
    assert_eq!(messages(&core, started.session.id), [expected]);
}

#[tokio::test]
async fn a_waiting_message_keeps_its_mentions_through_an_edit() {
    let fx = fixture();
    let core = core_with_tools(&fx).await;
    let here = session(&core, "one", "Here");
    let found = session(&core, "one", "Found it");
    let elsewhere = other_repository(&fx, &core).await;
    let queue = core
        .session_enqueue_message(here.id, "first", &Attached::default())
        .expect("queues");
    let message = queue.first().map(|message| message.id).expect("waits");

    let edited = core
        .session_queue_edit_message(here.id, message, "then this", &mentioning(&[found.id]))
        .expect("edits");
    let expected = format!(
        "then this\n\nSessions mentioned, to read with the read_session tool:\n\
         - session {} (\"Found it\")",
        found.id
    );
    assert_eq!(
        edited.first().map(|m| m.text.clone()),
        Some(expected.clone())
    );

    // One out of reach is refused, and the message stays as it was.
    let refusal =
        core.session_queue_edit_message(here.id, message, "no", &mentioning(&[elsewhere.id]));
    assert!(refused(&refusal, elsewhere.id));
    let queue = core.session_queue(here.id).expect("reads");
    assert_eq!(queue.first().map(|m| m.text.clone()), Some(expected));

    // Nothing at all takes it out.
    let emptied = core
        .session_queue_edit_message(here.id, message, " ", &Attached::default())
        .expect("edits");
    assert_eq!(emptied, vec![]);
}
