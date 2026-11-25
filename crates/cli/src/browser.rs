//! `swarmed task browser`: the task's own Chrome, for tools that drive a browser.

use anyhow::Result;
use clap::Args;
use swarmed_core::Core;

use crate::say;

#[derive(Debug, Args)]
pub(crate) struct BrowserArgs {
    project: String,
    name: String,
    /// Stop it instead. Its profile stays, with what was signed in to there.
    #[arg(long)]
    stop: bool,
}

/// Starts the task's Chrome, or stops it, and says where it answers.
pub(crate) async fn run(core: &Core, args: &BrowserArgs) -> Result<()> {
    let (_, task) = core.task(&args.project, &args.name)?;
    if args.stop {
        core.browser_stop(task.id).await?;
        say("stopped");
        return Ok(());
    }
    let state = core.browser_open(task.id).await?;
    let none = String::new;
    say(&format!(
        "{}\t{}",
        state.endpoint.unwrap_or_else(none),
        state.websocket.unwrap_or_else(none)
    ));
    for tab in &state.tabs {
        say(&format!("tab\t{}\t{}", tab.url, tab.title));
    }
    say(&format!("downloads\t{}", state.downloads.display()));
    Ok(())
}
