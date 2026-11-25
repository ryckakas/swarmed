//! Everything a sidebar shows, in one read.

use serde::Serialize;

use crate::Core;
use crate::cloud::CloudTask;
use crate::error::Result;
use crate::pulls::Pull;
use crate::sessions::Session;
use crate::setups::SettingUp;
use crate::stacks::StackState;
use crate::store::{Project, Task};

/// A project with its tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectOverview {
    /// The project itself.
    #[serde(flatten)]
    pub project: Project,
    /// Its tasks, oldest first, archived ones last.
    pub tasks: Vec<TaskOverview>,
    /// Whether the user keeps a note on the project.
    pub has_note: bool,
    /// The tasks handed to cloud agents from the project, newest first, as last heard of.
    pub cloud: Vec<CloudTask>,
}

/// A task with its sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskOverview {
    /// The task itself.
    #[serde(flatten)]
    pub task: Task,
    /// Its sessions, oldest first.
    pub sessions: Vec<Session>,
    /// The ids of those sessions whose agent is running.
    pub running: Vec<i64>,
    /// Where the task's stack is opened in a browser by default: what its project says, or
    /// the first address the running stack printed.
    pub url: Option<String>,
    /// Every address the stack can be opened at, `url` first.
    pub urls: Vec<String>,
    /// Whether the user keeps a note on the task.
    pub has_note: bool,
    /// How many closed sessions the task has, which can be listed and reopened.
    pub closed: usize,
    /// How far along the task's stack is; `None` if its project has no `start` command.
    pub stack: Option<StackState>,
    /// How its setup stands while it is not over: set up here, by another Swarmed, or cut
    /// short. `None` once it is over, well or not.
    pub setting_up: Option<SettingUp>,
    /// The pull request of the branch its worktree is on, as Swarmed last read it, if it has
    /// one (`following.rs`).
    pub pull: Option<Pull>,
    /// Whether the task's browser runs (`browser.rs`), here or in another Swarmed.
    pub browser: bool,
}

impl Core {
    /// Every project with its tasks and their sessions, archived tasks included.
    pub fn overview(&self) -> Result<Vec<ProjectOverview>> {
        // The queries under one lock, so the tree is consistent.
        let (projects, tasks, sessions, closed, mut pulls) = {
            let store = self.store();
            (
                store.projects()?,
                store.tasks_all()?,
                store.sessions_all()?,
                store.closed_counts()?,
                store.pulls_kept()?,
            )
        };
        let running = self.running_sessions();
        let task_overview = |task: Task| {
            let sessions: Vec<Session> = sessions
                .iter()
                .filter(|session| session.task_id == task.id)
                .cloned()
                .collect();
            let running = sessions
                .iter()
                .map(|s| s.id)
                .filter(|id| running.contains(id))
                .collect();
            TaskOverview {
                sessions,
                running,
                url: None,
                urls: Vec::new(),
                has_note: false,
                closed: closed.get(&task.id).copied().unwrap_or(0),
                stack: None,
                setting_up: self.setting_up(&task),
                pull: pulls.remove(&task.id),
                browser: !task.archived && self.browser_running(task.id),
                task,
            }
        };
        let mut tasks: Vec<TaskOverview> = tasks.into_iter().map(task_overview).collect();
        let mut cloud = self.cloud_tasks_of(None)?;
        let overview = projects.into_iter().map(|project| {
            let (mut own, rest): (Vec<_>, Vec<_>) = tasks
                .drain(..)
                .partition(|t| t.task.project_id == project.id);
            tasks = rest;
            self.add_urls(&project, &mut own);
            self.add_notes(&project, &mut own);
            let (delegated, others): (Vec<_>, Vec<_>) = cloud
                .drain(..)
                .partition(|task| task.project_id == project.id);
            cloud = others;
            ProjectOverview {
                has_note: self.has_note(&project.name, None),
                project,
                tasks: own,
                cloud: delegated,
            }
        });
        Ok(overview.collect())
    }

    /// Where one task's stack is reached, and whether it is running.
    fn add_url(&self, project: &Project, task: &mut TaskOverview) {
        let environment = task.task.environment.as_deref();
        let Ok(config) = self.config_in(&project.name, environment) else {
            return;
        };
        let reach = self.reach(project, &config, &task.task);
        task.url = reach.urls.first().cloned();
        task.urls = reach.urls;
        task.stack = reach.state;
    }

    /// Marks the tasks the user keeps a note on.
    fn add_notes(&self, project: &Project, tasks: &mut [TaskOverview]) {
        for task in tasks {
            task.has_note = self.has_note(&project.name, Some(&task.task.name));
        }
    }

    /// Fills in where each task's stack is reached. A config that cannot be read, or a URL that
    /// cannot be rendered, leaves a task without one instead of failing the whole overview.
    fn add_urls(&self, project: &Project, tasks: &mut [TaskOverview]) {
        for task in tasks.iter_mut().filter(|t| !t.task.archived) {
            self.add_url(project, task);
        }
    }
}
