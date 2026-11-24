//! Chats: sessions with no repository.
//!
//! A chat is a task of a home of its own, [`CHATS`], which is a row among the projects with no
//! repository behind it. Each chat has a folder of its own under the worktrees root, which is
//! not a git repository, and no branch. Everything sessions do works there as in a task; what
//! only a repository has (changes, publishing, pull requests, stacks, the cloud) is refused
//! with [`Error::NoRepository`].

use std::path::Path;

use crate::error::{Error, Result};
use crate::hooks::OutputSink;
use crate::store::{NewTask, Project, SetupStatus, Task};
use crate::tasks::{BranchPattern, NewTaskOptions, delete_later, move_aside};
use crate::{Core, EnvironmentChoice, leftovers};

/// The name the home of the chats is kept under among the projects.
///
/// A repository's name starts with a lower-case letter or a digit (`paths::validate_name`), so
/// no repository can ever take it, whatever a front end calls the chats. Their folders are
/// under it in the worktrees root, where no repository's folder can be either.
pub const CHATS: &str = ".chats";

/// What the agent of a chat is told in place of the guidance about a task's branch and
/// worktree: there is neither.
pub(crate) const INSTRUCTIONS: &str = "\
# No repository

This session is a chat: the user started it with no repository. You work in an empty folder of \
your own that Swarmed made for this chat, which is not a git repository. There is no branch, no \
worktree, no pull request and nothing to publish. Where other guidance speaks of your worktree, \
it means this folder.

When the user names files or folders elsewhere on this machine, read them where they are. Keep \
anything you write for the user in this folder, unless the user asks for it somewhere else.";

impl Core {
    /// The home of the chats, made the first time it is needed.
    pub(crate) fn chats_home(&self) -> Result<Project> {
        let store = self.store();
        if let Some(home) = store.project_by_name(CHATS)? {
            return Ok(home);
        }
        // No repository: the folder the chats' own folders are in, which is none.
        store.project_insert(CHATS, &self.paths.worktrees_root.join(CHATS))
    }

    /// The active task called `name` with its project, if the project has a repository.
    pub(crate) fn repo_task(&self, project: &str, name: &str) -> Result<(Project, Task)> {
        let (project, task) = self.task(project, name)?;
        project.repo()?;
        Ok((project, task))
    }

    /// Whether `task` is a chat.
    pub(crate) fn is_chat(&self, task: &Task) -> bool {
        let project = self.store().project_by_id(task.project_id);
        project.ok().flatten().is_some_and(|project| project.chats)
    }

    /// Makes a chat: an empty folder of its own, and a task for it in the home of the chats,
    /// which is made if it is not there yet. Named as [`Core::task_new`] names a task, with no
    /// branch to keep free. It has no setup to run, so it is set up from the start.
    pub(crate) async fn chat_new(&self, name: &str, options: &NewTaskOptions<'_>) -> Result<Task> {
        let repository_only = options.existing_branch.is_some()
            || matches!(options.environment, EnvironmentChoice::Named(_));
        if repository_only {
            return Err(Error::NoRepository);
        }
        let home = self.chats_home()?;
        let pattern = BranchPattern {
            project: &home,
            pattern: "",
            prefix: String::new(),
        };
        let claim = self.name_for_task(&home, name, &pattern, options).await?;
        let folder = self.paths.worktree(&home.name, &claim.name);
        tokio::fs::create_dir_all(&folder).await?;
        let new = NewTask {
            project_id: home.id,
            name: &claim.name,
            branch: "",
            base_ref: "",
            worktree_path: &folder,
            slot: 0,
            environment: None,
            branch_made: false,
        };
        let task = match self.insert_in_free_slot(new) {
            Ok(task) => task,
            Err(error) => {
                tokio::fs::remove_dir_all(&folder).await.ok();
                return Err(error);
            }
        };
        drop(claim);
        self.store().task_set_setup(task.id, SetupStatus::Ok)?;
        Ok(Task {
            setup: SetupStatus::Ok,
            ..task
        })
    }

    /// Removes a chat as [`Core::task_remove`] removes a task: its agents are stopped, what
    /// still runs in its folder is ended, and the folder goes. Without `force` a folder that
    /// holds anything is kept, since what an agent wrote there exists nowhere else.
    pub(crate) async fn chat_remove(
        &self,
        home: &Project,
        chat: &Task,
        force: bool,
        sink: OutputSink<'_>,
    ) -> Result<Vec<String>> {
        self.none_run_elsewhere(&self.store().session_ids(chat.id)?)?;
        self.sessions_ended(chat.id).await?;
        let folder = &chat.worktree_path;
        let exists = folder.exists();
        if exists && !force && holds_anything(folder)? {
            return Err(Error::TaskNotClean {
                task: chat.name.clone(),
                reason: "files in its folder".to_string(),
            });
        }
        let mut notes = vec![];
        if exists {
            notes.extend(leftovers::end_in(folder, sink).await);
            match move_aside(folder, chat.id) {
                Some(aside) => delete_later(&aside),
                None => tokio::fs::remove_dir_all(folder).await?,
            }
        }
        self.store().task_archive(chat.id)?;
        self.forget_note(&home.name, &chat.name)?;
        self.sessions_forgotten(chat.id)?;
        Ok(notes)
    }
}

/// Whether there is anything in `folder`, hidden files included.
fn holds_anything(folder: &Path) -> Result<bool> {
    Ok(std::fs::read_dir(folder)?.next().is_some())
}
