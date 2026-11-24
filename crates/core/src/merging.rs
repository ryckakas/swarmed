//! Merging a task's pull request from Swarmed, with the user's own `gh`.
//!
//! - **Only the head the user saw is merged.** Every merge names the commit the pull request
//!   was read at (`--match-head-commit`). One that moved since is refused, here or by GitHub,
//!   and is read again, so the window shows what is there now.
//! - **No branch is deleted.** `gh pr merge --delete-branch` would delete the local branch and
//!   move the worktree off it, under the agents' feet. The remote branch is left to the
//!   repository's own setting; the local one goes with the task, as removal decides.
//! - **Three ways to merge now**, as the window's split button offers them: a merge, which
//!   Swarmed refuses while a check failed; a forced one, which merges anyway where no rule
//!   requires those checks; and one past the repository's requirements (`--admin`), which only
//!   its admins can make. And GitHub's own auto-merge (`--auto`, `--disable-auto`), which merges
//!   once the requirements are met. It is refused where they are met already, since `gh` would
//!   merge at once, and GitHub offers no auto-merge then either.
//! - **What the repository allows** (its methods, auto-merge, whether the user administers it)
//!   is asked once per repository while Swarmed runs. The method last merged with there is
//!   kept in the database, as the default environment is, not in the repository's config file,
//!   which an older Swarmed on the same data would refuse.
//! - **What came of it is read back**: merged, queued (the base's merge queue took it), or
//!   auto-merge on. That reading is taken in as every other (`following.rs`), which records
//!   the merge once and updates every badge.
//! - **GitHub's refusals reach the user in its words**, without `gh`'s marks and hints.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::agent_setup::find_program;
use crate::error::Error;
use crate::pulls::{CheckOutcome, FIELDS, GhFailed, Pull, PullState, ask_gh, ask_gh_within, parse};
use crate::store::{Project, Store, Task};
use crate::{Core, Result, git, lock};

/// How long a merge may take: GitHub squashes or rebases before it answers.
const MERGE_TIMEOUT: Duration = Duration::from_secs(90);

/// What is asked of GitHub about a repository: one query, since `gh repo view` cannot say
/// whether auto-merge is allowed.
const RULES_QUERY: &str = "query($owner: String!, $name: String!) { \
     repository(owner: $owner, name: $name) { \
       mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed autoMergeAllowed \
       viewerCanAdminister viewerDefaultMergeMethod } }";

/// How a pull request's commits go into its base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeMethod {
    /// With a merge commit.
    Merge,
    /// As one commit.
    Squash,
    /// Its commits one by one, onto the base.
    Rebase,
}

impl MergeMethod {
    /// Every method, in the order GitHub lists them.
    pub const ALL: [Self; 3] = [Self::Merge, Self::Squash, Self::Rebase];

    /// The method GitHub names `name`: `MERGE`, `SQUASH` or `REBASE`.
    pub(crate) fn of_github(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|method| method.name().eq_ignore_ascii_case(name))
    }

    /// Its name, as it is kept and as `gh` takes it for a flag.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }
}

/// When, and past what, a pull request is merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeWhen {
    /// Now, unless one of its checks failed.
    Now,
    /// Now, although checks failed. GitHub still refuses where a rule requires them.
    Force,
    /// Now, past the repository's requirements, as only its admins can (`gh --admin`).
    Bypass,
    /// Once its requirements are met, by GitHub's own auto-merge (`gh --auto`).
    Auto,
}

impl MergeWhen {
    /// What `gh pr merge` is told besides the method.
    fn flag(self) -> Option<&'static str> {
        match self {
            Self::Now | Self::Force => None,
            Self::Bypass => Some("--admin"),
            Self::Auto => Some("--auto"),
        }
    }
}

/// What a repository allows for merging, and what the user may do there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergeRules {
    /// The methods it allows, in the order GitHub lists them.
    pub methods: Vec<MergeMethod>,
    /// The method a merge starts on: the one last merged with here, else the user's own
    /// default on GitHub, else the first it allows. Nothing if it allows none.
    pub method: Option<MergeMethod>,
    /// Whether it allows auto-merge.
    pub auto_merge: bool,
    /// Whether the user administers it, which lets them merge past its requirements.
    pub admin: bool,
}

/// What came of a merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeOutcome {
    /// It is merged into its base.
    Merged,
    /// Its base's merge queue took it, and merges it once the queue's own checks pass.
    Queued,
    /// GitHub merges it once its requirements are met.
    AutoMerge,
    /// Its auto-merge is off.
    AutoMergeOff,
    /// `gh` said it was done, and reading it back failed: what came of it shows at the next
    /// reading.
    Asked,
}

/// What came of a merge, and the pull request as read after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergeReport {
    /// What came of it.
    pub outcome: MergeOutcome,
    /// The pull request as read after it, if that could be read.
    pub pull: Option<Pull>,
}

/// What GitHub says of a repository, as the rules query prints it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "GitHub's answer, field by field"
)]
struct Allowed {
    #[serde(default, rename = "mergeCommitAllowed")]
    merge_commit: bool,
    #[serde(default, rename = "squashMergeAllowed")]
    squash: bool,
    #[serde(default, rename = "rebaseMergeAllowed")]
    rebase: bool,
    #[serde(default, rename = "autoMergeAllowed")]
    auto_merge: bool,
    #[serde(default, rename = "viewerCanAdminister")]
    admin: bool,
    #[serde(default, rename = "viewerDefaultMergeMethod")]
    users_method: String,
}

/// What GitHub said of each repository, by project id, while this process runs.
#[derive(Debug, Default)]
pub(crate) struct Merging {
    allowed: Mutex<HashMap<i64, Allowed>>,
}

/// The rules of a repository that GitHub says `allowed` of, where the method last merged
/// with was `kept`.
fn rules(allowed: &Allowed, kept: Option<MergeMethod>) -> MergeRules {
    let flags = [allowed.merge_commit, allowed.squash, allowed.rebase];
    let methods: Vec<MergeMethod> = (MergeMethod::ALL.into_iter().zip(flags))
        .filter_map(|(method, on)| on.then_some(method))
        .collect();
    let theirs = MergeMethod::of_github(&allowed.users_method);
    let method = [kept, theirs]
        .into_iter()
        .flatten()
        .find(|method| methods.contains(method))
        .or_else(|| methods.first().copied());
    MergeRules {
        methods,
        method,
        auto_merge: allowed.auto_merge,
        admin: allowed.admin,
    }
}

/// The first characters of a commit's id, as one names it.
fn short(head: &str) -> &str {
    head.get(..7).unwrap_or(head)
}

/// Whether GitHub would merge a pull request in `merge_state` now: then its auto-merge would
/// merge it at once.
fn mergeable_now(merge_state: Option<&str>) -> bool {
    matches!(merge_state, Some("clean" | "unstable" | "has_hooks"))
}

/// Why `pull`, as just read, is not merged `when` by a user who saw it at `seen_head`, if it
/// is not: only an open pull request that is not a draft, at the head that was seen; with no
/// failed check for a merge that is not forced; and with requirements still to be met for
/// auto-merge.
fn refusal(pull: &Pull, when: MergeWhen, seen_head: &str) -> Option<String> {
    let number = pull.number;
    match pull.state {
        PullState::Merged => return Some(format!("#{number} is merged already")),
        PullState::Closed => return Some(format!("#{number} is closed")),
        PullState::Open => {}
    }
    if pull.draft {
        return Some(format!(
            "#{number} is a draft: mark it ready for review first"
        ));
    }
    if pull.head != seen_head {
        return Some(format!(
            "#{number} moved on since it was read: its head is {} now. Look at it again before \
             merging.",
            short(&pull.head)
        ));
    }
    match when {
        MergeWhen::Now => failed_refusal(pull),
        MergeWhen::Auto if mergeable_now(pull.merge_state.as_deref()) => Some(format!(
            "#{number} can be merged now: its requirements are met, so auto-merge would merge it \
             at once. Merge it instead."
        )),
        _ => None,
    }
}

/// Why a merge that is not forced is refused, if checks of `pull` failed.
fn failed_refusal(pull: &Pull) -> Option<String> {
    let mut failed: Vec<&str> = Vec::new();
    for check in pull
        .checks
        .iter()
        .filter(|check| check.outcome == CheckOutcome::Failed)
    {
        if !failed.contains(&check.name.as_str()) {
            failed.push(&check.name);
        }
    }
    (!failed.is_empty()).then(|| {
        format!(
            "checks failed on #{}: {}. A forced merge merges it anyway, where no rule of the \
             repository requires them.",
            pull.number,
            failed.join(", ")
        )
    })
}

/// What came of asking `gh` to merge `when`, by the pull request as read after it.
fn outcome_of(when: MergeWhen, after: Option<&Pull>) -> MergeOutcome {
    match after {
        None => MergeOutcome::Asked,
        Some(pull) if pull.state == PullState::Merged => MergeOutcome::Merged,
        Some(_) if when == MergeWhen::Auto => MergeOutcome::AutoMerge,
        // GitHub merges before it answers, unless a merge queue took the pull request.
        Some(_) => MergeOutcome::Queued,
    }
}

/// Whether the pull request read after a `gh` that failed shows it done all the same, as it
/// is after a `gh` that took too long to say so.
fn done_anyway(when: MergeWhen, after: &Pull) -> bool {
    match when {
        MergeWhen::Auto => after.auto_merge.is_some() || after.state == PullState::Merged,
        _ => after.state == PullState::Merged,
    }
}

/// What `gh` said, without its marks and the hints it adds about itself.
pub(crate) fn gh_words(said: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut hint = false;
    for line in said.lines() {
        let goes_on = line.starts_with([' ', '\t']);
        hint = line.starts_with("hint:") || (hint && goes_on);
        if !hint {
            kept.push(line.strip_prefix("X ").unwrap_or(line));
        }
    }
    kept.join("\n").trim().to_string()
}

fn refused(why: impl Into<String>) -> Error {
    Error::Pull(why.into())
}

/// The user's `gh`, or why there is none.
fn gh_in(tool_dirs: &[PathBuf]) -> Result<PathBuf> {
    find_program("gh", tool_dirs)
        .ok_or_else(|| refused("the GitHub CLI `gh` is not installed: https://cli.github.com"))
}

impl Store {
    /// The method a pull request of project `id` was last merged with.
    fn project_merge_method(&self, id: i64) -> Result<Option<MergeMethod>> {
        let sql = "SELECT merge_method FROM projects WHERE id = ?1";
        let found: Option<Option<String>> = self
            .conn
            .query_row(sql, params![id], |row| row.get(0))
            .optional()?;
        let name = found.flatten();
        Ok(name.and_then(|name| MergeMethod::ALL.into_iter().find(|m| m.name() == name)))
    }

    fn project_set_merge_method(&self, id: i64, method: MergeMethod) -> Result<()> {
        let sql = "UPDATE projects SET merge_method = ?2 WHERE id = ?1";
        self.conn.execute(sql, params![id, method.name()])?;
        Ok(())
    }
}

impl Core {
    /// What a repository allows for merging its pull requests, and the method a merge there
    /// starts on. GitHub is asked once per repository while Swarmed runs.
    pub async fn project_merge_rules(&self, project: &str) -> Result<MergeRules> {
        let project = self.project(project)?;
        let gh = gh_in(&self.tool_dirs)?;
        let allowed = self.allowed(&gh, &project).await?;
        let kept = self.store().project_merge_method(project.id)?;
        Ok(rules(&allowed, kept))
    }

    /// What GitHub says of a repository, asked once while this process runs.
    async fn allowed(&self, gh: &Path, project: &Project) -> Result<Allowed> {
        if let Some(known) = lock(&self.merging.allowed).get(&project.id) {
            return Ok(known.clone());
        }
        let query = format!("query={RULES_QUERY}");
        let args = [
            "api",
            "graphql",
            "-F",
            "owner={owner}",
            "-F",
            "name={repo}",
            "-f",
            &query,
            "--jq",
            ".data.repository",
        ];
        let printed = ask_gh(gh, project.repo()?, &args).await;
        let printed = printed.map_err(|GhFailed(said)| refused(gh_words(&said)))?;
        let allowed: Allowed = serde_json::from_str(&printed)?;
        lock(&self.merging.allowed).insert(project.id, allowed.clone());
        Ok(allowed)
    }

    /// Merges the pull request of the branch a task's worktree is on, with `method`, `when`
    /// asked, if its head is still `head`, the one the user saw: through the user's own `gh`,
    /// which merges as them. No branch is deleted. Refused with the reason for a pull request
    /// that is not open, a draft, or moved on; for a merge that is not forced while a check
    /// failed; and for auto-merge where GitHub would merge it now. GitHub's refusals are given
    /// in its words. The pull request is read again after, and taken in as the follower's
    /// readings are, so that a merge is recorded once; the method is remembered for the
    /// repository.
    pub async fn task_pull_merge(
        &self,
        project: &str,
        name: &str,
        method: MergeMethod,
        when: MergeWhen,
        head: &str,
    ) -> Result<MergeReport> {
        let (project, task) = self.repo_task(project, name)?;
        let gh = gh_in(&self.tool_dirs)?;
        let pull = self.pull_now(&gh, &task).await?;
        if let Some(why) = refusal(&pull, when, head) {
            return Err(refused(why));
        }
        let number = pull.number.to_string();
        let flag = format!("--{}", method.name());
        let mut args = vec!["pr", "merge", &number, &flag, "--match-head-commit", head];
        args.extend(when.flag());
        let asked = ask_gh_within(&gh, &task.worktree_path, &args, MERGE_TIMEOUT).await;
        let after = self.pull_again(&gh, &task, pull.number).await;
        if let Err(GhFailed(said)) = asked
            && !after.as_ref().is_some_and(|after| done_anyway(when, after))
        {
            let said = gh_words(&said);
            return Err(refused(format!("#{number} was not merged: {said}")));
        }
        self.store().project_set_merge_method(project.id, method)?;
        Ok(MergeReport {
            outcome: outcome_of(when, after.as_ref()),
            pull: after,
        })
    }

    /// Turns off the auto-merge of the pull request of the branch a task's worktree is on,
    /// so that GitHub does not merge it by itself. Nothing to do when it is off already.
    pub async fn task_pull_auto_merge_off(&self, project: &str, name: &str) -> Result<MergeReport> {
        let (_, task) = self.repo_task(project, name)?;
        let gh = gh_in(&self.tool_dirs)?;
        let pull = self.pull_now(&gh, &task).await?;
        if pull.auto_merge.is_none() {
            return Ok(MergeReport {
                outcome: MergeOutcome::AutoMergeOff,
                pull: Some(pull),
            });
        }
        let number = pull.number.to_string();
        let args = ["pr", "merge", &number, "--disable-auto"];
        let asked = ask_gh(&gh, &task.worktree_path, &args).await;
        let after = self.pull_again(&gh, &task, pull.number).await;
        if let Err(GhFailed(said)) = asked
            && after
                .as_ref()
                .is_none_or(|after| after.auto_merge.is_some())
        {
            let said = gh_words(&said);
            return Err(refused(format!(
                "auto-merge of #{number} is still on: {said}"
            )));
        }
        Ok(MergeReport {
            outcome: MergeOutcome::AutoMergeOff,
            pull: after,
        })
    }

    /// The pull request of the branch a task's worktree is on, read now and taken in.
    async fn pull_now(&self, gh: &Path, task: &Task) -> Result<Pull> {
        let worktree = &task.worktree_path;
        let branch = git::current_branch(worktree).await?;
        let branch = branch.ok_or_else(|| {
            refused("this worktree is on no branch, which has no pull request to merge")
        })?;
        let printed = ask_gh(gh, worktree, &["pr", "view", &branch, "--json", FIELDS]).await;
        let reading = parse(&printed.map_err(|GhFailed(said)| refused(gh_words(&said)))?)?;
        let read = reading.pull.clone();
        // What is kept keeps what GitHub has not worked out yet from the reading before.
        let kept = self.pull_take_in(task, Some(reading), None)?;
        Ok(kept
            .map(|seen| seen.pull)
            .filter(|pull| pull.number == read.number)
            .unwrap_or(read))
    }

    /// Pull request `number` read again after a merge and taken in, which records the merge
    /// once; nothing if `gh` gives no answer.
    async fn pull_again(&self, gh: &Path, task: &Task, number: u64) -> Option<Pull> {
        let number = number.to_string();
        let args = ["pr", "view", &number, "--json", FIELDS];
        let printed = ask_gh(gh, &task.worktree_path, &args).await.ok()?;
        let reading = parse(&printed).ok()?;
        let read = reading.pull.clone();
        let kept = self.pull_take_in(task, Some(reading), None).ok().flatten();
        Some(kept.map_or(read, |seen| seen.pull))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pulls::Check;

    fn pull(state: PullState, outcomes: &[CheckOutcome]) -> Pull {
        let check = |(at, outcome): (usize, &CheckOutcome)| Check {
            name: format!("check {at}"),
            workflow: None,
            outcome: *outcome,
            url: None,
        };
        Pull {
            number: 142,
            title: "fix: the login".to_string(),
            url: "https://github.com/o/r/pull/142".to_string(),
            state,
            draft: false,
            review: None,
            checks: outcomes.iter().enumerate().map(check).collect(),
            conflicts: false,
            head: "0123456789abcdef".to_string(),
            base: "main".to_string(),
            auto_merge: None,
            merge_state: Some("blocked".to_string()),
            merged_at: None,
        }
    }

    const HEAD: &str = "0123456789abcdef";

    #[test]
    fn only_an_open_pull_request_at_the_head_that_was_seen_is_merged() {
        let open = pull(PullState::Open, &[CheckOutcome::Passed]);
        assert_eq!(refusal(&open, MergeWhen::Now, HEAD), None);
        let merged = pull(PullState::Merged, &[]);
        let said = refusal(&merged, MergeWhen::Force, HEAD);
        assert_eq!(said.as_deref(), Some("#142 is merged already"));
        let closed = pull(PullState::Closed, &[]);
        assert_eq!(
            refusal(&closed, MergeWhen::Bypass, HEAD).as_deref(),
            Some("#142 is closed")
        );
        let mut draft = open.clone();
        draft.draft = true;
        assert!(refusal(&draft, MergeWhen::Bypass, HEAD).is_some_and(|why| why.contains("draft")));
        let moved = refusal(&open, MergeWhen::Bypass, "fedcba9876543210").expect("refused");
        assert!(moved.contains("its head is 0123456 now"), "{moved}");
    }

    #[test]
    fn failed_checks_need_a_forced_merge() {
        use CheckOutcome::{Failed, Passed, Running, Skipped};
        let open = pull(PullState::Open, &[Passed, Failed, Running, Skipped, Failed]);
        let said = refusal(&open, MergeWhen::Now, HEAD).expect("refused");
        assert!(
            said.starts_with("checks failed on #142: check 1, check 4."),
            "{said}"
        );
        assert!(said.contains("A forced merge merges it anyway"), "{said}");
        assert_eq!(refusal(&open, MergeWhen::Force, HEAD), None);
        assert_eq!(refusal(&open, MergeWhen::Bypass, HEAD), None);
        // Running checks are GitHub's to wait for, as its own button does.
        let running = pull(PullState::Open, &[Passed, Running]);
        assert_eq!(refusal(&running, MergeWhen::Now, HEAD), None);
    }

    #[test]
    fn auto_merge_is_only_for_requirements_still_to_be_met() {
        let mut open = pull(PullState::Open, &[CheckOutcome::Failed]);
        assert_eq!(refusal(&open, MergeWhen::Auto, HEAD), None);
        for state in ["clean", "unstable", "has_hooks"] {
            open.merge_state = Some(state.to_string());
            let said = refusal(&open, MergeWhen::Auto, HEAD).expect("refused");
            assert!(said.contains("can be merged now"), "{said}");
        }
        open.merge_state = None;
        assert_eq!(refusal(&open, MergeWhen::Auto, HEAD), None);
    }

    #[test]
    fn what_came_of_it_is_read_from_the_pull_request_after() {
        let merged = pull(PullState::Merged, &[]);
        let open = pull(PullState::Open, &[]);
        assert_eq!(
            outcome_of(MergeWhen::Now, Some(&merged)),
            MergeOutcome::Merged
        );
        assert_eq!(
            outcome_of(MergeWhen::Force, Some(&open)),
            MergeOutcome::Queued
        );
        assert_eq!(
            outcome_of(MergeWhen::Auto, Some(&open)),
            MergeOutcome::AutoMerge
        );
        assert_eq!(
            outcome_of(MergeWhen::Auto, Some(&merged)),
            MergeOutcome::Merged
        );
        assert_eq!(outcome_of(MergeWhen::Bypass, None), MergeOutcome::Asked);
        assert!(done_anyway(MergeWhen::Now, &merged));
        assert!(!done_anyway(MergeWhen::Now, &open));
        let mut auto = open.clone();
        auto.auto_merge = Some(MergeMethod::Squash);
        assert!(done_anyway(MergeWhen::Auto, &auto));
    }

    #[test]
    fn the_method_starts_on_the_last_used_then_the_users_then_the_first() {
        let allowed = Allowed {
            merge_commit: false,
            squash: true,
            rebase: true,
            auto_merge: true,
            admin: false,
            users_method: "REBASE".to_string(),
        };
        let found = rules(&allowed, Some(MergeMethod::Squash));
        assert_eq!(found.methods, [MergeMethod::Squash, MergeMethod::Rebase]);
        assert_eq!(found.method, Some(MergeMethod::Squash));
        assert!(found.auto_merge);
        assert!(!found.admin);
        // One no longer allowed gives way to the user's own default.
        let found = rules(&allowed, Some(MergeMethod::Merge));
        assert_eq!(found.method, Some(MergeMethod::Rebase));
        let theirs_off = Allowed {
            users_method: "MERGE".to_string(),
            ..allowed
        };
        assert_eq!(rules(&theirs_off, None).method, Some(MergeMethod::Squash));
        assert_eq!(rules(&Allowed::default(), None).method, None);
    }

    #[test]
    fn what_gh_says_is_kept_without_its_marks_and_hints() {
        let said = "X Pull request o/r#142 is not mergeable: the base branch policy prohibits \
                    the merge.\nhint: if you need to wait for this pull request, use this:\n    \
                    gh event listen --resource x\n  - It prints the first event\nTo use \
                    administrator privileges, add the `--admin` flag.\n";
        assert_eq!(
            gh_words(said),
            "Pull request o/r#142 is not mergeable: the base branch policy prohibits the \
             merge.\nTo use administrator privileges, add the `--admin` flag."
        );
        assert_eq!(MergeMethod::of_github("SQUASH"), Some(MergeMethod::Squash));
        assert_eq!(MergeMethod::of_github("FAST_FORWARD"), None);
    }
}
