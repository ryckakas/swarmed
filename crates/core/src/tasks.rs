//! Task lifecycle: create, set up, remove.

use serde::Serialize;

use crate::config::ProjectConfig;
use crate::error::{Error, Result};
use crate::hooks::OutputSink;
use crate::store::{NewTask, Project, SetupStatus, Task};
use crate::vars::Vars;
use crate::{Core, git, hooks, paths};
use crate::{Settings, open};

/// Options for [`Core::task_new`].
#[derive(Debug, Default, Clone, Copy)]
pub struct NewTaskOptions<'a> {
    /// Check out this existing branch instead of creating one from the base.
    pub existing_branch: Option<&'a str>,
}

/// What a new task in a project can start from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BranchChoices {
    /// The ref a new branch would be created from.
    pub base: String,
    /// Existing branches a task could check out instead, sorted.
    pub existing: Vec<String>,
}

/// Options for [`Core::task_remove`].
#[derive(Debug, Default, Clone, Copy)]
pub struct RemoveOptions {
    /// Remove even with uncommitted changes, unpushed commits or a failing `teardown` hook.
    pub force: bool,
    /// Also delete the task's local branch.
    pub delete_branch: bool,
}

impl Core {
    /// What a new task in `project` can start from. Reads local refs only; it does not fetch.
    pub async fn branch_choices(&self, project: &str) -> Result<BranchChoices> {
        let project = self.project(project)?;
        let config = self.project_config(&project.name)?;
        let base = if config.base_branch.is_empty() {
            git::default_base(&project.repo_path).await?
        } else {
            config.base_branch
        };
        let existing = git::available_branches(&project.repo_path).await?;
        Ok(BranchChoices { base, existing })
    }

    /// The project's tasks, oldest first, archived ones last when included.
    pub fn tasks(&self, project: &str, include_archived: bool) -> Result<Vec<Task>> {
        let project = self.project(project)?;
        self.store().tasks(project.id, include_archived)
    }

    /// The active task called `name`, with its project.
    pub fn task(&self, project: &str, name: &str) -> Result<(Project, Task)> {
        let project = self.project(project)?;
        let task = self.store().task_by_name(project.id, name)?;
        let task = task.ok_or_else(|| Error::TaskNotFound {
            project: project.name.clone(),
            task: name.to_string(),
        })?;
        Ok((project, task))
    }

    /// Creates a task: a branch, a worktree outside the repo, and a slot, then runs `setup`.
    ///
    /// If `setup` fails the task is kept with [`SetupStatus::Failed`] and the hook's error is
    /// returned, so the worktree can be inspected and `setup` retried with [`Core::task_setup`].
    pub async fn task_new(
        &self,
        project: &str,
        name: &str,
        options: NewTaskOptions<'_>,
        sink: OutputSink<'_>,
    ) -> Result<Task> {
        paths::validate_name(name)?;
        let project = self.project(project)?;
        if self.store().task_by_name(project.id, name)?.is_some() {
            return Err(Error::TaskExists {
                project: project.name,
                task: name.to_string(),
            });
        }
        let config = self.project_config(&project.name)?;
        let repo = &project.repo_path;

        git::fetch(repo).await?;
        // A brand new repository has nothing to branch from until it has a commit.
        git::ensure_first_commit(repo).await?;
        let base = if config.base_branch.is_empty() {
            git::default_base(repo).await?
        } else {
            config.base_branch.clone()
        };
        let worktree = self.paths.worktree(&project.name, name);
        if let Some(parent) = worktree.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let branch = if let Some(branch) = options.existing_branch {
            git::worktree_add_existing(repo, &worktree, branch).await?;
            branch.to_string()
        } else {
            let branch = branch_vars(&project, name).render(&config.branch_pattern)?;
            git::worktree_add_new(repo, &worktree, &branch, &base).await?;
            branch
        };

        let new = NewTask {
            project_id: project.id,
            name,
            branch: &branch,
            base_ref: &base,
            worktree_path: &worktree,
            slot: 0,
        };
        let task = match self.insert_in_free_slot(new) {
            Ok(task) => task,
            Err(e) => {
                // Do not leave a worktree behind that Swarmed has no record of.
                let created = options.existing_branch.is_none().then_some(branch.as_str());
                discard_worktree(&project, &worktree, created).await;
                return Err(e);
            }
        };
        self.run_setup(&project, &config, task, sink).await
    }

    /// Takes the slot and writes the row under one lock, so two concurrent creations cannot
    /// get the same slot.
    fn insert_in_free_slot(&self, mut new: NewTask<'_>) -> Result<Task> {
        let store = self.store();
        new.slot = store.free_slot(new.project_id)?;
        store.task_insert(&new)
    }

    /// Runs the `setup` hook again, for a task whose setup failed or whose hook changed.
    pub async fn task_setup(
        &self,
        project: &str,
        name: &str,
        sink: OutputSink<'_>,
    ) -> Result<Task> {
        let (project, task) = self.task(project, name)?;
        let config = self.project_config(&project.name)?;
        self.run_setup(&project, &config, task, sink).await
    }

    async fn run_setup(
        &self,
        project: &Project,
        config: &ProjectConfig,
        mut task: Task,
        sink: OutputSink<'_>,
    ) -> Result<Task> {
        let result = self
            .run_hook("setup", &config.hooks.setup, project, config, &task, sink)
            .await;
        task.setup = if result.is_ok() {
            SetupStatus::Ok
        } else {
            SetupStatus::Failed
        };
        self.store().task_set_setup(task.id, task.setup)?;
        result.map(|()| task)
    }

    /// Removes a task: runs `teardown`, removes the worktree, archives the record.
    ///
    /// Without `force` this refuses when the worktree has uncommitted changes or commits that
    /// were never pushed, and when `teardown` fails.
    pub async fn task_remove(
        &self,
        project: &str,
        name: &str,
        options: RemoveOptions,
        sink: OutputSink<'_>,
    ) -> Result<()> {
        let (project, task) = self.task(project, name)?;
        let repo = &project.repo_path;
        if task.worktree_path.exists() {
            self.remove_worktree(&project, &task, options.force, sink)
                .await?;
        } else {
            // The directory was deleted by hand; make git forget it too.
            git::git(repo, &["worktree", "prune"]).await.ok();
        }
        self.store().task_archive(task.id)?;
        self.forget_note(&project.name, &task.name)?;
        if options.delete_branch && git::local_branch_exists(repo, &task.branch).await {
            git::branch_delete(repo, &task.branch).await?;
        }
        Ok(())
    }

    /// Checks for unsaved work, runs `teardown`, then removes the worktree. `force` skips the
    /// check and carries on past a failing `teardown`.
    async fn remove_worktree(
        &self,
        project: &Project,
        task: &Task,
        force: bool,
        sink: OutputSink<'_>,
    ) -> Result<()> {
        if !force {
            ensure_clean(task).await?;
        }
        let config = self.project_config(&project.name)?;
        let teardown = &config.hooks.teardown;
        match self
            .run_hook("teardown", teardown, project, &config, task, sink)
            .await
        {
            Err(e) if force => sink(&format!("teardown failed, removing anyway: {e}")),
            other => other?,
        }
        git::worktree_remove(&project.repo_path, &task.worktree_path).await
    }

    /// Where the task's running stack is reached in a browser, if its project says so.
    pub fn task_url(&self, project: &str, task: &str) -> Result<Option<String>> {
        let (project, task) = self.task(project, task)?;
        let config = self.project_config(&project.name)?;
        self.url_of(&project, &config, &task)
    }

    pub(crate) fn url_of(
        &self,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
    ) -> Result<Option<String>> {
        if config.open_url.trim().is_empty() {
            return Ok(None);
        }
        let vars = self.task_vars(project, config, task)?;
        Ok(Some(vars.render(config.open_url.trim())?))
    }

    /// Opens the task's running stack in the default browser.
    pub fn task_open_url(&self, project: &str, task: &str) -> Result<()> {
        let url = self.task_url(project, task)?;
        let url = url.ok_or_else(|| Error::NoOpenUrl(project.to_string()))?;
        open::in_browser(&url)
    }

    /// Opens the task's worktree, with its branch checked out, in the user's editor.
    pub fn task_open_editor(&self, project: &str, task: &str) -> Result<()> {
        let (_, task) = self.task(project, task)?;
        open::in_editor(&self.settings()?.editor, &task.worktree_path)
    }

    /// The user's own preferences.
    pub fn settings(&self) -> Result<Settings> {
        Settings::load(&self.paths.settings())
    }

    /// Replaces the user's preferences.
    pub fn set_settings(&self, settings: &Settings) -> Result<()> {
        settings.save(&self.paths.settings())
    }

    /// The variables a task's hooks and environment values can use.
    fn task_vars(&self, project: &Project, config: &ProjectConfig, task: &Task) -> Result<Vars> {
        let mut vars = branch_vars(project, &task.name);
        vars.set("branch", task.branch.as_str());
        vars.set("slot", task.slot.to_string());
        vars.set("worktree", task.worktree_path.to_string_lossy());
        vars.set("repo", project.repo_path.to_string_lossy());
        for (name, base) in &config.ports {
            let port = config
                .port_for(*base, task.slot)
                .ok_or_else(|| Error::Config {
                    path: self.paths.project_config(&project.name),
                    message: format!("port `{name}` overflows for slot {}", task.slot),
                })?;
            vars.set(format!("port.{name}"), port.to_string());
        }
        Ok(vars)
    }

    /// Renders and runs one hook in the task's worktree. Every variable is also exported as
    /// `SWARMED_<NAME>`, so a hook can be a script that never sees the template syntax.
    async fn run_hook(
        &self,
        hook: &'static str,
        template: &str,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
        sink: OutputSink<'_>,
    ) -> Result<()> {
        if template.trim().is_empty() {
            return Ok(());
        }
        let vars = self.task_vars(project, config, task)?;
        let command = vars.render(template)?;
        let mut env: Vec<(String, String)> = vars
            .iter()
            .map(|(name, value)| {
                (
                    format!("SWARMED_{}", name.replace('.', "_").to_uppercase()),
                    value.to_string(),
                )
            })
            .collect();
        for (key, value) in &config.env {
            env.push((key.clone(), vars.render(value)?));
        }
        hooks::run(hook, &command, &task.worktree_path, &env, sink).await
    }
}

/// The variables known before the task exists, which is all a branch pattern may use.
fn branch_vars(project: &Project, task: &str) -> Vars {
    let mut vars = Vars::default();
    vars.set("user", user_name());
    vars.set("task", task);
    vars.set("project", project.name.as_str());
    vars
}

/// Best-effort removal of a worktree, and the branch created for it, after a failed creation.
async fn discard_worktree(project: &Project, worktree: &std::path::Path, branch: Option<&str>) {
    git::worktree_remove(&project.repo_path, worktree)
        .await
        .ok();
    if let Some(branch) = branch {
        git::branch_delete(&project.repo_path, branch).await.ok();
    }
}

/// Refuses when removing the worktree would destroy work that exists nowhere else.
async fn ensure_clean(task: &Task) -> Result<()> {
    let reason = if git::is_dirty(&task.worktree_path).await? {
        "uncommitted changes".to_string()
    } else {
        match git::unpushed_commits(&task.worktree_path, &task.base_ref).await? {
            0 => return Ok(()),
            count => format!("{count} unpushed commit(s)"),
        }
    };
    Err(Error::TaskNotClean {
        task: task.name.clone(),
        reason,
    })
}

fn user_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string())
}
