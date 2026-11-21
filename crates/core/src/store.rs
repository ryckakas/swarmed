//! Persistence in a local SQLite file.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, ToSql, Transaction, params};
use serde::Serialize;

use crate::error::{Error, Result};

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
    /// The project's environment the task was created in, if it was created in one.
    pub environment: Option<String>,
}

pub(crate) struct NewTask<'a> {
    pub(crate) project_id: i64,
    pub(crate) name: &'a str,
    pub(crate) branch: &'a str,
    pub(crate) base_ref: &'a str,
    pub(crate) worktree_path: &'a Path,
    pub(crate) slot: u32,
    pub(crate) environment: Option<&'a str>,
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
];

/// Stands in [`MIGRATIONS`] for the one change SQL in a transaction cannot make: every
/// table's `INTEGER PRIMARY KEY` becomes `INTEGER PRIMARY KEY AUTOINCREMENT`
/// ([`rebuild_with_autoincrement`]).
const AUTOINCREMENT_IDS: &str = "-- every table's id becomes AUTOINCREMENT";

const TASK_SELECT: &str = "SELECT id, project_id, name, branch, base_ref, worktree_path, slot, \
                           setup_status, archived_at IS NOT NULL, environment FROM tasks";

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
            "INSERT INTO tasks (project_id, name, branch, base_ref, worktree_path, slot, environment)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                new.project_id,
                new.name,
                new.branch,
                new.base_ref,
                new.worktree_path.to_string_lossy(),
                new.slot,
                new.environment
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
            environment: new.environment.map(String::from),
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
        environment: row.get(9)?,
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

    #[test]
    fn a_store_made_before_keeps_its_rows_links_and_indexes_and_reuses_no_id() {
        let mut conn = Connection::open_in_memory().expect("opens");
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("sets");
        // The store as the step before this one left it.
        let before = MIGRATIONS.len() - 1;
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
