//! Checkpoints: the worktree's files as a git tree before each message goes to the agent and
//! when its turn ends, kept outside the repository, so that what one turn changed can be shown
//! on its own (#363).
//!
//! The trees and their objects are in a store of the task's own under the data directory
//! ([`Paths::checkpoints`]); a row of the `checkpoints` table names each, with the session,
//! the message's number (counted as a fork counts them) and its side. Nothing is written into
//! the repository. Each session keeps the latest [`KEPT_TURNS`] turns; the objects only older
//! ones needed are pruned on a thread of their own. A chat has no repository and no
//! checkpoints.

mod trees;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Row, params};
use serde::Serialize;

use crate::changes::{FileChange, FileDiff, cut, stays_inside};
use crate::error::{Error, Result};
use crate::sessions::{Live, is_busy_status, title_from};
use crate::store::Store;
use crate::{Core, Paths, lock};

pub use trees::{MAX_UNTRACKED_BYTES, SkippedFile};

/// How many turns of each session keep their checkpoints: the latest ones.
pub const KEPT_TURNS: usize = 50;

/// Which end of a turn a checkpoint was taken at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// Before the message that began the turn went to the agent.
    Before,
    /// When the turn ended.
    After,
}

impl Side {
    fn as_str(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
        }
    }
}

/// One checkpoint as the table keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Checkpoint {
    session: i64,
    message: usize,
    side: Side,
    tree: String,
    skipped: Vec<SkippedFile>,
    /// The start of the message, kept with its `before`.
    words: String,
    at_ms: i64,
}

/// One turn of a session: a message sent to its agent, and what came of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Turn {
    /// The message that began it, by its number in the session: counting from 0 every message
    /// sent into it, as a fork counts them.
    pub message: usize,
    /// The start of that message.
    pub words: String,
    /// When the message went, in milliseconds since 1970.
    pub at_ms: i64,
    /// Whether its end was recorded. Without one, its changes are counted up to now: it is
    /// under way, or its agent went without ending it.
    pub ended: bool,
    /// The titles of the task's other sessions that were at work while it was. What they
    /// changed meanwhile is in its changes too: a checkpoint cannot tell who wrote a file.
    pub beside: Vec<String>,
}

/// What one turn changed in the worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnChanges {
    /// The files that differ from where the turn began to where it ended, sorted by path.
    pub files: Vec<FileChange>,
    /// Files git does not track that were too large to keep, at either end: what became of
    /// them is not known.
    pub skipped: Vec<SkippedFile>,
    /// Whether the turn's end was recorded; without one, this is up to now.
    pub ended: bool,
}

/// What this process knows of the turns under way, and of the stores being pruned.
#[derive(Debug, Default)]
pub(crate) struct Checkpoints {
    /// The message whose turn each session is in, for a turn that has its `before`.
    under_way: Mutex<HashMap<i64, usize>>,
    /// The tasks whose store is being pruned now.
    pruning: Arc<Mutex<HashSet<i64>>>,
}

impl Checkpoints {
    /// Forgets the turn session `id` was in: its agent went without ending it.
    pub(crate) fn forget(&self, id: i64) {
        lock(&self.under_way).remove(&id);
    }
}

/// Where a session's checkpoints are taken and kept.
struct Place {
    task: i64,
    worktree: PathBuf,
    store: PathBuf,
}

/// Session `id`'s place, or nothing for a chat's, which has no repository.
fn place(store: &Mutex<Store>, paths: &Paths, id: i64) -> Option<Place> {
    let store = lock(store);
    let session = store.session_by_id(id).ok()?;
    let task = store.task_by_id(session.task_id).ok()??;
    let project = store.project_by_id(task.project_id).ok()??;
    (!project.chats).then(|| Place {
        task: task.id,
        store: paths.checkpoints(task.id),
        worktree: task.worktree_path,
    })
}

/// The worktree as it stands before message number `message` of session `id`, whose text is
/// `text`, goes to its agent: called once the message is recorded, and finished before it
/// goes, so that nothing the agent writes is in it. A checkpoint that cannot be taken keeps
/// no message back: that turn is only not listed.
pub(crate) fn before_message(
    (store, live, paths): (&Mutex<Store>, &Live, &Paths),
    id: i64,
    (message, text): (usize, &str),
) {
    live.checkpoints.forget(id);
    let Some(place) = place(store, paths, id) else {
        return;
    };
    let Ok(taken) = trees::take(&place.worktree, &place.store) else {
        return;
    };
    let checkpoint = Checkpoint {
        session: id,
        message,
        side: Side::Before,
        tree: taken.tree,
        skipped: taken.skipped,
        words: title_from(text),
        at_ms: now_ms(),
    };
    if lock(store)
        .checkpoint_keep(&checkpoint, taken.took_ms)
        .is_err()
    {
        return;
    }
    lock(&live.checkpoints.under_way).insert(id, message);
    let dropped = lock(store).checkpoints_drop_old(id, KEPT_TURNS);
    if dropped.is_ok_and(|dropped| dropped > 0) {
        prune_later(store, &live.checkpoints, place);
    }
}

/// The worktree as it stands when session `id`'s turn ends, for the turn whose `before` this
/// process took. Called before the end is told, so that a message sent at once does not begin
/// its turn before this one's end is kept.
pub(crate) fn after_turn((store, live, paths): (&Mutex<Store>, &Live, &Paths), id: i64) {
    let Some(message) = lock(&live.checkpoints.under_way).remove(&id) else {
        return;
    };
    let Some(place) = place(store, paths, id) else {
        return;
    };
    let Ok(taken) = trees::take(&place.worktree, &place.store) else {
        return;
    };
    let checkpoint = Checkpoint {
        session: id,
        message,
        side: Side::After,
        tree: taken.tree,
        skipped: taken.skipped,
        words: String::new(),
        at_ms: now_ms(),
    };
    lock(store).checkpoint_keep(&checkpoint, taken.took_ms).ok();
}

/// Deletes, on a thread of its own, the objects of the task's store that no kept checkpoint
/// needs. One prune per store at a time; one asked for meanwhile is left to the next turn.
fn prune_later(store: &Mutex<Store>, checkpoints: &Checkpoints, place: Place) {
    let Ok(keep) = lock(store).checkpoint_trees(place.task) else {
        return;
    };
    let pruning = Arc::clone(&checkpoints.pruning);
    if !lock(&pruning).insert(place.task) {
        return;
    }
    std::thread::spawn(move || {
        trees::prune(&place.worktree, &place.store, &keep).ok();
        lock(&pruning).remove(&place.task);
    });
}

fn now_ms() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
}

/// Where one turn began and ended, as checkpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span<'a> {
    before: &'a Checkpoint,
    /// Its `after`, or else the next turn's `before`, or nothing: it has no end yet.
    end: Option<&'a Checkpoint>,
}

/// The turns of one session's `checkpoints`, sorted by message, each with its end.
fn spans(checkpoints: &[Checkpoint]) -> Vec<Span<'_>> {
    let starts: Vec<&Checkpoint> = checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.side == Side::Before)
        .collect();
    let after = |message: usize| {
        checkpoints
            .iter()
            .find(|checkpoint| checkpoint.side == Side::After && checkpoint.message == message)
    };
    starts
        .iter()
        .enumerate()
        .map(|(at, before)| Span {
            before,
            end: after(before.message).or_else(|| starts.get(at + 1).copied()),
        })
        .collect()
}

/// When `span` began and ended, in milliseconds since 1970: a turn with no end lasts until
/// `now` if its session is still at work, and is over the moment it began if not.
fn interval(span: &Span<'_>, at_work: bool, now: i64) -> (i64, i64) {
    let start = span.before.at_ms;
    let end = span
        .end
        .map_or(if at_work { now } else { start }, |end| end.at_ms);
    (start, end.max(start))
}

/// Whether two stretches of time share a moment. Ones that only touch do not.
fn overlap(a: (i64, i64), b: (i64, i64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// The other sessions of the task, each with its title, whether it is at work now, and when
/// its turns were.
struct Others(Vec<(String, Vec<(i64, i64)>)>);

impl Others {
    /// The titles of those at work at some moment of `during`.
    fn beside(&self, during: (i64, i64)) -> Vec<String> {
        self.0
            .iter()
            .filter(|(_, turns)| turns.iter().any(|turn| overlap(*turn, during)))
            .map(|(title, _)| title.clone())
            .collect()
    }
}

impl Core {
    /// The turns of session `id` whose checkpoints are kept, oldest first: the latest
    /// [`KEPT_TURNS`]. A chat's session has none.
    pub fn session_turns(&self, id: i64) -> Result<Vec<Turn>> {
        let session = self.session(id)?;
        let mut by_session = self.store().checkpoints_of_task(session.task_id)?;
        let own = by_session.remove(&id).unwrap_or_default();
        let now = now_ms();
        let others = self.others(by_session, now);
        Ok(spans(&own)
            .iter()
            .map(|span| Turn {
                message: span.before.message,
                words: span.before.words.clone(),
                at_ms: span.before.at_ms,
                ended: span.end.is_some(),
                beside: others.beside(interval(span, true, now)),
            })
            .collect())
    }

    /// The other sessions of a task, from their checkpoints. One deleted meanwhile is left out.
    fn others(&self, by_session: BTreeMap<i64, Vec<Checkpoint>>, now: i64) -> Others {
        let other = |(id, checkpoints): (i64, Vec<Checkpoint>)| {
            let session = self.store().session_by_id(id).ok()?;
            let at_work = is_busy_status(session.status);
            let turns = spans(&checkpoints)
                .iter()
                .map(|span| interval(span, at_work, now))
                .collect();
            Some((session.title, turns))
        };
        Others(by_session.into_iter().filter_map(other).collect())
    }

    /// What turn `message` of session `id` changed in the worktree. A turn with no end yet is
    /// compared with the worktree as it is now.
    pub async fn session_turn_changes(&self, id: i64, message: usize) -> Result<TurnChanges> {
        let ends = self.turn_ends(id, message)?;
        blocking(move || {
            let (to, mut skipped) = ends.end()?;
            let files = trees::changed(&ends.worktree, &ends.store, (&ends.from, &to))?;
            skipped.extend(ends.skipped.iter().cloned());
            let skipped = once_each(skipped);
            let left_out: HashSet<&str> = skipped.iter().map(|file| file.path.as_str()).collect();
            let files = files
                .into_iter()
                .filter(|file| !left_out.contains(file.path.as_str()))
                .collect();
            Ok(TurnChanges {
                files,
                skipped,
                ended: ends.to.is_some(),
            })
        })
        .await
    }

    /// How one file changed in turn `message` of session `id`, as a unified diff.
    pub async fn session_turn_diff(&self, id: i64, message: usize, path: &str) -> Result<FileDiff> {
        if !stays_inside(path) {
            return Err(Error::PathOutsideWorktree(path.to_string()));
        }
        let ends = self.turn_ends(id, message)?;
        let path = path.to_string();
        blocking(move || {
            let (to, _) = ends.end()?;
            let text = trees::diff(&ends.worktree, &ends.store, (&ends.from, &to), &path)?;
            Ok(cut(text))
        })
        .await
    }

    /// Where turn `message` of session `id` began and ended.
    fn turn_ends(&self, id: i64, message: usize) -> Result<Ends> {
        let session = self.session(id)?;
        let not_kept = || Error::TurnNotKept {
            session: id,
            message,
        };
        let store = self.store();
        let task = store.task_by_id(session.task_id)?.ok_or_else(not_kept)?;
        let checkpoints = store
            .checkpoints_of_task(task.id)?
            .remove(&id)
            .unwrap_or_default();
        drop(store);
        let spans = spans(&checkpoints);
        let span = spans
            .iter()
            .find(|span| span.before.message == message)
            .ok_or_else(not_kept)?;
        let mut skipped = span.before.skipped.clone();
        skipped.extend(span.end.iter().flat_map(|end| end.skipped.iter().cloned()));
        Ok(Ends {
            store: self.paths.checkpoints(task.id),
            worktree: task.worktree_path,
            from: span.before.tree.clone(),
            to: span.end.map(|end| end.tree.clone()),
            skipped,
        })
    }

    /// Deletes a task's checkpoints, its store and its rows: the task is going.
    pub(crate) fn forget_checkpoints(&self, task: i64) -> Result<()> {
        self.store().checkpoints_forget_task(task)?;
        match std::fs::remove_dir_all(self.paths.checkpoints(task)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }
}

/// Where one turn began and ended, with where to read them.
struct Ends {
    worktree: PathBuf,
    store: PathBuf,
    from: String,
    /// Where it ended; nothing if it has not, as far as is known.
    to: Option<String>,
    skipped: Vec<SkippedFile>,
}

impl Ends {
    /// The tree the turn ended at, with what was left out of it: the worktree as it is now
    /// for a turn with no end, which is not kept.
    fn end(&self) -> Result<(String, Vec<SkippedFile>)> {
        match &self.to {
            Some(tree) => Ok((tree.clone(), vec![])),
            None => trees::take(&self.worktree, &self.store).map(|now| (now.tree, now.skipped)),
        }
    }
}

/// `files` with each path once, the largest size kept, sorted by path.
fn once_each(files: Vec<SkippedFile>) -> Vec<SkippedFile> {
    let mut by_path: BTreeMap<String, u64> = BTreeMap::new();
    for file in files {
        let bytes = by_path.entry(file.path).or_default();
        *bytes = (*bytes).max(file.bytes);
    }
    by_path
        .into_iter()
        .map(|(path, bytes)| SkippedFile { path, bytes })
        .collect()
}

/// Runs `work`, which waits on git, where it holds up no task of the runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| Error::Io(std::io::Error::other(error)))?
}

impl Store {
    /// Keeps a checkpoint, in place of one of the same session, message and side.
    fn checkpoint_keep(&self, checkpoint: &Checkpoint, took_ms: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO checkpoints \
             (session_id, message, side, tree, skipped, words, took_ms, at_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                checkpoint.session,
                i64::try_from(checkpoint.message).unwrap_or(i64::MAX),
                checkpoint.side.as_str(),
                checkpoint.tree,
                serde_json::to_string(&checkpoint.skipped)?,
                checkpoint.words,
                i64::try_from(took_ms).unwrap_or(i64::MAX),
                checkpoint.at_ms,
            ],
        )?;
        Ok(())
    }

    /// The checkpoints of every session of a task, by session, each sorted by message.
    fn checkpoints_of_task(&self, task: i64) -> Result<BTreeMap<i64, Vec<Checkpoint>>> {
        let mut statement = self.conn.prepare(
            "SELECT c.session_id, c.message, c.side, c.tree, c.skipped, c.words, c.at_ms \
             FROM checkpoints c JOIN sessions s ON s.id = c.session_id \
             WHERE s.task_id = ?1 ORDER BY c.session_id, c.message, c.side DESC",
        )?;
        let rows = statement.query_map(params![task], checkpoint_from_row)?;
        let mut by_session: BTreeMap<i64, Vec<Checkpoint>> = BTreeMap::new();
        for checkpoint in rows {
            let checkpoint = checkpoint?;
            by_session
                .entry(checkpoint.session)
                .or_default()
                .push(checkpoint);
        }
        Ok(by_session)
    }

    /// The trees every kept checkpoint of a task's sessions names.
    fn checkpoint_trees(&self, task: i64) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT c.tree FROM checkpoints c JOIN sessions s ON s.id = c.session_id \
             WHERE s.task_id = ?1",
        )?;
        let rows = statement.query_map(params![task], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Forgets the checkpoints of session `id`'s turns but the latest `kept`; says how many
    /// were forgotten.
    fn checkpoints_drop_old(&self, id: i64, kept: usize) -> Result<usize> {
        let dropped = self.conn.execute(
            "DELETE FROM checkpoints WHERE session_id = ?1 AND message < (
                 SELECT MIN(message) FROM (
                     SELECT DISTINCT message FROM checkpoints WHERE session_id = ?1
                     ORDER BY message DESC LIMIT ?2))",
            params![id, i64::try_from(kept).unwrap_or(i64::MAX)],
        )?;
        Ok(dropped)
    }

    /// Forgets every checkpoint of a task's sessions.
    pub(crate) fn checkpoints_forget_task(&self, task: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM checkpoints WHERE session_id IN \
             (SELECT id FROM sessions WHERE task_id = ?1)",
            params![task],
        )?;
        Ok(())
    }
}

fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<Checkpoint> {
    let side: String = row.get(2)?;
    let skipped: String = row.get(4)?;
    Ok(Checkpoint {
        session: row.get(0)?,
        message: usize::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
        side: if side == "after" {
            Side::After
        } else {
            Side::Before
        },
        tree: row.get(3)?,
        // Written by this code only; one that cannot be read leaves out nothing it can name.
        skipped: serde_json::from_str(&skipped).unwrap_or_default(),
        words: row.get(5)?,
        at_ms: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkpoint(message: usize, side: Side, at_ms: i64) -> Checkpoint {
        Checkpoint {
            session: 1,
            message,
            side,
            tree: format!("{message}-{}", side.as_str()),
            skipped: vec![],
            words: format!("message {message}"),
            at_ms,
        }
    }

    #[test]
    fn only_the_latest_turns_of_a_session_keep_their_checkpoints() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&dir.path().join("swarmed.db")).expect("opens");
        let project = store.project_insert("p", dir.path()).expect("inserts");
        let task = store
            .task_insert(&crate::store::NewTask {
                project_id: project.id,
                name: "t",
                branch: "b",
                base_ref: "main",
                worktree_path: dir.path(),
                slot: 1,
                environment: None,
                branch_made: true,
                issue: None,
            })
            .expect("inserts");
        let options = crate::SessionOptions::default();
        let session = store.session_insert(task.id, &options).expect("inserts");
        let sides = (0..KEPT_TURNS + 3).flat_map(|message| {
            [Side::Before, Side::After].map(|side| checkpoint(message, side, 0))
        });
        for kept in sides {
            let kept = Checkpoint {
                session: session.id,
                ..kept
            };
            store.checkpoint_keep(&kept, 0).expect("keeps");
        }
        assert_eq!(
            store
                .checkpoints_drop_old(session.id, KEPT_TURNS)
                .expect("drops"),
            6
        );
        assert_eq!(
            store
                .checkpoints_drop_old(session.id, KEPT_TURNS)
                .expect("drops"),
            0
        );
        let kept = store.checkpoints_of_task(task.id).expect("reads");
        let kept = &kept[&session.id];
        assert_eq!(kept.len(), 2 * KEPT_TURNS);
        assert_eq!(kept[0].message, 3);
        assert_eq!(
            store.checkpoint_trees(task.id).expect("reads").len(),
            2 * KEPT_TURNS
        );
        store.checkpoints_forget_task(task.id).expect("forgets");
        assert!(
            store
                .checkpoints_of_task(task.id)
                .expect("reads")
                .is_empty()
        );
    }

    #[test]
    fn a_turn_ends_at_its_after_or_else_where_the_next_began() {
        let checkpoints = [
            checkpoint(0, Side::Before, 10),
            checkpoint(0, Side::After, 20),
            checkpoint(2, Side::Before, 30),
            checkpoint(3, Side::Before, 40),
        ];
        let spans = spans(&checkpoints);
        let ends: Vec<Option<&str>> = spans
            .iter()
            .map(|span| span.end.map(|end| end.tree.as_str()))
            .collect();
        // Message 1 was told mid-turn or came from elsewhere: it has no turn of its own.
        assert_eq!(ends, [Some("0-after"), Some("3-before"), None]);
    }

    #[test]
    fn a_turn_with_no_end_lasts_until_now_only_while_its_session_works() {
        let before = checkpoint(0, Side::Before, 10);
        let span = Span {
            before: &before,
            end: None,
        };
        assert_eq!(interval(&span, true, 50), (10, 50));
        assert_eq!(interval(&span, false, 50), (10, 10));
    }

    #[test]
    fn only_sessions_at_work_during_a_turn_are_beside_it() {
        let others = Others(vec![
            ("before it".to_string(), vec![(0, 10)]),
            ("during it".to_string(), vec![(0, 10), (15, 25)]),
            ("right after".to_string(), vec![(20, 30)]),
            ("began during it, end unknown".to_string(), vec![(12, 12)]),
            ("began after it, end unknown".to_string(), vec![(25, 25)]),
        ]);
        assert_eq!(
            others.beside((10, 20)),
            ["during it", "began during it, end unknown"]
        );
    }

    #[test]
    fn a_file_left_out_at_both_ends_is_said_once_at_its_largest() {
        let file = |path: &str, bytes| SkippedFile {
            path: path.to_string(),
            bytes,
        };
        let skipped = once_each(vec![file("b.bin", 20), file("a.bin", 5), file("b.bin", 30)]);
        assert_eq!(skipped, [file("a.bin", 5), file("b.bin", 30)]);
    }
}
