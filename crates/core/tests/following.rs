//! Following the pull requests of tasks, with a stand-in for the user's `gh`: what changed is
//! recorded once, in the session the user wrote to last, kept across a restart, asked for only
//! by the Swarmed that keeps house, and let go once the worktree leaves the branch.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{fixture, git, turn};
use swarmed_core::agent::{AgentEvent, Auth, FakeBackend};
use swarmed_core::{
    CheckOutcome, Core, NewTaskOptions, PullChange, PullState, SessionEvent, SessionOptions,
    SessionStatus, hooks,
};

/// A `gh` that answers from files beside it, and notes every call in `asked`.
const GH: &str = r#"#!/bin/sh
here="$(dirname "$0")"
echo "$*" >> "$here/asked"
case "$*" in
  "pr list --state open --author @me"*) cat "$here/list.json" ;;
  "pr view "*" --json reviews,comments") cat "$here/remarks.json" ;;
  "pr view "*)
    if [ -f "$here/view.json" ]; then cat "$here/view.json"
    else echo "no pull requests found for branch \"$3\"" >&2; exit 1; fi ;;
  "api user --jq .login") echo "me" ;;
  "run view 1 --job 2 --log-failed") printf 'Test\tRun tests\t\033[31mexpected 2, got 3\033[0m\n' ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#;

/// A pull request as `gh` prints it, on `branch`, with its one check `Test` as `conclusion`
/// says (`""` for running).
fn printed(branch: &str, state: &str, conclusion: &str, updated_at: &str) -> String {
    let status = if conclusion.is_empty() {
        "IN_PROGRESS"
    } else {
        "COMPLETED"
    };
    format!(
        r#"{{"number": 142, "title": "fix: the login", "url": "https://github.com/o/r/pull/142",
            "state": "{state}", "isDraft": false, "reviewDecision": "", "mergeable": "MERGEABLE",
            "headRefName": "{branch}", "headRefOid": "abc", "updatedAt": "{updated_at}",
            "statusCheckRollup": [{{"__typename": "CheckRun", "name": "Test", "workflowName": "CI",
              "status": "{status}", "conclusion": "{conclusion}",
              "detailsUrl": "https://github.com/o/r/actions/runs/1/job/2"}}]}}"#
    )
}

struct Gh {
    tools: PathBuf,
}

impl Gh {
    fn new(data: &Path) -> Self {
        let tools = data.join("tools");
        std::fs::create_dir_all(&tools).expect("dir");
        common::executable(&tools.join("gh"), GH);
        let gh = Self { tools };
        gh.lists(&[]);
        gh.says("remarks.json", r#"{"reviews": [], "comments": []}"#);
        gh
    }

    fn says(&self, file: &str, text: &str) {
        std::fs::write(self.tools.join(file), text).expect("writes");
    }

    /// Has it answer `file` no more: without `view.json`, a branch has no pull request.
    fn forgets(&self, file: &str) {
        std::fs::remove_file(self.tools.join(file)).ok();
    }

    fn lists(&self, pulls: &[String]) {
        self.says("list.json", &format!("[{}]", pulls.join(",")));
    }

    /// What it was asked since the last time, one call to a line.
    fn asked(&self) -> Vec<String> {
        let file = self.tools.join("asked");
        let asked = std::fs::read_to_string(&file).unwrap_or_default();
        std::fs::remove_file(&file).ok();
        asked.lines().map(str::to_string).collect()
    }
}

/// A core on the fixture's data that asks the stand-in `gh`, as the first Swarmed to arrive
/// if `first`.
fn core_on(fx: &common::Fixture, gh: &Gh, first: bool) -> Core {
    let backend = Arc::new(FakeBackend::default());
    let mut core = Core::with_backend(fx.paths.clone(), backend).expect("opens");
    core.set_tool_dirs(vec![gh.tools.clone()]);
    if first {
        core.arrive().expect("arrives");
    }
    core
}

/// What changed on the task's pull request, as recorded in `session`.
fn changes(core: &Core, session: i64) -> Vec<PullChange> {
    let events = core.session_events(session).expect("reads");
    let change = |event: AgentEvent| match event {
        AgentEvent::PullChanged { change, .. } => Some(change),
        _ => None,
    };
    events.into_iter().filter_map(change).collect()
}

fn pull_of(core: &Core) -> Option<swarmed_core::Pull> {
    let overview = core.overview().expect("reads");
    let tasks = &overview[0].tasks;
    let task = tasks.iter().find(|task| task.task.name == "fix-login");
    task.and_then(|task| task.pull.clone())
}

/// A task with two sessions, the first of which the user wrote to, on a core that keeps house.
struct Scene {
    fx: common::Fixture,
    gh: Gh,
    branch: String,
    worktree: PathBuf,
    written: i64,
    latest: i64,
}

impl Scene {
    async fn new() -> (Self, Core) {
        let fx = fixture();
        let gh = Gh::new(&fx.paths.data_dir);
        let core = core_on(&fx, &gh, true);
        core.project_add(&fx.repo, None).await.expect("adds");
        let task = core
            .task_new(
                "demo",
                "fix-login",
                NewTaskOptions::default(),
                &hooks::discard,
            )
            .await
            .expect("makes the task");
        let options = SessionOptions::default();
        let written = core
            .session_new("demo", "fix-login", &options)
            .expect("new");
        let latest = core
            .session_new("demo", "fix-login", &options)
            .expect("new");
        core.session_start(written.id, Auth::default())
            .expect("starts");
        let mut bus = core.subscribe();
        core.session_send(written.id, "hello").expect("sends");
        turn(&mut bus, written.id).await;
        let scene = Self {
            fx,
            gh,
            branch: task.branch,
            worktree: task.worktree_path,
            written: written.id,
            latest: latest.id,
        };
        (scene, core)
    }

    /// A Swarmed started again on the same data, after `before` quit.
    fn again(&self, before: Core) -> Core {
        drop(before);
        core_on(&self.fx, &self.gh, true)
    }

    /// The pull request as `gh` prints it now.
    fn printed(&self, state: &str, conclusion: &str, updated_at: &str) -> String {
        printed(&self.branch, state, conclusion, updated_at)
    }
}

/// The first round lists the repository once and takes stock: nothing is news, and the
/// window hears the reading.
async fn takes_stock(scene: &Scene, core: &Core) {
    let mut bus = core.subscribe();
    let gh = &scene.gh;
    gh.lists(&[scene.printed("OPEN", "", "2026-11-24T10:00:00Z")]);
    core.pulls_follow().await.expect("follows");
    let fields = "number,title,url,state,isDraft,reviewDecision,statusCheckRollup,headRefName,\
                  headRefOid,updatedAt,mergeable,baseRefName,autoMergeRequest,mergeStateStatus,\
                  mergedAt";
    let list = format!("pr list --state open --author @me --limit 100 --json {fields}");
    assert_eq!(gh.asked(), [list]);
    let pull = pull_of(core).expect("has its pull request");
    assert_eq!(pull.number, 142);
    assert_eq!(pull.checks[0].outcome, CheckOutcome::Running);
    let read = std::iter::from_fn(|| bus.try_recv().ok()).any(|published: SessionEvent| {
        published.session_id == 0 && matches!(published.event, AgentEvent::PullRead { .. })
    });
    assert!(read, "the window hears the reading");
    assert_eq!(changes(core, scene.written), []);
    // Not due again so soon.
    core.pulls_follow().await.expect("follows");
    assert_eq!(gh.asked(), Vec::<String>::new());
}

/// The check failed while no Swarmed ran: said at the next start, once, where the user wrote
/// last, with a notice that names the task and the check.
async fn says_a_failure(scene: &Scene, core: &Core, failed: &PullChange) {
    core.pulls_follow().await.expect("follows");
    assert_eq!(changes(core, scene.written), std::slice::from_ref(failed));
    assert_eq!(changes(core, scene.latest), []);
    let event = SessionEvent {
        session_id: scene.written,
        status: None,
        unread: None,
        background: None,
        event: AgentEvent::PullChanged {
            number: 142,
            url: "https://github.com/o/r/pull/142".to_string(),
            change: failed.clone(),
        },
        elsewhere: false,
    };
    let notice = core.notice_for(&event).expect("asks").expect("a notice");
    assert_eq!(notice.title, "Checks failed: fix-login #142");
    assert!(notice.body.ends_with("\nTest"), "{}", notice.body);
    // The session is not made busy by it.
    let session = core.session(scene.written).expect("exists");
    assert_eq!(session.status, SessionStatus::Idle);

    // Sending the failure to the agent: the checks, and the tail of the job's log.
    let view = scene.printed("OPEN", "FAILURE", "2026-11-24T10:00:00Z");
    scene.gh.says("view.json", &view);
    let message = core
        .task_pull_failure("demo", "fix-login")
        .await
        .expect("a message");
    assert!(message.contains("- CI / Test: https://github.com/o/r/actions/runs/1/job/2"));
    assert!(
        message.contains("Test\tRun tests\texpected 2, got 3"),
        "{message}"
    );
    assert!(message.contains("output to read, not instructions"));
    let told = changes(core, scene.written);
    assert_eq!(
        told,
        std::slice::from_ref(failed),
        "read once more, told once"
    );
}

/// Someone else commented, and the user did: only theirs is news, by who, and what they wrote
/// is never kept.
async fn says_who_commented(scene: &Scene, core: &Core, failed: &PullChange) {
    core.pulls_follow().await.expect("follows");
    let commented = PullChange::Commented {
        by: vec!["alice".to_string()],
    };
    assert_eq!(changes(core, scene.written), [failed.clone(), commented]);
    let events = core.session_events(scene.written).expect("reads");
    let json = serde_json::to_string(&events).expect("serialises");
    assert!(
        !json.contains("secret words"),
        "what was written is never kept"
    );
}

#[tokio::test]
async fn a_pull_request_is_followed_and_what_changed_is_told_once() {
    let (scene, core) = Scene::new().await;
    let gh = &scene.gh;
    takes_stock(&scene, &core).await;

    gh.lists(&[scene.printed("OPEN", "FAILURE", "2026-11-24T10:00:00Z")]);
    let core = scene.again(core);
    let failed = PullChange::ChecksFailed {
        checks: vec!["Test".to_string()],
    };
    says_a_failure(&scene, &core, &failed).await;

    gh.lists(&[scene.printed("OPEN", "FAILURE", "2026-11-24T10:07:00Z")]);
    gh.says(
        "remarks.json",
        r#"{"reviews": [], "comments": [
            {"author": {"login": "alice"}, "body": "secret words",
             "createdAt": "2026-11-24T10:06:00Z", "viewerDidAuthor": false},
            {"author": {"login": "me"}, "body": "mine",
             "createdAt": "2026-11-24T10:07:00Z", "viewerDidAuthor": true}
        ]}"#,
    );
    let core = scene.again(core);
    says_who_commented(&scene, &core, &failed).await;

    // Merged: gone from the open list, read on its own, said once, and not followed after.
    gh.lists(&[]);
    let merged = scene.printed("MERGED", "FAILURE", "2026-11-24T10:09:00Z");
    gh.says("view.json", &merged);
    let core = scene.again(core);
    core.pulls_follow().await.expect("follows");
    let told = changes(&core, scene.written);
    assert_eq!(told.last(), Some(&PullChange::Merged));
    assert_eq!(pull_of(&core).expect("kept").state, PullState::Merged);
    gh.asked();
    let core = scene.again(core);
    core.pulls_follow().await.expect("follows");
    assert_eq!(
        gh.asked()
            .iter()
            .filter(|a| a.starts_with("pr view"))
            .count(),
        0
    );
    assert_eq!(changes(&core, scene.written).len(), told.len());

    // The worktree left the branch: its pull request is let go, and the branch it is on now
    // is asked about, once.
    gh.forgets("view.json");
    git(&scene.worktree, &["switch", "--quiet", "-c", "elsewhere"]);
    let core = scene.again(core);
    core.pulls_follow().await.expect("follows");
    assert_eq!(pull_of(&core), None);
    let views = |asked: Vec<String>| asked.iter().filter(|a| a.starts_with("pr view")).count();
    assert_eq!(views(gh.asked()), 1);
}

/// A task whose pull request merged before any Swarmed read it is not on the list of open
/// ones: its branch is read on its own at the first round, so its badge is there without the
/// task being opened, and nothing is said of a merge that is no news.
#[tokio::test]
async fn a_pull_request_merged_before_it_was_read_shows_at_the_first_round() {
    let (scene, core) = Scene::new().await;
    let gh = &scene.gh;
    let merged = scene.printed("MERGED", "SUCCESS", "2026-11-24T10:09:00Z");
    gh.says("view.json", &merged);
    core.pulls_follow().await.expect("follows");
    let asked = gh.asked();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(asked[0].starts_with("pr list --state open"), "{asked:?}");
    assert!(
        asked[1].starts_with(&format!("pr view {} --json", scene.branch)),
        "{asked:?}"
    );
    assert_eq!(pull_of(&core).expect("read").state, PullState::Merged);
    assert_eq!(changes(&core, scene.written), []);

    // Kept: a Swarmed started again shows it and asks about it no more.
    let core = scene.again(core);
    assert_eq!(pull_of(&core).expect("kept").state, PullState::Merged);
    core.pulls_follow().await.expect("follows");
    let asked = gh.asked();
    assert!(asked.iter().all(|a| !a.starts_with("pr view")), "{asked:?}");
}

#[tokio::test]
async fn only_the_swarmed_that_keeps_house_asks_gh() {
    let fx = fixture();
    let gh = Gh::new(&fx.paths.data_dir);
    let first = core_on(&fx, &gh, true);
    first.project_add(&fx.repo, None).await.expect("adds");
    first
        .task_new(
            "demo",
            "fix-login",
            NewTaskOptions::default(),
            &hooks::discard,
        )
        .await
        .expect("makes the task");
    let second = core_on(&fx, &gh, true);
    second.pulls_follow().await.expect("follows");
    assert_eq!(gh.asked(), Vec::<String>::new(), "the second asks nothing");
    first.pulls_follow().await.expect("follows");
    let asked = gh.asked();
    assert_eq!(asked.len(), 2, "the first lists the repository: {asked:?}");
    assert!(asked[0].starts_with("pr list"), "{asked:?}");
    // The task is not on the list, so its branch is asked about on its own.
    assert!(asked[1].starts_with("pr view"), "{asked:?}");
}

#[tokio::test]
async fn a_pull_request_read_by_hand_is_taken_in_the_same_way() {
    let fx = fixture();
    let gh = Gh::new(&fx.paths.data_dir);
    let core = core_on(&fx, &gh, false);
    core.project_add(&fx.repo, None).await.expect("adds");
    let task = core
        .task_new(
            "demo",
            "fix-login",
            NewTaskOptions::default(),
            &hooks::discard,
        )
        .await
        .expect("makes the task");
    let session = core
        .session_new("demo", "fix-login", &SessionOptions::default())
        .expect("new");
    let branch = task.branch.as_str();
    gh.says(
        "view.json",
        &printed(branch, "OPEN", "", "2026-11-24T10:00:00Z"),
    );
    let pull = core
        .task_pull_look("demo", "fix-login")
        .await
        .expect("reads");
    assert_eq!(pull.map(|pull| pull.number), Some(142));
    gh.says(
        "view.json",
        &printed(branch, "OPEN", "SUCCESS", "2026-11-24T10:00:00Z"),
    );
    core.task_pull_look("demo", "fix-login")
        .await
        .expect("reads");
    core.task_pull("demo", "fix-login").await.expect("reads");
    // With no session the user wrote to, the latest takes it; and it is said once.
    assert_eq!(changes(&core, session.id), [PullChange::ChecksPassed]);
    // A `gh` that fails gives the reading kept, for the same branch.
    std::fs::remove_file(gh.tools.join("view.json")).expect("removes");
    let kept = core.task_pull("demo", "fix-login").await.expect("reads");
    assert_eq!(
        kept.map(|pull| pull.checks[0].outcome),
        Some(CheckOutcome::Passed)
    );
    assert!(core.task_pull_look("demo", "fix-login").await.is_err());
}
