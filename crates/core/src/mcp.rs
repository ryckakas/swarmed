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
//! Server definitions hold secrets (tokens in `env` and `headers`). They are passed on and
//! never logged, which is why [`McpServer`]'s `Debug` output leaves the values out.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

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
    let user_file = read_json(claude_config);
    let project_file = read_json(&worktree.join(".mcp.json"));
    let local = format!(
        "/projects/{}/mcpServers",
        pointer_escape(&repo.to_string_lossy())
    );
    let ordered = [
        user_file.get("mcpServers"),
        project_file.get("mcpServers"),
        user_file.pointer(&local),
    ];

    let mut by_name: BTreeMap<String, McpServer> = BTreeMap::new();
    let entries = ordered
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .flatten();
    for (name, definition) in entries {
        if let Some(server) = parse_server(name, definition) {
            by_name.insert(name.clone(), server);
        }
    }
    by_name.into_values().collect()
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
