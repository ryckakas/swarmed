//! Merging a task's pull request, with a stand-in for the user's `gh`: only the head the user
//! saw is merged, no branch is deleted, GitHub's refusals reach the user in its words, a merge
//! queue is named as one, and auto-merge is set and taken back.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::fixture;
use swarmed_core::agent::{AgentEvent, FakeBackend};
use swarmed_core::{
    Core, MergeMethod, MergeOutcome, MergeWhen, NewTaskOptions, PullChange, PullState,
    SessionOptions, hooks,
};

/// A `gh` that answers from files beside it and notes every call in `asked`. A merge leaves
/// the pull request as `after.json` says, unless `refusal` holds what GitHub says instead.
const GH: &str = r#"#!/bin/sh
here="$(dirname "$0")"
echo "$*" >> "$here/asked"
case "$*" in
  "api graphql "*) cat "$here/rules.json" ;;
  "pr merge "*"--disable-auto") cp "$here/off.json" "$here/view.json" ;;
  "pr merge "*)
    if [ -f "$here/refusal" ]; then cat "$here/refusal" >&2; exit 1; fi
    cp "$here/after.json" "$here/view.json" ;;
  "pr view "*) cat "$here/view.json" ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#;

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

/// What GitHub says the repository allows.
const RULES: &str = r#"{"mergeCommitAllowed": true, "squashMergeAllowed": true,
    "rebaseMergeAllowed": false, "autoMergeAllowed": true, "viewerCanAdminister": false,
    "viewerDefaultMergeMethod": "MERGE"}"#;

/// Pull request #142 as `gh` prints it: in `state`, at `head`, with its one check as
/// `conclusion` says, GitHub's `merge_state`, and auto-merge on with `auto` if given.
fn printed(state: &str, head: &str, conclusion: &str, merge_state: &str, auto: &str) -> String {
    let auto = if auto.is_empty() {
        "null".to_string()
    } else {
        format!(r#"{{"mergeMethod": "{auto}", "commitBody": "never read"}}"#)
    };
    format!(
        r#"{{"number": 142, "title": "fix: the login", "url": "https://github.com/o/r/pull/142",
            "state": "{state}", "isDraft": false, "reviewDecision": "", "mergeable": "MERGEABLE",
            "headRefName": "x", "headRefOid": "{head}", "baseRefName": "main",
            "updatedAt": "2026-11-24T10:00:00Z", "mergeStateStatus": "{merge_state}",
            "autoMergeRequest": {auto},
            "statusCheckRollup": [{{"__typename": "CheckRun", "name": "Test", "workflowName": "CI",
              "status": "COMPLETED", "conclusion": "{conclusion}"}}]}}"#
    )
}

struct Gh {
    tools: PathBuf,
}

impl Gh {
    fn says(&self, file: &str, text: &str) {
        std::fs::write(self.tools.join(file), text).expect("writes");
    }

    /// The pull request as it is read now, and as a merge leaves it.
    fn reads(&self, now: &str, after: &str) {
        self.says("view.json", now);
        self.says("after.json", after);
    }

    /// What it was asked since the last time, one call to a line.
    fn asked(&self) -> Vec<String> {
        let file = self.tools.join("asked");
        let asked = std::fs::read_to_string(&file).unwrap_or_default();
        std::fs::remove_file(&file).ok();
        asked.lines().map(str::to_string).collect()
    }
}

struct Scene {
    gh: Gh,
    worktree: PathBuf,
    branch: String,
    session: i64,
    _fx: common::Fixture,
}

async fn scene() -> (Scene, Core) {
    let fx = fixture();
    let tools = fx.paths.data_dir.join("tools");
    std::fs::create_dir_all(&tools).expect("dir");
    common::executable(&tools.join("gh"), GH);
    let gh = Gh { tools };
    gh.says("rules.json", RULES);
    let backend = Arc::new(FakeBackend::default());
    let mut core = Core::with_backend(fx.paths.clone(), backend).expect("opens");
    core.set_tool_dirs(vec![gh.tools.clone()]);
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
    let scene = Scene {
        gh,
        worktree: task.worktree_path,
        branch: task.branch,
        session: session.id,
        _fx: fx,
    };
    (scene, core)
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

fn current_branch(worktree: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(worktree)
        .output()
        .expect("runs git");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[tokio::test]
async fn a_merge_passes_the_head_seen_deletes_no_branch_and_is_told_once() {
    let (scene, core) = scene().await;
    let gh = &scene.gh;
    let rules = core.project_merge_rules("demo").await.expect("rules");
    assert_eq!(rules.methods, [MergeMethod::Merge, MergeMethod::Squash]);
    assert_eq!(
        rules.method,
        Some(MergeMethod::Merge),
        "the user's own default"
    );
    assert!(rules.auto_merge);
    assert!(!rules.admin);
    assert_eq!(gh.asked().len(), 1);

    let open = printed("OPEN", HEAD, "SUCCESS", "CLEAN", "");
    gh.reads(&open, &printed("MERGED", HEAD, "SUCCESS", "", ""));
    let report = core
        .task_pull_merge(
            "demo",
            "fix-login",
            MergeMethod::Squash,
            MergeWhen::Now,
            HEAD,
        )
        .await
        .expect("merges");
    assert_eq!(report.outcome, MergeOutcome::Merged);
    assert_eq!(report.pull.map(|pull| pull.state), Some(PullState::Merged));
    let asked = gh.asked();
    let merge = format!("pr merge 142 --squash --match-head-commit {HEAD}");
    assert!(asked.contains(&merge), "{asked:?}");
    assert!(!asked.iter().any(|call| call.contains("--delete-branch")));
    // The worktree stays on its branch, which is still there.
    assert_eq!(current_branch(&scene.worktree), scene.branch);
    // The first reading took stock; the merge is told once, and not again when read again.
    assert_eq!(changes(&core, scene.session), [PullChange::Merged]);
    core.task_pull("demo", "fix-login").await.expect("reads");
    assert_eq!(changes(&core, scene.session), [PullChange::Merged]);
    gh.asked();
    // The method is remembered for the repository, and GitHub is not asked again.
    let rules = core.project_merge_rules("demo").await.expect("rules");
    assert_eq!(rules.method, Some(MergeMethod::Squash));
    assert_eq!(gh.asked(), Vec::<String>::new());
}

#[tokio::test]
async fn a_head_that_moved_and_failed_checks_are_refused_before_gh_merges() {
    let (scene, core) = scene().await;
    let gh = &scene.gh;
    let merge = |method, when, head: &'static str| {
        core.task_pull_merge("demo", "fix-login", method, when, head)
    };
    // Someone pushed since the user looked: refused, and what is there now is taken in.
    let moved = printed("OPEN", "fedcba9876543210", "SUCCESS", "CLEAN", "");
    gh.reads(&moved, &printed("MERGED", HEAD, "SUCCESS", "", ""));
    let refused = merge(MergeMethod::Merge, MergeWhen::Now, HEAD)
        .await
        .expect_err("moved on");
    assert!(
        refused.to_string().contains("moved on since it was read"),
        "{refused}"
    );
    let pull = core.task_pull("demo", "fix-login").await.expect("reads");
    assert_eq!(
        pull.map(|pull| pull.head),
        Some("fedcba9876543210".to_string())
    );
    // A check failed: a merge points to a forced one, which goes through.
    let failed = printed("OPEN", HEAD, "FAILURE", "UNSTABLE", "");
    gh.reads(&failed, &printed("MERGED", HEAD, "FAILURE", "", ""));
    let refused = merge(MergeMethod::Merge, MergeWhen::Now, HEAD)
        .await
        .expect_err("a check failed");
    assert!(refused.to_string().contains("A forced merge"), "{refused}");
    assert!(!gh.asked().iter().any(|call| call.starts_with("pr merge")));
    let report = merge(MergeMethod::Merge, MergeWhen::Force, HEAD)
        .await
        .expect("forced");
    assert_eq!(report.outcome, MergeOutcome::Merged);
    let asked = gh.asked();
    assert!(
        asked.contains(&format!("pr merge 142 --merge --match-head-commit {HEAD}")),
        "{asked:?}"
    );
    // Merged already: nothing more to merge.
    let refused = merge(MergeMethod::Merge, MergeWhen::Bypass, HEAD)
        .await
        .expect_err("merged");
    assert_eq!(refused.to_string(), "#142 is merged already");
}

#[tokio::test]
async fn githubs_refusal_is_said_in_its_words_and_a_queue_is_named() {
    let (scene, core) = scene().await;
    let gh = &scene.gh;
    let open = printed("OPEN", HEAD, "FAILURE", "BLOCKED", "");
    gh.reads(&open, &open);
    gh.says(
        "refusal",
        "X Pull request o/r#142 is not mergeable: the base branch policy prohibits the merge.\n\
         hint: if you need to wait for this pull request, use this instead of polling:\n    \
         gh event listen --resource https://github.com/o/r/pull/142\n",
    );
    let refused = core
        .task_pull_merge(
            "demo",
            "fix-login",
            MergeMethod::Merge,
            MergeWhen::Force,
            HEAD,
        )
        .await
        .expect_err("refused");
    assert_eq!(
        refused.to_string(),
        "#142 was not merged: Pull request o/r#142 is not mergeable: the base branch policy \
         prohibits the merge."
    );
    // As an admin, past the requirements: a merge queue takes it, and that is what is said.
    std::fs::remove_file(gh.tools.join("refusal")).expect("removes");
    let report = core
        .task_pull_merge(
            "demo",
            "fix-login",
            MergeMethod::Merge,
            MergeWhen::Bypass,
            HEAD,
        )
        .await
        .expect("queued");
    assert_eq!(report.outcome, MergeOutcome::Queued);
    let asked = gh.asked();
    let bypass = format!("pr merge 142 --merge --match-head-commit {HEAD} --admin");
    assert!(asked.contains(&bypass), "{asked:?}");
    assert_eq!(changes(&core, scene.session), []);
}

#[tokio::test]
async fn auto_merge_is_set_while_requirements_wait_and_taken_back() {
    let (scene, core) = scene().await;
    let gh = &scene.gh;
    let auto = |method| core.task_pull_merge("demo", "fix-login", method, MergeWhen::Auto, HEAD);
    // Requirements met already: auto-merge would merge it at once, so it is refused.
    gh.reads(
        &printed("OPEN", HEAD, "SUCCESS", "CLEAN", ""),
        &printed("MERGED", HEAD, "SUCCESS", "", ""),
    );
    let refused = auto(MergeMethod::Squash).await.expect_err("mergeable now");
    assert!(
        refused.to_string().contains("can be merged now"),
        "{refused}"
    );
    // Waiting on a required check: GitHub merges it once that passes.
    let waiting = printed("OPEN", HEAD, "", "BLOCKED", "");
    gh.reads(&waiting, &printed("OPEN", HEAD, "", "BLOCKED", "SQUASH"));
    let report = auto(MergeMethod::Squash).await.expect("auto-merge on");
    assert_eq!(report.outcome, MergeOutcome::AutoMerge);
    let pull = report.pull.expect("read back");
    assert_eq!(pull.auto_merge, Some(MergeMethod::Squash));
    assert_eq!(pull.base, "main");
    let asked = gh.asked();
    let set = format!("pr merge 142 --squash --match-head-commit {HEAD} --auto");
    assert!(asked.contains(&set), "{asked:?}");
    // Taken back.
    gh.says("off.json", &waiting);
    let report = core
        .task_pull_auto_merge_off("demo", "fix-login")
        .await
        .expect("off");
    assert_eq!(report.outcome, MergeOutcome::AutoMergeOff);
    assert_eq!(report.pull.and_then(|pull| pull.auto_merge), None);
    assert!(
        gh.asked()
            .contains(&"pr merge 142 --disable-auto".to_string())
    );
    // Off already: nothing is asked to change.
    core.task_pull_auto_merge_off("demo", "fix-login")
        .await
        .expect("off");
    assert!(!gh.asked().iter().any(|call| call.starts_with("pr merge")));
}
