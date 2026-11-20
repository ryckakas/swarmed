//! The `layout` commands: the user's custom layout.

use anyhow::Result;
use clap::Subcommand;
use swarmed_core::{Core, layout_schema};

use crate::say;

#[derive(Debug, Subcommand)]
pub(crate) enum LayoutCommand {
    /// Print the custom layout, as JSON.
    Show,
    /// Read a custom layout from standard input, as JSON, and keep it if it can be drawn.
    /// The app is drawn in it once Custom is chosen in Settings.
    Set,
    /// Print the JSON Schema of a custom layout.
    Schema,
}

pub(crate) fn run(core: &Core, command: &LayoutCommand) -> Result<()> {
    let layout = match command {
        LayoutCommand::Show => serde_json::to_value(core.custom_layout()?)?,
        LayoutCommand::Set => {
            let text = std::io::read_to_string(std::io::stdin())?;
            serde_json::to_value(core.set_custom_layout(&text)?)?
        }
        LayoutCommand::Schema => layout_schema(),
    };
    say(&serde_json::to_string_pretty(&layout)?);
    Ok(())
}
