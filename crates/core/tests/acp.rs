//! The ACP backend against a scripted ACP agent (`fixtures/fake_acp_agent.py`).

mod common;

use std::path::Path;
use std::sync::Arc;

use common::{fixture, next, turn};
use serde_json::json;
use swarmed_core::agent::{AcpBackend, AgentEvent, Auth, Effort, FakeBackend, Supervision};
use swarmed_core::{
    AgentInfo, Core, Error, McpServerInput, NewTaskOptions, RemoveOptions, SessionEvent,
    SessionOptions, SessionSettings, SessionStatus, hooks,
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
        default_model: None,
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
        default_model: None,
        hears_mid_turn: false,
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
        remote: false,
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

/// The commit message an agent with these modes of its own writes, if it is asked at all.
async fn commit_message_from(modes: &str) -> Result<String, Error> {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[("FAKE_ACP_MODES", modes)])).await;
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
    core.task_commit_message("demo", "one", Some(id)).await
}

#[tokio::test]
async fn an_agent_asked_for_words_is_held_to_planning_or_not_asked_at_all() {
    // The changes it is shown are nobody's word to act on: it is put into the mode of its
    // own that plans, or failing that only reads.
    let planned = commit_message_from("agent,plan").await.expect("writes");
    assert!(planned.contains("(mode: plan)"), "{planned}");
    let read = commit_message_from("workspace-write,read-only").await;
    let read = read.expect("writes");
    assert!(read.contains("(mode: read-only)"), "{read}");
    // With modes none of which holds it back it would act without asking: it is not asked.
    let refused = commit_message_from("build,yolo").await;
    assert!(refused.is_err(), "{refused:?}");
}

/// Skips ahead to the next permission request, as it was published, and the tool it names.
async fn asked_permission(bus: &mut Receiver<SessionEvent>) -> (SessionEvent, String) {
    loop {
        let published = next(bus).await;
        if let AgentEvent::PermissionRequest { tool_name, .. } = &published.event {
            let tool_name = tool_name.clone();
            return (published, tool_name);
        }
    }
}

#[tokio::test]
async fn a_request_titled_with_its_command_is_named_by_its_kind() {
    let fx = fixture();
    let titled = scripted_agent(&[("FAKE_ACP_TITLE_COMMAND", "1")]);
    let core = core_with(&fx, titled).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);
    core.session_send(id, "run: curl -H token:hunter2 example.com")
        .expect("sends");
    let (asked, tool_name) = asked_permission(&mut bus).await;
    assert_eq!(tool_name, "execute");
    // Which is what keeps the command, an argument, out of the notification.
    let notice = core.notice_for(&asked).expect("reads").expect("tells");
    assert!(notice.body.ends_with("Wants to use execute"), "{notice:?}");
}

/// How much the agent that stays after its input closed has written to show it is alive.
fn beats(file: &Path) -> u64 {
    std::fs::metadata(file).map_or(0, |written| written.len())
}

/// Whether that agent has stopped writing, which it only does by being gone.
async fn has_gone_quiet(file: &Path) -> bool {
    let before = beats(file);
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    beats(file) == before
}

/// A core whose scripted agent carries on after its input has closed, a session on it that
/// has had one turn, and the file the agent writes to for as long as it lives.
async fn core_with_an_agent_that_stays(fx: &common::Fixture) -> (Core, i64, std::path::PathBuf) {
    let beat = fx.repo.with_file_name("beat");
    let staying = scripted_agent(&[("FAKE_ACP_STAY", &beat.to_string_lossy())]);
    let core = core_with(fx, staying).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);
    core.session_send(id, "hello").expect("sends");
    turn(&mut bus, id).await;
    (core, id, beat)
}

#[tokio::test]
async fn an_agent_that_carries_on_after_it_was_stopped_is_killed() {
    let fx = fixture();
    let (core, id, beat) = core_with_an_agent_that_stays(&fx).await;
    let mut bus = core.subscribe();

    core.session_stop(id);
    // It counts as stopped at once, and is given a moment to go by itself, which it does
    // not take.
    assert_eq!(core.running_sessions(), Vec::<i64>::new());
    while beats(&beat) == 0 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let ended = next(&mut bus).await;
    assert_eq!(
        ended.event,
        AgentEvent::Exited {
            code: None,
            stderr: String::new()
        }
    );
    assert_eq!(ended.status, Some(SessionStatus::Idle));
    assert!(has_gone_quiet(&beat).await);
    // The conversation is as it was, and goes on with a new agent.
    core.session_start(id, Auth::Subscription).expect("starts");
    core.session_send(id, "again").expect("sends");
    assert!(turn(&mut bus, id).await.contains(&text("echo: again")));
}

#[tokio::test]
async fn a_task_is_not_removed_under_an_agent_that_is_still_at_work() {
    let fx = fixture();
    let (core, _, beat) = core_with_an_agent_that_stays(&fx).await;
    let force = RemoveOptions {
        force: true,
        delete_branch: false,
    };
    core.task_remove("demo", "one", force, &hooks::discard)
        .await
        .expect("removes");
    // The agent did carry on, and was gone before the worktree went.
    assert_ne!(beats(&beat), 0);
    assert!(has_gone_quiet(&beat).await);
}

#[tokio::test]
async fn deleting_a_session_kills_its_agent_at_once() {
    let fx = fixture();
    let (core, id, beat) = core_with_an_agent_that_stays(&fx).await;
    let mut bus = core.subscribe();
    core.session_delete(id).expect("deletes");
    assert_eq!(next(&mut bus).await.event, AgentEvent::Deleted);
    // No moment to finish in: nothing is left for it to finish.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(has_gone_quiet(&beat).await);
    assert!(bus.try_recv().is_err(), "nothing follows the deletion");
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
async fn a_conversation_is_cloned_onto_another_agent_and_carried_on_there() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    // A conversation on Claude Code, with a model of its own.
    let options = SessionOptions {
        model: Some("opus".into()),
        ..Default::default()
    };
    let source = core.session_new("demo", "one", &options).expect("creates");
    core.session_start(source.id, Auth::Subscription)
        .expect("starts");
    core.session_send(source.id, "echo: the password is swordfish")
        .expect("sends");
    turn(&mut bus, source.id).await;
    let said = core.session_events(source.id).expect("reads");

    let clone = core
        .session_fork(source.id, None, Some(AGENT))
        .expect("clones");
    // On the other agent, in the same task, with that agent's own model.
    assert_eq!(
        (clone.agent.as_str(), clone.task_id, clone.model.as_deref()),
        (AGENT, source.task_id, None)
    );
    // The first session is as it was, and still on its own agent.
    assert_eq!(core.session_events(source.id).expect("reads"), said);
    assert_eq!(core.session(source.id).expect("exists").agent, "claude");

    // The other agent is given what was said with the first message sent to it.
    core.session_start(clone.id, Auth::Subscription)
        .expect("starts");
    core.session_send(clone.id, "what was it?").expect("sends");
    let events = turn(&mut bus, clone.id).await;
    let heard = events.iter().find_map(|event| match event {
        AgentEvent::Text { text } => Some(text.as_str()),
        _ => None,
    });
    let heard = heard.expect("the scripted agent repeats what it is sent");
    assert!(heard.contains("what was it?"), "{heard}");
    assert!(heard.contains("the password is swordfish"), "{heard}");

    // The same agent is an ordinary fork, and an agent nobody set up is refused.
    let same = core
        .session_fork(source.id, None, Some("claude"))
        .expect("forks");
    assert_eq!(
        (same.agent.as_str(), same.model.as_deref()),
        ("claude", Some("opus"))
    );
    assert!(matches!(
        core.session_fork(source.id, None, Some("nope")),
        Err(Error::AgentNotFound(_))
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

/// Has the session's agent say which model it is on.
async fn model_heard(core: &Core, bus: &mut Receiver<SessionEvent>, id: i64) -> Vec<AgentEvent> {
    core.session_send(id, "model").expect("sends");
    turn(bus, id).await
}

#[tokio::test]
async fn a_session_that_leaves_the_model_open_runs_on_the_default_swarmed_has_for_the_agent() {
    const FIRST: &str = "openrouter/first/listed";
    let fx = fixture();
    let agent = AcpBackend {
        default_model: Some(FIRST.into()),
        ..scripted_agent(&[])
    };
    let core = core_with(&fx, agent).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);
    let events = model_heard(&core, &mut bus, id).await;
    // Named where the session starts, as what it runs on, and not the agent's own default.
    assert!(
        matches!(events.get(1), Some(AgentEvent::Init { model, .. }) if model == FIRST),
        "{events:?}"
    );
    assert!(
        events.contains(&text(&format!("model: {FIRST}"))),
        "{events:?}"
    );

    // Another model chosen, then the choice left open again: back on Swarmed's default.
    for (chosen, on) in [
        (Some("openrouter/other/model"), "openrouter/other/model"),
        (None, FIRST),
    ] {
        let settings = SessionSettings {
            model: chosen.map(String::from),
            supervision: Supervision::Ask,
            ..SessionSettings::default()
        };
        core.session_configure(id, &settings).expect("configures");
        let events = model_heard(&core, &mut bus, id).await;
        assert!(
            events.contains(&text(&format!("model: {on}"))),
            "{events:?}"
        );
    }

    // Started again, the agent has forgotten what it was put on and is put back on it.
    core.session_stop(id);
    while !matches!(next(&mut bus).await.event, AgentEvent::Exited { .. }) {}
    core.session_start(id, Auth::Subscription)
        .expect("restarts");
    let events = model_heard(&core, &mut bus, id).await;
    assert!(
        events.contains(&text(&format!("model: {FIRST}"))),
        "{events:?}"
    );
}

#[tokio::test]
async fn a_running_session_goes_back_to_the_default_as_it_stands_now() {
    const NOW: &str = "openrouter/now/first";
    let fx = fixture();
    let agent = AcpBackend {
        default_model: Some("openrouter/removed/since".into()),
        ..scripted_agent(&[])
    };
    let mut core = core_with(&fx, agent).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);
    model_heard(&core, &mut bus, id).await;

    // The list changes while the agent runs: another model is first in it now.
    let info = AgentInfo {
        id: AGENT.into(),
        name: "Scripted".into(),
        models: vec![NOW.into()],
        default_model: Some(NOW.into()),
        hears_mid_turn: false,
    };
    let agent = AcpBackend {
        default_model: Some(NOW.into()),
        ..scripted_agent(&[])
    };
    core.add_agent(info, Arc::new(agent));

    for chosen in [Some("openrouter/other/model"), None] {
        let settings = SessionSettings {
            model: chosen.map(String::from),
            supervision: Supervision::Ask,
            ..SessionSettings::default()
        };
        core.session_configure(id, &settings).expect("configures");
    }
    let events = model_heard(&core, &mut bus, id).await;
    assert!(
        events.contains(&text(&format!("model: {NOW}"))),
        "{events:?}"
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
            text: "guidance".into(),
            remote: false,
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
        default_model: None,
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

/// Writes a server into Codex's configuration, one into OpenCode's, and a plugin with one
/// into Claude Code's home.
fn write_other_servers(fx: &common::Fixture) {
    let codex = "[mcp_servers.tracker]\ncommand = \"tracker\"\n\
                 [mcp_servers.files]\ncommand = \"codex-files\"\n";
    std::fs::write(&fx.paths.codex_config, codex).expect("writes");
    let dir = &fx.paths.opencode_config_dir;
    std::fs::create_dir_all(dir).expect("creates");
    let opencode =
        json!({ "mcp": { "notes": { "type": "local", "command": ["notes", "serve"] } } });
    std::fs::write(dir.join("opencode.json"), opencode.to_string()).expect("writes");

    let home = fx.paths.claude_config.parent().expect("has a folder");
    let plugin = home.join(".claude/plugins/cache/design");
    std::fs::create_dir_all(&plugin).expect("creates");
    let installed =
        json!({ "plugins": { "design@m": [{ "scope": "user", "installPath": plugin }] } });
    let record = home.join(".claude/plugins/installed_plugins.json");
    std::fs::write(record, installed.to_string()).expect("writes");
    let servers =
        json!({ "canvas": { "command": "canvas" }, "files": { "command": "plugin-files" } });
    std::fs::write(plugin.join(".mcp.json"), servers.to_string()).expect("writes");
}

#[tokio::test]
async fn an_agent_is_given_the_servers_of_every_other_agent() {
    let fx = fixture();
    write_claude_config(&fx);
    write_other_servers(&fx);
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);

    core.session_send(id, "mcp").expect("sends");
    let events = turn(&mut bus, id).await;
    let Some(AgentEvent::Init { mcp_servers, .. }) = events.get(1) else {
        panic!("no init: {events:?}")
    };
    let given: Vec<&str> = mcp_servers.iter().map(|s| s.name.as_str()).collect();
    // Claude Code's own and its plugin's, Codex's and OpenCode's. `files` is configured for
    // Claude Code, which goes before the plugin's and before Codex's of that name.
    assert_eq!(given, ["canvas", "context7", "files", "notes", "tracker"]);
    assert_eq!(
        events.get(2),
        Some(&text(
            "servers: canvas(stdio, 0 set), context7(http, 1 set), files(stdio, 2 set), \
             notes(stdio, 0 set), tracker(stdio, 0 set)"
        ))
    );
}

#[tokio::test]
async fn an_agent_with_servers_of_its_own_is_given_only_the_ones_it_lacks() {
    let fx = fixture();
    write_claude_config(&fx);
    write_other_servers(&fx);
    let mut core = core_with(&fx, scripted_agent(&[])).await;
    let codex = AgentInfo {
        id: "codex".into(),
        name: "Codex".into(),
        models: vec![],
        default_model: None,
        hears_mid_turn: false,
    };
    core.add_agent(codex, Arc::new(scripted_agent(&[])));
    let mut bus = core.subscribe();
    let options = SessionOptions {
        agent: Some("codex".into()),
        ..Default::default()
    };
    let session = core.session_new("demo", "one", &options).expect("creates");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");

    core.session_send(session.id, "mcp").expect("sends");
    let events = turn(&mut bus, session.id).await;
    let Some(AgentEvent::Init { mcp_servers, .. }) = events.get(1) else {
        panic!("no init: {events:?}")
    };
    let given: Vec<&str> = mcp_servers.iter().map(|s| s.name.as_str()).collect();
    // Not `tracker` and `files`, which it has itself.
    assert_eq!(given, ["canvas", "context7", "notes"]);
}

#[tokio::test]
async fn the_start_of_a_session_counts_the_skills_it_is_offered() {
    let fx = fixture();
    // An agent that speaks ACP says nothing of skills, so the count is Swarmed's own.
    let home = fx.paths.claude_config.parent().expect("has a folder");
    let folder = home.join(".claude/skills/review");
    std::fs::create_dir_all(&folder).expect("creates");
    let text = "---\nname: review\ndescription: Review a change.\n---\nLook closely.\n";
    std::fs::write(folder.join("SKILL.md"), text).expect("writes");
    let core = core_with(&fx, scripted_agent(&[])).await;
    let mut bus = core.subscribe();
    let id = started(&core, Supervision::Ask);

    core.session_send(id, "hello").expect("sends");
    let events = turn(&mut bus, id).await;
    assert!(
        matches!(events.get(1), Some(AgentEvent::Init { skills: 1, .. })),
        "{events:?}"
    );
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

#[tokio::test]
async fn an_acp_agent_is_not_told_a_message_in_the_middle_of_its_turn() {
    let fx = fixture();
    let core = core_with(&fx, scripted_agent(&[])).await;
    let id = started(&core, Supervision::Ask);
    let queue = core.session_enqueue(id, "a hint").expect("queues");
    let told = core.session_tell_now(id, queue[0].id);
    assert!(
        matches!(told, Err(Error::HearsNoMessageMidTurn(_))),
        "{told:?}"
    );
    // It still waits, to be sent when the turn ends or sent now.
    assert_eq!(core.session_queue(id).expect("reads"), queue);
}
