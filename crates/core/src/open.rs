//! Handing a task over to other programs: the user's editor and their browser.

use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::error::{Error, Result};

/// macOS applications for editor commands that are often not on a desktop app's `PATH`, which
/// is shorter than a shell's.
const APPLICATIONS: &[(&str, &str)] = &[
    ("code", "Visual Studio Code"),
    ("cursor", "Cursor"),
    ("zed", "Zed"),
    ("subl", "Sublime Text"),
    ("idea", "IntelliJ IDEA"),
];

/// The program and arguments that open `folder` with the editor command the user configured.
fn editor_command(editor: &str, folder: &Path) -> Result<Command> {
    let mut words = editor.split_whitespace();
    let program = words
        .next()
        .ok_or_else(|| could_not_open("the editor", "no editor command is configured"))?;
    let mut command = Command::new(program);
    command.args(words).arg(folder);
    Ok(command)
}

/// The same through macOS's `open`, for an editor that is installed but not on `PATH`.
fn application_command(editor: &str, folder: &Path) -> Option<Command> {
    let program = editor.split_whitespace().next()?;
    let (_, application) = APPLICATIONS.iter().find(|(name, _)| *name == program)?;
    let mut command = Command::new("open");
    command.args(["-a", application]).arg(folder);
    Some(command)
}

fn could_not_open(what: &str, reason: impl std::fmt::Display) -> Error {
    Error::OpenFailed {
        what: what.to_string(),
        reason: reason.to_string(),
    }
}

/// Starts `command` without waiting for it and without tying its output to ours.
fn launch(mut command: Command) -> std::io::Result<()> {
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    reap(child);
    Ok(())
}

/// Waits for a launched program on a thread of its own, so it never lingers as a zombie.
fn reap(mut child: Child) {
    drop(std::thread::spawn(move || child.wait()));
}

/// Opens `folder` in the editor the user configured.
pub(crate) fn in_editor(editor: &str, folder: &Path) -> Result<()> {
    let direct = launch(editor_command(editor, folder)?);
    let Err(error) = direct else {
        return Ok(());
    };
    let fallback = application_command(editor, folder).filter(|_| cfg!(target_os = "macos"));
    let Some(command) = fallback else {
        return Err(could_not_open(editor, error));
    };
    launch(command).map_err(|e| could_not_open(editor, e))
}

/// Opens a window of the terminal application called `app` in `folder`.
pub(crate) fn in_terminal(app: &str, folder: &Path) -> Result<()> {
    let app = app.trim();
    if app.is_empty() {
        return Err(could_not_open(
            "the terminal",
            "no terminal application is configured",
        ));
    }
    let mut command = Command::new("open");
    command.args(["-a", app]).arg(folder);
    // `open` itself tells whether there is such an application, so it is waited for.
    let found = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| could_not_open(app, e))?;
    if found.success() {
        return Ok(());
    }
    Err(could_not_open(app, "there is no application of that name"))
}

/// Opens `url` in the user's default browser.
pub(crate) fn in_browser(url: &str) -> Result<()> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut command = Command::new(opener);
    command.arg(url);
    launch(command).map_err(|e| could_not_open(url, e))
}

/// What a link in a conversation may be for it to be handed to the user's browser or mail
/// program. Anything else, a file or another program's own scheme, is refused.
const LINK_SCHEMES: &[&str] = &["http://", "https://", "mailto:"];

/// Opens a link from a conversation in the user's default browser, never in Swarmed's window.
pub fn open_link(url: &str) -> Result<()> {
    let url = url.trim();
    let known = LINK_SCHEMES.iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    });
    if !known {
        return Err(could_not_open(url, "only web and mail links are opened"));
    }
    in_browser(url)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn the_editor_command_keeps_its_own_arguments_before_the_folder() {
        let command = editor_command("code --new-window", Path::new("/w/one")).expect("builds");
        assert_eq!(command.get_program(), "code");
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(args, ["--new-window", "/w/one"]);
    }

    #[test]
    fn an_empty_editor_command_is_refused() {
        assert!(matches!(
            editor_command("  ", Path::new("/w/one")),
            Err(Error::OpenFailed { .. })
        ));
    }

    #[test]
    fn known_editors_can_be_opened_as_applications() {
        let command = application_command("code -n", Path::new("/w/one")).expect("known");
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(args, ["-a", "Visual Studio Code", "/w/one"]);
        assert!(application_command("my-editor", Path::new("/w/one")).is_none());
    }

    #[test]
    fn an_editor_that_does_not_exist_is_reported_by_name() {
        let missing = in_editor("swarmed-no-such-editor", Path::new("/w/one"));
        assert!(
            matches!(missing, Err(Error::OpenFailed { what, .. }) if what == "swarmed-no-such-editor")
        );
    }

    #[test]
    fn only_web_and_mail_links_are_opened() {
        for refused in [
            "file:///etc/passwd",
            "-a Calculator",
            "vscode://x",
            "",
            "javascript:1",
        ] {
            assert!(
                matches!(open_link(refused), Err(Error::OpenFailed { .. })),
                "{refused}"
            );
        }
    }
}
