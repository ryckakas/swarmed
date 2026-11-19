//! OpenCode's own MCP servers, read from its configuration to be shown, never to be changed.
//!
//! OpenCode keeps them in `opencode.json` or `opencode.jsonc` in its configuration directory,
//! under `mcp.servers`, or straight under `mcp` in files written for its earlier versions. Only
//! the user's own file is read, not those of single projects. What a server is given in secret
//! (environment values, headers) is named here and never read out.

use std::path::Path;

use serde_json::{Map, Value};

use crate::mcp_own::{McpServerView, quoted};

/// What a front end knows OpenCode's servers by.
pub(crate) const ORIGIN: &str = "opencode";

/// The servers in OpenCode's configuration directory `dir`, by name. None if there is no
/// configuration or it cannot be read: this is a look at somebody else's file.
pub(crate) fn opencode_views(dir: &Path) -> Vec<McpServerView> {
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
    let text = read("opencode.json").or_else(|| read("opencode.jsonc"));
    let config: Value =
        serde_json::from_str(&plain_json(&text.unwrap_or_default())).unwrap_or_default();
    let Some(mcp) = config.get("mcp").and_then(Value::as_object) else {
        return vec![];
    };
    let servers = mcp.get("servers").and_then(Value::as_object).unwrap_or(mcp);
    let view = |(name, entry): (&String, &Value)| Some(view_of(name, entry.as_object()?));
    let mut views: Vec<McpServerView> = servers.iter().filter_map(view).collect();
    views.sort_by(|a, b| a.name.cmp(&b.name));
    views
}

fn view_of(name: &str, entry: &Map<String, Value>) -> McpServerView {
    let words = entry.get("command").and_then(Value::as_array);
    let url = entry.get("url").and_then(Value::as_str);
    let (kind, target) = match (words, url) {
        (Some(words), _) => ("stdio", command_line(words)),
        (None, Some(url)) => ("http", url.to_string()),
        (None, None) => ("", String::new()),
    };
    let flag = |key: &str| entry.get(key).and_then(Value::as_bool);
    McpServerView {
        name: name.to_string(),
        origin: ORIGIN.to_string(),
        kind: kind.to_string(),
        target,
        secrets: secret_names(entry),
        enabled: flag("enabled") != Some(false) && flag("disabled") != Some(true),
        status: None,
    }
}

/// The program and its arguments on one line.
fn command_line(words: &[Value]) -> String {
    let words = words.iter().filter_map(Value::as_str);
    words.map(quoted).collect::<Vec<_>>().join(" ")
}

/// The names of the server's environment variables and headers. Never the values.
fn secret_names(entry: &Map<String, Value>) -> Vec<String> {
    let keys = |table: &str| {
        let table = entry.get(table).and_then(Value::as_object);
        table.into_iter().flat_map(|table| table.keys().cloned())
    };
    keys("environment").chain(keys("headers")).collect()
}

/// `text` without what JSON with comments allows on top of JSON: `//` and `/* */` comments,
/// and a comma before a closing bracket. What stands inside strings is left alone.
fn plain_json(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text.chars().peekable();
    while let Some(c) = rest.next() {
        match (c, rest.peek().copied()) {
            ('"', _) => copy_string(&mut rest, &mut plain),
            ('/', Some('/')) => drop(rest.by_ref().find(|c| *c == '\n')),
            ('/', Some('*')) => skip_block(&mut rest),
            _ => plain.push(c),
        }
    }
    without_trailing_commas(&plain)
}

/// Copies a string that has just been opened, up to and with the quote that closes it.
fn copy_string(rest: &mut impl Iterator<Item = char>, plain: &mut String) {
    plain.push('"');
    let mut escaped = false;
    for c in rest {
        plain.push(c);
        if c == '"' && !escaped {
            return;
        }
        escaped = c == '\\' && !escaped;
    }
}

/// Passes over a block comment whose `/` has been read.
fn skip_block(rest: &mut impl Iterator<Item = char>) {
    let mut star = false;
    for c in rest {
        if star && c == '/' {
            return;
        }
        star = c == '*';
    }
}

/// Removes each comma that only blank space separates from a closing bracket. Run on text
/// whose comments are gone; a comma inside a string is never followed by a bare bracket only
/// when the string closes first, so strings are tracked here too.
fn without_trailing_commas(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text.chars().peekable();
    while let Some(c) = rest.next() {
        if c == '"' {
            copy_string(&mut rest, &mut plain);
            continue;
        }
        let mut ahead = rest.clone().skip_while(|next| next.is_whitespace());
        let closes = matches!(ahead.next(), Some('}' | ']'));
        if c != ',' || !closes {
            plain.push(c);
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_servers_are_read_from_json_with_comments() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = r#"{
  // The user's own servers.
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "tracker": {
        "type": "local",
        "command": ["npx", "-y", "@acme/tracker mcp"], /* quoted when shown */
        "environment": { "TRACKER_TOKEN": "sk-secret", },
      },
      "docs": {
        "type": "remote",
        "url": "https://example.com/mcp?a=1,}//not-a-comment",
        "headers": { "Authorization": "Bearer sk-header" },
        "enabled": false,
      },
    },
  },
}"#;
        std::fs::write(dir.path().join("opencode.jsonc"), config).expect("writes");
        let views = opencode_views(dir.path());
        let names: Vec<&str> = views.iter().map(|view| view.name.as_str()).collect();
        assert_eq!(names, ["docs", "tracker"]);
        let (docs, tracker) = (&views[0], &views[1]);
        assert_eq!((docs.kind.as_str(), docs.enabled), ("http", false));
        assert_eq!(docs.target, "https://example.com/mcp?a=1,}//not-a-comment");
        assert_eq!(docs.secrets, ["Authorization"]);
        assert_eq!(tracker.target, "npx -y \"@acme/tracker mcp\"");
        assert_eq!(tracker.secrets, ["TRACKER_TOKEN"]);
        assert!(views.iter().all(|view| view.origin == "opencode"));
        assert!(!format!("{views:?}").contains("sk-"));
    }

    #[test]
    fn the_earlier_layout_is_read_too_and_no_file_is_no_servers() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(opencode_views(dir.path()), vec![]);
        let config = r#"{ "mcp": { "old": { "type": "local", "command": ["old-mcp"] } } }"#;
        std::fs::write(dir.path().join("opencode.json"), config).expect("writes");
        let views = opencode_views(dir.path());
        assert_eq!(views.len(), 1);
        assert_eq!(
            (views[0].name.as_str(), views[0].target.as_str()),
            ("old", "old-mcp")
        );
        std::fs::write(dir.path().join("opencode.json"), "{ not json").expect("writes");
        assert_eq!(opencode_views(dir.path()), vec![]);
    }
}
