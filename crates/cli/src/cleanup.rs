//! The `cleanup` command: what removed tasks left behind, and the snapshots.

use anyhow::{Result, bail};
use clap::{Args, ValueEnum};
use swarmed_core::{CleanupItem, CleanupKind, Core};

use crate::{bytes, say};

/// What the command line calls each kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Kind {
    /// Removed tasks' worktrees that are not deleted yet, or could not be.
    Removed,
    /// Branches Swarmed made for tasks that are removed, kept when they were.
    Branches,
    /// Folders beside the tasks' worktrees that no task owns, and git's records of worktrees
    /// whose folder is gone.
    Stale,
    /// Snapshots: what new tasks of each environment start from.
    Snapshots,
}

impl Kind {
    fn core(self) -> CleanupKind {
        match self {
            Self::Removed => CleanupKind::Removed,
            Self::Branches => CleanupKind::Branches,
            Self::Stale => CleanupKind::Stale,
            Self::Snapshots => CleanupKind::Snapshots,
        }
    }

    fn of(kind: CleanupKind) -> Self {
        match kind {
            CleanupKind::Removed => Self::Removed,
            CleanupKind::Branches => Self::Branches,
            CleanupKind::Stale => Self::Stale,
            CleanupKind::Snapshots => Self::Snapshots,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Removed => "removed",
            Self::Branches => "branches",
            Self::Stale => "stale",
            Self::Snapshots => "snapshots",
        }
    }
}

#[derive(Debug, Args)]
pub(crate) struct CleanupArgs {
    /// The kind to list or delete; without it, every kind is listed.
    #[arg(value_enum)]
    kind: Option<Kind>,
    /// Delete what is listed of the kind: every item that loses no work, or only the keys
    /// given. An item marked `at risk` would lose work found nowhere else, and is deleted only
    /// when its key is given.
    #[arg(long, requires = "kind")]
    delete: bool,
    /// The keys of the items to delete, as listed.
    #[arg(requires = "delete")]
    keys: Vec<String>,
}

/// Lists what can be cleaned up, or with `--delete` deletes it and says what went and what did
/// not.
pub(crate) async fn run(core: &Core, args: CleanupArgs) -> Result<()> {
    let Some(kind) = args.kind else {
        for kind in CleanupKind::ALL {
            list(Kind::of(kind), &core.cleanup_items(kind).await?);
        }
        return Ok(());
    };
    if !args.delete {
        list(kind, &core.cleanup_items(kind.core()).await?);
        return Ok(());
    }
    let keys = (!args.keys.is_empty()).then_some(args.keys.as_slice());
    let cleaned = core.cleanup_clean(kind.core(), keys, None).await?;
    for key in &cleaned.deleted {
        say(&format!("deleted\t{key}"));
    }
    for kept in &cleaned.kept {
        say(&format!("kept\t{}\t{}", kept.key, kept.reason));
    }
    if !cleaned.kept.is_empty() {
        bail!("{} not deleted", cleaned.kept.len());
    }
    Ok(())
}

/// One item to a line: its kind, size, key, the task it was or was taken from, and why it is
/// there, `at risk` first where deleting it would lose work.
fn list(kind: Kind, items: &[CleanupItem]) {
    for item in items {
        let size = item.bytes.map_or_else(|| "-".to_string(), bytes);
        let task = item.task.as_deref().unwrap_or("-");
        let risk = if item.at_risk { "at risk: " } else { "" };
        say(&format!(
            "{}\t{size}\t{}\t{task}\t{risk}{}",
            kind.name(),
            item.key,
            item.reason
        ));
    }
}
