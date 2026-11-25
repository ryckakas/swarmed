//! `swarmed session rewind` and `swarmed session put-back`: a session taken back to before one
//! of its messages, its files too if asked, and those files put back.

use anyhow::Result;
use clap::Args;
use swarmed_core::{Core, RewindCheck};

use crate::say;
use crate::session::kind_mark;

/// What `swarmed session rewind` is given.
#[derive(Debug, Args)]
pub(crate) struct RewindArgs {
    id: i64,
    /// The message to go back to before, counting the messages sent into the session from 0.
    message: usize,
    /// Take the worktree's files back to how they were before that message too.
    #[arg(long)]
    files: bool,
    /// Only say what it would do, and change nothing.
    #[arg(long, conflicts_with = "files")]
    check: bool,
}

/// Takes a session back to before one of its messages, or says what that would do.
pub(crate) async fn rewind(core: &Core, args: RewindArgs) -> Result<()> {
    let RewindArgs {
        id,
        message,
        files,
        check,
    } = args;
    if check {
        print_check(&core.session_rewind_check(id, message).await?);
        return Ok(());
    }
    let done = core.session_rewind(id, message, files).await?;
    let agent = if done.resumes {
        "its agent resumes its conversation there"
    } else {
        "its agent starts a new conversation and is told what was said up to there"
    };
    say(&format!(
        "taken back to before message {}: {agent}, with your next message",
        done.to
    ));
    if done.files {
        say("the files are as they were before it; `swarmed session put-back` puts them back");
    }
    if let Some(commits) = done.commits_since.filter(|commits| *commits > 0) {
        say(&format!(
            "{commits} commit(s) made since stay on the branch: only the files went back"
        ));
    }
    Ok(())
}

/// Prints what a rewind would do now.
fn print_check(check: &RewindCheck) {
    if let Some(reason) = &check.refused {
        say(&format!("refused: {reason}"));
        return;
    }
    say(if check.resumes {
        "conversation: its agent would resume it at that message"
    } else {
        "conversation: its agent would start a new one, told what was said up to that message"
    });
    if let Some(reason) = &check.files_refused {
        say(&format!("files: cannot go back: {reason}"));
        return;
    }
    say(&format!(
        "files: {} would go back (--files)",
        check.files.len()
    ));
    for file in &check.files {
        say(&format!("\t{}\t{}", kind_mark(file.kind), file.path));
    }
    if let Some(commits) = check.commits_since.filter(|commits| *commits > 0) {
        say(&format!(
            "{commits} commit(s) made since would stay on the branch"
        ));
    }
}

/// Puts back the files the session's latest rewind took back.
pub(crate) async fn put_back(core: &Core, id: i64) -> Result<()> {
    core.session_files_put_back(id).await?;
    say("the files are back as they were before the rewind");
    Ok(())
}

/// The line `swarmed session show` gives a rewind: what follows it in the history up to it, from
/// message `to` on, no longer counts.
pub(crate) fn rewound(to: usize, files: bool) -> String {
    let files = if files { ", files too" } else { "" };
    format!(
        "— taken back to before message {to}{files}: what stands above from there on no longer counts"
    )
}
