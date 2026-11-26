//! The order the sidebar lists things in, which the user can change: repositories, a
//! repository's tasks and cloud tasks, and a task's sessions. Each moves among its siblings
//! only, so nothing changes its parent by being moved. Sub-agents have no order of their own:
//! they are not stored, and come and go with the tool call that started them.
//!
//! A repository, a task or a session can also be pinned: it then stays above its siblings that
//! are not, and moves among the pinned ones only, as the others move among themselves.

use std::collections::HashSet;

use rusqlite::{OptionalExtension, Params, params};

use crate::Core;
use crate::agent::AgentEvent;
use crate::chats::CHATS;
use crate::error::{Error, Result};
use crate::sessions::SESSION_ORDER;
use crate::store::{PROJECT_ORDER, Store, TASK_ORDER};

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

/// One row of a list, in the list's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row {
    id: i64,
    /// Whether the sidebar lists it among the others: not a closed session, an archived task
    /// or one put away, or the chats among the repositories.
    listed: bool,
    pinned: bool,
}

/// `rows` as [`moved`] takes them for a move of `id`: only the listed rows pinned as it is
/// count, so a pinned row moves among the pinned and any other among the others.
fn in_group(rows: &[Row], id: i64) -> Vec<(i64, bool)> {
    let pinned = rows.iter().any(|row| row.id == id && row.pinned);
    let group = |row: &Row| row.listed && row.pinned == pinned;
    rows.iter().map(|row| (row.id, group(row))).collect()
}

/// The ids of `rows` once `id` is pinned, put last among the pinned rows, or unpinned, put first
/// among the others: either way between the two, so it does not jump far. `None` if `id` is not
/// a listed row, or is pinned or not already.
fn pinned_as(rows: &[Row], id: i64, pinned: bool) -> Option<Vec<i64>> {
    let changes = |row: &Row| row.id == id && row.listed && row.pinned != pinned;
    if !rows.iter().any(changes) {
        return None;
    }
    let others = |pinned: bool| {
        rows.iter()
            .filter(move |row| row.id != id && row.pinned == pinned)
            .map(|row| row.id)
    };
    Some(others(true).chain([id]).chain(others(false)).collect())
}

/// The ids of `rows`, which are `(id, listed)` in order, with `id` moved `by` places among
/// the listed ones, earlier for a negative number, and no further than either end. Rows that
/// are not listed, a closed session or an archived task or one put away, keep their places
/// among the others, so one that comes back comes back where it was. `None` if `id` is not a
/// listed row.
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

/// A task's session, with the session it works for, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionRow {
    row: Row,
    parent: Option<i64>,
}

impl Store {
    /// Rows of a list in its order, from `sql` with `params`, which selects each row's id,
    /// whether it is listed and whether it is pinned.
    fn order_rows(&self, sql: &str, params: impl Params) -> Result<Vec<Row>> {
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params, |row| {
            Ok(Row {
                id: row.get(0)?,
                listed: row.get(1)?,
                pinned: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Writes `ids` as the order of their rows of `kind`, and with `pin` pins row `id` or
    /// unpins it, all at once.
    fn order_write(&self, kind: Kind, ids: &[i64], pin: Option<(i64, bool)>) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if let Some((id, pinned)) = pin {
            let sql = format!("UPDATE {} SET pinned = ?1 WHERE id = ?2", kind.table());
            tx.execute(&sql, params![pinned, id])?;
        }
        let sql = format!("UPDATE {} SET position = ?1 WHERE id = ?2", kind.table());
        for (position, id) in (1_i64..).zip(ids) {
            tx.execute(&sql, params![position, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    fn session_rows(&self, task_id: i64) -> Result<Vec<SessionRow>> {
        let sql = format!(
            "SELECT id, closed_at IS NULL, pinned, spawned_by FROM sessions WHERE task_id = ?1 \
             ORDER BY {SESSION_ORDER}"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params![task_id], |row| {
            Ok(SessionRow {
                row: Row {
                    id: row.get(0)?,
                    listed: row.get(1)?,
                    pinned: row.get(2)?,
                },
                parent: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// The siblings of session `id` among `rows`: the sessions that work for the same open session
/// of the task, or, for one that works for no open session of the task, every other such
/// session. One whose parent is gone, closed or in another task is ordinary. However deep the
/// sidebar nests them, these are the rows it draws together under one parent.
fn session_siblings(rows: &[SessionRow], id: i64) -> Vec<Row> {
    let open: HashSet<i64> = rows
        .iter()
        .filter(|session| session.row.listed)
        .map(|session| session.row.id)
        .collect();
    let head = |session: &SessionRow| {
        let by = session.parent;
        by.filter(|by| *by != session.row.id && open.contains(by))
    };
    let mine = rows
        .iter()
        .find(|session| session.row.id == id)
        .and_then(head);
    let siblings = rows.iter().filter(|session| head(session) == mine);
    siblings.map(|session| session.row).collect()
}

/// Every repository's row, the chats' among them but not listed.
fn project_rows_sql() -> String {
    format!("SELECT id, name <> ?1, pinned FROM projects ORDER BY {PROJECT_ORDER}")
}

/// Every row of a repository's tasks, the archived ones and those put away among them but not
/// listed: a task put away is listed apart, with no order of its own, so a move counted among
/// the tasks the sidebar lists together passes none of them, and one brought back comes back
/// where it was.
fn task_rows_sql() -> String {
    format!(
        "SELECT id, archived_at IS NULL AND done_at IS NULL, pinned FROM tasks \
         WHERE project_id = ?1 ORDER BY {TASK_ORDER}"
    )
}

impl Core {
    /// Moves repository `name` `by` places among the others, earlier for a negative number,
    /// and no further than either end: a pinned one among the pinned, any other among the
    /// others. The chats are listed apart, above the repositories, and are not among them.
    pub fn project_move(&self, name: &str, by: i32) -> Result<()> {
        let project = self.project(name)?;
        let rows = self
            .store()
            .order_rows(&project_rows_sql(), params![CHATS])?;
        self.reorder(Kind::Project, &rows, project.id, by)
    }

    /// Pins repository `name` above the repositories that are not pinned, or with `pinned`
    /// false unpins it. Pinned, it is put last among the pinned; unpinned, first among the
    /// others. The chats, listed apart, are left as they are.
    pub fn project_pin(&self, name: &str, pinned: bool) -> Result<()> {
        let project = self.project(name)?;
        let rows = self
            .store()
            .order_rows(&project_rows_sql(), params![CHATS])?;
        self.repin(Kind::Project, &rows, project.id, pinned)
    }

    /// Moves task `task` `by` places among the other open tasks of `project` pinned as it is.
    /// A task put away is not among them, and is refused ([`Error::TaskPutAway`]).
    pub fn task_move(&self, project: &str, task: &str, by: i32) -> Result<()> {
        let (id, rows) = self.open_task_rows(project, task)?;
        self.reorder(Kind::Task, &rows, id, by)
    }

    /// Pins task `task` above the open tasks of `project` that are not pinned, or with
    /// `pinned` false unpins it, as [`Core::project_pin`] does a repository. A chat is pinned
    /// among the chats. A task put away keeps its pin and cannot be pinned or unpinned until it
    /// is brought back ([`Error::TaskPutAway`]).
    pub fn task_pin(&self, project: &str, task: &str, pinned: bool) -> Result<()> {
        let (id, rows) = self.open_task_rows(project, task)?;
        self.repin(Kind::Task, &rows, id, pinned)
    }

    /// The id of task `task` of `project` and the rows of that repository's tasks, or
    /// [`Error::TaskPutAway`] for one put away: saying nothing would look as if it had worked.
    fn open_task_rows(&self, project: &str, task: &str) -> Result<(i64, Vec<Row>)> {
        let (project, task) = self.task(project, task)?;
        if task.done_at.is_some() {
            return Err(Error::TaskPutAway {
                project: project.name,
                task: task.name,
            });
        }
        let rows = self
            .store()
            .order_rows(&task_rows_sql(), params![project.id])?;
        Ok((task.id, rows))
    }

    /// Moves session `id` `by` places among the other open sessions of its task pinned as it
    /// is: a session that works for another of its task among the others that work for that
    /// one, since it is listed under it.
    pub fn session_move(&self, id: i64, by: i32) -> Result<()> {
        let siblings = self.open_session_siblings(id)?;
        self.reorder(Kind::Session, &siblings, id, by)
    }

    /// Pins session `id` above its siblings that are not pinned, or with `pinned` false
    /// unpins it, as [`Core::project_pin`] does a repository: among the others that work for
    /// its parent, for one that works for another session of its task. A closed session keeps
    /// its pin and cannot be pinned or unpinned until it is reopened.
    pub fn session_pin(&self, id: i64, pinned: bool) -> Result<()> {
        let siblings = self.open_session_siblings(id)?;
        self.repin(Kind::Session, &siblings, id, pinned)
    }

    /// The siblings of open session `id`, itself among them, or [`Error::SessionClosed`].
    fn open_session_siblings(&self, id: i64) -> Result<Vec<Row>> {
        let session = self.session(id)?;
        let rows = self.store().session_rows(session.task_id)?;
        let siblings = session_siblings(&rows, id);
        if siblings.iter().any(|row| row.id == id && !row.listed) {
            return Err(Error::SessionClosed(id));
        }
        Ok(siblings)
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
        let sql =
            "SELECT id, 1, 0 FROM cloud_tasks WHERE project_id = ?1 ORDER BY position, id DESC";
        let rows = store.order_rows(sql, params![project])?;
        drop(store);
        self.reorder(Kind::CloudTask, &rows, id, by)
    }

    /// Puts `id` `by` places further among `rows` of `kind` pinned as it is, and tells every
    /// front end.
    fn reorder(&self, kind: Kind, rows: &[Row], id: i64, by: i32) -> Result<()> {
        let before: Vec<i64> = rows.iter().map(|row| row.id).collect();
        // A row that is not listed, such as the chats among the repositories, stays.
        let order = moved(&in_group(rows, id), id, by).unwrap_or_else(|| before.clone());
        // A move that stops where it is, at either end or by nothing, changes nothing: front
        // ends are not sent to read everything again for it.
        if order == before {
            return Ok(());
        }
        self.store().order_write(kind, &order, None)?;
        self.told_reordered(kind);
        Ok(())
    }

    /// Pins `id` among `rows` of `kind`, or unpins it, and tells every front end. One that is
    /// so already, or is not listed, such as the chats among the repositories, stays.
    fn repin(&self, kind: Kind, rows: &[Row], id: i64, pinned: bool) -> Result<()> {
        let Some(order) = pinned_as(rows, id, pinned) else {
            return Ok(());
        };
        self.store().order_write(kind, &order, Some((id, pinned)))?;
        self.told_reordered(kind);
        Ok(())
    }

    /// Has front ends read the tree again, after rows of `kind` were put in another order.
    fn told_reordered(&self, kind: Kind) {
        let kind = kind.name().to_string();
        self.live.publish_apart(AgentEvent::Reordered { kind });
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

    /// Rows from `(id, listed, pinned)`.
    fn rows(rows: &[(i64, bool, bool)]) -> Vec<Row> {
        let row = |&(id, listed, pinned): &(i64, bool, bool)| Row { id, listed, pinned };
        rows.iter().map(row).collect()
    }

    #[test]
    fn a_session_moves_among_those_that_work_for_its_parent() {
        // 2 and 3 work for 1; 4 for 5, which is closed; 6 for a session elsewhere.
        let session = |id: i64, parent: Option<i64>, listed: bool| SessionRow {
            row: Row {
                id,
                listed,
                pinned: false,
            },
            parent,
        };
        let sessions = vec![
            session(1, None, true),
            session(2, Some(1), true),
            session(3, Some(1), true),
            session(4, Some(5), true),
            session(5, None, false),
            session(6, Some(99), true),
        ];
        let brood = rows(&[(2, true, false), (3, true, false)]);
        assert_eq!(session_siblings(&sessions, 3), brood);
        let ordinary = rows(&[
            (1, true, false),
            (4, true, false),
            (5, false, false),
            (6, true, false),
        ]);
        assert_eq!(session_siblings(&sessions, 4), ordinary);
        assert_eq!(session_siblings(&sessions, 1), ordinary);
    }

    #[test]
    fn a_session_deeper_down_moves_among_those_under_the_same_parent() {
        // 1 over 2 and 3; 2 over 4 and 5.
        let session = |id: i64, parent: Option<i64>| SessionRow {
            row: Row {
                id,
                listed: true,
                pinned: false,
            },
            parent,
        };
        let sessions = vec![
            session(1, None),
            session(2, Some(1)),
            session(4, Some(2)),
            session(5, Some(2)),
            session(3, Some(1)),
        ];
        let under_two = rows(&[(4, true, false), (5, true, false)]);
        assert_eq!(session_siblings(&sessions, 5), under_two);
        let under_one = rows(&[(2, true, false), (3, true, false)]);
        assert_eq!(session_siblings(&sessions, 3), under_one);
    }

    #[test]
    fn a_row_moves_among_those_pinned_as_it_is() {
        // 1 and 2 are pinned, 3 is pinned and closed, 4 and 5 are not pinned.
        let list = rows(&[
            (1, true, true),
            (2, true, true),
            (3, false, true),
            (4, true, false),
            (5, true, false),
        ]);
        let pinned = in_group(&list, 2);
        assert_eq!(
            moved(&pinned, 2, 9),
            Some(vec![1, 2, 3, 4, 5]),
            "no further"
        );
        assert_eq!(moved(&pinned, 2, -1), Some(vec![2, 1, 3, 4, 5]));
        let others = in_group(&list, 4);
        assert_eq!(
            moved(&others, 4, -9),
            Some(vec![1, 2, 3, 4, 5]),
            "no higher"
        );
        assert_eq!(moved(&others, 4, 1), Some(vec![1, 2, 3, 5, 4]));
    }

    #[test]
    fn a_pinned_row_goes_last_among_the_pinned_and_an_unpinned_one_first_among_the_others() {
        let list = rows(&[
            (1, true, true),
            (2, false, true),
            (3, true, false),
            (4, true, false),
        ]);
        assert_eq!(pinned_as(&list, 4, true), Some(vec![1, 2, 4, 3]));
        assert_eq!(pinned_as(&list, 1, false), Some(vec![2, 1, 3, 4]));
        assert_eq!(pinned_as(&list, 1, true), None, "pinned already");
        assert_eq!(pinned_as(&list, 3, false), None, "not pinned already");
        assert_eq!(
            pinned_as(&list, 2, false),
            None,
            "a closed one is not unpinned"
        );
        assert_eq!(pinned_as(&list, 9, true), None);
    }
}
