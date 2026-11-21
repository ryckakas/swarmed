//! One pool of MCP servers for every agent.
//!
//! Each agent keeps a list of its own and knows nothing of the others'. Swarmed reads them
//! all when a session starts and gives the session's agent the ones it lacks, so a server
//! set up in one agent works in all of them. Nothing is copied into an agent's files, and a
//! server added to one is there for the next session of any.
//!
//! An agent is never given a server by a name it already has, switched on or off: it would
//! refuse the second, and one the user switched off there is off for a reason.

use std::collections::{BTreeMap, BTreeSet};

use std::path::Path;

use crate::agents::CLAUDE;
use crate::mcp::{self, McpServer};
use crate::paths::Paths;
use crate::{mcp_codex, mcp_opencode, mcp_plugins};

/// The servers of one agent, by the id that agent goes by.
pub(crate) type Source = (&'static str, Vec<McpServer>);

/// What a session on `agent` is given out of `sources`: every server of another agent whose
/// name is not among those the agent `has`. Where two agents name a server alike, the one
/// earlier in `sources` has it.
pub(crate) fn pooled(agent: &str, sources: Vec<Source>, has: &BTreeSet<String>) -> Vec<McpServer> {
    let mut by_name: BTreeMap<String, McpServer> = BTreeMap::new();
    let others = sources.into_iter().filter(|(origin, _)| *origin != agent);
    for server in others.flat_map(|(_, servers)| servers) {
        if !has.contains(&server.name) {
            by_name.entry(server.name.clone()).or_insert(server);
        }
    }
    by_name.into_values().collect()
}

impl Paths {
    /// What a session on `agent` in `worktree` of the repository at `repo` is given of the
    /// other agents' servers, each read from its agent's own configuration as it is now.
    pub(crate) fn shared_mcp_servers(
        &self,
        agent: &str,
        repo: &Path,
        worktree: &Path,
    ) -> Vec<McpServer> {
        let claude = self.claude_code_servers(repo, worktree);
        let name = |server: &McpServer| server.name.clone();
        let has = match agent {
            CLAUDE => claude.iter().map(name).collect(),
            mcp_codex::ORIGIN => mcp_codex::codex_names(&self.codex_config),
            mcp_opencode::ORIGIN => mcp_opencode::opencode_names(&self.opencode_config_dir),
            _ => BTreeSet::new(),
        };
        let codex = mcp_codex::codex_servers(&self.codex_config);
        let opencode = mcp_opencode::opencode_servers(&self.opencode_config_dir);
        let sources = vec![
            (CLAUDE, claude),
            (mcp_codex::ORIGIN, codex),
            (mcp_opencode::ORIGIN, opencode),
        ];
        pooled(agent, sources, &has)
    }

    /// Claude Code's servers: the ones its plugins bring, and the ones configured, which go
    /// before a plugin's of the same name.
    fn claude_code_servers(&self, repo: &Path, worktree: &Path) -> Vec<McpServer> {
        let home = self.claude_config.parent().map(|home| home.join(".claude"));
        let mut servers = home.map_or_else(Vec::new, |home| mcp_plugins::plugin_servers(&home));
        let configured = mcp::claude_code_servers(&self.claude_config, repo, worktree);
        servers.retain(|plugin| configured.iter().all(|server| server.name != plugin.name));
        servers.extend(configured);
        servers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::McpTransport;

    fn server(name: &str, command: &str) -> McpServer {
        McpServer {
            name: name.to_string(),
            transport: McpTransport::Stdio {
                command: command.to_string(),
                args: vec![],
                env: BTreeMap::new(),
            },
        }
    }

    fn sources() -> Vec<Source> {
        vec![
            (
                "claude",
                vec![server("docs", "claude"), server("both", "claude")],
            ),
            (
                "codex",
                vec![server("tracker", "codex"), server("both", "codex")],
            ),
            ("opencode", vec![server("files", "opencode")]),
        ]
    }

    fn given(agent: &str, has: &[&str]) -> Vec<(String, String)> {
        let has = has.iter().map(ToString::to_string).collect();
        let command = |server: McpServer| match server.transport {
            McpTransport::Stdio { command, .. } => (server.name, command),
            McpTransport::Http { url, .. } => (server.name, url),
        };
        pooled(agent, sources(), &has)
            .into_iter()
            .map(command)
            .collect()
    }

    fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        let owned = |(name, command): &(&str, &str)| (name.to_string(), command.to_string());
        pairs.iter().map(owned).collect()
    }

    #[test]
    fn an_agent_with_none_of_its_own_gets_them_all_and_the_first_of_a_name() {
        assert_eq!(
            given("antigravity", &[]),
            pairs(&[
                ("both", "claude"),
                ("docs", "claude"),
                ("files", "opencode"),
                ("tracker", "codex")
            ])
        );
    }

    #[test]
    fn an_agent_gets_what_the_others_have_and_it_lacks() {
        // Codex has `both` and `tracker` itself, and one more that it switched off.
        assert_eq!(
            given("codex", &["both", "tracker", "files"]),
            pairs(&[("docs", "claude")])
        );
        assert_eq!(
            given("claude", &["docs", "both"]),
            pairs(&[("files", "opencode"), ("tracker", "codex")])
        );
    }
}
