//! Remote Control against the scripted fake agent: turning it on and off, what is typed on
//! claude.ai, and attaching to the same session there again when the agent starts again.

mod common;

use std::sync::Arc;

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Auth, FakeBackend, RemoteState};
use swarmed_core::{
    AgentInfo, Core, Error, NewTaskOptions, SessionEvent, SessionOptions, SessionStatus, hooks,
};
use tokio::sync::broadcast::Receiver;

async fn core_with_task(fx: &common::Fixture) -> Core {
    let core = core_with_project(fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core
}

fn new_session(core: &Core) -> i64 {
    let session = core.session_new("demo", "one", &SessionOptions::default());
    session.expect("test setup").id
}

/// Skips ahead to the session's next report of Remote Control, and gives how it stands, the
/// session on claude.ai and its address.
async fn remote(
    bus: &mut Receiver<SessionEvent>,
    id: i64,
) -> (RemoteState, Option<String>, Option<String>) {
    loop {
        let published = next(bus).await;
        if published.session_id != id {
            continue;
        }
        if let AgentEvent::RemoteControl {
            state,
            remote_id,
            url,
            ..
        } = published.event
        {
            return (state, remote_id, url);
        }
    }
}

/// How Remote Control stood, each time it changed, as the session recorded it.
fn recorded(core: &Core, id: i64) -> Vec<RemoteState> {
    let events = core.session_events(id).expect("reads");
    let states = events.into_iter().filter_map(|event| match event {
        AgentEvent::RemoteControl { state, .. } => Some(state),
        _ => None,
    });
    states.collect()
}

/// The user's messages as recorded, each with whether it came through Remote Control.
fn messages(core: &Core, id: i64) -> Vec<(String, bool)> {
    let events = core.session_events(id).expect("reads");
    let said = events.into_iter().filter_map(|event| match event {
        AgentEvent::UserMessage { text, remote } => Some((text, remote)),
        _ => None,
    });
    said.collect()
}

#[tokio::test]
async fn what_is_typed_on_claude_ai_is_recorded_as_the_users_and_answered_here() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let id = new_session(&core);
    let mut bus = core.subscribe();
    core.session_remote_control(id, true, Auth::Subscription)
        .expect("turns it on");
    // Asked for first, then where the session is on claude.ai.
    assert_eq!(remote(&mut bus, id).await.0, RemoteState::Starting);
    let (state, remote_id, url) = remote(&mut bus, id).await;
    assert_eq!(state, RemoteState::On);
    assert_eq!(remote_id.as_deref(), Some("remote-1"));
    assert_eq!(url.as_deref(), Some("https://claude.ai/code/remote-1"));
    // On already: asking again changes nothing.
    core.session_remote_control(id, true, Auth::Subscription)
        .expect("is on");
    assert_eq!(
        recorded(&core, id),
        [RemoteState::Starting, RemoteState::On]
    );

    // Said back as it is taken in, a message sent from here is recorded once.
    core.session_send(id, "hello").expect("sends");
    turn(&mut bus, id).await;
    // One typed on claude.ai is answered in a turn of its own, which makes the session busy.
    core.session_send(id, "remote: from my phone")
        .expect("sends");
    turn(&mut bus, id).await;
    let afar = turn(&mut bus, id).await;
    assert_eq!(
        afar.first(),
        Some(&AgentEvent::UserMessage {
            text: "from my phone".into(),
            remote: true,
        })
    );
    assert_eq!(
        messages(&core, id),
        [
            ("hello".to_string(), false),
            ("remote: from my phone".to_string(), false),
            ("from my phone".to_string(), true),
        ]
    );
    assert_eq!(
        core.session(id).expect("exists").status,
        SessionStatus::Idle
    );
}

#[tokio::test]
async fn the_agent_attaches_to_the_same_session_again_when_it_starts_again_until_turned_off() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let id = new_session(&core);
    let mut bus = core.subscribe();
    core.session_remote_control(id, true, Auth::Subscription)
        .expect("turns it on");
    while remote(&mut bus, id).await.0 != RemoteState::On {}

    // The agent goes: it is only paused, still at the same session on claude.ai.
    core.session_stop(id);
    let (state, remote_id, _) = remote(&mut bus, id).await;
    assert_eq!(
        (state, remote_id.as_deref()),
        (RemoteState::Paused, Some("remote-1"))
    );
    // And comes back with the next message, attached to it again.
    core.session_start(id, Auth::Subscription).expect("starts");
    let (state, remote_id, _) = remote(&mut bus, id).await;
    assert_eq!(
        (state, remote_id.as_deref()),
        (RemoteState::Starting, Some("remote-1"))
    );
    let (state, remote_id, url) = remote(&mut bus, id).await;
    assert_eq!(state, RemoteState::On);
    assert_eq!(remote_id.as_deref(), Some("remote-1"));
    assert_eq!(url.as_deref(), Some("https://claude.ai/code/remote-1"));

    // Turned off, it stays off whatever the agent does, and nothing typed elsewhere comes.
    core.session_remote_control(id, false, Auth::Subscription)
        .expect("turns it off");
    let (state, remote_id, _) = remote(&mut bus, id).await;
    assert_eq!(
        (state, remote_id.as_deref()),
        (RemoteState::Off, Some("remote-1"))
    );
    core.session_stop(id);
    core.session_start(id, Auth::Subscription).expect("starts");
    core.session_send(id, "remote: not here").expect("sends");
    turn(&mut bus, id).await;
    assert_eq!(
        recorded(&core, id),
        [
            RemoteState::Starting,
            RemoteState::On,
            RemoteState::Paused,
            RemoteState::Starting,
            RemoteState::On,
            RemoteState::Off,
        ]
    );
    assert!(messages(&core, id).iter().all(|(_, remote)| !remote));

    // On again, it is the same session on claude.ai once more.
    core.session_remote_control(id, true, Auth::Subscription)
        .expect("turns it on");
    let (state, remote_id, _) = remote(&mut bus, id).await;
    assert_eq!(
        (state, remote_id.as_deref()),
        (RemoteState::Starting, Some("remote-1"))
    );
}

#[tokio::test]
async fn a_failure_ends_it_and_the_next_time_starts_a_new_session_on_claude_ai() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let id = new_session(&core);
    let mut bus = core.subscribe();
    // Remote Control takes a claude.ai sign-in, never an API key.
    let key = Auth::ApiKey("sk-test".into()); // gitleaks:allow
    core.session_remote_control(id, true, key).expect("asks");
    assert_eq!(remote(&mut bus, id).await.0, RemoteState::Starting);
    assert_eq!(remote(&mut bus, id).await.0, RemoteState::Failed);
    let events = core.session_events(id).expect("reads");
    let said = events.iter().find_map(|event| match event {
        AgentEvent::RemoteControl {
            state: RemoteState::Failed,
            detail,
            ..
        } => detail.clone(),
        _ => None,
    });
    assert_eq!(
        said.as_deref(),
        Some("Remote Control needs a claude.ai subscription")
    );
    // Not wanted any more: a new run of the agent does not try again by itself.
    core.session_stop(id);
    core.session_start(id, Auth::Subscription).expect("starts");
    core.session_send(id, "hello").expect("sends");
    turn(&mut bus, id).await;
    assert_eq!(
        recorded(&core, id),
        [RemoteState::Starting, RemoteState::Failed]
    );
}

#[tokio::test]
async fn closing_a_session_turns_it_off_and_other_agents_have_none() {
    let fx = fixture();
    let mut core = core_with_task(&fx).await;
    let id = new_session(&core);
    let mut bus = core.subscribe();
    core.session_remote_control(id, true, Auth::Subscription)
        .expect("turns it on");
    while remote(&mut bus, id).await.0 != RemoteState::On {}
    core.session_close(id).expect("closes");
    assert_eq!(remote(&mut bus, id).await.0, RemoteState::Off);
    // Closed, it is neither paused when its agent goes nor turned on again when reopened.
    core.session_reopen(id).expect("reopens");
    core.session_start(id, Auth::Subscription).expect("starts");
    core.session_send(id, "hello").expect("sends");
    turn(&mut bus, id).await;
    assert_eq!(
        recorded(&core, id),
        [RemoteState::Starting, RemoteState::On, RemoteState::Off]
    );
    // A closed session is not started for it.
    core.session_close(id).expect("closes");
    assert!(matches!(
        core.session_remote_control(id, true, Auth::Subscription),
        Err(Error::SessionClosed(_))
    ));

    let other = AgentInfo {
        id: "other".into(),
        name: "Other".into(),
        models: vec![],
        default_model: None,
        hears_mid_turn: false,
    };
    core.add_agent(other, Arc::new(FakeBackend::default()));
    let options = SessionOptions {
        agent: Some("other".into()),
        ..SessionOptions::default()
    };
    let elsewhere = core.session_new("demo", "one", &options).expect("creates");
    let refused = core.session_remote_control(elsewhere.id, true, Auth::Subscription);
    assert!(
        matches!(&refused, Err(Error::NoRemoteControl(agent)) if agent == "other"),
        "{refused:?}"
    );
    assert!(!core.running_sessions().contains(&elsewhere.id));
}

#[tokio::test]
async fn a_swarmed_that_starts_again_reaches_the_sessions_that_had_it_on() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let id = new_session(&core);
    let idle = new_session(&core);
    let mut bus = core.subscribe();
    core.session_remote_control(id, true, Auth::Subscription)
        .expect("turns it on");
    while remote(&mut bus, id).await.0 != RemoteState::On {}
    drop(core);

    let again =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    // Only the first Swarmed on the data starts agents for it: a second one beside it does not.
    let beside =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let mut bus = again.subscribe();
    assert_eq!(resumed_first(&again).await, 1);
    assert_eq!(
        beside
            .remote_controls_resume(&Auth::Subscription)
            .expect("resumes"),
        0
    );
    assert_eq!(again.running_sessions(), vec![id]);
    let (state, remote_id, _) = remote(&mut bus, id).await;
    assert_eq!(
        (state, remote_id.as_deref()),
        (RemoteState::Starting, Some("remote-1"))
    );
    assert_eq!(remote(&mut bus, id).await.0, RemoteState::On);
    assert!(!again.running_sessions().contains(&idle));
}

/// How many sessions a core starts again once it is the first on its data. The lock that
/// says so is let go when its file is closed everywhere, and a program another test is just
/// starting holds a copy of it for a moment, until it has become that program.
async fn resumed_first(core: &Core) -> usize {
    for _ in 0..200 {
        core.arrive().expect("arrives");
        let started = core
            .remote_controls_resume(&Auth::Subscription)
            .expect("resumes");
        if started > 0 {
            return started;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    0
}

#[tokio::test]
async fn rc_turns_it_on_and_off_at_once_and_never_waits_in_the_queue() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let id = new_session(&core);
    let mut bus = core.subscribe();
    core.session_message(id, "/rc", &[], Auth::Subscription)
        .expect("turns it on");
    while remote(&mut bus, id).await.0 != RemoteState::On {}
    // Nothing was said: it is Swarmed's to do, not the agent's.
    assert_eq!(messages(&core, id), Vec::<(String, bool)>::new());
    // Sent as typed from the queue, it would reach Claude Code as words it cannot act on.
    core.session_send(id, "slow").expect("sends");
    let waits = core.session_enqueue(id, "/rc off");
    assert!(
        matches!(&waits, Err(Error::TerminalOnly { command, .. }) if command == "rc"),
        "{waits:?}"
    );
    // While the agent works, it is turned off all the same.
    core.session_send(id, "/remote-control off")
        .expect("turns it off");
    assert_eq!(remote(&mut bus, id).await.0, RemoteState::Off);
    core.session_interrupt(id).expect("stops");
}
