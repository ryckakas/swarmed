//! Persistence in a local SQLite file.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

use crate::error::Result;

/// A registered git repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Project {
    /// The database id.
    pub id: i64,
    /// The unique name the project is addressed by.
    pub name: String,
    /// The repository's top-level directory.
    pub repo_path: PathBuf,
}

/// Whether a task's `setup` hook has run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    /// The hook has not finished yet.
    Pending,
    /// The hook succeeded, or the project has none.
    Ok,
    /// The hook failed; the worktree exists and setup can be retried.
    Failed,
}

impl SetupStatus {
    /// The value stored in the database.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ok => "ok",
            Self::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "ok" => Self::Ok,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// One unit of work: a branch, a worktree and a slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Task {
    /// The database id.
    pub id: i64,
    /// The project the task belongs to.
    pub project_id: i64,
    /// The name, unique among the project's active tasks.
    pub name: String,
    /// The branch checked out in the worktree.
    pub branch: String,
    /// The ref the branch was created from; used to detect commits that were never pushed.
    pub base_ref: String,
    /// The worktree directory, outside the repository.
    pub worktree_path: PathBuf,
    /// Unique among the project's active tasks; drives the task's ports.
    pub slot: u32,
    /// Whether the `setup` hook has run.
    pub setup: SetupStatus,
    /// Whether the task was removed. Archived tasks keep their record but have no worktree.
    pub archived: bool,
}

pub(crate) struct NewTask<'a> {
    pub(crate) project_id: i64,
    pub(crate) name: &'a str,
    pub(crate) branch: &'a str,
    pub(crate) base_ref: &'a str,
    pub(crate) worktree_path: &'a Path,
    pub(crate) slot: u32,
}

/// One entry per schema change, applied in order and never edited afterwards.
const MIGRATIONS: &[&str] = &[
    "
    CREATE TABLE projects (
        id        INTEGER PRIMARY KEY,
        name      TEXT NOT NULL UNIQUE,
        repo_path TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    CREATE TABLE tasks (
        id            INTEGER PRIMARY KEY,
        project_id    INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        name          TEXT NOT NULL,
        branch        TEXT NOT NULL,
        base_ref      TEXT NOT NULL,
        worktree_path TEXT NOT NULL,
        slot          INTEGER NOT NULL,
        setup_status  TEXT NOT NULL DEFAULT 'pending',
        archived_at   TEXT,
        created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    CREATE UNIQUE INDEX tasks_active_name ON tasks(project_id, name) WHERE archived_at IS NULL;
    CREATE UNIQUE INDEX tasks_active_slot ON tasks(project_id, slot) WHERE archived_at IS NULL;
",
    "
    CREATE TABLE sessions (
        id               INTEGER PRIMARY KEY,
        task_id          INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        title            TEXT NOT NULL DEFAULT '',
        agent_session_id TEXT,
        model            TEXT,
        supervision      TEXT NOT NULL,
        status           TEXT NOT NULL DEFAULT 'idle',
        created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    CREATE TABLE session_events (
        id         INTEGER PRIMARY KEY,
        session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        event      TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    CREATE INDEX session_events_by_session ON session_events(session_id, id);
",
    "
    ALTER TABLE sessions ADD COLUMN agent TEXT NOT NULL DEFAULT 'claude';
",
    "
    ALTER TABLE sessions ADD COLUMN closed_at TEXT;
",
    "
    ALTER TABLE sessions ADD COLUMN effort TEXT;
",
];

const TASK_SELECT: &str = "SELECT id, project_id, name, branch, base_ref, worktree_path, slot, \
                           setup_status, archived_at IS NOT NULL FROM tasks";

#[derive(Debug)]
pub(crate) struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub(crate) fn project_insert(&self, name: &str, repo_path: &Path) -> Result<Project> {
        self.conn.execute(
            "INSERT INTO projects (name, repo_path) VALUES (?1, ?2)",
            params![name, repo_path.to_string_lossy()],
        )?;
        Ok(Project {
            id: self.conn.last_insert_rowid(),
            name: name.to_string(),
            repo_path: repo_path.to_path_buf(),
        })
    }

    /// Deletes a project with its tasks, their sessions and those sessions' events.
    pub(crate) fn project_delete(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub(crate) fn project_by_name(&self, name: &str) -> Result<Option<Project>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, repo_path FROM projects WHERE name = ?1",
                params![name],
                project_from_row,
            )
            .optional()?)
    }

    pub(crate) fn project_by_id(&self, id: i64) -> Result<Option<Project>> {
        let sql = "SELECT id, name, repo_path FROM projects WHERE id = ?1";
        Ok(self
            .conn
            .query_row(sql, params![id], project_from_row)
            .optional()?)
    }

    pub(crate) fn projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, repo_path FROM projects ORDER BY name")?;
        let rows = stmt.query_map([], project_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn task_insert(&self, new: &NewTask<'_>) -> Result<Task> {
        self.conn.execute(
            "INSERT INTO tasks (project_id, name, branch, base_ref, worktree_path, slot)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                new.project_id,
                new.name,
                new.branch,
                new.base_ref,
                new.worktree_path.to_string_lossy(),
                new.slot
            ],
        )?;
        Ok(Task {
            id: self.conn.last_insert_rowid(),
            project_id: new.project_id,
            name: new.name.to_string(),
            branch: new.branch.to_string(),
            base_ref: new.base_ref.to_string(),
            worktree_path: new.worktree_path.to_path_buf(),
            slot: new.slot,
            setup: SetupStatus::Pending,
            archived: false,
        })
    }

    pub(crate) fn task_by_id(&self, task_id: i64) -> Result<Option<Task>> {
        let sql = format!("{TASK_SELECT} WHERE id = ?1");
        Ok(self
            .conn
            .query_row(&sql, params![task_id], task_from_row)
            .optional()?)
    }

    /// The active task with this name, if any.
    pub(crate) fn task_by_name(&self, project_id: i64, name: &str) -> Result<Option<Task>> {
        let sql =
            format!("{TASK_SELECT} WHERE project_id = ?1 AND name = ?2 AND archived_at IS NULL");
        Ok(self
            .conn
            .query_row(&sql, params![project_id, name], task_from_row)
            .optional()?)
    }

    pub(crate) fn tasks(&self, project_id: i64, include_archived: bool) -> Result<Vec<Task>> {
        let filter = if include_archived {
            ""
        } else {
            "AND archived_at IS NULL"
        };
        let mut stmt = self.conn.prepare(&format!(
            "{TASK_SELECT} WHERE project_id = ?1 {filter} \
             ORDER BY archived_at IS NOT NULL, created_at, id"
        ))?;
        let rows = stmt.query_map(params![project_id], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The lowest slot, starting at 1, that no active task of the project uses.
    pub(crate) fn free_slot(&self, project_id: i64) -> Result<u32> {
        let mut stmt = self.conn.prepare(
            "SELECT slot FROM tasks WHERE project_id = ?1 AND archived_at IS NULL ORDER BY slot",
        )?;
        let used = stmt
            .query_map(params![project_id], |row| row.get::<_, u32>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let taken = used
            .iter()
            .zip(1..)
            .take_while(|(slot, expected)| *slot == expected);
        Ok(taken.last().map_or(1, |(_, expected)| expected + 1))
    }

    pub(crate) fn tasks_all(&self) -> Result<Vec<Task>> {
        let sql = format!("{TASK_SELECT} ORDER BY archived_at IS NOT NULL, created_at, id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn task_set_setup(&self, task_id: i64, status: SetupStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET setup_status = ?2 WHERE id = ?1",
            params![task_id, status.as_str()],
        )?;
        Ok(())
    }

    pub(crate) fn task_archive(&self, task_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET archived_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![task_id],
        )?;
        Ok(())
    }
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        repo_path: PathBuf::from(row.get::<_, String>(2)?),
    })
}

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        branch: row.get(3)?,
        base_ref: row.get(4)?,
        worktree_path: PathBuf::from(row.get::<_, String>(5)?),
        slot: row.get(6)?,
        setup: SetupStatus::parse(&row.get::<_, String>(7)?),
        archived: row.get(8)?,
    })
}

/// Applies the migrations that have not run yet. `user_version` counts applied migrations.
fn migrate(conn: &mut Connection) -> Result<()> {
    let applied: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (version, sql) in (1_i64..).zip(MIGRATIONS) {
        if version <= applied {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}
