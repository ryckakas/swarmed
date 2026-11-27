//! Persistence in a local SQLite file.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, ToSql, Transaction, params};
use serde::Serialize;

use crate::chats::CHATS;
use crate::elsewhere::Written;
use crate::error::{Error, Result};
use crate::marks::MarkColour;
use crate::put_away::DoneWhy;

/// A registered git repository, or the home of the chats, which has none ([`CHATS`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Project {
    /// The database id.
    pub id: i64,
    /// The unique name the project is addressed by.
    pub name: String,
    /// The repository's top-level directory. For the chats, the folder their own folders are
    /// in, which is no repository: [`Project::repo`] says so.
    pub repo_path: PathBuf,
    /// Whether this is the home of the chats: sessions with no repository, each chat a task in
    /// a folder of its own.
    pub chats: bool,
    /// Whether the user pinned it above the repositories that are not pinned.
    pub pinned: bool,
}

impl Project {
    /// The repository's top-level directory, or [`Error::NoRepository`] for the chats.
    pub fn repo(&self) -> Result<&Path> {
        if self.chats {
            return Err(Error::NoRepository);
        }
        Ok(&self.repo_path)
    }
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
    /// The branch the task began on, checked out in the worktree when it was made. It never
    /// changes. The task panel's heading, the status line and removal's branch deletion mean
    /// it; what is shown and published follows the branch the worktree is on now (`publish.rs`).
    pub branch: String,
    /// The ref the branch was created from; used to detect commits that were never pushed.
    pub base_ref: String,
    /// The worktree directory, outside the repository.
    pub worktree_path: PathBuf,
    /// Unique among the project's active tasks; drives the task's ports.
    pub slot: u32,
    /// Whether the `setup` hook has run.
    pub setup: SetupStatus,
    /// Why the task's last setup did not finish well, in words: what it failed of, or what
    /// stopped it. `None` once one finished well, and before any ended.
    pub setup_reason: Option<String>,
    /// Whether the task was removed. Archived tasks keep their record but have no worktree.
    pub archived: bool,
    /// The project's environment the task was created in, if it was created in one.
    pub environment: Option<String>,
    /// Whether the user pinned it above its repository's tasks that are not pinned.
    pub pinned: bool,
    /// When it was put away, in seconds since 1970; `None` while it is not (`put_away.rs`).
    pub done_at: Option<i64>,
    /// Why it was put away, while it is.
    pub done_why: Option<DoneWhy>,
    /// The number of the repository's GitHub issue the task was started for, if it was
    /// (`issues.rs`).
    pub issue: Option<u64>,
    /// The colour the user highlighted it in, if any (`highlights.rs`).
    pub highlight: Option<MarkColour>,
}

pub(crate) struct NewTask<'a> {
    pub(crate) project_id: i64,
    pub(crate) name: &'a str,
    pub(crate) branch: &'a str,
    pub(crate) base_ref: &'a str,
    pub(crate) worktree_path: &'a Path,
    pub(crate) slot: u32,
    pub(crate) environment: Option<&'a str>,
    /// Whether the branch was made for the task, rather than checked out as it was.
    pub(crate) branch_made: bool,
    /// The repository's issue the task is for, if any.
    pub(crate) issue: Option<u64>,
}

/// A removed task's branch, as the task left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemovedBranch {
    /// The removed task's name.
    pub(crate) task: String,
    pub(crate) branch: String,
    pub(crate) base_ref: String,
    /// Whether the branch was made for the task; `None` for a task made before that was kept.
    pub(crate) made: Option<bool>,
    /// When the task was removed, in seconds since 1970.
    pub(crate) removed_at: Option<u64>,
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
    "
    ALTER TABLE sessions ADD COLUMN unread INTEGER NOT NULL DEFAULT 0;
",
    "
    ALTER TABLE sessions ADD COLUMN spawned_by INTEGER;
",
    "
    ALTER TABLE tasks ADD COLUMN environment TEXT;
",
    "
    CREATE TABLE cloud_tasks (
        id         INTEGER PRIMARY KEY,
        project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        provider   TEXT NOT NULL,
        remote_id  TEXT NOT NULL,
        url        TEXT NOT NULL,
        prompt     TEXT NOT NULL,
        branch     TEXT NOT NULL,
        status     TEXT NOT NULL,
        session_id INTEGER,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
",
    "
    ALTER TABLE sessions ADD COLUMN fast INTEGER NOT NULL DEFAULT 0;
",
    "
    ALTER TABLE sessions ADD COLUMN mark_colour TEXT;
    ALTER TABLE sessions ADD COLUMN mark_label TEXT NOT NULL DEFAULT '';
",
    // No id is given twice. Without AUTOINCREMENT, SQLite gives a deleted last row's id to the
    // next row, and what is kept by that id, such as a window's copy of a deleted session's
    // conversation, turns up in the new one. A table made from now on is declared with it.
    AUTOINCREMENT_IDS,
    // The branches a task's worktree has been seen on, besides the task's own.
    "
    CREATE TABLE task_branches (
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        name    TEXT NOT NULL,
        UNIQUE (task_id, name)
    );
",
    // The order the user puts things in, among their siblings, starting from the order they
    // were listed in before: repositories by name, tasks and sessions oldest first, cloud
    // tasks newest first.
    "
    ALTER TABLE projects ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE tasks ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE sessions ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE cloud_tasks ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
    UPDATE projects SET position = (
        SELECT n FROM (SELECT id, ROW_NUMBER() OVER (ORDER BY name) AS n FROM projects) AS o
        WHERE o.id = projects.id);
    UPDATE tasks SET position = (
        SELECT n FROM (SELECT id, ROW_NUMBER() OVER (
            PARTITION BY project_id ORDER BY created_at, id) AS n FROM tasks) AS o
        WHERE o.id = tasks.id);
    UPDATE sessions SET position = (
        SELECT n FROM (SELECT id, ROW_NUMBER() OVER (
            PARTITION BY task_id ORDER BY id) AS n FROM sessions) AS o
        WHERE o.id = sessions.id);
    UPDATE cloud_tasks SET position = (
        SELECT n FROM (SELECT id, ROW_NUMBER() OVER (
            PARTITION BY project_id ORDER BY id DESC) AS n FROM cloud_tasks) AS o
        WHERE o.id = cloud_tasks.id);
",
    // The environment a new task of the project is set up in when none is chosen. Here, not
    // in the project's files, where a Swarmed that does not know the key would refuse it or
    // name it as a problem.
    "
    ALTER TABLE projects ADD COLUMN default_environment TEXT;
",
    // Whether a task's branch was made for it, rather than one that was there and was
    // checked out: a clean-up deletes only branches Swarmed made. Not known for tasks made
    // before this.
    "
    ALTER TABLE tasks ADD COLUMN branch_made INTEGER;
",
    // Why a task's last setup did not finish well, in words; and the message a session was
    // made with that waits for its task's setup. Kept, so that quitting loses neither.
    "
    ALTER TABLE tasks ADD COLUMN setup_reason TEXT;
    ALTER TABLE sessions ADD COLUMN setup_prompt TEXT;
",
    // Rows the user pinned, which stay above their siblings that are not.
    "
    ALTER TABLE projects ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE tasks ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
",
    // When a session goes on by itself, once the usage limit that stopped it resets, in
    // seconds since 1970. Kept, since a reset can be days away.
    "
    ALTER TABLE sessions ADD COLUMN continue_at INTEGER;
",
    // The pull request of a task's branch as Swarmed last read it, as JSON: what the window
    // shows, and what the next reading is compared with to tell what changed. Kept, so that
    // a change while no Swarmed ran is told once, at the next start (`following.rs`).
    "
    ALTER TABLE tasks ADD COLUMN pull TEXT;
",
    // The method a pull request of the project was last merged with from Swarmed, which the
    // next merge starts on. Here, not in the project's files, for the reason the default
    // environment is (`merging.rs`).
    "
    ALTER TABLE projects ADD COLUMN merge_method TEXT;
",
    // When a task was put away, in seconds since 1970, and why; and when it was last brought
    // back, which the rules that put tasks away by themselves count from (`put_away.rs`).
    "
    ALTER TABLE tasks ADD COLUMN done_at INTEGER;
    ALTER TABLE tasks ADD COLUMN done_why TEXT;
    ALTER TABLE tasks ADD COLUMN back_at INTEGER;
",
    // The number of the repository's GitHub issue a task was started for (`issues.rs`).
    "
    ALTER TABLE tasks ADD COLUMN issue INTEGER;
",
    // The worktree as a git tree before each message went to the agent and when its turn
    // ended, kept in a store of the task's own (`checkpoints.rs`). The message is its number
    // in the session, counted as a fork counts them; `skipped` is JSON of the files left out.
    "
    CREATE TABLE checkpoints (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        message    INTEGER NOT NULL,
        side       TEXT NOT NULL,
        tree       TEXT NOT NULL,
        skipped    TEXT NOT NULL DEFAULT '[]',
        words      TEXT NOT NULL DEFAULT '',
        took_ms    INTEGER NOT NULL DEFAULT 0,
        at_ms      INTEGER NOT NULL,
        UNIQUE (session_id, message, side)
    );
",
    // The commit a checkpoint's worktree was on, so that taking files back can say which
    // commits stay on the branch; and the worktree as it stood before a session's files were
    // taken back to a checkpoint, kept so that they can be put back (`rewind.rs`). `restored`
    // is the tree they were taken back to, which the worktree must still be at for that.
    "
    ALTER TABLE checkpoints ADD COLUMN head TEXT;
    CREATE TABLE rewinds (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        message    INTEGER NOT NULL,
        was        TEXT NOT NULL,
        restored   TEXT NOT NULL,
        at_ms      INTEGER NOT NULL
    );
",
    // What each session's agent has running in the background now, sub-agents and commands
    // apart, as its stored reports and ends add up (`background.rs`): a flag beside the status,
    // which the sidebar shows. Worked out again whenever such a report or an end is stored.
    "
    ALTER TABLE sessions ADD COLUMN background_agents INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE sessions ADD COLUMN background_commands INTEGER NOT NULL DEFAULT 0;
",
    // The colour the user highlighted a task or a session in, by its name, so that it stands
    // out where it is (`highlights.rs`); none for none.
    "
    ALTER TABLE tasks ADD COLUMN highlight TEXT;
    ALTER TABLE sessions ADD COLUMN highlight TEXT;
",
    // `spawned_by` is whom a session works for now, which the user can set too (`parents.rs`);
    // whether another session's agent started it, which is all it said before, is kept apart.
    "
    ALTER TABLE sessions ADD COLUMN spawned INTEGER NOT NULL DEFAULT 0;
    UPDATE sessions SET spawned = 1 WHERE spawned_by IS NOT NULL;
",
    // What a session is for, by the name of its role (`roles.rs`); none for no role.
    "
    ALTER TABLE sessions ADD COLUMN role TEXT;
",
    // The supervision a session's role took the place of, which taking the role off puts
    // back (`roles.rs`); none while no role holds one, or once the user chose another.
    "
    ALTER TABLE sessions ADD COLUMN supervision_before_role TEXT;
",
    // Which session's agent started a session, however it did, apart from whom it works for,
    // which the user can change (`spawned_by`); none for one the user started. A session
    // started before this is known by its first message, as each way marked it, when that
    // names the session it works for: `start_task`'s, and `spawn_session`'s. Never a fork: it
    // holds a copy of that first message, mark and all, and the user made it.
    "
    ALTER TABLE sessions ADD COLUMN started_by INTEGER;
    WITH firsts AS (
        SELECT id, spawned_by AS parent, spawned, COALESCE(setup_prompt, (
            SELECT CASE WHEN json_valid(event) THEN json_extract(event, '$.text') END
            FROM session_events AS said
            WHERE said.session_id = sessions.id
              AND CASE WHEN json_valid(event)
                  THEN json_extract(event, '$.type') = 'user_message' END
            ORDER BY said.id LIMIT 1)) AS text
        FROM sessions WHERE spawned_by IS NOT NULL
          AND NOT EXISTS (
            SELECT 1 FROM session_events AS copied
            WHERE copied.session_id = sessions.id
              AND CASE WHEN json_valid(event)
                  THEN json_extract(event, '$.type') = 'forked' END)
    )
    UPDATE sessions SET started_by = spawned_by WHERE id IN (
        SELECT id FROM firsts
        WHERE text LIKE 'Message from session ' || parent || ' (%(Session ' || parent
                        || ' started this task for that work%'
           OR (spawned = 1 AND text LIKE '%(You were started by session ' || parent
                                         || ', another agent session of the same user%')
    );
",
];

/// Stands in [`MIGRATIONS`] for the one change SQL in a transaction cannot make: every
/// table's `INTEGER PRIMARY KEY` becomes `INTEGER PRIMARY KEY AUTOINCREMENT`
/// ([`rebuild_with_autoincrement`]).
const AUTOINCREMENT_IDS: &str = "-- every table's id becomes AUTOINCREMENT";

const PROJECT_SELECT: &str = "SELECT id, name, repo_path, pinned FROM projects";

/// The order the sidebar lists repositories in: the pinned ones first, then as the user put
/// them.
pub(crate) const PROJECT_ORDER: &str = "pinned DESC, position, name";

/// The order the sidebar lists a repository's tasks in: the pinned ones first, then as the
/// user put them.
pub(crate) const TASK_ORDER: &str = "pinned DESC, position, created_at, id";

const TASK_SELECT: &str = "SELECT id, project_id, name, branch, base_ref, worktree_path, slot, \
                           setup_status, archived_at IS NOT NULL, environment, setup_reason, \
                           pinned, done_at, done_why, issue, highlight FROM tasks";

#[derive(Debug)]
pub(crate) struct Store {
    pub(crate) conn: Connection,
    /// The events this process stored, for telling them from those another process stored.
    /// Only ever borrowed for one short step: the store is behind a lock already.
    pub(crate) written: RefCell<Written>,
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
        let written = RefCell::new(Written::from_now(&conn)?);
        Ok(Self { conn, written })
    }

    pub(crate) fn project_insert(&self, name: &str, repo_path: &Path) -> Result<Project> {
        self.conn.execute(
            // At the end of the list.
            "INSERT INTO projects (name, repo_path, position)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1 FROM projects))",
            params![name, repo_path.to_string_lossy()],
        )?;
        Ok(Project {
            id: self.conn.last_insert_rowid(),
            name: name.to_string(),
            repo_path: repo_path.to_path_buf(),
            chats: name == CHATS,
            pinned: false,
        })
    }

    /// Deletes a project with its tasks, their sessions and those sessions' events.
    pub(crate) fn project_delete(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub(crate) fn project_by_name(&self, name: &str) -> Result<Option<Project>> {
        let sql = format!("{PROJECT_SELECT} WHERE name = ?1");
        Ok(self
            .conn
            .query_row(&sql, params![name], project_from_row)
            .optional()?)
    }

    pub(crate) fn project_by_id(&self, id: i64) -> Result<Option<Project>> {
        let sql = format!("{PROJECT_SELECT} WHERE id = ?1");
        Ok(self
            .conn
            .query_row(&sql, params![id], project_from_row)
            .optional()?)
    }

    /// The name the project's default environment was last given, whether or not the project
    /// still has an environment of that name.
    pub(crate) fn project_default_environment(&self, id: i64) -> Result<Option<String>> {
        let sql = "SELECT default_environment FROM projects WHERE id = ?1";
        let found: Option<Option<String>> = self
            .conn
            .query_row(sql, params![id], |row| row.get(0))
            .optional()?;
        Ok(found.flatten())
    }

    pub(crate) fn project_set_default_environment(
        &self,
        id: i64,
        name: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET default_environment = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        Ok(())
    }

    pub(crate) fn projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{PROJECT_SELECT} ORDER BY {PROJECT_ORDER}"))?;
        let rows = stmt.query_map([], project_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn task_insert(&self, new: &NewTask<'_>) -> Result<Task> {
        self.conn.execute(
            // At the end of its repository's tasks.
            "INSERT INTO tasks (project_id, name, branch, base_ref, worktree_path, slot, environment,
                                branch_made, issue, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                     (SELECT COALESCE(MAX(position), 0) + 1 FROM tasks WHERE project_id = ?1))",
            params![
                new.project_id,
                new.name,
                new.branch,
                new.base_ref,
                new.worktree_path.to_string_lossy(),
                new.slot,
                new.environment,
                new.branch_made,
                new.issue.and_then(|number| i64::try_from(number).ok())
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
            setup_reason: None,
            archived: false,
            environment: new.environment.map(String::from),
            pinned: false,
            done_at: None,
            done_why: None,
            issue: new.issue,
            highlight: None,
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

    /// Remembers that a task's worktree was seen on `branch`. Once is enough.
    pub(crate) fn task_branch_seen(&self, task: i64, branch: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO task_branches (task_id, name) VALUES (?1, ?2)",
            params![task, branch],
        )?;
        Ok(())
    }

    /// The branches a task's worktree was seen on, in the order they were first seen.
    pub(crate) fn task_branches(&self, task: i64) -> Result<Vec<String>> {
        let sql = "SELECT name FROM task_branches WHERE task_id = ?1 ORDER BY id";
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params![task], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn tasks(&self, project_id: i64, include_archived: bool) -> Result<Vec<Task>> {
        let filter = if include_archived {
            ""
        } else {
            "AND archived_at IS NULL"
        };
        let mut stmt = self.conn.prepare(&format!(
            "{TASK_SELECT} WHERE project_id = ?1 {filter} \
             ORDER BY archived_at IS NOT NULL, {TASK_ORDER}"
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
        let sql = format!("{TASK_SELECT} ORDER BY archived_at IS NOT NULL, {TASK_ORDER}");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn task_set_setup(&self, task_id: i64, status: SetupStatus) -> Result<()> {
        self.task_set_setup_said(task_id, status, None)
    }

    /// Notes how the task's setup came out, and why, in words, if it did not finish well.
    pub(crate) fn task_set_setup_said(
        &self,
        task_id: i64,
        status: SetupStatus,
        reason: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET setup_status = ?2, setup_reason = ?3 WHERE id = ?1",
            params![task_id, status.as_str(), reason],
        )?;
        Ok(())
    }

    /// The branches of the project's removed tasks, the latest removed first.
    pub(crate) fn removed_branches(&self, project_id: i64) -> Result<Vec<RemovedBranch>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, branch, base_ref, branch_made, CAST(strftime('%s', archived_at) AS INTEGER)
             FROM tasks WHERE project_id = ?1 AND archived_at IS NOT NULL
             ORDER BY archived_at DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(RemovedBranch {
                task: row.get(0)?,
                branch: row.get(1)?,
                base_ref: row.get(2)?,
                made: row.get(3)?,
                removed_at: row
                    .get::<_, Option<i64>>(4)?
                    .and_then(|seconds| u64::try_from(seconds).ok()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
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
    let name: String = row.get(1)?;
    Ok(Project {
        id: row.get(0)?,
        chats: name == CHATS,
        name,
        repo_path: PathBuf::from(row.get::<_, String>(2)?),
        pinned: row.get(3)?,
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
        environment: row.get(9)?,
        setup_reason: row.get(10)?,
        pinned: row.get(11)?,
        done_at: row.get(12)?,
        done_why: row
            .get::<_, Option<String>>(13)?
            .as_deref()
            .map(DoneWhy::parse),
        issue: row
            .get::<_, Option<i64>>(14)?
            .and_then(|number| u64::try_from(number).ok()),
        highlight: row
            .get::<_, Option<String>>(15)?
            .as_deref()
            .and_then(MarkColour::parse),
    })
}

/// Applies the migrations that have not run yet. `user_version` counts applied migrations.
fn migrate(conn: &mut Connection) -> Result<()> {
    let applied: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (version, sql) in (1_i64..).zip(MIGRATIONS) {
        if version <= applied {
            continue;
        }
        if *sql == AUTOINCREMENT_IDS {
            rebuild_with_autoincrement(conn, version)?;
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

/// Makes every table's `INTEGER PRIMARY KEY` an `INTEGER PRIMARY KEY AUTOINCREMENT`, which
/// SQLite only takes when a table is created: each table is made again with it, under another
/// name, its rows are copied over, the old one is dropped, the new one takes its name, and its
/// indexes are made again. Copied with their ids, the rows set where AUTOINCREMENT goes on from.
///
/// Foreign keys are off meanwhile, as SQLite's own procedure for remaking a table has it:
/// dropping a table deletes its rows first, and with the keys on, every row that refers to
/// them as well. They can only be switched outside a transaction, so this one step runs apart
/// from the others, and checks the keys before it commits. `version` is the step's number.
fn rebuild_with_autoincrement(conn: &mut Connection, version: i64) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let rebuilt = remake_tables(conn, version);
    conn.pragma_update(None, "foreign_keys", "ON")?;
    rebuilt
}

fn remake_tables(conn: &mut Connection, version: i64) -> Result<()> {
    let tx = conn.transaction()?;
    let tables = schema(
        &tx,
        "SELECT name, sql FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
         AND sql LIKE '%INTEGER PRIMARY KEY%' AND sql NOT LIKE '%AUTOINCREMENT%'",
        &[],
    )?;
    for (table, sql) in &tables {
        remake(&tx, table, sql)?;
    }
    let broken: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_foreign_key_check()",
        [],
        |row| row.get(0),
    )?;
    if broken > 0 {
        // Dropped without a commit: nothing of it stays.
        return Err(Error::Migration(format!(
            "{broken} rows refer to rows that are not there"
        )));
    }
    tx.pragma_update(None, "user_version", version)?;
    tx.commit()?;
    Ok(())
}

/// Remakes `table`, created by `sql`, with an AUTOINCREMENT id, as [`remake_tables`] says.
fn remake(tx: &Transaction<'_>, table: &str, sql: &str) -> Result<()> {
    let indexes = schema(
        tx,
        "SELECT name, sql FROM sqlite_schema WHERE type IN ('index', 'trigger') \
         AND tbl_name = ?1 AND sql IS NOT NULL",
        &[&table],
    )?;
    let new = format!("{table}_autoincrement");
    let created = sql
        .replacen(
            &format!("CREATE TABLE {table} "),
            &format!("CREATE TABLE {new} "),
            1,
        )
        .replacen(
            "INTEGER PRIMARY KEY",
            "INTEGER PRIMARY KEY AUTOINCREMENT",
            1,
        );
    if !created.starts_with(&format!("CREATE TABLE {new} ")) {
        return Err(Error::Migration(format!(
            "{table} is not created as expected"
        )));
    }
    tx.execute_batch(&format!(
        "{created};
         INSERT INTO {new} SELECT * FROM {table};
         DROP TABLE {table};
         ALTER TABLE {new} RENAME TO {table};"
    ))?;
    for (_, index) in &indexes {
        tx.execute_batch(index)?;
    }
    Ok(())
}

/// The names and SQL of the schema's entries `query` selects.
fn schema(
    tx: &Transaction<'_>,
    query: &str,
    parameters: &[&dyn ToSql],
) -> Result<Vec<(String, String)>> {
    let mut statement = tx.prepare(query)?;
    let rows = statement.query_map(parameters, |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables that have an integer id, with whether it is AUTOINCREMENT.
    fn ids(conn: &Connection) -> Vec<(String, bool)> {
        let mut statement = conn
            .prepare(
                "SELECT name, sql LIKE '%AUTOINCREMENT%' FROM sqlite_schema \
                 WHERE type = 'table' AND sql LIKE '%INTEGER PRIMARY KEY%' ORDER BY name",
            )
            .expect("reads the schema");
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("reads the schema");
        rows.collect::<rusqlite::Result<_>>()
            .expect("reads the schema")
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |row| row.get(0)).expect("counts")
    }

    #[test]
    fn every_integer_id_is_autoincrement_in_a_new_store() {
        let mut conn = Connection::open_in_memory().expect("opens");
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("sets");
        migrate(&mut conn).expect("migrates");
        let tables = ids(&conn);
        // A table added later is held to it too.
        assert!(tables.len() >= 5, "{tables:?}");
        assert!(
            tables.iter().all(|(_, autoincrement)| *autoincrement),
            "{tables:?}"
        );
        assert_eq!(count(&conn, "PRAGMA foreign_keys"), 1);
    }

    /// The ids of a table's rows in the order its positions give, `WHERE` `filter`.
    fn in_order(conn: &Connection, table: &str, filter: &str) -> Vec<i64> {
        let sql = format!("SELECT id FROM {table} WHERE {filter} ORDER BY position");
        let mut statement = conn.prepare(&sql).expect("reads");
        let rows = statement.query_map([], |row| row.get(0)).expect("reads");
        rows.collect::<rusqlite::Result<_>>().expect("reads")
    }

    #[test]
    fn a_store_made_before_positions_keeps_the_order_it_listed_things_in() {
        let mut conn = Connection::open_in_memory().expect("opens");
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("sets");
        // Every step before the one that adds the positions.
        let adds = |sql: &&str| sql.contains("ADD COLUMN position");
        let before = MIGRATIONS.iter().position(adds).expect("there");
        let plain = MIGRATIONS[..before]
            .iter()
            .filter(|sql| **sql != AUTOINCREMENT_IDS);
        for sql in plain {
            conn.execute_batch(sql).expect("migrates");
        }
        let version = i64::try_from(before).expect("a few migrations");
        conn.pragma_update(None, "user_version", version)
            .expect("sets");
        conn.execute_batch(
            "INSERT INTO projects (id, name, repo_path) VALUES (1, 'zeta', '/z'), (2, 'alpha', '/a');
             INSERT INTO tasks (id, project_id, name, branch, base_ref, worktree_path, slot,
                                created_at)
                 VALUES (3, 1, 'late', 'b1', 'main', '/w1', 1, '2026-02-01'),
                        (4, 1, 'early', 'b2', 'main', '/w2', 2, '2026-01-01'),
                        (5, 2, 'other', 'b3', 'main', '/w3', 1, '2026-03-01');
             INSERT INTO sessions (id, task_id, supervision) VALUES (9, 4, 'ask'), (7, 4, 'ask'),
                                                                    (8, 3, 'ask');
             INSERT INTO cloud_tasks (id, project_id, provider, remote_id, url, prompt, branch,
                                      status)
                 VALUES (10, 1, 'codex', 'r1', 'u', 'p', 'b', 'done'),
                        (11, 1, 'codex', 'r2', 'u', 'p', 'b', 'done');",
        )
        .expect("test setup");

        migrate(&mut conn).expect("migrates");
        // Repositories by name, tasks and sessions oldest first, cloud tasks newest first.
        assert_eq!(in_order(&conn, "projects", "1"), [2, 1]);
        assert_eq!(in_order(&conn, "tasks", "project_id = 1"), [4, 3]);
        assert_eq!(in_order(&conn, "sessions", "task_id = 4"), [7, 9]);
        assert_eq!(in_order(&conn, "cloud_tasks", "project_id = 1"), [11, 10]);
    }

    #[test]
    fn a_store_made_before_knows_who_started_a_session_by_its_first_message() {
        let mut conn = Connection::open_in_memory().expect("opens");
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("sets");
        // Every step before the one that adds the column.
        let adds = |sql: &&str| sql.contains("ADD COLUMN started_by");
        let before = MIGRATIONS.iter().position(adds).expect("there");
        let plain = MIGRATIONS[..before]
            .iter()
            .filter(|sql| **sql != AUTOINCREMENT_IDS);
        for sql in plain {
            conn.execute_batch(sql).expect("migrates");
        }
        let version = i64::try_from(before).expect("a few migrations");
        conn.pragma_update(None, "user_version", version)
            .expect("sets");
        // Session 5 started 6 with `start_task`, 7 the same way with its prompt still waiting
        // for the setup, and 8 with `spawn_session`. The user set 9 to work for 5; 10 was
        // started by 5 and moved to work for 6; 11's first message was the user's; 12 has an
        // event that is no JSON, which must not stop the step; 13 is the user's fork of 6,
        // with a copy of its first message, set to work for 5.
        conn.execute_batch(
            r#"INSERT INTO projects (id, name, repo_path) VALUES (1, 'demo', '/repo');
             INSERT INTO tasks (id, project_id, name, branch, base_ref, worktree_path, slot)
                 VALUES (3, 1, 'one', 'b', 'main', '/w', 1);
             INSERT INTO sessions (id, task_id, supervision, spawned_by, spawned, setup_prompt)
                 VALUES (5, 3, 'ask', NULL, 0, NULL), (6, 3, 'ask', 5, 0, NULL),
                        (7, 3, 'ask', 5, 0, 'Message from session 5 ("Plan"), another agent session of the same user:

Build it.

(Session 5 started this task for that work, which is yours.)'),
                        (8, 3, 'ask', 5, 1, NULL), (9, 3, 'ask', 5, 0, NULL),
                        (10, 3, 'ask', 6, 0, NULL), (11, 3, 'ask', 5, 0, NULL),
                        (12, 3, 'ask', 5, 0, NULL), (13, 3, 'ask', 5, 0, NULL);
             INSERT INTO session_events (session_id, event) VALUES
                 (6, '{"type":"user_message","text":"Message from session 5 (\"Plan\"), another agent session of the same user:\n\nBuild it.\n\n(Session 5 started this task for that work, which is yours.)"}'),
                 (8, '{"type":"user_message","text":"A part.\n\n(You were started by session 5, another agent session of the same user, working in this same worktree.)"}'),
                 (9, '{"type":"user_message","text":"Do it yourself."}'),
                 (10, '{"type":"user_message","text":"Message from session 5 (\"Plan\"), another agent session of the same user:\n\nBuild it.\n\n(Session 5 started this task for that work, which is yours.)"}'),
                 (11, '{"type":"agent_started"}'),
                 (11, '{"type":"user_message","text":"Hello."}'),
                 (11, '{"type":"user_message","text":"Message from session 5 (\"Plan\"), another agent session of the same user:\n\n(Session 5 started this task for that work)"}'),
                 (12, 'not json'),
                 (13, '{"type":"user_message","text":"Message from session 5 (\"Plan\"), another agent session of the same user:\n\nBuild it.\n\n(Session 5 started this task for that work, which is yours.)"}'),
                 (13, '{"type":"forked","from":6}');"#,
        )
        .expect("test setup");

        migrate(&mut conn).expect("migrates");
        let mut statement = conn
            .prepare("SELECT id FROM sessions WHERE started_by = 5 ORDER BY id")
            .expect("reads");
        let rows = statement.query_map([], |row| row.get(0)).expect("reads");
        let started: Vec<i64> = rows.collect::<rusqlite::Result<_>>().expect("reads");
        assert_eq!(started, [6, 7, 8]);
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM sessions WHERE started_by IS NOT NULL"
            ),
            3
        );
    }

    #[test]
    fn a_store_made_before_keeps_its_rows_links_and_indexes_and_reuses_no_id() {
        let mut conn = Connection::open_in_memory().expect("opens");
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("sets");
        // The store as the step before this one left it.
        let rebuilt = MIGRATIONS.iter().position(|sql| *sql == AUTOINCREMENT_IDS);
        let before = rebuilt.expect("the step is there");
        for sql in &MIGRATIONS[..before] {
            conn.execute_batch(sql).expect("migrates");
        }
        let version = i64::try_from(before).expect("a few migrations");
        conn.pragma_update(None, "user_version", version)
            .expect("sets");
        conn.execute_batch(
            "INSERT INTO projects (id, name, repo_path) VALUES (1, 'demo', '/repo');
             INSERT INTO tasks (id, project_id, name, branch, base_ref, worktree_path, slot)
                 VALUES (3, 1, 'one', 'b', 'main', '/w', 1);
             INSERT INTO sessions (id, task_id, supervision) VALUES (7, 3, 'ask');
             INSERT INTO sessions (id, task_id, supervision) VALUES (8, 3, 'ask');
             INSERT INTO session_events (session_id, event) VALUES (7, '{}'), (8, '{}');
             DELETE FROM sessions WHERE id = 8;",
        )
        .expect("test setup");

        migrate(&mut conn).expect("migrates");
        assert!(ids(&conn).iter().all(|(_, autoincrement)| *autoincrement));
        assert_eq!(count(&conn, "PRAGMA foreign_keys"), 1);
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM session_events WHERE session_id = 7"
            ),
            1
        );
        // The indexes were made again: a second active task of that name is refused.
        let twin = conn.execute(
            "INSERT INTO tasks (project_id, name, branch, base_ref, worktree_path, slot) \
             VALUES (1, 'one', 'c', 'main', '/x', 2)",
            [],
        );
        assert!(twin.is_err());
        // The keys still cascade: a session's events go with it.
        conn.execute("DELETE FROM sessions WHERE id = 7", [])
            .expect("deletes");
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM session_events"), 0);
        // 7 was the last session and is gone: it is not given again. (8 was deleted before the
        // store counted, and can be: SQLite starts counting from the highest id there is.)
        conn.execute(
            "INSERT INTO sessions (task_id, supervision) VALUES (3, 'ask')",
            [],
        )
        .expect("inserts");
        assert!(conn.last_insert_rowid() > 7);
    }
}
