//! The agents sessions can run on: Claude Code, plus any ACP agent the user has configured.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::agent::{AcpBackend, AgentBackend, ClaudeBackend};
use crate::error::{Error, Result};

/// The id of the agent every installation has.
pub const CLAUDE: &str = "claude";

/// An agent as a front end shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentInfo {
    /// What sessions refer to the agent by.
    pub id: String,
    /// The name to show.
    pub name: String,
    /// Model names worth offering. A session may also name one that is not listed.
    pub models: Vec<String>,
}

/// One entry of `agents.toml`: an ACP agent and how to start it.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AgentConfig {
    /// The name to show; defaults to the entry's key.
    name: Option<String>,
    /// The agent's executable.
    command: String,
    /// The arguments that put it into ACP mode, such as `["acp"]`.
    #[serde(default)]
    args: Vec<String>,
    /// Extra environment variables for the agent.
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Model names to offer for it.
    #[serde(default)]
    models: Vec<String>,
    /// Whether to give it the MCP servers configured for Claude Code. On unless set to false.
    #[serde(default = "yes")]
    claude_code_mcp_servers: bool,
}

fn yes() -> bool {
    true
}

/// The agents by id, each with the backend that runs it.
#[derive(Debug, Clone)]
pub(crate) struct Agents(BTreeMap<String, Agent>);

#[derive(Debug, Clone)]
struct Agent {
    info: AgentInfo,
    backend: Arc<dyn AgentBackend>,
    /// Whether sessions on it are given the MCP servers configured for Claude Code. Claude Code
    /// itself is not: it reads them on its own.
    gets_claude_code_mcp_servers: bool,
}

fn claude_info() -> AgentInfo {
    let models = ["opus", "sonnet", "haiku", "fable"]
        .map(String::from)
        .to_vec();
    AgentInfo {
        id: CLAUDE.to_string(),
        name: "Claude Code".to_string(),
        models,
    }
}

impl Agents {
    /// Claude Code on `claude`, and nothing else.
    pub(crate) fn only_claude(claude: Arc<dyn AgentBackend>) -> Self {
        let agent = Agent {
            info: claude_info(),
            backend: claude,
            gets_claude_code_mcp_servers: false,
        };
        Self(BTreeMap::from([(CLAUDE.to_string(), agent)]))
    }

    /// Claude Code plus the ACP agents in `agents.toml`, if that file exists.
    pub(crate) fn load(config: &Path) -> Result<Self> {
        let mut agents = Self::only_claude(Arc::new(ClaudeBackend::default()));
        let text = match std::fs::read_to_string(config) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(agents),
            Err(e) => return Err(e.into()),
        };
        let invalid = |message: String| Error::Config {
            path: config.to_path_buf(),
            message,
        };
        let entries: BTreeMap<String, AgentConfig> =
            toml::from_str(&text).map_err(|e| invalid(e.to_string()))?;
        if entries.contains_key(CLAUDE) {
            return Err(invalid(format!(
                "`{CLAUDE}` is built in and cannot be redefined"
            )));
        }
        for (id, entry) in entries {
            agents.add(id, entry);
        }
        Ok(agents)
    }

    fn add(&mut self, id: String, entry: AgentConfig) {
        let name = entry.name.unwrap_or_else(|| id.clone());
        let info = AgentInfo {
            id: id.clone(),
            name,
            models: entry.models,
        };
        let backend = AcpBackend {
            program: entry.command.into(),
            args: entry.args,
            env: entry.env.into_iter().collect(),
        };
        let agent = Agent {
            info,
            backend: Arc::new(backend),
            gets_claude_code_mcp_servers: entry.claude_code_mcp_servers,
        };
        self.0.insert(id, agent);
    }

    /// Registers an agent directly; how the tests add ones that are not in a file.
    pub(crate) fn insert(&mut self, info: AgentInfo, backend: Arc<dyn AgentBackend>) {
        let agent = Agent {
            info,
            backend,
            gets_claude_code_mcp_servers: true,
        };
        self.0.insert(agent.info.id.clone(), agent);
    }

    pub(crate) fn infos(&self) -> Vec<AgentInfo> {
        // Claude Code first, the rest by id.
        let (claude, others): (Vec<_>, Vec<_>) =
            self.0.values().partition(|agent| agent.info.id == CLAUDE);
        claude
            .into_iter()
            .chain(others)
            .map(|agent| agent.info.clone())
            .collect()
    }

    fn agent(&self, id: &str) -> Result<&Agent> {
        self.0
            .get(id)
            .ok_or_else(|| Error::AgentNotFound(id.to_string()))
    }

    pub(crate) fn backend(&self, id: &str) -> Result<Arc<dyn AgentBackend>> {
        Ok(Arc::clone(&self.agent(id)?.backend))
    }

    /// Whether sessions on this agent are given the MCP servers configured for Claude Code.
    pub(crate) fn gets_claude_code_mcp_servers(&self, id: &str) -> Result<bool> {
        Ok(self.agent(id)?.gets_claude_code_mcp_servers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(toml: &str) -> Result<Agents> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("agents.toml");
        std::fs::write(&path, toml)?;
        Agents::load(&path)
    }

    #[test]
    fn claude_is_always_there_and_listed_first() {
        let agents = load("[aaa]\ncommand = \"aaa\"\n").expect("loads");
        let ids: Vec<String> = agents.infos().into_iter().map(|info| info.id).collect();
        assert_eq!(ids, ["claude", "aaa"]);
        assert!(agents.backend("claude").is_ok());
        assert!(matches!(agents.backend("nope"), Err(Error::AgentNotFound(id)) if id == "nope"));
    }

    #[test]
    fn an_entry_describes_how_to_start_an_acp_agent() {
        let agents = load(
            r#"
[opencode]
name = "OpenCode"
command = "opencode"
args = ["acp"]
models = ["openrouter/openai/gpt-5.6"]
[opencode.env]
OPENCODE_CONFIG = "/tmp/x"
"#,
        )
        .expect("loads");
        let info = agents.infos().pop().expect("two agents");
        assert_eq!((info.name.as_str(), info.models.len()), ("OpenCode", 1));
    }

    #[test]
    fn other_agents_get_claude_codes_mcp_servers_unless_told_not_to() {
        let agents =
            load("[a]\ncommand = \"a\"\n[b]\ncommand = \"b\"\nclaude_code_mcp_servers = false\n");
        let gets = |id: &str| {
            agents
                .as_ref()
                .ok()
                .and_then(|agents| agents.gets_claude_code_mcp_servers(id).ok())
        };
        assert_eq!(
            (gets("a"), gets("b"), gets("claude")),
            (Some(true), Some(false), Some(false))
        );
    }

    #[test]
    fn a_missing_file_means_only_claude_and_a_bad_one_is_an_error() {
        let missing = Agents::load(Path::new("/nonexistent/agents.toml")).expect("loads");
        assert_eq!(missing.infos().len(), 1);
        assert!(matches!(
            load("[x]\ncomand = \"typo\"\n"),
            Err(Error::Config { .. })
        ));
        assert!(matches!(
            load("[claude]\ncommand = \"other\"\n"),
            Err(Error::Config { .. })
        ));
    }
}
