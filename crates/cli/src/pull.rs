//! `swarmed task pull`: a task's pull request and its checks, once or followed.

use std::time::Duration;

use anyhow::Result;
use clap::Args;
use swarmed_core::agent::AgentEvent;
use swarmed_core::{CheckOutcome, Core, Pull, PullChange, SessionEvent, pull_wait};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;

use crate::{aside, say};

/// How often what other Swarmed processes wrote is looked for while following, as the app
/// does: the app may read the pull request first and record what changed.
const LOOK_EVERY: Duration = Duration::from_secs(1);

/// Prints a pull request and its checks.
fn print(pull: &Pull) {
    let draft = if pull.draft { ", draft" } else { "" };
    let conflicts = if pull.conflicts {
        ", conflicts with its base"
    } else {
        ""
    };
    say(&format!(
        "#{} {:?}{draft}{conflicts}  {}\n{}",
        pull.number, pull.state, pull.title, pull.url
    ));
    for check in &pull.checks {
        say(&format!("{:?}\t{}", check.outcome, check.name));
    }
}

/// What `swarmed task pull` is given.
#[derive(Debug, Args)]
pub(crate) struct PullArgs {
    project: String,
    name: String,
    /// Keep following it until Ctrl-C, and print each change: checks failing or all passing,
    /// a review or a comment from someone else, a conflict, a merge or a close.
    #[arg(long)]
    follow: bool,
}

/// Prints a task's pull request and its checks, once, or follows it.
pub(crate) async fn run(core: &Core, args: &PullArgs) -> Result<()> {
    let PullArgs {
        project,
        name,
        follow,
    } = args;
    if *follow {
        self::follow(core, project, name).await
    } else {
        show(core, project, name).await
    }
}

/// Prints a task's pull request and its checks, once.
async fn show(core: &Core, project: &str, name: &str) -> Result<()> {
    match core.task_pull(project, name).await? {
        Some(pull) => print(&pull),
        None => say("no pull request"),
    }
    Ok(())
}

/// Follows a task's pull request until Ctrl-C: prints it, then each change as it is heard,
/// whichever Swarmed read it, and a line whenever its checks stand otherwise. Looks as often
/// as the app's follower would, later and later while `gh` fails.
async fn follow(core: &Core, project: &str, name: &str) -> Result<()> {
    let (_, task) = core.task(project, name)?;
    let mut bus = core.subscribe();
    let first = core.task_pull_look(project, name).await?;
    match &first {
        Some(pull) => print(pull),
        None => say("no pull request: the worktree is on no branch"),
    }
    let mut next = Instant::now() + pull_wait(first.as_ref(), 0);
    let mut failures = 0;
    let mut looks = tokio::time::interval(LOOK_EVERY);
    loop {
        tokio::select! {
            published = bus.recv() => match published {
                Ok(published) => heard(core, task.id, &published),
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(()),
            },
            _ = looks.tick() => {
                // One that failed is made again at the next look.
                core.notice_others().ok();
                if Instant::now() < next {
                    continue;
                }
                let looked = core.task_pull_look(project, name).await;
                match &looked {
                    Ok(_) => failures = 0,
                    Err(error) => {
                        if failures == 0 {
                            aside(&format!("could not read the pull request: {error}"));
                        }
                        failures += 1;
                    }
                }
                let pull = looked.ok().flatten();
                next = Instant::now() + pull_wait(pull.as_ref(), failures);
            },
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
}

/// Prints what an event says of task `task`'s pull request, if it says anything of it.
fn heard(core: &Core, task: i64, published: &SessionEvent) {
    match &published.event {
        AgentEvent::PullRead {
            task_id,
            pull: Some(pull),
        } if *task_id == task => say(&standing(pull)),
        AgentEvent::PullChanged { number, change, .. } => {
            let ours = core.session(published.session_id);
            if ours.is_ok_and(|session| session.task_id == task) {
                say(&changed(*number, change));
            }
        }
        _ => {}
    }
}

/// How a pull request stands, in one line: `#142 Open: 3 passed, 1 failed, 1 running`.
fn standing(pull: &Pull) -> String {
    let count = |outcome| pull.checks.iter().filter(|c| c.outcome == outcome).count();
    let counts = [
        (CheckOutcome::Passed, "passed"),
        (CheckOutcome::Failed, "failed"),
        (CheckOutcome::Running, "running"),
    ];
    let said: Vec<String> = counts
        .into_iter()
        .map(|(outcome, word)| (count(outcome), word))
        .filter(|(n, _)| *n > 0)
        .map(|(n, word)| format!("{n} {word}"))
        .collect();
    let checks = if said.is_empty() {
        "no checks".to_string()
    } else {
        said.join(", ")
    };
    format!("#{} {:?}: {checks}", pull.number, pull.state)
}

/// What changed on pull request `number`, in one line: `pull request #142: checks failed:
/// Test`. Who, never what they wrote.
pub(crate) fn changed(number: u64, change: &PullChange) -> String {
    let what = match change {
        PullChange::ChecksFailed { checks } => format!("checks failed: {}", checks.join(", ")),
        PullChange::ChecksPassed => "all checks passed".to_string(),
        PullChange::ChangesRequested { by } => format!("changes requested by {}", by.join(", ")),
        PullChange::Approved { by } => format!("approved by {}", by.join(", ")),
        PullChange::Reviewed { by } => format!("reviewed by {}", by.join(", ")),
        PullChange::Commented { by } => format!("comment from {}", by.join(", ")),
        PullChange::Conflicts => "conflicts with its base".to_string(),
        PullChange::Merged => "merged".to_string(),
        PullChange::Closed => "closed".to_string(),
    };
    format!("pull request #{number}: {what}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_one_line_that_names_who_and_which() {
        let checks = vec!["Test".to_string(), "Lint".to_string()];
        assert_eq!(
            changed(142, &PullChange::ChecksFailed { checks }),
            "pull request #142: checks failed: Test, Lint"
        );
        let by = vec!["alice".to_string()];
        assert_eq!(
            changed(7, &PullChange::Commented { by }),
            "pull request #7: comment from alice"
        );
        assert_eq!(changed(7, &PullChange::Merged), "pull request #7: merged");
    }
}
