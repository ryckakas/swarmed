//! The apps MCP servers attach to their tools: kept as the session's own, served under the
//! policy their server's declaration comes to, calling only their own server's tools, through
//! the agent, and handed on to nothing.

mod common;

use std::path::PathBuf;

use common::{core_with_project, fixture, turn};
use serde_json::json;
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{AppCsp, Attached, Core, Error, NewTaskOptions, Session, SessionOptions, hooks};

async fn session_in(core: &Core, project: &str, task: &str) -> Session {
    let options = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    core.task_new(project, task, options, &hooks::discard)
        .await
        .expect("makes the task");
    core.session_new(project, task, &SessionOptions::default())
        .expect("makes a session")
}

/// The file the first app among `events` is kept in.
fn kept(events: &[AgentEvent]) -> PathBuf {
    events
        .iter()
        .find_map(|event| match event {
            AgentEvent::App { document, html, .. } => {
                assert!(html.is_none(), "the core keeps what was read");
                document.clone()
            }
            _ => None,
        })
        .expect("an app")
}

#[tokio::test]
async fn an_app_is_kept_served_under_its_policy_and_calls_only_its_own_servers_tools() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let session = session_in(&core, "demo", "one").await;
    let mut bus = core.subscribe();
    core.session_message(session.id, "app", &Attached::default(), Auth::Subscription)
        .expect("sends");

    let document = kept(&turn(&mut bus, session.id).await);
    assert_eq!(
        document.parent(),
        Some(fx.paths.attachments(session.id).as_path())
    );
    let stored = core.session_events(session.id).expect("reads");
    assert_eq!(kept(&stored), document);
    assert!(
        !serde_json::to_string(&stored)
            .expect("serialises")
            .contains("<h1>Chart</h1>"),
        "the HTML is kept in its file, not in the conversation"
    );

    // Served under a policy of its own, which reaches what was declared and nothing else.
    let declared = AppCsp {
        connect: vec![
            "https://api.example.com".into(),
            "http://plain.example.com".into(),
        ],
        ..AppCsp::default()
    };
    let shown = core
        .app_put(session.id, "tool-app", &document, &declared)
        .expect("served");
    assert_eq!(shown.policy.reaches, ["https://api.example.com"]);
    assert_eq!(shown.policy.refused.len(), 1);
    let (html, policy) = core.app_document(&shown.id).expect("there");
    assert_eq!(html, "<!doctype html><h1>Chart</h1>");
    assert!(policy.contains("connect-src https://api.example.com;"));
    assert!(core.app_document("unknown").is_none());
    // Only an app kept in the session's own folder is served.
    let elsewhere = fx.paths.attachments(session.id + 1);
    assert!(matches!(
        core.app_put(session.id + 1, "tool-app", &document, &declared),
        Err(Error::AppNotFound(_))
    ));
    assert!(!elsewhere.exists());

    // A tool of its own server it may call goes through the agent, and comes back.
    let called = core
        .session_app_call(session.id, "tool-app", "refresh", json!({ "page": 2 }))
        .await
        .expect("called");
    assert_eq!(
        called,
        json!({ "content": [{ "type": "text", "text": "called refresh" }],
                "structuredContent": { "page": 2 } })
    );
    assert!(matches!(
        core.session_app_call(session.id, "tool-app", "delete_everything", json!({}))
            .await,
        Err(Error::AppToolRefused(tool)) if tool == "delete_everything"
    ));
    assert!(matches!(
        core.session_app_call(session.id, "no-such-call", "refresh", json!({}))
            .await,
        Err(Error::AppNotFound(_))
    ));

    // A fork carries what was said, never an app.
    let fork = core.session_fork(session.id, None, None).expect("forks");
    let forked = core.session_events(fork.id).expect("reads");
    assert!(
        !forked
            .iter()
            .any(|event| matches!(event, AgentEvent::App { .. }))
    );
    assert!(!fx.paths.attachments(fork.id).exists());

    // An agent that is not running calls nothing.
    core.session_close(session.id, false).expect("closes");
    assert!(
        core.session_app_call(session.id, "tool-app", "refresh", json!({}))
            .await
            .is_err()
    );
    assert!(document.exists());
    core.session_delete(session.id).expect("deletes");
    assert!(!document.exists());
}
