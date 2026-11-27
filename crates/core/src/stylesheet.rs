//! The user's own stylesheet: `style.css` in the data directory, which the window applies after
//! every stylesheet of its own, the custom layout's included, so that it wins where they
//! disagree. One for every layout: a rule meant for one says so with `data-layout`.
//!
//! The custom layout (`layouts.rs`) only changes values. A stylesheet can hide or fake anything
//! in the window: a permission prompt's Deny button, what a proposal says, the problem banner,
//! made-up text with `content:`. So:
//!
//! - **No agent reaches it through Swarmed.** `configure` has no kind for it, not even a
//!   proposal: a proposal would show the CSS, and few users can tell from CSS what it hides.
//! - **The window applies only what the user has seen.** The text they last saved in Settings,
//!   or accepted when told the file changed, is kept in the database (`stylesheets`), and that
//!   is what the window applies, whatever the file holds now. An agent runs as the user and can
//!   write the file; Swarmed cannot stop that, but it does not act on it silently: a change on
//!   disk is [`Stylesheet::waiting`] until the user says yes, and the window says what changed.
//!   A file taken away is no stylesheet: that can only give the window its own look back.
//! - **A way out**, for the day a stylesheet hides the very switch that turns it off: the
//!   setting, from Settings or `swarmed layout stylesheet off`, and leaving it out for one run
//!   ([`Core::set_stylesheet_left_out`]), which the app does when ⌥ is held as it starts.
//!
//! It loads nothing from elsewhere: the window's policy (`tauri.conf.json`) has no address of
//! elsewhere for styles, fonts or images, and is not loosened for it. A rule the window cannot
//! parse costs only itself, as in any browser, and a file too large to be a stylesheet is not
//! read and is named with the configuration's problems. Nothing of it can stop Swarmed from
//! starting.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::UNIX_EPOCH;

use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::lenient::Problems;
use crate::store::Store;
use crate::{Core, open};

/// The largest stylesheet read or kept, in bytes.
pub const MAX_STYLESHEET_BYTES: usize = 200_000;

/// What a new `style.css` holds: which names stay from version to version, and what cannot work.
const STARTER: &str = "\
/* Your own stylesheet for Swarmed's window, applied after the app's own, so it wins where
   they disagree. A change saved here is applied once you say yes in the window; one saved in
   Settings, at once.

   What stays from version to version:
   - the app's colours: --dusk (the window), --panel, --raised, --line, --text, --muted,
     --faint, --bright, --focus, --sky (links), --pollen, --moss, --busy, --clay, --lilac,
     --teal, --sand;
   - its fonts and sizes: --font-ui, --font-mono, --text-xs to --text-xl, --space-1 to
     --space-8;
   - what the root element says is chosen: data-theme (dark or light; none follows the
     system), data-palette, data-layout (classic or senior; a custom layout is drawn as the
     one it starts from).
   Class names change between versions: a rule that uses one may stop working.

   Nothing is loaded from elsewhere: no @import of an address, no web fonts, no remote
   images. Fonts installed on this computer work by name; images as data: addresses.

   For example, larger text in the senior layout only:
   :root[data-layout=\"senior\"] { --text-sm: 18px; }
*/
";

/// The user's own stylesheet, as the window is to apply it and as the file stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Stylesheet {
    /// Whether the user turned it on (`user_stylesheet` in the settings).
    pub on: bool,
    /// Whether this run leaves it out all the same.
    pub left_out: bool,
    /// What the window applies now: the accepted text, while the stylesheet is on, not left out
    /// and its file there; empty otherwise.
    pub applied: String,
    /// The text the user last saved or accepted; empty for none.
    pub accepted: String,
    /// What `style.css` holds; `None` while there is no such file, or none that can be read.
    pub file: Option<String>,
    /// When the file last changed, in seconds since 1970.
    pub changed_at: Option<i64>,
    /// Whether the file holds what the user has not accepted: a change made on disk, which
    /// waits for their yes.
    pub waiting: bool,
}

/// The file at `path` as a stylesheet, and when it last changed; nothing for no file, one too
/// large, or one that cannot be read. Bytes that are not UTF-8 are read as the browser would
/// show them, replaced.
fn read_file(path: &Path) -> (Option<String>, Option<i64>) {
    let meta = match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => meta,
        _ => return (None, None),
    };
    let changed = meta
        .modified()
        .ok()
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_secs()).ok());
    let text = fits(meta.len())
        .then(|| std::fs::read(path).ok())
        .flatten()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    (text, changed)
}

/// Whether a file of `len` bytes is small enough to be read as a stylesheet.
fn fits(len: u64) -> bool {
    usize::try_from(len).is_ok_and(|len| len <= MAX_STYLESHEET_BYTES)
}

/// What is wrong with the stylesheet at `path`, for the configuration's problems.
pub(crate) fn problems_in(path: &Path) -> Problems {
    let too_large = std::fs::metadata(path).is_ok_and(|meta| !fits(meta.len()));
    if too_large {
        return vec![format!(
            "larger than {} kB, so it is not read and the window is drawn without it",
            MAX_STYLESHEET_BYTES / 1000
        )];
    }
    vec![]
}

/// Refuses a stylesheet too large to keep.
fn check_size(text: &str) -> Result<()> {
    if text.len() > MAX_STYLESHEET_BYTES {
        return Err(Error::Stylesheet(format!(
            "a stylesheet is at most {} kB",
            MAX_STYLESHEET_BYTES / 1000
        )));
    }
    Ok(())
}

fn seconds_now() -> i64 {
    let since = std::time::SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    })
}

impl Store {
    /// The stylesheet the user last saved or accepted; empty for none.
    fn stylesheet_accepted(&self) -> Result<String> {
        let sql = "SELECT css FROM stylesheets ORDER BY id DESC LIMIT 1";
        let css = self.conn.query_row(sql, [], |row| row.get(0)).optional()?;
        Ok(css.unwrap_or_default())
    }

    /// Keeps `css` as the stylesheet the user accepted, in place of the one before.
    fn stylesheet_keep(&self, css: &str) -> Result<()> {
        let sql = "INSERT INTO stylesheets (css, accepted_at) VALUES (?1, ?2)";
        self.conn.execute(sql, params![css, seconds_now()])?;
        let id = self.conn.last_insert_rowid();
        self.conn
            .execute("DELETE FROM stylesheets WHERE id < ?1", params![id])?;
        Ok(())
    }
}

impl Core {
    /// The user's own stylesheet: what the window applies, and what the file holds.
    pub fn stylesheet(&self) -> Result<Stylesheet> {
        let on = self.settings()?.user_stylesheet;
        let left_out = self.stylesheet_left_out.load(Ordering::Relaxed);
        let accepted = self.store().stylesheet_accepted()?;
        let (file, changed_at) = read_file(&self.paths.stylesheet());
        let applied = if on && !left_out && file.is_some() {
            accepted.clone()
        } else {
            String::new()
        };
        let waiting = file.as_ref().is_some_and(|file| *file != accepted);
        Ok(Stylesheet {
            on,
            left_out,
            applied,
            accepted,
            file,
            changed_at,
            waiting,
        })
    }

    /// Writes `text` to `style.css` and takes it as what the user accepted: saved in Settings,
    /// it is theirs, and applied at once.
    pub fn stylesheet_save(&self, text: &str) -> Result<Stylesheet> {
        check_size(text)?;
        let path = self.paths.stylesheet();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, text)?;
        self.store().stylesheet_keep(text)?;
        self.stylesheet()
    }

    /// Takes `text`, the file as the user was shown it, as what they accepted, without writing
    /// it: the file may have changed again since, and what they write in their editor is not
    /// to be lost. What it holds now, if that differs, waits for their yes in turn.
    pub fn stylesheet_accept(&self, text: &str) -> Result<Stylesheet> {
        check_size(text)?;
        self.store().stylesheet_keep(text)?;
        self.stylesheet()
    }

    /// Turns the user's stylesheet on or off, as Settings does.
    pub fn stylesheet_use(&self, on: bool) -> Result<Stylesheet> {
        let mut settings = self.settings()?;
        settings.user_stylesheet = on;
        self.set_settings(&settings)?;
        self.stylesheet()
    }

    /// Leaves the user's stylesheet out for as long as this process runs, or takes it back, the
    /// settings unchanged: the app leaves it out when ⌥ is held as it starts.
    pub fn set_stylesheet_left_out(&self, left_out: bool) -> Result<Stylesheet> {
        self.stylesheet_left_out.store(left_out, Ordering::Relaxed);
        self.stylesheet()
    }

    /// Opens `style.css` in the user's editor, made first with a comment that says what a
    /// stylesheet can use if there is none. Swarmed wrote that one, so it is taken as accepted.
    pub fn stylesheet_open(&self) -> Result<()> {
        let path = self.paths.stylesheet();
        if !path.exists() {
            self.stylesheet_save(STARTER)?;
        }
        open::lone_file_in_editor(&self.settings()?.editor, &path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Paths;

    fn core() -> (tempfile::TempDir, Core) {
        let dir = tempfile::tempdir().expect("temp dir");
        let at = dir.path();
        let paths = Paths {
            data_dir: at.join("data"),
            worktrees_root: at.join("worktrees"),
            claude_config: at.join("claude.json"),
            codex_config: at.join("codex.toml"),
            opencode_config_dir: at.join("opencode"),
        };
        let backend = std::sync::Arc::new(crate::agent::FakeBackend::default());
        let core = Core::with_backend(paths, backend).expect("opens");
        (dir, core)
    }

    fn write(core: &Core, text: &str) {
        std::fs::write(core.paths.stylesheet(), text).expect("writes");
    }

    #[test]
    fn nothing_is_applied_until_it_is_turned_on() {
        let (_dir, core) = core();
        let none = core.stylesheet().expect("reads");
        assert_eq!((none.on, none.file, none.waiting), (false, None, false));
        let saved = core.stylesheet_save("body { color: red; }").expect("saves");
        assert_eq!(saved.applied, "");
        assert!(!saved.waiting);
        let on = core.stylesheet_use(true).expect("turns on");
        assert_eq!(on.applied, "body { color: red; }");
        assert!(core.settings().expect("reads").user_stylesheet);
        let off = core.stylesheet_use(false).expect("turns off");
        assert_eq!(off.applied, "");
    }

    #[test]
    fn a_change_on_disk_waits_for_the_users_yes() {
        let (_dir, core) = core();
        core.stylesheet_use(true).expect("turns on");
        core.stylesheet_save("a { color: blue; }").expect("saves");
        write(&core, ".deny { display: none; }");
        let changed = core.stylesheet().expect("reads");
        assert!(changed.waiting);
        assert_eq!(changed.applied, "a { color: blue; }");
        assert_eq!(changed.file.as_deref(), Some(".deny { display: none; }"));

        // Accepted as shown, while the file changed once more: the shown text is applied, the
        // file is not written over, and the newer change waits in turn.
        write(&core, ".allow { display: none; }");
        let accepted = core
            .stylesheet_accept(".deny { display: none; }")
            .expect("accepts");
        assert_eq!(accepted.applied, ".deny { display: none; }");
        assert_eq!(accepted.file.as_deref(), Some(".allow { display: none; }"));
        assert!(accepted.waiting);
        let caught_up = core
            .stylesheet_accept(".allow { display: none; }")
            .expect("accepts");
        assert!(!caught_up.waiting);
    }

    #[test]
    fn a_file_taken_away_is_no_stylesheet() {
        let (_dir, core) = core();
        core.stylesheet_use(true).expect("turns on");
        core.stylesheet_save("a { color: blue; }").expect("saves");
        std::fs::remove_file(core.paths.stylesheet()).expect("removes");
        let gone = core.stylesheet().expect("reads");
        assert_eq!((gone.applied.as_str(), gone.waiting), ("", false));
        // Back as it was, it is what the user accepted.
        write(&core, "a { color: blue; }");
        assert_eq!(
            core.stylesheet().expect("reads").applied,
            "a { color: blue; }"
        );
    }

    #[test]
    fn a_run_can_leave_it_out_and_take_it_back() {
        let (_dir, core) = core();
        core.stylesheet_use(true).expect("turns on");
        core.stylesheet_save("a { color: blue; }").expect("saves");
        let out = core.set_stylesheet_left_out(true).expect("leaves out");
        assert_eq!(
            (out.on, out.left_out, out.applied.as_str()),
            (true, true, "")
        );
        let back = core.set_stylesheet_left_out(false).expect("takes back");
        assert_eq!(back.applied, "a { color: blue; }");
    }

    #[test]
    fn a_file_too_large_is_not_read_and_said() {
        let (_dir, core) = core();
        core.stylesheet_use(true).expect("turns on");
        let large = "a{}".repeat(MAX_STYLESHEET_BYTES);
        write(&core, &large);
        let read = core.stylesheet().expect("reads");
        assert_eq!((read.file, read.applied.as_str()), (None, ""));
        let said = problems_in(&core.paths.stylesheet());
        assert!(said[0].contains("not read"), "{said:?}");
        assert!(core.stylesheet_save(&large).is_err());
        assert!(core.stylesheet_accept(&large).is_err());
    }

    #[test]
    fn the_starter_is_taken_as_accepted() {
        let (_dir, core) = core();
        let mut settings = core.settings().expect("reads");
        // Nothing opens in a test: an editor that is not there is refused after the file is made.
        settings.editor = "swarmed-no-such-editor".to_string();
        core.set_settings(&settings).expect("saves");
        assert!(core.stylesheet_open().is_err());
        let made = core.stylesheet().expect("reads");
        assert_eq!(made.file.as_deref(), Some(STARTER));
        assert!(!made.waiting);
        // A file that is there is left as it is.
        write(&core, "a {}");
        assert!(core.stylesheet_open().is_err());
        assert_eq!(
            core.stylesheet().expect("reads").file.as_deref(),
            Some("a {}")
        );
    }
}
