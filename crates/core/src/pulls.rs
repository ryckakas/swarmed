//! A task's pull request and its checks, as the GitHub CLI reports them.
//!
//! Swarmed holds no GitHub token: it asks the user's own `gh`, in the task's worktree, and shows
//! what that says. Without `gh`, without a pull request, or in a repository that is not on
//! GitHub, a task simply has none.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::agent_setup::find_program;
use crate::error::Error;
use crate::following::Reading;
use crate::merging::MergeMethod;
use crate::{Core, Result, git, open};

/// How long `gh` gets to answer. It asks the network, and nothing waits on it.
pub(crate) const GH_TIMEOUT: Duration = Duration::from_secs(20);

/// What `gh pr view` and `gh pr list` are asked for: what is shown, and what tells a change.
pub(crate) const FIELDS: &str = "number,title,url,state,isDraft,reviewDecision,\
                                 statusCheckRollup,headRefName,headRefOid,updatedAt,mergeable,\
                                 baseRefName,autoMergeRequest,mergeStateStatus,mergedAt";

/// Where a pull request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullState {
    /// Open and waiting.
    Open,
    /// Merged into its base.
    Merged,
    /// Closed without merging.
    Closed,
}

/// How one check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    /// Finished well.
    Passed,
    /// Finished badly: failed, timed out, was cancelled or needs someone to act.
    Failed,
    /// Queued or still going.
    Running,
    /// Left out on purpose, which is neither good nor bad.
    Skipped,
}

/// One check on a pull request: a CI job, or a status another service reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// What the check is called.
    pub name: String,
    /// The workflow it is a job of, for a check that is one: `E2E` for its job `shard 3`.
    pub workflow: Option<String>,
    /// How it came out.
    pub outcome: CheckOutcome,
    /// Where its details are, if it said.
    pub url: Option<String>,
}

/// The pull request of a task's branch.
///
/// Kept as JSON with the task (`following.rs`), so its fields are only added, never renamed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pull {
    /// Its number in the repository.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Where it is.
    pub url: String,
    /// Where it stands.
    pub state: PullState,
    /// Whether it is a draft.
    pub draft: bool,
    /// What its reviewers decided: `approved`, `changes_requested`, `review_required`, or
    /// nothing if no review is asked for.
    pub review: Option<String>,
    /// Its checks, in the order GitHub lists them, each check run only as its latest run.
    pub checks: Vec<Check>,
    /// Whether its branch conflicts with its base, as GitHub last worked it out. Absent from
    /// what was kept before this.
    #[serde(default)]
    pub conflicts: bool,
    /// Its head commit, which a merge names so that only what was read is merged. Absent
    /// from what was kept before this.
    #[serde(default)]
    pub head: String,
    /// The branch it goes into. Absent from what was kept before this.
    #[serde(default)]
    pub base: String,
    /// The method GitHub merges it with once its requirements are met, while its auto-merge
    /// is on.
    #[serde(default)]
    pub auto_merge: Option<MergeMethod>,
    /// Whether GitHub would merge it now, as it last worked it out: `clean`, `unstable`
    /// (checks no rule requires did not pass), `has_hooks`, `blocked` (a requirement is not
    /// met), `behind` (its base moved on and a rule wants it up to date), `dirty`
    /// (conflicts) or `draft`; nothing while GitHub has not worked it out.
    #[serde(default)]
    pub merge_state: Option<String>,
    /// When it was merged, as GitHub writes it (`2026-11-20T10:00:00Z`), if it was. Absent
    /// from what was kept before this.
    #[serde(default)]
    pub merged_at: Option<String>,
}

/// A check as `gh` prints it: a check run has a status and a conclusion, a commit status only a
/// state, and their names sit in different fields.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCheck {
    #[serde(default)]
    name: String,
    #[serde(default)]
    workflow_name: String,
    #[serde(default)]
    context: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    details_url: String,
    #[serde(default)]
    target_url: String,
    #[serde(default)]
    started_at: String,
    #[serde(default)]
    completed_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPull {
    number: u64,
    title: String,
    url: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    review_decision: String,
    #[serde(default)]
    status_check_rollup: Vec<RawCheck>,
    #[serde(default)]
    head_ref_name: String,
    #[serde(default)]
    head_ref_oid: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    mergeable: String,
    #[serde(default)]
    base_ref_name: String,
    #[serde(default)]
    auto_merge_request: Option<RawAutoMerge>,
    #[serde(default)]
    merge_state_status: String,
    #[serde(default)]
    merged_at: Option<String>,
}

/// A pull request's auto-merge as `gh` prints it. The commit message it would merge with is
/// in that too, and never read into this.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAutoMerge {
    #[serde(default)]
    merge_method: String,
}

/// How a check came out, from whichever of its fields says so.
fn outcome(check: &RawCheck) -> CheckOutcome {
    // A check run that is not finished has no conclusion yet.
    let unfinished = !check.status.is_empty() && check.status != "COMPLETED";
    let verdict = if check.conclusion.is_empty() {
        &check.state
    } else {
        &check.conclusion
    };
    match verdict.as_str() {
        _ if unfinished => CheckOutcome::Running,
        "SUCCESS" => CheckOutcome::Passed,
        "SKIPPED" | "NEUTRAL" => CheckOutcome::Skipped,
        "" | "PENDING" | "EXPECTED" => CheckOutcome::Running,
        // Failure, error, timed out, cancelled, action required, and whatever comes later:
        // anything that is not known to be fine is worth a look.
        _ => CheckOutcome::Failed,
    }
}

fn check(raw: RawCheck) -> Check {
    let outcome = outcome(&raw);
    let name = if raw.name.is_empty() {
        raw.context
    } else {
        raw.name
    };
    let url = if raw.details_url.is_empty() {
        raw.target_url
    } else {
        raw.details_url
    };
    Check {
        name,
        workflow: (!raw.workflow_name.is_empty()).then_some(raw.workflow_name),
        outcome,
        url: (!url.is_empty()).then_some(url),
    }
}

/// When a check run started, and then when it finished, to tell a later run of the same check
/// from one it replaced. One that has not started yet, which `gh` gives the year 1 or no time
/// at all, is the latest: it is a run queued after the others.
fn when(check: &RawCheck) -> (&str, &str) {
    let queued = check.started_at.is_empty() || check.started_at.starts_with("0001-");
    let started = if queued {
        "~"
    } else {
        check.started_at.as_str()
    };
    (started, check.completed_at.as_str())
}

/// Whether a later run of the same check (the same job name in the same workflow) replaced the
/// check run at `at`: a workflow run that a newer one cancelled, or an attempt run again, which
/// starts once the one before it has finished. Two runs under way at the same time are two
/// runs of one job, for `push` and for `pull_request`, say, which `gh pr view` does not tell
/// apart: each counts, or one that passed would hide one that failed. Of two that started at
/// the same moment, the one listed later counts as the later. A commit status, which has a
/// context and no name, comes once per context already.
fn replaced(checks: &[RawCheck], at: usize) -> bool {
    let check = &checks[at];
    let same =
        |other: &RawCheck| other.name == check.name && other.workflow_name == check.workflow_name;
    let finished = !check.completed_at.is_empty() && !check.completed_at.starts_with("0001-");
    let made_way = |other: &RawCheck| {
        check.conclusion == "CANCELLED"
            || (finished && when(other).0 >= check.completed_at.as_str())
    };
    !check.name.is_empty()
        && checks.iter().enumerate().any(|(later, other)| {
            later != at
                && same(other)
                && (when(other), later) > (when(check), at)
                && made_way(other)
        })
}

/// The checks as GitHub's own page and `gh pr checks` list them: `gh pr view` also reports
/// every run a later one replaced, and a cancelled one would show as failed beside it.
fn latest(checks: Vec<RawCheck>) -> Vec<RawCheck> {
    let kept: Vec<bool> = (0..checks.len()).map(|at| !replaced(&checks, at)).collect();
    checks
        .into_iter()
        .zip(kept)
        .filter_map(|(check, kept)| kept.then_some(check))
        .collect()
}

/// A pull request as one reading of it: what is shown, and what tells a change.
fn reading(raw: RawPull) -> Reading {
    let state = match raw.state.as_str() {
        "MERGED" => PullState::Merged,
        "CLOSED" => PullState::Closed,
        _ => PullState::Open,
    };
    let review = (!raw.review_decision.is_empty()).then(|| raw.review_decision.to_lowercase());
    // GitHub works out whether a branch conflicts some time after it moved, and says
    // `UNKNOWN` meanwhile: that says nothing either way.
    let conflicts = match raw.mergeable.as_str() {
        "CONFLICTING" => Some(true),
        "MERGEABLE" => Some(false),
        _ => None,
    };
    // GitHub merges with no method but these three.
    let auto_merge = (raw.auto_merge_request)
        .map(|auto| MergeMethod::of_github(&auto.merge_method).unwrap_or(MergeMethod::Merge));
    // `UNKNOWN` while GitHub works it out, as for conflicts.
    let merge_state = match raw.merge_state_status.as_str() {
        "" | "UNKNOWN" => None,
        known => Some(known.to_lowercase()),
    };
    let pull = Pull {
        number: raw.number,
        title: raw.title,
        url: raw.url,
        state,
        draft: raw.is_draft,
        review,
        checks: latest(raw.status_check_rollup)
            .into_iter()
            .map(check)
            .collect(),
        conflicts: conflicts.unwrap_or(false),
        head: raw.head_ref_oid.clone(),
        base: raw.base_ref_name,
        auto_merge,
        merge_state,
        // GitHub says no merge with the year 1, or nothing.
        merged_at: raw
            .merged_at
            .filter(|at| crate::put_away::seconds_of(at).is_some()),
    };
    Reading {
        pull,
        branch: raw.head_ref_name,
        head: raw.head_ref_oid,
        updated_at: raw.updated_at,
        conflicts,
    }
}

/// Reads what `gh pr view --json` printed.
pub(crate) fn parse(json: &str) -> Result<Reading> {
    Ok(reading(serde_json::from_str(json)?))
}

/// Reads what `gh pr list --json` printed.
pub(crate) fn parse_list(json: &str) -> Result<Vec<Reading>> {
    let raw: Vec<RawPull> = serde_json::from_str(json)?;
    Ok(raw.into_iter().map(reading).collect())
}

/// Why `gh` gave no answer: it is not there, took too long, could not be started, or said no,
/// in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GhFailed(pub(crate) String);

/// What `gh` printed when asked `args` in `dir`, or why it printed nothing of use. Never
/// waits longer than [`GH_TIMEOUT`], and never reads from the terminal.
pub(crate) async fn ask_gh(
    gh: &Path,
    dir: &Path,
    args: &[&str],
) -> std::result::Result<String, GhFailed> {
    ask_gh_within(gh, dir, args, GH_TIMEOUT).await
}

/// What `gh` printed when asked `args` in `dir`, as [`ask_gh`], given `timeout` to answer.
pub(crate) async fn ask_gh_within(
    gh: &Path,
    dir: &Path,
    args: &[&str],
    timeout: Duration,
) -> std::result::Result<String, GhFailed> {
    let mut command = Command::new(gh);
    command
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = match tokio::time::timeout(timeout, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return Err(GhFailed(error.to_string())),
        Err(_) => return Err(GhFailed("`gh` took too long to answer".to_string())),
    };
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        return Err(GhFailed(said.trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

impl Core {
    /// The pull request of the branch a task's worktree is on, with its checks, if it has one
    /// that `gh` can see: the task's own branch, unless something switched the worktree.
    ///
    /// Nothing, not an error, when `gh` is not installed, is not signed in, takes too long, or
    /// finds no pull request: a task without one is the ordinary case. When `gh` gives no
    /// answer, the pull request as last read is given, if it is the branch's.
    ///
    /// What is read is taken in as the follower's readings are (`following.rs`): what changed
    /// since the last reading is recorded, and every front end hears the reading.
    pub async fn task_pull(&self, project: &str, name: &str) -> Result<Option<Pull>> {
        let (_, task) = self.repo_task(project, name)?;
        // On no branch there is none to have a pull request.
        let Ok(Some(branch)) = git::current_branch(&task.worktree_path).await else {
            return Ok(None);
        };
        let kept = self
            .pull_kept(task.id)?
            .filter(|seen| seen.branch == branch);
        let Some(gh) = find_program("gh", &self.tool_dirs) else {
            return Ok(kept.map(|seen| seen.pull));
        };
        let args = ["pr", "view", &branch, "--json", FIELDS];
        let Ok(printed) = ask_gh(&gh, &task.worktree_path, &args).await else {
            return Ok(kept.map(|seen| seen.pull));
        };
        let reading = parse(&printed)?;
        let seen = self.pull_take_in(&task, Some(reading), None)?;
        Ok(seen.map(|seen| seen.pull))
    }

    /// Opens a task's pull request in the browser, or the details of its check named `check`.
    pub async fn task_pull_open(
        &self,
        project: &str,
        name: &str,
        check: Option<&str>,
    ) -> Result<()> {
        let pull = self.task_pull(project, name).await?;
        let pull = pull.ok_or_else(|| Error::Pull(format!("`{name}` has no pull request")))?;
        // Both addresses are what GitHub answered, which anyone who can add a check has a
        // say in: only a web address is handed to the system to open.
        let Some(wanted) = check else {
            return open::open_link(&pull.url);
        };
        let found = pull.checks.iter().find(|check| check.name == wanted);
        let url = found.and_then(|check| check.url.as_deref());
        open::open_link(url.unwrap_or(&pull.url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRINTED: &str = r#"{
        "isDraft": true, "number": 115, "reviewDecision": "CHANGES_REQUESTED", "state": "OPEN",
        "title": "feat: one list", "url": "https://github.com/o/r/pull/115",
        "statusCheckRollup": [
            {"__typename": "CheckRun", "name": "Format and lint", "workflowName": "CI", "status": "QUEUED",
             "conclusion": "", "detailsUrl": "https://github.com/o/r/actions/runs/1/job/2"},
            {"__typename": "CheckRun", "name": "Test", "status": "COMPLETED",
             "conclusion": "SUCCESS", "detailsUrl": ""},
            {"__typename": "CheckRun", "name": "Dependencies", "status": "COMPLETED",
             "conclusion": "FAILURE", "detailsUrl": "https://github.com/o/r/actions/runs/1/job/3"},
            {"__typename": "CheckRun", "name": "Linux", "status": "COMPLETED",
             "conclusion": "SKIPPED"},
            {"__typename": "CheckRun", "name": "Deploy", "status": "COMPLETED",
             "conclusion": "TIMED_OUT"},
            {"__typename": "StatusContext", "context": "coverage", "state": "PENDING",
             "targetUrl": "https://cov.example/1"},
            {"__typename": "StatusContext", "context": "license", "state": "SUCCESS"}
        ]
    }"#;

    #[test]
    fn what_gh_prints_is_read_whole() {
        let pull = parse(PRINTED).expect("parses").pull;
        assert_eq!(pull.number, 115);
        assert_eq!(pull.state, PullState::Open);
        assert!(pull.draft);
        assert_eq!(pull.review.as_deref(), Some("changes_requested"));
        let outcomes: Vec<_> = pull
            .checks
            .iter()
            .map(|check| (check.name.as_str(), check.outcome))
            .collect();
        assert_eq!(
            outcomes,
            [
                ("Format and lint", CheckOutcome::Running),
                ("Test", CheckOutcome::Passed),
                ("Dependencies", CheckOutcome::Failed),
                ("Linux", CheckOutcome::Skipped),
                ("Deploy", CheckOutcome::Failed),
                ("coverage", CheckOutcome::Running),
                ("license", CheckOutcome::Passed),
            ]
        );
        assert_eq!(pull.checks[1].url, None);
        // A job says which workflow it belongs to; a status from another service has none.
        assert_eq!(pull.checks[0].workflow.as_deref(), Some("CI"));
        assert_eq!(pull.checks[5].workflow, None);
        assert_eq!(pull.checks[5].url.as_deref(), Some("https://cov.example/1"));
    }

    #[test]
    fn a_pull_request_without_checks_or_review_is_still_one() {
        let printed = r#"{"number": 7, "title": "t", "url": "u", "state": "MERGED"}"#;
        let pull = parse(printed).expect("parses").pull;
        assert_eq!(pull.state, PullState::Merged);
        assert_eq!(pull.review, None);
        assert_eq!(pull.checks, []);
        assert!(!pull.draft);
        assert!(parse("no pull requests found").is_err());
    }

    #[test]
    fn an_outcome_nobody_has_heard_of_counts_as_failed() {
        let printed = r#"{"number": 1, "title": "t", "url": "u", "state": "CLOSED",
            "statusCheckRollup": [{"name": "x", "status": "COMPLETED", "conclusion": "STALE"}]}"#;
        let pull = parse(printed).expect("parses").pull;
        assert_eq!(pull.state, PullState::Closed);
        assert_eq!(pull.checks[0].outcome, CheckOutcome::Failed);
    }

    /// A check run as `gh` prints it, with when it started and finished.
    fn run(workflow: &str, name: &str, conclusion: &str, started: &str, completed: &str) -> String {
        format!(
            r#"{{"__typename": "CheckRun", "workflowName": "{workflow}", "name": "{name}",
               "status": "{}", "conclusion": "{conclusion}",
               "startedAt": "{started}", "completedAt": "{completed}"}}"#,
            if conclusion.is_empty() {
                "QUEUED"
            } else {
                "COMPLETED"
            }
        )
    }

    fn checks_of(runs: &[String]) -> Vec<(String, CheckOutcome)> {
        let printed = format!(
            r#"{{"number": 1, "title": "t", "url": "u", "state": "OPEN", "statusCheckRollup": [{}]}}"#,
            runs.join(",")
        );
        let pull = parse(&printed).expect("parses").pull;
        // Named with the workflow, which tells two jobs of the same name apart.
        let named = |check: Check| {
            let workflow = check.workflow.unwrap_or_default();
            (format!("{workflow} / {}", check.name), check.outcome)
        };
        pull.checks.into_iter().map(named).collect()
    }

    #[test]
    fn a_run_that_a_newer_run_cancelled_is_left_out() {
        // The e2e workflow's run for `opened`, cancelled by its run for `labeled`, which skipped.
        let checks = checks_of(&[
            run(
                "e2e",
                "e2e",
                "CANCELLED",
                "2026-11-22T15:01:50Z",
                "2026-11-22T15:01:56Z",
            ),
            run(
                "e2e",
                "e2e",
                "SKIPPED",
                "2026-11-22T15:01:55Z",
                "2026-11-22T15:01:57Z",
            ),
        ]);
        assert_eq!(checks, [("e2e / e2e".to_string(), CheckOutcome::Skipped)]);
    }

    #[test]
    fn two_runs_of_a_job_under_way_at_once_both_count() {
        // One workflow run for `push` and one for `pull_request`, on the same commit.
        let checks = checks_of(&[
            run(
                "CI",
                "Test",
                "FAILURE",
                "2026-11-22T15:01:00Z",
                "2026-11-22T15:04:00Z",
            ),
            run(
                "CI",
                "Test",
                "SUCCESS",
                "2026-11-22T15:01:02Z",
                "2026-11-22T15:05:00Z",
            ),
        ]);
        let expected = [CheckOutcome::Failed, CheckOutcome::Passed];
        assert_eq!(
            checks,
            expected.map(|outcome| ("CI / Test".to_string(), outcome))
        );
    }

    #[test]
    fn the_latest_attempt_of_a_check_counts_and_the_rest_keep_their_place() {
        let at = |minute: u8| format!("2026-11-22T15:{minute:02}:00Z");
        let checks = checks_of(&[
            run("CI", "Test", "FAILURE", &at(1), &at(4)),
            run("CI", "Lint", "SUCCESS", &at(1), &at(2)),
            // Another workflow's job of the same name is a check of its own.
            run("Nightly", "Test", "FAILURE", &at(1), &at(3)),
            run("CI", "Test", "SUCCESS", &at(5), &at(8)),
            // Run again and not started yet: queued after the others, so it is the latest.
            run(
                "CI",
                "Lint",
                "",
                "0001-01-01T00:00:00Z",
                "0001-01-01T00:00:00Z",
            ),
        ]);
        let expected = [
            ("Nightly / Test", CheckOutcome::Failed),
            ("CI / Test", CheckOutcome::Passed),
            ("CI / Lint", CheckOutcome::Running),
        ];
        let expected: Vec<_> = expected
            .into_iter()
            .map(|(name, outcome)| (name.to_string(), outcome))
            .collect();
        assert_eq!(checks, expected);
    }
}
