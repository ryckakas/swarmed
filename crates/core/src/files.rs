//! A task's files as they lie in its worktree: one folder at a time, for a front end to walk
//! through, and a way to open one of them in the user's editor.

use std::path::Path;

use serde::Serialize;

use crate::changes::stays_inside;
use crate::error::{Error, Result};
use crate::{Core, open};

/// The most entries of one folder that are listed. A folder with more says how many are left
/// out: nobody reads further down such a list, and the editor shows all of it.
const MAX_ENTRIES: usize = 1000;

/// One file or folder in a folder of a task's worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileEntry {
    /// Its name within its folder.
    pub name: String,
    /// Whether it is a folder that can be listed in turn. A link to a folder is not: it may
    /// lead out of the worktree.
    pub dir: bool,
}

/// What one folder of a task's worktree holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileListing {
    /// Folders first, then files, each by name without regard to case.
    pub entries: Vec<FileEntry>,
    /// How many more there are than are listed.
    pub more: usize,
}

/// What `folder` holds, without git's own folder.
fn listed(folder: &Path) -> Result<FileListing> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        // Not followed: a link is listed as what it is, not as what it points at.
        let dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        entries.push(FileEntry { name, dir });
    }
    entries.sort_by_key(|entry| (!entry.dir, entry.name.to_lowercase()));
    let more = entries.len().saturating_sub(MAX_ENTRIES);
    entries.truncate(MAX_ENTRIES);
    Ok(FileListing { entries, more })
}

impl Core {
    /// What the folder `dir` of a task's worktree holds; the worktree itself for an empty
    /// `dir`. `dir` is relative to the worktree and has to stay inside it.
    pub fn task_files(&self, project: &str, task: &str, dir: &str) -> Result<FileListing> {
        let (_, task) = self.task(project, task)?;
        if !dir.is_empty() && !stays_inside(dir) {
            return Err(Error::PathOutsideWorktree(dir.to_string()));
        }
        listed(&task.worktree_path.join(dir))
    }

    /// Opens the file `path` of a task's worktree in the user's editor, in the window of the
    /// worktree where the editor has such a thing.
    pub fn task_open_file(&self, project: &str, task: &str, path: &str) -> Result<()> {
        let (_, task) = self.task(project, task)?;
        if !stays_inside(path) {
            return Err(Error::PathOutsideWorktree(path.to_string()));
        }
        let file = task.worktree_path.join(path);
        if !file.exists() {
            return Err(Error::PathOutsideWorktree(path.to_string()));
        }
        open::file_in_editor(&self.settings()?.editor, &task.worktree_path, &file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_lists_folders_first_and_leaves_gits_own_out() {
        let dir = tempfile::tempdir().expect("temp dir");
        for folder in [".git", "src", "Docs"] {
            std::fs::create_dir(dir.path().join(folder)).expect("dir");
        }
        for file in ["b.txt", "A.md", ".env"] {
            std::fs::write(dir.path().join(file), "").expect("writes");
        }
        let listing = listed(dir.path()).expect("lists");
        let shown: Vec<(&str, bool)> = listing
            .entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.dir))
            .collect();
        assert_eq!(
            shown,
            [
                ("Docs", true),
                ("src", true),
                (".env", false),
                ("A.md", false),
                ("b.txt", false)
            ]
        );
        assert_eq!(listing.more, 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_folder_is_not_one_to_walk_into() {
        let dir = tempfile::tempdir().expect("temp dir");
        let outside = tempfile::tempdir().expect("temp dir");
        std::os::unix::fs::symlink(outside.path(), dir.path().join("elsewhere")).expect("links");
        let listing = listed(dir.path()).expect("lists");
        assert_eq!(listing.entries.len(), 1);
        assert!(!listing.entries[0].dir);
    }

    #[test]
    fn a_folder_with_too_much_says_how_much_is_left_out() {
        let dir = tempfile::tempdir().expect("temp dir");
        for number in 0..MAX_ENTRIES + 3 {
            std::fs::write(dir.path().join(format!("f{number}")), "").expect("writes");
        }
        let listing = listed(dir.path()).expect("lists");
        assert_eq!((listing.entries.len(), listing.more), (MAX_ENTRIES, 3));
    }
}
