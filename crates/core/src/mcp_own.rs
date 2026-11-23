//! MCP servers the user sets up in Swarmed itself, for every agent it runs.
//!
//! Claude Code has its own list of servers, which Swarmed reads and never writes. This is a
//! second list, kept in `mcp.toml` in the data directory, for servers the user wants in their
//! Swarmed sessions whichever agent runs them. A server's secrets, the values of environment
//! variables for a program or of headers for an address, are kept in the system's keychain:
//! the file only names them, and nothing here returns, prints or logs a value.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::agent::AgentEvent;
use crate::agents::CLAUDE;
use crate::error::{Error, Result};
use crate::lenient::{self, Problems};
use crate::mcp::{self, McpServer, McpTransport};
use crate::mcp_codex;
use crate::mcp_opencode;
use crate::toolserver::SERVER_NAME;

/// One entry of `mcp.toml`. A key it does not know is read past and named (`lenient`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct OwnServer {
    /// The program to start, for a server spoken to over its standard input and output.
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    /// That program's arguments.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    /// The address, for a server reached over HTTP.
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    /// The names of its secrets: environment variables for a program, headers for an address.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    secrets: Vec<String>,
    /// Whether sessions are given it. Off keeps the entry and its secrets for later.
    enabled: bool,
}

type OwnServers = BTreeMap<String, OwnServer>;

/// A server as a front end shows it. Never holds a secret's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct McpServerView {
    /// The name its tools appear under.
    pub name: String,
    /// Whose it is: `swarmed` for one set up here, `claude` for one of Claude Code's own,
    /// `codex` or `opencode` for one of that agent's own.
    pub origin: String,
    /// `stdio` for a program, `http` for an address, empty if Swarmed cannot tell.
    pub kind: String,
    /// The program with its arguments, or the address. Empty if Swarmed cannot tell.
    pub target: String,
    /// The names of the secrets it is given.
    pub secrets: Vec<String>,
    /// Whether sessions are given it.
    pub enabled: bool,
    /// What Claude Code last said of it, such as `connected` or `failed`, if it has. Other
    /// agents do not say.
    pub status: Option<String>,
}

/// A server as a front end hands it over to be saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct McpServerInput {
    /// The name its tools appear under.
    pub name: String,
    /// The program and its arguments on one line, for a server started as a program.
    #[serde(default)]
    pub command: String,
    /// The address, for a server reached over HTTP.
    #[serde(default)]
    pub url: String,
    /// Its secrets by name. A value of `None` keeps the one already stored.
    #[serde(default)]
    pub secrets: Vec<(String, Option<String>)>,
    /// Whether sessions are given it.
    pub enabled: bool,
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::Mcp(reason.into())
}

/// Where a server's secret is kept in the keychain.
fn secret_key(server: &str, secret: &str) -> String {
    format!("mcp:{server}:{secret}")
}

/// Takes a command line apart, one character at a time.
#[derive(Default)]
struct Splitter {
    words: Vec<String>,
    word: String,
    /// The quote a word is inside of, if it is inside one.
    quote: Option<char>,
    /// Whether a word has begun: an empty pair of quotes is a word too.
    started: bool,
}

impl Splitter {
    fn end_word(&mut self) {
        if self.started {
            self.words.push(std::mem::take(&mut self.word));
        }
        self.started = false;
    }

    fn take(&mut self, char: char) {
        match (self.quote, char) {
            (Some(open), _) if char == open => self.quote = None,
            (None, '"' | '\'') => (self.quote, self.started) = (Some(char), true),
            (None, _) if char.is_whitespace() => self.end_word(),
            _ => {
                self.word.push(char);
                self.started = true;
            }
        }
    }
}

/// A command line as its words. Quotes keep a word with spaces in it together.
fn words(line: &str) -> Vec<String> {
    let mut splitter = Splitter::default();
    line.chars().for_each(|char| splitter.take(char));
    splitter.end_word();
    splitter.words
}

/// A word as it has to be written to come back as itself from [`words`].
pub(crate) fn quoted(word: &str) -> String {
    if word.is_empty() || word.chars().any(char::is_whitespace) {
        format!("\"{word}\"")
    } else {
        word.to_string()
    }
}

fn valid_name(name: &str) -> bool {
    let fits = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_');
    !name.is_empty() && name.len() <= 64 && name.chars().all(fits)
}

/// The servers of `mcp.toml` that can be read. One that cannot is left out, and a file that is
/// not TOML gives none: every agent still starts, without them (`problems_in` says why).
fn read(path: &Path) -> Result<OwnServers> {
    Ok(read_with_problems(path)?.0)
}

/// The servers of `mcp.toml` that can be read, with what could not, said for the user.
fn read_with_problems(path: &Path) -> Result<(OwnServers, Problems)> {
    let Some(text) = lenient::read_if_there(path)? else {
        return Ok((OwnServers::new(), Vec::new()));
    };
    match text.parse::<toml::Table>() {
        Ok(table) => Ok(lenient::entries::<OwnServer>(table)),
        Err(error) => {
            let meanwhile = format!(
                "No server set up in Swarmed is given to agents until it is fixed, and saving a \
                 server keeps a copy of it as `{}`.",
                lenient::unreadable_name(path)
            );
            Ok((
                OwnServers::new(),
                vec![lenient::not_toml(&text, &error, &meanwhile)],
            ))
        }
    }
}

/// What of the `mcp.toml` at `path` cannot be understood, said for the user.
pub(crate) fn problems_in(path: &Path) -> Result<Problems> {
    Ok(read_with_problems(path)?.1)
}

fn write(path: &Path, servers: &OwnServers) -> Result<()> {
    let text = toml::to_string(servers).map_err(|e| Error::Config {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    lenient::keep_if_unreadable(path)?;
    let header = "# MCP servers set up in Swarmed, given to every agent it runs. Secrets are \
                  named here and kept in the keychain.\n\n";
    Ok(std::fs::write(path, format!("{header}{text}"))?)
}

impl OwnServer {
    fn kind(&self) -> &'static str {
        if self.url.is_some() { "http" } else { "stdio" }
    }

    fn target(&self) -> String {
        if let Some(url) = &self.url {
            return url.clone();
        }
        let program = self.command.iter().chain(&self.args);
        program
            .map(|word| quoted(word))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What a front end handed over, as an entry; an error says what is wrong with it.
fn entry_from(input: &McpServerInput) -> Result<OwnServer> {
    let name = input.name.trim();
    if !valid_name(name) {
        return Err(invalid(
            "a server's name is made of letters, digits, - and _",
        ));
    }
    if name == SERVER_NAME || name == CLAUDE {
        return Err(invalid(format!("`{name}` is a name Swarmed uses itself")));
    }
    let mut program = words(&input.command).into_iter();
    let command = program.next();
    let url = Some(input.url.trim().to_string()).filter(|url| !url.is_empty());
    if command.is_some() == url.is_some() {
        return Err(invalid(
            "a server is either a command to start or an address, one of the two",
        ));
    }
    let secrets: Vec<String> = input
        .secrets
        .iter()
        .map(|(secret, _)| secret.trim().to_string())
        .filter(|secret| !secret.is_empty())
        .collect();
    Ok(OwnServer {
        command,
        args: program.collect(),
        url,
        secrets,
        enabled: input.enabled,
    })
}

impl Core {
    /// Every MCP server Swarmed knows of: the ones set up here first, then Claude Code's own,
    /// each with what Claude Code last said of it. Secrets are named, never shown.
    pub fn mcp_servers(&self) -> Result<Vec<McpServerView>> {
        let status = self.mcp_statuses()?;
        let own = read(&self.paths.mcp_config())?;
        let mut views: Vec<McpServerView> = own
            .iter()
            .map(|(name, server)| McpServerView {
                name: name.clone(),
                origin: "swarmed".to_string(),
                kind: server.kind().to_string(),
                target: server.target(),
                secrets: server.secrets.clone(),
                enabled: server.enabled,
                status: status.get(name).cloned(),
            })
            .collect();
        views.extend(self.claude_views(&status, &own));
        views.extend(mcp_codex::codex_views(&self.paths.codex_config));
        views.extend(mcp_opencode::opencode_views(
            &self.paths.opencode_config_dir,
        ));
        Ok(views)
    }

    /// What Claude Code said of each server when one of its sessions last started.
    fn mcp_statuses(&self) -> Result<BTreeMap<String, String>> {
        let init = self.store().latest_init(None, CLAUDE)?;
        let Some(AgentEvent::Init { mcp_servers, .. }) = init else {
            return Ok(BTreeMap::new());
        };
        let pair = |server: crate::agent::McpServerInfo| (server.name, server.status);
        Ok(mcp_servers.into_iter().map(pair).collect())
    }

    /// Claude Code's own servers: those in its configuration, with what Swarmed can read of
    /// them, and those it only reported, such as connectors of the user's Claude account.
    fn claude_views(
        &self,
        status: &BTreeMap<String, String>,
        own: &OwnServers,
    ) -> Vec<McpServerView> {
        let nowhere = Path::new("");
        let configured = mcp::claude_code_servers(&self.paths.claude_config, nowhere, nowhere);
        let mut views: BTreeMap<String, McpServerView> = BTreeMap::new();
        for server in configured {
            let (kind, target, secrets) = described(&server.transport);
            let view = claude_view(&server.name, kind, target, secrets, status);
            views.insert(server.name, view);
        }
        // What Claude Code was given of another agent's is listed under that agent.
        let codex = mcp_codex::codex_names(&self.paths.codex_config);
        let opencode = mcp_opencode::opencode_names(&self.paths.opencode_config_dir);
        let shared = |name: &String| codex.contains(name) || opencode.contains(name);
        for name in status.keys().filter(|name| !shared(name)) {
            let reported = || claude_view(name, "", String::new(), Vec::new(), status);
            views.entry(name.clone()).or_insert_with(reported);
        }
        // Swarmed's own tools and the servers set up here are listed where they belong.
        views.retain(|name, _| name != SERVER_NAME && !own.contains_key(name));
        let mut views: Vec<McpServerView> = views.into_values().collect();
        // The ones that work first, then by name, which they already are in.
        views.sort_by_key(|view| usability(view.status.as_deref()));
        views
    }

    /// Adds a server, or replaces the one of that name. A secret given a value has it stored;
    /// one given none keeps the value it has; one no longer named has its value removed.
    pub fn mcp_save(&self, input: &McpServerInput) -> Result<Vec<McpServerView>> {
        let entry = entry_from(input)?;
        let name = input.name.trim().to_string();
        let path = self.paths.mcp_config();
        let mut servers = read(&path)?;
        let before = servers.get(&name).map(|server| server.secrets.clone());
        for (secret, value) in &input.secrets {
            self.store_secret(&name, secret, value.as_deref())?;
        }
        for dropped in before
            .iter()
            .flatten()
            .filter(|old| !entry.secrets.contains(old))
        {
            self.secrets.delete(&secret_key(&name, dropped))?;
        }
        servers.insert(name, entry);
        write(&path, &servers)?;
        self.mcp_servers()
    }

    /// Removes a server set up here, with its secrets.
    pub fn mcp_remove(&self, name: &str) -> Result<Vec<McpServerView>> {
        let path = self.paths.mcp_config();
        let mut servers = read(&path)?;
        let Some(server) = servers.remove(name) else {
            return Err(invalid(format!("no server called `{name}` is set up here")));
        };
        for secret in &server.secrets {
            self.secrets.delete(&secret_key(name, secret))?;
        }
        write(&path, &servers)?;
        self.mcp_servers()
    }

    /// Stores the value given for a server's secret. None given, or an empty one, changes
    /// nothing: the form shows no stored value, so an untouched field must not erase one.
    fn store_secret(&self, server: &str, secret: &str, value: Option<&str>) -> Result<()> {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(());
        };
        self.secrets.set(&secret_key(server, secret.trim()), value)
    }

    /// The values of a server's secrets that have one, by name.
    fn secrets_of(&self, name: &str, server: &OwnServer) -> Result<BTreeMap<String, String>> {
        let mut secrets = BTreeMap::new();
        for secret in &server.secrets {
            let value = self.secrets.get(&secret_key(name, secret))?;
            secrets.extend(value.map(|value| (secret.clone(), value)));
        }
        Ok(secrets)
    }

    /// The servers set up here that are switched on, with their secrets filled in, as an agent
    /// is given them. A secret that was never given a value is left out.
    pub(crate) fn own_mcp_servers(&self) -> Result<Vec<McpServer>> {
        let servers = read(&self.paths.mcp_config())?;
        let mut given = Vec::new();
        for (name, server) in servers.into_iter().filter(|(_, server)| server.enabled) {
            let secrets = self.secrets_of(&name, &server)?;
            let transport = transport_of(server, secrets);
            given.extend(transport.map(|transport| McpServer { name, transport }));
        }
        Ok(given)
    }
}

/// How an entry is reached, with its secrets as the environment or the headers.
fn transport_of(server: OwnServer, secrets: BTreeMap<String, String>) -> Option<McpTransport> {
    if let Some(url) = server.url {
        return Some(McpTransport::Http {
            url,
            headers: secrets,
        });
    }
    Some(McpTransport::Stdio {
        command: server.command?,
        args: server.args,
        env: secrets,
    })
}

/// A server's kind, where it points, and the names of what it is given, for showing.
fn described(transport: &McpTransport) -> (&'static str, String, Vec<String>) {
    match transport {
        McpTransport::Stdio { command, args, env } => {
            let line = std::iter::once(command).chain(args);
            let line = line.map(|word| quoted(word)).collect::<Vec<_>>();
            ("stdio", line.join(" "), env.keys().cloned().collect())
        }
        McpTransport::Http { url, headers } => {
            ("http", url.clone(), headers.keys().cloned().collect())
        }
    }
}

/// Where a server stands in the list by what was last said of it: usable ones first, then
/// those not seen in any session yet, then those that failed or want signing in.
fn usability(status: Option<&str>) -> u8 {
    match status {
        Some("connected" | "passed") => 0,
        None => 1,
        Some(_) => 2,
    }
}

fn claude_view(
    name: &str,
    kind: &str,
    target: String,
    secrets: Vec<String>,
    status: &BTreeMap<String, String>,
) -> McpServerView {
    McpServerView {
        name: name.to_string(),
        origin: CLAUDE.to_string(),
        kind: kind.to_string(),
        target,
        secrets,
        enabled: true,
        status: status.get(name).cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_that_cannot_be_read_is_left_out_and_the_others_are_given() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("mcp.toml");
        let text = "[docs]\ncommand = \"docs-mcp\"\nenabled = true\ntimeout = 30\n\
                    [broken]\nargs = \"not a list\"\n";
        std::fs::write(&path, text).expect("writes");
        let (servers, problems) = read_with_problems(&path).expect("reads");
        assert_eq!(servers.keys().collect::<Vec<_>>(), ["docs"]);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems[0].contains("`broken` could not be read"),
            "{problems:?}"
        );
        assert!(problems[1].contains("`docs.timeout`"), "{problems:?}");

        // One that is not TOML gives none, and is kept when a server is saved over it.
        std::fs::write(&path, "[docs\n").expect("writes");
        assert!(read(&path).expect("reads").is_empty());
        write(&path, &servers).expect("writes");
        let kept = dir.path().join("mcp.toml.unreadable");
        assert_eq!(std::fs::read_to_string(kept).expect("kept"), "[docs\n");
    }

    #[test]
    fn a_command_line_is_split_into_words_and_quotes_keep_spaces() {
        assert_eq!(words("npx -y @x/server"), ["npx", "-y", "@x/server"]);
        assert_eq!(
            words("  node \"/Users/me/My Tools/s.js\" --name 'a b'  "),
            ["node", "/Users/me/My Tools/s.js", "--name", "a b"]
        );
        assert_eq!(words("run \"\" x"), ["run", "", "x"]);
        assert_eq!(words("   "), Vec::<String>::new());
        // And back: what is shown can be saved again unchanged.
        let line = "node \"/Users/me/My Tools/s.js\" --flag";
        let again: Vec<String> = words(line).iter().map(|word| quoted(word)).collect();
        assert_eq!(again.join(" "), line);
    }

    #[test]
    fn an_entry_is_a_command_or_an_address_under_a_plain_name() {
        let input = |name: &str, command: &str, url: &str| McpServerInput {
            name: name.into(),
            command: command.into(),
            url: url.into(),
            secrets: vec![(" TOKEN ".into(), None), (String::new(), None)],
            enabled: true,
        };
        let program = entry_from(&input("db", "npx db --port 1", "")).expect("valid");
        assert_eq!(program.command.as_deref(), Some("npx"));
        assert_eq!(program.args, ["db", "--port", "1"]);
        assert_eq!(program.secrets, ["TOKEN"]);
        assert_eq!(
            entry_from(&input("docs", "", "https://x/mcp"))
                .expect("valid")
                .kind(),
            "http"
        );
        for wrong in [
            input("has space", "x", ""),
            input("", "x", ""),
            input("swarmed", "x", ""),
            input("both", "x", "https://x"),
            input("neither", "", ""),
        ] {
            assert!(
                matches!(entry_from(&wrong), Err(Error::Mcp(_))),
                "{wrong:?}"
            );
        }
    }

    #[test]
    fn servers_that_work_are_listed_before_those_that_do_not() {
        let mut said = vec![Some("needs-auth"), None, Some("connected"), Some("failed")];
        said.sort_by_key(|status| usability(*status));
        assert_eq!(
            said,
            [Some("connected"), None, Some("needs-auth"), Some("failed")]
        );
    }
}
