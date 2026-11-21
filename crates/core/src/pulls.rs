//! A task's pull request and its checks, as the GitHub CLI reports them.
//!
//! Swarmed holds no GitHub token: it asks the user's own `gh`, in the task's worktree, and shows
//! what that says. Without `gh`, without a pull request, or in a repository that is not on
//! GitHub, a task simply has none.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::agent_setup::find_program;
use crate::error::Error;
use crate::{Core, Result, open};

/// How long `gh` gets to answer. It asks the network, and nothing waits on it.
const GH_TIMEOUT: Duration = Duration::from_secs(20);

/// What `gh pr view` is asked for.
const FIELDS: &str = "number,title,url,state,isDraft,reviewDecision,statusCheckRollup";

/// Where a pull request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// Its checks, in the order GitHub lists them.
    pub checks: Vec<Check>,
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

/// Reads what `gh pr view --json` printed.
fn parse(json: &str) -> Result<Pull> {
    let raw: RawPull = serde_json::from_str(json)?;
    let state = match raw.state.as_str() {
        "MERGED" => PullState::Merged,
        "CLOSED" => PullState::Closed,
        _ => PullState::Open,
    };
    let review = (!raw.review_decision.is_empty()).then(|| raw.review_decision.to_lowercase());
    Ok(Pull {
        number: raw.number,
        title: raw.title,
        url: raw.url,
        state,
        draft: raw.is_draft,
        review,
        checks: raw.status_check_rollup.into_iter().map(check).collect(),
    })
}

impl Core {
    /// The pull request of a task's branch with its checks, if it has one that `gh` can see.
    ///
    /// Nothing, not an error, when `gh` is not installed, is not signed in, takes too long, or
    /// finds no pull request: a task without one is the ordinary case.
    pub async fn task_pull(&self, project: &str, name: &str) -> Result<Option<Pull>> {
        let (_, task) = self.task(project, name)?;
        let Some(gh) = find_program("gh", &self.tool_dirs) else {
            return Ok(None);
        };
        let mut command = Command::new(gh);
        command
            .args(["pr", "view", &task.branch, "--json", FIELDS])
            .current_dir(&task.worktree_path)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let Ok(Ok(output)) = tokio::time::timeout(GH_TIMEOUT, command.output()).await else {
            return Ok(None);
        };
        if !output.status.success() {
            return Ok(None);
        }
        parse(&String::from_utf8_lossy(&output.stdout)).map(Some)
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
        let pull = parse(PRINTED).expect("parses");
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
        let pull = parse(printed).expect("parses");
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
        let pull = parse(printed).expect("parses");
        assert_eq!(pull.state, PullState::Closed);
        assert_eq!(pull.checks[0].outcome, CheckOutcome::Failed);
    }
}
