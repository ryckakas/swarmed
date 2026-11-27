//! Settings that apply to every project on this machine.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::agent::Supervision;
use crate::browser::MAX_IDLE_MINUTES;
use crate::error::{Error, Result};
use crate::lenient::{self, Problems};
use crate::peers::{MAX_SPAWNED, WORKER_LIMITS};
use crate::put_away::MAX_DAYS;
use crate::statusline::StatusItem;

/// The user's own preferences, one TOML file in the data directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// Unknown keys are read past and named (`lenient`), never refused: see `Settings::read`.
#[serde(default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a yes or no of the user's own, apart from the others, as the file has it"
)]
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
    /// How many open sessions may work for one session, within [`WORKER_LIMITS`].
    ///
    /// Beyond it its agent starts no more, beside it or in a task. One out of range in the
    /// file is read as the default, [`MAX_SPAWNED`], and said. Not reachable through
    /// `configure`: an agent must not raise its own ceiling.
    pub max_workers: usize,
    /// Whether the system shows a notification when a session needs the user, finishes or
    /// fails while they look elsewhere.
    pub notifications: bool,
    /// Whether agents are told they may answer with small HTML views, and those are shown.
    pub html_views: bool,
    /// How the agents' tool calls show in a conversation.
    pub tool_calls: ToolCalls,
    /// Whether a conversation draws a line where each turn that showed something ends. A turn
    /// that failed or was stopped keeps its line either way.
    pub turn_end_lines: bool,
    /// Which formulas in what agents write are drawn as math.
    pub math: MathShown,
    /// Which levels of the account's usage the notice at the top of the window says.
    pub limit_warnings: LimitWarnings,
    /// Whether a Claude Code session that a usage limit stopped goes on by itself when the
    /// limit resets, without being asked to each time: a message is sent to it then.
    pub continue_after_limit: bool,
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
    /// Whether a task whose pull request merged is put away by itself, `put_away_merged_days`
    /// after the merge, unless work goes on in it then (`put_away.rs`).
    pub put_away_merged: bool,
    /// How many days after its pull request merged a task is put away, from 0 to 365.
    pub put_away_merged_days: u32,
    /// Whether a task none of whose sessions recorded anything for `put_away_idle_days` is put
    /// away by itself. A terminal or a running stack does not count.
    pub put_away_idle: bool,
    /// How many days without anything in its sessions put a task away, from 1 to 365.
    pub put_away_idle_days: u32,
    /// Whether putting a task away also closes its sessions. Without it they stay open, with
    /// their agents stopped.
    pub put_away_closes_sessions: bool,
    /// The program a task's browser runs, such as Chrome Canary's. Empty to use the first of
    /// Google Chrome, Chromium, Microsoft Edge and Brave that is installed.
    pub browser_program: String,
    /// How many minutes a task's browser may sit unused before it is stopped, up to a day; 0
    /// to leave it running until its task is put away or Swarmed quits.
    pub browser_idle_minutes: u32,
    /// Where a task's browser is shown: drawn in the pane, or as a Chrome window of its own.
    pub browser_display: BrowserDisplay,
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

/// Which formulas in what agents write are drawn as math.
///
/// The window draws them with `KaTeX`. Code is never read for them, and the user's own messages
/// are not Markdown, so they show as typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MathShown {
    /// None: formulas show as they were written.
    Off,
    /// `$$…$$`, `\(…\)`, `\[…\]` and fenced `math` blocks. A single `$` stays a dollar sign,
    /// so replies about prices and shell variables (`$5 and $10`, `$HOME`) stay text.
    #[default]
    Standard,
    /// Those, and `$…$` where it reads as math: no space after the first `$` or before the
    /// second, and no digit after it.
    WithSingleDollars,
}

/// Which levels of the account's usage the notice at the top of the window says.
///
/// The levels are 50%, 75% and 90% of a window used, a limit reached with extra usage paying
/// for the work, and a limit reached with nothing paying, so that requests are refused. The
/// conversation says only a turn a limit refused, and the usage coins show the same whatever
/// this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitWarnings {
    /// Every level, from 50% of a window used.
    #[default]
    Frequent,
    /// From 90% of a window used: 90%, billed and stopped.
    Rare,
    /// Only a limit that stops requests.
    None,
}

/// Where a task's browser is shown. Either way it is the same Chrome, on the task's profile,
/// and agents' browser tools act in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserDisplay {
    /// Chrome runs without a window, and the Browser pane draws pictures of its page.
    #[default]
    Pane,
    /// Chrome runs with a window of its own, drawn by Chrome itself; the pane lists its tabs
    /// and brings its window forward.
    Window,
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
            max_workers: MAX_SPAWNED,
            notifications: true,
            html_views: true,
            tool_calls: ToolCalls::Folded,
            turn_end_lines: true,
            math: MathShown::Standard,
            limit_warnings: LimitWarnings::Frequent,
            continue_after_limit: false,
            status_items: StatusItem::defaults(),
            status_command: String::new(),
            rules: String::new(),
            updates: UpdateMode::Automatic,
            branch_prefix: String::new(),
            put_away_merged: false,
            put_away_merged_days: 1,
            put_away_idle: false,
            put_away_idle_days: 14,
            put_away_closes_sessions: false,
            browser_program: String::new(),
            browser_idle_minutes: 30,
            browser_display: BrowserDisplay::Pane,
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
                let (mut settings, mut problems) = lenient::fields::<Self>(table);
                problems.extend(settings.workers_at_default_if_out_of_range());
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

    /// Puts `max_workers` back at its default if it is out of range, and says so. Not brought
    /// to the nearest end of the range, as the sizes are: the user wrote a ceiling by hand that
    /// this version does not take, and the default is the one the app chose for them.
    fn workers_at_default_if_out_of_range(&mut self) -> Option<String> {
        if WORKER_LIMITS.contains(&self.max_workers) {
            return None;
        }
        let written = std::mem::replace(&mut self.max_workers, MAX_SPAWNED);
        Some(format!(
            "`max_workers` is {written}, but it takes {} to {}, so it is at its default, \
             {MAX_SPAWNED}.",
            WORKER_LIMITS.start(),
            WORKER_LIMITS.end()
        ))
    }

    /// The settings with every value brought into the range the app can show, and the branch
    /// prefix into the form it is kept in.
    fn within_limits(mut self) -> Self {
        let (smallest, largest) = TEXT_SCALE_RANGE;
        self.text_scale = self.text_scale.clamp(smallest, largest);
        self.put_away_merged_days = self.put_away_merged_days.min(MAX_DAYS);
        self.put_away_idle_days = self.put_away_idle_days.clamp(1, MAX_DAYS);
        self.browser_idle_minutes = self.browser_idle_minutes.min(MAX_IDLE_MINUTES);
        self.branch_prefix = crate::prefix::trimmed(&self.branch_prefix).to_string();
        self
    }

    /// Writes the settings. Keys this version does not know, which a newer Swarmed may have
    /// written, are written back as they were, so this one does not erase that one's choices. A
    /// file that is not TOML is kept beside it first: what the user wrote there is not lost to
    /// the defaults that stood in for it.
    ///
    /// Refused, with nothing written, for rules longer than [`MAX_RULES_BYTES`] and for a
    /// `max_workers` out of [`WORKER_LIMITS`].
    pub(crate) fn save(&self, path: &Path) -> Result<()> {
        if self.rules.len() > MAX_RULES_BYTES {
            return Err(Error::RulesTooLong(MAX_RULES_BYTES));
        }
        if !WORKER_LIMITS.contains(&self.max_workers) {
            return Err(Error::WorkersOutOfRange(self.max_workers));
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
            max_workers: 12,
            notifications: false,
            html_views: false,
            tool_calls: ToolCalls::Hidden,
            turn_end_lines: false,
            math: MathShown::WithSingleDollars,
            limit_warnings: LimitWarnings::Rare,
            continue_after_limit: true,
            status_items: vec![StatusItem::Cost],
            status_command: "~/bin/status".to_string(),
            rules: "Keep answers short.\nNo em dashes.".to_string(),
            updates: UpdateMode::WhenAsked,
            branch_prefix: "jd/feat".to_string(),
            put_away_merged: true,
            put_away_merged_days: 3,
            put_away_idle: true,
            put_away_idle_days: 30,
            put_away_closes_sessions: true,
            browser_program: "/Applications/Chromium.app/Contents/MacOS/Chromium".to_string(),
            browser_idle_minutes: 90,
            browser_display: BrowserDisplay::Window,
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
    fn turn_end_lines_are_drawn_unless_turned_off() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        assert!(Settings::load(&path).expect("defaults").turn_end_lines);
        // A file written before the setting existed.
        std::fs::write(&path, "theme = \"dark\"").expect("writes");
        assert!(Settings::load(&path).expect("loads").turn_end_lines);
        std::fs::write(&path, "turn_end_lines = false").expect("writes");
        assert!(!Settings::load(&path).expect("loads").turn_end_lines);
    }

    #[test]
    fn math_is_drawn_without_single_dollars_unless_chosen_otherwise() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        assert_eq!(
            Settings::load(&path).expect("defaults").math,
            MathShown::Standard
        );
        // A file written before the setting existed.
        std::fs::write(&path, "theme = \"dark\"").expect("writes");
        assert_eq!(
            Settings::load(&path).expect("loads").math,
            MathShown::Standard
        );
        std::fs::write(&path, "math = \"with_single_dollars\"").expect("writes");
        assert_eq!(
            Settings::load(&path).expect("loads").math,
            MathShown::WithSingleDollars
        );
        // A choice a newer Swarmed may have: this one draws what it knows, and names the value.
        std::fs::write(&path, "math = \"everything\"\ntheme = \"dark\"").expect("writes");
        let (settings, problems) = Settings::read(&path).expect("reads");
        assert_eq!(
            (settings.math, settings.theme),
            (MathShown::Standard, Theme::Dark)
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`math`"), "{problems:?}");
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
    fn nothing_is_put_away_by_itself_unless_turned_on_and_days_stay_in_range() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        // A file written before the settings existed.
        std::fs::write(&path, "theme = \"dark\"").expect("writes");
        let loaded = Settings::load(&path).expect("loads");
        assert!(!loaded.put_away_merged && !loaded.put_away_idle);
        assert!(!loaded.put_away_closes_sessions);
        assert_eq!(
            (loaded.put_away_merged_days, loaded.put_away_idle_days),
            (1, 14)
        );
        let written = "put_away_merged_days = 0\nput_away_idle_days = 0\n";
        std::fs::write(&path, written).expect("writes");
        let loaded = Settings::load(&path).expect("loads");
        assert_eq!(
            (loaded.put_away_merged_days, loaded.put_away_idle_days),
            (0, 1)
        );
        std::fs::write(&path, "put_away_idle_days = 9000\n").expect("writes");
        assert_eq!(
            Settings::load(&path).expect("loads").put_away_idle_days,
            365
        );
        std::fs::write(&path, "browser_idle_minutes = 100000\n").expect("writes");
        assert_eq!(
            Settings::load(&path).expect("loads").browser_idle_minutes,
            MAX_IDLE_MINUTES
        );
        std::fs::write(&path, "browser_display = \"window\"\n").expect("writes");
        let display = Settings::load(&path).expect("loads").browser_display;
        assert_eq!(display, BrowserDisplay::Window);
        // A way of showing it a newer Swarmed knows is the pane here.
        std::fs::write(&path, "browser_display = \"tab\"\n").expect("writes");
        let display = Settings::load(&path).expect("loads").browser_display;
        assert_eq!(display, BrowserDisplay::Pane);
    }

    #[test]
    fn workers_are_six_unless_chosen_and_a_number_out_of_range_is_the_default_said() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        assert_eq!(Settings::load(&path).expect("defaults").max_workers, 6);
        // A file written before the setting existed.
        std::fs::write(&path, "theme = \"dark\"").expect("writes");
        let (settings, problems) = Settings::read(&path).expect("reads");
        assert_eq!((settings.max_workers, problems.len()), (MAX_SPAWNED, 0));
        for (written, read) in [(1, 1), (2, 2), (20, 20)] {
            std::fs::write(&path, format!("max_workers = {written}")).expect("writes");
            let (settings, problems) = Settings::read(&path).expect("reads");
            assert_eq!((settings.max_workers, problems), (read, vec![]));
        }
        // Written by hand out of range: the default, not the nearest end, and said.
        for written in [0, 21, 500] {
            std::fs::write(&path, format!("max_workers = {written}\ntheme = \"dark\""))
                .expect("writes");
            let (settings, problems) = Settings::read(&path).expect("reads");
            assert_eq!((settings.max_workers, settings.theme), (6, Theme::Dark));
            assert_eq!(problems.len(), 1, "{problems:?}");
            assert!(
                problems[0].contains(&format!("`max_workers` is {written}")),
                "{problems:?}"
            );
            assert!(problems[0].contains("1 to 20"), "{problems:?}");
            assert!(problems[0].contains("at its default, 6"), "{problems:?}");
        }
        // Not a number of sessions at all.
        std::fs::write(&path, "max_workers = -3").expect("writes");
        let (settings, problems) = Settings::read(&path).expect("reads");
        assert_eq!(settings.max_workers, 6);
        assert!(problems[0].contains("`max_workers`"), "{problems:?}");
    }

    #[test]
    fn workers_out_of_range_are_not_saved() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        let with = |max_workers| Settings {
            max_workers,
            ..Settings::default()
        };
        with(9).save(&path).expect("saves");
        for refused in [0, 21] {
            assert!(matches!(
                with(refused).save(&path),
                Err(Error::WorkersOutOfRange(n)) if n == refused
            ));
        }
        assert_eq!(Settings::load(&path).expect("loads").max_workers, 9);
        let said = Error::WorkersOutOfRange(0).to_string();
        assert_eq!(
            said,
            "how many open sessions may work for one session is from 1 to 20, not 0"
        );
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
