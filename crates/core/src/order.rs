//! The order the sidebar lists things in, which the user can change: repositories, a
//! repository's tasks and cloud tasks, and a task's sessions. Each moves among its siblings
//! only, so nothing changes its parent by being moved. Sub-agents have no order of their own:
//! they are not stored, and come and go with the tool call that started them.

use std::collections::HashSet;

use rusqlite::{OptionalExtension, Params, params};

use crate::Core;
use crate::agent::AgentEvent;
use crate::error::{Error, Result};
use crate::store::Store;

/// What can be put in order, each in a table of its own.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Project,
    Task,
    Session,
    CloudTask,
}

impl Kind {
    fn table(self) -> &'static str {
        match self {
            Self::Project => "projects",
            Self::Task => "tasks",
            Self::Session => "sessions",
            Self::CloudTask => "cloud_tasks",
        }
    }

    /// What front ends are told was moved.
    fn name(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Task => "task",
            Self::Session => "session",
            Self::CloudTask => "cloud_task",
        }
    }
}

/// The ids of `rows`, which are `(id, listed)` in order, with `id` moved `by` places among
/// the listed ones, earlier for a negative number, and no further than either end. Rows that
/// are not listed, a closed session or an archived task, keep their places among the others,
/// so one that comes back comes back where it was. `None` if `id` is not a listed row.
fn moved(rows: &[(i64, bool)], id: i64, by: i32) -> Option<Vec<i64>> {
    let listed: Vec<i64> = rows.iter().filter(|row| row.1).map(|row| row.0).collect();
    let at = listed.iter().position(|row| *row == id)?;
    let last = listed.len() - 1;
    let wanted = i64::try_from(at).unwrap_or(0) + i64::from(by);
    let to = usize::try_from(wanted.max(0)).map_or(last, |to| to.min(last));
    let mut order: Vec<i64> = rows
        .iter()
        .map(|row| row.0)
        .filter(|row| *row != id)
        .collect();
    if to == at {
        return Some(rows.iter().map(|row| row.0).collect());
    }
    // Next to the listed row it is moved past: after it going down, before it going up.
    let past = order.iter().position(|row| *row == listed[to])?;
    order.insert(if to > at { past + 1 } else { past }, id);
    Some(order)
}

/// A task's sessions as `(id, started by, open)`, in order.
type SessionRows = Vec<(i64, Option<i64>, bool)>;

impl Store {
    /// Rows of a list in its order, as `(id, listed)`, from `sql` with `params`.
    fn order_rows(&self, sql: &str, params: impl Params) -> Result<Vec<(i64, bool)>> {
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params, |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Writes `ids` as the order of their rows of `kind`, all at once.
    fn order_write(&self, kind: Kind, ids: &[i64]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let sql = format!("UPDATE {} SET position = ?1 WHERE id = ?2", kind.table());
        for (position, id) in (1_i64..).zip(ids) {
            tx.execute(&sql, params![position, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    fn session_rows(&self, task_id: i64) -> Result<SessionRows> {
        let mut statement = self.conn.prepare(
            "SELECT id, spawned_by, closed_at IS NULL FROM sessions WHERE task_id = ?1 \
             ORDER BY position, id",
        )?;
        let rows = statement.query_map(params![task_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// The siblings of session `id` among `rows`, as `(id, open)`: the sessions started by the
/// same open session of the task, or, for one that no open session of the task started,
/// every other such session. One whose starter is gone, closed or elsewhere is ordinary.
fn session_siblings(rows: &SessionRows, id: i64) -> Vec<(i64, bool)> {
    let open: HashSet<i64> = rows.iter().filter(|row| row.2).map(|row| row.0).collect();
    let head = |(session, by, _): &(i64, Option<i64>, bool)| {
        by.filter(|by| by != session && open.contains(by))
    };
    let mine = rows.iter().find(|row| row.0 == id).and_then(head);
    let siblings = rows.iter().filter(|row| head(row) == mine);
    siblings.map(|row| (row.0, row.2)).collect()
}

impl Core {
    /// Moves repository `name` `by` places among the others, earlier for a negative number,
    /// and no further than either end.
    pub fn project_move(&self, name: &str, by: i32) -> Result<()> {
        let project = self.project(name)?;
        let sql = "SELECT id, 1 FROM projects ORDER BY position, name";
        let rows = self.store().order_rows(sql, [])?;
        self.reorder(Kind::Project, &rows, project.id, by)
    }

    /// Moves task `task` `by` places among the other open tasks of `project`.
    pub fn task_move(&self, project: &str, task: &str, by: i32) -> Result<()> {
        let (project, task) = self.task(project, task)?;
        let sql = "SELECT id, archived_at IS NULL FROM tasks WHERE project_id = ?1 \
                   ORDER BY position, created_at, id";
        let rows = self.store().order_rows(sql, params![project.id])?;
        self.reorder(Kind::Task, &rows, task.id, by)
    }

    /// Moves session `id` `by` places among the other open sessions of its task: a session
    /// another started among the others that one started, since it is listed under it.
    pub fn session_move(&self, id: i64, by: i32) -> Result<()> {
        let session = self.session(id)?;
        let rows = self.store().session_rows(session.task_id)?;
        let siblings = session_siblings(&rows, id);
        if siblings
            .iter()
            .any(|(sibling, open)| *sibling == id && !open)
        {
            return Err(Error::SessionClosed(id));
        }
        self.reorder(Kind::Session, &siblings, id, by)
    }

    /// Moves cloud task `id` `by` places among the other cloud tasks of its repository.
    pub fn cloud_task_move(&self, id: i64, by: i32) -> Result<()> {
        let store = self.store();
        let project: Option<i64> = store
            .conn
            .query_row(
                "SELECT project_id FROM cloud_tasks WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let project = project.ok_or(Error::CloudTaskNotFound(id))?;
        let sql = "SELECT id, 1 FROM cloud_tasks WHERE project_id = ?1 ORDER BY position, id DESC";
        let rows = store.order_rows(sql, params![project])?;
        drop(store);
        self.reorder(Kind::CloudTask, &rows, id, by)
    }

    /// Puts `id` `by` places further among `rows` of `kind`, and tells every front end.
    fn reorder(&self, kind: Kind, rows: &[(i64, bool)], id: i64, by: i32) -> Result<()> {
        let before: Vec<i64> = rows.iter().map(|row| row.0).collect();
        // Every caller found `id` among `rows` as listed.
        let order = moved(rows, id, by).unwrap_or_else(|| before.clone());
        // A move that stops where it is, at either end or by nothing, changes nothing: front
        // ends are not sent to read everything again for it.
        if order == before {
            return Ok(());
        }
        self.store().order_write(kind, &order)?;
        let kind = kind.name().to_string();
        self.live.publish_apart(AgentEvent::Reordered { kind });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTED: [(i64, bool); 4] = [(1, true), (2, true), (3, true), (4, true)];

    #[test]
    fn a_row_moves_by_places_and_stops_at_either_end() {
        assert_eq!(moved(&LISTED, 3, -1), Some(vec![1, 3, 2, 4]));
        assert_eq!(moved(&LISTED, 1, 2), Some(vec![2, 3, 1, 4]));
        assert_eq!(moved(&LISTED, 2, -9), Some(vec![2, 1, 3, 4]));
        assert_eq!(moved(&LISTED, 2, 9), Some(vec![1, 3, 4, 2]));
        assert_eq!(moved(&LISTED, 4, 1), Some(vec![1, 2, 3, 4]));
        assert_eq!(moved(&LISTED, 9, 1), None);
    }

    #[test]
    fn rows_that_are_not_listed_keep_their_places() {
        // 2 is closed: moving 3 up passes 1, not 2, and 2 stays between where it was.
        let rows = [(1, true), (2, false), (3, true), (4, true)];
        assert_eq!(moved(&rows, 3, -1), Some(vec![3, 1, 2, 4]));
        assert_eq!(moved(&rows, 1, 1), Some(vec![2, 3, 1, 4]));
        assert_eq!(moved(&rows, 2, 1), None, "a closed one is not moved");
    }

    #[test]
    fn a_started_session_moves_among_those_its_starter_started() {
        // 1 started 2 and 3; 4 was started by 5, which is closed; 6 by a session elsewhere.
        let rows: SessionRows = vec![
            (1, None, true),
            (2, Some(1), true),
            (3, Some(1), true),
            (4, Some(5), true),
            (5, None, false),
            (6, Some(99), true),
        ];
        assert_eq!(session_siblings(&rows, 3), vec![(2, true), (3, true)]);
        let ordinary = vec![(1, true), (4, true), (5, false), (6, true)];
        assert_eq!(session_siblings(&rows, 4), ordinary);
        assert_eq!(session_siblings(&rows, 1), ordinary);
    }
}
