//! `swarmed issue`: a repository's GitHub issues, listed or read whole, through your own `gh`.

use anyhow::Result;
use clap::{Args, Subcommand};
use swarmed_core::{Core, ISSUES_LISTED, Issue, IssueDetail, IssueFilter, IssueState, IssueWho};

use crate::{aside, printable, say};

#[derive(Debug, Subcommand)]
pub(crate) enum IssueCommand {
    /// List a repository's open issues, the latest changed first. Without `gh`, or for a
    /// repository that is not on GitHub, nothing is listed and that is said.
    List(ListArgs),
    /// Print one issue whole: what it says, and its comments.
    Show {
        project: String,
        /// The issue's number.
        number: u64,
    },
}

/// What `swarmed issue list` is given.
#[derive(Debug, Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a flag of its own on the command line"
)]
pub(crate) struct ListArgs {
    project: String,
    /// Only those assigned to you.
    #[arg(long, group = "who")]
    mine: bool,
    /// Only those you opened.
    #[arg(long, group = "who")]
    created: bool,
    /// Only those that mention you.
    #[arg(long, group = "who")]
    mentioned: bool,
    /// Closed ones instead of open ones.
    #[arg(long)]
    closed: bool,
    /// Only those with these words, in GitHub's search syntax: `label:bug login`.
    #[arg(long)]
    search: Option<String>,
    /// How many at most, up to 200.
    #[arg(long, default_value_t = ISSUES_LISTED)]
    limit: u32,
}

impl ListArgs {
    fn filter(&self) -> IssueFilter {
        let who = match (self.mine, self.created, self.mentioned) {
            (true, _, _) => IssueWho::Assigned,
            (_, true, _) => IssueWho::Authored,
            (_, _, true) => IssueWho::Mentioned,
            _ => IssueWho::Anyone,
        };
        IssueFilter {
            who,
            state: if self.closed {
                IssueState::Closed
            } else {
                IssueState::Open
            },
            search: self.search.clone().unwrap_or_default(),
            limit: self.limit,
        }
    }
}

pub(crate) async fn run(core: &Core, command: IssueCommand) -> Result<()> {
    match command {
        IssueCommand::List(args) => list(core, &args).await,
        IssueCommand::Show { project, number } => {
            show(&core.project_issue(&project, number).await?);
            Ok(())
        }
    }
}

/// One line for an issue, its fields apart by tabs: number, state, title, labels.
fn row(issue: &Issue) -> String {
    let state = match issue.state {
        IssueState::Open => "open",
        IssueState::Closed => "closed",
    };
    let line = format!(
        "#{}\t{state}\t{}\t{}",
        issue.number,
        issue.title.replace('\t', " "),
        issue.labels.join(", ")
    );
    printable(&line)
}

async fn list(core: &Core, args: &ListArgs) -> Result<()> {
    let listed = core.project_issues(&args.project, &args.filter()).await?;
    if let Some(why) = listed.unavailable {
        aside(&format!("no issues to list: {}", printable(&why)));
        return Ok(());
    }
    if listed.issues.is_empty() {
        aside("no issues match");
    }
    for issue in &listed.issues {
        say(&row(issue));
    }
    Ok(())
}

/// Prints an issue whole: its heading, what it says, and each comment under who wrote it.
fn show(detail: &IssueDetail) {
    let issue = &detail.issue;
    say(&row(issue));
    say(&issue.url);
    let mut about = vec![format!("opened by {}", or_gone(&issue.author))];
    if !issue.assignees.is_empty() {
        about.push(format!("assigned to {}", issue.assignees.join(", ")));
    }
    say(&printable(&about.join("; ")));
    if !detail.body.trim().is_empty() {
        say("");
        say(&printable(detail.body.trim_end()));
    }
    for comment in &detail.comments {
        say("");
        say(&printable(&format!(
            "--- {}, {}",
            or_gone(&comment.author),
            comment.created_at
        )));
        match &comment.hidden {
            Some(why) => say(&printable(&format!("(hidden on GitHub: {why})"))),
            None => say(&printable(comment.body.trim_end())),
        }
    }
}

/// A login, or what stands for an account that is gone.
fn or_gone(login: &str) -> &str {
    if login.is_empty() {
        "a deleted account"
    } else {
        login
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_never_reach_the_terminal() {
        let said = "title\u{1b}]0;owned\u{7}\u{1b}[31m red\n\tnext";
        assert_eq!(printable(said), "title]0;owned[31m red\n\tnext");
    }

    #[test]
    fn a_row_keeps_its_fields_apart() {
        let issue = Issue {
            number: 12,
            title: "Login\tloops".to_string(),
            state: IssueState::Closed,
            url: "u".to_string(),
            labels: vec!["bug".to_string(), "ui".to_string()],
            assignees: vec![],
            author: String::new(),
            updated_at: String::new(),
        };
        assert_eq!(row(&issue), "#12\tclosed\tLogin loops\tbug, ui");
    }
}
