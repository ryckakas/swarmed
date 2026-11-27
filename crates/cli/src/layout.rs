//! The `layout` commands: the user's custom layout, and turning their own stylesheet on or off.

use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use swarmed_core::{Core, Stylesheet, layout_schema};

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
    /// Turn your own stylesheet, `style.css` in the data folder, on or off. Off, the window is
    /// drawn without it from its next look, within a second: the way out when a stylesheet
    /// hides the switch in Settings.
    Stylesheet {
        /// Whether the window applies it.
        state: Switch,
    },
}

/// On or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Switch {
    On,
    Off,
}

pub(crate) fn run(core: &Core, command: &LayoutCommand) -> Result<()> {
    let layout = match command {
        LayoutCommand::Show => serde_json::to_value(core.custom_layout()?)?,
        LayoutCommand::Set => {
            let text = std::io::read_to_string(std::io::stdin())?;
            serde_json::to_value(core.set_custom_layout(&text)?)?
        }
        LayoutCommand::Schema => layout_schema(),
        LayoutCommand::Stylesheet { state } => {
            let sheet = core.stylesheet_use(*state == Switch::On)?;
            say(&stylesheet_said(&sheet));
            return Ok(());
        }
    };
    say(&serde_json::to_string_pretty(&layout)?);
    Ok(())
}

/// What became of the stylesheet, in a line.
fn stylesheet_said(sheet: &Stylesheet) -> String {
    if !sheet.on {
        return "Your stylesheet is off: the window is drawn without it.".to_string();
    }
    if sheet.file.is_none() {
        return "Your stylesheet is on, but there is no style.css to apply.".to_string();
    }
    let waiting = if sheet.waiting {
        " style.css changed since: the window asks you before applying that."
    } else {
        ""
    };
    format!("Your stylesheet is on: the window applies what you last saved or accepted.{waiting}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(on: bool, file: Option<&str>, waiting: bool) -> Stylesheet {
        Stylesheet {
            on,
            left_out: false,
            applied: String::new(),
            accepted: "a {}".to_string(),
            file: file.map(str::to_string),
            changed_at: None,
            waiting,
        }
    }

    #[test]
    fn what_became_of_the_stylesheet_is_said() {
        assert!(stylesheet_said(&sheet(false, Some("a {}"), true)).contains("off"));
        assert!(stylesheet_said(&sheet(true, None, false)).contains("no style.css"));
        let asks = stylesheet_said(&sheet(true, Some("b {}"), true));
        assert!(asks.contains("asks you"), "{asks}");
        let applies = stylesheet_said(&sheet(true, Some("a {}"), false));
        assert!(!applies.contains("asks"), "{applies}");
    }
}
