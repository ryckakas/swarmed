//! Session lifecycle against the scripted fake agent.

mod common;

use common::{AskedNamer, core_with_project, fixture, git, next, turn};
use std::sync::Arc;

use swarmed_core::agent::{
    AgentEvent, Auth, BackgroundState, Effort, FakeBackend, Namer, Naming, PLAN_TOOL, Supervision,
};
use swarmed_core::{
    AccountUsage, AgentInfo, Attached, BackgroundCount, Core, Error, Mark, MarkColour,
    NewTaskOptions, Session, SessionEvent, SessionOptions, SessionSettings, SessionStatus,
    Settings, TerminalOutput, UsageWindow, hooks,
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
            text: "hello there".into(),
            remote: false,
            at_reset: false,
            from_session: None,
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
async fn a_suggested_next_message_follows_the_turn_and_leaves_the_session_idle() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "suggest: run the tests")
        .expect("sends");
    turn(&mut bus, session.id).await;
    let after = next(&mut bus).await;
    let suggested = AgentEvent::Suggestion {
        text: "run the tests".into(),
    };
    assert_eq!(after.event, suggested);
    // It changes nothing about the session, and is kept with the conversation.
    assert_eq!(after.status, None);
    let stored = core.session_events(session.id).expect("exists");
    assert_eq!(stored.last(), Some(&suggested));
    assert_eq!(
        core.session(session.id).expect("exists").status,
        SessionStatus::Idle
    );
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
async fn a_finished_turn_is_unread_until_the_user_looks_or_writes() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    assert!(!session.unread);

    core.session_send(session.id, "hello").expect("sends");
    assert!(!core.session(session.id).expect("exists").unread);
    turn(&mut bus, session.id).await;
    let done = core.session(session.id).expect("exists");
    assert_eq!((done.status, done.unread), (SessionStatus::Idle, true));
    // The lists say so too.
    assert!(core.overview().expect("reads")[0].tasks[0].sessions[0].unread);

    core.session_mark_read(session.id).expect("marks");
    assert!(!core.session(session.id).expect("exists").unread);

    // Writing to a session is having seen it.
    core.session_send(session.id, "again").expect("sends");
    turn(&mut bus, session.id).await;
    assert!(core.session(session.id).expect("exists").unread);
    core.session_send(session.id, "slow").expect("sends");
    next(&mut bus).await;
    assert!(!core.session(session.id).expect("exists").unread);
    assert!(matches!(
        core.session_mark_read(404),
        Err(Error::SessionNotFound(404))
    ));
}

#[tokio::test]
async fn a_view_handed_to_the_tool_is_shown_when_the_call_returns() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    // What an earlier run left behind is not this conversation's to show.
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    let spool = fx.paths.view_spool(session.id);
    std::fs::create_dir_all(&spool).expect("dir");
    std::fs::write(spool.join("0-stale.html"), "<p>stale</p>").expect("writes");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");

    // The tool server leaves the view while the agent's call is running; however the agent
    // made that call, the view follows its result.
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    std::fs::create_dir_all(&spool).expect("dir");
    std::fs::write(spool.join("1-view.html"), "<p>chart</p>").expect("writes");
    core.session_allow(session.id, &request_id).expect("sends");
    let events = turn(&mut bus, session.id).await;
    let views: Vec<&AgentEvent> = events
        .iter()
        .filter(|event| matches!(event, AgentEvent::View { .. }))
        .collect();
    let chart = AgentEvent::View {
        html: "<p>chart</p>".into(),
    };
    assert_eq!(views, [&chart]);
    // It is part of the conversation, and it is handed over once.
    assert!(
        core.session_events(session.id)
            .expect("reads")
            .contains(&chart)
    );
    assert!(!spool.join("1-view.html").exists());
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
    // A second answer to the same request, from a double click, changes nothing.
    core.session_allow(session.id, &request_id).expect("sends");
    let answered = next(&mut bus).await;
    let allowed = AgentEvent::PermissionAnswered {
        request_id,
        allowed: true,
    };
    assert_eq!(
        (answered.event, answered.status),
        (allowed, Some(SessionStatus::Working))
    );
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
        Some(AgentEvent::PermissionAnswered { allowed: false, .. })
    ));
    assert!(matches!(
        rest.get(1),
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

/// Names every conversation the same, after a moment, as a model would.
#[derive(Debug)]
struct FixedNamer(&'static str);

impl Namer for FixedNamer {
    fn name(&self, _prompt: String) -> Naming {
        let title = self.0.to_string();
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Some(title)
        })
    }

    fn commit_message(&self, changes: String) -> Naming {
        // What it was shown, so that a test can see it.
        Box::pin(async move { Some(format!("```\nfeat: written\n\n{changes}\n```")) })
    }

    fn pull_message(&self, branch: String) -> Naming {
        // What it was shown, so that a test can see it.
        Box::pin(async move { Some(format!("# feat: add notes\n\n**Adds notes.**\n\n{branch}")) })
    }
}

#[tokio::test]
async fn usage_asked_for_comes_from_the_sessions_agent_and_stays_out_of_the_conversation() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    let mut bus = core.subscribe();
    // The session's agent is started for it, and asked.
    core.session_usage_refresh(session.id, Auth::Subscription)
        .expect("asks");
    let answered = next(&mut bus).await;
    assert!(
        matches!(answered.event, AgentEvent::UsageLimits { .. }),
        "{answered:?}"
    );
    assert_eq!(answered.status, None);
    assert_eq!(core.running_sessions(), vec![session.id]);
    // Nothing was said, and the session is as idle as it was.
    let events = core.session_events(session.id).expect("reads");
    assert_eq!(events, vec![answered.event]);
    assert_eq!(
        core.session(session.id).expect("exists").status,
        SessionStatus::Idle
    );

    // Another agent reports no usage, and is not started for it.
    let other = AgentInfo {
        id: "other".into(),
        name: "Other".into(),
        models: vec![],
        default_model: None,
        hears_mid_turn: false,
        sends_to_background: false,
    };
    core.add_agent(other, Arc::new(FakeBackend::default()));
    let options = SessionOptions {
        agent: Some("other".into()),
        ..SessionOptions::default()
    };
    let elsewhere = core.session_new("demo", "one", &options).expect("creates");
    let refused = core.session_usage_refresh(elsewhere.id, Auth::Subscription);
    assert!(
        matches!(&refused, Err(Error::UsageNotReported(agent)) if agent == "other"),
        "{refused:?}"
    );
    assert_eq!(core.running_sessions(), vec![session.id]);
}

/// Now, in milliseconds since 1970, as Claude Code keeps time in its config file.
fn millis_now() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970");
    i64::try_from(since.as_millis()).expect("fits")
}

#[tokio::test]
async fn usage_asked_while_claude_code_answers_from_its_copy_waits_until_it_fetches_again() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    // Claude Code fetched its copy of the figures almost a minute ago, and would answer with it
    // for three seconds more: time for the agent to start before the core reads its clock,
    // even on a slow runner.
    let fetched = millis_now() - 58_000;
    let config = serde_json::json!({
        "cachedUsageUtilization": { "fetchedAtMs": fetched, "utilization": {} },
    });
    std::fs::write(&fx.paths.claude_config, config.to_string()).expect("writes");
    let mut bus = core.subscribe();

    let ask = core
        .session_usage_refresh(session.id, Auth::Subscription)
        .expect("asks");
    assert_eq!(ask.copy_fetched_at, Some(fetched));
    let until = ask
        .waits_until
        .expect("waits for the copy to be old enough");
    assert_eq!(until, fetched + 61_000);
    // The agent is started at once; the question goes once the copy is a minute old, some three
    // seconds on, so the answer is waited for longer than `next` waits.
    assert_eq!(core.running_sessions(), vec![session.id]);
    let answered = tokio::time::timeout(std::time::Duration::from_secs(15), bus.recv())
        .await
        .expect("answered once the copy was old enough")
        .expect("the event stream closed");
    assert!(
        matches!(answered.event, AgentEvent::UsageLimits { .. }),
        "{answered:?}"
    );
    assert!(
        millis_now() >= until,
        "asked before the copy was old enough"
    );

    // A copy a minute old or more is not answered from: the question goes at once.
    let config = serde_json::json!({ "cachedUsageUtilization": { "fetchedAtMs": fetched } });
    std::fs::write(&fx.paths.claude_config, config.to_string()).expect("writes");
    let ask = core
        .session_usage_refresh(session.id, Auth::Subscription)
        .expect("asks");
    assert_eq!(ask.waits_until, None);
}

/// Skips ahead to the next change of the account's usage, which belongs to no session.
async fn account_usage(bus: &mut Receiver<SessionEvent>) -> AccountUsage {
    loop {
        let heard = next(bus).await;
        if let AgentEvent::AccountUsage { usage } = heard.event {
            assert_eq!(heard.session_id, 0, "published apart");
            return usage;
        }
    }
}

/// Skips ahead to session `id`'s next report of usage after a turn.
async fn usage_reported(bus: &mut Receiver<SessionEvent>, id: i64) {
    loop {
        let heard = next(bus).await;
        if heard.session_id == id && matches!(heard.event, AgentEvent::RateLimit { .. }) {
            return;
        }
    }
}

/// Each window of the account's usage: its kind and how full it is, in percent.
fn shares(usage: &AccountUsage) -> Vec<(String, String)> {
    let share = |window: &UsageWindow| (window.kind.clone(), format!("{:.0}", window.used * 100.0));
    usage.windows.iter().map(share).collect()
}

fn share(kind: &str, percent: &str) -> (String, String) {
    (kind.to_string(), percent.to_string())
}

#[tokio::test]
async fn every_session_shows_the_newest_usage_any_heard_and_a_restart_reads_it_back() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    assert_eq!(
        core.account_usage().expect("reads"),
        AccountUsage::default()
    );

    let first = started_session(&core);
    core.session_send(first.id, "limit 50").expect("sends");
    let reported = account_usage(&mut bus).await;
    let half = [share("five_hour", "50"), share("seven_day", "50")];
    assert_eq!(shares(&reported), half);
    assert_eq!(core.account_usage().expect("reads"), reported);

    // Another agent's report is another account's, if it is one at all.
    let other = AgentInfo {
        id: "other".into(),
        name: "Other".into(),
        models: vec![],
        default_model: None,
        hears_mid_turn: false,
        sends_to_background: false,
    };
    core.add_agent(other, Arc::new(FakeBackend::default()));
    let options = SessionOptions {
        agent: Some("other".into()),
        ..SessionOptions::default()
    };
    let elsewhere = core.session_new("demo", "one", &options).expect("creates");
    core.session_start(elsewhere.id, Auth::Subscription)
        .expect("starts");
    core.session_send(elsewhere.id, "limit 90").expect("sends");
    usage_reported(&mut bus, elsewhere.id).await;
    assert_eq!(core.account_usage().expect("reads"), reported);

    // A session that never ran asks, and its answer is every session's: the 5-hour window it
    // gives is newer, and the weekly one it does not give stays as reported.
    let second = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    core.session_usage_refresh(second.id, Auth::Subscription)
        .expect("asks");
    let asked = account_usage(&mut bus).await;
    assert_eq!(
        shares(&asked),
        [share("five_hour", "36"), share("seven_day", "50")]
    );
    assert!(asked.windows[0].asked && !asked.windows[1].asked);
    let recorded = core.session_events(first.id).expect("reads");
    assert!(
        !recorded
            .iter()
            .any(|event| matches!(event, AgentEvent::AccountUsage { .. })),
        "never recorded"
    );

    // A Swarmed started again reads the same from what the sessions recorded.
    let again =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    assert_eq!(
        shares(&again.account_usage().expect("reads")),
        shares(&asked)
    );
}

/// Skips ahead to the next change of title.
async fn renamed(bus: &mut Receiver<SessionEvent>) -> String {
    loop {
        if let AgentEvent::Renamed { title } = next(bus).await.event {
            return title;
        }
    }
}

#[tokio::test]
async fn a_session_is_named_after_its_first_message_and_can_be_renamed() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    core.set_namer(Arc::new(FixedNamer("\"Greeting the agent\"")));
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "hello there").expect("sends");
    assert_eq!(
        core.session(session.id).expect("exists").title,
        "hello there"
    );
    assert_eq!(renamed(&mut bus).await, "Greeting the agent");
    assert_eq!(
        core.session(session.id).expect("exists").title,
        "Greeting the agent"
    );

    let by_hand = core
        .session_rename(session.id, "  My own name\nsecond line ")
        .expect("renames");
    assert_eq!(by_hand.title, "My own name");
    assert_eq!(renamed(&mut bus).await, "My own name");
    // Only the first message names a session.
    core.session_send(session.id, "another").expect("sends");
    turn(&mut bus, session.id).await;
    assert_eq!(
        core.session(session.id).expect("exists").title,
        "My own name"
    );
    assert!(matches!(
        core.session_rename(session.id, "  "),
        Err(Error::EmptyTitle)
    ));
}

#[tokio::test]
async fn a_mark_stays_on_a_session_until_it_is_taken_off_or_the_user_writes() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    assert_eq!(session.mark, None);

    let passed = Mark {
        colour: MarkColour::Green,
        label: "  e2e passed\nand more ".to_string(),
    };
    let marked = core.session_mark(session.id, Some(passed)).expect("marks");
    let kept = Mark {
        colour: MarkColour::Green,
        label: "e2e passed".to_string(),
    };
    assert_eq!(marked.mark, Some(kept.clone()));
    let told = next(&mut bus).await;
    assert_eq!(
        told.event,
        AgentEvent::Marked {
            colour: Some(MarkColour::Green),
            label: "e2e passed".to_string()
        }
    );
    // A mark says nothing about what the agent is doing.
    assert_eq!(told.status, None);
    assert_eq!(core.session(session.id).expect("exists").mark, Some(kept));

    let too_long = Mark {
        colour: MarkColour::Red,
        label: "x".repeat(41),
    };
    assert!(matches!(
        core.session_mark(session.id, Some(too_long)),
        Err(Error::Mark(_))
    ));

    // Writing to the session starts something new: the mark was about what came before.
    core.session_send(session.id, "next thing").expect("sends");
    turn(&mut bus, session.id).await;
    assert_eq!(core.session(session.id).expect("exists").mark, None);

    let failed = Mark {
        colour: MarkColour::Red,
        label: String::new(),
    };
    core.session_mark(session.id, Some(failed)).expect("marks");
    let cleared = core.session_mark(session.id, None).expect("takes it off");
    assert_eq!(cleared.mark, None);
    assert!(core.session_mark(9999, None).is_err());
}

#[tokio::test]
async fn a_highlight_stays_until_the_user_takes_it_off() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = started_session(&core);
    assert_eq!(session.highlight, None);
    let mut bus = core.subscribe();
    let told_to_read_the_tree = |bus: &mut Receiver<SessionEvent>| {
        let heard = bus.try_recv().expect("told");
        assert_eq!(heard.session_id, 0);
        assert_eq!(heard.event, AgentEvent::TreeChanged);
    };

    let lit = core
        .session_highlight(session.id, Some(MarkColour::Blue))
        .expect("highlights");
    assert_eq!(lit.highlight, Some(MarkColour::Blue));
    told_to_read_the_tree(&mut bus);
    assert!(matches!(
        core.session_highlight_named(session.id, Some("teal")),
        Err(Error::Highlight(_))
    ));
    assert_eq!(kept_now(&core, session.id), Some(MarkColour::Blue));

    // Writing to the session clears a mark, not a highlight.
    core.session_send(session.id, "next thing").expect("sends");
    turn(&mut bus, session.id).await;
    let kept = |core: &Core| core.session(session.id).expect("exists").highlight;
    assert_eq!(kept(&core), Some(MarkColour::Blue));

    // Closed and reopened, it is still there, and its row keeps its place.
    let other = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    core.session_close(session.id, false).expect("closes");
    core.session_reopen(session.id).expect("reopens");
    assert_eq!(kept(&core), Some(MarkColour::Blue));
    let order: Vec<i64> = core
        .sessions("demo", "one")
        .expect("lists")
        .iter()
        .map(|one| one.id)
        .collect();
    assert_eq!(order, [session.id, other.id]);

    // A fork starts without one.
    let fork = core.session_fork(session.id, None, None).expect("forks");
    assert_eq!(fork.highlight, None);

    // A task is highlighted the same way, and taken off with none.
    let task = core
        .task_highlight_named("demo", "one", Some("Purple"))
        .expect("highlights");
    assert_eq!(task.highlight, Some(MarkColour::Purple));
    assert!(matches!(
        core.task_highlight("demo", "nowhere", None),
        Err(Error::TaskNotFound { .. })
    ));
    assert!(core.session_highlight(9999, None).is_err());

    // A Swarmed started again on the same data finds both.
    drop(core);
    let again = core_again(&fx);
    assert_eq!(kept(&again), Some(MarkColour::Blue));
    let overview = again.overview().expect("reads");
    let one = &overview[0].tasks[0];
    assert_eq!(one.task.highlight, Some(MarkColour::Purple));
    assert_eq!(one.sessions[0].highlight, Some(MarkColour::Blue));
    // No word at all takes one off, as `none` does.
    let off = again
        .session_highlight_named(session.id, None)
        .expect("takes it off");
    assert_eq!(off.highlight, None);
    let off = again
        .task_highlight_named("demo", "one", Some("none"))
        .expect("takes it off");
    assert_eq!(off.highlight, None);
}

/// What session `id` is highlighted in now.
fn kept_now(core: &Core, id: i64) -> Option<MarkColour> {
    core.session(id).expect("exists").highlight
}

/// A Swarmed started again on the data `fx` holds, after the one before quit.
fn core_again(fx: &common::Fixture) -> Core {
    Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens")
}

#[tokio::test]
async fn a_title_chosen_before_the_suggestion_arrives_is_kept_and_naming_can_be_off() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    core.set_namer(Arc::new(FixedNamer("Suggested")));
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "hello").expect("sends");
    core.session_rename(session.id, "Mine").expect("renames");
    turn(&mut bus, session.id).await;
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    assert_eq!(core.session(session.id).expect("exists").title, "Mine");

    let off = Settings {
        auto_title: false,
        ..Settings::default()
    };
    core.set_settings(&off).expect("saves");
    let quiet = started_session(&core);
    core.session_send(quiet.id, "hello again").expect("sends");
    turn(&mut bus, quiet.id).await;
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    assert_eq!(core.session(quiet.id).expect("exists").title, "hello again");
}

#[tokio::test]
async fn a_status_line_script_is_told_about_the_session_it_is_for() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;
    assert_eq!(core.status_line(session.id).await.expect("no script"), None);

    // Runs in the worktree and reads the model the agent reported from its input.
    let script = r#"printf '%s on ' "$(basename "$PWD")"; sed -E 's/.*"model":"([^"]*)".*/\1/'"#;
    let settings = Settings {
        status_command: script.to_string(),
        ..Settings::default()
    };
    core.set_settings(&settings).expect("saves");
    let shown = core.status_line(session.id).await.expect("runs");
    assert_eq!(shown.as_deref(), Some("one on fake"));
    assert!(matches!(
        core.status_line(404).await,
        Err(Error::SessionNotFound(404))
    ));
}

/// Waits until the session's terminal has printed `wanted`.
async fn printed(output: &mut Receiver<TerminalOutput>, wanted: &str) {
    let mut seen = String::new();
    let wait = async {
        while !seen.contains(wanted) {
            seen.push_str(&output.recv().await.expect("the terminal is open").data);
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(20), wait)
        .await
        .expect("the terminal never printed what was typed");
}

/// Waits until what runs in a terminal has ended.
async fn ended(output: &mut Receiver<TerminalOutput>) {
    let wait = async { while !output.recv().await.expect("published").ended {} };
    tokio::time::timeout(std::time::Duration::from_secs(20), wait)
        .await
        .expect("it never said it ended");
}

/// Waits until the terminal has printed nothing for half a second. A shell writes its prompt
/// after a command's output, on a slow machine well after it, and a test that moves on at the
/// output alone sees that prompt arrive as something new later.
async fn settled(output: &mut Receiver<TerminalOutput>) {
    let quiet = std::time::Duration::from_millis(500);
    let wait = async { while let Ok(Ok(_)) = tokio::time::timeout(quiet, output.recv()).await {} };
    tokio::time::timeout(std::time::Duration::from_secs(20), wait)
        .await
        .expect("the terminal never went quiet");
}

#[tokio::test]
async fn what_happens_in_the_terminal_travels_with_the_next_message() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let mut output = core.subscribe_terminals();
    let session = started_session(&core);

    let fresh = core.terminal_open(session.id, 80, 24).expect("opens");
    assert_eq!((fresh.replay.as_str(), fresh.end), ("", 0));
    // The sum shows the command ran: its text alone would also match what the terminal echoes.
    core.terminal_write(session.id, "echo term-$((40 + 2))\n")
        .expect("types");
    printed(&mut output, "term-42").await;
    settled(&mut output).await;
    let reopened = core.terminal_open(session.id, 100, 30).expect("reopens");
    assert!(reopened.replay.contains("term-42"));
    // Nothing was cut from so short a replay, so it ends where its own text does.
    assert_eq!(reopened.end, reopened.replay.len() as u64);

    core.session_send(session.id, "what happened?")
        .expect("sends");
    let events = turn(&mut bus, session.id).await;
    let shared = events.iter().find_map(|event| match event {
        AgentEvent::TerminalShared { text } => Some(text.as_str()),
        _ => None,
    });
    assert!(
        shared.is_some_and(|text| text.contains("term-42")),
        "{events:?}"
    );
    let reply = events.iter().find_map(|event| match event {
        AgentEvent::Text { text } => Some(text.as_str()),
        _ => None,
    });
    let reply = reply.expect("the agent replied");
    assert!(
        reply.starts_with("echo: what happened?\n\n<terminal>"),
        "{reply}"
    );
    assert!(
        reply.contains("term-42") && reply.ends_with("</terminal>"),
        "{reply}"
    );

    // Nothing new in the terminal: the next message goes out as typed.
    core.session_send(session.id, "and now?").expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(events.contains(&text("echo: and now?")), "{events:?}");

    core.terminal_close(session.id);
    assert!(matches!(
        core.terminal_write(session.id, "ls\n"),
        Err(Error::TerminalNotOpen(id)) if id == session.id
    ));
}

#[tokio::test]
async fn claude_code_opened_on_a_screen_runs_apart_from_the_shell_and_is_not_shared() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let mut output = core.subscribe_claude_terminals();
    let mut shell = core.subscribe_terminals();
    let session = started_session(&core);
    core.terminal_open(session.id, 80, 24).expect("opens");

    // The fake agent's screen is `echo`: it prints the command it was opened on, and ends.
    let fresh = core
        .claude_terminal_open(session.id, "vim", (80, 24))
        .expect("opens");
    assert_eq!((fresh.replay.as_str(), fresh.end), ("", 0));
    printed(&mut output, "/vim").await;
    ended(&mut output).await;
    // Ended, it is gone: opened again, it starts anew.
    core.claude_terminal_open(session.id, "vim", (80, 24))
        .expect("opens again");
    printed(&mut output, "/vim").await;
    ended(&mut output).await;
    assert!(matches!(
        core.claude_terminal_write(session.id, "q"),
        Err(Error::TerminalNotOpen(id)) if id == session.id
    ));

    // The shell was not the terminal it ran in: it is still there, and what it printed goes
    // to the agent without anything Claude Code showed.
    core.terminal_write(session.id, "echo shell-$((40 + 2))\n")
        .expect("the shell is still open");
    printed(&mut shell, "shell-42").await;
    settled(&mut shell).await;
    core.session_send(session.id, "what happened?")
        .expect("sends");
    let events = turn(&mut bus, session.id).await;
    let shared = events.iter().find_map(|event| match event {
        AgentEvent::TerminalShared { text } => Some(text.as_str()),
        _ => None,
    });
    assert!(
        shared.is_some_and(|text| text.contains("shell-42") && !text.contains("/vim")),
        "{events:?}"
    );

    // Closing the session's terminals ends both.
    core.claude_terminal_open(session.id, "vim", (80, 24))
        .expect("opens once more");
    core.terminal_close(session.id);
    for gone in [
        core.terminal_write(session.id, "ls\n"),
        core.claude_terminal_write(session.id, "q"),
    ] {
        assert!(matches!(gone, Err(Error::TerminalNotOpen(id)) if id == session.id));
    }

    // Commands Swarmed does its own way, refuses, or that are no command's name, are not opened.
    for refused in ["theme", "login", "Login", "cd", "clear", "../vim", "-p", ""] {
        assert!(
            matches!(
                core.claude_terminal_open(session.id, refused, (80, 24)),
                Err(Error::NoClaudeScreen(_))
            ),
            "/{refused}"
        );
    }
}

/// What the fake agent says it was told when it started: Swarmed's guidance, whole.
async fn guidance_of(core: &Core, bus: &mut Receiver<SessionEvent>, session: i64) -> String {
    core.session_send(session, "instructions").expect("sends");
    let events = turn(bus, session).await;
    let said = events.iter().rev().find_map(|event| match event {
        AgentEvent::Text { text } => Some(text.clone()),
        _ => None,
    });
    said.expect("the agent says what it was told")
}

#[tokio::test]
async fn every_agent_is_told_to_keep_to_its_tasks_branch_and_worktree() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let branch = core.task("demo", "one").expect("test setup").1.branch;
    // With nothing else to tell, views and rules off, this is still told.
    let quiet = Settings {
        html_views: false,
        ..Settings::default()
    };
    core.set_settings(&quiet).expect("saves");
    let session = started_session(&core);
    let guidance = guidance_of(&core, &mut bus, session.id).await;
    assert!(
        guidance.starts_with("# Your task's branch and worktree"),
        "{guidance}"
    );
    assert!(guidance.contains(&format!("the task's own branch, `{branch}`")));
    // Work on another branch goes into a task the agent starts itself.
    assert!(
        guidance.contains("it belongs in a new task in Swarmed"),
        "{guidance}"
    );
    assert!(guidance.contains("`start_task`"), "{guidance}");
    // A skill that says to branch out or make a worktree first is answered in advance.
    assert!(guidance.contains("even when a skill"), "{guidance}");
    assert!(guidance.contains("`git worktree add`"), "{guidance}");
    assert!(guidance.contains("`spawn_session`"), "{guidance}");
}

#[tokio::test]
async fn agents_are_told_about_html_views_unless_the_user_turned_them_off() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "instructions")
        .expect("sends");
    let told = turn(&mut bus, session.id).await;
    assert!(
        told.iter()
            .any(|e| matches!(e, AgentEvent::Text { text } if text.contains("`html-view`"))),
        "{told:?}"
    );

    let off = Settings {
        html_views: false,
        ..Settings::default()
    };
    core.set_settings(&off).expect("saves");
    let quiet = started_session(&core);
    assert!(
        !guidance_of(&core, &mut bus, quiet.id)
            .await
            .contains("`html-view`")
    );

    // The user's own rules are told to every agent, and come first.
    let ruled = Settings {
        html_views: false,
        rules: "  Keep answers short.\nNo em dashes.\n".to_string(),
        ..Settings::default()
    };
    core.set_settings(&ruled).expect("saves");
    let obedient = started_session(&core);
    let guidance = guidance_of(&core, &mut bus, obedient.id).await;
    let (first, rest) = guidance.split_once("\n\n").expect("more than the rules");
    assert!(
        first.starts_with("The user has set these rules."),
        "{guidance}"
    );
    assert!(
        first.ends_with(":\nKeep answers short.\nNo em dashes."),
        "{guidance}"
    );
    assert!(
        rest.starts_with("# Your task's branch and worktree"),
        "{guidance}"
    );

    let id = core.view_put("<p>hi</p>".to_string());
    assert_eq!(core.view(&id).as_deref(), Some("<p>hi</p>"));
    assert_eq!(core.view("nope"), None);
}

#[tokio::test]
async fn slash_commands_come_from_the_last_start_of_the_session_or_of_another() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let reported = |session: Option<i64>| -> Vec<String> {
        let commands = core.slash_commands(session).expect("reads");
        let own = |name: &str| name == "context" || name == "review";
        let names = commands.into_iter().map(|command| command.name);
        names.filter(|name| own(name)).collect()
    };
    // Before any agent has started, only what Swarmed shows itself is known.
    assert_eq!(reported(None), ["context"]);

    let session = started_session(&core);
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;
    let known = ["context", "review"];
    assert_eq!(reported(Some(session.id)), known);

    let fresh = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    assert_eq!(reported(Some(fresh.id)), known);
    assert_eq!(reported(None), known);
    assert!(matches!(
        core.slash_commands(Some(404)),
        Err(Error::SessionNotFound(404))
    ));
}

#[tokio::test]
async fn settings_can_change_while_running_and_apply_to_the_next_start() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "slow").expect("sends");

    let settings = SessionSettings {
        model: Some("opus".into()),
        supervision: Supervision::Auto,
        effort: None,
        fast: false,
    };
    let changed = core
        .session_configure(session.id, &settings)
        .expect("changes while a turn runs");
    assert_eq!(
        (changed.model.as_deref(), changed.supervision),
        (Some("opus"), Supervision::Auto)
    );
    core.session_interrupt(session.id).expect("interrupts");
    turn(&mut bus, session.id).await;

    core.session_stop(session.id);
    next(&mut bus).await;
    core.session_start(session.id, Auth::Subscription)
        .expect("restarts");
    core.session_send(session.id, "again").expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(matches!(
        events.get(1),
        Some(AgentEvent::Init { model, permission_mode, .. })
            if model == "opus" && permission_mode == "auto"
    ));
    let unknown = core.session_configure(404, &settings);
    assert!(matches!(unknown, Err(Error::SessionNotFound(404))));
}

#[tokio::test]
async fn a_new_effort_restarts_the_agent_between_turns_and_keeps_the_conversation() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "first").expect("sends");
    turn(&mut bus, session.id).await;

    // Between turns: the agent is stopped at once.
    let high = SessionSettings {
        effort: Some(Effort::High),
        ..SessionSettings::default()
    };
    let changed = core.session_configure(session.id, &high).expect("changes");
    assert_eq!(changed.effort, Some(Effort::High));
    assert!(matches!(
        next(&mut bus).await.event,
        AgentEvent::Exited { .. }
    ));
    assert_eq!(core.running_sessions(), [0_i64; 0]);
    core.session_start(session.id, Auth::Subscription)
        .expect("restarts");

    // During a turn: it is stopped when the turn is over, not before.
    core.session_send(session.id, "slow").expect("sends");
    let max = SessionSettings {
        effort: Some(Effort::Max),
        ..SessionSettings::default()
    };
    core.session_configure(session.id, &max).expect("changes");
    assert_eq!(core.running_sessions(), [session.id]);
    core.session_interrupt(session.id).expect("interrupts");
    turn(&mut bus, session.id).await;
    assert!(matches!(
        next(&mut bus).await.event,
        AgentEvent::Exited { .. }
    ));
    assert_eq!(
        core.session(session.id).expect("exists").effort,
        Some(Effort::Max)
    );
}

#[tokio::test]
async fn a_deleted_sessions_number_is_never_given_to_another() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let new = |core: &Core| {
        core.session_new("demo", "one", &SessionOptions::default())
            .expect("test setup")
    };
    // The newest session is deleted: SQLite would give its number to the next one.
    let first = new(&core);
    core.session_delete(first.id).expect("deletes");
    let second = new(&core);
    assert!(second.id > first.id, "{} after {}", second.id, first.id);
    // Nor after Swarmed starts again.
    core.session_delete(second.id).expect("deletes");
    drop(core);
    let core = Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default()))
        .expect("opens again");
    let third = new(&core);
    assert!(third.id > second.id, "{} after {}", third.id, second.id);
}

#[tokio::test]
async fn a_deleted_session_is_gone_with_its_history_whether_open_or_closed() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let (open, closed) = (started_session(&core), started_session(&core));
    core.session_send(open.id, "hello").expect("sends");
    turn(&mut bus, open.id).await;
    core.session_close(closed.id, false).expect("closes");
    while next(&mut bus).await.event != AgentEvent::Closed {}

    // Deleted while its agent runs: every front end is told, and the agent is stopped.
    core.session_delete(open.id).expect("deletes");
    let mut told = next(&mut bus).await;
    while told.event != AgentEvent::Deleted {
        told = next(&mut bus).await;
    }
    assert_eq!(told.session_id, open.id);
    core.session_delete(closed.id).expect("deletes");
    for gone in [open.id, closed.id] {
        assert!(matches!(core.session(gone), Err(Error::SessionNotFound(id)) if id == gone));
        assert!(core.session_events(gone).is_err());
    }
    let overview = core.overview().expect("reads");
    assert_eq!(
        (
            overview[0].tasks[0].sessions.len(),
            overview[0].tasks[0].closed
        ),
        (0, 0)
    );
    assert_eq!(core.sessions_closed("demo", "one").expect("lists"), vec![]);
    // What its agent still says while it winds down has nowhere to go and breaks nothing.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(core.running_sessions(), [0_i64; 0]);
    assert!(matches!(
        core.session_delete(open.id),
        Err(Error::SessionNotFound(_))
    ));
    // The task itself is as it was, and takes new sessions.
    started_session(&core);
}

#[tokio::test]
async fn a_closed_session_stops_and_leaves_the_lists_but_keeps_its_history() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;

    core.session_close(session.id, false).expect("closes");
    // Every front end is told, so the session leaves their lists too; then its agent exits.
    assert_eq!(next(&mut bus).await.event, AgentEvent::Closed);
    assert!(matches!(
        next(&mut bus).await.event,
        AgentEvent::Exited { .. }
    ));
    assert_eq!(core.running_sessions(), [0_i64; 0]);
    assert_eq!(core.sessions("demo", "one").expect("lists"), vec![]);
    let overview = core.overview().expect("reads");
    assert_eq!(overview[0].tasks[0].sessions, vec![]);
    // The message, the start, the reply, where the agent's record stood, the turn's end, the
    // close and the agent's exit.
    assert_eq!(core.session_events(session.id).expect("kept").len(), 7);
    assert!(matches!(
        core.session_close(404, false),
        Err(Error::SessionNotFound(404))
    ));
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
            AgentEvent::UserMessage { text, .. } => Some(text.as_str()),
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

#[tokio::test]
async fn the_overview_nests_tasks_and_sessions_and_marks_the_running_ones() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    core.task_new("demo", "two", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let running = started_session(&core);
    let idle = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("test setup");

    let overview = core.overview().expect("reads");
    let [project] = overview.as_slice() else {
        panic!("one project expected: {overview:?}")
    };
    assert_eq!(project.project.name, "demo");
    let [one, two] = project.tasks.as_slice() else {
        panic!("two tasks expected")
    };
    assert_eq!(
        (one.task.name.as_str(), two.task.name.as_str()),
        ("one", "two")
    );
    assert_eq!(
        one.sessions.iter().map(|s| s.id).collect::<Vec<_>>(),
        [running.id, idle.id]
    );
    assert_eq!(one.running, [running.id]);
    assert_eq!(two.sessions, []);

    let json = serde_json::to_value(project).expect("serialises");
    assert_eq!(json["name"], "demo");
    assert_eq!(json["tasks"][0]["setup"], "ok");
    assert_eq!(json["tasks"][0]["sessions"][0]["status"], "idle");
    assert_eq!(json["tasks"][0]["sessions"][0]["supervision"], "ask");
}

/// A new session in plan mode, with its agent running.
fn planning_session(core: &Core) -> Session {
    let options = SessionOptions {
        supervision: Supervision::Plan,
        ..SessionOptions::default()
    };
    let session = core
        .session_new("demo", "one", &options)
        .expect("test setup");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    session
}

#[tokio::test]
async fn an_approved_plan_ends_plan_mode_and_a_refused_one_keeps_it() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = planning_session(&core);

    // Refused: the agent keeps planning.
    core.session_send(session.id, "add a login page")
        .expect("sends");
    let (request_id, tool_name) = permission_request(&mut bus).await;
    assert_eq!(tool_name, PLAN_TOOL);
    core.session_deny(session.id, &request_id, "Not yet.")
        .expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(events.contains(&text("still planning")));
    let still = core.session(session.id).expect("exists");
    assert_eq!(still.supervision, Supervision::Plan);

    // Approved: the session goes on the way new sessions start, and every front end is told.
    core.session_send(session.id, "add a login page")
        .expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    core.session_allow(session.id, &request_id).expect("sends");
    let events = turn(&mut bus, session.id).await;
    let default = Settings::default().default_supervision;
    assert!(events.contains(&AgentEvent::SupervisionChanged {
        supervision: default
    }));
    assert!(events.contains(&text("carrying out the plan")));
    let after = core.session(session.id).expect("exists");
    assert_eq!(after.supervision, default);

    // Out of plan mode, the agent acts again instead of planning.
    core.session_send(session.id, "hello").expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(events.contains(&text("echo: hello")));
}

#[tokio::test]
async fn allowing_an_ordinary_request_leaves_plan_mode_alone() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    core.session_allow(session.id, &request_id).expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::SupervisionChanged { .. }))
    );
}

/// Skips ahead to the next change of the session's queue and returns what it holds.
async fn queue_change(bus: &mut Receiver<SessionEvent>) -> Vec<String> {
    loop {
        if let AgentEvent::Queue { messages } = next(bus).await.event {
            return messages.into_iter().map(|message| message.text).collect();
        }
    }
}

#[tokio::test]
async fn queued_messages_are_sent_one_by_one_as_turns_end() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    // The agent is held up, so what comes next has to wait.
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    core.session_enqueue(session.id, "first").expect("queues");
    let queue = core.session_enqueue(session.id, "2nd").expect("queues");
    assert_eq!(queue.len(), 2);
    // A waiting message can still be corrected, and one emptied is taken out.
    let queue = core
        .session_queue_edit(session.id, queue[1].id, "second")
        .expect("edits");
    assert_eq!(queue[1].text, "second");
    let extra = core.session_enqueue(session.id, "third").expect("queues");
    core.session_queue_edit(session.id, extra[2].id, "  ")
        .expect("removes");
    assert_eq!(core.session_queue(session.id).expect("reads").len(), 2);
    assert!(matches!(
        core.session_enqueue(session.id, " "),
        Err(Error::EmptyMessage)
    ));

    core.session_allow(session.id, &request_id).expect("sends");
    turn(&mut bus, session.id).await;
    // The turn ended: the first goes out, then, when that turn ends, the second.
    let first = turn(&mut bus, session.id).await;
    assert!(first.contains(&text("echo: first")), "{first:?}");
    let second = turn(&mut bus, session.id).await;
    assert!(second.contains(&text("echo: second")), "{second:?}");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);

    // The queue is never part of the stored conversation.
    let stored = core.session_events(session.id).expect("reads");
    assert!(
        !stored
            .iter()
            .any(|event| matches!(event, AgentEvent::Queue { .. }))
    );
}

#[tokio::test]
async fn stopping_a_turn_sends_the_next_waiting_message() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "slow").expect("sends");
    core.session_enqueue(session.id, "after").expect("queues");
    assert_eq!(queue_change(&mut bus).await, ["after"]);
    core.session_interrupt(session.id).expect("interrupts");
    let stopped = turn(&mut bus, session.id).await;
    assert!(
        stopped
            .iter()
            .any(|event| matches!(event, AgentEvent::TurnDone { is_error: true, .. })),
        "{stopped:?}"
    );
    let next = turn(&mut bus, session.id).await;
    assert!(next.contains(&text("echo: after")), "{next:?}");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);

    // Stopped while it waits for an answer, the request is taken back and the same follows.
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    core.session_enqueue(session.id, "then").expect("queues");
    core.session_interrupt(session.id).expect("interrupts");
    let stopped = turn(&mut bus, session.id).await;
    assert!(
        stopped.contains(&AgentEvent::PermissionWithdrawn { request_id }),
        "{stopped:?}"
    );
    let next = turn(&mut bus, session.id).await;
    assert!(next.contains(&text("echo: then")), "{next:?}");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);
}

#[tokio::test]
async fn a_failed_turn_or_a_stopped_agent_leaves_the_queue_for_the_user_to_send() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_enqueue(session.id, "after").expect("queues");
    core.session_send(session.id, "fail").expect("sends");
    let failed = turn(&mut bus, session.id).await;
    assert!(failed.contains(&text("trying…")), "{failed:?}");
    assert_eq!(core.session_queue(session.id).expect("reads").len(), 1);

    core.session_send_queued(session.id).expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(events.contains(&text("echo: after")), "{events:?}");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);

    // With the agent stopped the message stays where it was.
    core.session_enqueue(session.id, "later").expect("queues");
    core.session_stop(session.id);
    assert!(matches!(
        core.session_send_queued(session.id),
        Err(Error::SessionNotRunning(_))
    ));
    assert_eq!(core.session_queue(session.id).expect("reads").len(), 1);
}

#[tokio::test]
async fn a_stop_cut_short_by_stopping_the_agent_does_not_send_the_queue_later() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "slow").expect("sends");
    core.session_enqueue(session.id, "after").expect("queues");
    core.session_interrupt(session.id).expect("interrupts");
    // Stopped and started again before the stopped turn ends: that end is the old run's, and
    // is dropped, so nothing but the stop of the agent clears what the interrupt marked.
    core.session_stop(session.id);
    core.session_start(session.id, Auth::default())
        .expect("starts");
    core.session_send(session.id, "fail").expect("sends");
    let failed = turn(&mut bus, session.id).await;
    assert!(failed.contains(&text("trying…")), "{failed:?}");
    assert_eq!(core.session_queue(session.id).expect("reads").len(), 1);
}

#[tokio::test]
async fn a_waiting_message_sent_now_stops_the_turn_and_goes_ahead_of_the_others() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "slow").expect("sends");
    core.session_enqueue(session.id, "first").expect("queues");
    let queue = core.session_enqueue(session.id, "second").expect("queues");
    core.session_send_now(session.id, queue[1].id, Auth::default())
        .expect("sends now");
    assert_eq!(queue_change(&mut bus).await, ["first"]);
    assert_eq!(queue_change(&mut bus).await, ["first", "second"]);
    assert_eq!(queue_change(&mut bus).await, ["second", "first"]);

    // The turn is stopped, the one sent now goes, and the rest follow as turns end well.
    let stopped = turn(&mut bus, session.id).await;
    assert!(
        stopped
            .iter()
            .any(|event| matches!(event, AgentEvent::TurnDone { is_error: true, .. })),
        "{stopped:?}"
    );
    let now = turn(&mut bus, session.id).await;
    assert!(now.contains(&text("echo: second")), "{now:?}");
    let then = turn(&mut bus, session.id).await;
    assert!(then.contains(&text("echo: first")), "{then:?}");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);

    // One no longer waiting is left alone.
    core.session_send_now(session.id, queue[0].id, Auth::default())
        .expect("nothing to send");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);
}

#[tokio::test]
async fn a_waiting_message_sent_now_takes_the_place_of_an_open_request() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "run: rm -rf build")
        .expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    let queue = core.session_enqueue(session.id, "instead").expect("queues");
    core.session_send_now(session.id, queue[0].id, Auth::default())
        .expect("sends now");

    let stopped = turn(&mut bus, session.id).await;
    assert!(
        stopped.contains(&AgentEvent::PermissionWithdrawn { request_id }),
        "{stopped:?}"
    );
    let now = turn(&mut bus, session.id).await;
    assert!(now.contains(&text("echo: instead")), "{now:?}");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);
}

#[tokio::test]
async fn a_waiting_message_sent_now_starts_an_agent_that_was_stopped() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "slow").expect("sends");
    let queue = core.session_enqueue(session.id, "after").expect("queues");
    core.session_stop(session.id);
    core.session_send_now(session.id, queue[0].id, Auth::default())
        .expect("starts and sends");
    let events = turn(&mut bus, session.id).await;
    let events = if events.contains(&text("echo: after")) {
        events
    } else {
        // What the stopped run still said is passed over.
        turn(&mut bus, session.id).await
    };
    assert!(events.contains(&text("echo: after")), "{events:?}");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);
}

#[tokio::test]
async fn a_session_left_busy_by_a_swarmed_that_ended_can_be_stopped_and_is_settled_at_start() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "slow").expect("sends");
    while core.session(session.id).expect("exists").status != SessionStatus::Working {
        next(&mut bus).await;
    }
    // That Swarmed ends in the middle of the turn: nothing gets to say the turn is over.
    drop(core);

    let later =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let status = |core: &Core| core.session(session.id).expect("exists").status;
    assert_eq!(status(&later), SessionStatus::Working);
    // Stopping it puts it right, with a turn recorded as stopped, instead of refusing.
    later.session_interrupt(session.id).expect("stops");
    assert_eq!(status(&later), SessionStatus::Idle);
    let events = later.session_events(session.id).expect("reads");
    let last = events.last().expect("recorded");
    assert!(
        matches!(last, AgentEvent::TurnDone { is_error: true, .. }),
        "{last:?}"
    );
    // An idle session that is not running still has nothing to stop.
    assert!(matches!(
        later.session_interrupt(session.id),
        Err(Error::SessionNotRunning(_))
    ));

    // And a front end that starts settles whatever is left busy, all at once.
    later.arrive().expect("arrives");
    let other = later
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    later
        .session_start(other.id, Auth::Subscription)
        .expect("starts");
    later.session_send(other.id, "slow").expect("sends");
    let mut bus = later.subscribe();
    while status_of(&later, other.id) != SessionStatus::Working {
        next(&mut bus).await;
    }
    // Not one that starts beside a Swarmed still running, though: the session is that one's.
    let beside =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    assert_eq!(beside.arrive().expect("arrives"), 0);
    assert_eq!(status_of(&beside, other.id), SessionStatus::Working);
    drop(later);
    let fresh =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    // Not exactly one: within one process the first core's agent, cut off above, may still
    // have marked its session busy, which a Swarmed that really ended could not.
    assert!(arrives_first(&fresh).await >= 1);
    assert_eq!(status_of(&fresh, other.id), SessionStatus::Idle);
}

/// How many sessions a core settles once it is the first on its data. The lock is let go when
/// its file is closed everywhere, and a program another test is just starting holds a copy of
/// it for a moment, until it has become that program.
async fn arrives_first(core: &Core) -> usize {
    for _ in 0..200 {
        let settled = core.arrive().expect("arrives");
        if settled > 0 {
            return settled;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    0
}

fn status_of(core: &Core, id: i64) -> SessionStatus {
    core.session(id).expect("exists").status
}

#[tokio::test]
async fn a_commit_message_is_written_from_what_changed_and_commits_nothing() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    let worktree = core.task("demo", "one").expect("exists").1.worktree_path;
    // Nothing changed, then nothing to write about; and nobody to write.
    assert!(core.task_commit_message("demo", "one", None).await.is_err());
    std::fs::write(worktree.join("notes.txt"), "one\n").expect("writes");
    assert!(core.task_commit_message("demo", "one", None).await.is_err());

    core.set_namer(Arc::new(FixedNamer("unused")));
    let message = core
        .task_commit_message("demo", "one", None)
        .await
        .expect("writes");
    // The fence is gone, and the model was shown the new file.
    assert!(
        message.starts_with("feat: written\n\ngit status"),
        "{message}"
    );
    assert!(message.contains("?? notes.txt"), "{message}");
    assert!(message.contains("new file notes.txt:\none"), "{message}");
    let left = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!((left.changed, left.unpushed), (1, 0));
}

#[tokio::test]
async fn a_pull_request_is_written_from_the_branch_and_opens_nothing() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    core.set_namer(Arc::new(FixedNamer("unused")));
    let worktree = core.task("demo", "one").expect("exists").1.worktree_path;
    // A branch with no commits of its own has nothing to describe.
    let nothing = core.task_pull_message("demo", "one", None).await;
    assert!(nothing.is_err());

    std::fs::write(worktree.join("notes.txt"), "one\n").expect("writes");
    std::fs::create_dir_all(worktree.join(".github")).expect("creates");
    let template = worktree.join(".github/pull_request_template.md");
    std::fs::write(template, "## What\n## Why\n").expect("writes");
    core.task_commit(
        "demo",
        "one",
        "feat: add notes\n\nSo there is somewhere to write.",
    )
    .await
    .expect("commits");

    let draft = core
        .task_pull_message("demo", "one", None)
        .await
        .expect("writes");
    // The first line is the title, without the heading mark some models put before it.
    assert_eq!(draft.title, "feat: add notes");
    assert!(draft.body.starts_with("**Adds notes.**"), "{}", draft.body);
    // The writer was shown the commits, the diff and the repository's own template.
    assert!(draft.body.contains("So there is somewhere to write."));
    assert!(draft.body.contains("+one"), "{}", draft.body);
    assert!(draft.body.contains("2 files changed"), "{}", draft.body);
    assert!(draft.body.contains("pull request template:\n## What"));

    // Without anything that can write, it says so.
    let bare = Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default()));
    let refused = bare
        .expect("opens")
        .task_pull_message("demo", "one", None)
        .await;
    assert!(refused.is_err());
}

#[tokio::test]
async fn a_new_task_is_named_by_the_namer_and_its_session_keeps_that_title() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    // Nothing to ask yet: the caller names the task itself.
    assert_eq!(core.task_name_for("demo", "x").await.expect("asks"), None);

    core.set_namer(Arc::new(FixedNamer("\"Fix Login Redirect\"")));
    let naming = core
        .task_name_for("demo", "users get stuck after logging in, please fix")
        .await
        .expect("asks")
        .expect("names");
    assert_eq!(naming.name, "fix-login-redirect");
    assert_eq!(naming.title, "Fix Login Redirect");

    // A name already taken gets a number.
    core.task_new(
        "demo",
        &naming.name,
        NewTaskOptions::default(),
        &hooks::discard,
    )
    .await
    .expect("creates");
    let again = core
        .task_name_for("demo", "same again")
        .await
        .expect("asks");
    assert_eq!(again.expect("names").name, "fix-login-redirect-2");

    // The session takes the title, and its first message does not ask for another.
    let mut bus = core.subscribe();
    let session = core
        .session_new("demo", &naming.name, &SessionOptions::default())
        .expect("creates");
    core.session_rename(session.id, &naming.title)
        .expect("titles");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;
    let title = core.session(session.id).expect("exists").title;
    assert_eq!(title, "Fix Login Redirect");

    // Switched off, nothing is asked.
    let off = Settings {
        auto_title: false,
        ..Settings::default()
    };
    core.set_settings(&off).expect("saves");
    assert_eq!(core.task_name_for("demo", "x").await.expect("asks"), None);
    assert!(matches!(
        core.task_name_for("nowhere", "x").await,
        Err(Error::ProjectNotFound(_))
    ));
}

#[tokio::test]
async fn a_message_that_calls_a_skill_is_named_after_what_the_skill_is_for() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    core.skill_save(
        "triage",
        "Sort a bug report by how urgent it is",
        "Read it.",
    )
    .expect("saves");
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let last_asked = || asked.lock().expect("not poisoned").pop().expect("asked");
    // Given a skill call as typed, a model without tools answers with a stand-in for one.
    core.set_namer(Arc::new(AskedNamer {
        answer: "<invoke_skill_or_bash_placeholder>",
        asked: Arc::clone(&asked),
    }));
    let naming = core
        .task_name_for("demo", "/triage 123")
        .await
        .expect("asks");
    assert_eq!(naming, None, "the caller names the task after the prompt");
    assert_eq!(
        last_asked(),
        "The user calls the skill `triage`, which is for: Sort a bug report by how urgent it \
         is\nWhat they give after it: 123"
    );

    // Nor does it rename a session.
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "/triage 456").expect("sends");
    turn(&mut bus, session.id).await;
    // Renamed by hand once the namer's answer could have arrived: had it been taken, its
    // title would have come first, and after this one the namer leaves the title alone.
    core.session_rename(session.id, "Mine").expect("renames");
    assert_eq!(renamed(&mut bus).await, "Mine");
    assert!(last_asked().contains("skill `triage`"));

    // A command Swarmed knows nothing of is named by its name and what follows.
    core.set_namer(Arc::new(AskedNamer {
        answer: "Triage Bug 789",
        asked: Arc::clone(&asked),
    }));
    let other = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    core.session_start(other.id, Auth::Subscription)
        .expect("starts");
    core.session_send(other.id, "/unheard-of 789")
        .expect("sends");
    assert_eq!(renamed(&mut bus).await, "Triage Bug 789");
    assert_eq!(
        last_asked(),
        "The user calls the command `unheard-of`.\nWhat they give after it: 789"
    );
}

#[tokio::test]
async fn what_was_said_is_found_again_in_open_and_closed_sessions() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let first = started_session(&core);
    core.session_send(first.id, "Why is the Zeppelin 100% \"late\"?")
        .expect("sends");
    turn(&mut bus, first.id).await;
    let second = started_session(&core);
    core.session_send(second.id, "Nothing about airships here")
        .expect("sends");
    turn(&mut bus, second.id).await;

    let hits = core.sessions_search("zeppelin").expect("searches");
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!((hit.session_id, hit.closed), (first.id, false));
    assert_eq!((hit.project.as_str(), hit.task.as_str()), ("demo", "one"));
    assert!(hit.snippet.contains("Zeppelin"));
    // What the fake agent answers repeats the message, so both sides may hold the word.
    assert!(hit.matches >= 1);

    // Signs that mean something to the database or to JSON are taken as they are typed.
    assert_eq!(
        core.sessions_search("100% \"late\"")
            .expect("searches")
            .len(),
        1
    );
    assert_eq!(core.sessions_search("100_").expect("searches"), []);
    assert_eq!(core.sessions_search("   ").expect("searches"), []);
    // A word that only stands in an event's own field names is not something anybody said.
    assert_eq!(core.sessions_search("user_message").expect("searches"), []);

    core.session_close(first.id, false).expect("closes");
    let closed = core.sessions_search("ZEPPELIN").expect("searches");
    assert_eq!(closed.len(), 1);
    assert!(closed[0].closed);
}

#[tokio::test]
async fn a_fork_from_a_message_holds_it_and_what_followed_and_nothing_from_before() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let source = started_session(&core);
    for said in ["first question", "second question", "third question"] {
        core.session_send(source.id, said).expect("sends");
        turn(&mut bus, source.id).await;
    }
    let messages = |id: i64| -> Vec<String> {
        let events = core.session_events(id).expect("reads");
        let said = events.into_iter().filter_map(|event| match event {
            AgentEvent::UserMessage { text, .. } => Some(text),
            _ => None,
        });
        said.collect()
    };
    // From the second message: it and the third, each with its answer, the last one too.
    let fork = core.session_fork(source.id, Some(1), None).expect("forks");
    assert_eq!(messages(fork.id), ["second question", "third question"]);
    let copied = core.session_events(fork.id).expect("reads");
    let answers = copied
        .iter()
        .filter(|event| matches!(event, AgentEvent::Text { .. }));
    assert_eq!(answers.count(), 2);
    assert_eq!(copied.last(), Some(&AgentEvent::Forked { from: source.id }));
    // From the last: that message and its answer alone.
    let last = core.session_fork(source.id, Some(2), None).expect("forks");
    assert_eq!(messages(last.id), ["third question"]);
    // From the first: all of it. The first session keeps everything either way.
    let whole = core.session_fork(source.id, Some(0), None).expect("forks");
    assert_eq!(messages(whole.id).len(), 3);
    assert_eq!(messages(source.id).len(), 3);
}

#[tokio::test]
async fn a_fork_starts_with_what_was_said_and_tells_its_agent_once() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let source = started_session(&core);
    core.session_send(source.id, "The password is swordfish")
        .expect("sends");
    turn(&mut bus, source.id).await;
    let said = core.session_events(source.id).expect("reads");

    let fork = core.session_fork(source.id, None, None).expect("forks");
    assert_eq!(fork.task_id, source.task_id);
    assert_eq!(fork.agent, source.agent);
    let fork_row = core.session(fork.id).expect("reads");
    assert!(fork_row.title.ends_with("(fork)"), "{}", fork_row.title);
    assert!(!fork_row.unread);
    assert_eq!(fork_row.status, SessionStatus::Idle);
    let copied = core.session_events(fork.id).expect("reads");
    assert_eq!(copied.first(), said.first());
    assert_eq!(copied.last(), Some(&AgentEvent::Forked { from: source.id }));
    // The source is as it was.
    assert_eq!(core.session_events(source.id).expect("reads"), said);

    // The fake agent repeats what it is sent: the first message carries the conversation,
    // the second does not, and what is recorded is what was typed.
    core.session_start(fork.id, Auth::Subscription)
        .expect("starts");
    core.session_send(fork.id, "echo: what was it?")
        .expect("sends");
    let first = turn(&mut bus, fork.id).await;
    assert_eq!(
        first.first(),
        Some(&AgentEvent::UserMessage {
            text: "echo: what was it?".into(),
            remote: false,
            at_reset: false,
            from_session: None,
        })
    );
    let heard = |events: &[AgentEvent]| {
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Text { text } if text.contains("swordfish")))
    };
    assert!(heard(&first), "{first:?}");
    core.session_send(fork.id, "echo: and again?")
        .expect("sends");
    assert!(!heard(&turn(&mut bus, fork.id).await));
}

#[tokio::test]
async fn a_fork_whose_agent_died_on_the_first_message_is_told_again() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let source = started_session(&core);
    core.session_send(source.id, "The password is swordfish")
        .expect("sends");
    turn(&mut bus, source.id).await;
    let fork = core.session_fork(source.id, None, None).expect("forks");

    // The first message got the conversation, but its agent went before the turn was over.
    core.session_start(fork.id, Auth::Subscription)
        .expect("starts");
    core.session_send(fork.id, "run: ls").expect("sends");
    permission_request(&mut bus).await;
    core.session_stop(fork.id);
    while !matches!(next(&mut bus).await.event, AgentEvent::Exited { .. }) {}

    let heard = |events: &[AgentEvent]| {
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Text { text } if text.contains("swordfish")))
    };
    // So the next try carries it too, and from the first turn that ends well no more.
    core.session_start(fork.id, Auth::Subscription)
        .expect("starts");
    core.session_send(fork.id, "what was it?").expect("sends");
    let retried = turn(&mut bus, fork.id).await;
    assert!(heard(&retried), "{retried:?}");
    core.session_send(fork.id, "and again?").expect("sends");
    assert!(!heard(&turn(&mut bus, fork.id).await));
}

#[tokio::test]
async fn a_queued_message_is_sent_to_the_agent_restarted_for_new_settings() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    // A turn is under way, a message waits for its end, and the effort is changed meanwhile.
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    core.session_enqueue(session.id, "next").expect("queues");
    let high = SessionSettings {
        effort: Some(Effort::High),
        ..SessionSettings::default()
    };
    core.session_configure(session.id, &high).expect("changes");
    core.session_allow(session.id, &request_id).expect("sends");
    turn(&mut bus, session.id).await;

    // The message goes out all the same, and to an agent started with the new effort: it
    // says that it has started, which an agent does once, and continues the conversation.
    let queued = turn(&mut bus, session.id).await;
    assert!(queued.contains(&text("echo: next")), "{queued:?}");
    assert!(
        queued.iter().any(|event| matches!(
            event,
            AgentEvent::Init { agent_session_id, .. } if agent_session_id == "fake-1"
        )),
        "{queued:?}"
    );
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);
    assert_eq!(core.running_sessions(), [session.id]);
    assert_eq!(status_of(&core, session.id), SessionStatus::Idle);
    // The agent it took over from goes quietly: its end is not the session's.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    while let Ok(late) = bus.try_recv() {
        assert!(!matches!(late.event, AgentEvent::Exited { .. }), "{late:?}");
    }
    assert_eq!(core.running_sessions(), [session.id]);
}

/// Everything published up to the next permission request, and that request's id.
async fn heard_until_asked(bus: &mut Receiver<SessionEvent>) -> (Vec<AgentEvent>, String) {
    let mut heard = vec![];
    loop {
        let published = next(bus).await.event;
        if let AgentEvent::PermissionRequest { request_id, .. } = &published {
            return (heard, request_id.clone());
        }
        heard.push(published);
    }
}

#[tokio::test]
async fn what_a_replaced_agent_still_says_does_not_change_how_the_session_stands() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "slow").expect("sends");
    while next(&mut bus).await.event != text("working…") {}

    // Stopped and started again at once, before the first agent has had time to go. The new
    // one is asked for something that needs a decision.
    core.session_stop(session.id);
    core.session_start(session.id, Auth::Subscription)
        .expect("restarts");
    core.session_send(session.id, "run: ls").expect("sends");
    let (mut heard, request_id) = heard_until_asked(&mut bus).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    while let Ok(late) = bus.try_recv() {
        heard.push(late.event);
    }

    // The first agent has ended its turn and gone by now. None of that is the session's:
    // it still waits, on a request that is still open, with nothing new to read.
    let an_end = |event: &AgentEvent| {
        matches!(
            event,
            AgentEvent::TurnDone { .. } | AgentEvent::Exited { .. }
        )
    };
    assert!(!heard.iter().any(an_end), "{heard:?}");
    let stands = core.session(session.id).expect("exists");
    assert_eq!(stands.status, SessionStatus::Waiting);
    assert!(!stands.unread);
    assert_eq!(core.running_sessions(), [session.id]);
    core.session_allow(session.id, &request_id).expect("sends");
    let rest = turn(&mut bus, session.id).await;
    assert!(rest.contains(&text("ran it")), "{rest:?}");
}

#[tokio::test]
async fn nothing_is_heard_of_a_session_after_it_was_deleted() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "slow").expect("sends");
    while next(&mut bus).await.event != text("working…") {}

    core.session_delete(session.id).expect("deletes");
    assert_eq!(next(&mut bus).await.event, AgentEvent::Deleted);
    // Its agent was in the middle of a turn. How that ends is nobody's business any more.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(bus.try_recv().is_err(), "something followed the deletion");
    assert_eq!(core.running_sessions(), [0_i64; 0]);
}

/// Starts the session's agent from a task of its own, which may run on another thread.
fn start_elsewhere(core: &Arc<Core>, id: i64) -> tokio::task::JoinHandle<Result<(), Error>> {
    let core = Arc::clone(core);
    tokio::spawn(async move { core.session_start(id, Auth::Subscription) })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_session_started_from_two_places_at_once_gets_one_agent() {
    let fx = fixture();
    let core = Arc::new(core_with_task(&fx).await);
    let mut bus = core.subscribe();
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    let starts: Vec<_> = (0..8).map(|_| start_elsewhere(&core, session.id)).collect();
    for start in starts {
        start.await.expect("runs").expect("starts");
    }
    assert_eq!(core.running_sessions(), [session.id]);
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;

    // The scripted agent numbers the agents it starts: had the session been given two, the
    // next session's would be the third.
    let other = started_session(&core);
    core.session_send(other.id, "hello").expect("sends");
    let events = turn(&mut bus, other.id).await;
    assert!(
        events.iter().any(|event| matches!(
            event,
            AgentEvent::Init { agent_session_id, .. } if agent_session_id == "fake-2"
        )),
        "{events:?}"
    );
}

/// A tool that moves the task's worktree to another branch is noticed when it answers: once,
/// in the session at work, worth a notice, and the branch is remembered for the task. A move
/// the agent did not see, made between its turns, is told to it with the next message.
#[tokio::test]
async fn a_worktree_moved_by_a_tool_is_noticed_and_one_moved_between_turns_is_told() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let (_, task) = core.task("demo", "one").expect("test setup");
    let own = task.branch.clone();
    let session = started_session(&core);
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;

    // The tool runs while its permission is asked: the worktree moves meanwhile.
    core.session_send(session.id, "run: git switch -c second")
        .expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    git(&task.worktree_path, &["switch", "--quiet", "-c", "second"]);
    core.session_allow(session.id, &request_id).expect("sends");
    let events = turn(&mut bus, session.id).await;
    let moved = AgentEvent::WorktreeMoved {
        from: Some(own.clone()),
        to: Some("second".to_string()),
        between_turns: false,
    };
    assert_eq!(
        events.iter().filter(|event| **event == moved).count(),
        1,
        "{events:?}"
    );
    let event = SessionEvent {
        session_id: session.id,
        status: None,
        unread: None,
        background: None,
        event: moved,
        elsewhere: false,
    };
    let notice = core.notice_for(&event).expect("reads").expect("a notice");
    assert!(notice.title.starts_with("Moved its worktree"), "{notice:?}");
    assert!(notice.body.contains("Now on second"), "{notice:?}");
    let unpublished = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!(unpublished.branches, [own.clone(), "second".to_string()]);

    // Put back from Swarmed between turns: nothing is reported for that, and the agent is
    // told with the next message, once.
    core.task_switch("demo", "one", &own)
        .await
        .expect("switches");
    core.session_send(session.id, "next").expect("sends");
    let events = turn(&mut bus, session.id).await;
    let told = AgentEvent::WorktreeMoved {
        from: Some("second".to_string()),
        to: Some(own.clone()),
        between_turns: true,
    };
    assert_eq!(
        events.iter().filter(|event| **event == told).count(),
        1,
        "{events:?}"
    );
    let heard = format!("it was on `second`, and is on `{own}` now");
    let echoed = |event: &AgentEvent| matches!(event, AgentEvent::Text { text } if text.starts_with("echo: next") && text.contains(&heard));
    assert!(events.iter().any(echoed), "{events:?}");
    core.session_send(session.id, "again").expect("sends");
    let events = turn(&mut bus, session.id).await;
    let quiet = !events
        .iter()
        .any(|event| matches!(event, AgentEvent::WorktreeMoved { .. }));
    assert!(quiet, "{events:?}");
}

/// An agent is started with the task's variables, as hooks have them: a skill can tell from
/// them that it already runs in a task's own worktree, and on which branch.
#[tokio::test]
async fn an_agent_knows_the_task_it_works_in() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let (_, task) = core.task("demo", "one").expect("test setup");
    let worktree = task.worktree_path.to_string_lossy().into_owned();
    let session = started_session(&core);
    let expected = [
        ("SWARMED_PROJECT", "demo"),
        ("SWARMED_TASK", "one"),
        ("SWARMED_BRANCH", task.branch.as_str()),
        ("SWARMED_WORKTREE", worktree.as_str()),
    ];
    for (name, value) in expected {
        core.session_send(session.id, &format!("env: {name}"))
            .expect("sends");
        let said = turn(&mut bus, session.id).await;
        assert!(said.contains(&text(value)), "{name}: {said:?}");
    }
}

/// Whether nothing is sent to the session's agent for a moment: no message of the user's
/// arrives on the bus.
async fn nothing_sent(bus: &mut Receiver<SessionEvent>, session: i64) -> bool {
    let sent_here = |event: &SessionEvent| {
        event.session_id == session && matches!(event.event, AgentEvent::UserMessage { .. })
    };
    let sent = async {
        // A stream that lagged or closed counts as sent: the test then fails rather than pass.
        while bus
            .recv()
            .await
            .as_ref()
            .is_ok_and(|event| !sent_here(event))
        {}
    };
    let moment = std::time::Duration::from_millis(300);
    tokio::time::timeout(moment, sent).await.is_err()
}

#[tokio::test]
async fn a_waiting_message_open_for_editing_is_sent_once_the_edit_is_over() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    // The turn ends well while the first message is open: it is not sent.
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    let queue = core.session_enqueue(session.id, "first").expect("queues");
    assert!(
        core.session_queue_hold(session.id, queue[0].id)
            .expect("holds")
    );
    core.session_allow(session.id, &request_id).expect("sends");
    turn(&mut bus, session.id).await;
    assert!(nothing_sent(&mut bus, session.id).await);
    assert_eq!(core.session_queue(session.id).expect("reads"), queue);

    // Saved, it goes at once, with its new text.
    core.session_queue_edit(session.id, queue[0].id, "edited")
        .expect("edits");
    let sent = turn(&mut bus, session.id).await;
    assert!(sent.contains(&text("echo: edited")), "{sent:?}");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);

    // Given up, it goes as it was. A message no longer waiting cannot be held.
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    let queue = core
        .session_enqueue(session.id, "as it was")
        .expect("queues");
    assert!(
        core.session_queue_hold(session.id, queue[0].id)
            .expect("holds")
    );
    core.session_allow(session.id, &request_id).expect("sends");
    turn(&mut bus, session.id).await;
    assert!(nothing_sent(&mut bus, session.id).await);
    core.session_queue_let_go(session.id, queue[0].id)
        .expect("lets go");
    let sent = turn(&mut bus, session.id).await;
    assert!(sent.contains(&text("echo: as it was")), "{sent:?}");
    assert!(
        !core
            .session_queue_hold(session.id, queue[0].id)
            .expect("asks")
    );
}

#[tokio::test]
async fn a_held_message_further_back_does_not_stop_those_ahead_of_it() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    core.session_enqueue(session.id, "first").expect("queues");
    let queue = core.session_enqueue(session.id, "second").expect("queues");
    core.session_queue_hold(session.id, queue[1].id)
        .expect("holds");
    core.session_allow(session.id, &request_id).expect("sends");
    turn(&mut bus, session.id).await;
    let first = turn(&mut bus, session.id).await;
    assert!(first.contains(&text("echo: first")), "{first:?}");
    // Now first, and still open: it waits for the edit.
    assert!(nothing_sent(&mut bus, session.id).await);
    core.session_queue_edit(session.id, queue[1].id, "second, edited")
        .expect("edits");
    let second = turn(&mut bus, session.id).await;
    assert!(second.contains(&text("echo: second, edited")), "{second:?}");
}

#[tokio::test]
async fn a_stopped_turn_and_a_message_sent_now_wait_for_an_edit_too() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    // Stopping the turn would send the first message: it is open, so it waits.
    core.session_send(session.id, "slow").expect("sends");
    let queue = core.session_enqueue(session.id, "after").expect("queues");
    core.session_queue_hold(session.id, queue[0].id)
        .expect("holds");
    core.session_interrupt(session.id).expect("interrupts");
    turn(&mut bus, session.id).await;
    assert!(nothing_sent(&mut bus, session.id).await);
    core.session_queue_let_go(session.id, queue[0].id)
        .expect("lets go");
    let after = turn(&mut bus, session.id).await;
    assert!(after.contains(&text("echo: after")), "{after:?}");

    // Sent now while open, the turn is stopped and the message goes once the edit is over.
    core.session_send(session.id, "slow").expect("sends");
    let queue = core.session_enqueue(session.id, "now").expect("queues");
    core.session_queue_hold(session.id, queue[0].id)
        .expect("holds");
    core.session_send_now(session.id, queue[0].id, Auth::default())
        .expect("sends now");
    turn(&mut bus, session.id).await;
    assert!(nothing_sent(&mut bus, session.id).await);
    core.session_queue_edit(session.id, queue[0].id, "now, edited")
        .expect("edits");
    let now = turn(&mut bus, session.id).await;
    assert!(now.contains(&text("echo: now, edited")), "{now:?}");
}

/// What the session published up to and with its `turns`th turn end from now.
async fn until_turns(
    bus: &mut Receiver<SessionEvent>,
    session: i64,
    turns: usize,
) -> Vec<SessionEvent> {
    let mut heard = vec![];
    let mut ended = 0;
    while ended < turns {
        let event = next(bus).await;
        if event.session_id != session {
            continue;
        }
        ended += usize::from(matches!(event.event, AgentEvent::TurnDone { .. }));
        heard.push(event);
    }
    heard
}

fn turn_ends(events: &[SessionEvent]) -> Vec<&SessionEvent> {
    let ends = |event: &&SessionEvent| matches!(event.event, AgentEvent::TurnDone { .. });
    events.iter().filter(ends).collect()
}

#[tokio::test]
async fn a_session_waiting_on_background_sub_agents_is_neither_done_nor_busy() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "background agent")
        .expect("sends");
    let events = until_turns(&mut bus, session.id, 2).await;
    let ends = turn_ends(&events);
    // The turn ends to wait: the session can take a message, but nothing is handed back yet.
    let waiting = ends[0];
    assert_eq!(waiting.status, Some(SessionStatus::Idle));
    assert_eq!(waiting.unread, None);
    assert_eq!(core.notice_for(waiting).expect("reads"), None);
    // What the sub-agent says meanwhile leaves the session idle.
    let sub = events
        .iter()
        .find(|event| matches!(event.event, AgentEvent::Sub { .. }));
    assert_eq!(sub.expect("the sub-agent wrote").status, None);
    // Its report, read in a turn of the agent's own, is what hands something back.
    let back = ends[1];
    assert_eq!(back.unread, Some(true));
    let notice = core.notice_for(back).expect("reads").expect("a notice");
    assert!(notice.title.starts_with("Finished"), "{notice:?}");
    assert!(core.session(session.id).expect("reads").unread);

    // A command left running in the background holds nothing up.
    core.session_send(session.id, "background bash")
        .expect("sends");
    let events = until_turns(&mut bus, session.id, 1).await;
    assert_eq!(turn_ends(&events)[0].unread, Some(true));
}

#[tokio::test]
async fn the_agent_ending_ends_a_wait_on_background_sub_agents() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "background agent left")
        .expect("sends");
    let events = until_turns(&mut bus, session.id, 1).await;
    assert_eq!(turn_ends(&events)[0].unread, None);
    assert!(!core.session(session.id).expect("reads").unread);

    // The answer to a later message is handed back, though the sub-agent works on.
    core.session_send(session.id, "hello").expect("sends");
    let events = until_turns(&mut bus, session.id, 1).await;
    assert_eq!(turn_ends(&events)[0].unread, Some(true));
    // And the agent starting another leaves nothing to read again.
    core.session_send(session.id, "background agent left")
        .expect("sends");
    let events = until_turns(&mut bus, session.id, 1).await;
    assert_eq!(turn_ends(&events)[0].unread, None);

    // Its agent stopped, what it left working went with it: the next answer counts again.
    core.session_stop(session.id);
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    core.session_send(session.id, "hello").expect("sends");
    let events = until_turns(&mut bus, session.id, 1).await;
    assert_eq!(turn_ends(&events)[0].unread, Some(true));
}

/// What the session has running in the background, as `agents` sub-agents and `commands`.
fn counted(agents: u32, commands: u32) -> BackgroundCount {
    BackgroundCount { agents, commands }
}

/// The counts the session's events carried, in order.
fn counts_carried(events: &[SessionEvent]) -> Vec<BackgroundCount> {
    events.iter().filter_map(|event| event.background).collect()
}

/// Skips ahead to the end of the session's agent.
async fn exited(bus: &mut Receiver<SessionEvent>, session: i64) -> SessionEvent {
    loop {
        let event = next(bus).await;
        if event.session_id == session && matches!(event.event, AgentEvent::Exited { .. }) {
            return event;
        }
    }
}

/// What the tree, read again from the database, says session `id` has in the background.
fn counted_in_tree(core: &Core, id: i64) -> BackgroundCount {
    let tree = core.overview().expect("reads");
    let sessions = tree.iter().flat_map(|project| &project.tasks);
    let session = sessions
        .flat_map(|task| &task.sessions)
        .find(|session| session.id == id);
    session.expect("listed").background
}

#[tokio::test]
async fn a_sessions_background_count_follows_its_reports_and_rides_on_its_events() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    assert_eq!(session.background, counted(0, 0));

    // A sub-agent counts from the report of its start to the report of its end.
    core.session_send(session.id, "background agent")
        .expect("sends");
    let events = until_turns(&mut bus, session.id, 2).await;
    assert_eq!(counts_carried(&events), [counted(1, 0), counted(0, 0)]);
    // Only what can change it carries it.
    for event in &events {
        let report = matches!(event.event, AgentEvent::Background { .. });
        assert_eq!(event.background.is_some(), report, "{event:?}");
    }
    assert_eq!(counted_in_tree(&core, session.id), counted(0, 0));

    // A command left running is counted apart from a sub-agent left at work.
    core.session_send(session.id, "background bash")
        .expect("sends");
    let events = until_turns(&mut bus, session.id, 1).await;
    assert_eq!(counts_carried(&events), [counted(0, 1)]);
    core.session_send(session.id, "background agent left")
        .expect("sends");
    let events = until_turns(&mut bus, session.id, 1).await;
    assert_eq!(counts_carried(&events), [counted(1, 1)]);
    // Not a status: the session is idle, and takes a message.
    let read = core.session(session.id).expect("reads");
    assert_eq!(read.status, SessionStatus::Idle);
    // Read again from the database, it is what was published, here and in another Swarmed.
    assert_eq!(read.background, counted(1, 1));
    assert_eq!(counted_in_tree(&core, session.id), counted(1, 1));
    let other =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    assert_eq!(counted_in_tree(&other, session.id), counted(1, 1));

    // The agent's end takes everything it left with it.
    core.session_stop(session.id);
    let exited = exited(&mut bus, session.id).await;
    assert_eq!(exited.background, Some(counted(0, 0)));
    assert_eq!(counted_in_tree(&core, session.id), counted(0, 0));
    assert_eq!(counted_in_tree(&other, session.id), counted(0, 0));
}

#[tokio::test]
async fn a_swarmed_that_starts_records_the_end_of_what_was_left_in_the_background() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "background agent left")
        .expect("sends");
    until_turns(&mut bus, session.id, 1).await;
    let read = core.session(session.id).expect("reads");
    assert_eq!(read.background, counted(1, 0));
    // Swarmed ends with the sub-agent at work: nothing says that it, or its agent, ended.
    drop(core);

    let later =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let ends = |core: &Core| {
        let events = core.session_events(session.id).expect("reads");
        let ended = |event: &&AgentEvent| matches!(event, AgentEvent::Exited { .. });
        events.iter().filter(ended).count()
    };
    // The first core's hold on being first is let go a moment after it is dropped.
    let mut tries = 0;
    while ends(&later) == 0 && tries < 200 {
        later.arrive().expect("arrives");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        tries += 1;
    }
    let events = later.session_events(session.id).expect("reads");
    let last = events.last().expect("recorded");
    assert!(
        matches!(last, AgentEvent::Exited { code: None, .. }),
        "{last:?}"
    );
    // Once: the end is there now.
    later.arrive().expect("arrives");
    assert_eq!(ends(&later), 1);
    // And the session no longer counts what went with its agent.
    let read = later.session(session.id).expect("reads");
    assert_eq!(read.background, counted(0, 0));
}

/// Skips ahead to the session's call of tool `id`.
async fn called(bus: &mut Receiver<SessionEvent>, session: i64, call: &str) {
    loop {
        let published = next(bus).await;
        let called = matches!(&published.event, AgentEvent::ToolUse { id, .. } if id == call);
        if published.session_id == session && called {
            return;
        }
    }
}

#[tokio::test]
async fn a_command_a_call_waits_on_goes_to_the_background_and_the_turn_goes_on() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "foreground bash")
        .expect("sends");
    called(&mut bus, session.id, "tool-fg-bash").await;
    // Waited on, it is no work left running.
    let listed = core.task_background("demo", "one").expect("lists");
    assert!(listed.is_empty(), "{listed:?}");

    let sent = core.session_send_to_background(session.id, Some("tool-fg-bash"));
    assert!(sent.await.expect("answered"), "it went");
    // The turn goes on and ends, and the command does not hold back its answer.
    let events = until_turns(&mut bus, session.id, 1).await;
    let ends = turn_ends(&events);
    assert_eq!(ends[0].status, Some(SessionStatus::Idle));
    assert_eq!(ends[0].unread, Some(true));
    let listed = core.task_background("demo", "one").expect("lists");
    let moved = listed.iter().find(|task| task.task_id == "bg-fg-bash");
    let moved = moved.expect("listed once it moved");
    assert_eq!(moved.state, BackgroundState::Running);
    assert_eq!(moved.tool_use_id.as_deref(), Some("tool-fg-bash"));
    assert!(moved.output);

    // Asked again, it is in the background already: it does not go, and nothing breaks.
    let again = core.session_send_to_background(session.id, Some("tool-fg-bash"));
    assert!(!again.await.expect("answered"));
    // The answers are the asker's alone: neither kept nor published.
    let stored = core.session_events(session.id).expect("reads");
    let answer = |event: &AgentEvent| matches!(event, AgentEvent::SentToBackground { .. });
    assert!(!stored.iter().any(answer));
    assert!(!events.iter().any(|published| answer(&published.event)));
}

#[tokio::test]
async fn a_sub_agent_sent_to_the_background_mid_turn_holds_back_what_it_hands_back() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "foreground agent")
        .expect("sends");
    called(&mut bus, session.id, "tool-fg-agent").await;
    // Every call is sent, as Ctrl+B sends them.
    let sent = core.session_send_to_background(session.id, None).await;
    assert!(sent.expect("answered"));
    let events = until_turns(&mut bus, session.id, 2).await;
    let ends = turn_ends(&events);
    // The turn that sent it there ends to wait, as one that started it there does.
    assert_eq!(ends[0].status, Some(SessionStatus::Idle));
    assert_eq!(ends[0].unread, None);
    assert_eq!(core.notice_for(ends[0]).expect("reads"), None);
    // Its report, read in a turn of the agent's own, hands something back.
    assert_eq!(ends[1].unread, Some(true));
    let listed = core.task_background("demo", "one").expect("lists");
    let ended = listed.iter().find(|task| task.task_id == "bg-fg-agent");
    let ended = ended.expect("listed once it moved");
    assert_eq!(ended.state, BackgroundState::Completed);
    assert_eq!(ended.kind, "local_agent");
}

#[tokio::test]
async fn only_a_running_claude_code_is_asked_to_send_work_to_the_background() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    let idle = core.session_send_to_background(session.id, Some("toolu_1"));
    assert!(
        matches!(idle.await, Err(Error::SessionNotRunning(_))),
        "nothing runs to ask"
    );
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    // Between turns no call waits: a call's work does not go, and every call's is taken.
    let one = core.session_send_to_background(session.id, Some("toolu_1"));
    assert!(!one.await.expect("answered"));
    let every = core.session_send_to_background(session.id, None);
    assert!(every.await.expect("answered"));
    let blank = core.session_send_to_background(session.id, Some(" "));
    assert!(matches!(blank.await, Err(Error::NotSentToBackground(_))));

    let other = AgentInfo {
        id: "other".into(),
        name: "Other".into(),
        models: vec![],
        default_model: None,
        hears_mid_turn: false,
        sends_to_background: false,
    };
    core.add_agent(other, Arc::new(FakeBackend::default()));
    let options = SessionOptions {
        agent: Some("other".into()),
        ..SessionOptions::default()
    };
    let elsewhere = core.session_new("demo", "one", &options).expect("creates");
    core.session_start(elsewhere.id, Auth::Subscription)
        .expect("starts");
    let refused = core.session_send_to_background(elsewhere.id, None).await;
    assert!(
        matches!(&refused, Err(Error::NoSendingToBackground(agent)) if agent == "other"),
        "{refused:?}"
    );
}

#[tokio::test]
async fn what_a_background_command_printed_is_read_only_by_its_own_session() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);
    let other = started_session(&core);

    core.session_send(session.id, "background bash")
        .expect("sends");
    until_turns(&mut bus, session.id, 1).await;
    let listed = core.task_background("demo", "one").expect("lists");
    assert!(
        listed
            .iter()
            .any(|task| task.task_id == "bg-bash" && task.output)
    );
    // Its own session finds the file gone, which is no error: Claude Code clears its folder.
    let own = core.session_background_output(session.id, "bg-bash", None);
    assert!(matches!(own, Ok(None)), "{own:?}");
    // Another session cannot reach it, nor a task nobody named.
    let theirs = core.session_background_output(other.id, "bg-bash", None);
    assert!(
        matches!(theirs, Err(Error::BackgroundOutput(_))),
        "{theirs:?}"
    );
    let nobody = core.session_background_output(session.id, "nothing", None);
    assert!(
        matches!(nobody, Err(Error::BackgroundOutput(_))),
        "{nobody:?}"
    );
}

#[tokio::test]
async fn a_message_sent_now_stops_the_turn_and_goes_ahead_of_those_waiting() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    core.session_send(session.id, "slow").expect("sends");
    core.session_enqueue(session.id, "later").expect("queues");
    core.session_message_now(session.id, "now", &Attached::default(), Auth::default())
        .expect("sends now");
    let stopped = turn(&mut bus, session.id).await;
    assert!(
        stopped
            .iter()
            .any(|event| matches!(event, AgentEvent::TurnDone { is_error: true, .. })),
        "{stopped:?}"
    );
    let now = turn(&mut bus, session.id).await;
    assert!(now.contains(&text("echo: now")), "{now:?}");
    let later = turn(&mut bus, session.id).await;
    assert!(later.contains(&text("echo: later")), "{later:?}");

    // With nothing under way it goes at once.
    core.session_message_now(session.id, "at once", &Attached::default(), Auth::default())
        .expect("sends");
    let at_once = turn(&mut bus, session.id).await;
    assert!(at_once.contains(&text("echo: at once")), "{at_once:?}");
    // Nothing, and a command Swarmed does not send on, are refused.
    let empty = core.session_message_now(session.id, " ", &Attached::default(), Auth::default());
    assert!(matches!(empty, Err(Error::EmptyMessage)), "{empty:?}");
    let login =
        core.session_message_now(session.id, "/login", &Attached::default(), Auth::default());
    assert!(
        matches!(login, Err(Error::TerminalOnly { .. })),
        "{login:?}"
    );
}

#[tokio::test]
async fn a_waiting_message_told_now_is_taken_into_the_turn_and_recorded_where_it_was_heard() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    // A request stays open while the message is told: nothing takes it away.
    core.session_send(session.id, "run: ls").expect("sends");
    let (request_id, _) = permission_request(&mut bus).await;
    let queue = core.session_enqueue(session.id, "a hint").expect("queues");
    core.session_tell_now(session.id, queue[0].id)
        .expect("tells");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);
    core.session_allow(session.id, &request_id).expect("sends");
    let events = turn(&mut bus, session.id).await;
    let at = |wanted: &AgentEvent| events.iter().position(|event| event == wanted);
    let message = AgentEvent::UserMessage {
        text: "a hint".to_string(),
        remote: false,
        at_reset: false,
        from_session: None,
    };
    let result = events
        .iter()
        .position(|event| matches!(event, AgentEvent::ToolResult { .. }));
    // Recorded once, after the tool it waited behind, and answered in the same turn.
    assert!(at(&message) > result, "{events:?}");
    assert_eq!(events.iter().filter(|event| **event == message).count(), 1);
    assert!(events.contains(&text("heard: a hint")), "{events:?}");
    let stored = core.session_events(session.id).expect("reads");
    assert!(
        !stored
            .iter()
            .any(|event| matches!(event, AgentEvent::Heard { .. }))
    );

    // Between turns it goes as any message does.
    let queue = core.session_enqueue(session.id, "next").expect("queues");
    core.session_tell_now(session.id, queue[0].id)
        .expect("sends");
    let next = turn(&mut bus, session.id).await;
    assert!(next.contains(&text("echo: next")), "{next:?}");
}

#[tokio::test]
async fn a_message_told_to_an_agent_that_ended_before_hearing_it_waits_again() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let session = started_session(&core);

    // The agent waits on a request, so it has not taken the message in when it is stopped.
    core.session_send(session.id, "run: ls").expect("sends");
    permission_request(&mut bus).await;
    core.session_enqueue(session.id, "first hint")
        .expect("queues");
    let queue = core
        .session_enqueue(session.id, "second hint")
        .expect("queues");
    for message in &queue {
        core.session_tell_now(session.id, message.id)
            .expect("tells");
    }
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);
    core.session_stop(session.id);
    while !matches!(next(&mut bus).await.event, AgentEvent::Exited { .. }) {}

    // Not shown as said: the agent never got them. They wait, in the order they were told.
    let waiting: Vec<String> = core
        .session_queue(session.id)
        .expect("reads")
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert_eq!(waiting, ["first hint", "second hint"]);
    let stored = core.session_events(session.id).expect("reads");
    let said = |event: &AgentEvent| matches!(event, AgentEvent::UserMessage { text, .. } if text.ends_with("hint"));
    assert!(!stored.iter().any(said), "{stored:?}");
}

#[tokio::test]
async fn configuration_swarmed_cannot_understand_stops_no_agent_and_is_named() {
    let fx = fixture();
    // A key a newer Swarmed may write, in the repository's own configuration and its hooks.
    common::write_config(
        &fx,
        "max_stacks = 3\nlater = true\n[hooks]\nwarmup = \"x\"\n",
    );
    let core = core_with_task(&fx).await;
    let data = &fx.paths.data_dir;
    let write = |name: &str, text: &str| std::fs::write(data.join(name), text).expect("writes");
    write("settings.toml", "editor = \"zed\"\ncoming_later = 1\n");
    write("agents.toml", "[other]\ncomand = \"typo\"\n");
    write(
        "mcp.toml",
        "[docs]\ncommand = \"docs\"\nenabled = true\ntimeout = 3\n",
    );
    write("skills.toml", "hidden = [\"own/one\"\n");

    // The agent starts and answers: none of it stood in its way.
    let mut bus = core.subscribe();
    let session = started_session(&core);
    core.session_send(session.id, "hello").expect("sends");
    let events = turn(&mut bus, session.id).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::TurnDone { .. }))
    );

    let problems = core.config_problems().expect("reads");
    let named = |file: &str, key: &str| {
        problems
            .iter()
            .any(|problem| problem.starts_with(file) && problem.contains(key))
    };
    assert!(named("settings.toml: ", "`coming_later`"), "{problems:?}");
    assert!(
        named("agents.toml: ", "`other` could not be read"),
        "{problems:?}"
    );
    assert!(named("mcp.toml: ", "`docs.timeout`"), "{problems:?}");
    assert!(named("skills.toml: ", "not TOML"), "{problems:?}");
    assert!(named("projects/demo.toml: ", "`later`"), "{problems:?}");
    assert!(
        named("projects/demo.toml: ", "`hooks.warmup`"),
        "{problems:?}"
    );
    assert_eq!(problems.len(), 6, "{problems:?}");
}
