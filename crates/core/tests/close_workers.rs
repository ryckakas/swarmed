//! Closing a session with the sessions that work for it, all the way down the tree and across
//! tasks, and putting a task away with the sessions of other tasks that work for its own (#470).

mod common;

use std::sync::Arc;

use common::{core_with_project, fixture, next};
use swarmed_core::agent::{AgentEvent, Auth, FakeBackend};
use swarmed_core::{
    Attached, Core, Descendant, Error, NewTaskOptions, RemoveOptions, Session, SessionOptions,
    SessionStatus, hooks,
};

/// No session at all.
const NONE: [i64; 0] = [];

/// A core with a repository `demo` and three tasks in it, `one`, `two` and `three`.
async fn core_with_tasks(fx: &common::Fixture) -> Core {
    let core = core_with_project(fx).await;
    for name in ["one", "two", "three"] {
        core.task_new("demo", name, NewTaskOptions::default(), &hooks::discard)
            .await
            .expect("test setup");
    }
    core
}

fn new_session(core: &Core, task: &str) -> Session {
    core.session_new("demo", task, &SessionOptions::default())
        .expect("creates")
}

/// A new session of `task` that works for `parent`.
fn worker(core: &Core, task: &str, parent: i64) -> Session {
    let session = new_session(core, task);
    core.session_set_parent(session.id, Some(parent))
        .expect("works for it")
}

/// The sessions of a tree, in tasks `one` and `two`: an architect over two planners, one in
/// each task, the one in `two` over an implementer in `one`, and that one over a reviewer.
struct Tree {
    architect: Session,
    planner_one: Session,
    planner_two: Session,
    implementer: Session,
    reviewer: Session,
}

fn tree(core: &Core) -> Tree {
    let architect = new_session(core, "one");
    let planner_two = worker(core, "two", architect.id);
    let planner_one = worker(core, "one", architect.id);
    let implementer = worker(core, "one", planner_two.id);
    let reviewer = worker(core, "two", implementer.id);
    Tree {
        architect,
        planner_one,
        planner_two,
        implementer,
        reviewer,
    }
}

/// Each listed session's id with its depth.
fn listed(descendants: &[Descendant]) -> Vec<(i64, usize)> {
    descendants
        .iter()
        .map(|one| (one.session.id, one.depth))
        .collect()
}

/// The ids of the open sessions of `task`.
fn open_in(core: &Core, task: &str) -> Vec<i64> {
    let sessions = core.sessions("demo", task).expect("lists");
    sessions.iter().map(|session| session.id).collect()
}

/// How many `Closed` events session `id` recorded.
fn closings(core: &Core, id: i64) -> usize {
    let events = core.session_events(id).expect("reads");
    let closed = |event: &&AgentEvent| matches!(event, AgentEvent::Closed);
    events.iter().filter(closed).count()
}

/// Starts a turn of session `id` that does not end until it is stopped, and waits until it is
/// under way.
async fn at_work(core: &Core, id: i64) {
    let mut bus = core.subscribe();
    core.session_message(id, "slow", &Attached::default(), Auth::Subscription)
        .expect("sends");
    loop {
        let event = next(&mut bus).await;
        if event.session_id == id && matches!(event.event, AgentEvent::Text { .. }) {
            return;
        }
    }
}

#[tokio::test]
async fn every_session_under_one_is_listed_at_every_depth_and_in_any_task() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let t = tree(&core);

    // Each right after the one it works for, the oldest first among those of one.
    let under = core
        .session_descendants(t.architect.id, false)
        .expect("lists");
    assert_eq!(
        listed(&under),
        [
            (t.planner_two.id, 1),
            (t.implementer.id, 2),
            (t.reviewer.id, 3),
            (t.planner_one.id, 1),
        ]
    );
    assert!(under.iter().all(|one| !one.closed));
    let below = core
        .session_descendants(t.reviewer.id, false)
        .expect("lists");
    assert!(below.is_empty(), "{below:?}");

    // A closed one is listed only when asked for; those under it are still under the top.
    core.session_close(t.planner_two.id, false).expect("closes");
    let open = core
        .session_descendants(t.architect.id, false)
        .expect("lists");
    assert_eq!(
        listed(&open),
        [
            (t.implementer.id, 2),
            (t.reviewer.id, 3),
            (t.planner_one.id, 1)
        ]
    );
    let all = core
        .session_descendants(t.architect.id, true)
        .expect("lists");
    let closed: Vec<(i64, bool)> = all.iter().map(|one| (one.session.id, one.closed)).collect();
    assert_eq!(
        closed,
        [
            (t.planner_two.id, true),
            (t.implementer.id, false),
            (t.reviewer.id, false),
            (t.planner_one.id, false),
        ]
    );

    let missing = core.session_descendants(404, false);
    assert!(
        matches!(missing, Err(Error::SessionNotFound(404))),
        "{missing:?}"
    );
}

#[tokio::test]
async fn the_sessions_of_a_removed_task_are_not_listed() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let architect = new_session(&core, "one");
    let gone = worker(&core, "three", architect.id);
    let kept = worker(&core, "two", gone.id);
    let forced = RemoveOptions {
        force: true,
        delete_branch: false,
    };
    core.task_remove("demo", "three", forced, &hooks::discard)
        .await
        .expect("removes");

    let under = core.session_descendants(architect.id, true).expect("lists");
    assert_eq!(listed(&under), [(kept.id, 2)]);
}

#[tokio::test]
async fn closing_a_session_alone_leaves_those_that_work_for_it_open() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let t = tree(&core);

    let closed = core.session_close(t.architect.id, false).expect("closes");
    assert_eq!(closed, [t.architect.id]);
    assert_eq!(open_in(&core, "one"), [t.planner_one.id, t.implementer.id]);
    assert_eq!(open_in(&core, "two"), [t.planner_two.id, t.reviewer.id]);
    // They still work for it.
    assert_eq!(
        core.session(t.planner_one.id).expect("reads").parent,
        Some(t.architect.id)
    );
}

#[tokio::test]
async fn closing_with_its_workers_closes_the_whole_branch_the_deepest_first() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let t = tree(&core);
    let beside = new_session(&core, "two");
    at_work(&core, t.implementer.id).await;
    assert!(core.running_sessions().contains(&t.implementer.id));

    let closed = core.session_close(t.architect.id, true).expect("closes");
    assert_eq!(
        closed,
        [
            t.reviewer.id,
            t.implementer.id,
            t.planner_two.id,
            t.planner_one.id,
            t.architect.id,
        ]
    );
    assert_eq!(open_in(&core, "one"), NONE);
    assert_eq!(open_in(&core, "two"), [beside.id]);
    for id in &closed {
        assert_eq!(closings(&core, *id), 1, "session {id} recorded its closing");
    }
    // The one at work was stopped mid-turn, as closing it alone stops it.
    assert!(!core.running_sessions().contains(&t.implementer.id));
    assert_eq!(
        core.session(t.implementer.id).expect("reads").status,
        SessionStatus::Idle
    );

    // Reopening the top reopens only it.
    core.session_reopen(t.architect.id).expect("reopens");
    assert_eq!(open_in(&core, "one"), [t.architect.id]);
    assert_eq!(open_in(&core, "two"), [beside.id]);
    let under = core
        .session_descendants(t.architect.id, true)
        .expect("lists");
    assert!(under.iter().all(|one| one.closed), "{under:?}");
}

#[tokio::test]
async fn one_run_by_another_swarmed_refuses_the_whole_close_and_nothing_is_closed() {
    let fx = fixture();
    let here = core_with_tasks(&fx).await;
    let there =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let t = tree(&here);
    at_work(&there, t.reviewer.id).await;

    let refused = here.session_close(t.architect.id, true);
    assert!(
        matches!(refused, Err(Error::RunsElsewhere(id)) if id == t.reviewer.id),
        "{refused:?}"
    );
    assert_eq!(
        open_in(&here, "one"),
        [t.architect.id, t.planner_one.id, t.implementer.id]
    );
    assert_eq!(open_in(&here, "two"), [t.planner_two.id, t.reviewer.id]);
    assert_eq!(closings(&here, t.architect.id), 0);

    // Alone, it closes: the one run elsewhere is not among what it closes.
    here.session_close(t.architect.id, false).expect("closes");
    there.session_stop(t.reviewer.id);
}

#[tokio::test]
async fn putting_a_task_away_closes_the_sessions_of_other_tasks_that_work_for_it_only_when_asked() {
    let fx = fixture();
    let core = core_with_tasks(&fx).await;
    let t = tree(&core);
    let elsewhere = worker(&core, "three", t.planner_one.id);

    // Those of other tasks, however far down, each once: the implementer in `one` is the
    // task's own, and the reviewer under it in `two` is not.
    let listed_elsewhere = core
        .task_descendants_elsewhere("demo", "one")
        .expect("lists");
    let mut ids: Vec<i64> = listed_elsewhere.iter().map(|one| one.session.id).collect();
    ids.sort_unstable();
    let mut expected = vec![t.planner_two.id, t.reviewer.id, elsewhere.id];
    expected.sort_unstable();
    assert_eq!(ids, expected);

    // Not asked, they stay open, and none is named.
    let done = core
        .task_done("demo", "one", false, &hooks::discard)
        .await
        .expect("puts away");
    assert_eq!(done.workers_closed, Vec::<i64>::new());
    assert_eq!(open_in(&core, "two"), [t.planner_two.id, t.reviewer.id]);
    assert_eq!(open_in(&core, "three"), [elsewhere.id]);

    // Put away already, asking closes nothing, and says it closed nothing.
    let again = core
        .task_done("demo", "one", true, &hooks::discard)
        .await
        .expect("left as it is");
    assert_eq!(again.workers_closed, Vec::<i64>::new());
    assert_eq!(open_in(&core, "two"), [t.planner_two.id, t.reviewer.id]);
    assert_eq!(open_in(&core, "three"), [elsewhere.id]);
    core.task_undone("demo", "one").expect("brings back");

    // Asked, they are closed and their tasks stay; the task's own follow the settings, which
    // keep them open. Each closed is named, the deepest first.
    let done = core
        .task_done("demo", "one", true, &hooks::discard)
        .await
        .expect("puts away");
    let mut closed = done.workers_closed.clone();
    closed.sort_unstable();
    assert_eq!(closed, expected);
    assert_eq!(done.workers_closed.first(), Some(&t.reviewer.id));
    assert_eq!(open_in(&core, "two"), NONE);
    assert_eq!(open_in(&core, "three"), NONE);
    assert_eq!(
        open_in(&core, "one"),
        [t.architect.id, t.planner_one.id, t.implementer.id]
    );
    for task in ["two", "three"] {
        let (_, kept) = core.task("demo", task).expect("there");
        assert!(kept.done_at.is_none(), "task {task} is not put away");
    }
    assert!(core.task("demo", "one").expect("there").1.done_at.is_some());
}

#[tokio::test]
async fn putting_a_task_away_with_its_workers_is_refused_whole_while_one_runs_elsewhere() {
    let fx = fixture();
    let here = core_with_tasks(&fx).await;
    let there =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let architect = new_session(&here, "one");
    let planner = worker(&here, "two", architect.id);
    at_work(&there, planner.id).await;

    let refused = here.task_done("demo", "one", true, &hooks::discard).await;
    assert!(
        matches!(refused, Err(Error::RunsElsewhere(id)) if id == planner.id),
        "{refused:?}"
    );
    assert!(here.task("demo", "one").expect("there").1.done_at.is_none());
    assert_eq!(open_in(&here, "two"), [planner.id]);
    there.session_stop(planner.id);
}
