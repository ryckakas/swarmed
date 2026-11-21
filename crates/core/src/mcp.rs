//! The user's MCP servers, as Claude Code has them configured.
//!
//! Claude Code reads these files itself. Other agents do not, so Swarmed reads them and hands
//! the servers to those agents when a session starts. Three places hold servers, and a name in
//! a later one replaces the same name in an earlier one, as in Claude Code:
//!
//! 1. the user's own, under `mcpServers` in `~/.claude.json`;
//! 2. the repository's, in `.mcp.json` at the top of the checkout;
//! 3. the user's for that repository, under `projects.<path>.mcpServers` in `~/.claude.json`.
//!
//! The second place is not the user's own. Whoever wrote the branch that is checked out
//! wrote that file, and a server in it is a command to run. Claude Code asks before it uses
//! one, and writes the answer down; Swarmed asks nothing, so it hands such a server to
//! another agent only where that answer is yes (`claude_code_servers_to_share`).
//!
//! Server definitions hold secrets (tokens in `env` and `headers`). They are passed on and
//! never logged, which is why [`McpServer`]'s `Debug` output leaves the values out.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

/// How to reach one MCP server.
#[derive(Clone, PartialEq, Eq)]
pub enum McpTransport {
    /// A program started by the agent, spoken to over its stdio.
    Stdio {
        /// The executable.
        command: String,
        /// Its arguments.
        args: Vec<String>,
        /// Extra environment variables for it.
        env: BTreeMap<String, String>,
    },
    /// A server reached over HTTP.
    Http {
        /// The server's address.
        url: String,
        /// Headers to send with every request, usually credentials.
        headers: BTreeMap<String, String>,
    },
}

/// One MCP server to give to an agent.
#[derive(Clone, PartialEq, Eq)]
pub struct McpServer {
    /// The name its tools appear under.
    pub name: String,
    /// How to reach it.
    pub transport: McpTransport,
}

impl fmt::Debug for McpTransport {
    /// Names the kind, where it points and which variables it sets, never their values.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdio { command, args, env } => {
                let keys: Vec<&String> = env.keys().collect();
                write!(
                    f,
                    "stdio `{command}` with {} argument(s), sets {keys:?}",
                    args.len()
                )
            }
            Self::Http { url, headers } => {
                let keys: Vec<&String> = headers.keys().collect();
                write!(f, "http {url}, sends {keys:?}")
            }
        }
    }
}

impl fmt::Debug for McpServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "McpServer({}: {:?})", self.name, self.transport)
    }
}

/// The servers Claude Code would load for a session in `worktree` of the repository at `repo`.
///
/// A file that is missing or cannot be parsed contributes nothing: a broken MCP config must
/// not stop a session from starting.
#[must_use]
pub fn claude_code_servers(claude_config: &Path, repo: &Path, worktree: &Path) -> Vec<McpServer> {
    servers(claude_config, repo, worktree, None)
}

/// The servers of Claude Code's that a session of another agent in `worktree` may be given:
/// the user's own, and of the repository's `.mcp.json` only those the user has approved in
/// Claude Code and has not rejected or switched off there.
///
/// One that is not approved is left out as if the file did not name it, so a server of the
/// user's own by the same name is still given.
pub(crate) fn claude_code_servers_to_share(
    claude_config: &Path,
    repo: &Path,
    worktree: &Path,
) -> Vec<McpServer> {
    let approvals = Approvals::read(claude_config, repo, worktree);
    servers(claude_config, repo, worktree, Some(&approvals))
}

/// The servers from the three places. With `approvals`, the repository's are only those
/// approved; a definition that is not is not even read, so nothing of this process's
/// environment is filled into it.
fn servers(
    claude_config: &Path,
    repo: &Path,
    worktree: &Path,
    approvals: Option<&Approvals>,
) -> Vec<McpServer> {
    let user_file = read_json(claude_config);
    let project_file = read_json(&worktree.join(".mcp.json"));
    let local = format!("{}/mcpServers", project_pointer(repo));
    let named = |servers: Option<&Value>| -> Vec<(String, Value)> {
        let entries = servers.and_then(Value::as_object).into_iter().flatten();
        entries
            .map(|(name, definition)| (name.clone(), definition.clone()))
            .collect()
    };
    let mut of_repository = named(project_file.get("mcpServers"));
    of_repository.retain(|(name, _)| approvals.is_none_or(|approvals| approvals.allow(name)));
    let ordered = [
        named(user_file.get("mcpServers")),
        of_repository,
        named(user_file.pointer(&local)),
    ];

    let mut by_name: BTreeMap<String, McpServer> = BTreeMap::new();
    for (name, definition) in ordered.into_iter().flatten() {
        if let Some(server) = parse_server(&name, &definition) {
            by_name.insert(name, server);
        }
    }
    by_name.into_values().collect()
}

/// Where Claude Code keeps what it knows of the folder `folder`, in its own config file.
fn project_pointer(folder: &Path) -> String {
    format!("/projects/{}", pointer_escape(&folder.to_string_lossy()))
}

/// What the user has told Claude Code about the servers of a repository's `.mcp.json`.
///
/// Claude Code writes an approval into `.claude/settings.local.json` of the folder it was
/// asked in (`enabledMcpjsonServers`, or `enableAllProjectMcpServers` for all of them), and
/// a rejection there as `disabledMcpjsonServers`. Earlier versions kept the same keys under
/// the folder's entry in `~/.claude.json`, where a server switched off by hand is also
/// noted (`disabledMcpServers`). The user's own `~/.claude/settings.json` may hold them too.
///
/// An approval only counts from a file the repository cannot have brought along: not from
/// `.claude/settings.json`, which is checked in, and from `settings.local.json` only if git
/// does not track it. A rejection counts from anywhere, and goes before any approval.
#[derive(Debug, Default)]
struct Approvals {
    /// Whether every server of the repository's is approved.
    all: bool,
    /// Whether some file says that not every server is: that goes before one that says so.
    not_all: bool,
    approved: BTreeSet<String>,
    rejected: BTreeSet<String>,
}

impl Approvals {
    /// What is written down for a session in `worktree` of the repository at `repo`. Both
    /// folders are asked: the user may have answered Claude Code in either.
    fn read(claude_config: &Path, repo: &Path, worktree: &Path) -> Self {
        let mut found = Self::default();
        let user_file = read_json(claude_config);
        if let Some(home) = claude_config.parent() {
            found.take(&read_json(&home.join(".claude/settings.json")), true);
        }
        for folder in [repo, worktree] {
            let entry = user_file.pointer(&project_pointer(folder));
            let entry = entry.unwrap_or(&Value::Null);
            found.take(entry, true);
            found.rejected.extend(names(entry, "disabledMcpServers"));
            let settings = folder.join(".claude");
            found.take(&read_json(&settings.join("settings.json")), false);
            let local = read_json(&settings.join("settings.local.json"));
            let own = approves(&local) && untracked(folder, ".claude/settings.local.json");
            found.take(&local, own);
        }
        found
    }

    /// Adds what one file says. Its approvals count only if it is `trusted`.
    fn take(&mut self, settings: &Value, trusted: bool) {
        self.rejected
            .extend(names(settings, "disabledMcpjsonServers"));
        let all = settings.get("enableAllProjectMcpServers");
        self.not_all |= all.and_then(Value::as_bool) == Some(false);
        if trusted {
            self.all |= all.and_then(Value::as_bool) == Some(true);
            self.approved
                .extend(names(settings, "enabledMcpjsonServers"));
        }
    }

    /// Whether the repository's server `name` may be given to another agent.
    fn allow(&self, name: &str) -> bool {
        let approved = (self.all && !self.not_all) || self.approved.contains(name);
        approved && !self.rejected.contains(name)
    }
}

/// The names listed under `key` in a settings file.
fn names(settings: &Value, key: &str) -> Vec<String> {
    let listed = settings.get(key).and_then(Value::as_array);
    let names = listed.into_iter().flatten().filter_map(Value::as_str);
    names.map(String::from).collect()
}

/// Whether a settings file approves anything at all.
fn approves(settings: &Value) -> bool {
    let all = settings.get("enableAllProjectMcpServers");
    all.and_then(Value::as_bool) == Some(true)
        || !names(settings, "enabledMcpjsonServers").is_empty()
}

/// Whether git, asked in `folder`, says that it does not track `file`. Anything else, git
/// not answering included, counts as tracked: the file is then not known to be the user's.
///
/// Asked without Tokio: sessions are started from threads that have no runtime too.
fn untracked(folder: &Path, file: &str) -> bool {
    let asked = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(["ls-files", "--error-unmatch", "--", file])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // 1 is git's "no such file among those I track"; a folder that is no repository gives 128.
    asked.is_ok_and(|status| status.code() == Some(1))
}

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

/// Escapes a key for use in a JSON pointer, where `~` and `/` have a meaning of their own.
fn pointer_escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// One server as Claude Code's files write it, if it is of a kind every agent takes.
pub(crate) fn parse_server(name: &str, definition: &Value) -> Option<McpServer> {
    let text = |key: &str| definition.get(key).and_then(Value::as_str).map(expand);
    let map = |key: &str| {
        let entries = definition.get(key).and_then(Value::as_object);
        let pairs = entries.into_iter().flatten();
        pairs
            .filter_map(|(k, v)| Some((k.clone(), expand(v.as_str()?))))
            .collect()
    };
    let kind = definition
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("stdio");
    let transport = match kind {
        "stdio" => {
            let args = definition.get("args").and_then(Value::as_array);
            let args = args
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(expand)
                .collect();
            McpTransport::Stdio {
                command: text("command")?,
                args,
                env: map("env"),
            }
        }
        "http" | "streamable-http" => McpTransport::Http {
            url: text("url")?,
            headers: map("headers"),
        },
        // Server-sent events and anything newer: not every agent takes them, so they are left out.
        _ => return None,
    };
    Some(McpServer {
        name: name.to_string(),
        transport,
    })
}

/// Fills in `${VAR}` and `${VAR:-default}` from the environment, as Claude Code does in these
/// files. An unset variable without a default becomes empty.
fn expand(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        let Some(length) = rest[start..].find('}') else {
            break;
        };
        out.push_str(&rest[..start]);
        let inside = &rest[start + 2..start + length];
        let (name, default) = inside.split_once(":-").unwrap_or((inside, ""));
        out.push_str(&std::env::var(name).unwrap_or_else(|_| default.to_string()));
        rest = &rest[start + length + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn write(path: &Path, value: &Value) {
        std::fs::write(path, value.to_string()).expect("writes");
    }

    fn names(servers: &[McpServer]) -> Vec<&str> {
        servers.iter().map(|server| server.name.as_str()).collect()
    }

    #[test]
    fn servers_come_from_three_places_and_the_nearest_wins() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (repo, worktree) = (dir.path().join("repo"), dir.path().join("worktree"));
        std::fs::create_dir_all(&worktree).expect("creates");
        let config = dir.path().join("claude.json");
        let stdio = |command: &str| json!({ "command": command });
        write(
            &config,
            &json!({
                "mcpServers": { "everywhere": stdio("user"), "shadowed": stdio("user") },
                "projects": { repo.to_string_lossy(): { "mcpServers": { "mine": stdio("local"), "shadowed": stdio("local") } } },
            }),
        );
        write(
            &worktree.join(".mcp.json"),
            &json!({ "mcpServers": { "team": stdio("project"), "shadowed": stdio("project") } }),
        );

        let servers = claude_code_servers(&config, &repo, &worktree);
        assert_eq!(names(&servers), ["everywhere", "mine", "shadowed", "team"]);
        let shadowed = servers
            .iter()
            .find(|server| server.name == "shadowed")
            .expect("present");
        assert!(
            matches!(&shadowed.transport, McpTransport::Stdio { command, .. } if command == "local")
        );

        let elsewhere = claude_code_servers(
            &config,
            &dir.path().join("other"),
            &dir.path().join("other"),
        );
        assert_eq!(names(&elsewhere), ["everywhere", "shadowed"]);
    }

    /// A repository whose `.mcp.json` names `team`, `shadowed`, `other` and `off`, with a
    /// Claude Code config in which the user has a `shadowed` of their own.
    fn shared_fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let worktree = dir.path().join("worktree");
        std::fs::create_dir_all(worktree.join(".claude")).expect("creates");
        let run = |args: &[&str]| {
            let git = Command::new("git")
                .arg("-C")
                .arg(&worktree)
                .args(args)
                .output();
            assert!(git.expect("git runs").status.success(), "git {args:?}");
        };
        run(&["init", "--quiet"]);
        let stdio = |command: &str| json!({ "command": command });
        let project = json!({ "mcpServers": {
            "team": stdio("project"), "shadowed": stdio("project"),
            "other": stdio("project"), "off": stdio("project"),
        } });
        write(&worktree.join(".mcp.json"), &project);
        let config = dir.path().join("claude.json");
        write(
            &config,
            &json!({ "mcpServers": { "shadowed": stdio("user") } }),
        );
        (dir, config, worktree)
    }

    fn shared(config: &Path, worktree: &Path) -> Vec<(String, String)> {
        let command = |server: McpServer| match server.transport {
            McpTransport::Stdio { command, .. } => (server.name, command),
            McpTransport::Http { url, .. } => (server.name, url),
        };
        let servers = claude_code_servers_to_share(config, worktree, worktree);
        servers.into_iter().map(command).collect()
    }

    fn pair(name: &str, command: &str) -> (String, String) {
        (name.to_string(), command.to_string())
    }

    #[test]
    fn a_server_of_the_repository_is_shared_only_once_the_user_approved_it() {
        let (_dir, config, worktree) = shared_fixture();
        // Nothing is approved: only the user's own is given, under a name the file has too.
        assert_eq!(shared(&config, &worktree), [pair("shadowed", "user")]);
        // Claude Code itself is still told of them all, to ask about in its own way.
        let all = claude_code_servers(&config, &worktree, &worktree);
        assert_eq!(names(&all), ["off", "other", "shadowed", "team"]);

        // Approved in Claude Code, which writes it into a file git does not track.
        let local = worktree.join(".claude/settings.local.json");
        write(
            &local,
            &json!({ "enabledMcpjsonServers": ["team", "shadowed", "off"], "disabledMcpjsonServers": ["off"] }),
        );
        assert_eq!(
            shared(&config, &worktree),
            [pair("shadowed", "project"), pair("team", "project")]
        );
    }

    #[test]
    fn a_repository_cannot_approve_its_own_servers() {
        let (_dir, config, worktree) = shared_fixture();
        let everything =
            json!({ "enableAllProjectMcpServers": true, "enabledMcpjsonServers": ["team"] });
        // The settings that are checked in are the repository's word, not the user's.
        write(&worktree.join(".claude/settings.json"), &everything);
        assert_eq!(shared(&config, &worktree), [pair("shadowed", "user")]);
        // Nor does the local file count once git tracks it: then a branch brought it.
        let local = worktree.join(".claude/settings.local.json");
        write(&local, &everything);
        let add = Command::new("git")
            .arg("-C")
            .arg(&worktree)
            .args(["add", "--force", ".claude/settings.local.json"])
            .output();
        assert!(add.expect("git runs").status.success());
        assert_eq!(shared(&config, &worktree), [pair("shadowed", "user")]);
    }

    #[test]
    fn approvals_in_the_users_own_files_count_and_a_rejection_anywhere_wins() {
        let (dir, config, worktree) = shared_fixture();
        // All of them, in the user's settings; one switched off for this folder, and one
        // rejected by the repository's own settings, which may always say no.
        std::fs::create_dir_all(dir.path().join(".claude")).expect("creates");
        write(
            &dir.path().join(".claude/settings.json"),
            &json!({ "enableAllProjectMcpServers": true }),
        );
        let stdio = |command: &str| json!({ "command": command });
        write(
            &config,
            &json!({
                "mcpServers": { "shadowed": stdio("user") },
                "projects": { worktree.to_string_lossy(): { "disabledMcpServers": ["off"] } },
            }),
        );
        write(
            &worktree.join(".claude/settings.json"),
            &json!({ "disabledMcpjsonServers": ["other"] }),
        );
        assert_eq!(
            shared(&config, &worktree),
            [pair("shadowed", "project"), pair("team", "project")]
        );

        // Approved by name in Claude Code's own file, as its earlier versions wrote it.
        std::fs::remove_file(dir.path().join(".claude/settings.json")).expect("removes");
        write(
            &config,
            &json!({ "projects": { worktree.to_string_lossy(): { "enabledMcpjsonServers": ["team"] } } }),
        );
        assert_eq!(shared(&config, &worktree), [pair("team", "project")]);
    }

    #[test]
    fn both_transports_are_read_and_others_are_left_out() {
        let http = parse_server(
            "docs",
            &json!({ "type": "http", "url": "https://x/mcp", "headers": { "Key": "secret" } }),
        );
        let expected = McpTransport::Http {
            url: "https://x/mcp".into(),
            headers: BTreeMap::from([("Key".to_string(), "secret".to_string())]),
        };
        assert_eq!(http.map(|server| server.transport), Some(expected));

        let stdio = parse_server(
            "db",
            &json!({ "type": "stdio", "command": "npx", "args": ["-y", "db"], "env": { "A": "1" } }),
        );
        assert!(matches!(
            stdio.map(|server| server.transport),
            Some(McpTransport::Stdio { command, args, env }) if command == "npx" && args == ["-y", "db"] && env["A"] == "1"
        ));

        assert_eq!(
            parse_server("old", &json!({ "type": "sse", "url": "https://x/sse" })),
            None
        );
        assert_eq!(parse_server("broken", &json!({ "type": "http" })), None);
    }

    #[test]
    fn missing_and_broken_files_contribute_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join(".mcp.json"), "{ not json").expect("writes");
        let servers = claude_code_servers(&dir.path().join("absent.json"), dir.path(), dir.path());
        assert_eq!(servers, []);
    }

    #[test]
    fn variables_are_filled_in_from_the_environment() {
        // HOME is set wherever the tests run; the other name is made up.
        let home = std::env::var("HOME").expect("HOME is set");
        assert_eq!(expand("${HOME}/bin"), format!("{home}/bin"));
        assert_eq!(
            expand("a ${SWARMED_NO_SUCH_VARIABLE:-fallback} b"),
            "a fallback b"
        );
        assert_eq!(expand("${SWARMED_NO_SUCH_VARIABLE}"), "");
        assert_eq!(expand("plain $HOME ${unclosed"), "plain $HOME ${unclosed");
    }

    #[test]
    fn debug_output_names_variables_but_not_their_values() {
        let server = parse_server(
            "docs",
            &json!({ "type": "http", "url": "https://x", "headers": { "Key": "secret" } }),
        );
        let shown = format!("{:?}", server.expect("parses"));
        assert!(
            shown.contains("Key") && !shown.contains("secret"),
            "{shown}"
        );
    }
}
