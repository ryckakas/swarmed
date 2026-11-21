//! What a task has changed: the files that differ from where its branch started, and their diffs.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::{Core, git};

/// Diffs longer than this are cut off; nobody reads more, and the UI stays quick.
const MAX_DIFF_BYTES: usize = 400_000;
/// New files larger than this are listed without counting their lines.
const MAX_COUNTED_BYTES: u64 = 1_000_000;

/// How a file differs from the task's starting point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// The file is new, whether git tracks it yet or not.
    Added,
    /// The file existed and has different content.
    Modified,
    /// The file is gone.
    Deleted,
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileChange {
    /// The path, relative to the worktree.
    pub path: String,
    /// How it changed.
    pub kind: ChangeKind,
    /// Lines added; `None` for a binary or very large file.
    pub added: Option<u32>,
    /// Lines removed; `None` for a binary or very large file.
    pub removed: Option<u32>,
}

/// One file's changes as a unified diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDiff {
    /// The diff, in git's unified format.
    pub text: String,
    /// Whether the diff was too long and its end is missing.
    pub truncated: bool,
}

impl Core {
    /// The files a task has changed since its branch left its base: committed, staged, unstaged
    /// and new files alike, sorted by path.
    pub async fn task_changes(&self, project: &str, task: &str) -> Result<Vec<FileChange>> {
        let (_, task) = self.task(project, task)?;
        let worktree = &task.worktree_path;
        let base = starting_point(worktree, &task.base_ref).await;
        let kinds = git::git(
            worktree,
            &["diff", "--no-renames", "--name-status", "-z", &base],
        )
        .await?;
        let counts = git::git(
            worktree,
            &["diff", "--no-renames", "--numstat", "-z", &base],
        )
        .await?;
        let mut files = tracked_changes(&kinds, &counts);
        for path in untracked(worktree).await? {
            let lines = line_count(&worktree.join(&path));
            files.insert(path, (ChangeKind::Added, lines.map(|added| (added, 0))));
        }
        Ok(files.into_iter().map(file_change).collect())
    }

    /// One changed file's diff against the task's starting point.
    pub async fn task_diff(&self, project: &str, task: &str, path: &str) -> Result<FileDiff> {
        let (_, task) = self.task(project, task)?;
        let worktree = &task.worktree_path;
        if !stays_inside(path) {
            return Err(Error::PathOutsideWorktree(path.to_string()));
        }
        let text = if untracked(worktree).await?.iter().any(|p| p == path) {
            new_file_diff(path, &worktree.join(path))
        } else {
            let base = starting_point(worktree, &task.base_ref).await;
            git::git(worktree, &["diff", "--no-renames", &base, "--", path]).await?
        };
        Ok(cut(text))
    }
}

fn file_change((path, (kind, counts)): (String, (ChangeKind, Option<(u32, u32)>))) -> FileChange {
    FileChange {
        path,
        kind,
        added: counts.map(|(added, _)| added),
        removed: counts.map(|(_, removed)| removed),
    }
}

/// Whether a path from a front end names something in the worktree and nothing above it.
pub(crate) fn stays_inside(path: &str) -> bool {
    let mut components = Path::new(path).components();
    !path.is_empty() && components.all(|part| matches!(part, Component::Normal(_)))
}

/// The commit the task's branch started from. If the base is gone, only uncommitted work shows.
async fn starting_point(worktree: &Path, base_ref: &str) -> String {
    let merge_base = git::git(worktree, &["merge-base", "HEAD", base_ref]).await;
    merge_base.unwrap_or_else(|_| "HEAD".to_string())
}

/// The files in `worktree` that git neither tracks nor ignores.
pub(crate) async fn untracked(worktree: &Path) -> Result<Vec<String>> {
    let listed = git::git(
        worktree,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .await?;
    Ok(nul_separated(&listed).map(String::from).collect())
}

/// The parts of what git printed with `-z`: names as they are, whatever is in them, where
/// a listing by lines puts any unusual name in quotes with its bytes spelt out.
pub(crate) fn nul_separated(text: &str) -> impl Iterator<Item = &str> {
    text.split('\0').filter(|part| !part.is_empty())
}

type Tracked = BTreeMap<String, (ChangeKind, Option<(u32, u32)>)>;

/// Joins git's two views of the same changes: `--name-status -z` (`M\0path\0…`) and
/// `--numstat -z` (`added\tremoved\tpath\0…`, with `-` for a binary file's counts).
fn tracked_changes(kinds: &str, counts: &str) -> Tracked {
    let mut files = Tracked::new();
    let mut parts = nul_separated(kinds);
    while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
        let kind = match status.chars().next() {
            Some('A') => ChangeKind::Added,
            Some('D') => ChangeKind::Deleted,
            _ => ChangeKind::Modified,
        };
        files.insert(path.to_string(), (kind, None));
    }
    for line in nul_separated(counts) {
        let mut fields = line.splitn(3, '\t');
        let added = fields.next().and_then(|n| n.parse().ok());
        let removed = fields.next().and_then(|n| n.parse().ok());
        let entry = fields.next().and_then(|path| files.get_mut(path));
        if let Some(entry) = entry {
            entry.1 = added.zip(removed);
        }
    }
    files
}

/// The number of lines in a text file of reasonable size. A link is not read through: what
/// it points at may be anywhere on the machine, and is no part of the task's work.
fn line_count(file: &Path) -> Option<u32> {
    let found = std::fs::symlink_metadata(file).ok()?;
    if !found.is_file() || found.len() > MAX_COUNTED_BYTES {
        return None;
    }
    let text = String::from_utf8(std::fs::read(file).ok()?).ok()?;
    u32::try_from(text.lines().count()).ok()
}

/// A diff for a file git does not track yet, in the shape git gives an added file. A link is
/// shown as git shows one, by where it points, and never by what is there.
fn new_file_diff(path: &str, file: &Path) -> String {
    let header = format!("diff --git a/{path} b/{path}\nnew file\n--- /dev/null\n+++ b/{path}\n");
    let kind = std::fs::symlink_metadata(file).map(|found| found.file_type());
    if kind.as_ref().is_ok_and(std::fs::FileType::is_symlink) {
        let target = std::fs::read_link(file).unwrap_or_default();
        return format!(
            "{header}@@ -0,0 +1 @@\n+{}\n\\ No newline at end of file\n",
            target.display()
        );
    }
    let regular = kind.is_ok_and(|kind| kind.is_file());
    let text = regular.then(|| std::fs::read(file).ok()).flatten();
    let Some(text) = text.and_then(|bytes| String::from_utf8(bytes).ok()) else {
        return format!("{header}Binary file {path} is new\n");
    };
    let lines: Vec<&str> = text.lines().collect();
    let body = lines.iter().fold(String::new(), |mut body, line| {
        body.push('+');
        body.push_str(line);
        body.push('\n');
        body
    });
    format!("{header}@@ -0,0 +1,{} @@\n{body}", lines.len())
}

/// Cuts an overlong diff at a line end.
fn cut(text: String) -> FileDiff {
    if text.len() <= MAX_DIFF_BYTES {
        return FileDiff {
            text,
            truncated: false,
        };
    }
    let mut end = MAX_DIFF_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let kept = &text[..end];
    let whole_lines = kept.rfind('\n').map_or(kept, |at| &kept[..=at]);
    FileDiff {
        text: whole_lines.to_string(),
        truncated: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gits_two_listings_are_joined_by_path() {
        let kinds = "M\0src/a.rs\0A\0new file.txt\0D\0gone.md\0M\0logo.png\0";
        let counts = "3\t1\tsrc/a.rs\x0010\t0\tnew file.txt\x000\t7\tgone.md\x00-\t-\tlogo.png\0";
        let files: Vec<FileChange> = tracked_changes(kinds, counts)
            .into_iter()
            .map(file_change)
            .collect();
        let change = |path: &str, kind, counts: Option<(u32, u32)>| FileChange {
            path: path.to_string(),
            kind,
            added: counts.map(|c| c.0),
            removed: counts.map(|c| c.1),
        };
        assert_eq!(
            files,
            [
                change("gone.md", ChangeKind::Deleted, Some((0, 7))),
                change("logo.png", ChangeKind::Modified, None),
                change("new file.txt", ChangeKind::Added, Some((10, 0))),
                change("src/a.rs", ChangeKind::Modified, Some((3, 1))),
            ]
        );
    }

    #[test]
    fn paths_that_leave_the_worktree_are_refused() {
        assert!(stays_inside("src/a.rs"));
        assert!(!stays_inside("../secrets"));
        assert!(!stays_inside("src/../../secrets"));
        assert!(!stays_inside("/etc/passwd"));
        assert!(!stays_inside(""));
    }

    #[cfg(unix)]
    #[test]
    fn a_new_link_is_shown_by_where_it_points_and_not_by_what_is_there() {
        let outside = tempfile::tempdir().expect("temp dir");
        let secret = outside.path().join("credentials");
        std::fs::write(&secret, "key = hunter2\nmore\n").expect("writes");
        let worktree = tempfile::tempdir().expect("temp dir");
        let link = worktree.path().join("creds");
        std::os::unix::fs::symlink(&secret, &link).expect("links");

        assert_eq!(line_count(&link), None);
        let diff = new_file_diff("creds", &link);
        assert!(!diff.contains("hunter2"), "{diff}");
        assert!(diff.contains(&format!("+{}\n", secret.display())), "{diff}");
        // A file of the worktree's own is shown as before.
        std::fs::write(worktree.path().join("own.txt"), "a\nb\n").expect("writes");
        assert_eq!(line_count(&worktree.path().join("own.txt")), Some(2));
        let own = new_file_diff("own.txt", &worktree.path().join("own.txt"));
        assert!(own.ends_with("@@ -0,0 +1,2 @@\n+a\n+b\n"), "{own}");
    }

    #[test]
    fn an_overlong_diff_is_cut_at_a_line_end() {
        let line = "+é line\n";
        let long = line.repeat(MAX_DIFF_BYTES / line.len() + 10);
        let diff = cut(long);
        assert!(diff.truncated);
        assert!(diff.text.len() <= MAX_DIFF_BYTES);
        assert!(diff.text.ends_with(line));
        assert!(!cut("short\n".to_string()).truncated);
    }
}
