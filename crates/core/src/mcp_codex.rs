//! Codex's own MCP servers, read from its configuration, never changed.
//!
//! Codex keeps them under `[mcp_servers.<name>]` in `~/.codex/config.toml`. They are read
//! twice: to be shown, where what a server is given in secret (environment values, headers)
//! is named and never read out; and to be handed to the other agents, where the values go
//! with the server and nowhere else.

use std::path::Path;

use toml::{Table, Value};

use std::collections::{BTreeMap, BTreeSet};

use crate::mcp::{McpServer, McpTransport};
use crate::mcp_own::{McpServerView, quoted};

/// What a front end knows Codex's servers by.
pub(crate) const ORIGIN: &str = "codex";

/// The servers in Codex's configuration at `path`, by name. None if there is no such file or
/// it cannot be read: this is a look at somebody else's file, not something to fail over.
pub(crate) fn codex_views(path: &Path) -> Vec<McpServerView> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let config: Table = toml::from_str(&text).unwrap_or_default();
    let servers = config.get("mcp_servers").and_then(Value::as_table);
    let view = |(name, entry): (&String, &Value)| Some(view_of(name, entry.as_table()?));
    servers.map_or_else(Vec::new, |servers| {
        servers.iter().filter_map(view).collect()
    })
}

/// The names of all the servers Codex has, switched on or off.
pub(crate) fn codex_names(path: &Path) -> BTreeSet<String> {
    let views = codex_views(path);
    views.into_iter().map(|view| view.name).collect()
}

/// The servers in Codex's configuration at `path` that are switched on, as another agent
/// can be given them.
pub(crate) fn codex_servers(path: &Path) -> Vec<McpServer> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let config: Table = toml::from_str(&text).unwrap_or_default();
    let servers = config.get("mcp_servers").and_then(Value::as_table);
    let server = |(name, entry): (&String, &Value)| server_of(name, entry.as_table()?);
    servers.map_or_else(Vec::new, |servers| {
        servers.iter().filter_map(server).collect()
    })
}

fn server_of(name: &str, entry: &Table) -> Option<McpServer> {
    if entry.get("enabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let text = |key: &str| entry.get(key).and_then(Value::as_str);
    let transport = match (text("command"), text("url")) {
        (Some(command), _) => McpTransport::Stdio {
            command: command.to_string(),
            args: strings(entry.get("args")),
            env: pairs(entry.get("env")),
        },
        (None, Some(url)) => McpTransport::Http {
            url: url.to_string(),
            headers: headers(entry),
        },
        (None, None) => return None,
    };
    Some(McpServer {
        name: name.to_string(),
        transport,
    })
}

/// The headers Codex would send: the ones written out, the ones it reads from variables, and
/// the token it reads from one. A variable that is not set gives no header.
fn headers(entry: &Table) -> BTreeMap<String, String> {
    let mut headers = pairs(entry.get("http_headers"));
    for (header, variable) in pairs(entry.get("env_http_headers")) {
        headers.extend(std::env::var(variable).ok().map(|value| (header, value)));
    }
    let token = entry.get("bearer_token_env_var").and_then(Value::as_str);
    let token = token.and_then(|variable| std::env::var(variable).ok());
    headers.extend(token.map(|token| ("Authorization".to_string(), format!("Bearer {token}"))));
    headers
}

fn strings(value: Option<&Value>) -> Vec<String> {
    let values = value.and_then(Value::as_array).into_iter().flatten();
    values.filter_map(Value::as_str).map(String::from).collect()
}

fn pairs(value: Option<&Value>) -> BTreeMap<String, String> {
    let table = value.and_then(Value::as_table).into_iter().flatten();
    table
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
        .collect()
}

fn view_of(name: &str, entry: &Table) -> McpServerView {
    let text = |key: &str| entry.get(key).and_then(Value::as_str);
    let (kind, target) = match (text("command"), text("url")) {
        (Some(command), _) => ("stdio", command_line(command, entry.get("args"))),
        (None, Some(url)) => ("http", url.to_string()),
        (None, None) => ("", String::new()),
    };
    McpServerView {
        name: name.to_string(),
        origin: ORIGIN.to_string(),
        kind: kind.to_string(),
        target,
        secrets: secret_names(entry),
        enabled: entry.get("enabled").and_then(Value::as_bool) != Some(false),
        status: None,
    }
}

/// The program and its arguments on one line.
fn command_line(command: &str, args: Option<&Value>) -> String {
    let args = args.and_then(Value::as_array).into_iter().flatten();
    let words = std::iter::once(command).chain(args.filter_map(Value::as_str));
    words.map(quoted).collect::<Vec<_>>().join(" ")
}

/// The names of what the server is given in secret: its environment variables, its headers,
/// and the variable its token is read from. Never the values.
fn secret_names(entry: &Table) -> Vec<String> {
    let keys = |table: &str| {
        let table = entry.get(table).and_then(Value::as_table);
        table.into_iter().flat_map(|table| table.keys().cloned())
    };
    let token = entry.get("bearer_token_env_var").and_then(Value::as_str);
    keys("env")
        .chain(keys("http_headers"))
        .chain(token.map(String::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_servers_are_listed_with_names_of_secrets_and_never_their_values() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.toml");
        let config = r#"
model = "gpt-a"

[mcp_servers.tracker]
command = "/usr/local/bin/npx"
args = ["-y", "@acme/tracker mcp"]
[mcp_servers.tracker.env]
TRACKER_TOKEN = "sk-secret"
[mcp_servers.tracker.tools.search]
approval = "never"

[mcp_servers.docs]
url = "https://example.com/mcp"
bearer_token_env_var = "DOCS_TOKEN"
enabled = false
[mcp_servers.docs.http_headers]
X-Team = "sk-header"
"#;
        std::fs::write(&path, config).expect("writes");
        let views = codex_views(&path);
        let names: Vec<&str> = views.iter().map(|view| view.name.as_str()).collect();
        assert_eq!(names, ["docs", "tracker"]);
        let (docs, tracker) = (&views[0], &views[1]);
        assert_eq!((docs.kind.as_str(), docs.enabled), ("http", false));
        assert_eq!(docs.target, "https://example.com/mcp");
        assert_eq!(docs.secrets, ["X-Team", "DOCS_TOKEN"]);
        assert_eq!((tracker.kind.as_str(), tracker.enabled), ("stdio", true));
        assert_eq!(
            tracker.target,
            "/usr/local/bin/npx -y \"@acme/tracker mcp\""
        );
        assert_eq!(tracker.secrets, ["TRACKER_TOKEN"]);
        assert!(views.iter().all(|view| view.origin == "codex"));
        assert!(!format!("{views:?}").contains("sk-"));
    }

    #[test]
    fn the_servers_switched_on_are_read_whole_for_other_agents() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.toml");
        let config = r#"
[mcp_servers.tracker]
command = "npx"
args = ["-y", "tracker"]
[mcp_servers.tracker.env]
TRACKER_TOKEN = "sk-secret"

[mcp_servers.docs]
url = "https://example.com/mcp"
bearer_token_env_var = "SWARMED_NO_SUCH_VARIABLE"
[mcp_servers.docs.http_headers]
X-Team = "sk-header"

[mcp_servers.off]
command = "off"
enabled = false
"#;
        std::fs::write(&path, config).expect("writes");
        let servers = codex_servers(&path);
        let names: Vec<&str> = servers.iter().map(|server| server.name.as_str()).collect();
        assert_eq!(names, ["docs", "tracker"]);
        let one = |key: &str, value: &str| BTreeMap::from([(key.to_string(), value.to_string())]);
        let docs = McpTransport::Http {
            url: "https://example.com/mcp".into(),
            // The token's variable is not set, so there is no header for it.
            headers: one("X-Team", "sk-header"),
        };
        let tracker = McpTransport::Stdio {
            command: "npx".into(),
            args: vec!["-y".into(), "tracker".into()],
            env: one("TRACKER_TOKEN", "sk-secret"),
        };
        assert_eq!(servers[0].transport, docs);
        assert_eq!(servers[1].transport, tracker);
        // Handed on, and still never printed.
        assert!(!format!("{servers:?}").contains("sk-"));
    }

    #[test]
    fn no_file_or_a_broken_one_is_no_servers() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(codex_views(&dir.path().join("none.toml")), vec![]);
        let broken = dir.path().join("config.toml");
        std::fs::write(&broken, "[mcp_servers").expect("writes");
        assert_eq!(codex_views(&broken), vec![]);
    }
}
