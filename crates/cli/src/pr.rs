//! `swarmed pr`: a repository's pull requests and issues, as the `#` menu of the message box
//! offers them, through your own `gh`.

use anyhow::Result;
use clap::Subcommand;
use swarmed_core::{Core, GitHubItem, GitHubKind};

use crate::{aside, printable, say};

#[derive(Debug, Subcommand)]
pub(crate) enum PrCommand {
    /// List a repository's latest changed pull requests and issues, open or not, as `#` in
    /// the message box offers them: with a number, that one; with words, those GitHub's search
    /// finds. Without `gh`, or for a repository that is not on GitHub, nothing is listed and
    /// that is said.
    List {
        project: String,
        /// A number, or words in GitHub's search syntax.
        query: Option<String>,
    },
}

pub(crate) async fn run(core: &Core, command: PrCommand) -> Result<()> {
    match command {
        PrCommand::List { project, query } => {
            let query = query.unwrap_or_default();
            let listed = core.project_github_items(&project, &query).await?;
            if let Some(why) = listed.unavailable {
                aside(&format!("nothing to list: {}", printable(&why)));
                return Ok(());
            }
            if listed.items.is_empty() {
                aside("nothing matches");
            }
            for item in &listed.items {
                say(&row(item));
            }
            Ok(())
        }
    }
}

/// One line for a pull request or an issue, its fields apart by tabs: number, kind, state,
/// title, and a pull request's branches.
fn row(item: &GitHubItem) -> String {
    let kind = match item.kind {
        GitHubKind::Pull => "pull request",
        GitHubKind::Issue => "issue",
    };
    let branches = match (&item.head, &item.base) {
        (Some(head), Some(base)) => format!("{head} into {base}"),
        _ => String::new(),
    };
    let line = format!(
        "#{}\t{kind}\t{}\t{}\t{branches}",
        item.number,
        item.state.word(),
        item.title.replace('\t', " ")
    );
    printable(line.trim_end())
}
