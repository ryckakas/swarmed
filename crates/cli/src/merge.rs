//! `swarmed task merge`: merges a task's pull request, or has GitHub merge it once its
//! requirements are met.

use anyhow::{Result, anyhow};
use clap::Args;
use swarmed_core::{Core, MergeMethod, MergeOutcome, MergeReport, MergeWhen};

use crate::say;

/// What `swarmed task merge` is given.
#[derive(Debug, Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a flag of its own on the command line"
)]
pub(crate) struct MergeArgs {
    project: String,
    name: String,
    /// Merge with a merge commit. Without a method, the one last used for the repository,
    /// else your own default on GitHub, else the first it allows.
    #[arg(long, group = "method")]
    merge: bool,
    /// Squash its commits into one.
    #[arg(long, group = "method")]
    squash: bool,
    /// Rebase its commits onto the base.
    #[arg(long, group = "method")]
    rebase: bool,
    /// Merge although checks failed. GitHub still refuses where a rule requires them.
    #[arg(long, group = "when")]
    force: bool,
    /// Merge past the repository's requirements, as only its admins can.
    #[arg(long, group = "when")]
    admin: bool,
    /// Turn on GitHub's auto-merge: GitHub merges it once its requirements are met.
    #[arg(long, group = "when")]
    auto: bool,
    /// Turn GitHub's auto-merge off again.
    #[arg(long, group = "when", conflicts_with = "method")]
    cancel_auto: bool,
}

impl MergeArgs {
    fn method(&self) -> Option<MergeMethod> {
        let flags = [self.merge, self.squash, self.rebase];
        (MergeMethod::ALL.into_iter().zip(flags)).find_map(|(method, on)| on.then_some(method))
    }

    fn when(&self) -> MergeWhen {
        match (self.force, self.admin, self.auto) {
            (true, _, _) => MergeWhen::Force,
            (_, true, _) => MergeWhen::Bypass,
            (_, _, true) => MergeWhen::Auto,
            _ => MergeWhen::Now,
        }
    }
}

/// What came of a merge, in one line.
fn said(report: &MergeReport, number: u64, method: Option<MergeMethod>) -> String {
    let base = report
        .pull
        .as_ref()
        .map_or("its base", |pull| pull.base.as_str());
    match report.outcome {
        MergeOutcome::Merged => format!("merged #{number} into {base}"),
        MergeOutcome::Queued => {
            format!("#{number} is in the merge queue of {base}, which merges it")
        }
        MergeOutcome::AutoMerge => {
            let with = method.map_or(String::new(), |method| format!(" ({})", method.name()));
            format!(
                "auto-merge is on for #{number}{with}: GitHub merges it once its requirements are met"
            )
        }
        MergeOutcome::AutoMergeOff => format!("auto-merge is off for #{number}"),
        MergeOutcome::Asked => {
            format!("GitHub took the merge of #{number}; `swarmed task pull` shows what came of it")
        }
    }
}

/// Merges a task's pull request as it is now, or turns its auto-merge on or off.
pub(crate) async fn run(core: &Core, args: &MergeArgs) -> Result<()> {
    let (project, name) = (args.project.as_str(), args.name.as_str());
    let pull = core.task_pull(project, name).await?;
    let pull = pull.ok_or_else(|| anyhow!("`{name}` has no pull request"))?;
    if args.cancel_auto {
        let report = core.task_pull_auto_merge_off(project, name).await?;
        say(&said(&report, pull.number, None));
        return Ok(());
    }
    let method = match args.method() {
        Some(method) => method,
        None => core
            .project_merge_rules(project)
            .await?
            .method
            .ok_or_else(|| anyhow!("the repository allows no way to merge"))?,
    };
    say(&format!("#{} {}  {}", pull.number, pull.title, pull.url));
    // The head just read is the one merged: one pushed since is refused.
    let when = args.when();
    let report = core
        .task_pull_merge(project, name, method, when, &pull.head)
        .await?;
    say(&said(&report, pull.number, Some(method)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_came_of_it_is_one_line() {
        let report = |outcome| MergeReport {
            outcome,
            pull: None,
        };
        assert_eq!(
            said(&report(MergeOutcome::Merged), 142, None),
            "merged #142 into its base"
        );
        assert!(said(&report(MergeOutcome::Queued), 142, None).contains("merge queue"));
        let auto = said(
            &report(MergeOutcome::AutoMerge),
            7,
            Some(MergeMethod::Squash),
        );
        assert!(
            auto.starts_with("auto-merge is on for #7 (squash)"),
            "{auto}"
        );
    }
}
