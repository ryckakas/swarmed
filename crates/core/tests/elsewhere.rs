//! What another Swarmed process on the same data writes, as one core sees another's writes:
//! two cores on one data directory, as the app and the command line are.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Auth, BackgroundState, FakeBackend};
use swarmed_core::{
    Action, Attached, BackgroundCount, Core, Error, NewTaskOptions, RemoveOptions, Session,
    SessionEvent, SessionOptions, SessionStatus, hooks,
};
use tokio::sync::broadcast::Receiver;

/// The first core, with a task, and a second core on the same data.
async fn two_cores(fx: &common::Fixture) -> (Core, Core) {
    let here = core_with_project(fx).await;
    here.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let there =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    (here, there)
}

fn new_session(core: &Core) -> Session {
    core.session_new("demo", "one", &SessionOptions::default())
        .expect("creates")
}

/// Everything published so far, without waiting for more.
fn heard(bus: &mut Receiver<SessionEvent>) -> Vec<SessionEvent> {
    let mut heard = vec![];
    while let Ok(event) = bus.try_recv() {
        heard.push(event);
    }
    heard
}

/// Looks at what the other core wrote until `found` is among what was published, and returns
/// all that was.
async fn noticed(
    core: &Core,
    bus: &mut Receiver<SessionEvent>,
    found: impl Fn(&SessionEvent) -> bool,
) -> Vec<SessionEvent> {
    let mut seen = vec![];
    for _ in 0..200 {
        core.notice_others().expect("looks");
        seen.extend(heard(bus));
        if seen.iter().any(&found) {
            return seen;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("not published: {seen:?}");
}

fn tree_changed(event: &SessionEvent) -> bool {
    event.session_id == 0 && event.event == AgentEvent::TreeChanged
}

#[tokio::test]
async fn a_rename_a_mark_and_a_new_session_from_elsewhere_are_published_here() {
    let fx = fixture();
    let (here, there) = two_cores(&fx).await;
    let session = new_session(&here);
    // The first look takes stock.
    here.notice_others().expect("looks");
    let mut bus = here.subscribe();

    there
        .session_rename(session.id, "Named elsewhere")
        .expect("renames");
    let seen = noticed(
        &here,
        &mut bus,
        |event| matches!(&event.event, AgentEvent::Renamed { title } if title == "Named elsewhere"),
    )
    .await;
    let renamed = seen.iter().find(|event| event.session_id == session.id);
    assert!(renamed.is_some_and(|event| event.elsewhere), "{seen:?}");
    assert!(seen.iter().any(tree_changed), "{seen:?}");

    there
        .session_mark_named(session.id, "green", "ready")
        .expect("marks");
    noticed(
        &here,
        &mut bus,
        |event| matches!(&event.event, AgentEvent::Marked { label, .. } if label == "ready"),
    )
    .await;

    // A highlight is in the tree, never in the conversation: the tree is read again.
    there
        .session_highlight(session.id, Some(swarmed_core::MarkColour::Yellow))
        .expect("highlights");
    noticed(&here, &mut bus, tree_changed).await;
    let lit = here.session(session.id).expect("found").highlight;
    assert_eq!(lit, Some(swarmed_core::MarkColour::Yellow));

    let made = new_session(&there);
    noticed(&here, &mut bus, tree_changed).await;
    let listed = here.sessions("demo", "one").expect("lists");
    assert!(listed.iter().any(|session| session.id == made.id));
}

#[tokio::test]
async fn nothing_this_core_wrote_is_published_twice() {
    let fx = fixture();
    let (here, there) = two_cores(&fx).await;
    let session = new_session(&here);
    here.notice_others().expect("looks");
    let mut bus = here.subscribe();

    here.session_rename(session.id, "Named here")
        .expect("renames");
    there
        .session_rename(session.id, "Named there")
        .expect("renames");
    here.session_rename(session.id, "Named here again")
        .expect("renames");
    let seen = noticed(
        &here,
        &mut bus,
        |event| matches!(&event.event, AgentEvent::Renamed { title } if title == "Named there"),
    )
    .await;
    here.notice_others().expect("looks again");
    let mut seen = seen;
    seen.extend(heard(&mut bus));
    let titles: Vec<&str> = seen
        .iter()
        .filter_map(|event| match &event.event {
            AgentEvent::Renamed { title } => Some(title.as_str()),
            _ => None,
        })
        .collect();
    // Each once, the other core's passed on after the two written here were published.
    assert_eq!(titles, ["Named here", "Named here again", "Named there"]);
}

#[tokio::test]
async fn a_turn_run_elsewhere_follows_here_with_its_status_and_notice() {
    let fx = fixture();
    let (here, there) = two_cores(&fx).await;
    // The app arrives first, and shows the notices of what the command line runs.
    here.arrive().expect("arrives");
    let session = new_session(&here);
    here.notice_others().expect("looks");
    let mut bus = here.subscribe();

    let mut theirs = there.subscribe();
    there
        .session_message(
            session.id,
            "hello",
            &Attached::default(),
            Auth::Subscription,
        )
        .expect("sends");
    turn(&mut theirs, session.id).await;
    let seen = noticed(&here, &mut bus, |event| {
        matches!(event.event, AgentEvent::TurnDone { .. })
    })
    .await;
    let asked = seen.iter().find(
        |event| matches!(&event.event, AgentEvent::UserMessage { text, .. } if text == "hello"),
    );
    assert_eq!(
        asked.and_then(|event| event.status),
        Some(SessionStatus::Working)
    );
    let said = seen
        .iter()
        .any(|event| matches!(&event.event, AgentEvent::Text { text } if text == "echo: hello"));
    assert!(said, "{seen:?}");
    let done = seen
        .iter()
        .find(|event| matches!(event.event, AgentEvent::TurnDone { .. }))
        .expect("the turn's end");
    assert_eq!(done.unread, Some(true), "{done:?}");
    let notice = here.notice_for(done).expect("reads");
    assert!(notice.is_some_and(|notice| notice.title.starts_with("Finished")));
    // A Swarmed that did not arrive first shows none of what it passes on: the first does.
    let second = there.notice_for(done).expect("reads");
    assert!(second.is_none(), "{second:?}");
}

#[tokio::test]
async fn what_a_session_run_elsewhere_left_in_the_background_counts_here() {
    let fx = fixture();
    let (here, there) = two_cores(&fx).await;
    let session = new_session(&here);
    here.notice_others().expect("looks");
    let mut bus = here.subscribe();

    let mut theirs = there.subscribe();
    let attached = Attached::default();
    there
        .session_message(session.id, "background bash", &attached, Auth::Subscription)
        .expect("sends");
    turn(&mut theirs, session.id).await;
    let left = BackgroundCount {
        agents: 0,
        commands: 1,
    };
    let seen = noticed(&here, &mut bus, |event| {
        matches!(event.event, AgentEvent::Background { .. })
    })
    .await;
    let report = seen
        .iter()
        .find(|event| matches!(event.event, AgentEvent::Background { .. }));
    assert_eq!(report.and_then(|event| event.background), Some(left));
    // It runs there, not stopped: the tree and the task's list here say so.
    assert_eq!(here.session(session.id).expect("reads").background, left);
    let listed = here.task_background("demo", "one").expect("lists");
    let running = listed
        .iter()
        .filter(|task| task.state == BackgroundState::Running);
    assert_eq!(running.count(), 1, "{listed:?}");
}

#[tokio::test]
async fn what_a_swarmed_that_shows_its_own_notices_runs_is_not_shown_here_too() {
    let fx = fixture();
    let (here, there) = two_cores(&fx).await;
    here.arrive().expect("arrives");
    // A second app: it shows the notices of the agents it runs.
    there.arrive().expect("arrives");
    let session = new_session(&here);
    here.notice_others().expect("looks");
    let mut bus = here.subscribe();
    there
        .session_message(
            session.id,
            "run: ls",
            &Attached::default(),
            Auth::Subscription,
        )
        .expect("sends");
    let seen = noticed(&here, &mut bus, |event| {
        matches!(event.event, AgentEvent::PermissionRequest { .. })
    })
    .await;
    let asked = seen
        .iter()
        .find(|event| matches!(event.event, AgentEvent::PermissionRequest { .. }))
        .expect("the request");
    assert_eq!(asked.status, Some(SessionStatus::Waiting));
    let notice = here.notice_for(asked).expect("reads");
    assert!(notice.is_none(), "{notice:?}");
}

#[tokio::test]
async fn a_session_run_elsewhere_is_not_started_here_and_its_queue_waits_for_that_run() {
    let fx = fixture();
    let (here, there) = two_cores(&fx).await;
    let session = new_session(&here);
    let mut theirs = there.subscribe();
    there
        .session_message(session.id, "slow", &Attached::default(), Auth::Subscription)
        .expect("sends");
    // Its turn is under way there.
    while !matches!(
        next(&mut theirs).await.event,
        AgentEvent::UserMessage { .. }
    ) {}

    let refused = here.session_start(session.id, Auth::Subscription);
    assert!(
        matches!(refused, Err(Error::RunsElsewhere(id)) if id == session.id),
        "{refused:?}"
    );
    let sent = here.session_message(session.id, "now", &Attached::default(), Auth::Subscription);
    assert!(matches!(sent, Err(Error::RunsElsewhere(_))), "{sent:?}");
    // Stopping is that Swarmed's: here nothing is put right that is not wrong.
    let stopped = here.session_interrupt(session.id);
    assert!(
        matches!(stopped, Err(Error::RunsElsewhere(_))),
        "{stopped:?}"
    );
    assert_eq!(
        here.session(session.id).expect("reads").status,
        SessionStatus::Working
    );
    // Nor is it put away or deleted here, nor its task or repository removed, even by force:
    // that agent would work on where no list shows it, or in a worktree that is gone.
    let closed = here.session_close(session.id);
    assert!(matches!(closed, Err(Error::RunsElsewhere(_))), "{closed:?}");
    let deleted = here.session_delete(session.id);
    assert!(
        matches!(deleted, Err(Error::RunsElsewhere(_))),
        "{deleted:?}"
    );
    let forced = RemoveOptions {
        force: true,
        delete_branch: false,
    };
    let removed = here
        .task_remove("demo", "one", forced, &hooks::discard)
        .await;
    assert!(
        matches!(removed, Err(Error::RunsElsewhere(_))),
        "{removed:?}"
    );
    let removed = here.project_remove("demo", true, &hooks::discard).await;
    assert!(
        matches!(removed, Err(Error::RunsElsewhere(_))),
        "{removed:?}"
    );
    let (_, task) = here.task("demo", "one").expect("still there");
    assert!(task.worktree_path.exists());
    assert!(here.session(session.id).is_ok());

    here.session_enqueue(session.id, "after that")
        .expect("queues");
    let mut bus = here.subscribe();
    here.notice_others().expect("looks");
    assert_eq!(here.running_sessions(), Vec::<i64>::new());

    there.session_stop(session.id);
    let seen = noticed(&here, &mut bus, |event| {
        matches!(&event.event, AgentEvent::UserMessage { text, .. } if text == "after that")
    })
    .await;
    let sent = seen.iter().find(|event| {
        matches!(&event.event, AgentEvent::UserMessage { text, .. } if text == "after that")
    });
    // Sent here, by this core's own agent now.
    assert!(sent.is_some_and(|event| !event.elsewhere), "{seen:?}");
    assert_eq!(here.running_sessions(), [session.id]);
    let refused = there.session_start(session.id, Auth::Subscription);
    assert!(
        matches!(refused, Err(Error::RunsElsewhere(_))),
        "{refused:?}"
    );
}

#[tokio::test]
async fn an_action_added_elsewhere_has_front_ends_read_the_configuration_again() {
    let fx = fixture();
    let (here, there) = two_cores(&fx).await;
    here.notice_others().expect("looks");
    let mut bus = here.subscribe();

    let action = Action {
        name: "Test".to_string(),
        icon: String::new(),
        prompt: "Run the tests".to_string(),
        command: String::new(),
        of_project: false,
    };
    there.action_save(None, &action).expect("saves");
    let seen = noticed(&here, &mut bus, |event| {
        event.session_id == 0
            && matches!(&event.event, AgentEvent::ConfigChanged { kind } if kind == "file")
    })
    .await;
    assert!(!seen.iter().any(tree_changed), "{seen:?}");
    let actions = here.actions(None).expect("reads");
    assert!(actions.iter().any(|action| action.name == "Test"));
}

#[tokio::test]
async fn a_history_read_while_another_core_writes_holds_each_event_once_with_what_follows() {
    let fx = fixture();
    let (here, there) = two_cores(&fx).await;
    let session = new_session(&here);
    here.notice_others().expect("looks");
    let mut bus = here.subscribe();
    // Stored there, and no look here has passed it on yet.
    let mut theirs = there.subscribe();
    there
        .session_message(
            session.id,
            "hello",
            &Attached::default(),
            Auth::Subscription,
        )
        .expect("sends");
    turn(&mut theirs, session.id).await;

    let history = here.session_events(session.id).expect("reads");
    let echo = AgentEvent::Text {
        text: "echo: hello".to_string(),
    };
    assert!(history.contains(&echo), "{history:?}");
    // Published before the history was read, where a front end reading it hears that it has
    // to read again; nothing of it comes after.
    let before = heard(&mut bus);
    assert!(before.iter().any(|event| event.event == echo), "{before:?}");
    here.notice_others().expect("looks");
    let after: Vec<SessionEvent> = heard(&mut bus)
        .into_iter()
        .filter(|event| event.session_id == session.id)
        .collect();
    assert_eq!(after, vec![], "published again after the read");

    // What is stored after the read is published once, and adds to what was read.
    there
        .session_message(
            session.id,
            "again",
            &Attached::default(),
            Auth::Subscription,
        )
        .expect("sends");
    turn(&mut theirs, session.id).await;
    let seen = noticed(
        &here,
        &mut bus,
        |event| matches!(&event.event, AgentEvent::Text { text } if text == "echo: again"),
    )
    .await;
    let again = seen
        .iter()
        .filter(|event| matches!(&event.event, AgentEvent::Text { text } if text == "echo: again"));
    assert_eq!(again.count(), 1, "{seen:?}");
    let echoed = seen.iter().any(|event| event.event == echo);
    assert!(!echoed, "{seen:?}");
}

#[tokio::test]
async fn the_app_left_over_shows_the_notices_once_the_first_has_quit() {
    let fx = fixture();
    let (first, second) = two_cores(&fx).await;
    first.arrive().expect("arrives");
    second.arrive().expect("arrives");
    let command =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let session = new_session(&first);
    second.notice_others().expect("looks");
    drop(first);
    // The first has quit: the second takes its place at its next look.
    second.notice_others().expect("looks");

    let mut bus = second.subscribe();
    let mut theirs = command.subscribe();
    command
        .session_message(
            session.id,
            "hello",
            &Attached::default(),
            Auth::Subscription,
        )
        .expect("sends");
    turn(&mut theirs, session.id).await;
    let seen = noticed(&second, &mut bus, |event| {
        matches!(event.event, AgentEvent::TurnDone { .. })
    })
    .await;
    let done = seen
        .iter()
        .find(|event| matches!(event.event, AgentEvent::TurnDone { .. }))
        .expect("the turn's end");
    let notice = second.notice_for(done).expect("reads");
    assert!(notice.is_some_and(|notice| notice.title.starts_with("Finished")));
}

/// Starts a turn that does not end, and waits until its agent has said all it says before it
/// is stopped: within one process an agent outlives its core for a moment, and what it stored
/// after that would make its session busy again, as a Swarmed that really ended could not.
async fn busy_session(core: &Core) -> Session {
    let session = new_session(core);
    let mut bus = core.subscribe();
    core.session_message(session.id, "slow", &Attached::default(), Auth::Subscription)
        .expect("sends");
    loop {
        let event = next(&mut bus).await;
        if event.session_id == session.id && matches!(event.event, AgentEvent::Text { .. }) {
            return session;
        }
    }
}

#[tokio::test]
async fn the_app_left_over_settles_what_the_first_left_busy_and_not_what_it_runs() {
    let fx = fixture();
    let (first, second) = two_cores(&fx).await;
    first.arrive().expect("arrives");
    second.arrive().expect("arrives");
    // Each app is in the middle of a turn of its own.
    let theirs = busy_session(&first).await;
    let own = busy_session(&second).await;
    second.notice_others().expect("looks");
    let status = |id: i64| second.session(id).expect("reads").status;
    assert_eq!(status(theirs.id), SessionStatus::Working);

    // The first ends without a word, as one that crashed does: its session stays busy.
    drop(first);
    let mut bus = second.subscribe();
    // The second takes the first's place at a look, and tells front ends to read the tree.
    noticed(&second, &mut bus, tree_changed).await;
    assert_eq!(status(theirs.id), SessionStatus::Idle);
    assert_eq!(status(own.id), SessionStatus::Working);
}
