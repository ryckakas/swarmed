//! The MCP servers that plugins installed in Claude Code bring.
//!
//! Claude Code starts these itself, under `plugin:<plugin>:<server>`. Other agents know
//! nothing of plugins, so Swarmed reads the servers and hands them over under their own
//! names. A plugin keeps them in `.mcp.json` at its top, in the manifest's `mcpServers`, or
//! in files the manifest names; packaged servers (`.mcpb`) are left out, since unpacking
//! them is Claude Code's business.
//!
//! Only plugins installed for the user count, and not the ones the user switched off.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::mcp::{McpServer, parse_server};

/// What a plugin's files write for the folder it is installed in.
const ROOT: &str = "${CLAUDE_PLUGIN_ROOT}";

/// The servers of every plugin in use, by name. Where two plugins name a server alike, the
/// plugin that comes later by its id has it.
pub(crate) fn plugin_servers(claude_home: &Path) -> Vec<McpServer> {
    let mut by_name = BTreeMap::new();
    for (_, root) in in_use(claude_home) {
        by_name.extend(servers_in(&root));
    }
    by_name.into_values().collect()
}

/// The servers of every plugin in use, each by the name Claude Code starts it under:
/// `plugin:<plugin>:<server>`.
pub(crate) fn plugin_servers_as_claude_code_names_them(claude_home: &Path) -> Vec<McpServer> {
    let mut named = Vec::new();
    for (plugin, root) in in_use(claude_home) {
        for (server, mut found) in servers_in(&root) {
            found.name = format!("plugin:{plugin}:{server}");
            named.push(found);
        }
    }
    named
}

/// The plugins in use, each by its name and where it is installed for the user.
fn in_use(claude_home: &Path) -> Vec<(String, PathBuf)> {
    let installed = read(&claude_home.join("plugins").join("installed_plugins.json"));
    let settings = read(&claude_home.join("settings.json"));
    let off = |id: &str| {
        settings
            .pointer("/enabledPlugins")
            .and_then(|all| all.get(id))
            == Some(&Value::Bool(false))
    };
    let plugins = installed.get("plugins").and_then(Value::as_object);
    let in_use = plugins.into_iter().flatten().filter(|(id, _)| !off(id));
    let mut found = Vec::new();
    for (id, installs) in in_use {
        for root in user_installs(installs).map(PathBuf::from) {
            found.push((plugin_name(id, &root), root));
        }
    }
    found
}

/// A plugin's name: as its manifest gives it, else its id without the marketplace.
fn plugin_name(id: &str, root: &Path) -> String {
    let manifest = read_in(root, ".claude-plugin/plugin.json");
    let named = manifest.get("name").and_then(Value::as_str);
    let short = id.split_once('@').map_or(id, |(name, _)| name);
    named.unwrap_or(short).to_string()
}

/// Where a plugin is installed for the user, as against for one project.
fn user_installs(installs: &Value) -> impl Iterator<Item = &str> {
    let for_user = |install: &&Value| install.get("scope").and_then(Value::as_str) == Some("user");
    let installs = installs.as_array().into_iter().flatten();
    installs
        .filter(for_user)
        .filter_map(|install| install.get("installPath")?.as_str())
}

/// The servers of the plugin installed in `root`: `.mcp.json` first, then what the manifest
/// says, a later name replacing the same earlier one, as in Claude Code.
fn servers_in(root: &Path) -> BTreeMap<String, McpServer> {
    let mut maps = vec![read_in(root, ".mcp.json")];
    let manifest = read_in(root, ".claude-plugin/plugin.json");
    let declared = manifest.get("mcpServers").cloned().unwrap_or_default();
    let declared = match declared {
        Value::Array(several) => several,
        one => vec![one],
    };
    for shape in declared {
        maps.push(match shape {
            Value::String(file) if is_json(&file) => read_in(root, &file),
            map @ Value::Object(_) => map,
            _ => Value::Null,
        });
    }
    let mut by_name = BTreeMap::new();
    for map in &maps {
        // The file may wrap its servers in `mcpServers` or list them at the top.
        let servers = map.get("mcpServers").unwrap_or(map).as_object();
        let parsed = servers
            .into_iter()
            .flatten()
            .filter_map(|(name, definition)| Some((name.clone(), parse_server(name, definition)?)));
        by_name.extend(parsed);
    }
    by_name
}

fn is_json(file: &str) -> bool {
    let extension = Path::new(file).extension();
    extension.is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

fn read(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

/// A file of the plugin in `root`, with the plugin's folder filled in where it is named. A
/// path that leaves the plugin is not read.
fn read_in(root: &Path, file: &str) -> Value {
    let inside = Path::new(file).components().all(|part| {
        matches!(
            part,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    });
    if !inside {
        return Value::Null;
    }
    let text = std::fs::read_to_string(root.join(file)).unwrap_or_default();
    // Filled in as it would stand inside a JSON string, so that an odd folder name cannot
    // break the file.
    let folder = Value::String(root.to_string_lossy().into_owned()).to_string();
    let folder = folder.trim_matches('"');
    serde_json::from_str(&text.replace(ROOT, folder)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::mcp::McpTransport;

    fn write(path: &Path, value: &Value) {
        std::fs::create_dir_all(path.parent().expect("has a folder")).expect("creates");
        std::fs::write(path, value.to_string()).expect("writes");
    }

    /// A Claude Code home with plugins `a` (two servers, one from the manifest), `b` (a file
    /// the manifest names), `off` (switched off) and `project` (not installed for the user).
    fn home(dir: &Path) {
        let root = |plugin: &str| dir.join("cache").join(plugin);
        let install =
            |plugin: &str, scope: &str| json!([{ "scope": scope, "installPath": root(plugin) }]);
        write(
            &dir.join("plugins/installed_plugins.json"),
            &json!({ "plugins": {
                "a@m": install("a", "user"), "b@m": install("b", "user"),
                "off@m": install("off", "user"), "project@m": install("project", "project"),
            }}),
        );
        write(
            &dir.join("settings.json"),
            &json!({ "enabledPlugins": { "a@m": true, "off@m": false } }),
        );
        let run =
            |command: &str| json!({ "command": command, "args": ["${CLAUDE_PLUGIN_ROOT}/x"] });
        write(
            &root("a").join(".mcp.json"),
            &json!({ "flat": run("flat"), "both": run("file") }),
        );
        write(
            &root("a").join(".claude-plugin/plugin.json"),
            &json!({ "mcpServers": { "both": run("manifest") } }),
        );
        write(
            &root("b").join(".claude-plugin/plugin.json"),
            &json!({ "mcpServers": ["./servers.json", "./packed.mcpb", "../a/.mcp.json"] }),
        );
        write(
            &root("b").join("servers.json"),
            &json!({ "mcpServers": { "named": { "type": "http", "url": "https://x/mcp" } } }),
        );
        write(
            &root("off").join(".mcp.json"),
            &json!({ "unwanted": run("off") }),
        );
        write(
            &root("project").join(".mcp.json"),
            &json!({ "elsewhere": run("project") }),
        );
    }

    #[test]
    fn servers_come_from_the_plugins_in_use_and_the_manifest_has_the_last_word() {
        let dir = tempfile::tempdir().expect("temp dir");
        home(dir.path());
        let servers = plugin_servers(dir.path());
        let names: Vec<&str> = servers.iter().map(|server| server.name.as_str()).collect();
        assert_eq!(names, ["both", "flat", "named"]);
        let folder = dir.path().join("cache/a").to_string_lossy().into_owned();
        let expected = McpTransport::Stdio {
            command: "manifest".into(),
            args: vec![format!("{folder}/x")],
            env: BTreeMap::new(),
        };
        assert_eq!(servers[0].transport, expected);
    }

    #[test]
    fn claude_code_names_a_plugins_servers_after_the_plugin() {
        let dir = tempfile::tempdir().expect("temp dir");
        home(dir.path());
        write(
            &dir.path().join("cache/b/.claude-plugin/plugin.json"),
            &json!({ "name": "bee", "mcpServers": ["./servers.json"] }),
        );
        let servers = plugin_servers_as_claude_code_names_them(dir.path());
        let mut names: Vec<&str> = servers.iter().map(|server| server.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["plugin:a:both", "plugin:a:flat", "plugin:bee:named"]
        );
    }

    #[test]
    fn no_plugins_or_a_broken_record_is_no_servers() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(plugin_servers(dir.path()), []);
        std::fs::create_dir_all(dir.path().join("plugins")).expect("creates");
        std::fs::write(
            dir.path().join("plugins/installed_plugins.json"),
            "{ not json",
        )
        .expect("writes");
        assert_eq!(plugin_servers(dir.path()), []);
    }
}
