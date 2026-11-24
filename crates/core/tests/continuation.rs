//! A session a usage limit stopped, going on by itself when the limit resets, against the
//! scripted fake agent: set by the user or by the setting, cancelled by whatever says the user
//! is back, kept across a restart, and sent by the Swarmed that keeps house alone.

mod common;

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{core_with_project, fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth, FakeBackend};
use swarmed_core::{CONTINUE_MESSAGE, Core, Error, NewTaskOptions, SessionOptions, hooks};

async fn core_with_task(fx: &common::Fixture) -> Core {
    let core = core_with_project(fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core
}

/// A session of the task with its agent running.
fn started(core: &Core) -> i64 {
    let session = core.session_new("demo", "one", &SessionOptions::default());
    let id = session.expect("test setup").id;
    core.session_start(id, Auth::default()).expect("starts");
    id
}

fn now() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    i64::try_from(since.expect("after 1970").as_secs()).expect("fits")
}

/// Sends `text` and waits for the end of its turn.
async fn said(core: &Core, id: i64, text: &str) -> Vec<AgentEvent> {
    let mut bus = core.subscribe();
    core.session_send(id, text).expect("sends");
    turn(&mut bus, id).await
}

fn waits(core: &Core, id: i64) -> Option<i64> {
    core.session(id).expect("exists").continue_at
}

/// Calls [`Core::continue_due`] until it continues a session, for at most five seconds.
async fn continued(core: &Core) -> Option<i64> {
    for _ in 0..50 {
        if let Some(id) = core.continue_due().expect("looks") {
            return Some(id);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
}

fn sent_at_reset(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::UserMessage { text, at_reset: true, .. } if text == CONTINUE_MESSAGE
    )
}

#[tokio::test]
async fn a_refused_turn_goes_on_by_itself_when_its_limit_resets() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    core.arrive().expect("arrives");
    let id = started(&core);
    let before = now();
    said(&core, id, "refuse 1").await;

    let at = core
        .session_continue_at_reset(id)
        .expect("a limit stopped it");
    assert!(
        (before + 1..=now() + 1).contains(&at),
        "{at} is not its reset"
    );
    assert_eq!(waits(&core, id), Some(at));
    let events = core.session_events(id).expect("reads");
    assert_eq!(
        events.last(),
        Some(&AgentEvent::Continuation { at: Some(at) })
    );

    let mut bus = core.subscribe();
    assert_eq!(continued(&core).await, Some(id));
    let events = turn(&mut bus, id).await;
    assert!(events.iter().any(sent_at_reset), "{events:?}");
    assert!(events.contains(&AgentEvent::Text {
        text: format!("echo: {CONTINUE_MESSAGE}")
    }));
    assert_eq!(waits(&core, id), None);
    // It went once: nothing is due any more.
    assert_eq!(core.continue_due().expect("looks"), None);
}

#[tokio::test]
async fn a_reset_that_has_passed_sets_nothing_and_nor_does_a_turn_that_went_well() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut settings = core.settings().expect("reads");
    settings.continue_after_limit = true;
    core.set_settings(&settings).expect("saves");
    let id = started(&core);

    said(&core, id, "refuse -5").await;
    assert_eq!(waits(&core, id), None);
    let refused = core.session_continue_at_reset(id);
    assert!(matches!(refused, Err(Error::NoLimitToWaitFor(of)) if of == id));

    said(&core, id, "refuse 60").await;
    assert!(waits(&core, id).is_some());
    // Written to, it goes on now; that turn went well, and there is nothing to wait for.
    said(&core, id, "hello").await;
    assert_eq!(waits(&core, id), None);
    let refused = core.session_continue_at_reset(id);
    assert!(matches!(refused, Err(Error::NoLimitToWaitFor(_))));
}

#[tokio::test]
async fn the_setting_continues_every_refused_turn_without_asking() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let id = started(&core);
    said(&core, id, "refuse 60").await;
    assert_eq!(waits(&core, id), None, "off by default");

    let mut settings = core.settings().expect("reads");
    settings.continue_after_limit = true;
    core.set_settings(&settings).expect("saves");
    let before = now();
    said(&core, id, "refuse 60").await;
    let at = waits(&core, id).expect("set by itself");
    assert!((before + 60..=now() + 60).contains(&at));
}

#[tokio::test]
async fn stop_closing_and_cancelling_take_it_off() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let id = started(&core);
    said(&core, id, "refuse 60").await;

    core.session_continue_at_reset(id).expect("sets");
    core.session_continue_cancel(id).expect("cancels");
    assert_eq!(waits(&core, id), None);
    let events = core.session_events(id).expect("reads");
    assert_eq!(events.last(), Some(&AgentEvent::Continuation { at: None }));

    core.session_continue_at_reset(id).expect("sets");
    core.session_interrupt(id).expect("stops");
    assert_eq!(waits(&core, id), None);

    core.session_continue_at_reset(id).expect("sets");
    core.session_close(id).expect("closes");
    assert_eq!(waits(&core, id), None);
    let refused = core.session_continue_at_reset(id);
    assert!(matches!(refused, Err(Error::SessionClosed(_))));
}

#[tokio::test]
async fn it_outlives_a_restart_and_only_the_swarmed_that_keeps_house_sends_it() {
    let fx = fixture();
    let first = core_with_task(&fx).await;
    let id = started(&first);
    said(&first, id, "refuse 1").await;
    let at = first.session_continue_at_reset(id).expect("sets");
    drop(first);
    while now() < at {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let again =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    assert_eq!(waits(&again, id), Some(at));
    // Not the first to arrive: it sends nothing.
    assert_eq!(again.continue_due().expect("looks"), None);
    assert_eq!(waits(&again, id), Some(at));

    again.arrive().expect("arrives");
    let mut bus = again.subscribe();
    assert_eq!(again.continue_due().expect("looks"), Some(id));
    let events = turn(&mut bus, id).await;
    assert!(events.iter().any(sent_at_reset), "{events:?}");
}

#[tokio::test]
async fn sessions_one_limit_stopped_go_on_one_at_a_time() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    core.arrive().expect("arrives");
    let one = started(&core);
    let two = started(&core);
    said(&core, one, "refuse 1").await;
    said(&core, two, "refuse 1").await;
    core.session_continue_at_reset(one).expect("sets");
    core.session_continue_at_reset(two).expect("sets");

    let first = continued(&core).await;
    assert_eq!(first, Some(one));
    assert!(waits(&core, two).is_some(), "the second waits its turn");
    assert_eq!(continued(&core).await, Some(two));
}

#[tokio::test]
async fn what_waited_in_the_queue_goes_after_the_message_sent_at_the_reset() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    core.arrive().expect("arrives");
    let id = started(&core);
    said(&core, id, "refuse 1").await;
    core.session_continue_at_reset(id).expect("sets");
    // Queued, not written to the session: the user is not back, and it waits.
    core.session_enqueue(id, "and then this").expect("queues");

    let mut bus = core.subscribe();
    assert_eq!(continued(&core).await, Some(id));
    let first = turn(&mut bus, id).await;
    assert!(first.iter().any(sent_at_reset), "{first:?}");
    let second = turn(&mut bus, id).await;
    assert!(second.contains(&AgentEvent::Text {
        text: "echo: and then this".to_string()
    }));
}
