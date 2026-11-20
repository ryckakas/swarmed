//! Settings that apply to every project on this machine.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::agent::Supervision;
use crate::error::{Error, Result};
use crate::statusline::StatusItem;

/// The user's own preferences, one TOML file in the data directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// The command that opens a folder in the user's code editor, such as `code` or `zed`.
    pub editor: String,
    /// Whether a new session is given a short title by a model once its first message is sent.
    /// Without it, the title stays the start of that message.
    pub auto_title: bool,
    /// Whether the app is dark, light, or whatever the system is.
    pub theme: Theme,
    /// Which set of colours the app is drawn in, in dark and in light alike.
    pub palette: Palette,
    /// Which icons and words the app uses, whatever its colours.
    pub style: Style,
    /// The size of the app's text, in percent of its normal size.
    pub text_scale: u16,
    /// How much a new session's agent may do without asking, unless chosen otherwise.
    pub default_supervision: Supervision,
    /// Whether the system shows a notification when a session needs the user, finishes or
    /// fails while they look elsewhere.
    pub notifications: bool,
    /// Whether agents are told they may answer with small HTML views, and those are shown.
    pub html_views: bool,
    /// The built-in pieces of the status line under the message box, in order.
    pub status_items: Vec<StatusItem>,
    /// A command whose first line of output replaces the built-in pieces. Empty for none.
    pub status_command: String,
    /// Rules of the user's own that every agent is given with its instructions, whatever the
    /// agent or model: plain text, such as "keep answers short". Empty for none.
    pub rules: String,
}

/// The longest the user's rules may be, in bytes: they travel with every session's
/// instructions.
pub(crate) const MAX_RULES_BYTES: usize = 20_000;

/// The colours the app is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Dark or light, following the system.
    #[default]
    System,
    /// Always dark.
    Dark,
    /// Always light.
    Light,
}

/// A set of colours for the app's surfaces, text and links. Each has a dark and a light form;
/// the colours that carry meaning are the same in all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    /// Blue-grey, the one Swarmed started with.
    Dusk,
    /// Grey without a tint.
    Graphite,
    /// Green-grey.
    Forest,
    /// Purple-grey.
    Plum,
    /// Warm brown-grey.
    Ember,
    /// Violet under magenta, like the inside of a hive. What a new installation starts in.
    #[default]
    Swarm,
}

/// The icons and words the app uses for sessions and what they are doing. Apart from the
/// palette, so either goes with any colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// Dots and the plain words: idle, working, unread.
    Plain,
    /// Sessions as eggs and larvae, named after a swarm: dormant, mutating, hatched.
    #[default]
    Swarm,
    /// The swarm's icons and words, and its names for the tree as well: a colony for a
    /// repository, burrowed for closed.
    Colony,
}

/// The smallest and largest text the app can be set to, in percent.
const TEXT_SCALE_RANGE: (u16, u16) = (80, 150);

impl Default for Settings {
    /// Visual Studio Code.
    fn default() -> Self {
        Self {
            editor: "code".to_string(),
            auto_title: true,
            theme: Theme::System,
            palette: Palette::Swarm,
            style: Style::Swarm,
            text_scale: 100,
            default_supervision: Supervision::Auto,
            notifications: true,
            html_views: true,
            status_items: StatusItem::defaults(),
            status_command: String::new(),
            rules: String::new(),
        }
    }
}

impl Settings {
    /// Loads the settings, or returns the defaults if the file does not exist.
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let settings: Self = toml::from_str(&text).map_err(|e| invalid(path, &e))?;
        Ok(settings.within_limits())
    }

    /// The settings with every value brought into the range the app can show.
    fn within_limits(mut self) -> Self {
        let (smallest, largest) = TEXT_SCALE_RANGE;
        self.text_scale = self.text_scale.clamp(smallest, largest);
        self
    }

    pub(crate) fn save(&self, path: &Path) -> Result<()> {
        if self.rules.len() > MAX_RULES_BYTES {
            return Err(Error::RulesTooLong(MAX_RULES_BYTES));
        }
        let text = toml::to_string(&self.clone().within_limits()).map_err(|e| invalid(path, &e))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, text)?;
        Ok(())
    }
}

fn invalid(path: &Path, error: &dyn std::fmt::Display) -> Error {
    Error::Config {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_save_and_default_to_vs_code() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("settings.toml");
        assert_eq!(Settings::load(&path).expect("defaults").editor, "code");

        let chosen = Settings {
            editor: "zed --new".to_string(),
            auto_title: false,
            theme: Theme::Light,
            palette: Palette::Forest,
            style: Style::Plain,
            text_scale: 110,
            default_supervision: Supervision::Ask,
            notifications: false,
            html_views: false,
            status_items: vec![StatusItem::Cost],
            status_command: "~/bin/status".to_string(),
            rules: "Keep answers short.\nNo em dashes.".to_string(),
        };
        chosen.save(&path).expect("saves");
        assert_eq!(Settings::load(&path).expect("loads"), chosen);

        let endless = Settings {
            rules: "x".repeat(MAX_RULES_BYTES + 1),
            ..Settings::default()
        };
        assert!(matches!(endless.save(&path), Err(Error::RulesTooLong(_))));
        assert_eq!(Settings::load(&path).expect("loads"), chosen);
    }

    #[test]
    fn text_size_stays_within_what_the_app_can_show() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "text_scale = 900\ntheme = \"dark\"").expect("writes");
        let loaded = Settings::load(&path).expect("loads");
        assert_eq!((loaded.text_scale, loaded.theme), (150, Theme::Dark));
    }

    #[test]
    fn a_misspelt_key_is_an_error_not_a_silent_default() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "editr = \"zed\"").expect("writes");
        assert!(matches!(Settings::load(&path), Err(Error::Config { .. })));
    }
}
