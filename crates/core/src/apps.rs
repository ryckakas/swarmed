//! The apps MCP servers attach to their tools (MCP Apps), shown under the calls they belong to.
//!
//! A tool can name a `ui://` resource, an HTML document its server holds, to be shown with
//! the tool's result in a sandboxed frame that talks to its host over `postMessage`. Claude
//! Code says which tools have one and reads it for its host (`agent/claude/apps.rs`); here it
//! is kept and served.
//!
//! An app is a third party's HTML: the server's author's, reached through the agent's call.
//! It is treated as warily as an agent's own view, and more:
//!
//! - **Kept with the session, once.** Its HTML goes into the session's own folder beside the
//!   pasted images ([`kept`]), one file per distinct document (`app-<hash>.html`), at most
//!   [`MAX_APP_MB`]; the event names the file. It goes with that folder, and is never logged,
//!   nor handed to another session, a fork, the cloud or search.
//! - **Served under its own policy.** Each shown app gets an address of its own
//!   (`swarmed-view://app-<id>/`) and a policy built from what the server declared
//!   ([`app_policy`]): the network only where declared, each entry an `https:` origin (or
//!   `wss:` for connecting), anything else refused and named. Nested frames are never
//!   allowed: the window refuses to take any frame off Swarmed's own pages. Views keep
//!   [`crate::VIEW_POLICY`], unchanged.
//! - **Its tools only through the agent.** A tool call an app asks for goes to the agent
//!   ([`Core::session_app_call`]), decided as the agent's own call of that tool is, and only
//!   for a tool of the app's own server that it may call.

use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::agent::{AgentCommand, AgentEvent};
use crate::error::{Error, Result};
use crate::pasted::{created, private_folder, written};
use crate::paths::Paths;
use crate::{Core, lock};

/// The largest app kept, in megabytes. A larger one is named in a note under its call.
pub const MAX_APP_MB: usize = 4;

/// [`MAX_APP_MB`] in bytes.
pub(crate) const MAX_APP_BYTES: usize = MAX_APP_MB * 1024 * 1024;

/// The largest result of a call kept for its app, as JSON. A larger one is left out, and the
/// app is told the call's text alone.
pub(crate) const MAX_RESULT_BYTES: usize = 1024 * 1024;

/// The most origins of one kind taken from a declaration.
const MAX_DOMAINS: usize = 32;

/// What every kept app's file name starts with; a hash and `.html` follow.
const NAME_START: &str = "app-";

/// How many apps are served at once. Each open transcript holds a few; old ones are dropped,
/// and a frame that asks for one again gets it served again.
const MAX_SERVED: usize = 200;

/// How long an app's tool call may wait for the agent's answer, the user's permission
/// included.
const CALL_WAIT: Duration = Duration::from_secs(600);

/// What an MCP server declared its app may reach (`_meta.ui.csp`), as it declared it. What
/// of it is allowed is decided when the app is shown ([`app_policy`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppCsp {
    /// Where it may connect: `fetch`, requests, sockets (`connectDomains`).
    #[serde(default)]
    pub connect: Vec<String>,
    /// Where it may load scripts, styles, images, fonts and media from (`resourceDomains`).
    #[serde(default)]
    pub resources: Vec<String>,
    /// What it may show in frames of its own (`frameDomains`), which is never allowed here.
    #[serde(default)]
    pub frames: Vec<String>,
    /// What its document's base address may be (`baseUriDomains`).
    #[serde(default)]
    pub base_uris: Vec<String>,
}

impl AppCsp {
    /// Reads a declaration as a server gives it: `connectDomains` and the like, lists of
    /// strings. Anything else in it is passed over.
    #[must_use]
    pub fn declared(csp: &Value) -> Self {
        let list = |key: &str| {
            let items = csp.get(key).and_then(Value::as_array).into_iter().flatten();
            items
                .filter_map(Value::as_str)
                .take(MAX_DOMAINS * 2)
                .map(String::from)
                .collect()
        };
        Self {
            connect: list("connectDomains"),
            resources: list("resourceDomains"),
            frames: list("frameDomains"),
            base_uris: list("baseUriDomains"),
        }
    }
}

/// The policy an app is served under, with what of its declaration it lets through and what
/// it refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppPolicy {
    /// The `Content-Security-Policy` the app's document is served with.
    pub policy: String,
    /// Every origin the app may reach, each once, in the order declared: what the user is
    /// shown before an app that reaches anything is loaded.
    pub reaches: Vec<String>,
    /// What was declared and not let through, each with why, for the user to be told.
    pub refused: Vec<String>,
}

/// What an app's scripts may do whatever it declared: run its own code, inline or evaluated,
/// in a frame that has no origin of its own, where evaluating reaches nothing more than an
/// inline script does; and nothing that submits, embeds or plugs in.
const ALWAYS: &str = "default-src 'none'; form-action 'none'; object-src 'none'; frame-src 'none'";

/// Builds the policy for an app that declared `csp`.
///
/// Each entry must be an `https:` origin (`wss:` too for connecting), its host optionally
/// starting with `*.` for any subdomain, a port allowed, and no path; anything else is
/// refused. Frames are refused whole: the window keeps every frame on Swarmed's own pages.
#[must_use]
pub fn app_policy(csp: &AppCsp) -> AppPolicy {
    let mut refused = Vec::new();
    let connect = origins(&csp.connect, &["https", "wss"], "connecting", &mut refused);
    let resources = origins(&csp.resources, &["https"], "files", &mut refused);
    let base = origins(&csp.base_uris, &["https"], "base address", &mut refused);
    refused.extend(
        csp.frames
            .iter()
            .map(|frame| format!("{} (frames are not shown)", shown(frame))),
    );
    let with = |fixed: &str, origins: &[String]| {
        std::iter::once(fixed.to_string())
            .chain(origins.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let or_none = |origins: &[String]| {
        if origins.is_empty() {
            "'none'".to_string()
        } else {
            origins.join(" ")
        }
    };
    let code = "'unsafe-inline' 'unsafe-eval' 'wasm-unsafe-eval'";
    let policy = [
        ALWAYS.to_string(),
        format!("script-src {}", with(code, &resources)),
        format!("style-src {}", with("'unsafe-inline'", &resources)),
        format!("img-src {}", with("data: blob:", &resources)),
        format!("font-src {}", with("data:", &resources)),
        format!("media-src {}", with("data: blob:", &resources)),
        format!("connect-src {}", or_none(&connect)),
        format!("base-uri {}", or_none(&base)),
    ]
    .join("; ");
    let mut reaches: Vec<String> = Vec::new();
    for origin in connect.into_iter().chain(resources).chain(base) {
        if !reaches.contains(&origin) {
            reaches.push(origin);
        }
    }
    AppPolicy {
        policy,
        reaches,
        refused,
    }
}

/// The origins of `declared` a policy can name, each once, at most [`MAX_DOMAINS`]: one of
/// `schemes` each. Every other entry is added to `refused` with why, as kind `what`.
fn origins(
    declared: &[String],
    schemes: &[&str],
    what: &str,
    refused: &mut Vec<String>,
) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for entry in declared {
        match origin_of(entry, schemes) {
            None => refused.push(format!("{} ({what}: not an https origin)", shown(entry))),
            Some(origin) if kept.contains(&origin) => (),
            Some(origin) if kept.len() >= MAX_DOMAINS => {
                refused.push(format!("{origin} ({what}: more than {MAX_DOMAINS})"));
            }
            Some(origin) => kept.push(origin),
        }
    }
    kept
}

/// `entry` as an origin a policy can name, if it is one: a scheme of `schemes`, a host of
/// letters, digits, dashes and dots (or `*.` and such a host) that is no IP address, an
/// optional port, and nothing after it but a slash. Written in lower case.
fn origin_of(entry: &str, schemes: &[&str]) -> Option<String> {
    let entry = entry.trim().to_ascii_lowercase();
    let (scheme, rest) = entry.split_once("://")?;
    if !schemes.contains(&scheme) {
        return None;
    }
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    let port_ok = port.is_none_or(|port| {
        (1..=5).contains(&port.len()) && port.bytes().all(|b| b.is_ascii_digit())
    });
    let bare = host.strip_prefix("*.").unwrap_or(host);
    let label_ok = |label: &str| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    // No top-level domain is a number, so a host that ends in one is an IP address, which
    // would name a machine on the user's own network as readily as one on the web.
    let named = bare
        .rsplit('.')
        .next()
        .is_some_and(|last| !last.bytes().all(|b| b.is_ascii_digit()));
    let host_ok = named && bare.contains('.') && bare.split('.').all(label_ok);
    (port_ok && host_ok).then(|| format!("{scheme}://{authority}"))
}

/// An entry as it can be shown in a note: its printable characters, cut short.
fn shown(entry: &str) -> String {
    let printable: String = entry.chars().filter(|c| !c.is_control()).take(80).collect();
    if printable.trim().is_empty() {
        "an empty entry".to_string()
    } else {
        printable
    }
}

/// The name a document is kept under: its hash, so that the same app called again is kept
/// once.
fn name_of(html: &str, number: u32) -> String {
    let mut hasher = DefaultHasher::new();
    html.hash(&mut hasher);
    let hash = hasher.finish();
    if number == 0 {
        format!("{NAME_START}{hash:016x}.html")
    } else {
        format!("{NAME_START}{hash:016x}-{number}.html")
    }
}

/// Writes `html` into `folder`, or finds it there already, and returns its path. Never
/// writes over a file: a document of the same hash and other contents gets a name of its own.
fn keep(folder: &Path, html: &str) -> Result<PathBuf> {
    private_folder(folder)?;
    for number in 0..16 {
        let path = folder.join(name_of(html, number));
        if let Some(file) = created(&path)? {
            return written(file, path, html.as_bytes());
        }
        if std::fs::read(&path).is_ok_and(|there| there == html.as_bytes()) {
            return Ok(path);
        }
    }
    Err(Error::AppNotKept(
        "too many documents share its name".to_string(),
    ))
}

/// The app in `event` kept in session `session`'s own folder, and named on the event by its
/// file in place of its HTML. One that cannot be written says why in its note. A sub-agent's
/// app is kept as the agent's own is, and every other event is passed on as it is.
pub(crate) fn kept(paths: &Paths, session: i64, event: AgentEvent) -> AgentEvent {
    match event {
        AgentEvent::App {
            html: Some(html),
            tool_use_id,
            server,
            tool,
            uri,
            document: _,
            note,
            csp,
            permissions,
            prefers_border,
            callable,
            result,
        } => {
            let (document, note) = match keep(&paths.attachments(session), &html) {
                Ok(path) => (Some(path), note),
                Err(error) => (None, Some(format!("The app could not be kept: {error}"))),
            };
            AgentEvent::App {
                tool_use_id,
                server,
                tool,
                uri,
                document,
                note,
                csp,
                permissions,
                prefers_border,
                callable,
                result,
                html: None,
            }
        }
        AgentEvent::Sub { parent, event } => AgentEvent::Sub {
            parent,
            event: Box::new(kept(paths, session, *event)),
        },
        other => other,
    }
}

/// `path` as it really is, if it is a kept app right in `folder`: not in a folder below it,
/// not a link that leads out of it, and named as an app is.
fn inside(folder: &Path, path: &Path) -> Result<PathBuf> {
    let not_kept = || Error::AppNotFound(path.display().to_string());
    let root = folder.canonicalize().map_err(|_| not_kept())?;
    let real = path.canonicalize().map_err(|_| not_kept())?;
    let named = real
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with(NAME_START)
                && Path::new(name)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
                && !name.contains(std::path::is_separator)
        });
    if named && real.parent() == Some(root.as_path()) && real.is_file() {
        Ok(real)
    } else {
        Err(not_kept())
    }
}

/// The apps being shown, by the id their address is made of: each its kept file and its
/// policy. Their HTML is read from the file each time it is fetched, so that only what is on
/// screen is held.
#[derive(Debug, Default)]
pub(crate) struct Served {
    apps: HashMap<String, (PathBuf, String)>,
    order: VecDeque<String>,
}

impl Served {
    fn put(&mut self, id: String, served: (PathBuf, String)) {
        if self.apps.insert(id.clone(), served).is_none() {
            self.order.push_back(id);
        }
        while self.order.len() > MAX_SERVED
            && let Some(oldest) = self.order.pop_front()
        {
            self.apps.remove(&oldest);
        }
    }
}

/// What `Core` holds the served apps in.
pub(crate) type AppStore = Mutex<Served>;

/// The id an app is served under: the same for the same session, call, document and policy,
/// so that a frame drawn again loads the same address; another for any other app.
fn served_id(session: i64, tool_use_id: &str, document: &Path, policy: &str) -> String {
    let mut hasher = DefaultHasher::new();
    (session, tool_use_id, document, policy).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// What a front end needs to show an app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppShown {
    /// What its address is made of: `swarmed-view://app-<id>/`.
    pub id: String,
    /// The policy it is served under, and what of its declaration that lets through.
    #[serde(flatten)]
    pub policy: AppPolicy,
}

/// The answer to an app's tool call, as the agent gave it: the result, or why there is none.
pub(crate) type AppAnswer = (Option<Value>, Option<String>);

/// Who waits for the agents' answers to the tool calls apps asked for.
#[derive(Debug, Default)]
pub(crate) struct AppCalls {
    numbers: AtomicU64,
    waiting: Mutex<HashMap<(i64, u64), oneshot::Sender<AppAnswer>>>,
}

impl AppCalls {
    /// A number for a new call of session `id`'s, and where its answer arrives.
    fn begin(&self, id: i64) -> (u64, oneshot::Receiver<AppAnswer>) {
        let call = self.numbers.fetch_add(1, Ordering::Relaxed) + 1;
        let (answer, answered) = oneshot::channel();
        lock(&self.waiting).insert((id, call), answer);
        (call, answered)
    }

    /// Hands the agent's answer to call `call` of session `id` to whoever waits for it.
    pub(crate) fn answer(&self, id: i64, call: u64, answer: AppAnswer) {
        if let Some(waiting) = lock(&self.waiting).remove(&(id, call)) {
            waiting.send(answer).ok();
        }
    }

    /// Forgets who waits on session `id`'s agent, which has gone and answers nobody: they
    /// hear so at once.
    pub(crate) fn end(&self, id: i64) {
        lock(&self.waiting).retain(|(session, _), _| *session != id);
    }

    /// Forgets one call that is no longer waited for.
    fn give_up(&self, id: i64, call: u64) {
        lock(&self.waiting).remove(&(id, call));
    }
}

/// The app of call `tool_use_id` in `events`, the agent's own or a sub-agent's.
fn app_in(event: AgentEvent, tool_use_id: &str) -> Option<AgentEvent> {
    match event {
        AgentEvent::App {
            tool_use_id: ref id,
            ..
        } if id == tool_use_id => Some(event),
        AgentEvent::Sub { event, .. } => app_in(*event, tool_use_id),
        _ => None,
    }
}

impl Core {
    /// Makes the app kept for session `id` in `document` ready to be shown, under the policy
    /// its server's declaration `csp` comes to, and says what that lets it reach. Only a file
    /// kept as an app right in the session's own folder is served.
    pub fn app_put(
        &self,
        id: i64,
        tool_use_id: &str,
        document: &Path,
        csp: &AppCsp,
    ) -> Result<AppShown> {
        let real = inside(&self.paths.attachments(id), document)?;
        let policy = app_policy(csp);
        let served = served_id(id, tool_use_id, &real, &policy.policy);
        lock(&self.apps).put(served.clone(), (real, policy.policy.clone()));
        Ok(AppShown { id: served, policy })
    }

    /// The document and policy of the app served under `id`, if it is still served and its
    /// file still there.
    #[must_use]
    pub fn app_document(&self, id: &str) -> Option<(String, String)> {
        let (path, policy) = lock(&self.apps).apps.get(id).cloned()?;
        let size = std::fs::metadata(&path).ok()?.len();
        if size > MAX_APP_BYTES as u64 {
            return None;
        }
        let html = std::fs::read_to_string(path).ok()?;
        Some((html, policy))
    }

    /// Calls a tool for the app shown under call `tool_use_id` of session `id`, and returns
    /// the tool's result (MCP's `CallToolResult`). Only a tool of the app's own server that
    /// it may call: one whose server did not keep it from apps. The session's agent decides it
    /// as its own call of the tool, so it may ask the user first; its result stays out of the
    /// model's context. Refused if the agent is not running.
    pub async fn session_app_call(
        &self,
        id: i64,
        tool_use_id: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<Value> {
        let app = self.store().app_of(id, tool_use_id)?;
        let Some(AgentEvent::App {
            server, callable, ..
        }) = app.and_then(|event| app_in(event, tool_use_id))
        else {
            return Err(Error::AppNotFound(tool_use_id.to_string()));
        };
        if !callable.iter().any(|name| name == tool) {
            return Err(Error::AppToolRefused(tool.to_string()));
        }
        let arguments = if arguments.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            arguments
        };
        // Waited for before it is asked: the answer can come before this would be done.
        let (call, answered) = self.live.apps.begin(id);
        let command = AgentCommand::CallTool {
            call,
            server,
            tool: tool.to_string(),
            arguments,
        };
        if let Err(error) = self.live.command(id, command) {
            self.live.apps.give_up(id, call);
            return Err(error);
        }
        let answer = tokio::time::timeout(CALL_WAIT, answered).await;
        match answer {
            Ok(Ok((Some(result), _))) => Ok(result),
            Ok(Ok((None, why))) => Err(Error::AppCallFailed(
                why.unwrap_or_else(|| "the agent gave no reason".to_string()),
            )),
            // The agent went without answering.
            Ok(Err(_)) => Err(Error::SessionNotRunning(id)),
            Err(_) => {
                self.live.apps.give_up(id, call);
                Err(Error::AppCallFailed("the agent did not answer".to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn csp(connect: &[&str], resources: &[&str]) -> AppCsp {
        AppCsp {
            connect: connect.iter().map(|s| (*s).to_string()).collect(),
            resources: resources.iter().map(|s| (*s).to_string()).collect(),
            ..AppCsp::default()
        }
    }

    fn directive<'a>(policy: &'a str, name: &str) -> &'a str {
        policy
            .split("; ")
            .find(|part| part.split(' ').next() == Some(name))
            .unwrap_or_else(|| panic!("no {name} in {policy}"))
    }

    #[test]
    fn an_app_that_declares_nothing_reaches_nothing() {
        let built = app_policy(&AppCsp::default());
        assert_eq!(
            directive(&built.policy, "default-src"),
            "default-src 'none'"
        );
        assert_eq!(
            directive(&built.policy, "connect-src"),
            "connect-src 'none'"
        );
        assert_eq!(directive(&built.policy, "frame-src"), "frame-src 'none'");
        assert_eq!(directive(&built.policy, "object-src"), "object-src 'none'");
        assert_eq!(directive(&built.policy, "base-uri"), "base-uri 'none'");
        assert_eq!(
            directive(&built.policy, "form-action"),
            "form-action 'none'"
        );
        assert_eq!(
            directive(&built.policy, "img-src"),
            "img-src data: blob:",
            "nothing from the network"
        );
        assert!(!built.policy.contains("https:"));
        assert!(!built.policy.contains("'self'"));
        assert!(built.reaches.is_empty(), "there is none");
        assert!(built.refused.is_empty(), "there is none");
    }

    #[test]
    fn declared_origins_reach_only_what_they_were_declared_for() {
        let built = app_policy(&csp(
            &["https://api.example.com", "wss://live.example.com:8443/"],
            &["https://*.cdn.example.com", "HTTPS://Fonts.Example.com"],
        ));
        assert_eq!(
            directive(&built.policy, "connect-src"),
            "connect-src https://api.example.com wss://live.example.com:8443"
        );
        assert_eq!(
            directive(&built.policy, "script-src"),
            "script-src 'unsafe-inline' 'unsafe-eval' 'wasm-unsafe-eval' \
             https://*.cdn.example.com https://fonts.example.com"
        );
        assert!(directive(&built.policy, "font-src").ends_with("https://fonts.example.com"));
        assert_eq!(
            built.reaches,
            [
                "https://api.example.com",
                "wss://live.example.com:8443",
                "https://*.cdn.example.com",
                "https://fonts.example.com"
            ]
        );
        assert!(built.refused.is_empty(), "there is none");
    }

    #[test]
    fn anything_but_an_https_origin_is_refused_and_named() {
        let declared = [
            "http://plain.example.com",
            "https://example.com/path/to",
            "*",
            "https://*",
            "https://a.example.com; script-src *",
            "https://a.example.com 'unsafe-hashes'",
            "data:",
            "https://localhost",
            "https://-bad.example.com",
            "https://example.com:99999x",
            "wss://socket.example.com",
            "",
            "https://169.254.169.254",
            "https://192.168.1.1:8443",
            "https://*.0.1",
        ];
        let built = app_policy(&csp(&[], &declared));
        assert_eq!(built.reaches, Vec::<String>::new());
        assert_eq!(built.refused.len(), declared.len());
        assert!(built.refused[0].starts_with("http://plain.example.com (files"));
        assert!(built.refused[11].starts_with("an empty entry"));
        assert!(built.refused[12].starts_with("https://169.254.169.254 (files"));
        // Nothing declared makes its way into the policy as written.
        assert!(!built.policy.contains("plain"));
        assert!(!built.policy.contains("'unsafe-hashes'"));
        assert_eq!(built.policy.matches("script-src").count(), 1);
    }

    #[test]
    fn frames_are_never_allowed() {
        let declared = AppCsp {
            frames: vec!["https://www.youtube.com".to_string()],
            ..AppCsp::default()
        };
        let built = app_policy(&declared);
        assert_eq!(directive(&built.policy, "frame-src"), "frame-src 'none'");
        assert_eq!(
            built.refused,
            ["https://www.youtube.com (frames are not shown)"]
        );
    }

    #[test]
    fn a_long_list_is_cut_and_said() {
        let many: Vec<String> = (0..40)
            .map(|n| format!("https://h{n}.example.com"))
            .collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let built = app_policy(&csp(&many, &[]));
        assert_eq!(built.reaches.len(), MAX_DOMAINS);
        assert_eq!(built.refused.len(), 40 - MAX_DOMAINS);
    }

    #[test]
    fn a_declaration_is_read_as_a_server_gives_it() {
        let read = AppCsp::declared(&serde_json::json!({
            "connectDomains": ["https://api.example.com", 3],
            "resourceDomains": "https://not-a-list.example.com",
            "frameDomains": ["https://frames.example.com"],
            "other": ["x"],
        }));
        assert_eq!(read.connect, ["https://api.example.com"]);
        assert!(read.resources.is_empty(), "there is none");
        assert_eq!(read.frames, ["https://frames.example.com"]);
        assert!(read.base_uris.is_empty(), "there is none");
    }

    fn paths(dir: &Path) -> Paths {
        Paths {
            data_dir: dir.to_path_buf(),
            worktrees_root: dir.join("worktrees"),
            claude_config: dir.join("claude.json"),
            codex_config: dir.join("codex.toml"),
            opencode_config_dir: dir.join("opencode"),
        }
    }

    fn app(tool_use_id: &str, html: &str) -> AgentEvent {
        AgentEvent::App {
            tool_use_id: tool_use_id.to_string(),
            server: "demo".into(),
            tool: "chart".into(),
            uri: "ui://demo/chart".into(),
            document: None,
            note: None,
            csp: AppCsp::default(),
            permissions: vec![],
            prefers_border: None,
            callable: vec![],
            result: None,
            html: Some(html.to_string()),
        }
    }

    fn document_of(event: &AgentEvent) -> Option<PathBuf> {
        match event {
            AgentEvent::App { document, html, .. } => {
                assert!(html.is_none(), "the HTML is not passed on");
                document.clone()
            }
            AgentEvent::Sub { event, .. } => document_of(event),
            _ => None,
        }
    }

    #[test]
    fn an_app_is_kept_once_in_the_sessions_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        let paths = paths(dir.path());
        let first = kept(&paths, 4, app("t1", "<p>chart</p>"));
        let again = kept(&paths, 4, app("t2", "<p>chart</p>"));
        let other = kept(
            &paths,
            4,
            AgentEvent::Sub {
                parent: "agent".into(),
                event: Box::new(app("t3", "<p>other</p>")),
            },
        );
        let first = document_of(&first).expect("kept");
        assert_eq!(document_of(&again), Some(first.clone()));
        let other = document_of(&other).expect("kept");
        assert_ne!(other, first);
        assert_eq!(first.parent(), Some(paths.attachments(4).as_path()));
        assert_eq!(
            std::fs::read_to_string(&first).expect("reads"),
            "<p>chart</p>"
        );
        let mode = |path: &Path| std::fs::metadata(path).expect("there").permissions().mode();
        assert_eq!(mode(&first) & 0o777, 0o600);
        assert_eq!(mode(&paths.attachments(4)) & 0o777, 0o700);
        // The serialised event never carries the HTML.
        let json =
            serde_json::to_string(&kept(&paths, 4, app("t1", "<p>chart</p>"))).expect("serialises");
        assert!(!json.contains("<p>chart</p>"));
    }

    #[test]
    fn a_document_of_the_same_name_and_other_contents_is_not_written_over() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("attachments").join("1");
        private_folder(&folder).expect("folder");
        let taken = folder.join(name_of("<p>mine</p>", 0));
        std::fs::write(&taken, "<p>someone else's</p>").expect("writes");
        let path = keep(&folder, "<p>mine</p>").expect("kept");
        assert_eq!(path, folder.join(name_of("<p>mine</p>", 1)));
        assert_eq!(
            std::fs::read_to_string(&taken).expect("reads"),
            "<p>someone else's</p>"
        );
    }

    #[test]
    fn only_a_kept_app_right_in_the_folder_is_served() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("attachments").join("2");
        let kept = keep(&folder, "<p>app</p>").expect("kept");
        assert_eq!(
            inside(&folder, &kept).expect("served"),
            kept.canonicalize().expect("real")
        );
        let image = folder.join("pasted-1.png");
        std::fs::write(&image, b"png").expect("writes");
        assert!(inside(&folder, &image).is_err(), "not an app");
        let outside = dir.path().join("app-0000000000000000.html");
        std::fs::write(&outside, "<p>elsewhere</p>").expect("writes");
        assert!(inside(&folder, &outside).is_err());
        let sneaky = folder.join("..").join("app-0000000000000000.html");
        assert!(inside(&folder, &sneaky).is_err());
    }

    #[test]
    fn the_same_app_keeps_its_address_and_old_ones_make_room() {
        let id = served_id(1, "t1", Path::new("/a"), "p");
        assert_eq!(id, served_id(1, "t1", Path::new("/a"), "p"));
        assert_ne!(id, served_id(1, "t2", Path::new("/a"), "p"));
        assert_ne!(id, served_id(2, "t1", Path::new("/a"), "p"));
        let mut served = Served::default();
        served.put(id.clone(), (PathBuf::from("/a"), "p".into()));
        for n in 0..MAX_SERVED {
            served.put(format!("{n}"), (PathBuf::from("/b"), "p".into()));
        }
        assert_eq!(served.apps.len(), MAX_SERVED);
        assert!(!served.apps.contains_key(&id));
    }

    #[test]
    fn an_answer_reaches_whoever_waits_for_it_and_an_agent_that_went_answers_nobody() {
        let calls = AppCalls::default();
        let (call, mut answered) = calls.begin(7);
        calls.answer(7, call + 1, (Some(Value::Null), None));
        assert!(answered.try_recv().is_err(), "not this call's answer");
        calls.answer(7, call, (Some(serde_json::json!({"content": []})), None));
        assert_eq!(
            answered.try_recv().expect("answered"),
            (Some(serde_json::json!({"content": []})), None)
        );
        let (_, mut gone) = calls.begin(7);
        calls.end(7);
        assert!(matches!(
            gone.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
    }
}
