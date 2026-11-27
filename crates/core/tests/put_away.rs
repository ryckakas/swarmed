//! Putting finished tasks away, by hand and by the user's rules: what it stops and keeps, what
//! brings a task back, and that a rule never puts away a task that is at work.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{core_with_project, fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth, FakeBackend};
use swarmed_core::{
    CHATS, Core, DoneWhy, Error, NewTaskOptions, Session, SessionOptions, Settings, Task, hooks,
};

async fn core_with_task(fx: &common::Fixture) -> Core {
    let core = core_with_project(fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core
}

fn task(core: &Core) -> Task {
    core.task("demo", "one").expect("there").1
}

/// A new session of task `one`, its agent running, after one turn.
async fn worked_session(core: &Core) -> Session {
    let mut bus = core.subscribe();
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("test setup");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    core.session_send(session.id, "hello").expect("sends");
    turn(&mut bus, session.id).await;
    session
}

fn settings(change: impl FnOnce(&mut Settings)) -> Settings {
    let mut settings = Settings::default();
    change(&mut settings);
    settings
}

#[tokio::test]
async fn a_task_put_away_stops_its_agents_keeps_its_sessions_and_comes_back() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = worked_session(&core).await;
    assert!(core.running_sessions().contains(&session.id));
    let mut bus = core.subscribe();

    let notes = core
        .task_done("demo", "one", false, &hooks::discard)
        .await
        .expect("puts away");
    assert!(notes.is_empty(), "{notes:?}");
    let done = task(&core);
    assert!(done.done_at.is_some());
    assert_eq!(done.done_why, Some(DoneWhy::Hand));
    // The agent is stopped; the session, its conversation, the worktree and the branch stay.
    assert!(!core.running_sessions().contains(&session.id));
    let open = core.sessions("demo", "one").expect("lists");
    assert_eq!(open.iter().map(|s| s.id).collect::<Vec<_>>(), [session.id]);
    assert!(done.worktree_path.exists());
    let published = common::next(&mut bus).await;
    assert!(
        std::iter::once(published.event)
            .chain(std::iter::from_fn(|| bus.try_recv().ok().map(|e| e.event)))
            .any(|event| matches!(event, AgentEvent::TreeChanged)),
        "front ends are told to read the tree again"
    );
    // Listed in the overview with when and why, and still a task of its repository.
    let overview = core.overview().expect("reads");
    let listed = &overview[0].tasks[0].task;
    assert_eq!(listed.done_why, Some(DoneWhy::Hand));
    assert_eq!(core.tasks("demo", false).expect("lists").len(), 1);

    // Putting away what is done changes nothing.
    core.task_done("demo", "one", false, &hooks::discard)
        .await
        .expect("done already");
    assert_eq!(task(&core).done_at, done.done_at);

    core.task_undone("demo", "one").expect("brings back");
    let back = task(&core);
    assert_eq!((back.done_at, back.done_why), (None, None));
}

#[tokio::test]
async fn closing_the_sessions_is_a_setting_and_reopening_one_brings_the_task_back() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = worked_session(&core).await;
    core.set_settings(&settings(|s| s.put_away_closes_sessions = true))
        .expect("saves");

    core.task_done("demo", "one", false, &hooks::discard)
        .await
        .expect("puts away");
    assert_eq!(core.sessions("demo", "one").expect("lists"), []);
    assert_eq!(core.sessions_closed("demo", "one").expect("lists").len(), 1);
    assert!(task(&core).done_at.is_some());

    core.session_reopen(session.id).expect("reopens");
    assert_eq!(task(&core).done_at, None);
}

#[tokio::test]
async fn starting_one_of_its_agents_brings_a_task_back() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let session = worked_session(&core).await;
    core.task_done("demo", "one", false, &hooks::discard)
        .await
        .expect("puts away");
    assert!(task(&core).done_at.is_some());

    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    assert_eq!(task(&core).done_at, None);
}

#[tokio::test]
async fn its_stack_is_stopped_and_starting_it_brings_the_task_back() {
    let fx = fixture();
    common::write_config(&fx, "[hooks]\nstart = \"sleep 30\"\n");
    let core = core_with_task(&fx).await;
    assert!(core.stack_start("demo", "one").expect("starts"));
    assert!(core.stack_is_running("demo", "one").expect("asks"));

    core.task_done("demo", "one", false, &hooks::discard)
        .await
        .expect("puts away");
    assert!(!core.stack_is_running("demo", "one").expect("asks"));

    assert!(core.stack_start("demo", "one").expect("starts"));
    assert_eq!(task(&core).done_at, None);
    core.stack_stop("demo", "one", &hooks::discard)
        .await
        .expect("stops");
}

#[tokio::test]
async fn a_chat_is_not_put_away() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    core.task_new(CHATS, "talk", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("makes a chat");
    let refused = core.task_done(CHATS, "talk", false, &hooks::discard).await;
    assert!(matches!(refused, Err(Error::NoRepository)), "{refused:?}");
}

/// A `gh` that says the pull request of every branch merged long ago.
const GH: &str = r#"#!/bin/sh
cat "$(dirname "$0")/view.json"
"#;

const MERGED: &str = r#"{"number": 7, "title": "feat: one", "url": "https://github.com/o/r/pull/7",
    "state": "MERGED", "isDraft": false, "reviewDecision": "", "mergeable": "UNKNOWN",
    "headRefName": "BRANCH", "headRefOid": "abc", "updatedAt": "2026-01-02T00:00:00Z",
    "mergedAt": "2026-01-02T00:00:00Z", "statusCheckRollup": []}"#;

fn gh_says_merged(data: &Path, branch: &str) -> PathBuf {
    let tools = data.join("tools");
    std::fs::create_dir_all(&tools).expect("dir");
    common::executable(&tools.join("gh"), GH);
    std::fs::write(tools.join("view.json"), MERGED.replace("BRANCH", branch)).expect("writes");
    tools
}

/// A core that keeps house, with task `one` whose pull request merged long ago, as it read it.
async fn house_with_merged_task(fx: &common::Fixture) -> Core {
    let mut core =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    core.arrive().expect("arrives");
    core.project_add(&fx.repo, None).await.expect("adds");
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("makes");
    core.set_tool_dirs(vec![gh_says_merged(&fx.paths.data_dir, &task.branch)]);
    let pull = core.task_pull_look("demo", "one").await.expect("reads");
    assert_eq!(
        pull.and_then(|pull| pull.merged_at).as_deref(),
        Some("2026-01-02T00:00:00Z")
    );
    core
}

#[tokio::test]
async fn nothing_is_put_away_by_itself_unless_a_rule_is_on() {
    let fx = fixture();
    let core = house_with_merged_task(&fx).await;
    assert_eq!(core.tasks_put_away_due().await.expect("looks"), []);
    assert_eq!(task(&core).done_at, None);
}

#[tokio::test]
async fn the_merged_rule_puts_a_task_away_once_and_not_again_after_it_came_back() {
    let fx = fixture();
    let core = house_with_merged_task(&fx).await;
    core.set_settings(&settings(|s| s.put_away_merged = true))
        .expect("saves");

    let put = core.tasks_put_away_due().await.expect("looks");
    assert_eq!(put, [("demo".into(), "one".into(), DoneWhy::Merged)]);
    assert_eq!(task(&core).done_why, Some(DoneWhy::Merged));

    // Brought back, it was kept on purpose: the same merge does not put it away again.
    core.task_undone("demo", "one").expect("brings back");
    assert_eq!(core.tasks_put_away_due().await.expect("looks"), []);
    assert_eq!(task(&core).done_at, None);
}

#[tokio::test]
async fn no_rule_puts_away_a_task_whose_agent_is_at_work() {
    let fx = fixture();
    let core = house_with_merged_task(&fx).await;
    core.set_settings(&settings(|s| s.put_away_merged = true))
        .expect("saves");
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("makes");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    // A turn that only ends when stopped.
    core.session_send(session.id, "slow").expect("sends");

    assert_eq!(core.tasks_put_away_due().await.expect("looks"), []);
    assert_eq!(task(&core).done_at, None);

    core.session_interrupt(session.id).expect("stops the turn");
    let mut waits = 0;
    while core.session(session.id).expect("there").status != swarmed_core::SessionStatus::Idle
        && waits < 200
    {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        waits += 1;
    }
    let put = core.tasks_put_away_due().await.expect("looks");
    assert_eq!(put.len(), 1, "once the turn is over it goes");
}

#[tokio::test]
async fn the_idle_rule_counts_from_the_last_thing_its_sessions_recorded() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    core.arrive().expect("arrives");
    worked_session(&core).await;
    core.set_settings(&settings(|s| {
        s.put_away_idle = true;
        s.put_away_idle_days = 3;
    }))
    .expect("saves");
    assert_eq!(core.tasks_put_away_due().await.expect("looks"), []);

    // As if the task were made, and its session last spoke, four days ago.
    let db = rusqlite::Connection::open(fx.paths.db_file()).expect("opens");
    db.execute_batch(
        "UPDATE tasks SET created_at = datetime('now', '-4 days'); \
         UPDATE session_events SET created_at = datetime('now', '-4 days');",
    )
    .expect("backdates");
    let put = core.tasks_put_away_due().await.expect("looks");
    assert_eq!(put, [("demo".into(), "one".into(), DoneWhy::Idle)]);

    // Brought back by hand, it counts from then.
    core.task_undone("demo", "one").expect("brings back");
    assert_eq!(core.tasks_put_away_due().await.expect("looks"), []);
}
