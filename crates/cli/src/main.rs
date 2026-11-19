//! The `swarmed` command: a thin front end over `swarmed-core`.

use clap::Parser;

/// Swarmed: parallel Claude Code sessions in isolated git worktrees.
#[derive(Debug, Parser)]
#[command(name = "swarmed", version)]
struct Cli {}

fn main() {
    Cli::parse();
}
