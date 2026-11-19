//! Notes the user keeps on a project or on one of its tasks.
//!
//! A note is one plain text file in the data directory. It may hold credentials, so it is
//! readable by the user alone, never logged and never handed to an agent.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use crate::Core;
use crate::error::{Error, Result};

/// The longest a note may be, in bytes. It is a note, not a place to keep files.
pub(crate) const MAX_NOTE_BYTES: usize = 200_000;

impl Core {
    /// The note kept on `project`, or on its `task` when one is named. Empty if there is none.
    pub fn note(&self, project: &str, task: Option<&str>) -> Result<String> {
        match std::fs::read_to_string(self.note_file(project, task)?) {
            Ok(text) => Ok(text),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// Replaces the note on `project`, or on its `task`. A note of nothing but blank space is
    /// removed.
    pub fn set_note(&self, project: &str, task: Option<&str>, text: &str) -> Result<()> {
        let file = self.note_file(project, task)?;
        if text.trim().is_empty() {
            return remove(&file);
        }
        if text.len() > MAX_NOTE_BYTES {
            return Err(Error::NoteTooLong(MAX_NOTE_BYTES));
        }
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Created for the user alone: what is noted down may be a password.
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        options.open(&file)?.write_all(text.as_bytes())?;
        Ok(())
    }

    /// Whether anything is noted on `project`, or on its `task`. Names are not checked: this is
    /// asked for every row of the overview.
    pub(crate) fn has_note(&self, project: &str, task: Option<&str>) -> bool {
        self.paths.note(project, task).exists()
    }

    /// Removes a task's note along with the task, so a later task of the same name starts
    /// without it.
    pub(crate) fn forget_note(&self, project: &str, task: &str) -> Result<()> {
        remove(&self.paths.note(project, Some(task)))
    }

    /// Removes every note on a project and on its tasks, along with the project.
    pub(crate) fn forget_notes(&self, project: &str) -> Result<()> {
        match std::fs::remove_dir_all(self.paths.notes(project)) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }

    /// Where the note is kept, once the project and the task are known to exist.
    fn note_file(&self, project: &str, task: Option<&str>) -> Result<PathBuf> {
        match task {
            Some(task) => drop(self.task(project, task)?),
            None => drop(self.project(project)?),
        }
        Ok(self.paths.note(project, task))
    }
}

/// Removes a note's file. One that is not there is already removed.
fn remove(file: &std::path::Path) -> Result<()> {
    match std::fs::remove_file(file) {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}
