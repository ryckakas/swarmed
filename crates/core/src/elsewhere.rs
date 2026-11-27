//! What other Swarmed processes write to the same data: the command line, or a second app.
//!
//! Every process publishes only on its own stream, so a window hears nothing another process
//! does unless its own core notices. [`Core::notice_others`] looks, and publishes what it finds
//! as if this core had written it:
//!
//! - **Events.** Each event above the highest one dealt with is published in order, with its
//!   session's status and unread state, as `Live::note` would have. The events this process
//!   stored itself are left out ([`Written`]), so nothing is published twice.
//! - **The tree.** When SQLite's `data_version` says another connection committed, the rows the
//!   tree shows are read again; if they changed, [`AgentEvent::TreeChanged`] tells front ends
//!   to read it again. So does a stack started or stopped elsewhere.
//! - **Configuration files.** One changed on disk, by another process or by hand, is published
//!   as [`AgentEvent::ConfigChanged`], and a changed `agents.toml` is read again. They are
//!   known by their ending: `.toml`, `.json`, and `.css` for the user's stylesheet, whose
//!   change the window then asks about (`stylesheet.rs`).
//! - **Settings of a session whose agent runs here.** A model, effort or supervision another
//!   process stored, as `swarmed session set` does, is passed on to the agent as a change made
//!   here would be ([`Core::settings_told`]): that process can only store it.
//!
//! A session's agent runs in one process at a time. The process that starts it holds a lock on
//! `run/sessions/<id>.lock` until the agent has gone, and the system lets go of it when the
//! process ends, however it ends. Another process refuses to start that session meanwhile, and
//! a message queued for it waits until that run is over.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{File, OpenOptions, TryLockError};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, params};

use crate::agent::{AgentEvent, Auth};
use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::sessions::{
    Live, SessionEvent, SessionStatus, event_from_json, leaves_something_to_read, track,
};
use crate::store::Store;
use crate::{Core, lock};

/// How often [`follow_others`] looks.
const LOOK_EVERY: Duration = Duration::from_secs(1);

/// Where, in the data directory, the lock on each session's run is.
const RUN_LOCKS: &str = "run/sessions";

/// Where, in the data directory, the record of each running stack is (`stacks.rs`).
const STACK_RECORDS: &str = "run/stacks";

/// How many times a session's lock is tried before it counts as held elsewhere, and how long
/// to wait between: another process that looks whether it is held holds it for that moment.
const CLAIM_TRIES: u32 = 3;
const CLAIM_PAUSE: Duration = Duration::from_millis(5);

/// What a session's lock says of a holder that shows notices itself, as the app does.
const SHOWS_NOTICES: &str = "shows notices";

/// What a session's lock says of a holder that follows what others write ([`follow_others`]),
/// and so passes a change of the session's settings stored elsewhere on to its agent.
const FOLLOWS_OTHERS: &str = "follows others";

/// What became of a change of a session's settings, for its agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsReach {
    /// Its agent runs in this process and was told.
    Told,
    /// Its agent runs in another Swarmed that follows what this one writes, as the app does,
    /// which passes the change on to it at its next look, within a second.
    PassedOn,
    /// Its agent is not running, or runs in a Swarmed that does not follow what others write,
    /// such as another command: it takes the change when it next starts.
    NextStart,
}

/// The tables whose rows the tree shows.
const TREE: [&str; 4] = ["projects", "tasks", "sessions", "cloud_tasks"];

/// The events this process stored, which it published itself, and the highest event id dealt
/// with: stored here, or passed on from elsewhere.
#[derive(Debug, Default)]
pub(crate) struct Written {
    mark: i64,
    /// The events stored here above the mark.
    own: BTreeSet<i64>,
}

impl Written {
    /// From the events stored so far, which are history a front end reads for itself.
    pub(crate) fn from_now(conn: &Connection) -> rusqlite::Result<Self> {
        let sql = "SELECT COALESCE(MAX(id), 0) FROM session_events";
        let mark = conn.query_row(sql, [], |row| row.get(0))?;
        Ok(Self {
            mark,
            own: BTreeSet::new(),
        })
    }
}

impl Store {
    /// Notes that this process stored event `id`, which it publishes itself.
    pub(crate) fn wrote(&self, id: i64) {
        self.written.borrow_mut().own.insert(id);
    }

    /// The events another process stored since the last look, oldest first, each with its
    /// session, and the highest event id dealt with now. One statement reads them, so what it
    /// sees is one moment: SQLite gives ids in the order writes are committed, and none lower
    /// than one seen can come later.
    fn events_elsewhere(&self) -> Result<(Vec<(i64, AgentEvent)>, i64)> {
        let mark = self.written.borrow().mark;
        let sql = "SELECT id, session_id, event FROM session_events WHERE id > ?1 ORDER BY id";
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params![mark], |row| {
            Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get::<_, String>(2)?))
        })?;
        let rows = rows.collect::<rusqlite::Result<Vec<(i64, i64, String)>>>()?;
        let mut written = self.written.borrow_mut();
        written.mark = rows.last().map_or(mark, |row| row.0);
        let theirs = rows
            .into_iter()
            .filter(|(id, ..)| !written.own.remove(id))
            .map(|(_, session, text)| (session, event_from_json(&text)))
            .collect();
        // What was stored here and is not there any more went with its session.
        let mark = written.mark;
        written.own.retain(|id| *id > mark);
        Ok((theirs, mark))
    }

    /// A session's events up to event `upto`, oldest first.
    pub(crate) fn events_upto(&self, session_id: i64, upto: i64) -> Result<Vec<AgentEvent>> {
        let sql = "SELECT event FROM session_events WHERE session_id = ?1 AND id <= ?2 \
                   ORDER BY id";
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params![session_id, upto], |row| row.get::<_, String>(0))?;
        let texts = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(texts.iter().map(|text| event_from_json(text)).collect())
    }

    /// What changes whenever another connection commits, and not for this one's own writes.
    fn data_version(&self) -> Result<i64> {
        let version = self
            .conn
            .query_row("PRAGMA data_version", [], |row| row.get(0))?;
        Ok(version)
    }

    /// A fingerprint of every row the tree shows: repositories, tasks, sessions and cloud
    /// tasks, with their names, titles, marks, statuses and order.
    fn tree_print(&self) -> Result<u64> {
        let mut hasher = DefaultHasher::new();
        for table in TREE {
            self.hash_table(table, &mut hasher)?;
        }
        Ok(hasher.finish())
    }

    /// Adds every row of `table` to a fingerprint, in the order of their ids.
    fn hash_table(&self, table: &str, hasher: &mut DefaultHasher) -> Result<()> {
        let sql = format!("SELECT * FROM {table} ORDER BY id");
        let mut statement = self.conn.prepare(&sql)?;
        let columns = statement.column_count();
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            hash_row(row, columns, hasher)?;
        }
        Ok(())
    }
}

/// Adds the first `columns` values of `row` to a fingerprint.
fn hash_row(row: &rusqlite::Row<'_>, columns: usize, hasher: &mut DefaultHasher) -> Result<()> {
    for column in 0..columns {
        hash_value(row.get_ref(column)?, hasher);
    }
    Ok(())
}

/// Adds one value of a row to a fingerprint, its kind first, so that a null and a zero differ.
fn hash_value(value: ValueRef<'_>, hasher: &mut DefaultHasher) {
    match value {
        ValueRef::Null => 0_u8.hash(hasher),
        ValueRef::Integer(number) => (1_u8, number).hash(hasher),
        ValueRef::Real(number) => (2_u8, number.to_bits()).hash(hasher),
        ValueRef::Text(bytes) => (3_u8, bytes).hash(hasher),
        ValueRef::Blob(bytes) => (4_u8, bytes).hash(hasher),
    }
}

/// A file as last seen: when it was changed, and how long it was.
type FileSeen = (PathBuf, Option<SystemTime>, u64);

/// The files in `folder` whose names end in one of `endings`, as they are now.
fn files_in(folder: &Path, endings: &[&str]) -> Vec<FileSeen> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return vec![];
    };
    let named = |path: &Path| {
        let name = path.file_name().map(|name| name.to_string_lossy());
        name.is_some_and(|name| endings.iter().any(|ending| name.ends_with(ending)))
    };
    let seen = |entry: std::fs::DirEntry| {
        let path = entry.path();
        let meta = entry.metadata().ok().filter(std::fs::Metadata::is_file)?;
        named(&path).then(|| (path, meta.modified().ok(), meta.len()))
    };
    entries.flatten().filter_map(seen).collect()
}

/// The configuration files in the data directory and its `projects` folder, as they are now.
fn config_files(data_dir: &Path) -> Vec<FileSeen> {
    let endings = [".toml", ".json", ".css"];
    let mut files = files_in(data_dir, &endings);
    files.extend(files_in(&data_dir.join("projects"), &endings));
    files.sort();
    files
}

/// What was there at the last look.
#[derive(Debug)]
struct Seen {
    /// The database's `data_version`.
    version: i64,
    tree: u64,
    /// The records of running stacks and browsers.
    stacks: Vec<FileSeen>,
    files: Vec<FileSeen>,
}

/// What changed between two looks.
#[derive(Debug, Default)]
struct Changes {
    tree: bool,
    files: bool,
    agents: bool,
}

impl Changes {
    fn between(last: &Seen, now: &Seen, agents_file: &Path) -> Self {
        let agents = |files: &[FileSeen]| files.iter().find(|file| file.0 == agents_file).cloned();
        Self {
            tree: last.tree != now.tree || last.stacks != now.stacks,
            files: last.files != now.files,
            agents: agents(&last.files) != agents(&now.files),
        }
    }
}

/// What this core knows of other processes on the same data.
#[derive(Debug, Default)]
pub(crate) struct Elsewhere {
    /// What was there at the last look; `None` before the first.
    seen: Mutex<Option<Seen>>,
    /// The open permission requests of each session another process runs, as its events said.
    open: Mutex<HashMap<i64, HashSet<String>>>,
    /// The sessions with messages queued here while another process ran their agent: they go
    /// once that run is over.
    pub(crate) waiting: Mutex<HashSet<i64>>,
    /// Held from reading what other processes stored to publishing it, so that two readers,
    /// such as a look and a history being read, publish it in order and once.
    passing: Mutex<()>,
}

impl Elsewhere {
    /// The status and unread state an event another process stored leads to, worked out as
    /// that process did. Whether the agent left work in the background is that process's to
    /// know: the last event of each session in a look takes both from the session's row.
    fn after(&self, session: i64, event: &AgentEvent) -> (Option<SessionStatus>, Option<bool>) {
        let mut open = lock(&self.open);
        let requests = open.entry(session).or_default();
        track(requests, event);
        let status = SessionStatus::after(event, !requests.is_empty());
        if requests.is_empty() {
            open.remove(&session);
        }
        let unread = matches!(event, AgentEvent::UserMessage { .. }).then_some(false);
        (status, unread)
    }
}

/// Publishes what other Swarmed processes on the same data write, looking every second, for as
/// long as the core is there: for a front end that stays, such as the app.
///
/// That this process follows others is said in the locks of the agents it starts from when
/// this is called, not from when the looks begin: an agent started meanwhile, such as one
/// whose Remote Control is resumed as the app starts, would otherwise not say so.
pub fn follow_others(core: Arc<Core>) -> impl Future<Output = ()> + Send + 'static {
    core.live.follows_others.store(true, Ordering::Relaxed);
    async move {
        let mut looks = tokio::time::interval(LOOK_EVERY);
        // Nobody else holds the core once its front end has gone: there is nothing left to
        // tell.
        while Arc::strong_count(&core) > 1 {
            looks.tick().await;
            // A look that failed, such as one that found the database busy for longer than
            // SQLite waits, is made again next time.
            core.notice_others().ok();
        }
    }
}

impl Core {
    /// Publishes what other Swarmed processes on the same data, such as the command line,
    /// wrote since the last look, as if this core had written it: their sessions' events in
    /// order, with status and unread state, never one this core published itself;
    /// [`AgentEvent::TreeChanged`] when repositories, tasks or sessions changed or a stack was
    /// started or stopped; and [`AgentEvent::ConfigChanged`] when a configuration file changed
    /// on disk. A model, effort or supervision stored elsewhere for a session whose agent runs
    /// here is passed on to that agent (`settings_from_elsewhere`). Messages queued for a
    /// session whose agent ran elsewhere are sent once that run is over.
    ///
    /// The first look takes stock of the tree and the files and says nothing of them. Events
    /// stored elsewhere since the core was opened are published from the first. A front end
    /// that stays runs [`follow_others`], which looks every second. Must be called from within
    /// a tokio runtime.
    pub fn notice_others(&self) -> Result<()> {
        self.house_if_free();
        let passing = lock(&self.elsewhere.passing);
        let (events, _) = self.store().events_elsewhere()?;
        // The events are passed on whatever this look finds: they are past the mark already,
        // and would not be read again.
        let changes = self.look_again();
        if changes.as_ref().is_ok_and(|changes| changes.tree) {
            self.live.publish_apart(AgentEvent::TreeChanged);
        }
        self.pass_on(events);
        drop(passing);
        let changes = changes?;
        if changes.tree {
            self.settings_from_elsewhere();
        }
        if changes.agents {
            // What could not be read is said in the problem banner, which reads it again.
            self.reload_agents().ok();
        }
        if changes.files {
            let kind = "file".to_string();
            self.live.publish_apart(AgentEvent::ConfigChanged { kind });
        }
        self.send_what_waited();
        if lock(&self.house).is_some() {
            self.background_settle_gone()?;
        }
        Ok(())
    }

    /// A session's events, oldest first, once what other processes stored has been passed on:
    /// everything returned has been published, so a front end that reads it and then adds what
    /// it hears shows nothing twice. What arrives after is left out, and published later.
    pub(crate) fn events_noticed(&self, id: i64) -> Result<Vec<AgentEvent>> {
        let passing = lock(&self.elsewhere.passing);
        let (events, upto) = self.store().events_elsewhere()?;
        self.pass_on(events);
        drop(passing);
        self.store().events_upto(id, upto)
    }

    /// Takes the house if the Swarmed that kept it has gone, for a front end that stays: the
    /// first to arrive shows the notices of what others run and stops every stack when it
    /// quits, and with two apps open, the one left over does that once the other has quit.
    ///
    /// It also settles what the one that went left busy, as arriving first would have: that
    /// one, opened again, finds the house kept and settles nothing, and its sessions would
    /// count as working for good. The sessions this process runs are passed over.
    fn house_if_free(&self) {
        if !self.live.shows_notices.load(Ordering::Relaxed) || lock(&self.house).is_some() {
            return;
        }
        // One that cannot be taken now is tried again at the next look.
        if !self.keeps_house().unwrap_or(false) {
            return;
        }
        self.finish_removals();
        if self.sessions_settle().is_ok_and(|settled| settled > 0) {
            self.live.publish_apart(AgentEvent::TreeChanged);
        }
    }

    /// Publishes events another process stored, in order. The last of each session's carries
    /// the status and unread state its row has now, and so does an answer handed back, which
    /// is unread or not by what that process knew of work left in the background. A report of
    /// something left running, or the end of an agent, carries what the session has running
    /// in the background, worked out again here and kept on its row: a Swarmed that does not
    /// know that count stores the reports all the same.
    fn pass_on(&self, events: Vec<(i64, AgentEvent)>) {
        let last: HashMap<i64, usize> = events
            .iter()
            .enumerate()
            .map(|(at, (session, _))| (*session, at))
            .collect();
        let mut rows = HashMap::new();
        let read = |id: i64| self.session(id).ok().map(|row| (row.status, row.unread));
        let mut row = |id: i64| *rows.entry(id).or_insert_with(|| read(id));
        for (at, (session_id, event)) in events.into_iter().enumerate() {
            let (status, unread) = self.elsewhere.after(session_id, &event);
            let is_last = last.get(&session_id) == Some(&at);
            let stands = (is_last || leaves_something_to_read(&event))
                .then(|| row(session_id))
                .flatten();
            let status = match stands {
                Some((now, _)) if is_last => Some(now),
                _ => status,
            };
            let unread = stands.map_or(unread, |(_, unread_now)| Some(unread_now));
            let background = self.store().background_after(session_id, &event);
            let published = SessionEvent {
                session_id,
                status,
                unread,
                background: background.ok().flatten(),
                event,
                elsewhere: true,
            };
            self.live.pass_on(&self.store, published);
        }
    }

    /// What changed since the last look; nothing at the first.
    fn look_again(&self) -> Result<Changes> {
        let version = self.store().data_version()?;
        let mut seen = lock(&self.elsewhere.seen);
        // The rows are read again only after another process committed.
        let tree = match seen.as_ref() {
            Some(last) if last.version == version => last.tree,
            _ => self.store().tree_print()?,
        };
        let data = &self.paths.data_dir;
        let now = Seen {
            version,
            tree,
            stacks: [
                files_in(&data.join(STACK_RECORDS), &[".pid"]),
                files_in(&crate::browser::records(data), &[".pid"]),
            ]
            .concat(),
            files: config_files(data),
        };
        let agents = self.paths.agents_config();
        let changes = seen.as_ref().map_or_else(Changes::default, |last| {
            Changes::between(last, &now, &agents)
        });
        *seen = Some(now);
        Ok(changes)
    }

    /// Passes on to each agent this process runs the settings another process stored for its
    /// session since the agent knew them, as `swarmed session set` stores them: as a change
    /// made here is passed on, a model from its next turn and an effort once its turn is over
    /// ([`Core::settings_told`]). What the change records, such as a change of supervision,
    /// that process recorded, and it reaches front ends with its other events.
    fn settings_from_elsewhere(&self) {
        let running = self.running_sessions().into_iter();
        for session in running.filter_map(|id| self.session(id).ok()) {
            // Its settings are read again where they are told, not taken from this row: a
            // change made here since it was read would otherwise be told back to what it was.
            self.settings_told(session.id, &session.agent, session.status);
        }
    }

    /// What became of a change of session `id`'s settings just stored, for its agent: told
    /// here, passed on by another Swarmed that runs it and follows what this one writes, or
    /// taken when the agent next starts.
    pub fn session_settings_reach(&self, id: i64) -> Result<SettingsReach> {
        self.session(id)?;
        if self.live.is_running(id) {
            return Ok(SettingsReach::Told);
        }
        let path = run_lock(&self.paths, id);
        let follows = std::fs::read_to_string(&path)
            .is_ok_and(|said| said.lines().any(|line| line == FOLLOWS_OTHERS));
        if follows && self.runs_elsewhere(id) {
            return Ok(SettingsReach::PassedOn);
        }
        Ok(SettingsReach::NextStart)
    }

    /// Sends what was queued for sessions whose agent another process ran, once that run is
    /// over. A queue left standing here for another reason, such as a turn that failed, is
    /// not this one's to send.
    fn send_what_waited(&self) {
        let over: Vec<i64> = {
            let waiting = lock(&self.elsewhere.waiting);
            let over = waiting.iter().copied();
            over.filter(|id| !self.runs_elsewhere(*id)).collect()
        };
        for id in over {
            lock(&self.elsewhere.waiting).remove(&id);
            let sends = self.live.queues.is_waiting(id) && !self.live.is_running(id);
            // A send that fails leaves the message in the queue, where the user sees it.
            sends.then(|| self.session_resume_queue(id, Auth::default()).ok());
        }
    }

    /// Notes that a message was queued for session `id`: if another process runs its agent,
    /// the message goes once that run is over.
    pub(crate) fn queued_for(&self, id: i64) {
        if self.runs_elsewhere(id) {
            lock(&self.elsewhere.waiting).insert(id);
        }
    }

    /// Deletes the lock on session `id`'s run, for a session that is going: nothing is to
    /// start it again. Not while another process holds it: a third would make a new one, lock
    /// it and start the session beside that process's agent.
    pub(crate) fn forget_run_lock(&self, id: i64) {
        if !self.runs_elsewhere(id) {
            std::fs::remove_file(run_lock(&self.paths, id)).ok();
        }
    }

    /// Refuses, with the first of them, if another process runs the agent of any of `ids`.
    pub(crate) fn none_run_elsewhere(&self, ids: &[i64]) -> Result<()> {
        match ids.iter().find(|id| self.runs_elsewhere(**id)) {
            Some(id) => Err(Error::RunsElsewhere(*id)),
            None => Ok(()),
        }
    }

    /// Whether another Swarmed process runs session `id`'s agent now.
    pub(crate) fn runs_elsewhere(&self, id: i64) -> bool {
        if lock(&self.live.runs_held).contains_key(&id) {
            return false;
        }
        held_elsewhere(&run_lock(&self.paths, id))
    }

    /// `error`, unless another Swarmed process runs session `id`'s agent, which says more.
    pub(crate) fn or_elsewhere(&self, id: i64, error: Error) -> Error {
        if self.runs_elsewhere(id) {
            Error::RunsElsewhere(id)
        } else {
            error
        }
    }

    /// The sessions whose agent another Swarmed process runs now.
    pub(crate) fn sessions_run_elsewhere(&self) -> Vec<i64> {
        let locks = files_in(&self.paths.data_dir.join(RUN_LOCKS), &[".lock"]);
        let id = |(path, ..): FileSeen| path.file_stem()?.to_str()?.parse::<i64>().ok();
        let ids = locks.into_iter().filter_map(id);
        ids.filter(|id| self.runs_elsewhere(*id)).collect()
    }

    /// Whether this process shows the notices of session `id`, whose event another process
    /// stored. Only the Swarmed that arrived first does, so that two windows do not both
    /// interrupt the user; and not while the agent runs in a Swarmed that shows its own.
    pub(crate) fn shows_notices_of(&self, id: i64) -> bool {
        if lock(&self.house).is_none() {
            return false;
        }
        let path = run_lock(&self.paths, id);
        let shown_there = std::fs::read_to_string(&path)
            .is_ok_and(|said| said.lines().any(|line| line == SHOWS_NOTICES));
        !(shown_there && self.runs_elsewhere(id))
    }
}

impl Drop for Core {
    /// A core that goes lets go of the sessions it ran and of the house, as its process would
    /// by ending: a core opened on the same data after it, as a Swarmed started again, may run
    /// them and keep house. What of its agents is still on its way out is no longer any core's.
    fn drop(&mut self) {
        let mut held = lock(&self.live.runs_held);
        let ids: Vec<i64> = held.keys().copied().collect();
        for id in ids {
            let_go_run(&mut held, id);
        }
        // Unlocked before it is closed, as in `let_go_run`.
        if let Some(house) = lock(&self.house).take() {
            house.unlock().ok();
        }
    }
}

/// Where the lock on session `id`'s run is.
fn run_lock(paths: &Paths, id: i64) -> PathBuf {
    paths.data_dir.join(RUN_LOCKS).join(format!("{id}.lock"))
}

/// Whether another holds the lock at `path`. Looking takes it for a moment, shared, if nobody
/// does.
pub(crate) fn held_elsewhere(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    match file.try_lock_shared() {
        Ok(()) => {
            // Unlocked before it is closed, as in `let_go_run`.
            file.unlock().ok();
            false
        }
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(_)) => false,
    }
}

/// Lets go of the lock on session `id`'s run, if `held` has it. It is unlocked before it is
/// closed: a process another thread starts meanwhile holds every open file of this one until
/// it has become the program it was started as, and closing alone would leave the lock held
/// for that long.
pub(crate) fn let_go_run(held: &mut HashMap<i64, File>, id: i64) {
    if let Some(file) = held.remove(&id) {
        file.unlock().ok();
    }
}

/// Takes the lock that says session `id`'s agent runs in this process, which `live` holds
/// until the agent has gone. Refused while another process holds it. Taking it again changes
/// nothing.
pub(crate) fn claim_run(live: &Live, paths: &Paths, id: i64) -> Result<()> {
    let mut held = lock(&live.runs_held);
    if held.contains_key(&id) {
        return Ok(());
    }
    let path = run_lock(paths, id);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)?;
    if !taken(&file)? {
        return Err(Error::RunsElsewhere(id));
    }
    // Who holds it, for whoever reads the file, whether that one shows notices itself, and
    // whether it passes on a change of settings made elsewhere.
    let said_if = |flag: &AtomicBool, words: &'static str| {
        if flag.load(Ordering::Relaxed) {
            words
        } else {
            ""
        }
    };
    let shows = said_if(&live.shows_notices, SHOWS_NOTICES);
    let follows = said_if(&live.follows_others, FOLLOWS_OTHERS);
    let said = format!("{}\n{shows}\n{follows}\n", std::process::id());
    // Nothing depends on the words but notices, which are shown twice at worst, and what a
    // command that changed a session's settings says became of the change.
    file.set_len(0)
        .and_then(|()| file.write_all(said.as_bytes()))
        .ok();
    held.insert(id, file);
    Ok(())
}

/// Locks `file` for this process alone, trying again for a moment. Whether it was locked.
pub(crate) fn taken(file: &File) -> Result<bool> {
    for attempt in 1..=CLAIM_TRIES {
        match file.try_lock() {
            Ok(()) => return Ok(true),
            Err(TryLockError::WouldBlock) if attempt < CLAIM_TRIES => {
                std::thread::sleep(CLAIM_PAUSE);
            }
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_configuration_files_are_watched() {
        let dir = tempfile::tempdir().expect("test setup");
        let projects = dir.path().join("projects");
        std::fs::create_dir(&projects).expect("test setup");
        for name in [
            "settings.toml",
            "layout.json",
            "style.css",
            "swarmed.db",
            "notes.txt",
        ] {
            std::fs::write(dir.path().join(name), "").expect("test setup");
        }
        std::fs::write(projects.join("demo.actions.toml"), "").expect("test setup");
        std::fs::create_dir(dir.path().join("folder.toml")).expect("test setup");
        let names: Vec<String> = config_files(dir.path())
            .into_iter()
            .map(|(path, ..)| path.file_name().expect("named").to_string_lossy().into())
            .collect();
        // By their whole paths: `projects/` sorts between the two.
        assert_eq!(
            names,
            [
                "layout.json",
                "demo.actions.toml",
                "settings.toml",
                "style.css"
            ]
        );
    }

    #[test]
    fn a_change_to_agents_toml_is_told_from_a_change_to_another_file() {
        let file = |name: &str, len: u64| (PathBuf::from(name), None, len);
        let seen = |files: Vec<FileSeen>| Seen {
            version: 1,
            tree: 7,
            stacks: vec![],
            files,
        };
        let before = seen(vec![file("agents.toml", 1), file("settings.toml", 1)]);
        let settings = seen(vec![file("agents.toml", 1), file("settings.toml", 2)]);
        let agents = seen(vec![file("agents.toml", 2), file("settings.toml", 1)]);
        let path = Path::new("agents.toml");
        let changed = Changes::between(&before, &settings, path);
        assert!(changed.files && !changed.agents && !changed.tree);
        let changed = Changes::between(&before, &agents, path);
        assert!(changed.files && changed.agents);
    }

    #[test]
    fn a_lock_held_by_one_holder_is_refused_to_another_until_let_go() {
        let dir = tempfile::tempdir().expect("test setup");
        let paths = Paths {
            data_dir: dir.path().to_path_buf(),
            worktrees_root: dir.path().join("worktrees"),
            claude_config: dir.path().join("claude.json"),
            codex_config: dir.path().join("codex.toml"),
            opencode_config_dir: dir.path().join("opencode"),
        };
        let (here, there) = (Live::default(), Live::default());
        claim_run(&here, &paths, 4).expect("taken");
        claim_run(&here, &paths, 4).expect("taken again");
        assert!(held_elsewhere(&run_lock(&paths, 4)));
        let refused = claim_run(&there, &paths, 4);
        assert!(
            matches!(refused, Err(Error::RunsElsewhere(4))),
            "{refused:?}"
        );
        let_go_run(&mut lock(&here.runs_held), 4);
        assert!(!held_elsewhere(&run_lock(&paths, 4)));
        claim_run(&there, &paths, 4).expect("taken once let go");
    }
}
