//! `swarmed mcp builtin`: Swarmed's built-in MCP servers, repolore so far.
//!
//! The command never opens repolore's page: that is the app's, which ends it as it quits.
//! `status` prints the line that opens it from a terminal instead.

use anyhow::Result;
use clap::{Args, Subcommand};
use swarmed_core::{BuiltinMcpView, BuiltinState, Core, REPOLORE};

use crate::{hook_line, say};

#[derive(Debug, Subcommand)]
pub(crate) enum BuiltinCommand {
    /// Show how the built-in repolore stands, without asking the network. The default.
    Status,
    /// Install repolore with uv, or upgrade it, installing uv with Homebrew if it is missing.
    Install,
    /// Turn it on: from a copy of your own setup, or fresh, to set up on its page.
    On(OnArgs),
    /// Turn it off. Its home stays, for turning it on again.
    Off,
    /// Check it again; with `--full`, GitHub and Claude Code's connection to it too.
    Check {
        #[arg(long)]
        full: bool,
    },
    /// Take out one of repolore's registrations elsewhere, by the key `status` lists it under.
    /// Only Claude Code's and Codex's own remove commands are run; the rest are your steps.
    Remove { key: String },
    /// Stop the agents of the idle sessions that still use the old repolore. Each starts again
    /// with its next message.
    Restart,
    /// Move your setup from before to the Trash, once the built-in is on and passes its check.
    TrashOld,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub(crate) struct OnArgs {
    /// Copy your own setup: its repositories and index. It stays as it is.
    #[arg(long)]
    copy: bool,
    /// Start with an empty home, set up on repolore's page.
    #[arg(long)]
    fresh: bool,
}

/// Runs one `swarmed mcp builtin` command, then shows how the built-in stands.
pub(crate) async fn run(core: &Core, command: Option<BuiltinCommand>) -> Result<()> {
    let view = match command.unwrap_or(BuiltinCommand::Status) {
        BuiltinCommand::Status => core.builtin_mcp_servers().await?,
        BuiltinCommand::Install => {
            say(&format!("installing {REPOLORE}"));
            vec![core.builtin_mcp_install(REPOLORE, &hook_line).await?]
        }
        BuiltinCommand::On(args) => vec![core.builtin_mcp_turn_on(REPOLORE, args.copy).await?],
        BuiltinCommand::Off => vec![core.builtin_mcp_turn_off(REPOLORE).await?],
        BuiltinCommand::Check { full } => vec![core.builtin_mcp_check(REPOLORE, full).await?],
        BuiltinCommand::Remove { key } => {
            let removed = core.builtin_mcp_remove_registration(REPOLORE, &key, &hook_line);
            vec![removed.await?]
        }
        BuiltinCommand::Restart => vec![core.builtin_mcp_restart_sessions(REPOLORE).await?],
        BuiltinCommand::TrashOld => vec![core.builtin_mcp_trash_old(REPOLORE).await?],
    };
    view.iter().for_each(show);
    Ok(())
}

/// A word as `sh` reads it back as itself.
fn for_shell(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

fn show(view: &BuiltinMcpView) {
    let version = view.version.as_deref().unwrap_or("-");
    say(&format!(
        "{}\t{}\t{version}\t(at least {})",
        view.name,
        view.state.as_str(),
        view.minimum
    ));
    if matches!(view.state, BuiltinState::Missing | BuiltinState::Outdated) {
        say(&format!("install\t{}", view.install_command));
    }
    let ready = if view.home_ready {
        "set up"
    } else {
        "not set up"
    };
    say(&format!("home\t{}\t{ready}", view.home));
    show_index(view);
    show_elsewhere(view);
    for check in &view.checks {
        let how = match (check.ok, check.warning) {
            (true, true) => "warning",
            (true, false) => "ok",
            (false, _) => "failed",
        };
        let fix = check.fix.as_deref().unwrap_or("");
        say(&format!(
            "check\t{how}\t{}\t{}\t{fix}",
            check.name, check.detail
        ));
    }
    if let Some(problem) = &view.problem {
        say(&format!("problem\t{problem}"));
    }
    // What opens its page from a terminal, where the command has no page of its own.
    let home = for_shell(&view.home);
    say(&format!(
        "page\tREPOLORE_HOME={home} REPOLORE_MANAGED_BY=Swarmed {REPOLORE} ui"
    ));
}

/// When the index was built and refreshed, how it was checked, and what Claude Code said.
fn show_index(view: &BuiltinMcpView) {
    if let Some(built) = &view.built_at {
        let refreshed = view.refreshed_at.as_deref().unwrap_or("-");
        say(&format!("index\tbuilt {built}\trefreshed {refreshed}"));
    }
    if let Some(checked) = view.checked {
        say(&format!("checked\t{}", checked.as_str()));
    }
    if let Some(status) = &view.status {
        say(&format!("status\t{status}"));
    }
}

/// What else there is of repolore: the setup from before, its registrations, its server, the
/// sessions that use it, and the hook in Claude Code's settings.
fn show_elsewhere(view: &BuiltinMcpView) {
    if let Some(existing) = &view.existing {
        let indexed = if existing.indexed {
            "indexed"
        } else {
            "not indexed"
        };
        say(&format!(
            "existing\t{}\t{indexed}",
            existing.folders.join(", ")
        ));
    }
    for registration in &view.registrations {
        let how = if registration.runs_here {
            registration.command.clone()
        } else {
            registration
                .step
                .clone()
                .or_else(|| registration.command.clone())
        };
        say(&format!(
            "registration\t{}\t{}\t{}\t{}",
            registration.key,
            registration.client,
            registration.scope,
            how.unwrap_or_default()
        ));
    }
    if let Some(server) = view.old_server {
        let pid = server
            .pid
            .map_or_else(|| "-".to_string(), |pid| pid.to_string());
        say(&format!(
            "old server\tpid {pid}\t{} relay(s)",
            server.relays
        ));
    }
    for held in &view.sessions {
        let idle = if held.idle { "idle" } else { "busy" };
        say(&format!("held\t{}\t{idle}\t{}", held.id, held.title));
    }
    if view.hook {
        say(
            "hook\tClaude Code's settings.json runs `repolore session-start`, which the \
             built-in does by itself: remove that hook yourself",
        );
    }
}
