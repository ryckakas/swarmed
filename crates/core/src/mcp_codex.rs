//! Codex's own MCP servers, read from its configuration to be shown, never to be changed.
//!
//! Codex keeps them under `[mcp_servers.<name>]` in `~/.codex/config.toml`. What a server is
//! given in secret (environment values, headers) is named here and never read out.

use std::path::Path;

use toml::{Table, Value};

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
    fn no_file_or_a_broken_one_is_no_servers() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(codex_views(&dir.path().join("none.toml")), vec![]);
        let broken = dir.path().join("config.toml");
        std::fs::write(&broken, "[mcp_servers").expect("writes");
        assert_eq!(codex_views(&broken), vec![]);
    }
}
