//! Snoozing a session until a time: what is kept and published, what refuses it, what wakes it
//! and what does not, and that the core counts a snoozed session as the sidebar draws it.

mod common;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Auth, FakeBackend};
use swarmed_core::{
    Core, Delivery, DoneWhy, Error, NewTaskOptions, Session, SessionEvent, SessionOptions,
    Settings, hooks,
};
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
        .expect("test setup")
}

/// A new session with its agent running, after one turn.
async fn worked_session(core: &Core) -> Session {
    let mut bus = core.subscribe();
    let session = new_session(core);
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;
    session
}

/// Now, in seconds since 1970.
fn now() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after 1970");
    i64::try_from(since.as_secs()).expect("fits")
}

/// An hour from now.
fn in_an_hour() -> i64 {
    now() + 3600
}

fn snoozed(core: &Core, id: i64) -> Option<i64> {
    core.session(id).expect("there").snoozed_until
}

/// The next event published for session `id` that `wanted` picks.
async fn published(
    bus: &mut Receiver<SessionEvent>,
    id: i64,
    wanted: impl Fn(&AgentEvent) -> bool,
) -> SessionEvent {
    loop {
        let event = next(bus).await;
        if event.session_id == id && wanted(&event.event) {
            return event;
        }
    }
}

fn ids(core: &Core) -> Vec<i64> {
    let sessions = core.sessions("demo", "one").expect("lists");
    sessions.iter().map(|session| session.id).collect()
}

/// A Swarmed started again on the data `fx` holds, after the one before quit.
fn core_again(fx: &common::Fixture) -> Core {
    Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens")
}

#[tokio::test]
async fn a_snooze_is_kept_published_apart_and_outlives_a_restart() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = new_session(&core);
    let mut bus = core.subscribe();
    let until = in_an_hour();

    let asleep = core
        .session_snooze(session.id, Some(until))
        .expect("snoozes");
    assert_eq!(asleep.snoozed_until, Some(until));
    let told = next(&mut bus).await;
    assert_eq!(
        (told.session_id, told.event),
        (0, AgentEvent::TreeChanged),
        "the tree is read again; nothing is said in the conversation"
    );
    assert_eq!(core.session_events(session.id).expect("reads"), []);
    // Still among its task's open sessions: front ends decide what is out of sight.
    assert_eq!(ids(&core), [session.id]);
    drop(core);

    let again = core_again(&fx);
    assert_eq!(snoozed(&again, session.id), Some(until));
    let awake = again.session_snooze(session.id, None).expect("wakes");
    assert_eq!(awake.snoozed_until, None);
    // Waking one that is awake changes nothing.
    again.session_snooze(session.id, None).expect("wakes");
}

#[tokio::test]
async fn a_time_that_has_come_a_closed_session_and_none_are_refused() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = new_session(&core);
    for until in [now() - 60, now()] {
        let refused = core.session_snooze(session.id, Some(until));
        assert!(
            matches!(refused, Err(Error::SnoozePast { id, .. }) if id == session.id),
            "{refused:?}"
        );
    }
    assert_eq!(snoozed(&core, session.id), None);
    core.session_close(session.id, false).expect("closes");
    let closed = core.session_snooze(session.id, Some(in_an_hour()));
    assert!(matches!(closed, Err(Error::SessionClosed(_))), "{closed:?}");
    let missing = core.session_snooze(999, Some(in_an_hour()));
    assert!(
        matches!(missing, Err(Error::SessionNotFound(999))),
        "{missing:?}"
    );
}

#[tokio::test]
async fn writing_to_a_snoozed_session_wakes_it() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = worked_session(&core).await;
    let mut bus = core.subscribe();
    core.session_snooze(session.id, Some(in_an_hour()))
        .expect("snoozes");
    core.session_send(session.id, "and now?").expect("sends");
    turn(&mut bus, session.id).await;
    assert_eq!(snoozed(&core, session.id), None);
}

/// Whether `events` begin with a message session `from` left.
fn left_by(events: &[AgentEvent], from: i64) -> bool {
    matches!(
        events.first(),
        Some(AgentEvent::UserMessage { from_session: Some(by), .. }) if *by == from
    )
}

#[tokio::test]
async fn a_message_another_session_leaves_does_not_wake_it_sent_or_queued() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let worker = worked_session(&core).await;
    let planner = worked_session(&core).await;
    let mut bus = core.subscribe();
    let until = in_an_hour();
    core.session_snooze(planner.id, Some(until))
        .expect("snoozes");

    // Idle: it goes at once, recorded as the worker's, and the planner sleeps on.
    let sent = core.peer_message(worker.id, planner.id, "done with the tests");
    assert_eq!(sent.expect("delivers"), Delivery::Sent);
    let events = turn(&mut bus, planner.id).await;
    assert!(left_by(&events, worker.id), "{events:?}");
    assert_eq!(snoozed(&core, planner.id), Some(until));

    // Busy: it waits with who left it, and goes as the worker's once the turn is stopped.
    core.session_send(planner.id, "slow").expect("sends");
    published(&mut bus, planner.id, |event| {
        matches!(event, AgentEvent::Text { .. })
    })
    .await;
    core.session_snooze(planner.id, Some(until))
        .expect("snoozes");
    let queued = core.peer_message(worker.id, planner.id, "and the docs");
    assert_eq!(queued.expect("queues"), Delivery::Queued);
    let waiting = core.session_queue(planner.id).expect("reads");
    let senders: Vec<Option<i64>> = waiting.iter().map(|message| message.from_session).collect();
    assert_eq!(senders, [Some(worker.id)]);
    core.session_interrupt(planner.id).expect("stops the turn");
    turn(&mut bus, planner.id).await;
    let events = turn(&mut bus, planner.id).await;
    assert!(left_by(&events, worker.id), "{events:?}");
    assert_eq!(snoozed(&core, planner.id), Some(until));
}

#[tokio::test]
async fn a_request_is_told_and_a_turn_that_ends_well_keeps_the_snooze_and_its_notice() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = worked_session(&core).await;
    let mut bus = core.subscribe();
    core.session_send(session.id, "run: ls").expect("sends");
    let request = published(&mut bus, session.id, |event| {
        matches!(event, AgentEvent::PermissionRequest { .. })
    })
    .await;
    let AgentEvent::PermissionRequest { request_id, .. } = &request.event else {
        unreachable!("picked for it");
    };

    // Snoozed while it waits: what needs the user is told all the same.
    let until = in_an_hour();
    core.session_snooze(session.id, Some(until))
        .expect("snoozes");
    let needs = core.notice_for(&request).expect("decides");
    assert!(
        needs.is_some_and(|notice| notice.title.starts_with("Needs you")),
        "a request is told"
    );

    core.session_allow(session.id, request_id).expect("allows");
    let done = published(&mut bus, session.id, |event| {
        matches!(event, AgentEvent::TurnDone { .. })
    })
    .await;
    assert_eq!(done.unread, Some(true));
    assert_eq!(snoozed(&core, session.id), Some(until), "it stays asleep");
    assert_eq!(core.notice_for(&done).expect("decides"), None);

    // Awake, the same turn is worth telling.
    core.session_snooze(session.id, None).expect("wakes");
    let finished = core.notice_for(&done).expect("decides");
    assert!(
        finished.is_some_and(|notice| notice.title.starts_with("Finished")),
        "told once awake"
    );
}

#[tokio::test]
async fn closing_wakes_and_a_fork_starts_awake() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = worked_session(&core).await;
    core.session_snooze(session.id, Some(in_an_hour()))
        .expect("snoozes");

    let fork = core.session_fork(session.id, None, None).expect("forks");
    assert_eq!(fork.snoozed_until, None);
    assert_eq!(snoozed(&core, fork.id), None);

    core.session_close(session.id, false).expect("closes");
    assert_eq!(snoozed(&core, session.id), None);
    core.session_reopen(session.id).expect("reopens");
    assert_eq!(snoozed(&core, session.id), None, "reopened to be seen");
}

#[tokio::test]
async fn a_move_counts_the_siblings_drawn_and_passes_over_a_snoozed_one() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let [a, b, c, d] = [(); 4].map(|()| new_session(&core).id);
    core.session_set_parent(d, Some(b)).expect("works for b");
    // Under b, d has no sibling: a move takes it nowhere.
    core.session_move(d, -1).expect("moves");
    assert_eq!(ids(&core), [a, b, c, d]);

    // With b snoozed, d is drawn among the task's others, and b among none of them.
    core.session_snooze(b, Some(in_an_hour())).expect("snoozes");
    core.session_move(d, -1).expect("moves");
    assert_eq!(ids(&core), [a, b, d, c]);
    core.session_move(a, 1).expect("moves");
    assert_eq!(ids(&core), [b, d, a, c]);

    // Snoozed, it is still moved and pinned among the siblings drawn.
    core.session_move(b, 1).expect("moves");
    assert_eq!(ids(&core), [d, b, a, c]);
    core.session_pin(b, true).expect("pins");
    assert!(core.session(b).expect("there").pinned);
    assert_eq!(ids(&core), [b, d, a, c]);
}

#[tokio::test]
async fn a_snoozed_session_keeps_its_idle_task_from_being_put_away() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    core.arrive().expect("arrives");
    let session = worked_session(&core).await;
    let settings = Settings {
        put_away_idle: true,
        put_away_idle_days: 3,
        ..Settings::default()
    };
    core.set_settings(&settings).expect("saves");
    // As if the task were made, and its session last spoke, four days ago.
    let db = rusqlite::Connection::open(fx.paths.db_file()).expect("opens");
    db.execute_batch(
        "UPDATE tasks SET created_at = datetime('now', '-4 days'); \
         UPDATE session_events SET created_at = datetime('now', '-4 days');",
    )
    .expect("backdates");

    core.session_snooze(session.id, Some(in_an_hour()))
        .expect("snoozes");
    assert_eq!(core.tasks_put_away_due().await.expect("looks"), []);

    core.session_snooze(session.id, None).expect("wakes");
    let put = core.tasks_put_away_due().await.expect("looks");
    assert_eq!(put, [("demo".into(), "one".into(), DoneWhy::Idle)]);
}
