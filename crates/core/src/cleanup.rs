//! Clean-up: what removed tasks left behind, and the snapshots, listed and deleted on request.
//!
//! Four kinds of thing are listed:
//! - removed tasks' worktrees that are not deleted yet, in each project's [`REMOVED`] folder;
//! - the branches of removed tasks that Swarmed made for them and that are still there;
//! - stale worktrees: folders beside the tasks' own that no task owns, and git's records of
//!   worktrees whose folder is gone;
//! - snapshots, one per environment.
//!
//! Nothing of a current task, no remote branch, no branch Swarmed did not make and no
//! repository's own checkout is ever listed. What would lose work found nowhere else is
//! listed as at risk, and deleted only when it is named on its own. Deleting lists again
//! first, and deletes only what is still listed: much can change while the user reads.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::error::{Error, Result};
use crate::snapshots::SnapshotFolders;
use crate::store::{Project, RemovedBranch};
use crate::tasks::REMOVED;
use crate::usage::folder_bytes;
use crate::{Core, git, lock};

/// A kind of thing a clean-up deletes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupKind {
    /// Removed tasks' worktrees that are not deleted yet, or could not be.
    Removed,
    /// Branches Swarmed made for tasks that are removed, kept when they were.
    Branches,
    /// Folders beside the tasks' worktrees that no task owns, and git's records of worktrees
    /// whose folder is gone.
    Stale,
    /// Snapshots: what new tasks of each environment start from.
    Snapshots,
}

impl CleanupKind {
    /// Every kind, in the order they are shown.
    pub const ALL: [Self; 4] = [Self::Removed, Self::Branches, Self::Stale, Self::Snapshots];
}

/// One thing a clean-up can delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CleanupItem {
    /// What names it to [`Core::cleanup_clean`]: unique within its kind.
    pub key: String,
    /// The project it belongs to.
    pub project: String,
    /// What it is: a folder's path, a branch, or an environment.
    pub name: String,
    /// The task it was, or for a snapshot the task it was taken from, where known.
    pub task: Option<String>,
    /// Why it is there, or what deleting it would lose, in words.
    pub reason: String,
    /// What it takes up on disk, if it is on disk and could be measured.
    pub bytes: Option<u64>,
    /// Since when it is there, in seconds since 1970, where known: when its task was removed,
    /// when a folder last changed, when a snapshot was taken.
    pub since: Option<u64>,
    /// Whether deleting it would lose work found nowhere else, or work that could not be
    /// checked. Such an item is deleted only when it is named on its own.
    pub at_risk: bool,
    /// Where it is on disk, for a folder or a record of one.
    #[serde(skip)]
    path: Option<PathBuf>,
    /// Whether it is only git's record of a worktree, whose folder was gone when it was listed:
    /// no folder is ever deleted for one.
    #[serde(skip)]
    record: bool,
}

/// What can be cleaned up, kind by kind, as of when it was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cleanup {
    /// Removed tasks' worktrees that are not deleted yet.
    pub removed: Vec<CleanupItem>,
    /// Branches of removed tasks.
    pub branches: Vec<CleanupItem>,
    /// Stale worktrees: folders no task owns, and records whose folder is gone.
    pub stale: Vec<CleanupItem>,
    /// Snapshots.
    pub snapshots: Vec<CleanupItem>,
}

/// Something a clean-up was asked to delete and did not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotCleaned {
    /// Its key.
    pub key: String,
    /// What it is, as it was listed; the key itself when it is no longer listed.
    pub name: String,
    /// Why it is still there.
    pub reason: String,
}

/// What a clean-up did.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Cleaned {
    /// The keys of what was deleted.
    pub deleted: Vec<String>,
    /// What was not, each with why.
    pub kept: Vec<NotCleaned>,
}

impl Core {
    /// What can be cleaned up, every kind of it. Measures folders and asks git, so this can
    /// take a few seconds.
    pub async fn cleanup(&self) -> Result<Cleanup> {
        let (removed, branches, stale, snapshots) = tokio::join!(
            self.cleanup_items(CleanupKind::Removed),
            self.cleanup_items(CleanupKind::Branches),
            self.cleanup_items(CleanupKind::Stale),
            self.cleanup_items(CleanupKind::Snapshots),
        );
        Ok(Cleanup {
            removed: removed?,
            branches: branches?,
            stale: stale?,
            snapshots: snapshots?,
        })
    }

    /// What of one kind can be cleaned up, with sizes.
    pub async fn cleanup_items(&self, kind: CleanupKind) -> Result<Vec<CleanupItem>> {
        let items = self.listed(kind).await?;
        Ok(with_sizes(items).await)
    }

    /// Deletes things of one kind: those `keys` name, at risk or not, or without `keys` every
    /// one that is not at risk. What is deleted is listed again first, and only what is still
    /// listed goes; the rest is returned with why, as is what could not be deleted.
    ///
    /// A caller that showed the user a list says in `shown_at_risk` which of `keys` it showed
    /// as at risk. What is at risk now and was not shown so is kept: the list may be old, and
    /// the user agreed to lose nothing by it. Without it, naming a key is agreeing.
    pub async fn cleanup_clean(
        &self,
        kind: CleanupKind,
        keys: Option<&[String]>,
        shown_at_risk: Option<&[String]>,
    ) -> Result<Cleaned> {
        let listed = self.listed(kind).await?;
        let items: Vec<CleanupItem> = listed.into_iter().map(|(item, _)| item).collect();
        let (chosen, kept) = choose(&items, keys, shown_at_risk);
        let mut cleaned = Cleaned {
            deleted: vec![],
            kept,
        };
        for item in chosen {
            match self.clean_one(kind, item).await {
                Ok(()) => cleaned.deleted.push(item.key.clone()),
                Err(reason) => cleaned.kept.push(NotCleaned {
                    key: item.key.clone(),
                    name: item.name.clone(),
                    reason,
                }),
            }
        }
        Ok(cleaned)
    }

    /// What of one kind can be cleaned up, each with the folders its size is the sum of.
    async fn listed(&self, kind: CleanupKind) -> Result<Vec<Measured>> {
        match kind {
            CleanupKind::Removed => Ok(self.removed_items()),
            CleanupKind::Branches => self.branch_items().await,
            CleanupKind::Stale => self.stale_items().await,
            CleanupKind::Snapshots => self.snapshot_items(),
        }
    }

    /// Deletes one listed thing; why not, when it could not be.
    async fn clean_one(
        &self,
        kind: CleanupKind,
        item: &CleanupItem,
    ) -> std::result::Result<(), String> {
        match kind {
            CleanupKind::Removed => delete_folder(item_path(item)?).await,
            CleanupKind::Branches => self.delete_branch(item).await,
            CleanupKind::Stale => self.delete_stale(item).await,
            CleanupKind::Snapshots => self.forget_snapshot(item),
        }
    }

    /// What every project's removals moved aside and is still there.
    fn removed_items(&self) -> Vec<Measured> {
        let Ok(projects) = std::fs::read_dir(&self.paths.worktrees_root) else {
            return vec![];
        };
        projects
            .flatten()
            .flat_map(|project| removed_in(&project.path()))
            .collect()
    }

    /// The branches of every project's removed tasks that can be deleted.
    async fn branch_items(&self) -> Result<Vec<Measured>> {
        let projects = self.projects()?;
        let mut items = vec![];
        // The chats have no repository, and no branch.
        for project in projects.iter().filter(|project| !project.chats) {
            items.extend(self.project_branch_items(project, &projects).await?);
        }
        Ok(items)
    }

    /// The branches Swarmed made for a project's removed tasks that are still there and that
    /// nothing current uses.
    async fn project_branch_items(
        &self,
        project: &Project,
        projects: &[Project],
    ) -> Result<Vec<Measured>> {
        let removed = made_by_swarmed(self.store().removed_branches(project.id)?);
        if removed.is_empty() {
            return Ok(vec![]);
        }
        // A repository that cannot be asked, one moved or on a disk that is away, has no
        // branch that could be checked: none is listed, and the other kinds still are.
        let Some(in_use) = self.branches_in_use(project, projects).await? else {
            return Ok(vec![]);
        };
        let mut items = vec![];
        for branch in removed {
            items.extend(removable_branch(project, &in_use, branch).await);
        }
        Ok(items)
    }

    /// The branches of the repository that something current uses, and which are never
    /// deleted: every current task's own, the others its worktree was seen on and its base;
    /// the project's configured base; and whatever any worktree of the repository has checked
    /// out, the repository's own checkout included. Over every project of the same repository.
    /// `None` when git cannot say what the repository's worktrees have checked out.
    async fn branches_in_use(
        &self,
        project: &Project,
        projects: &[Project],
    ) -> Result<Option<HashSet<String>>> {
        let mut in_use = HashSet::new();
        let here = resolved(&project.repo_path);
        let same = projects
            .iter()
            .filter(|other| resolved(&other.repo_path) == here);
        for other in same {
            in_use.extend(self.branches_used_by(other)?);
        }
        let Ok(worktrees) = git::worktree_list(&project.repo_path).await else {
            return Ok(None);
        };
        in_use.extend(worktrees.into_iter().filter_map(|record| record.branch));
        Ok(Some(in_use))
    }

    /// The branches one project's current tasks use, and the base it is configured with.
    fn branches_used_by(&self, project: &Project) -> Result<Vec<String>> {
        let tasks = self.store().tasks(project.id, false)?;
        let mut used = vec![];
        for task in tasks {
            used.extend(self.store().task_branches(task.id)?);
            used.push(local_name(&task.base_ref).to_string());
            used.push(task.branch);
        }
        // A configuration that cannot be read names no base, and costs this nothing.
        let config = self.project_config(&project.name).ok();
        used.extend(config.map(|config| local_name(&config.base_branch).to_string()));
        Ok(used)
    }

    /// Deletes a listed branch with `git branch -D`.
    async fn delete_branch(&self, item: &CleanupItem) -> std::result::Result<(), String> {
        let project = self
            .project(&item.project)
            .map_err(|error| error.to_string())?;
        let repo = project.repo().map_err(|error| error.to_string())?;
        git::branch_delete(repo, &item.name)
            .await
            .map_err(|error| format!("git would not delete it: {}", said(&error)))
    }

    /// Every project's stale worktrees: folders beside its tasks' that no task owns, and
    /// git's records of worktrees whose folder is gone.
    async fn stale_items(&self) -> Result<Vec<Measured>> {
        let projects = self.projects()?;
        let keep = self.kept_places(&projects)?;
        let mut seen = HashSet::new();
        let mut items = vec![];
        for project in &projects {
            let area = self.paths.worktrees_root.join(&project.name);
            items.extend(stale_folder_items(&project.name, stale_folders(&area, &keep)).await);
            // Chats' folders are no worktrees: git has no records of them.
            let records = match project.repo() {
                Ok(repo) => gone_records(repo, &keep).await,
                Err(_) => vec![],
            };
            let new = records.into_iter().filter(|path| seen.insert(path.clone()));
            items.extend(new.map(|path| record_item(&project.name, path)));
        }
        Ok(items)
    }

    /// The places a stale worktree is never looked for in: current tasks' worktrees, those of
    /// tasks being made, and the repositories themselves.
    fn kept_places(&self, projects: &[Project]) -> Result<Kept> {
        let by_id: HashMap<i64, &str> = projects
            .iter()
            .map(|project| (project.id, project.name.as_str()))
            .collect();
        // The names claimed are read before the tasks: a task is written down and only then
        // lets go of its name, so one being made is in at least one of the two. Read under the
        // lock, resolved after it: tasks being made wait on it.
        let being_made: Vec<PathBuf> = lock(&self.names_claimed)
            .iter()
            .filter_map(|(project, name)| Some(self.paths.worktree(by_id.get(project)?, name)))
            .collect();
        let tasks = self.store().tasks_all()?;
        let current = tasks.iter().filter(|task| !task.archived);
        let mut owned: HashSet<PathBuf> =
            current.map(|task| resolved(&task.worktree_path)).collect();
        owned.extend(being_made.iter().map(|path| resolved(path)));
        let repos = projects
            .iter()
            .filter_map(|project| project.repo().ok())
            .map(resolved)
            .collect();
        Ok(Kept { owned, repos })
    }

    /// Deletes a stale folder and git's record of it, or only the record for an item listed as
    /// one. A record's folder is never deleted: one that is there again is somebody's, made
    /// since, and nothing looked at what it holds.
    async fn delete_stale(&self, item: &CleanupItem) -> std::result::Result<(), String> {
        let path = item_path(item)?;
        let project = self
            .project(&item.project)
            .map_err(|error| error.to_string())?;
        // A chat's folder was never a worktree, and has no record.
        let repo = project.repo().ok();
        if item.record {
            let repo = repo.ok_or_else(|| Error::NoRepository.to_string())?;
            return forget_record(repo, path).await;
        }
        delete_folder(path).await?;
        // A folder that was a worktree of the repository leaves git's record of it, which
        // would keep its branch from being checked out anywhere else. One that was not has
        // no record to forget.
        if let Some(repo) = repo {
            git::worktree_remove(repo, path).await.ok();
        }
        Ok(())
    }

    /// Every project's snapshots, with what a taking that was cut short left.
    fn snapshot_items(&self) -> Result<Vec<Measured>> {
        let mut items = vec![];
        for project in self.projects()? {
            let found = self.snapshot_folders(&project.name);
            items.extend(
                found
                    .into_iter()
                    .map(|one| snapshot_item(&project.name, one)),
            );
        }
        Ok(items)
    }

    /// Forgets a listed snapshot, as the environments dialog does: refused while one is being
    /// taken.
    fn forget_snapshot(&self, item: &CleanupItem) -> std::result::Result<(), String> {
        let (project, environment) = item.key.split_once(':').unwrap_or((&item.key, ""));
        let environment = (!environment.is_empty()).then_some(environment);
        self.snapshot_remove(project, environment)
            .map_err(|error| error.to_string())
    }
}

/// An item, with the folders whose sizes add up to its own.
type Measured = (CleanupItem, Vec<PathBuf>);

/// The items, each with its size, all measured at once.
async fn with_sizes(items: Vec<Measured>) -> Vec<CleanupItem> {
    let measuring: Vec<_> = items
        .into_iter()
        .map(|(item, folders)| (item, tokio::spawn(folders_bytes(folders))))
        .collect();
    let mut measured = Vec::with_capacity(measuring.len());
    for (mut item, size) in measuring {
        item.bytes = size.await.ok().flatten();
        measured.push(item);
    }
    measured
}

/// What the folders take up together; `None` when none of them could be measured.
async fn folders_bytes(folders: Vec<PathBuf>) -> Option<u64> {
    let mut sum = None;
    for folder in &folders {
        if let Some(bytes) = folder_bytes(folder).await {
            sum = Some(sum.unwrap_or(0) + bytes);
        }
    }
    sum
}

/// Which of `items` to delete: those `keys` names, or without them every one not at risk.
/// A key that names nothing listed is returned as kept: what it named is gone already, or is
/// in use again. So is one at risk now that the caller did not show as at risk
/// (`shown_at_risk`), with what it would lose.
fn choose<'a>(
    items: &'a [CleanupItem],
    keys: Option<&[String]>,
    shown_at_risk: Option<&[String]>,
) -> (Vec<&'a CleanupItem>, Vec<NotCleaned>) {
    let Some(keys) = keys else {
        return (items.iter().filter(|item| !item.at_risk).collect(), vec![]);
    };
    let risky_since = |item: &CleanupItem| {
        item.at_risk && shown_at_risk.is_some_and(|shown| !shown.contains(&item.key))
    };
    let named = || items.iter().filter(|item| keys.contains(&item.key));
    let chosen = named().filter(|item| !risky_since(item)).collect();
    let risky = named()
        .filter(|item| risky_since(item))
        .map(|item| NotCleaned {
            key: item.key.clone(),
            name: item.name.clone(),
            reason: format!("since it was listed, {}", item.reason),
        });
    let unlisted = keys
        .iter()
        .filter(|key| !items.iter().any(|item| &item.key == *key))
        .map(|key| NotCleaned {
            key: key.clone(),
            name: key.clone(),
            reason: "it is no longer listed: gone already, or in use again".to_string(),
        });
    (chosen, risky.chain(unlisted).collect())
}

/// What a project's removals moved aside and is still there, in the project's folder beside
/// its worktrees.
fn removed_in(project: &Path) -> Vec<Measured> {
    let name = project.file_name().unwrap_or_default().to_string_lossy();
    let Ok(left) = std::fs::read_dir(project.join(REMOVED)) else {
        return vec![];
    };
    let item = |entry: std::fs::DirEntry| removed_item(&name, entry.path());
    left.flatten().map(item).collect()
}

/// A removed task's worktree, from the name it was moved aside under:
/// `<task>-<id>-<nanoseconds>`, which says when.
fn removed_item(project: &str, path: PathBuf) -> Measured {
    let folder = path.file_name().unwrap_or_default().to_string_lossy();
    let (task, since) = removed_name(&folder);
    let item = CleanupItem {
        key: path.to_string_lossy().into_owned(),
        project: project.to_string(),
        name: path.to_string_lossy().into_owned(),
        task,
        reason: "its task was removed, and it was not deleted, or could not be".to_string(),
        bytes: None,
        since: since.or_else(|| changed_at(&path)),
        at_risk: false,
        path: Some(path.clone()),
        record: false,
    };
    (item, vec![path])
}

/// The task a folder moved aside was the worktree of, and when it was moved, read from its
/// name; nothing for a name of another form.
fn removed_name(folder: &str) -> (Option<String>, Option<u64>) {
    let mut parts = folder.rsplitn(3, '-');
    let (Some(nanos), Some(id), Some(task)) = (parts.next(), parts.next(), parts.next()) else {
        return (None, None);
    };
    let (Ok(nanos), Ok(_)) = (nanos.parse::<u128>(), id.parse::<i64>()) else {
        return (None, None);
    };
    let seconds = u64::try_from(nanos / 1_000_000_000).ok();
    (Some(task.to_string()), seconds)
}

/// When what is at `path` last changed, in seconds since 1970.
fn changed_at(path: &Path) -> Option<u64> {
    let changed = std::fs::symlink_metadata(path).ok()?.modified().ok()?;
    Some(changed.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

/// The removed tasks' branches that Swarmed made, once each, the latest removed first. A
/// branch a task checked out as it was is not Swarmed's to delete, even if a task made a
/// branch of that name before. A task made before that was kept counts as removal counts
/// it: its branch, unless it started from it, which is checked against git.
fn made_by_swarmed(removed: Vec<RemovedBranch>) -> Vec<RemovedBranch> {
    let not_made: HashSet<String> = removed
        .iter()
        .filter(|branch| branch.made == Some(false))
        .map(|branch| branch.branch.clone())
        .collect();
    let mut seen = HashSet::new();
    removed
        .into_iter()
        .filter(|branch| !not_made.contains(&branch.branch))
        .filter(|branch| seen.insert(branch.branch.clone()))
        .collect()
}

/// A ref as the name of a local branch, if it is written as one.
fn local_name(name: &str) -> &str {
    let name = name.strip_prefix("refs/").unwrap_or(name);
    name.strip_prefix("heads/").unwrap_or(name)
}

/// Whether the task started from its branch rather than making it: its base names it.
async fn started_from(repo: &Path, branch: &RemovedBranch) -> bool {
    local_name(&branch.base_ref) == branch.branch
        || git::names_branch(repo, &branch.base_ref, &branch.branch)
            .await
            .unwrap_or(false)
}

/// A removed task's branch as an item, unless something current uses it, it is gone, or the
/// task started from it.
async fn removable_branch(
    project: &Project,
    in_use: &HashSet<String>,
    branch: RemovedBranch,
) -> Option<Measured> {
    let repo = project.repo().ok()?;
    if in_use.contains(&branch.branch)
        || !git::local_branch_exists(repo, &branch.branch).await
        || started_from(repo, &branch).await
    {
        return None;
    }
    let only_here = git::commits_only_on(repo, &branch.branch, &branch.base_ref).await;
    Some((branch_item(&project.name, &branch, only_here), vec![]))
}

/// A removed task's branch, with what deleting it would lose.
fn branch_item(project: &str, branch: &RemovedBranch, only_here: Result<u32>) -> CleanupItem {
    let base = &branch.base_ref;
    let (reason, at_risk) = match only_here {
        Ok(0) => (
            format!("everything on it is also on {base} or a remote"),
            false,
        ),
        Ok(1) => (
            format!("1 commit on it is on neither {base} nor a remote"),
            true,
        ),
        Ok(count) => (
            format!("{count} commits on it are on neither {base} nor a remote"),
            true,
        ),
        Err(error) => (
            format!(
                "it could not be compared with {base}, so it may hold work found nowhere else: {}",
                said(&error)
            ),
            true,
        ),
    };
    CleanupItem {
        key: format!("{project}:{}", branch.branch),
        project: project.to_string(),
        name: branch.branch.clone(),
        task: Some(branch.task.clone()),
        reason,
        bytes: None,
        since: branch.removed_at,
        at_risk,
        path: None,
        record: false,
    }
}

/// Where a stale worktree is never looked for.
struct Kept {
    /// Current tasks' worktrees and those of tasks being made, resolved.
    owned: HashSet<PathBuf>,
    /// The repositories' own folders, resolved.
    repos: Vec<PathBuf>,
}

impl Kept {
    /// Whether `path` is a current task's worktree, or one being made.
    fn owns(&self, path: &Path) -> bool {
        self.owned.contains(&resolved(path))
    }

    /// Whether the folder at `path` is to be left alone: what a current task owns, a
    /// repository, in one or holding one, or a repository of its own, which may be anybody's
    /// checkout or clone.
    fn keeps(&self, path: &Path) -> bool {
        let place = resolved(path);
        let repository = |repo: &PathBuf| repo.starts_with(&place) || place.starts_with(repo);
        self.owned.contains(&place)
            || self.repos.iter().any(repository)
            || path.join(".git").is_dir()
    }
}

/// The folders in a project's place beside its worktrees that no task owns. Only folders: a
/// link is not followed, and is not one.
fn stale_folders(area: &Path, keep: &Kept) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(area) else {
        return vec![];
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name() != REMOVED)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|path| !keep.keeps(path))
        .collect()
}

/// The folders no task owns, as items.
async fn stale_folder_items(project: &str, folders: Vec<PathBuf>) -> Vec<Measured> {
    let mut items = Vec::with_capacity(folders.len());
    for folder in folders {
        items.push(stale_folder_item(project, folder).await);
    }
    items
}

/// The worktrees of the repository at `repo` whose folder is gone, which no current task owns.
/// Locked ones are left alone, as git leaves them: their folder may only be out of reach. A
/// repository that cannot be asked, one deleted by hand say, has none to clean up here.
///
/// Git calls a record prunable when the folder's `.git` file is missing, which a folder made
/// again in its place, with anything in it, also is. So the folder itself is looked for.
async fn gone_records(repo: &Path, keep: &Kept) -> Vec<PathBuf> {
    let records = git::worktree_list(repo).await.unwrap_or_default();
    records
        .into_iter()
        .filter(|record| record.prunable && !record.locked && !keep.owns(&record.path))
        .filter(|record| !is_there(&record.path))
        .map(|record| record.path)
        .collect()
}

/// Has git forget its record of the worktree at `path`, whose folder was gone when it was
/// listed. Refused if something is there again, which is left alone.
async fn forget_record(repo: &Path, path: &Path) -> std::result::Result<(), String> {
    if is_there(path) {
        return Err("a folder is there again, which is left alone".to_string());
    }
    git::worktree_remove(repo, path)
        .await
        .map_err(|error| format!("git would not forget it: {}", said(&error)))
}

/// Whether anything is at `path`: a folder, a file, or a link, wherever it leads.
fn is_there(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// A folder no task owns, with what deleting it would lose, if anything.
async fn stale_folder_item(project: &str, folder: PathBuf) -> Measured {
    let risk = work_in(&folder).await;
    let item = CleanupItem {
        key: folder.to_string_lossy().into_owned(),
        project: project.to_string(),
        name: folder.to_string_lossy().into_owned(),
        task: None,
        at_risk: risk.is_some(),
        reason: risk.unwrap_or_else(|| "no task owns this folder".to_string()),
        bytes: None,
        since: changed_at(&folder),
        path: Some(folder.clone()),
        record: false,
    };
    (item, vec![folder])
}

/// A record git keeps of a worktree whose folder is gone.
fn record_item(project: &str, path: PathBuf) -> Measured {
    let item = CleanupItem {
        key: path.to_string_lossy().into_owned(),
        project: project.to_string(),
        name: path.to_string_lossy().into_owned(),
        task: None,
        reason: "git still keeps a record of this worktree, whose folder is gone".to_string(),
        bytes: None,
        since: None,
        at_risk: false,
        path: Some(path),
        record: true,
    };
    (item, vec![])
}

/// What deleting a git worktree's folder would lose that is found nowhere else: changes never
/// committed, or commits on no branch. `None` for one that holds no such work, and for a
/// folder that is no worktree.
async fn work_in(folder: &Path) -> Option<String> {
    if !folder.join(".git").is_file() {
        return None;
    }
    let unsure =
        |error: &Error| format!("git could not tell whether it holds work: {}", said(error));
    match git::is_dirty(folder).await {
        Ok(true) => return Some("it holds changes that were never committed".to_string()),
        Ok(false) => {}
        Err(error) => return Some(unsure(&error)),
    }
    match git::commits_on_no_branch(folder).await {
        Ok(0) => None,
        Ok(1) => Some("1 commit in it is on no branch and no remote".to_string()),
        Ok(count) => Some(format!(
            "{count} commits in it are on no branch and no remote"
        )),
        Err(error) => Some(unsure(&error)),
    }
}

/// An environment's snapshot, or what a taking of one that was cut short left.
fn snapshot_item(project: &str, found: SnapshotFolders) -> Measured {
    let environment = found.environment.clone().unwrap_or_default();
    let name = found
        .environment
        .unwrap_or_else(|| "tasks without an environment".to_string());
    let (task, since, reason) = match found.snapshot {
        Some(snapshot) => (
            Some(snapshot.task),
            Some(snapshot.taken_at),
            "new tasks start from it; without it, their setup does all the work again".to_string(),
        ),
        None => (
            None,
            found.folders.first().and_then(|folder| changed_at(folder)),
            "the taking of a snapshot was cut short and left this".to_string(),
        ),
    };
    let item = CleanupItem {
        key: format!("{project}:{environment}"),
        project: project.to_string(),
        name,
        task,
        reason,
        bytes: None,
        since,
        at_risk: false,
        path: None,
        record: false,
    };
    (item, found.folders)
}

/// The folder or record an item of a kind on disk stands for.
fn item_path(item: &CleanupItem) -> std::result::Result<&Path, String> {
    item.path
        .as_deref()
        .ok_or_else(|| "it is not on disk".to_string())
}

/// Deletes a folder and everything in it. A folder some tool left without write permission,
/// such as Go's module cache, is made writable and deleted again.
async fn delete_folder(folder: &Path) -> std::result::Result<(), String> {
    let mut said = rm(folder).await;
    if std::fs::symlink_metadata(folder).is_ok() {
        // Symbolic links inside are neither followed nor changed.
        let writable = Command::new("chmod")
            .args(["-R", "u+w", "--"])
            .arg(folder)
            .output()
            .await;
        drop(writable);
        said = rm(folder).await;
    }
    match std::fs::symlink_metadata(folder) {
        Ok(_) => Err(format!("it could not be deleted: {said}")),
        Err(_) => Ok(()),
    }
}

/// Runs `rm -rf` on `folder`; the first thing it complained of, if anything.
async fn rm(folder: &Path) -> String {
    let deleting = Command::new("rm")
        .args(["-rf", "--"])
        .arg(folder)
        .output()
        .await;
    match deleting {
        Ok(output) => {
            let said = String::from_utf8_lossy(&output.stderr);
            said.lines().next().unwrap_or("rm failed").to_string()
        }
        Err(error) => error.to_string(),
    }
}

/// What git said when it refused, without the command it was given; other errors as they are.
fn said(error: &Error) -> String {
    match error {
        Error::Git { stderr, .. } => {
            let first = stderr.lines().next().unwrap_or("it gave no reason");
            first.strip_prefix("error: ").unwrap_or(first).to_string()
        }
        other => other.to_string(),
    }
}

/// `path` with its links resolved, so that two ways of writing one place compare equal. A
/// path that is not there is resolved as far as its folder is.
fn resolved(path: &Path) -> PathBuf {
    if let Ok(real) = std::fs::canonicalize(path) {
        return real;
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return path.to_path_buf();
    };
    std::fs::canonicalize(parent).map_or_else(|_| path.to_path_buf(), |real| real.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, at_risk: bool) -> CleanupItem {
        CleanupItem {
            key: key.to_string(),
            project: "demo".to_string(),
            name: key.to_string(),
            task: None,
            reason: String::new(),
            bytes: None,
            since: None,
            at_risk,
            path: None,
            record: false,
        }
    }

    #[test]
    fn what_is_at_risk_goes_only_when_it_is_named() {
        let items = [item("a", false), item("b", true), item("c", false)];
        let keys = |chosen: Vec<&CleanupItem>| -> Vec<String> {
            chosen.into_iter().map(|item| item.key.clone()).collect()
        };
        let (all, kept) = choose(&items, None, None);
        assert_eq!(keys(all), ["a", "c"]);
        assert_eq!(kept, []);
        let named = ["b".to_string(), "gone".to_string()];
        let (chosen, kept) = choose(&items, Some(&named), None);
        assert_eq!(keys(chosen), ["b"]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].key, "gone");
    }

    #[test]
    fn what_became_at_risk_since_it_was_shown_is_kept() {
        let items = [item("a", false), item("b", true), item("c", true)];
        let named = ["a".to_string(), "b".to_string(), "c".to_string()];
        // The user was shown `c` as at risk and ticked it; `b` looked safe then.
        let shown = ["c".to_string()];
        let (chosen, kept) = choose(&items, Some(&named), Some(&shown));
        let chosen: Vec<&str> = chosen.iter().map(|item| item.key.as_str()).collect();
        assert_eq!(chosen, ["a", "c"]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].key, "b");
        assert!(kept[0].reason.starts_with("since it was listed"));
    }

    #[test]
    fn a_folder_moved_aside_says_its_task_and_when() {
        assert_eq!(
            removed_name("fix-login-12-1700000000123456789"),
            (Some("fix-login".to_string()), Some(1_700_000_000))
        );
        assert_eq!(removed_name("leftover"), (None, None));
        assert_eq!(removed_name("a-b-c"), (None, None));
    }

    fn removed(task: &str, branch: &str, made: Option<bool>) -> RemovedBranch {
        RemovedBranch {
            task: task.to_string(),
            branch: branch.to_string(),
            base_ref: "main".to_string(),
            made,
            removed_at: None,
        }
    }

    #[test]
    fn only_branches_swarmed_made_are_listed_and_each_once() {
        let listed = made_by_swarmed(vec![
            removed("two", "jd/one", Some(true)),
            removed("one", "jd/one", Some(true)),
            removed("old", "jd/old", None),
            removed("theirs", "release", Some(false)),
            // Made once, and later checked out as it was by another task.
            removed("again", "jd/again", Some(false)),
            removed("first", "jd/again", Some(true)),
        ]);
        let names: Vec<(&str, &str)> = listed
            .iter()
            .map(|branch| (branch.task.as_str(), branch.branch.as_str()))
            .collect();
        assert_eq!(names, [("two", "jd/one"), ("old", "jd/old")]);
    }

    #[test]
    fn a_ref_is_read_as_a_local_branch_where_it_names_one() {
        assert_eq!(local_name("refs/heads/main"), "main");
        assert_eq!(local_name("heads/main"), "main");
        assert_eq!(local_name("main"), "main");
        assert_eq!(local_name("origin/main"), "origin/main");
    }
}
