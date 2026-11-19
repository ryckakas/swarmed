//! The ACP backend against a scripted ACP agent (`fixtures/fake_acp_agent.py`).

mod common;

use std::path::Path;
use std::sync::Arc;

use common::{fixture, next, turn};
use serde_json::json;
use swarmed_core::agent::{AcpBackend, AgentEvent, Auth, FakeBackend, Supervision};
use swarmed_core::{
    AgentInfo, Core, Error, NewTaskOptions, SessionEvent, SessionOptions, SessionStatus, hooks,
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
    };
    let said = AgentEvent::UserMessage {
        text: "hello".into(),
    };
    let context = AgentEvent::ContextUsage {
        used: 1,
        max: 2,
        categories: vec![],
    };
    assert_eq!(events, [said, init, text("echo: hello"), context, done]);
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
    };
    assert_eq!(rest, [text("working"), stopped]);

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
