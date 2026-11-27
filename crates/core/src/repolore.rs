//! repolore's command line, as the built-in server (`builtin_mcp.rs`) asks it: the one module
//! that speaks it.
//!
//! repolore searches the user's own engineering history on GitHub, and serves it to agents as
//! an MCP server (`repolore-mcp`). Swarmed finds out how it stands by asking its command line,
//! never by reading its files. What is asked, and the shape of each answer, is the contract
//! with repolore, which it keeps from [`MINIMUM`] on. No fallback is kept for older answers:
//! an older repolore is offered the upgrade instead.
//!
//! - `repolore version`: `repolore X.Y.Z` on the first line. Every version answers it, so it
//!   is what tells whether the rest can be asked.
//! - `repolore doctor --json [--offline]`: one JSON object on stdout, whatever the exit code.
//!   It counts as an answer only with its `checks` and `registrations` as lists: an object
//!   without them would read as nothing in the way. Past that it is read leniently: a field
//!   that is missing, or not of the kind expected, counts as empty, and one Swarmed does not
//!   know is passed over. `--offline` asks neither GitHub nor `claude mcp get`; the
//!   registrations are still read from their files.
//! - `repolore server status --json`: the shared server of a home, the `server` of the doctor.
//!   It counts as an answer only with `running` true or false.
//! - `repolore copy <folder>`: a copy of the user's setup in an empty folder, or why not on
//!   the last line of its error output. It cleans up after itself, and has no time limit.
//! - `repolore session-start --hours 12`: a refresh in the background. It returns at once.
//! - `repolore ui --no-browser`: its page's address alone on the first line, running until
//!   it is ended.
//!
//! Which home a command is about is said by its environment. The built-in's is
//! `REPOLORE_HOME=<data dir>/repolore` with `REPOLORE_MANAGED_BY=Swarmed`, which is repolore's
//! managed mode: it registers itself nowhere, and reports every other registration as a
//! conflict. The user's own, the old home, is asked with both removed. Every command gets the
//! `PATH` of the user's shell (`shellenv.rs`).
//!
//! The page's address carries its token: it is never logged, never in a view, never in an
//! error, and [`Page`]'s `Debug` leaves it out.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use tokio::process::Command;

use crate::agent_setup::find_program;
use crate::builtin_mcp::DoctorCheck;

/// The oldest repolore that keeps the contract above.
pub(crate) const MINIMUM: &str = "0.5.1";
/// repolore's command line.
pub(crate) const PROGRAM: &str = "repolore";
/// The program repolore's MCP server runs as.
pub(crate) const SERVER_PROGRAM: &str = "repolore-mcp";
/// The variable that names the home repolore keeps its configuration and data in.
pub(crate) const HOME_ENV: &str = "REPOLORE_HOME";
/// The variable that puts repolore into its managed mode, naming who manages it.
pub(crate) const MANAGED_ENV: &str = "REPOLORE_MANAGED_BY";
/// Who manages the built-in home.
pub(crate) const MANAGER: &str = "Swarmed";

/// How long `version` and `server status` may take: Python starts slowly the first time.
const QUICK_LIMIT: Duration = Duration::from_secs(60);
/// How long the doctor may take without asking the network.
const OFFLINE_LIMIT: Duration = Duration::from_secs(60);
/// How long the doctor may take when it asks GitHub and Claude Code too.
const FULL_LIMIT: Duration = Duration::from_secs(120);
/// How long the page may take to say where it is.
const PAGE_LIMIT: Duration = Duration::from_secs(30);
/// How long the page is given to end once asked, before it is killed.
const PAGE_GRACE: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(50);

/// repolore, found on this machine.
#[derive(Debug, Clone)]
pub(crate) struct Repolore {
    program: PathBuf,
}

/// Which of repolore's homes a command is about.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Home<'a> {
    /// The built-in's, in Swarmed's data directory, in managed mode.
    Builtin(&'a Path),
    /// The user's own, where repolore keeps it by itself.
    Old,
}

/// What a command printed, and how it ended.
#[derive(Debug)]
struct Said {
    out: String,
    /// The last line of its error output that holds anything.
    last_error: Option<String>,
}

/// The variables that point repolore at the built-in home, in managed mode: what its server
/// is given too.
pub(crate) fn builtin_env(home: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        (HOME_ENV.to_string(), home.to_string_lossy().into_owned()),
        (MANAGED_ENV.to_string(), MANAGER.to_string()),
    ])
}

impl Repolore {
    /// repolore's command line, if it is in one of `dirs`.
    pub(crate) fn find(dirs: &[PathBuf]) -> Option<Self> {
        find_program(PROGRAM, dirs).map(|program| Self { program })
    }

    /// A command about `home`, with the user's `PATH`, and nothing tied to this process's
    /// input.
    fn command(&self, home: Home<'_>) -> std::process::Command {
        let mut command = std::process::Command::new(&self.program);
        command
            .env("PATH", crate::shellenv::path())
            .stdin(Stdio::null());
        match home {
            Home::Builtin(dir) => {
                command.envs(builtin_env(dir));
            }
            Home::Old => {
                command.env_remove(HOME_ENV).env_remove(MANAGED_ENV);
            }
        }
        command
    }

    /// Runs `repolore <args>` about `home` to its end, for no longer than `limit`.
    async fn ask(&self, home: Home<'_>, args: &[&str], limit: Duration) -> Result<Said, String> {
        let mut command = Command::from(self.command(home));
        command.args(args).kill_on_drop(true);
        let asked = format!("`repolore {}`", args.join(" "));
        let output = tokio::time::timeout(limit, command.output())
            .await
            .map_err(|_| format!("{asked} gave no answer within {} s", limit.as_secs()))?
            .map_err(|error| format!("{asked} could not be started: {error}"))?;
        Ok(Said {
            out: String::from_utf8_lossy(&output.stdout).into_owned(),
            last_error: last_line(&output.stderr),
        })
    }

    /// The version this repolore says it is.
    pub(crate) async fn version(&self) -> Result<String, String> {
        let said = self.ask(Home::Old, &["version"], QUICK_LIMIT).await?;
        version_said(&said.out).ok_or_else(|| unread("version", &said))
    }

    /// What repolore's doctor says of `home`: everything with `full`, else what it can tell
    /// without the network.
    pub(crate) async fn doctor(&self, home: Home<'_>, full: bool) -> Result<Doctor, String> {
        let (args, limit): (&[&str], _) = if full {
            (&["doctor", "--json"], FULL_LIMIT)
        } else {
            (&["doctor", "--json", "--offline"], OFFLINE_LIMIT)
        };
        let said = self.ask(home, args, limit).await?;
        let lists = |answer: &Value| {
            let list = |key| answer.get(key).is_some_and(Value::is_array);
            list("checks") && list("registrations")
        };
        let asked = "doctor --json";
        read_answer(&said, asked, "its checks and registrations", lists)
    }

    /// How the shared server of `home` stands.
    pub(crate) async fn server_status(&self, home: Home<'_>) -> Result<ServerStatus, String> {
        let args = ["server", "status", "--json"];
        let said = self.ask(home, &args, QUICK_LIMIT).await?;
        let running = |answer: &Value| answer.get("running").is_some_and(Value::is_boolean);
        let asked = "server status --json";
        read_answer(&said, asked, "whether its server runs", running)
    }

    /// Copies the user's own setup into `folder`, which has to be empty or not there yet. The
    /// error is repolore's reason, the last line it printed of one.
    pub(crate) async fn copy(&self, folder: &Path) -> Result<(), String> {
        let mut command = Command::from(self.command(Home::Old));
        command.arg("copy").arg(folder).kill_on_drop(true);
        let output = command
            .output()
            .await
            .map_err(|error| format!("`repolore copy` could not be started: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        Err(last_line(&output.stderr).unwrap_or_else(|| format!("it ended with {}", output.status)))
    }

    /// Starts a refresh of the built-in home's index, if one is due, without waiting for it.
    pub(crate) fn session_start(&self, home: &Path) {
        let mut command = self.command(Home::Builtin(home));
        command.args(["session-start", "--hours", "12"]);
        crate::open::launch(command).ok();
    }
}

/// The version on the first line `repolore version` printed, `repolore X.Y.Z`.
fn version_said(printed: &str) -> Option<String> {
    let mut words = printed.lines().next()?.split_whitespace();
    if words.next()? != PROGRAM {
        return None;
    }
    let version = words.next()?;
    Version::parse(version).map(|_| version.to_string())
}

/// Whether `version` is [`MINIMUM`] or newer.
pub(crate) fn current(version: &str) -> bool {
    let minimum = Version::parse(MINIMUM);
    Version::parse(version).is_some_and(|version| Some(version) >= minimum)
}

/// A version's numbers, compared in order. What follows a number, such as `rc1`, is left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version([u64; 3]);

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let number = |part: &str| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u64>().ok()
        };
        let mut parts = text.trim().split('.').map(number);
        let major = parts.next()??;
        let minor = parts.next().flatten().unwrap_or(0);
        let patch = parts.next().flatten().unwrap_or(0);
        Some(Self([major, minor, patch]))
    }
}

/// The last line of `bytes` that holds anything.
fn last_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let last = text.lines().map(str::trim).rfind(|line| !line.is_empty());
    last.map(String::from)
}

/// Why an answer could not be read: what repolore said last on its error output, if anything.
fn unread(asked: &str, said: &Said) -> String {
    match &said.last_error {
        Some(line) => format!("`repolore {asked}` said: {line}"),
        None => format!("`repolore {asked}` said nothing Swarmed can read"),
    }
}

/// What `said` printed as the answer to `asked`, if it is JSON that `answers`: else why not,
/// naming `wanted`, what it lacks.
fn read_answer<T: DeserializeOwned>(
    said: &Said,
    asked: &str,
    wanted: &str,
    answers: impl Fn(&Value) -> bool,
) -> Result<T, String> {
    let answer: Value = serde_json::from_str(said.out.trim()).map_err(|_| unread(asked, said))?;
    if !answers(&answer) {
        return Err(format!("`repolore {asked}` answered without {wanted}"));
    }
    serde_json::from_value(answer).map_err(|_| unread(asked, said))
}

/// A field as it is, or empty if it is not of the kind expected.
pub(crate) fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// The entries of a list that can be read; none if it is no list.
fn each<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    let Value::Array(items) = Value::deserialize(deserializer)? else {
        return Ok(Vec::new());
    };
    let read = |item| serde_json::from_value(item).ok();
    Ok(items.into_iter().filter_map(read).collect())
}

/// What `repolore doctor --json` says of a home.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Doctor {
    /// Where the home's configuration and data are.
    #[serde(deserialize_with = "lenient")]
    pub(crate) paths: DoctorPaths,
    /// Whether the home has been set up: repositories chosen.
    #[serde(deserialize_with = "lenient")]
    pub(crate) configured: bool,
    /// When its index was built and last refreshed.
    #[serde(deserialize_with = "lenient")]
    pub(crate) index: DoctorIndex,
    /// What it checked, each with how it went.
    #[serde(deserialize_with = "each")]
    pub(crate) checks: Vec<DoctorCheck>,
    /// Where repolore is registered with an agent or another client.
    #[serde(deserialize_with = "each")]
    pub(crate) registrations: Vec<Registration>,
}

/// Where a home's configuration and data are.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct DoctorPaths {
    /// The home, when one folder holds both, as `REPOLORE_HOME` makes it.
    #[serde(deserialize_with = "lenient")]
    pub(crate) home: Option<PathBuf>,
    /// The configuration file.
    #[serde(deserialize_with = "lenient")]
    pub(crate) config_file: Option<PathBuf>,
    /// The folder of the index and what else it keeps.
    #[serde(deserialize_with = "lenient")]
    pub(crate) data_dir: Option<PathBuf>,
}

impl DoctorPaths {
    /// The folders a home is in, each once: the home, or where its configuration file is and
    /// its data folder, which are one on macOS.
    pub(crate) fn folders(&self) -> Vec<PathBuf> {
        if let Some(home) = &self.home {
            return vec![home.clone()];
        }
        let config = self.config_file.as_deref().and_then(Path::parent);
        let mut folders: Vec<PathBuf> = config.map(Path::to_path_buf).into_iter().collect();
        if let Some(data) = self.data_dir.clone().filter(|data| !folders.contains(data)) {
            folders.push(data);
        }
        folders
    }
}

/// When a home's index was built and last refreshed, as repolore writes the times.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct DoctorIndex {
    #[serde(deserialize_with = "lenient")]
    pub(crate) built_at: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub(crate) refreshed_at: Option<String>,
}

/// How the shared server of a home stands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct ServerStatus {
    #[serde(deserialize_with = "lenient")]
    pub(crate) running: bool,
    #[serde(deserialize_with = "lenient")]
    pub(crate) pid: Option<u32>,
    /// How many sessions' servers it answers through.
    #[serde(deserialize_with = "lenient")]
    pub(crate) relays: u32,
}

/// One place repolore is registered, with how it is taken out of there. Every field is read
/// leniently, so that an entry is never lost for a field that is odd: a lost one would let the
/// built-in run beside it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct Registration {
    /// The client's id, such as `claude` or `codex`.
    #[serde(deserialize_with = "lenient")]
    pub(crate) client: String,
    /// The client's name, to show.
    #[serde(deserialize_with = "lenient")]
    pub(crate) client_name: String,
    /// The name repolore is registered under there.
    #[serde(deserialize_with = "lenient")]
    pub(crate) name: String,
    /// Where in the client: `user`, `local`, `plugin` and the like.
    #[serde(deserialize_with = "lenient")]
    pub(crate) scope: String,
    /// The command that takes it out, word by word, if there is one.
    #[serde(deserialize_with = "lenient")]
    pub(crate) remove: Option<Vec<String>>,
    /// How to take it out, said for the user.
    #[serde(deserialize_with = "lenient")]
    pub(crate) remove_text: String,
}

/// repolore's page for the built-in home, while it runs: its process, and its address, which
/// carries the page's token.
pub(crate) struct Page {
    child: Child,
    address: String,
}

impl fmt::Debug for Page {
    /// Names the process, never the address.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pid = self.child.id();
        f.debug_struct("Page")
            .field("pid", &pid)
            .finish_non_exhaustive()
    }
}

impl Page {
    /// Starts repolore's page for `home`, in a process group of its own, and waits for its
    /// address. Without the standard library's processes alone: this may run where no Tokio
    /// runtime is.
    pub(crate) fn start(repolore: &Repolore, home: &Path) -> Result<Self, String> {
        let mut command = repolore.command(Home::Builtin(home));
        command
            .args(["ui", "--no-browser"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command
            .spawn()
            .map_err(|error| format!("repolore's page could not be started: {error}"))?;
        let Some(stdout) = child.stdout.take() else {
            end(child);
            return Err("repolore's page could not be started".to_string());
        };
        match first_line(stdout).recv_timeout(PAGE_LIMIT) {
            Ok(line) if web_address(&line) => Ok(Self {
                child,
                address: line.trim().to_string(),
            }),
            Ok(_) => {
                end(child);
                Err("repolore's page did not say where it is".to_string())
            }
            Err(_) => {
                end(child);
                let limit = PAGE_LIMIT.as_secs();
                Err(format!("repolore's page did not start within {limit} s"))
            }
        }
    }

    /// Whether its process still runs.
    pub(crate) fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Where it is. Never to be logged or shown: it carries the page's token.
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    /// Ends it, with whatever it started, if it still runs.
    pub(crate) fn end(self) {
        end(self.child);
    }
}

/// Whether the page's first line is an address a browser opens.
fn web_address(line: &str) -> bool {
    let line = line.trim();
    (line.starts_with("http://") || line.starts_with("https://"))
        && !line.chars().any(char::is_whitespace)
}

/// The first line `stdout` gives, from a thread of its own, which then reads on and drops the
/// rest: a pipe nobody reads would stop the page once it is full.
fn first_line(stdout: ChildStdout) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    let reading = move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).ok();
        sender.send(line).ok();
        std::io::copy(&mut reader, &mut std::io::sink()).ok();
    };
    drop(std::thread::spawn(reading));
    receiver
}

/// Ends a process that leads a group of its own, with everything in the group: asked first,
/// then killed if it has not gone after a moment. Waited for on a thread of its own, so that
/// nothing waits here. One that has gone already is sent nothing: its id may be another's by
/// now.
fn end(mut child: Child) {
    if !matches!(child.try_wait(), Ok(None)) {
        return;
    }
    let group = format!("-{}", child.id());
    signal("-TERM", &group);
    drop(std::thread::spawn(move || wind_down(child, &group)));
}

fn wind_down(mut child: Child, group: &str) {
    let until = Instant::now() + PAGE_GRACE;
    while matches!(child.try_wait(), Ok(None)) && Instant::now() < until {
        std::thread::sleep(POLL);
    }
    if matches!(child.try_wait(), Ok(None)) {
        signal("-KILL", group);
        child.kill().ok();
    }
    child.wait().ok();
}

/// Sends `which` to `target` with `kill`: a minus before a process's id addresses its group.
fn signal(which: &str, target: &str) {
    let sent = std::process::Command::new("kill")
        .args([which, "--", target])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    sent.ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_the_second_word_of_the_first_line() {
        assert_eq!(
            version_said("repolore 0.5.1\nmore\n").as_deref(),
            Some("0.5.1")
        );
        assert_eq!(
            version_said("repolore 0.6.0rc1").as_deref(),
            Some("0.6.0rc1")
        );
        assert_eq!(version_said("repolore-mcp 0.5.1"), None);
        assert_eq!(version_said("something else 0.5.1"), None);
        assert_eq!(version_said(""), None);
        assert_eq!(version_said("repolore unknown"), None);
    }

    #[test]
    fn versions_are_compared_by_their_numbers() {
        assert!(current("0.5.1"));
        assert!(current("0.5.10"));
        assert!(current("0.6"));
        assert!(current("1.0.0"));
        assert!(current("0.5.1rc1"));
        assert!(!current("0.5.0"));
        assert!(!current("0.4.99"));
        assert!(!current("not a version"));
    }

    #[test]
    fn the_doctor_is_read_whatever_is_odd_in_it() {
        let said = r#"{
            "version": "0.5.1", "managed_by": null, "something_new": [1, 2],
            "paths": {"home": null, "config_file": "/c/repolore/repolore.toml", "data_dir": 7},
            "configured": "yes",
            "index": {"built_at": "2026-11-20T10:00:00Z", "refreshed_at": null},
            "checks": [
                {"name": "config", "ok": true, "warning": false, "detail": "fine", "fix": null},
                "not a check",
                {"name": "model", "ok": true, "warning": true, "detail": "slow"},
                {"name": "github", "ok": false, "warning": false, "detail": 404, "fix": ["x"]}
            ],
            "server": {"running": true, "pid": 42, "version": "0.5.1", "relays": 2},
            "registrations": [{"client": "claude", "client_name": "Claude Code",
                "name": "repolore", "command": ["repolore-mcp"], "scope": "user",
                "for_every_project": true, "remove": ["claude", "mcp", "remove", "repolore"],
                "remove_text": null}]
        }"#;
        let doctor: Doctor = serde_json::from_str(said).expect("reads");
        assert_eq!(doctor.paths.folders(), [PathBuf::from("/c/repolore")]);
        assert!(!doctor.configured);
        assert_eq!(
            doctor.index.built_at.as_deref(),
            Some("2026-11-20T10:00:00Z")
        );
        let checks: Vec<&str> = doctor.checks.iter().map(|c| c.name.as_str()).collect();
        // One with an odd field is kept: dropped, a failing check would read as one passed.
        assert_eq!(checks, ["config", "model", "github"]);
        assert!(!doctor.checks[2].ok && doctor.checks[2].detail.is_empty());
        assert!(doctor.checks[1].warning && doctor.checks[1].fix.is_none());
        let registration = &doctor.registrations[0];
        assert_eq!(registration.client, "claude");
        assert_eq!(registration.remove_text, "");
        assert_eq!(registration.remove.as_deref().map(<[String]>::len), Some(4));
    }

    #[test]
    fn an_answer_counts_only_with_what_it_is_asked_for() {
        let said = |out: &str| Said {
            out: out.to_string(),
            last_error: None,
        };
        let lists = |answer: &Value| answer.get("checks").is_some_and(Value::is_array);
        let read =
            |out: &str| read_answer::<Doctor>(&said(out), "doctor --json", "its checks", lists);
        assert!(read(r#"{"checks": []}"#).is_ok());
        assert_eq!(
            read("{}").map(|_| ()),
            Err("`repolore doctor --json` answered without its checks".to_string())
        );
        assert!(read(r#"{"checks": "none"}"#).is_err());
        assert!(read("not json").is_err());
        let running = |answer: &Value| answer.get("running").is_some_and(Value::is_boolean);
        let server = |out: &str| {
            read_answer::<ServerStatus>(&said(out), "server status --json", "it", running)
        };
        assert!(server(r#"{"running": false}"#).is_ok());
        assert!(server(r#"{"pid": 7, "relays": 2}"#).is_err());
        assert!(server(r#"{"running": "yes"}"#).is_err());
    }

    #[test]
    fn the_folders_of_a_home_are_each_named_once() {
        let paths = |home: Option<&str>, config: &str, data: &str| DoctorPaths {
            home: home.map(PathBuf::from),
            config_file: Some(PathBuf::from(config)),
            data_dir: Some(PathBuf::from(data)),
        };
        let mac = paths(None, "/l/repolore/repolore.toml", "/l/repolore");
        assert_eq!(mac.folders(), [PathBuf::from("/l/repolore")]);
        let linux = paths(None, "/c/repolore/repolore.toml", "/d/repolore");
        assert_eq!(
            linux.folders(),
            [PathBuf::from("/c/repolore"), PathBuf::from("/d/repolore")]
        );
        let home = paths(Some("/h"), "/h/repolore.toml", "/h/data");
        assert_eq!(home.folders(), [PathBuf::from("/h")]);
    }

    /// A stand-in for repolore in `dir` that runs `script`, written by a child process: one this
    /// process held open would be "Text file busy" to a test starting it on Linux.
    fn stand_in(dir: &Path, script: &str) -> Repolore {
        let (source, program) = (dir.join("source"), dir.join(PROGRAM));
        std::fs::write(&source, format!("#!/bin/sh\n{script}\n")).expect("written");
        let installed = std::process::Command::new("install")
            .args(["-m", "755"])
            .arg(&source)
            .arg(&program)
            .status()
            .expect("installs");
        assert!(installed.success());
        Repolore { program }
    }

    /// Whether process `pid` is there, waiting a moment for it to go.
    fn gone(pid: u32) -> bool {
        let there = || {
            let asked = std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status();
            asked.is_ok_and(|status| status.success())
        };
        let until = Instant::now() + Duration::from_secs(10);
        while there() && Instant::now() < until {
            std::thread::sleep(POLL);
        }
        !there()
    }

    #[test]
    fn the_page_says_where_it_is_and_goes_with_whatever_it_started() {
        let dir = tempfile::tempdir().expect("temp dir");
        let started = dir.path().join("started");
        let env = dir.path().join("env");
        let script = format!(
            "[ \"$1 $2\" = \"ui --no-browser\" ] || exit 2\n\
             echo \"$REPOLORE_HOME $REPOLORE_MANAGED_BY\" > '{}'\n\
             sleep 60 &\necho $! > '{}'\n\
             echo 'http://127.0.0.1:9/?token=secret'\nwait",
            env.display(),
            started.display()
        );
        let repolore = stand_in(dir.path(), &script);
        let home = dir.path().join("home");
        let mut page = Page::start(&repolore, &home).expect("starts");
        assert_eq!(page.address(), "http://127.0.0.1:9/?token=secret");
        let said = std::fs::read_to_string(&env).expect("written");
        assert_eq!(said.trim(), format!("{} Swarmed", home.display()));
        assert!(page.alive());
        assert!(!format!("{page:?}").contains("secret"));
        let child: u32 = std::fs::read_to_string(&started)
            .expect("written")
            .trim()
            .parse()
            .expect("a pid");
        let pid = page.child.id();
        page.end();
        assert!(gone(pid) && gone(child));
    }

    #[test]
    fn a_page_that_says_no_address_is_ended() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repolore = stand_in(dir.path(), "echo 'Starting up'\nexec sleep 60");
        let refused = Page::start(&repolore, dir.path()).map(|page| page.address().to_string());
        assert_eq!(
            refused,
            Err("repolore's page did not say where it is".to_string())
        );
    }

    #[test]
    fn only_a_web_address_is_taken_for_the_pages() {
        assert!(web_address("http://127.0.0.1:8123/?token=abc\n"));
        assert!(!web_address("Starting…"));
        assert!(!web_address("http://x y"));
        assert!(!web_address(""));
    }

    #[test]
    fn the_last_line_with_anything_on_it_is_the_reason() {
        assert_eq!(
            last_line(b"Traceback\n  x\nError: a refresh is running\n\n").as_deref(),
            Some("Error: a refresh is running")
        );
        assert_eq!(last_line(b"  \n"), None);
    }
}
