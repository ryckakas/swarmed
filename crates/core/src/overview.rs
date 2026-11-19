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
}

impl Core {
    /// Every project with its tasks and their sessions, archived tasks included.
    pub fn overview(&self) -> Result<Vec<ProjectOverview>> {
        // Three queries under one lock, so the tree is consistent.
        let (projects, tasks, sessions) = {
            let store = self.store();
            (store.projects()?, store.tasks_all()?, store.sessions_all()?)
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
                task,
                sessions,
                running,
            }
        };
        let mut tasks: Vec<TaskOverview> = tasks.into_iter().map(task_overview).collect();
        let overview = projects.into_iter().map(|project| {
            let (own, rest) = tasks
                .drain(..)
                .partition(|t| t.task.project_id == project.id);
            tasks = rest;
            ProjectOverview {
                project,
                tasks: own,
            }
        });
        Ok(overview.collect())
    }
}
