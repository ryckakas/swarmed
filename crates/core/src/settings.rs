//! Settings that apply to every project on this machine.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::agent::Supervision;
use crate::error::{Error, Result};
use crate::lenient::{self, Problems};
use crate::statusline::StatusItem;

/// The user's own preferences, one TOML file in the data directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// Unknown keys are read past and named (`lenient`), never refused: see `Settings::read`.
#[serde(default)]
pub struct Settings {
    /// The command that opens a folder in the user's code editor, such as `code` or `zed`.
    pub editor: String,
    /// The terminal application a task's worktree is opened in from the task panel, by its
    /// name: `Terminal`, `iTerm`, `Ghostty`.
    pub terminal_app: String,
    /// Whether a new session is given a short title by a model once its first message is sent.
    /// Without it, the title stays the start of that message.
    pub auto_title: bool,
    /// Whether the app is dark, light, or whatever the system is.
    pub theme: Theme,
    /// Which set of colours the app is drawn in, in dark and in light alike.
    pub palette: Palette,
    /// Which icons and words the app uses, whatever its colours.
    pub style: Style,
    /// How large and how far apart the app draws things, whatever its colours.
    pub layout: Layout,
    /// The size of the app's text, in percent of what its layout starts from.
    pub text_scale: u16,
    /// How much a new session's agent may do without asking, unless chosen otherwise.
    pub default_supervision: Supervision,
    /// Whether the system shows a notification when a session needs the user, finishes or
    /// fails while they look elsewhere.
    pub notifications: bool,
    /// Whether agents are told they may answer with small HTML views, and those are shown.
    pub html_views: bool,
    /// How the agents' tool calls show in a conversation.
    pub tool_calls: ToolCalls,
    /// What a conversation says when Claude Code warns that a usage window fills up.
    pub limit_warnings: LimitWarnings,
    /// The built-in pieces of the status line under the message box, in order.
    pub status_items: Vec<StatusItem>,
    /// A command whose first line of output replaces the built-in pieces. Empty for none.
    pub status_command: String,
    /// Rules of the user's own that every agent is given with its instructions, whatever the
    /// agent or model: plain text, such as "keep answers short". Empty for none.
    pub rules: String,
    /// How Swarmed gets its new versions.
    pub updates: UpdateMode,
    /// What a new task's branch starts with, joined to the task's name by one `/`, in every
    /// repository whose `branch_pattern` is the default. Kept without slashes at its ends; empty
    /// for the user's login.
    pub branch_prefix: String,
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
    Swarm,
    /// The swarm's icons and words, and its names for the tree as well: a colony for a
    /// repository, burrowed for closed. What a new installation starts in.
    #[default]
    Colony,
}

/// How large and how far apart the app draws things. Apart from the palette and the style,
/// so it goes with any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Dense, with small text: the most sessions in view. What Swarmed started with.
    #[default]
    Classic,
    /// For easier reading: larger text, taller rows, larger buttons and stronger contrast.
    Senior,
    /// The user's own, as their layout file describes it.
    Custom,
}

/// How the agents' tool calls show in a conversation. Permission requests, questions, plans and
/// views show whatever this says: they need the user or show them something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCalls {
    /// Each call on a line of its own.
    All,
    /// Calls that follow each other folded into one line, which counts them and names the
    /// last one used.
    #[default]
    Folded,
    /// None, with a larger sign that the agent works for as long as it does.
    Hidden,
}

/// What a conversation says when Claude Code warns that a usage window fills up.
///
/// A limit reached, and work that goes on with extra usage, are said whatever this is, and the
/// usage coins show the same either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitWarnings {
    /// Each warning Claude Code gives, once for as long as the same window keeps warning. It
    /// warns from about half a window.
    #[default]
    Frequent,
    /// Only once a window is at least 75% used, when the usage coins warn too, and once for
    /// as long as it stays there.
    Rare,
    /// None at all.
    None,
}

/// How Swarmed gets its new versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMode {
    /// It looks by itself, at launch and every few hours, downloads what it finds, and
    /// installs that when it quits.
    #[default]
    Automatic,
    /// It looks only when asked to, in the About view.
    WhenAsked,
}

/// The smallest and largest text the app can be set to, in percent.
const TEXT_SCALE_RANGE: (u16, u16) = (80, 150);

impl Default for Settings {
    /// Visual Studio Code.
    fn default() -> Self {
        Self {
            editor: "code".to_string(),
            terminal_app: "Terminal".to_string(),
            auto_title: true,
            theme: Theme::System,
            palette: Palette::Swarm,
            style: Style::Colony,
            layout: Layout::Classic,
            text_scale: 100,
            default_supervision: Supervision::Auto,
            notifications: true,
            html_views: true,
            tool_calls: ToolCalls::Folded,
            limit_warnings: LimitWarnings::Frequent,
            status_items: StatusItem::defaults(),
            status_command: String::new(),
            rules: String::new(),
            updates: UpdateMode::Automatic,
            branch_prefix: String::new(),
        }
    }
}

impl Settings {
    /// Loads the settings, or returns the defaults if the file does not exist. What cannot be
    /// understood costs only itself (`read`): only a file that cannot be read is an error.
    pub(crate) fn load(path: &Path) -> Result<Self> {
        Ok(Self::read(path)?.0)
    }

    /// Reads the settings key by key, with what could not be understood, said for the user. A
    /// key this version does not know is left out (a newer Swarmed may have written it), a value
    /// it cannot take leaves that setting at its default, and a file that is not TOML at all
    /// gives the defaults. One key must not stop the app, and none goes unsaid.
    pub(crate) fn read(path: &Path) -> Result<(Self, Problems)> {
        let Some(text) = lenient::read_if_there(path)? else {
            return Ok((Self::default(), Vec::new()));
        };
        match text.parse::<toml::Table>() {
            Ok(table) => {
                let (settings, problems) = lenient::fields::<Self>(table);
                Ok((settings.within_limits(), problems))
            }
            Err(error) => {
                let meanwhile = format!(
                    "Every setting is at its default until it is fixed, and saving the settings \
                     keeps a copy of it as `{}`.",
                    lenient::unreadable_name(path)
                );
                Ok((
                    Self::default(),
                    vec![lenient::not_toml(&text, &error, &meanwhile)],
                ))
            }
        }
    }

    /// The settings with every value brought into the range the app can show, and the branch
    /// prefix into the form it is kept in.
    fn within_limits(mut self) -> Self {
        let (smallest, largest) = TEXT_SCALE_RANGE;
        self.text_scale = self.text_scale.clamp(smallest, largest);
        self.branch_prefix = crate::prefix::trimmed(&self.branch_prefix).to_string();
        self
    }

    /// Writes the settings. Keys this version does not know, which a newer Swarmed may have
    /// written, are written back as they were, so this one does not erase that one's choices. A
    /// file that is not TOML is kept beside it first: what the user wrote there is not lost to
    /// the defaults that stood in for it.
    pub(crate) fn save(&self, path: &Path) -> Result<()> {
        if self.rules.len() > MAX_RULES_BYTES {
            return Err(Error::RulesTooLong(MAX_RULES_BYTES));
        }
        let mut text =
            toml::to_string(&self.clone().within_limits()).map_err(|e| invalid(path, &e))?;
        let unknown = unknown_kept(path)?;
        // After every setting: the settings hold no tables, so nothing written here can land
        // inside one of theirs.
        if !unknown.is_empty() {
            text.push_str(&toml::to_string(&unknown).map_err(|e| invalid(path, &e))?);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// The keys this version of the settings file has: every field, as it is written.
fn known_keys() -> Vec<String> {
    let written = toml::Value::try_from(Settings::default()).ok();
    let table = written.as_ref().and_then(toml::Value::as_table);
    table.map_or_else(Vec::new, |table| table.keys().cloned().collect())
}

/// The keys of the file at `path` that this version does not know, to be written back. A file
/// that is not TOML is moved aside instead (`lenient::keep_if_unreadable`), before it is
/// written over.
fn unknown_kept(path: &Path) -> Result<toml::Table> {
    lenient::keep_if_unreadable(path)?;
    let Some(text) = lenient::read_if_there(path)? else {
        return Ok(toml::Table::new());
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return Ok(toml::Table::new());
    };
    let known = known_keys();
    Ok(table
        .into_iter()
        .filter(|(key, _)| !known.contains(key))
        .collect())
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
            terminal_app: "Ghostty".to_string(),
            auto_title: false,
            theme: Theme::Light,
            palette: Palette::Forest,
            style: Style::Plain,
            layout: Layout::Senior,
            text_scale: 110,
            default_supervision: Supervision::Ask,
            notifications: false,
            html_views: false,
            tool_calls: ToolCalls::Hidden,
            limit_warnings: LimitWarnings::Rare,
            status_items: vec![StatusItem::Cost],
            status_command: "~/bin/status".to_string(),
            rules: "Keep answers short.\nNo em dashes.".to_string(),
            updates: UpdateMode::WhenAsked,
            branch_prefix: "jd/feat".to_string(),
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
    fn tool_calls_are_folded_unless_chosen_otherwise() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        assert_eq!(
            Settings::load(&path).expect("defaults").tool_calls,
            ToolCalls::Folded
        );
        // A file written before the setting existed, or by someone who never chose.
        std::fs::write(&path, "theme = \"dark\"").expect("writes");
        assert_eq!(
            Settings::load(&path).expect("loads").tool_calls,
            ToolCalls::Folded
        );
        std::fs::write(&path, "tool_calls = \"all\"").expect("writes");
        assert_eq!(
            Settings::load(&path).expect("loads").tool_calls,
            ToolCalls::All
        );
    }

    #[test]
    fn usage_warnings_are_frequent_unless_chosen_otherwise() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        // A file written before the setting existed says every warning, as Swarmed did then.
        std::fs::write(&path, "theme = \"dark\"").expect("writes");
        assert_eq!(
            Settings::load(&path).expect("loads").limit_warnings,
            LimitWarnings::Frequent
        );
        std::fs::write(&path, "limit_warnings = \"none\"").expect("writes");
        assert_eq!(
            Settings::load(&path).expect("loads").limit_warnings,
            LimitWarnings::None
        );
        // A choice a newer Swarmed may have: this one says every warning, and names the value.
        std::fs::write(&path, "limit_warnings = \"hourly\"\ntheme = \"dark\"").expect("writes");
        let (settings, problems) = Settings::read(&path).expect("reads");
        assert_eq!(
            (settings.limit_warnings, settings.theme),
            (LimitWarnings::Frequent, Theme::Dark)
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`limit_warnings`"), "{problems:?}");
    }

    #[test]
    fn a_key_this_version_does_not_know_is_named_and_costs_nothing_else() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        // A newer Swarmed's setting, and a misspelt one: neither stops the rest.
        let written = "editor = \"zed\"\ncoming_later = \"yes\"\neditr = \"vim\"\n";
        std::fs::write(&path, written).expect("writes");
        let (settings, problems) = Settings::read(&path).expect("reads");
        assert_eq!(settings.editor, "zed");
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems[0].contains("`coming_later` is not something"),
            "{problems:?}"
        );
        assert!(problems[1].contains("`editr`"), "{problems:?}");

        // Saved by this version, the newer one's keys are still there for it.
        Settings {
            auto_title: false,
            ..settings
        }
        .save(&path)
        .expect("saves");
        let saved = std::fs::read_to_string(&path).expect("reads back");
        assert!(saved.contains("coming_later = \"yes\""), "{saved}");
        let (again, _) = Settings::read(&path).expect("reads again");
        assert_eq!((again.editor.as_str(), again.auto_title), ("zed", false));
    }

    #[test]
    fn a_branch_prefix_is_kept_without_the_slashes_at_its_ends() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        // A file written before the setting existed has none: branches start with the login.
        std::fs::write(&path, "theme = \"dark\"").expect("writes");
        assert_eq!(Settings::load(&path).expect("loads").branch_prefix, "");
        // Written by hand with slashes at its ends, it is read without them.
        std::fs::write(&path, "branch_prefix = \" /jd/feat/ \"").expect("writes");
        assert_eq!(
            Settings::load(&path).expect("loads").branch_prefix,
            "jd/feat"
        );
        let typed = Settings {
            branch_prefix: "//team-a/".to_string(),
            ..Settings::default()
        };
        typed.save(&path).expect("saves");
        let saved = std::fs::read_to_string(&path).expect("reads back");
        assert!(saved.contains("branch_prefix = \"team-a\""), "{saved}");
    }

    #[test]
    fn a_value_that_cannot_be_read_leaves_only_its_own_setting_at_its_default() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        let written = "theme = \"neon\"\npalette = \"forest\"\ntext_scale = \"big\"\n";
        std::fs::write(&path, written).expect("writes");
        let (settings, problems) = Settings::read(&path).expect("reads");
        assert_eq!(
            (settings.theme, settings.palette, settings.text_scale),
            (Theme::System, Palette::Forest, 100)
        );
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|problem| problem.contains("`theme`")));
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("`text_scale`"))
        );
        assert!(problems[0].contains("at its default"), "{problems:?}");
    }

    #[test]
    fn a_file_that_is_not_toml_gives_the_defaults_and_is_kept_when_saved_over() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        let written = "editor = \"zed\"\nrules = \"keep it short\n";
        std::fs::write(&path, written).expect("writes");
        let (settings, problems) = Settings::read(&path).expect("reads");
        assert_eq!(settings, Settings::default());
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("not TOML (line 2"), "{problems:?}");

        Settings::default().save(&path).expect("saves");
        let kept = std::fs::read_to_string(dir.path().join("settings.toml.unreadable"))
            .expect("kept aside");
        assert_eq!(kept, written);
        assert_eq!(
            Settings::read(&path).expect("reads").1,
            Vec::<String>::new()
        );
    }
}
