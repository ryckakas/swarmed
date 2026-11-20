//! The ACP backend against a scripted ACP agent (`fixtures/fake_acp_agent.py`).

mod common;

use std::path::Path;
use std::sync::Arc;

use common::{fixture, next, turn};
use serde_json::json;
use swarmed_core::agent::{AcpBackend, AgentEvent, Auth, Effort, FakeBackend, Supervision};
use swarmed_core::{
    AgentInfo, Core, Error, McpServerInput, NewTaskOptions, SessionEvent, SessionOptions,
    SessionStatus, hooks,
};
use tokio::sync::broadcast::Receiver;

const AGENT: &str = "scripted";

fn scripted_agent(env: &[(&str, &str)]) -> AcpBackend {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_agent.py");
    AcpBackend {
        program: "python3".into(),
        args: vec![script.to_string_lossy().into_owned()],
        env: env
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect(),
    }
}

/// A core with one task and the scripted ACP agent registered next to (fake) Claude Code.
async fn core_with(fx: &common::Fixture, backend: AcpBackend) -> Core {
    let mut core =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let info = AgentInfo {
        id: AGENT.into(),
        name: "Scripted".into(),
        models: vec![],
    };
    core.add_agent(info, Arc::new(backend));
    core.project_add(&fx.repo, None).await.expect("test setup");
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core
}

/// A started session on the scripted agent.
fn started(core: &Core, supervision: Supervision) -> i64 {
    let options = SessionOptions {
        agent: Some(AGENT.into()),
        supervision,
        ..Default::default()
    };
    let session = core.session_new("demo", "one", &options).expect("creates");
    assert_eq!(session.agent, AGENT);
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    session.id
}

/// The events with how long their turns took left out: that differs from run to run.
fn untimed(events: &[AgentEvent]) -> Vec<AgentEvent> {
    let untime = |event: &AgentEvent| match event.clone() {
        AgentEvent::TurnDone {
            is_error,
            result,
            cost_usd,
            ..
        } => AgentEvent::TurnDone {
            is_error,
            result,
            cost_usd,
            duration_ms: None,
        },
        other => other,
    };
    events.iter().map(untime).collect()
}

fn text(text: &str) -> AgentEvent {
    AgentEvent::Text {
        text: text.to_string(),
    }
}

async fn permission_request(bus: &mut Receiver<SessionEvent>) -> String {
    loop {
        if let AgentEvent::PermissionRequest {
            request_id,
            tool_name,
            input,
            ..
        } = next(bus).await.event
        {
            assert_eq!(
                (tool_name.as_str(), &input),
                ("Terminal", &json!({ "command": "ls" }))
            );
            return request_id;
        }
    }
}

#[tokio::test]
async fn a_turn_joins_the_streamed_pieces_into_one_reply() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);

    core.session_send(id, "hello").expect("sends");
    let events = turn(&mut bus, id).await;
    let init = AgentEvent::Init {
        agent_session_id: "acp-1".into(),
        model: "fake/default".into(),
        permission_mode: "ask".into(),
        api_key_source: "agent".into(),
        mcp_servers: vec![],
        skills: 0,
        slash_commands: vec![],
    };
    let done = AgentEvent::TurnDone {
        is_error: false,
        result: None,
        cost_usd: Some(0.5),
        duration_ms: None,
    };
    let said = AgentEvent::UserMessage {
        text: "hello".into(),
    };
    let context = AgentEvent::ContextUsage {
        used: 1,
        max: 2,
        categories: vec![],
    };
    assert_eq!(
        untimed(&events),
        [said, init, text("echo: hello"), context, done]
    );
    // The end of a turn says how long it took, whatever the agent.
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnDone {
            duration_ms: Some(_),
            ..
        })
    ));
    assert_eq!(
        core.session(id)
            .expect("exists")
            .agent_session_id
            .as_deref(),
        Some("acp-1")
    );
}

/// The pieces of the reply published until the turn ends, joined.
async fn streamed_pieces(bus: &mut Receiver<SessionEvent>) -> String {
    let mut pieces = String::new();
    loop {
        match next(bus).await.event {
            AgentEvent::TextDelta { text } => pieces.push_str(&text),
            AgentEvent::TurnDone { .. } => return pieces,
            _ => {}
        }
    }
}

#[tokio::test]
async fn a_stored_api_key_reaches_other_agents_and_never_claude_code() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    core.set_api_key("OPENROUTER_API_KEY", "sk-or-test")
        .expect("stores");

    let id = started(&core, Supervision::Ask);
    core.session_send(id, "key").expect("sends");
    assert!(turn(&mut bus, id).await.contains(&text("key: sk-or-test")));

    // What agents.toml sets for an agent wins over the stored key.
    let own = scripted_agent(&[("OPENROUTER_API_KEY", "from-agents-toml")]);
    let other = fixture();
    let core = core_with(&other, own).await;
    let mut bus = core.subscribe();
    core.set_api_key("OPENROUTER_API_KEY", "sk-or-test")
        .expect("stores");
    let id = started(&core, Supervision::Ask);
    core.session_send(id, "key").expect("sends");
    assert!(
        turn(&mut bus, id)
            .await
            .contains(&text("key: from-agents-toml"))
    );
}

#[tokio::test]
async fn pieces_of_a_reply_are_published_as_they_arrive_but_not_stored() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);

    core.session_send(id, "hello").expect("sends");
    let pieces = streamed_pieces(&mut bus).await;
    assert_eq!(pieces, "echo: hello");
    let stored = core.session_events(id).expect("exists");
    assert!(stored.contains(&text("echo: hello")));
    assert!(
        !stored
            .iter()
            .any(|event| matches!(event, AgentEvent::TextDelta { .. }))
    );
}

#[tokio::test]
async fn a_permission_request_is_answered_with_the_matching_option() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);

    core.session_send(id, "run: ls").expect("sends");
    let request = permission_request(&mut bus).await;
    assert_eq!(
        core.session(id).expect("exists").status,
        SessionStatus::Waiting
    );
    core.session_allow(id, &request).expect("sends");
    let rest = turn(&mut bus, id).await;
    let ok = AgentEvent::ToolResult {
        tool_use_id: "call-1".into(),
        content: "ok".into(),
        is_error: false,
    };
    assert_eq!(rest.get(1), Some(&ok));
    assert!(rest.contains(&text("ran it")));

    core.session_send(id, "run: ls").expect("sends");
    let request = permission_request(&mut bus).await;
    core.session_deny(id, &request, "no").expect("sends");
    let rest = turn(&mut bus, id).await;
    assert!(matches!(
        rest.get(1),
        Some(AgentEvent::ToolResult { is_error: true, .. })
    ));
    assert!(rest.contains(&text("not allowed")));
}

#[tokio::test]
async fn auto_supervision_allows_without_asking() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Auto);

    core.session_send(id, "run: ls").expect("sends");
    let events = turn(&mut bus, id).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::PermissionRequest { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolResult {
            is_error: false,
            ..
        }
    )));
    let tool_use = events
        .iter()
        .find(|event| matches!(event, AgentEvent::ToolUse { .. }));
    let expected = AgentEvent::ToolUse {
        id: "call-1".into(),
        name: "Terminal".into(),
        input: json!({ "command": "ls" }),
    };
    assert_eq!(tool_use, Some(&expected));
}

#[tokio::test]
async fn an_interrupt_cancels_the_turn_and_an_agent_error_ends_it() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);

    core.session_send(id, "slow").expect("sends");
    while !matches!(next(&mut bus).await.event, AgentEvent::Init { .. }) {}
    core.session_interrupt(id).expect("sends");
    let rest = turn(&mut bus, id).await;
    let stopped = AgentEvent::TurnDone {
        is_error: true,
        result: None,
        cost_usd: None,
        duration_ms: None,
    };
    assert_eq!(untimed(&rest), [text("working"), stopped]);

    core.session_send(id, "fail").expect("sends");
    let rest = turn(&mut bus, id).await;
    assert_eq!(
        rest.get(1),
        Some(&text("The agent reported an error: auth_required"))
    );
    assert!(matches!(
        rest.last(),
        Some(AgentEvent::TurnDone { is_error: true, .. })
    ));
}

/// Runs a turn, stops the agent, starts it again and runs another; returns the second turn.
async fn turn_after_restart(core: &Core, bus: &mut Receiver<SessionEvent>) -> Vec<AgentEvent> {
    let id = started(core, Supervision::Ask);
    core.session_send(id, "first").expect("sends");
    turn(bus, id).await;
    core.session_stop(id);
    while !matches!(next(bus).await.event, AgentEvent::Exited { .. }) {}

    core.session_start(id, Auth::Subscription)
        .expect("restarts");
    core.session_send(id, "second").expect("sends");
    turn(bus, id).await
}

#[tokio::test]
async fn a_restarted_session_continues_the_agents_conversation() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let events = turn_after_restart(&core, &mut bus).await;
    assert!(matches!(
        events.get(1),
        Some(AgentEvent::Init { agent_session_id, .. }) if agent_session_id == "acp-1"
    ));
    assert_eq!(events.get(2), Some(&text("echo: second")));
}

#[tokio::test]
async fn history_an_agent_replays_while_loading_is_not_recorded_twice() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[("FAKE_ACP_LOAD_ONLY", "1")])).await;
    let mut bus = core.subscribe();
    let events = turn_after_restart(&core, &mut bus).await;
    assert_eq!(events.get(2), Some(&text("echo: second")));
    assert!(!events.contains(&text("replayed history")));
}

#[tokio::test]
async fn what_an_agent_says_of_the_model_it_reopened_with_is_dropped_once_the_sessions_is_back() {
    let fx = fixture();
    let agent = scripted_agent(&[("FAKE_ACP_RESUME_NOTICE", "1")]);
    let core = core_with(&fx, agent).await;
    let mut bus = core.subscribe();
    let options = SessionOptions {
        agent: Some(AGENT.into()),
        model: Some("some/model".into()),
        ..Default::default()
    };
    let id = core
        .session_new("demo", "one", &options)
        .expect("creates")
        .id;
    core.session_start(id, Auth::Subscription).expect("starts");
    core.session_send(id, "first").expect("sends");
    turn(&mut bus, id).await;
    core.session_stop(id);
    while !matches!(next(&mut bus).await.event, AgentEvent::Exited { .. }) {}

    core.session_start(id, Auth::Subscription)
        .expect("restarts");
    core.session_send(id, "second").expect("sends");
    let events = turn(&mut bus, id).await;
    assert!(events.contains(&text("echo: second")), "{events:?}");
    let warned =
        |event: &AgentEvent| matches!(event, AgentEvent::Text { text } if text.contains("Warning"));
    assert!(!events.iter().any(warned), "{events:?}");
}

#[tokio::test]
async fn a_session_whose_model_was_left_to_the_agent_still_hears_what_it_says_on_reopening() {
    let fx = fixture();
    let agent = scripted_agent(&[("FAKE_ACP_RESUME_NOTICE", "1")]);
    let core = core_with(&fx, agent).await;
    let mut bus = core.subscribe();
    let events = turn_after_restart(&core, &mut bus).await;
    let warned =
        |event: &AgentEvent| matches!(event, AgentEvent::Text { text } if text.contains("Warning"));
    assert!(events.iter().any(warned), "{events:?}");
}

#[tokio::test]
async fn the_effort_an_agent_opens_with_is_recorded_and_is_its_default_from_then_on() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[("FAKE_ACP_EFFORT", "1")])).await;
    let mut bus = core.subscribe();
    assert_eq!(core.effort_default(AGENT, None).expect("reads"), None);
    let id = started(&core, Supervision::Ask);
    core.session_send(id, "hi").expect("sends");
    let events = turn(&mut bus, id).await;
    let applied = AgentEvent::Applied {
        model: "fake/default".into(),
        effort: Some("medium".into()),
    };
    assert!(events.contains(&applied), "{events:?}");
    let known = core.effort_default(AGENT, None).expect("reads");
    assert_eq!(known.as_deref(), Some("medium"));
    // Another model, or another agent, has said nothing yet.
    assert_eq!(
        core.effort_default(AGENT, Some("other")).expect("reads"),
        None
    );
    assert_eq!(core.effort_default("claude", None).expect("reads"), None);

    // A session that chose its effort runs with the agent's nearest, and is no default.
    let options = SessionOptions {
        agent: Some(AGENT.into()),
        effort: Some(Effort::Max),
        ..Default::default()
    };
    let chosen = core
        .session_new("demo", "one", &options)
        .expect("creates")
        .id;
    core.session_start(chosen, Auth::Subscription)
        .expect("starts");
    core.session_send(chosen, "hi").expect("sends");
    let events = turn(&mut bus, chosen).await;
    let high = AgentEvent::Applied {
        model: "fake/default".into(),
        effort: Some("high".into()),
    };
    assert!(events.contains(&high), "{events:?}");
    let known = core.effort_default(AGENT, None).expect("reads");
    assert_eq!(known.as_deref(), Some("medium"));
}

#[tokio::test]
async fn a_commit_message_is_written_by_the_agent_the_session_runs_on() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let options = SessionOptions {
        agent: Some(AGENT.into()),
        ..Default::default()
    };
    let id = core
        .session_new("demo", "one", &options)
        .expect("creates")
        .id;
    let worktree = core.task("demo", "one").expect("exists").1.worktree_path;
    std::fs::write(worktree.join("notes.txt"), "one\n").expect("writes");

    // No namer is set: the session's own agent is asked, and it echoes what it was shown.
    let message = core
        .task_commit_message("demo", "one", Some(id))
        .await
        .expect("writes");
    assert!(
        message.starts_with("echo: You write git commit messages."),
        "{message}"
    );
    assert!(message.contains("new file notes.txt:\none"), "{message}");
    // The question was a conversation of its own: the session has heard nothing of it.
    assert_eq!(core.session_events(id).expect("reads"), []);
    assert_eq!(core.running_sessions(), Vec::<i64>::new());
    // Without a session there is nobody to ask here.
    assert!(core.task_commit_message("demo", "one", None).await.is_err());
}

#[tokio::test]
async fn what_an_agent_does_by_itself_after_its_turn_ends_like_a_turn() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);
    core.session_send(id, "wake").expect("sends");
    let asked = turn(&mut bus, id).await;
    assert!(asked.contains(&text("I will tell you when the timer goes off.")));

    // Nobody asked again: the agent's timer went off, it says so, and is then done.
    let own = turn(&mut bus, id).await;
    assert!(own.contains(&text("The timer went off.")), "{own:?}");
    let done = own.last().expect("ends");
    assert!(
        matches!(
            done,
            AgentEvent::TurnDone {
                is_error: false,
                ..
            }
        ),
        "{own:?}"
    );
    assert_eq!(
        core.session(id).expect("exists").status,
        SessionStatus::Idle
    );
}

#[tokio::test]
async fn an_agent_at_work_by_itself_can_be_stopped() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);
    core.session_send(id, "linger").expect("sends");
    turn(&mut bus, id).await;
    // It starts a tool after its turn and says no more: the session counts as working.
    while !matches!(next(&mut bus).await.event, AgentEvent::ToolUse { .. }) {}
    assert_eq!(
        core.session(id).expect("exists").status,
        SessionStatus::Working
    );

    core.session_interrupt(id).expect("sends");
    let stopped = turn(&mut bus, id).await;
    let done = stopped.last().expect("ends");
    assert!(
        matches!(done, AgentEvent::TurnDone { is_error: true, .. }),
        "{stopped:?}"
    );
    assert_eq!(
        core.session(id).expect("exists").status,
        SessionStatus::Idle
    );
}

#[tokio::test]
async fn a_session_can_change_its_agent_until_it_has_begun() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    // Started the way the sidebar starts one: on Claude Code, with a model of its own.
    let options = SessionOptions {
        model: Some("opus".into()),
        ..Default::default()
    };
    let id = core
        .session_new("demo", "one", &options)
        .expect("creates")
        .id;
    let moved = core.session_set_agent(id, AGENT).expect("changes");
    // The model was the other agent's: this one starts on its default.
    assert_eq!((moved.agent.as_str(), moved.model), (AGENT, None));
    assert!(matches!(
        core.session_set_agent(id, "nope"),
        Err(Error::AgentNotFound(_))
    ));

    core.session_start(id, Auth::Subscription).expect("starts");
    core.session_send(id, "hi").expect("sends");
    let events = turn(&mut bus, id).await;
    assert!(events.contains(&text("echo: hi")), "{events:?}");
    // Now the scripted agent holds the conversation.
    assert!(matches!(
        core.session_set_agent(id, "claude"),
        Err(Error::SessionBegun(_))
    ));
    core.session_stop(id);
    while !matches!(next(&mut bus).await.event, AgentEvent::Exited { .. }) {}
    assert!(matches!(
        core.session_set_agent(id, "claude"),
        Err(Error::SessionBegun(_))
    ));
}

#[tokio::test]
async fn a_session_can_name_its_model_and_only_a_configured_agent() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let options = SessionOptions {
        agent: Some(AGENT.into()),
        model: Some("openrouter/some/model".into()),
        ..Default::default()
    };
    let session = core.session_new("demo", "one", &options).expect("creates");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    core.session_send(session.id, "hi").expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(matches!(
        events.get(1),
        Some(AgentEvent::Init { model, .. }) if model == "openrouter/some/model"
    ));

    let unknown = SessionOptions {
        agent: Some("nope".into()),
        ..Default::default()
    };
    let refused = core.session_new("demo", "one", &unknown);
    assert!(matches!(refused, Err(Error::AgentNotFound(id)) if id == "nope"));
    assert_eq!(
        core.agents()
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        ["claude", AGENT]
    );
}

#[tokio::test]
async fn an_acp_agent_is_given_swarmeds_guidance_once_with_its_first_message() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);

    core.session_send(id, "guidance").expect("sends");
    let first = turn(&mut bus, id).await;
    assert!(first.contains(&text("guidance: yes")), "{first:?}");
    // The user's own message is recorded as they wrote it.
    assert_eq!(
        first.first(),
        Some(&AgentEvent::UserMessage {
            text: "guidance".into()
        })
    );
    core.session_send(id, "guidance").expect("sends");
    let second = turn(&mut bus, id).await;
    assert!(second.contains(&text("guidance: none")), "{second:?}");
}

#[tokio::test]
async fn an_agent_that_cannot_be_started_is_an_error_not_a_hang() {
    let fx = fixture();
    let missing = AcpBackend {
        program: "/nonexistent/agent".into(),
        args: vec![],
        env: vec![],
    };
    let core = core_with(&fx, missing).await;
    let options = SessionOptions {
        agent: Some(AGENT.into()),
        ..Default::default()
    };
    let session = core.session_new("demo", "one", &options).expect("creates");
    // And the error names the program and what to do, not just the system's "No such file".
    let failed = core.session_start(session.id, Auth::Subscription);
    assert!(
        matches!(&failed, Err(Error::AgentMissing(program)) if program == "/nonexistent/agent"),
        "{failed:?}"
    );
    let said = failed.expect_err("fails").to_string();
    assert!(said.contains("Settings, under Agents"), "{said}");
    assert_eq!(core.running_sessions(), [0_i64; 0]);
}

/// Writes a Claude Code config with one server of each kind, as the fixture's `claude.json`.
fn write_claude_config(fx: &common::Fixture) {
    let config = json!({ "mcpServers": {
        "context7": { "type": "http", "url": "https://example.test/mcp", "headers": { "API_KEY": "secret" } },
        "files": { "command": "npx", "args": ["-y", "files"], "env": { "ROOT": "/tmp", "MODE": "ro" } },
    }});
    std::fs::write(&fx.paths.claude_config, config.to_string()).expect("writes");
}

#[tokio::test]
async fn the_agent_is_given_the_mcp_servers_configured_for_claude_code() {
    let fx = fixture();
    write_claude_config(&fx);
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);

    core.session_send(id, "mcp").expect("sends");
    let events = turn(&mut bus, id).await;
    assert_eq!(
        events.get(2),
        Some(&text("servers: context7(http, 1 set), files(stdio, 2 set)"))
    );
    let Some(AgentEvent::Init { mcp_servers, .. }) = events.get(1) else {
        panic!("no init: {events:?}")
    };
    let reported: Vec<_> = mcp_servers
        .iter()
        .map(|s| (s.name.as_str(), s.status.as_str()))
        .collect();
    assert_eq!(reported, [("context7", "passed"), ("files", "passed")]);
}

#[tokio::test]
async fn servers_set_up_in_swarmed_are_given_with_their_secrets_and_never_show_them() {
    let fx = fixture();
    write_claude_config(&fx);
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();

    let server = |enabled: bool, token: Option<&str>| McpServerInput {
        name: "tracker".into(),
        command: "npx -y \"@acme/tracker mcp\"".into(),
        url: String::new(),
        secrets: vec![
            ("TRACKER_TOKEN".into(), token.map(String::from)),
            ("TRACKER_ORG".into(), None),
        ],
        enabled,
    };
    let listed = core.mcp_save(&server(true, Some("s3cret"))).expect("saves");
    // Set up here first, then Claude Code's own, with secrets by name only.
    let names: Vec<(&str, &str)> = listed
        .iter()
        .map(|view| (view.name.as_str(), view.origin.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("tracker", "swarmed"),
            ("context7", "claude"),
            ("files", "claude")
        ]
    );
    assert_eq!(listed[0].target, "npx -y \"@acme/tracker mcp\"");
    assert_eq!(listed[0].secrets, ["TRACKER_TOKEN", "TRACKER_ORG"]);
    assert_eq!(listed[1].secrets, ["API_KEY"]);
    let written = std::fs::read_to_string(fx.paths.mcp_config()).expect("reads");
    assert!(
        written.contains("TRACKER_TOKEN") && !written.contains("s3cret"),
        "{written}"
    );
    assert!(!format!("{listed:?}").contains("s3cret"));

    // The agent gets it with the one secret that has a value.
    let id = started(&core, Supervision::Ask);
    core.session_send(id, "mcp").expect("sends");
    let events = turn(&mut bus, id).await;
    assert_eq!(
        events.get(2),
        Some(&text(
            "servers: context7(http, 1 set), files(stdio, 2 set), tracker(stdio, 1 set)"
        ))
    );

    // Saved again without a value the secret stays; switched off, the server is not given.
    core.mcp_save(&server(false, None)).expect("saves");
    core.session_stop(id);
    let again = started(&core, Supervision::Ask);
    core.session_send(again, "mcp").expect("sends");
    let events = turn(&mut bus, again).await;
    assert!(events.contains(&text("servers: context7(http, 1 set), files(stdio, 2 set)")));

    let left = core.mcp_remove("tracker").expect("removes");
    assert!(left.iter().all(|view| view.origin == "claude"));
    assert!(matches!(core.mcp_remove("tracker"), Err(Error::Mcp(_))));
}

#[tokio::test]
async fn an_agent_that_does_not_take_http_servers_gets_only_the_others() {
    let fx = fixture();
    write_claude_config(&fx);
    let core = core_with(&fx, scripted_agent(&[("FAKE_ACP_NO_HTTP", "1")])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);

    core.session_send(id, "mcp").expect("sends");
    assert_eq!(
        turn(&mut bus, id).await.get(2),
        Some(&text("servers: files(stdio, 2 set)"))
    );
}
