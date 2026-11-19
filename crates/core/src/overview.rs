//! Everything a sidebar shows, in one read.

use serde::Serialize;

use crate::Core;
use crate::error::Result;
use crate::sessions::Session;
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
    /// Where the task's running stack is reached in a browser, if its project says so.
    pub url: Option<String>,
    /// Whether the user keeps a note on the task.
    pub has_note: bool,
    /// How many closed sessions the task has, which can be listed and reopened.
    pub closed: usize,
}

impl Core {
    /// Every project with its tasks and their sessions, archived tasks included.
    pub fn overview(&self) -> Result<Vec<ProjectOverview>> {
        // Three queries under one lock, so the tree is consistent.
        let (projects, tasks, sessions, closed) = {
            let store = self.store();
            (
                store.projects()?,
                store.tasks_all()?,
                store.sessions_all()?,
                store.closed_counts()?,
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
                has_note: false,
                closed: closed.get(&task.id).copied().unwrap_or(0),
                task,
            }
        };
        let mut tasks: Vec<TaskOverview> = tasks.into_iter().map(task_overview).collect();
        let overview = projects.into_iter().map(|project| {
            let (mut own, rest): (Vec<_>, Vec<_>) = tasks
                .drain(..)
                .partition(|t| t.task.project_id == project.id);
            tasks = rest;
            self.add_urls(&project, &mut own);
            self.add_notes(&project, &mut own);
            ProjectOverview {
                has_note: self.has_note(&project.name, None),
                project,
                tasks: own,
            }
        });
        Ok(overview.collect())
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
            let environment = task.task.environment.as_deref();
            let config = self.config_in(&project.name, environment);
            let url = config.and_then(|config| self.url_of(project, &config, &task.task));
            task.url = url.ok().flatten();
        }
    }
}
