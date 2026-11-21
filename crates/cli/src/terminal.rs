//! The `terminal` commands: how the terminal looks and behaves.

use anyhow::Result;
use clap::Subcommand;
use swarmed_core::{Core, terminal_schema};

use crate::say;

#[derive(Debug, Subcommand)]
pub(crate) enum TerminalCommand {
    /// Print the terminal's configuration, as JSON.
    Show,
    /// Read a terminal configuration from standard input, as JSON, and keep it if it can be used.
    Set,
    /// Print the JSON Schema of a terminal configuration.
    Schema,
}

pub(crate) fn run(core: &Core, command: &TerminalCommand) -> Result<()> {
    let config = match command {
        TerminalCommand::Show => serde_json::to_value(core.terminal_config()?)?,
        TerminalCommand::Set => {
            let text = std::io::read_to_string(std::io::stdin())?;
            serde_json::to_value(core.set_terminal_config(&text)?)?
        }
        TerminalCommand::Schema => terminal_schema(),
    };
    say(&serde_json::to_string_pretty(&config)?);
    Ok(())
}
