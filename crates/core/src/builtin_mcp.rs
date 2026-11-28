//! Swarmed's built-in MCP servers: repolore, off until the user turns it on.
//!
//! repolore searches the user's own history on GitHub. Set up the usual way, it is registered
//! in each agent separately, and every setup runs a shared server that holds a large embedding
//! model. As a built-in, Swarmed installs it, keeps its home in `<data dir>/repolore`, and gives
//! it to every session itself, as one of the servers set up here (`mcp_own.rs`). Everything
//! else is configured in repolore's own page, which Swarmed opens. What is asked of repolore,
//! and how, is in `repolore.rs`.
//!
//! **One repolore at a time.** Two would hold two models, so the built-in is given to sessions
//! only while repolore is registered nowhere else and the old home's shared server is not in
//! use: stopped, and no session connected to it, since each would start it again. A row is in
//! one of five states, worked out by [`state_of`]:
//!
//! - `missing`: `repolore` or `repolore-mcp` is not installed;
//! - `outdated`: its version is older than `repolore::MINIMUM`, or it does not say;
//! - `off`: not turned on;
//! - `waiting`: turned on, but a registration is left or the old server is still in use;
//! - `on`: turned on, and nothing stands in its way.
//!
//! Registrations come from repolore's doctor, which knows every client repolore registers
//! with, and from Swarmed's own readers of Claude Code, its plugins, Codex, OpenCode and
//! `mcp.toml`. repolore says how each is taken out as text (`removal`): a command in
//! backticks for Claude Code, a step elsewhere. Swarmed runs a command only for one of
//! repolore's doctor, found again by its key, and only Claude Code's or Codex's own that
//! belongs to no folder (`remover`): a folder's would run in none. For Codex's own, of which
//! repolore says to edit its file, Swarmed runs `codex mcp remove <name>`. The command is
//! shown before it runs. Anything else is a step for the user, as repolore says it. Nothing
//! is written into an agent's files, and a `repolore session-start` hook in Claude Code's
//! settings is pointed out, never edited.
//!
//! **Whether it stands clear** is stored (`builtin_mcp` in the database, beside whether it is
//! turned on), so that a session starts without asking repolore anything: no Python runs
//! then. Every reading that got its answers stores it; one that got none, or an answer
//! without what it is asked for, leaves it as it was and says why in `problem`. Turning on
//! or off forgets it, so that what a reading found while it was off gives no session the
//! built-in. At a session's start the gate is cheap: turned on, clear, `repolore-mcp` there,
//! and no server like repolore in Swarmed's own readers. A registration that comes back in
//! one of those stops the built-in at once; one in another client, at the next reading.
//! Nothing reads on a timer. A repository's own repolore, in its `.mcp.json`, is no
//! registration of the user's: once the user approved it in Claude Code, a session there gets
//! that one and not the built-in (`repolore_use`), and the row stays as it is. Unapproved, the
//! branch's file alone changes nothing.
//!
//! **Held sessions** are the sessions running here whose agent got a repolore other than the
//! built-in (`RepoloreUse`, kept with the run in `sessions.rs`): they keep the old server
//! running until they end, and the idle ones can be stopped to start again on their next
//! message. A session that starts with the built-in has repolore refresh its index if one is
//! due (`session-start`), which takes the place of the hook users add to Claude Code. What
//! Claude Code said of the built-in (`status`) is read from such a session alone, the newest
//! running here: another's `repolore` may be the old one.
//!
//! **The page**, `repolore ui`, is started by the Swarmed that keeps the house alone
//! (`presence.rs`), one at a time, and ended as that one leaves or the built-in is turned off,
//! even while it starts: it is kept from when its process starts. Nothing is held while it
//! says where it is, and a page dropped is ended. Its
//! address carries a token: it stays in memory, never in a view, an error or a log. The old
//! setup's folders can be moved to the Trash once the built-in is on and every check of a
//! check made then passed, as a step the user takes; never a folder that is, holds or is
//! inside Swarmed's data directory.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::AgentEvent;
use crate::agent_setup::find_program;
use crate::agents::CLAUDE;
use crate::error::{Error, Result};
use crate::hooks::{self, OutputSink};
use crate::mcp::{self, McpServer, McpTransport};
use crate::mcp_own::quoted;
use crate::repolore::{
    self, Doctor, FirstLine, Home, MANAGED_ENV, MANAGER, MINIMUM, Page, Registration, Repolore,
    SERVER_PROGRAM, ServerStatus,
};
use crate::sessions::SessionStatus;
use crate::store::{Store, Task};
use crate::{Core, lock, mcp_codex, mcp_opencode, mcp_plugins};

/// The built-in repolore's name: what its tools appear under, and what operations take.
pub const REPOLORE: &str = "repolore";

/// Swarmed's built-in servers, by name.
const BUILTINS: &[&str] = &[REPOLORE];

/// How far along a built-in server is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinState {
    /// Not installed.
    Missing,
    /// Installed, but older than Swarmed needs, or not saying which version it is.
    Outdated,
    /// Installed and not turned on.
    Off,
    /// Turned on, but not given to sessions yet: something else is in its way.
    Waiting,
    /// Turned on and given to every session.
    On,
}

impl BuiltinState {
    /// The word for it, as it is serialised.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Outdated => "outdated",
            Self::Off => "off",
            Self::Waiting => "waiting",
            Self::On => "on",
        }
    }
}

/// How thoroughly a built-in was last checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    /// Without the network: what settings show when they open.
    Offline,
    /// With the network too, from the Check button only.
    Full,
}

impl CheckKind {
    /// The word for it, as it is serialised.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Full => "full",
        }
    }
}

/// A built-in server as a front end shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuiltinMcpView {
    /// Its name, which its tools appear under.
    pub name: String,
    /// How far along it is.
    pub state: BuiltinState,
    /// The version installed, if it said.
    pub version: Option<String>,
    /// The oldest version Swarmed takes.
    pub minimum: String,
    /// What installs or upgrades it, as the user would type it.
    pub install_command: String,
    /// Its home in Swarmed's data directory.
    pub home: String,
    /// Whether that home is set up: its configuration file is there, or it has an index.
    pub home_ready: bool,
    /// The user's own setup from before, if one was found: what turning on can copy.
    pub existing: Option<ExistingSetup>,
    /// Where else repolore is registered: what keeps the built-in waiting.
    pub registrations: Vec<BuiltinRegistration>,
    /// The old setup's shared server, while it is in use: see [`OldServer`].
    pub old_server: Option<OldServer>,
    /// The sessions running here on a repolore other than the built-in.
    pub sessions: Vec<HeldSession>,
    /// Whether Claude Code's settings have a `repolore session-start` hook, which the built-in
    /// makes unnecessary. Pointed out, never edited.
    pub hook: bool,
    /// What its last check found.
    pub checks: Vec<DoctorCheck>,
    /// How that check was made, if one was. Its index and last refresh are among the checks.
    pub checked: Option<CheckKind>,
    /// What Claude Code last said of the server `repolore`, such as `connected`.
    pub status: Option<String>,
    /// Whether its page runs.
    pub page_open: bool,
    /// What could not be found out, said for the user.
    pub problem: Option<String>,
}

/// The user's own repolore setup from before the built-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExistingSetup {
    /// The folders it is kept in: where its configuration file is, and its data folder.
    pub folders: Vec<String>,
    /// Whether it has an index, which copying keeps: its index check passed.
    pub indexed: bool,
}

/// One place repolore is registered besides the built-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuiltinRegistration {
    /// What it is taken out by: [`Core::builtin_mcp_remove_registration`].
    pub key: String,
    /// The client it is registered with, to show.
    pub client: String,
    /// Where in that client: `user`, `local`, `project`, `plugin` and the like.
    pub scope: String,
    /// The command Swarmed would take it out with, to show before it runs, or to run by hand
    /// where Swarmed does not find its program. None for one only the user takes out.
    pub command: Option<String>,
    /// Whether Swarmed runs that command itself: there is one, and its program is here.
    pub runs_here: bool,
    /// What the user does to take it out, as repolore says it, when there is no command.
    pub step: Option<String>,
}

/// The old setup's shared server, while it is in use: running, starting, or with a session
/// connected, which would start it again. The field names are kept for the front ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OldServer {
    /// Its process: none while it is stopped, or starting and not saying yet.
    pub pid: Option<u32>,
    /// How many sessions are connected to it (repolore's `sessions`), counted while it is
    /// stopped too.
    pub relays: u32,
}

/// A session running here on a repolore other than the built-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HeldSession {
    /// The session.
    pub id: i64,
    /// Its title.
    pub title: String,
    /// Whether its agent is between turns, so that stopping it loses nothing.
    pub idle: bool,
}

/// One check of repolore's doctor. Every field is read leniently, so that a check is never
/// lost for a field that is odd: a failing one lost would read as a check that passed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoctorCheck {
    /// What was checked.
    #[serde(deserialize_with = "repolore::lenient")]
    pub name: String,
    /// Whether it passed.
    #[serde(deserialize_with = "repolore::lenient")]
    pub ok: bool,
    /// Whether what it found is worth a warning, though it passed: repolore writes a warning
    /// with `ok` true.
    #[serde(deserialize_with = "repolore::lenient")]
    pub warning: bool,
    /// What it found.
    #[serde(deserialize_with = "repolore::lenient")]
    pub detail: String,
    /// repolore's fix for it, if it has one.
    #[serde(deserialize_with = "repolore::lenient")]
    pub fix: Option<String>,
}

impl DoctorCheck {
    /// Whether it did not pass. A warning passed: it stops nothing.
    fn failed(&self) -> bool {
        !self.ok
    }
}

/// repolore's page while this Swarmed runs it, from when its process is started, before it
/// says where it is, so that turning off and leaving end a page still starting too; and
/// whether Swarmed is leaving, after which none is started: one would outlive it.
#[derive(Debug, Default)]
pub(crate) struct PageSlot {
    /// The page, with which start it is: a start that finds another number there when the
    /// page has said where it is was ended meanwhile.
    page: Option<(u64, Page)>,
    /// How many pages were started.
    started: u64,
    leaving: bool,
}

/// What opening the page found, with the page held.
#[derive(Debug)]
enum Opening {
    /// A page that runs, at this address.
    Running(String),
    /// A page still starting, which another open started: that one opens the browser.
    Starting,
    /// A page just started, by its number, with its first line to come.
    Started(u64, FirstLine),
}

/// Which repolore a session's agent was started with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RepoloreUse {
    /// The built-in.
    Builtin,
    /// One registered elsewhere, which keeps the old server running.
    Elsewhere,
    /// None.
    #[default]
    None,
}

/// Whether `server` is a repolore: one by that name, or one whose command has a word that is
/// the program `repolore-mcp`, wherever it is.
pub(crate) fn repolore_like(server: &McpServer) -> bool {
    if server.name == REPOLORE {
        return true;
    }
    let McpTransport::Stdio { command, args, .. } = &server.transport else {
        return false;
    };
    let program = |word: &String| {
        Path::new(word)
            .file_name()
            .is_some_and(|n| n == SERVER_PROGRAM)
    };
    std::iter::once(command).chain(args).any(program)
}

/// Whether `server` is the built-in repolore, as [`Core::builtin_repolore`] makes it.
fn is_builtin(server: &McpServer) -> bool {
    let McpTransport::Stdio { env, .. } = &server.transport else {
        return false;
    };
    server.name == REPOLORE && env.get(MANAGED_ENV).is_some_and(|by| by == MANAGER)
}

/// Whether a built-in is installed, and in a version Swarmed takes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Installed {
    /// One of its programs is not there.
    #[default]
    No,
    /// Older than Swarmed takes, or not saying which it is.
    Outdated,
    Current,
}

/// What a built-in's state is worked out from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Facts {
    installed: Installed,
    turned_on: bool,
    /// Nothing stands in its way, as the last reading with answers said.
    clear: bool,
}

/// A built-in's state, the first of these that holds: not installed, too old, not turned on,
/// something in its way; or else on.
fn state_of(facts: Facts) -> BuiltinState {
    match facts {
        Facts {
            installed: Installed::No,
            ..
        } => BuiltinState::Missing,
        Facts {
            installed: Installed::Outdated,
            ..
        } => BuiltinState::Outdated,
        Facts {
            turned_on: false, ..
        } => BuiltinState::Off,
        Facts { clear: false, .. } => BuiltinState::Waiting,
        Facts { .. } => BuiltinState::On,
    }
}

/// What one reading of repolore got: each answer, or why there was none.
#[derive(Debug, Default)]
struct Reading {
    version: Option<String>,
    /// Whether that version is one Swarmed takes.
    current: bool,
    /// The doctor of the built-in home.
    builtin: Option<Doctor>,
    /// The doctor of the old home, when it was asked.
    old: Option<Doctor>,
    /// The old home's shared server.
    server: Option<ServerStatus>,
    problems: Vec<String>,
}

impl Reading {
    /// An answer, or nothing with why kept among the problems.
    fn answer<T>(&mut self, answer: std::result::Result<T, String>) -> Option<T> {
        answer.map_err(|why| self.problems.push(why)).ok()
    }

    /// Where repolore says it is registered.
    fn registrations(&self) -> &[Registration] {
        self.builtin
            .as_ref()
            .map_or(&[], |doctor| doctor.registrations.as_slice())
    }

    /// Puts what was found into `view`.
    fn fill(self, view: &mut BuiltinMcpView, full: bool) {
        view.version = self.version;
        view.problem = (!self.problems.is_empty()).then(|| self.problems.join(" "));
        view.old_server = self.server.as_ref().and_then(old_server_of);
        view.existing = self.old.as_ref().and_then(existing_of);
        let Some(doctor) = self.builtin else {
            return;
        };
        view.home_ready = doctor.set_up();
        view.checks = doctor.checks;
        view.checked = Some(if full {
            CheckKind::Full
        } else {
            CheckKind::Offline
        });
    }
}

/// The old server as a front end shows it, while it is in use: its process only while it
/// runs, and its sessions as `relays`.
fn old_server_of(server: &ServerStatus) -> Option<OldServer> {
    server.in_use().then(|| OldServer {
        pid: server.pid.filter(|_| server.running),
        relays: server.sessions,
    })
}

/// Whether nothing stands in the built-in's way, if this reading can tell: an older
/// repolore, a registration or the old server in use says no; none, and the old server not
/// in use, says yes. `None` when it got too few answers to tell, which leaves the last word
/// as it was.
fn clear_from(reading: &Reading, registrations: usize) -> Option<bool> {
    if reading.version.is_some() && !reading.current {
        return Some(false);
    }
    if registrations > 0 {
        return Some(false);
    }
    reading.builtin.as_ref()?;
    let server = reading.server.as_ref()?;
    Some(!server.in_use())
}

/// The user's setup from before, from what the old home's doctor says and its files: none if
/// it was never set up, or its folders are not there.
fn existing_of(doctor: &Doctor) -> Option<ExistingSetup> {
    if !doctor.set_up() {
        return None;
    }
    let indexed = doctor.indexed();
    let folders = doctor.paths.folders().into_iter();
    let folders: Vec<String> = folders
        .filter(|folder| folder.is_dir())
        .map(|folder| folder.to_string_lossy().into_owned())
        .collect();
    (!folders.is_empty()).then_some(ExistingSetup { folders, indexed })
}

/// A server like repolore in an agent's configuration that Swarmed reads itself.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    /// The client's id, as repolore's doctor names it.
    client: &'static str,
    scope: String,
    name: String,
    /// What the user does to take it out.
    step: String,
}

impl Found {
    /// `server`, found in `place` of `client`'s configuration, if it is a repolore.
    fn of(client: &'static str, scope: &str, server: &McpServer, place: &str) -> Option<Self> {
        repolore_like(server).then(|| Self {
            client,
            scope: scope.to_string(),
            name: server.name.clone(),
            step: format!("Remove `{}` from {place}.", server.name),
        })
    }
}

/// The keys registrations are known by: client, scope and name, with a number from the second
/// of the same on.
#[derive(Debug, Default)]
struct Keys(BTreeMap<String, usize>);

impl Keys {
    fn next(&mut self, client: &str, scope: &str, name: &str) -> String {
        let key = format!("{client}:{scope}:{name}");
        let seen = self.0.entry(key.clone()).or_default();
        *seen += 1;
        if *seen == 1 {
            key
        } else {
            format!("{key}:{seen}")
        }
    }
}

/// Each registration repolore's doctor lists, with its key.
fn keyed(said: &[Registration]) -> Vec<(String, &Registration)> {
    let mut keys = Keys::default();
    said.iter()
        .map(|said| (keys.next(&said.client, &said.scope, &said.name), said))
        .collect()
}

/// The client's name, to show.
fn client_name(client: &str) -> &str {
    match client {
        CLAUDE => "Claude Code",
        mcp_codex::ORIGIN => "Codex",
        mcp_opencode::ORIGIN => "OpenCode",
        "swarmed" => "Swarmed",
        other => other,
    }
}

/// The command Swarmed takes out a registration repolore's doctor lists with, if it runs
/// one: for Codex's user scope, of which repolore says to delete the entry from its file,
/// `codex mcp remove <name>`; else the command repolore names in backticks, if it is one
/// Swarmed runs (`remover`). Codex's other scopes belong to a folder, and are the user's step,
/// as is anything repolore says that is no command.
fn removal(said: &Registration) -> Option<Vec<String>> {
    let argv = match (said.client.as_str(), said.scope.as_str()) {
        (mcp_codex::ORIGIN, "user") => codex_removal(&said.name)?,
        (mcp_codex::ORIGIN, _) => return None,
        _ => said.remove_command()?,
    };
    remover(&argv).is_some().then_some(argv)
}

/// Codex's command that takes out its server `name`, unless the name would read as an option.
fn codex_removal(name: &str) -> Option<Vec<String>> {
    let plain = !name.trim().is_empty() && !name.starts_with('-');
    plain.then(|| ["codex", "mcp", "remove", name].map(String::from).to_vec())
}

/// The program a registration's remove command `argv` starts with, if Swarmed runs it itself:
/// Claude Code's `mcp remove` of the user's own servers, its `plugin uninstall`, and Codex's
/// `mcp remove`. Claude Code's other scopes, or none named, belong to a folder, and the
/// command would run in none: they are the user's step, as is any other command.
fn remover(argv: &[String]) -> Option<&str> {
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    let scope = scope_of(words.get(1..).unwrap_or_default());
    let runs = match words.as_slice() {
        ["claude", "mcp", "remove", _, ..] => scope == Some("user"),
        ["claude", "plugin", "uninstall", _, ..] => scope.is_none_or(|scope| scope == "user"),
        ["codex", "mcp", "remove", _, ..] => true,
        _ => false,
    };
    argv.first().map(String::as_str).filter(|_| runs)
}

/// The scope a command's arguments name: `-s X`, `--scope X` or `--scope=X`.
fn scope_of<'a>(args: &[&'a str]) -> Option<&'a str> {
    let named = |pair: &&[&str]| matches!(pair.first(), Some(&("-s" | "--scope")));
    let after = args
        .windows(2)
        .find(named)
        .and_then(|pair| pair.get(1).copied());
    after.or_else(|| args.iter().find_map(|arg| arg.strip_prefix("--scope=")))
}

/// A command's words on one line, to show.
fn shown_line(argv: &[String]) -> String {
    let words: Vec<String> = argv.iter().map(|word| quoted(word)).collect();
    words.join(" ")
}

/// One registration repolore's doctor lists, as a front end shows it: with the command Swarmed
/// would run, or else repolore's step. `runs_here` says whether a program is on this machine.
fn registration_of(
    said: &Registration,
    key: String,
    runs_here: &dyn Fn(&str) -> bool,
) -> BuiltinRegistration {
    let argv = removal(said);
    let here = argv.as_deref().and_then(remover).is_some_and(runs_here);
    let client = if said.client_name.trim().is_empty() {
        client_name(&said.client).to_string()
    } else {
        said.client_name.clone()
    };
    let step = match said.remove.trim() {
        "" => format!("Remove `{}` from {client} yourself.", said.name),
        text => text.to_string(),
    };
    BuiltinRegistration {
        key,
        client,
        scope: said.scope.clone(),
        command: argv.as_deref().map(shown_line),
        runs_here: here,
        step: argv.is_none().then_some(step),
    }
}

/// Every registration, as a front end shows it: repolore's, then those Swarmed's readers
/// found with a client repolore listed none for.
fn registrations_of(
    said: &[Registration],
    found: &[Found],
    runs_here: &dyn Fn(&str) -> bool,
) -> Vec<BuiltinRegistration> {
    let shown = keyed(said).into_iter();
    let mut shown: Vec<BuiltinRegistration> = shown
        .map(|(key, said)| registration_of(said, key, runs_here))
        .collect();
    let covered: BTreeSet<&str> = said.iter().map(|said| said.client.as_str()).collect();
    let mut keys = Keys::default();
    for found in found.iter().filter(|found| !covered.contains(found.client)) {
        shown.push(BuiltinRegistration {
            key: keys.next(found.client, &found.scope, &found.name),
            client: client_name(found.client).to_string(),
            scope: found.scope.clone(),
            command: None,
            runs_here: false,
            step: Some(found.step.clone()),
        });
    }
    shown
}

/// A command for `sh -c` that runs `program` with `args`, each a word of its own whatever is
/// in it: every word is handed over as a variable, never written into the command.
fn argv_for_shell(program: &Path, args: &[String]) -> (String, Vec<(String, String)>) {
    let words = std::iter::once(program.to_string_lossy().into_owned()).chain(args.iter().cloned());
    let named = |(at, word)| (format!("SWARMED_WORD_{at}"), word);
    let env: Vec<(String, String)> = words.enumerate().map(named).collect();
    let quoted: Vec<String> = env.iter().map(|(name, _)| format!("\"${name}\"")).collect();
    (quoted.join(" "), env)
}

/// The arguments `uv` installs or upgrades repolore with.
fn install_args() -> String {
    format!("--managed-python --python 3.12 --upgrade 'repolore-mcp>={MINIMUM}'")
}

/// Whether a folder is empty or not there at all.
fn empty_or_missing(folder: &Path) -> bool {
    std::fs::read_dir(folder).map_or(true, |mut entries| entries.next().is_none())
}

/// Whether Claude Code's settings beside its file `claude_config` run `repolore session-start`
/// as a hook.
fn repolore_hook(claude_config: &Path) -> bool {
    let Some(home) = claude_config.parent() else {
        return false;
    };
    let path = home.join(".claude").join("settings.json");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let settings: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    settings.get("hooks").is_some_and(starts_a_session)
}

/// Whether anything in `value` is a hook that runs `repolore session-start`.
fn starts_a_session(value: &Value) -> bool {
    match value {
        Value::Object(hook) if hook.contains_key("command") => runs_session_start(hook),
        Value::Array(items) => items.iter().any(starts_a_session),
        Value::Object(entries) => entries.values().any(starts_a_session),
        _ => false,
    }
}

/// Whether a hook's command, with its arguments when it gives them apart, runs the program
/// `repolore` with `session-start`.
fn runs_session_start(hook: &serde_json::Map<String, Value>) -> bool {
    let command = hook
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args = hook
        .get("args")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    let args = args.filter_map(Value::as_str);
    let words: Vec<&str> = command.split_whitespace().chain(args).collect();
    let program = |word: &&str| {
        Path::new(word)
            .file_name()
            .is_some_and(|name| name == REPOLORE)
    };
    words.iter().any(program) && words.contains(&"session-start")
}

/// The old setup's folders, if they may go to the Trash now: once the built-in is on, its check
/// passed and the old server is no longer in use.
fn trashable(view: &BuiltinMcpView) -> Result<Vec<PathBuf>> {
    let refuse = |why: &str| {
        Err(Error::Mcp(format!(
            "the old setup stays where it is: {why}"
        )))
    };
    if view.state != BuiltinState::On {
        return refuse("only once the built-in repolore is on");
    }
    if let Some(problem) = &view.problem {
        return refuse(&format!("not every answer came: {problem}"));
    }
    let failed = view.checks.is_empty() || view.checks.iter().any(DoctorCheck::failed);
    if view.checked.is_none() || failed {
        return refuse("only once its check passes");
    }
    if view.old_server.is_some() {
        return refuse("its server still runs, or a session would start it again");
    }
    let Some(existing) = &view.existing else {
        return refuse("no old setup was found");
    };
    Ok(existing.folders.iter().map(PathBuf::from).collect())
}

/// `folder`, followed to where it really is, if it may go to the Trash: never one that is,
/// holds or is inside Swarmed's data directory, nor one that holds the user's home, nor one
/// whose name does not say it is repolore's.
fn may_trash(folder: &Path, data_dir: &Path, user_home: Option<&Path>) -> Result<PathBuf> {
    let refuse = |why: &str| Error::Mcp(format!("{} stays where it is: {why}", folder.display()));
    let real = folder
        .canonicalize()
        .map_err(|_| refuse("it is not there"))?;
    let followed = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let data = followed(data_dir);
    if real.starts_with(&data) || data.starts_with(&real) {
        return Err(refuse("it is Swarmed's own data, or holds it"));
    }
    if user_home.is_some_and(|home| followed(home).starts_with(&real)) {
        return Err(refuse("it holds your home folder"));
    }
    let name = real
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase());
    if !name.is_some_and(|name| name.contains(REPOLORE)) {
        return Err(refuse("its name does not say it is repolore's"));
    }
    Ok(real)
}

fn trash_failed(folder: &Path, why: &str) -> Error {
    let folder = folder.display();
    Error::Mcp(format!("{folder} could not be moved to the Trash: {why}"))
}

/// Moves `folder` to the Trash, as the Finder does.
#[cfg(target_os = "macos")]
fn to_trash(folder: &Path) -> Result<()> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let path = NSString::from_str(&folder.to_string_lossy());
    let url = NSURL::fileURLWithPath(&path);
    let manager = NSFileManager::defaultManager();
    let moved = manager.trashItemAtURL_resultingItemURL_error(&url, None);
    moved.map_err(|error| trash_failed(folder, &error.localizedDescription().to_string()))
}

/// Moves `folder` to the Trash with `gio`, where the desktop has it; else leaves it.
#[cfg(not(target_os = "macos"))]
fn to_trash(folder: &Path) -> Result<()> {
    let moved = std::process::Command::new("gio")
        .args(["trash", "--"])
        .arg(folder)
        .env("PATH", crate::shellenv::path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match moved {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(trash_failed(
            folder,
            &format!("`gio trash` ended with {status}"),
        )),
        Err(_) => Err(trash_failed(
            folder,
            "nothing here moves it to the Trash (`gio` was not found): delete it yourself",
        )),
    }
}

/// The built-in called `name`, or an error naming those there are.
fn builtin(name: &str) -> Result<&'static str> {
    let found = BUILTINS.iter().find(|known| **known == name).copied();
    found.ok_or_else(|| {
        let known = BUILTINS.join(", ");
        Error::Mcp(format!(
            "Swarmed has no built-in server called `{name}`: it has {known}"
        ))
    })
}

impl Core {
    /// Every built-in server, each with how it stands now: read without the network.
    pub async fn builtin_mcp_servers(&self) -> Result<Vec<BuiltinMcpView>> {
        let mut views = Vec::new();
        for name in BUILTINS {
            views.push(self.builtin_view(name, false).await?);
        }
        Ok(views)
    }

    /// Checks the built-in `name` again: with `full`, the network too (GitHub, and whether
    /// Claude Code connects), which only the user asks for.
    pub async fn builtin_mcp_check(&self, name: &str, full: bool) -> Result<BuiltinMcpView> {
        self.builtin_view(builtin(name)?, full).await
    }

    /// Installs the built-in `name`, or upgrades it, with `uv`, first installing `uv` with
    /// Homebrew where it is missing. What the installer prints goes to `sink`.
    pub async fn builtin_mcp_install(
        &self,
        name: &str,
        sink: OutputSink<'_>,
    ) -> Result<BuiltinMcpView> {
        let name = builtin(name)?;
        let failed = |reason: String| Error::Install {
            agent: name.to_string(),
            reason,
        };
        let (command, env) = self.install_run().map_err(failed)?;
        let ran = hooks::run("install", &command, &std::env::temp_dir(), &env, sink).await;
        ran.map_err(|error| failed(error.to_string()))?;
        if !self.repolore_installed() {
            let reason = "the installer finished, but `repolore` is nowhere to be found";
            return Err(failed(reason.to_string()));
        }
        self.builtin_view(name, false).await
    }

    /// Turns the built-in `name` on, from a copy of the user's own setup with `copy`, else
    /// fresh, with an empty home to set up on its page. A home set up already is kept as it
    /// is. A copy that fails changes nothing, and the error says repolore's reason.
    pub async fn builtin_mcp_turn_on(&self, name: &str, copy: bool) -> Result<BuiltinMcpView> {
        let name = builtin(name)?;
        let repolore = self.repolore_current().await?;
        let home = self.paths.repolore_home();
        let doctor = repolore.doctor(Home::Builtin(&home), false).await;
        if !doctor.is_ok_and(|doctor| doctor.set_up()) {
            home_made(&repolore, &home, copy).await?;
        }
        self.store().builtin_mcp_turn(name, true)?;
        self.builtin_view(name, false).await
    }

    /// Turns the built-in `name` off and ends its page. Its home stays, for turning it on again.
    pub async fn builtin_mcp_turn_off(&self, name: &str) -> Result<BuiltinMcpView> {
        let name = builtin(name)?;
        self.store().builtin_mcp_turn(name, false)?;
        self.repolore_page_end();
        self.builtin_view(name, false).await
    }

    /// Takes out repolore's registration `key` with its client's own remove command, which
    /// goes to `sink` as it runs. The command is never one a front end hands over: it is made
    /// from what repolore's doctor lists under that key now, and only if it is one of Claude
    /// Code's or Codex's own that belongs to no folder (`removal`). Any other is the user's
    /// step, and the error says it.
    pub async fn builtin_mcp_remove_registration(
        &self,
        name: &str,
        key: &str,
        sink: OutputSink<'_>,
    ) -> Result<BuiltinMcpView> {
        let name = builtin(name)?;
        let repolore = self.repolore_current().await?;
        let home = self.paths.repolore_home();
        let doctor = repolore.doctor(Home::Builtin(&home), false).await;
        let doctor = doctor.map_err(Error::Mcp)?;
        let keyed = keyed(&doctor.registrations);
        let Some((_, said)) = keyed.into_iter().find(|(known, _)| known == key) else {
            return self.not_listed(name, key).await;
        };
        let argv = removal(said);
        let program = argv.as_deref().and_then(remover);
        let program = program.and_then(|program| find_program(program, &self.tool_dirs));
        let (Some(argv), Some(program)) = (argv.as_deref(), program) else {
            return Err(yours(&registration_of(said, key.to_string(), &|_| false)));
        };
        let (command, env) = argv_for_shell(&program, argv.get(1..).unwrap_or_default());
        let ran = hooks::run("remove", &command, &std::env::temp_dir(), &env, sink).await;
        ran.map_err(|error| {
            let line = shown_line(argv);
            Error::Mcp(format!(
                "`{line}` did not work ({error}): run it yourself, then check again"
            ))
        })?;
        self.builtin_view(name, false).await
    }

    /// For a key repolore no longer lists: the view, if it is gone; the user's step, if it is
    /// one only Swarmed's own readers found.
    async fn not_listed(&self, name: &str, key: &str) -> Result<BuiltinMcpView> {
        let view = self.builtin_view(name, false).await?;
        match view.registrations.iter().find(|left| left.key == key) {
            Some(left) => Err(yours(left)),
            None => Ok(view),
        }
    }

    /// Stops the agents of the idle sessions that keep the old server running. Each starts
    /// again, on the built-in if it is given by then, with its next message.
    pub async fn builtin_mcp_restart_sessions(&self, name: &str) -> Result<BuiltinMcpView> {
        let name = builtin(name)?;
        for held in self.held_sessions().into_iter().filter(|held| held.idle) {
            self.session_stop(held.id);
        }
        self.builtin_view(name, false).await
    }

    /// Opens the built-in's own page in the user's browser, starting it if it does not run.
    ///
    /// Only the Swarmed that keeps the house starts it, so that one runs, and it ends as that
    /// one leaves, even while it starts. Opened again while it starts, nothing more is done:
    /// the open that started it opens the browser. With the standard library's processes
    /// alone: a window's command that is not `async` runs where no Tokio runtime is. The
    /// address is never logged or said.
    pub fn builtin_mcp_open(&self, name: &str) -> Result<()> {
        let name = builtin(name)?;
        if !self.keeps_house()? {
            return Err(Error::Mcp(
                "repolore's page is opened by the Swarmed that started first on this data: \
                 open it there"
                    .to_string(),
            ));
        }
        let (number, first) = match self.page_opening(name)? {
            Opening::Running(address) => return show(&address),
            Opening::Starting => return Ok(()),
            Opening::Started(number, first) => (number, first),
        };
        // Waited for with nothing held: it can take a while, and every view asks whether a
        // page runs, as do turning it off and quitting, which end this one meanwhile.
        let address = self.page_ready(number, first.wait())?;
        show(&address)
    }

    /// With the page held: the one that runs or starts already, or else a new one, started
    /// and kept as starting. Whether the built-in is on is read with the page held, so that
    /// turning it off, which ends the page after it writes, ends this one or is seen here.
    fn page_opening(&self, name: &str) -> Result<Opening> {
        let mut slot = lock(&self.repolore_page);
        if !self.store().builtin_mcp_row(name)?.0 {
            return Err(Error::Mcp(format!("turn {name} on first")));
        }
        if slot.leaving {
            let why = "Swarmed is quitting";
            return Err(Error::Mcp(format!("repolore's page was not opened: {why}")));
        }
        if slot.page.as_mut().is_some_and(|(_, page)| page.alive()) {
            let address = slot.page.as_ref().and_then(|(_, page)| page.address());
            let address = address.map(String::from);
            return Ok(address.map_or(Opening::Starting, Opening::Running));
        }
        let repolore = Repolore::find(&self.tool_dirs)
            .ok_or_else(|| Error::Mcp(format!("{name} is not installed")))?;
        let home = self.paths.repolore_home();
        let (page, first) = Page::spawn(&repolore, &home).map_err(Error::Mcp)?;
        slot.started += 1;
        // One that has gone is dropped here, which sends it nothing.
        slot.page = Some((slot.started, page));
        Ok(Opening::Started(slot.started, first))
    }

    /// Takes the first line of start `number`, `said`, as its page's address, and says it: if
    /// that page is still the one kept, and said an address. One that said none is ended; one
    /// no longer kept was ended meanwhile, by turning off or leaving.
    fn page_ready(&self, number: u64, said: Option<String>) -> Result<String> {
        let mut slot = lock(&self.repolore_page);
        let Some((_, page)) = slot.page.as_mut().filter(|(kept, _)| *kept == number) else {
            let why = "it was turned off, or Swarmed is quitting";
            return Err(Error::Mcp(format!("repolore's page was not opened: {why}")));
        };
        let ready = page.ready(said);
        if ready.is_err() {
            slot.page = None;
        }
        ready.map_err(Error::Mcp)
    }

    /// Moves the user's setup from before to the Trash: only once the built-in is on, passes
    /// a check made now, and the old server is no longer in use. Every folder is checked
    /// before any is moved.
    pub async fn builtin_mcp_trash_old(&self, name: &str) -> Result<BuiltinMcpView> {
        let name = builtin(name)?;
        let view = self.builtin_view(name, false).await?;
        let home = std::env::home_dir();
        let data = &self.paths.data_dir;
        let folders = trashable(&view)?.into_iter();
        let checked = folders.map(|folder| may_trash(&folder, data, home.as_deref()));
        for folder in checked.collect::<Result<Vec<PathBuf>>>()? {
            to_trash(&folder)?;
        }
        self.builtin_view(name, false).await
    }

    /// The built-in repolore, as sessions are given it, unless something stands in its way:
    /// not turned on, not clear at the last reading, `repolore-mcp` missing, or a server like
    /// repolore among those set up here (`own`) or in an agent's configuration. Asks repolore
    /// nothing.
    pub(crate) fn builtin_repolore(&self, own: &[McpServer]) -> Result<Option<McpServer>> {
        let (turned_on, clear) = self.store().builtin_mcp_row(REPOLORE)?;
        if !(turned_on && clear) {
            return Ok(None);
        }
        let Some(program) = find_program(SERVER_PROGRAM, &self.tool_dirs) else {
            return Ok(None);
        };
        if own.iter().any(repolore_like) || !self.found_in_readers(&[]).is_empty() {
            return Ok(None);
        }
        let home = self.paths.repolore_home();
        Ok(Some(McpServer {
            name: REPOLORE.to_string(),
            transport: McpTransport::Stdio {
                command: program.to_string_lossy().into_owned(),
                args: Vec::new(),
                env: repolore::builtin_env(&home),
            },
        }))
    }

    /// Which repolore the agent of a session on `agent` in `task` starts with, given the
    /// other agents' servers `shared` and the ones set up here, `own`: the built-in, one of its
    /// own or another's, or none. A repolore of the agent's own, a repository's `.mcp.json`
    /// one among them, or another agent's goes before the built-in, which is then taken out
    /// of `own`: never two at once. The gate (`builtin_repolore`) reads no repository's file.
    pub(crate) fn repolore_use(
        &self,
        agent: &str,
        task: &Task,
        shared: &[McpServer],
        own: &mut Vec<McpServer>,
    ) -> RepoloreUse {
        let other = |server: &McpServer| repolore_like(server) && !is_builtin(server);
        // Its own configuration is read only when what it is given does not tell already.
        let has = || {
            self.agent_own_servers(agent, task)
                .iter()
                .any(repolore_like)
        };
        if shared.iter().chain(own.iter()).any(other) || has() {
            own.retain(|server| !is_builtin(server));
            return RepoloreUse::Elsewhere;
        }
        if own.iter().any(is_builtin) {
            return RepoloreUse::Builtin;
        }
        RepoloreUse::None
    }

    /// Has repolore refresh the built-in's index if one is due, without waiting: for a
    /// session that started with the built-in.
    pub(crate) fn repolore_refresh(&self) {
        if let Some(repolore) = Repolore::find(&self.tool_dirs) {
            repolore.session_start(&self.paths.repolore_home());
        }
    }

    /// Ends repolore's page, if this Swarmed started it, even one still starting: dropped, it
    /// is ended.
    pub(crate) fn repolore_page_end(&self) {
        let page = lock(&self.repolore_page).page.take();
        drop(page);
    }

    /// Ends repolore's page as Swarmed leaves, even one still starting, and has none started
    /// from now on.
    pub(crate) fn repolore_page_leave(&self) {
        let mut slot = lock(&self.repolore_page);
        slot.leaving = true;
        drop(slot.page.take());
    }

    /// How the built-in `name` stands: repolore asked, without the network unless `full`,
    /// with whether it stands clear stored when the answers say.
    async fn builtin_view(&self, name: &str, full: bool) -> Result<BuiltinMcpView> {
        let (turned_on, stored) = self.store().builtin_mcp_row(name)?;
        let mut view = self.view_without_answers(name);
        let found = Repolore::find(&self.tool_dirs).filter(|_| self.repolore_installed());
        let Some(repolore) = found else {
            view.state = state_of(Facts::default());
            return Ok(view);
        };
        let reading = self.reading(&repolore, turned_on, full).await;
        let found = self.found_in_readers(&self.mcp_set_up_here()?);
        let runs_here = |program: &str| find_program(program, &self.tool_dirs).is_some();
        let registrations = registrations_of(reading.registrations(), &found, &runs_here);
        let answered = clear_from(&reading, registrations.len());
        if let Some(clear) = answered.filter(|clear| *clear != stored) {
            self.store().builtin_mcp_clear(name, clear)?;
        }
        let installed = if reading.current {
            Installed::Current
        } else {
            Installed::Outdated
        };
        view.state = state_of(Facts {
            installed,
            turned_on,
            clear: answered.unwrap_or(stored),
        });
        view.registrations = registrations;
        reading.fill(&mut view, full);
        Ok(view)
    }

    /// What is known of the built-in `name` without asking repolore.
    fn view_without_answers(&self, name: &str) -> BuiltinMcpView {
        let status = self.builtin_status(name);
        BuiltinMcpView {
            name: name.to_string(),
            state: BuiltinState::Missing,
            version: None,
            minimum: MINIMUM.to_string(),
            install_command: self.install_shown(),
            home: self.paths.repolore_home().to_string_lossy().into_owned(),
            home_ready: false,
            existing: None,
            registrations: Vec::new(),
            old_server: None,
            sessions: self.held_sessions(),
            hook: repolore_hook(&self.paths.claude_config),
            checks: Vec::new(),
            checked: None,
            status,
            page_open: self.repolore_page_open(),
            problem: None,
        }
    }

    /// What repolore says: its version, then, if it is one Swarmed takes, the doctor of the
    /// built-in home, the old home's server and, where copying or the Trash could be offered,
    /// the old home's doctor, all at once.
    async fn reading(&self, repolore: &Repolore, turned_on: bool, full: bool) -> Reading {
        let mut reading = Reading::default();
        match repolore.version().await {
            Ok(version) => {
                reading.current = repolore::current(&version);
                reading.version = Some(version);
            }
            Err(why) => reading.problems.push(why),
        }
        if !reading.current {
            return reading;
        }
        let home = self.paths.repolore_home();
        let old_wanted = turned_on || empty_or_missing(&home);
        let (builtin, server, old) = tokio::join!(
            repolore.doctor(Home::Builtin(&home), full),
            repolore.server_status(Home::Old),
            old_doctor(repolore, old_wanted)
        );
        reading.builtin = reading.answer(builtin);
        reading.server = reading.answer(server);
        reading.old = old.and_then(|old| reading.answer(old));
        reading
    }

    /// Whether both of repolore's programs are there.
    fn repolore_installed(&self) -> bool {
        let found = |program| find_program(program, &self.tool_dirs).is_some();
        found(repolore::PROGRAM) && found(SERVER_PROGRAM)
    }

    /// repolore, if it is there and of a version Swarmed takes; else why not.
    async fn repolore_current(&self) -> Result<Repolore> {
        let missing = || Error::Mcp(format!("{REPOLORE} is not installed: install it first"));
        let repolore = Repolore::find(&self.tool_dirs).ok_or_else(missing)?;
        let version = repolore.version().await.map_err(Error::Mcp)?;
        if !repolore::current(&version) {
            return Err(Error::Mcp(format!(
                "{REPOLORE} {version} is older than {MINIMUM}: upgrade it first"
            )));
        }
        Ok(repolore)
    }

    /// The install command as the user would type it.
    fn install_shown(&self) -> String {
        let uv = find_program("uv", &self.tool_dirs);
        let first = if uv.is_some() {
            ""
        } else {
            "brew install uv && "
        };
        format!("{first}uv tool install {}", install_args())
    }

    /// The install command as the shell runs it, with the programs' whole paths, which a
    /// desktop app's `PATH` may lack, handed over as variables.
    fn install_run(&self) -> std::result::Result<(String, Vec<(String, String)>), String> {
        let install = format!("\"$SWARMED_UV\" tool install {}", install_args());
        let path = |program: PathBuf| program.to_string_lossy().into_owned();
        if let Some(uv) = find_program("uv", &self.tool_dirs) {
            return Ok((install, vec![("SWARMED_UV".to_string(), path(uv))]));
        }
        let Some(brew) = find_program("brew", &self.tool_dirs) else {
            return Err(
                "uv was not found, nor Homebrew to install it with: install uv \
                        (https://docs.astral.sh/uv/), then come back"
                    .to_string(),
            );
        };
        // Homebrew puts what it installs beside itself.
        let uv = brew.with_file_name("uv");
        let env = vec![
            ("SWARMED_BREW".to_string(), path(brew)),
            ("SWARMED_UV".to_string(), path(uv)),
        ];
        Ok((format!("\"$SWARMED_BREW\" install uv && {install}"), env))
    }

    /// The servers like repolore in the agents' configurations Swarmed reads, and in `own`,
    /// the servers set up here.
    fn found_in_readers(&self, own: &[McpServer]) -> Vec<Found> {
        let paths = &self.paths;
        let mut found = Vec::new();
        for (folder, server) in mcp::claude_code_servers_anywhere(&paths.claude_config) {
            let place = folder.as_ref().map_or_else(
                || "Claude Code's own servers".to_string(),
                |folder| format!("Claude Code's servers for {folder}"),
            );
            let scope = folder.as_ref().map_or("user", |_| "local");
            found.extend(Found::of(CLAUDE, scope, &server, &place));
        }
        let plugins = paths
            .claude_config
            .parent()
            .map(|home| home.join(".claude"));
        let plugins = plugins.map(|home| mcp_plugins::plugin_servers(&home));
        for server in plugins.into_iter().flatten() {
            found.extend(Found::of(
                CLAUDE,
                "plugin",
                &server,
                "Claude Code's plugins",
            ));
        }
        let place = "Codex's config.toml";
        for server in mcp_codex::codex_servers(&paths.codex_config) {
            found.extend(Found::of(mcp_codex::ORIGIN, "user", &server, place));
        }
        let place = "OpenCode's configuration";
        for server in mcp_opencode::opencode_servers(&paths.opencode_config_dir) {
            found.extend(Found::of(mcp_opencode::ORIGIN, "user", &server, place));
        }
        let place = "the MCP servers set up in Swarmed";
        found.extend(
            own.iter()
                .filter_map(|server| Found::of("swarmed", "own", server, place)),
        );
        found
    }

    /// The servers `agent` has in its own configuration, which it starts by itself.
    fn agent_own_servers(&self, agent: &str, task: &Task) -> Vec<McpServer> {
        let paths = &self.paths;
        match agent {
            CLAUDE => {
                let project = self.store().project_by_id(task.project_id).ok().flatten();
                let repo = project.and_then(|project| project.repo().ok().map(Path::to_path_buf));
                let repo = repo.unwrap_or_else(|| task.worktree_path.clone());
                let worktree = &task.worktree_path;
                // Of the repository's `.mcp.json`, only what the user approved in Claude Code:
                // a branch's file alone does not take the built-in away.
                let config = &paths.claude_config;
                let mut own = mcp::claude_code_servers_to_share(config, &repo, worktree);
                let home = paths
                    .claude_config
                    .parent()
                    .map(|home| home.join(".claude"));
                own.extend(home.map_or_default(|home| mcp_plugins::plugin_servers(&home)));
                own
            }
            mcp_codex::ORIGIN => mcp_codex::codex_servers(&paths.codex_config),
            mcp_opencode::ORIGIN => mcp_opencode::opencode_servers(&paths.opencode_config_dir),
            _ => Vec::new(),
        }
    }

    /// What Claude Code said of the built-in `name` as the newest session running here that
    /// was given it started; none if none did. Never another session's, whose `repolore` may
    /// be a registration of its own.
    fn builtin_status(&self, name: &str) -> Option<String> {
        let given = self.live.running_on(RepoloreUse::Builtin);
        given.iter().rev().find_map(|id| self.status_in(*id, name))
    }

    /// What session `id`'s agent said of the server `name` as it started, if it did.
    fn status_in(&self, id: i64, name: &str) -> Option<String> {
        let init = self.store().latest_event(id, "init").ok().flatten();
        let Some(AgentEvent::Init { mcp_servers, .. }) = init else {
            return None;
        };
        let server = mcp_servers.into_iter().find(|server| server.name == name);
        server.map(|server| server.status)
    }

    /// The sessions running here on a repolore other than the built-in.
    fn held_sessions(&self) -> Vec<HeldSession> {
        let ids = self.live.running_on(RepoloreUse::Elsewhere);
        let sessions = ids.into_iter().filter_map(|id| self.session(id).ok());
        let held = |session: crate::Session| HeldSession {
            id: session.id,
            title: session.title,
            idle: session.status == SessionStatus::Idle,
        };
        sessions.map(held).collect()
    }

    /// Whether repolore's page runs, started by this Swarmed, and has said where it is.
    fn repolore_page_open(&self) -> bool {
        let mut slot = lock(&self.repolore_page);
        let open = |(_, page): &mut (u64, Page)| page.alive() && page.address().is_some();
        slot.page.as_mut().is_some_and(open)
    }
}

/// Makes the built-in's `home`: a copy of the user's own setup with `copy`, else an empty
/// folder. A copy that fails leaves nothing behind (repolore cleans up), and says why.
async fn home_made(repolore: &Repolore, home: &Path, copy: bool) -> Result<()> {
    if !copy {
        return Ok(std::fs::create_dir_all(home)?);
    }
    repolore.copy(home).await.map_err(|why| {
        Error::Mcp(format!(
            "your setup could not be copied: {why}. Nothing was changed: start fresh instead, \
             or try again later"
        ))
    })
}

/// The old home's doctor, if it is `wanted`: only where copying or the Trash could be offered.
async fn old_doctor(
    repolore: &Repolore,
    wanted: bool,
) -> Option<std::result::Result<Doctor, String>> {
    if !wanted {
        return None;
    }
    Some(repolore.doctor(Home::Old, false).await)
}

/// Opens repolore's page at `address` in the user's browser. Whatever goes wrong is said
/// without the address, which carries the page's token.
fn show(address: &str) -> Result<()> {
    crate::open::in_browser(address).map_err(|_| Error::OpenFailed {
        what: "repolore's page".to_string(),
        reason: "the browser could not be started".to_string(),
    })
}

/// The error for a registration Swarmed does not take out itself, with the user's step: the
/// command to run by hand where its program is not found here.
fn yours(registration: &BuiltinRegistration) -> Error {
    let step = match (&registration.step, &registration.command) {
        (Some(step), _) => step.clone(),
        (None, Some(command)) => format!("run `{command}` yourself"),
        (None, None) => String::new(),
    };
    Error::Mcp(format!("Swarmed does not take this one out itself: {step}"))
}

impl Store {
    /// Whether the built-in `name` is turned on, and whether the last reading with answers
    /// found it clear.
    pub(crate) fn builtin_mcp_row(&self, name: &str) -> Result<(bool, bool)> {
        let sql = "SELECT turned_on, clear FROM builtin_mcp WHERE name = ?1";
        let pair = |row: &rusqlite::Row<'_>| Ok((row.get(0)?, row.get(1)?));
        let row = self.conn.query_row(sql, params![name], pair).optional()?;
        Ok(row.unwrap_or((false, false)))
    }

    /// Turns the built-in `name` on or off. Either way it is no longer known to be clear: what
    /// a reading found while it was off is old by now, so the reading that follows says again,
    /// and until then no session is given it.
    fn builtin_mcp_turn(&self, name: &str, on: bool) -> Result<()> {
        let sql = "INSERT INTO builtin_mcp (name, turned_on) VALUES (?1, ?2)
                   ON CONFLICT (name) DO UPDATE SET turned_on = ?2, clear = 0";
        self.conn.execute(sql, params![name, on])?;
        Ok(())
    }

    /// Keeps whether the built-in `name` stands clear, as a reading with answers found.
    fn builtin_mcp_clear(&self, name: &str, clear: bool) -> Result<()> {
        let sql = "INSERT INTO builtin_mcp (name, clear) VALUES (?1, ?2)
                   ON CONFLICT (name) DO UPDATE SET clear = ?2";
        self.conn.execute(sql, params![name, clear])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio(name: &str, command: &str, args: &[&str]) -> McpServer {
        McpServer {
            name: name.to_string(),
            transport: McpTransport::Stdio {
                command: command.to_string(),
                args: args.iter().map(ToString::to_string).collect(),
                env: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn a_state_is_the_first_thing_that_holds() {
        let facts = |installed, turned_on, clear| Facts {
            installed,
            turned_on,
            clear,
        };
        let current = Installed::Current;
        assert_eq!(
            state_of(facts(Installed::No, true, true)),
            BuiltinState::Missing
        );
        assert_eq!(
            state_of(facts(Installed::Outdated, true, true)),
            BuiltinState::Outdated
        );
        assert_eq!(state_of(facts(current, false, true)), BuiltinState::Off);
        assert_eq!(state_of(facts(current, true, false)), BuiltinState::Waiting);
        assert_eq!(state_of(facts(current, true, true)), BuiltinState::On);
        let shown = serde_json::to_string(&BuiltinState::Waiting).expect("serialises");
        assert_eq!(shown, "\"waiting\"");
    }

    /// A view with something in every field.
    fn sample_view() -> BuiltinMcpView {
        BuiltinMcpView {
            name: REPOLORE.into(),
            state: BuiltinState::Waiting,
            version: Some("0.5.1".into()),
            minimum: MINIMUM.into(),
            install_command: "uv tool install".into(),
            home: "/data/repolore".into(),
            home_ready: true,
            existing: Some(ExistingSetup {
                folders: vec!["/old/repolore".into()],
                indexed: true,
            }),
            registrations: vec![BuiltinRegistration {
                key: "claude:user:repolore".into(),
                client: "Claude Code".into(),
                scope: "user".into(),
                command: Some("claude mcp remove repolore -s user".into()),
                runs_here: true,
                step: None,
            }],
            old_server: Some(OldServer {
                pid: Some(42),
                relays: 1,
            }),
            sessions: vec![HeldSession {
                id: 7,
                title: "Fix it".into(),
                idle: true,
            }],
            hook: true,
            checks: vec![DoctorCheck {
                name: "model".into(),
                ok: true,
                warning: true,
                detail: "slow".into(),
                fix: None,
            }],
            checked: Some(CheckKind::Offline),
            status: Some("connected".into()),
            page_open: false,
            problem: None,
        }
    }

    #[test]
    fn a_view_is_serialised_in_the_shape_front_ends_read() {
        let shown = serde_json::to_value(sample_view()).expect("serialises");
        let expected = serde_json::json!({
            "name": "repolore", "state": "waiting", "version": "0.5.1", "minimum": "0.5.1",
            "install_command": "uv tool install", "home": "/data/repolore", "home_ready": true,
            "existing": {"folders": ["/old/repolore"], "indexed": true},
            "registrations": [{"key": "claude:user:repolore", "client": "Claude Code",
                "scope": "user", "command": "claude mcp remove repolore -s user",
                "runs_here": true, "step": null}],
            "old_server": {"pid": 42, "relays": 1},
            "sessions": [{"id": 7, "title": "Fix it", "idle": true}],
            "hook": true,
            "checks": [{"name": "model", "ok": true, "warning": true, "detail": "slow",
                "fix": null}],
            "checked": "offline", "status": "connected", "page_open": false, "problem": null,
        });
        assert_eq!(shown, expected);
    }

    #[test]
    fn the_old_setup_goes_only_once_every_check_is_ok_and_its_server_stopped() {
        let on = || BuiltinMcpView {
            state: BuiltinState::On,
            old_server: None,
            ..sample_view()
        };
        // A warning is a check that passed.
        let folders = trashable(&on()).expect("may go");
        assert_eq!(folders, [PathBuf::from("/old/repolore")]);
        let refused = |view: BuiltinMcpView| trashable(&view).map_err(|e| e.to_string());
        let unchecked_list = BuiltinMcpView {
            checks: Vec::new(),
            ..on()
        };
        assert!(refused(unchecked_list).is_err_and(|e| e.contains("check passes")));
        let mut failing = on();
        failing.checks[0].ok = false;
        assert!(refused(failing).is_err_and(|e| e.contains("check passes")));
        let unchecked = BuiltinMcpView {
            checked: None,
            ..on()
        };
        assert!(refused(unchecked).is_err_and(|e| e.contains("check passes")));
        let problem = BuiltinMcpView {
            problem: Some("no answer".into()),
            ..on()
        };
        assert!(refused(problem).is_err_and(|e| e.contains("not every answer came: no answer")));
        let waiting = BuiltinMcpView {
            state: BuiltinState::Waiting,
            ..on()
        };
        assert!(refused(waiting).is_err_and(|e| e.contains("is on")));
        let running = BuiltinMcpView {
            old_server: Some(OldServer {
                pid: None,
                relays: 0,
            }),
            ..on()
        };
        assert!(refused(running).is_err_and(|e| e.contains("still runs")));
        let none = BuiltinMcpView {
            existing: None,
            ..on()
        };
        assert!(refused(none).is_err_and(|e| e.contains("no old setup")));
    }

    #[test]
    fn a_reading_says_clear_only_with_every_answer() {
        let answered = |running| Reading {
            version: Some("0.5.1".into()),
            current: true,
            builtin: Some(Doctor::default()),
            server: Some(ServerStatus {
                running,
                ..ServerStatus::default()
            }),
            ..Reading::default()
        };
        assert_eq!(clear_from(&answered(false), 0), Some(true));
        assert_eq!(clear_from(&answered(true), 0), Some(false));
        assert_eq!(clear_from(&answered(false), 1), Some(false));
        // Stopped, but starting again, or with a session that would start it.
        let mut starting = answered(true);
        starting.server = Some(ServerStatus {
            running: true,
            starting: true,
            ..ServerStatus::default()
        });
        assert_eq!(clear_from(&starting, 0), Some(false));
        let mut connected = answered(false);
        connected.server = Some(ServerStatus {
            sessions: 1,
            ..ServerStatus::default()
        });
        assert_eq!(clear_from(&connected, 0), Some(false));
        // No server answer: nothing to say, unless a registration was found anyway.
        let mut silent = answered(false);
        silent.server = None;
        assert_eq!(clear_from(&silent, 0), None);
        assert_eq!(clear_from(&silent, 2), Some(false));
        // An older repolore is never clear; one that did not answer leaves it.
        let old = Reading {
            version: Some("0.5.0".into()),
            ..Reading::default()
        };
        assert_eq!(clear_from(&old, 0), Some(false));
        assert_eq!(clear_from(&Reading::default(), 0), None);
    }

    #[test]
    fn the_old_server_is_shown_while_it_is_in_use_with_its_sessions() {
        let status = |running, starting, pid, sessions| ServerStatus {
            running,
            starting,
            pid,
            sessions,
        };
        assert_eq!(old_server_of(&status(false, false, None, 0)), None);
        assert_eq!(
            old_server_of(&status(true, false, Some(42), 2)),
            Some(OldServer {
                pid: Some(42),
                relays: 2
            })
        );
        assert_eq!(
            old_server_of(&status(true, true, None, 0)),
            Some(OldServer {
                pid: None,
                relays: 0
            })
        );
        // Stopped with a session connected: in use, with no process.
        assert_eq!(
            old_server_of(&status(false, false, Some(42), 1)),
            Some(OldServer {
                pid: None,
                relays: 1
            })
        );
    }

    #[test]
    fn a_repolore_is_known_by_its_name_or_its_program() {
        assert!(repolore_like(&stdio("repolore", "anything", &[])));
        assert!(repolore_like(&stdio(
            "lore",
            "/Users/me/.local/bin/repolore-mcp",
            &[]
        )));
        assert!(repolore_like(&stdio("lore", "uvx", &["repolore-mcp"])));
        assert!(!repolore_like(&stdio(
            "lore",
            "uvx",
            &["repolore-mcp-extra"]
        )));
        assert!(!repolore_like(&stdio("docs", "repolore", &["serve"])));
        let http = McpServer {
            name: "remote".into(),
            transport: McpTransport::Http {
                url: "https://x/repolore-mcp".into(),
                headers: BTreeMap::new(),
            },
        };
        assert!(!repolore_like(&http));
        let mut builtin = stdio(REPOLORE, "/bin/repolore-mcp", &[]);
        assert!(!is_builtin(&builtin));
        if let McpTransport::Stdio { env, .. } = &mut builtin.transport {
            *env = repolore::builtin_env(Path::new("/data/repolore"));
        }
        assert!(is_builtin(&builtin));
    }

    fn registration(client: &str, scope: &str, remove: &str) -> Registration {
        Registration {
            client: client.into(),
            client_name: String::new(),
            name: REPOLORE.into(),
            scope: scope.into(),
            remove: remove.into(),
        }
    }

    #[test]
    fn registrations_are_keyed_and_only_claude_and_codex_are_run_here() {
        let said = [
            registration(
                "claude",
                "user",
                "`claude mcp remove repolore --scope user`",
            ),
            registration(
                "claude",
                "local",
                "`claude mcp remove repolore --scope local`",
            ),
            registration(
                "claude",
                "local",
                "`claude mcp remove repolore --scope=local`",
            ),
            registration(
                "claude",
                "plugin",
                "`claude plugin uninstall repolore@repolore --scope user`",
            ),
            registration(
                "codex",
                "user",
                "delete [mcp_servers.repolore] from /Users/me/.codex/config.toml",
            ),
            registration(
                "opencode",
                "user",
                "delete repolore from /Users/me/.config/opencode/opencode.json",
            ),
            registration("cursor", "user", "`sh -c \"rm -rf ~\"`"),
            registration("gemini", "user", ""),
        ];
        let found = [
            Found::of("claude", "user", &stdio("repolore", "x", &[]), "here").expect("found"),
            Found::of(
                "swarmed",
                "own",
                &stdio("lore", "repolore-mcp", &[]),
                "there",
            )
            .expect("found"),
        ];
        // Codex is not installed here.
        let shown = registrations_of(&said, &found, &|program| program == "claude");
        let keys: Vec<&str> = shown.iter().map(|shown| shown.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "claude:user:repolore",
                "claude:local:repolore",
                "claude:local:repolore:2",
                "claude:plugin:repolore",
                "codex:user:repolore",
                "opencode:user:repolore",
                "cursor:user:repolore",
                "gemini:user:repolore",
                "swarmed:own:lore",
            ]
        );
        let runs: Vec<bool> = shown.iter().map(|shown| shown.runs_here).collect();
        assert_eq!(
            runs,
            [true, false, false, true, false, false, false, false, false]
        );
        let commands: Vec<Option<&str>> = shown.iter().map(|s| s.command.as_deref()).collect();
        assert_eq!(
            commands,
            [
                Some("claude mcp remove repolore --scope user"),
                None,
                None,
                Some("claude plugin uninstall repolore@repolore --scope user"),
                // Built by Swarmed: repolore says to edit Codex's file.
                Some("codex mcp remove repolore"),
                None,
                None,
                None,
                None,
            ]
        );
        assert_eq!(shown[0].step, None);
        assert_eq!(shown[0].client, "Claude Code");
        // A folder's own is the user's step, as repolore says it.
        assert_eq!(
            shown[1].step.as_deref(),
            Some("`claude mcp remove repolore --scope local`")
        );
        assert!(shown[2].step.is_some());
        // Codex's command is there, though Codex is not: the user runs it by hand.
        assert_eq!(shown[4].step, None);
        assert_eq!(
            shown[5].step.as_deref(),
            Some("delete repolore from /Users/me/.config/opencode/opencode.json")
        );
        // A command of no kind Swarmed runs is never offered as one.
        assert_eq!(shown[6].step.as_deref(), Some("`sh -c \"rm -rf ~\"`"));
        assert_eq!(
            shown[7].step.as_deref(),
            Some("Remove `repolore` from gemini yourself.")
        );
        assert_eq!(shown[8].client, "Swarmed");
        assert_eq!(shown[8].step.as_deref(), Some("Remove `lore` from there."));
    }

    #[test]
    fn codex_is_taken_out_by_swarmed_only_from_its_user_scope() {
        let codex = |scope: &str, name: &str| {
            let said = Registration {
                name: name.into(),
                ..registration(
                    "codex",
                    scope,
                    "delete [mcp_servers.repolore] from /Users/me/.codex/config.toml",
                )
            };
            registration_of(&said, "codex".into(), &|program| program == "codex")
        };
        let user = codex("user", "repolore");
        assert_eq!(user.command.as_deref(), Some("codex mcp remove repolore"));
        assert!(user.runs_here);
        assert_eq!(user.step, None);
        assert_eq!(
            removal(&registration("codex", "user", "")),
            Some(vec![
                "codex".into(),
                "mcp".into(),
                "remove".into(),
                "repolore".into()
            ])
        );
        // A project's own belongs to its folder: the user's step, as repolore says it.
        let project = codex("project", "repolore");
        assert_eq!(project.command, None);
        assert!(!project.runs_here);
        assert_eq!(
            project.step.as_deref(),
            Some("delete [mcp_servers.repolore] from /Users/me/.codex/config.toml")
        );
        // Even when repolore names a command for it.
        let named = registration("codex", "project", "`codex mcp remove repolore`");
        assert_eq!(removal(&named), None);
        // A name Codex would read as an option is left to the user.
        assert_eq!(codex("user", "--help").command, None);
        assert_eq!(codex("user", " ").command, None);
    }

    #[test]
    fn only_removals_that_belong_to_no_folder_are_run() {
        let runs = |argv: &[&str]| {
            let argv: Vec<String> = argv.iter().map(ToString::to_string).collect();
            remover(&argv).map(String::from)
        };
        let claude = Some("claude".to_string());
        assert_eq!(
            runs(&["claude", "mcp", "remove", "repolore", "-s", "user"]),
            claude
        );
        assert_eq!(
            runs(&["claude", "mcp", "remove", "-s", "user", "repolore"]),
            claude
        );
        assert_eq!(
            runs(&["claude", "mcp", "remove", "repolore", "--scope", "user"]),
            claude
        );
        assert_eq!(
            runs(&["claude", "mcp", "remove", "repolore", "--scope=user"]),
            claude
        );
        assert_eq!(
            runs(&["claude", "plugin", "uninstall", "repolore@repolore"]),
            claude
        );
        assert_eq!(
            runs(&["codex", "mcp", "remove", "repolore"]),
            Some("codex".to_string())
        );
        for refused in [
            &["claude", "mcp", "remove", "repolore"][..],
            &["claude", "mcp", "remove", "repolore", "-s", "local"],
            &["claude", "mcp", "remove", "repolore", "--scope", "project"],
            &[
                "claude",
                "plugin",
                "uninstall",
                "repolore@repolore",
                "--scope",
                "local",
            ],
            &["claude", "mcp", "remove"],
            &["claude", "-p", "remove everything", "-s", "user"],
            &["codex", "exec", "rm -rf ~"],
            &[
                "/usr/local/bin/claude",
                "mcp",
                "remove",
                "repolore",
                "-s",
                "user",
            ],
            &["opencode", "mcp", "remove", "repolore"],
            &[],
        ] {
            assert_eq!(runs(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn a_command_is_run_with_each_word_whole() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (command, env) =
            argv_for_shell(Path::new("/bin/echo"), &["a b".into(), "$HOME;x".into()]);
        assert_eq!(
            command,
            "\"$SWARMED_WORD_0\" \"$SWARMED_WORD_1\" \"$SWARMED_WORD_2\""
        );
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .envs(env)
            .current_dir(dir.path())
            .output()
            .expect("runs");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "a b $HOME;x\n");
    }

    #[test]
    fn nothing_of_swarmeds_or_holding_the_home_goes_to_the_trash() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().canonicalize().expect("real");
        let data = root.join("data");
        let make = |path: &Path| std::fs::create_dir_all(path).expect("made");
        for path in [
            &data.join("repolore"),
            &root.join("old/repolore"),
            &root.join("old/other"),
        ] {
            make(path);
        }
        let home = root.join("old");
        let may = |folder: &Path| may_trash(folder, &data, Some(&home)).map_err(|e| e.to_string());
        assert_eq!(
            may(&root.join("old/repolore")),
            Ok(root.join("old/repolore"))
        );
        assert!(may(&data.join("repolore")).is_err_and(|e| e.contains("Swarmed's own")));
        assert!(may(&data).is_err());
        assert!(may(&root).is_err_and(|e| e.contains("Swarmed's own")));
        assert!(may(&home).is_err_and(|e| e.contains("home folder")));
        assert!(may(&root.join("old/other")).is_err_and(|e| e.contains("name")));
        assert!(may(&root.join("old/gone-repolore")).is_err_and(|e| e.contains("not there")));
    }

    #[test]
    fn a_session_start_hook_is_found_anywhere_in_the_hooks() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = dir.path().join(".claude.json");
        assert!(!repolore_hook(&config));
        std::fs::create_dir_all(dir.path().join(".claude")).expect("made");
        let settings = dir.path().join(".claude/settings.json");
        let hook = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"repolore session-start --hours 12 >/dev/null 2>&1 &"}]}]}}"#;
        std::fs::write(&settings, hook).expect("written");
        assert!(repolore_hook(&config));
        // The program and its arguments apart, as `repolore setup` writes them.
        let apart = r#"{"hooks":{"SessionStart":[{"matcher":"startup","hooks":[{"type":"command","command":"/Users/me/.local/bin/repolore","args":["session-start","--hours","12"],"async":true}]}]}}"#;
        std::fs::write(&settings, apart).expect("written");
        assert!(repolore_hook(&config));
        for not in [
            r#"{"env":{"X":"repolore session-start"}}"#,
            r#"{"hooks":{"SessionStart":[{"hooks":[{"command":"repolore-mcp","args":["session-start"]}]}]}}"#,
            r#"{"hooks":{"SessionStart":[{"hooks":[{"command":"repolore refresh"}]}]}}"#,
        ] {
            std::fs::write(&settings, not).expect("written");
            assert!(!repolore_hook(&config), "{not}");
        }
    }
}
