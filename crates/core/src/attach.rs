//! Files the user points an agent at with a message.
//!
//! Nothing is uploaded or copied: the agent runs on this machine and can read the files where
//! they are. Attaching one is telling the agent its path, in a form every front end writes the
//! same way and can read back to show the files as such.

use std::fmt::Write;
use std::path::PathBuf;

use crate::agent::Auth;
use crate::error::{Error, Result};
use crate::{Core, QueuedMessage};

/// The line that opens the list of attached files in a message.
pub const ATTACHED_FILES: &str = "Attached files:";

/// `text` with `files` listed under it, as the message the agent is sent.
///
/// Each file has to be named by its whole path and be there, so that a slip is caught here and
/// not left for the agent to puzzle over. A message needs words or files: one with neither is
/// refused.
pub fn with_files(text: &str, files: &[PathBuf]) -> Result<String> {
    let text = text.trim();
    if files.is_empty() {
        return if text.is_empty() {
            Err(Error::EmptyMessage)
        } else {
            Ok(text.to_string())
        };
    }
    if let Some(missing) = files
        .iter()
        .find(|file| !(file.is_absolute() && file.exists()))
    {
        return Err(Error::NoSuchAttachment(missing.clone()));
    }
    let mut message = String::from(text);
    if !message.is_empty() {
        message.push_str("\n\n");
    }
    message.push_str(ATTACHED_FILES);
    for file in files {
        // Writing to a `String` cannot fail.
        write!(message, "\n- {}", file.display()).ok();
    }
    Ok(message)
}

impl Core {
    /// Sends `text`, with `files` listed under it, to a session, starting its agent first if
    /// it is not running.
    ///
    /// A message that cannot be sent as it is, such as one naming a file that is not there, is
    /// refused before the agent is started. Must be called from within a tokio runtime.
    pub fn session_message(
        &self,
        id: i64,
        text: &str,
        files: &[PathBuf],
        auth: Auth,
    ) -> Result<()> {
        let message = with_files(text, files)?;
        // One of the agent's own commands that Swarmed shows, asks about or refuses is not
        // a message to it, and most need no agent started.
        if self.command_taken(id, &message, Some(auth.clone()))? {
            return Ok(());
        }
        self.session_start(id, auth)?;
        self.session_send_as_typed(id, &message)
    }

    /// Sends `text`, with `files` listed under it, now: a turn under way is stopped and it goes
    /// when that turn has ended, ahead of anything waiting. Must be called from within a tokio
    /// runtime.
    pub fn session_message_now(
        &self,
        id: i64,
        text: &str,
        files: &[PathBuf],
        auth: Auth,
    ) -> Result<()> {
        self.session_send_text_now(id, &with_files(text, files)?, auth)
    }

    /// Adds `text`, with `files` listed under it, to the end of the session's queue and
    /// returns the queue.
    pub fn session_enqueue_message(
        &self,
        id: i64,
        text: &str,
        files: &[PathBuf],
    ) -> Result<Vec<QueuedMessage>> {
        self.session_enqueue(id, &with_files(text, files)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_listed_under_the_message_by_their_whole_paths() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (one, two) = (dir.path().join("a b.png"), dir.path().join("notes.md"));
        std::fs::write(&one, "").expect("writes");
        std::fs::write(&two, "").expect("writes");
        let message = with_files(" look at these ", &[one.clone(), two.clone()]).expect("valid");
        assert_eq!(
            message,
            format!(
                "look at these\n\nAttached files:\n- {}\n- {}",
                one.display(),
                two.display()
            )
        );
        // Files alone are a message; a folder is as good as a file.
        let alone = with_files("", &[dir.path().to_path_buf()]).expect("valid");
        assert_eq!(
            alone,
            format!("Attached files:\n- {}", dir.path().display())
        );
        assert_eq!(with_files("just words", &[]).expect("valid"), "just words");
    }

    #[test]
    fn a_file_that_is_not_there_or_not_fully_named_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("nope.txt");
        assert!(matches!(
            with_files("x", std::slice::from_ref(&missing)),
            Err(Error::NoSuchAttachment(path)) if path == missing
        ));
        assert!(matches!(
            with_files("x", &[PathBuf::from("relative.txt")]),
            Err(Error::NoSuchAttachment(_))
        ));
        assert!(matches!(with_files("  ", &[]), Err(Error::EmptyMessage)));
    }
}
