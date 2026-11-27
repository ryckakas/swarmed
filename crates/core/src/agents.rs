//! The agents sessions can run on: Claude Code, plus any ACP agent the user has configured.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::agent::{AcpBackend, AgentBackend, ClaudeBackend};
use crate::agent_setup::defaults_to_first_listed;
use crate::error::{Error, Result};
use crate::lenient::{self, Problems};
use crate::mcp_opencode::opencode_model;

/// The id of the agent every installation has.
pub const CLAUDE: &str = "claude";

/// The id OpenCode is set up under.
const OPENCODE: &str = "opencode";

/// An agent as a front end shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentInfo {
    /// What sessions refer to the agent by.
    pub id: String,
    /// The name to show.
    pub name: String,
    /// Model names worth offering. A session may also name one that is not listed.
    pub models: Vec<String>,
    /// What a session that leaves the model open runs on, where Swarmed decides that and not
    /// the agent: the first of `models`, for an agent whose own default is not one to go by
    /// and whose own configuration names none.
    pub default_model: Option<String>,
    /// Whether a message written while it works is taken into the turn under way, at its next
    /// step, without stopping it: Claude Code's way. Any other takes a message only between
    /// turns, so one written meanwhile waits, or stops the turn to be sent.
    pub hears_mid_turn: bool,
    /// Whether a command or sub-agent that a tool call waits on can be sent to the background,
    /// so that the call returns and the turn goes on: Claude Code's Ctrl+B.
    pub sends_to_background: bool,
}

/// One entry of `agents.toml`: an ACP agent and how to start it. A key it does not know is
/// read past and named (`lenient`), never a reason to leave the agent out.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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
    /// Whether to give it the MCP servers the other agents have. On unless set to false.
    shared_mcp_servers: Option<bool>,
    /// The name that setting had while only Claude Code's servers were shared. Swarmed wrote
    /// `false` here for agents with a list of their own, which now get what they lack, so it
    /// is read past: a file written then must still load.
    #[serde(rename = "claude_code_mcp_servers")]
    _before: Option<bool>,
}

/// The agents by id, each with the backend that runs it.
#[derive(Debug, Clone)]
pub(crate) struct Agents(BTreeMap<String, Agent>);

#[derive(Debug, Clone)]
struct Agent {
    info: AgentInfo,
    backend: Arc<dyn AgentBackend>,
    /// Whether sessions on it are given the MCP servers of the other agents that it lacks.
    gets_shared_mcp_servers: bool,
    /// Whether it comes from `agents.toml`, and goes when that file is read again without it.
    from_file: bool,
    /// The program that has to be on this machine for the agent to start, if Swarmed knows it.
    program: Option<PathBuf>,
}

/// The entries of `agents.toml` that can be read, with what could not, said for the user;
/// `None` if the text is not TOML at all.
fn entries_in(text: &str) -> Option<(BTreeMap<String, AgentConfig>, Problems)> {
    let table = text.parse::<toml::Table>().ok()?;
    let (mut entries, mut problems) = lenient::entries::<AgentConfig>(table);
    if entries.remove(CLAUDE).is_some() {
        problems.push(format!(
            "`{CLAUDE}` is built in and cannot be redefined, so that entry is left out."
        ));
    }
    Some((entries, problems))
}

/// What of `agents.toml`'s `text` cannot be understood, said for the user.
fn problems_in_text(text: &str) -> Problems {
    if let Some((_, problems)) = entries_in(text) {
        return problems;
    }
    let meanwhile = "Its agents are not offered until it is fixed; Claude Code is.";
    let error = text.parse::<toml::Table>().err();
    error.map_or_else(Vec::new, |error| {
        vec![lenient::not_toml(text, &error, meanwhile)]
    })
}

/// What of the `agents.toml` at `path` cannot be understood, said for the user.
pub(crate) fn problems_in(path: &Path) -> Result<Problems> {
    Ok(problems_in_text(
        &lenient::read_if_there(path)?.unwrap_or_default(),
    ))
}

/// Whether `program` can be started: the file it names if it names a path, else a file of
/// that name in one of `dirs`.
fn is_installed(program: &Path, dirs: &[PathBuf]) -> bool {
    if program.components().count() > 1 {
        return program.is_file();
    }
    dirs.iter().any(|dir| dir.join(program).is_file())
}

fn claude_info() -> AgentInfo {
    let models = ["opus", "sonnet", "haiku", "fable"]
        .map(String::from)
        .to_vec();
    AgentInfo {
        id: CLAUDE.to_string(),
        name: "Claude Code".to_string(),
        models,
        default_model: None,
        hears_mid_turn: true,
        sends_to_background: true,
    }
}

impl Agents {
    /// Claude Code on `claude`, and nothing else.
    pub(crate) fn only_claude(claude: Arc<dyn AgentBackend>) -> Self {
        let agent = Agent {
            info: claude_info(),
            backend: claude,
            gets_shared_mcp_servers: true,
            from_file: false,
            program: None,
        };
        Self(BTreeMap::from([(CLAUDE.to_string(), agent)]))
    }

    /// Claude Code plus the ACP agents in `agents.toml`, if that file exists. `opencode` is
    /// OpenCode's configuration directory. What of the file cannot be understood is left out
    /// (`problems_in` says what): it never keeps Swarmed from starting.
    pub(crate) fn load(config: &Path, opencode: &Path) -> Result<Self> {
        let mut agents = Self::only_claude(Arc::new(ClaudeBackend::default()));
        agents.reload(config, opencode)?;
        Ok(agents)
    }

    /// Reads `agents.toml` again: the agents it listed before are replaced by the ones it lists
    /// now. Claude Code and agents registered directly stay as they are. An entry that cannot
    /// be read is left out, and a file that is not TOML leaves everything as it was; what that
    /// was is returned, said for the user. OpenCode's own configuration, in `opencode`, is read
    /// with it: a model the user chose there is OpenCode's default, and Swarmed picks none.
    pub(crate) fn reload(&mut self, config: &Path, opencode: &Path) -> Result<Problems> {
        let text = lenient::read_if_there(config)?.unwrap_or_default();
        let Some((entries, problems)) = entries_in(&text) else {
            return Ok(problems_in_text(&text));
        };
        self.0.retain(|_, agent| !agent.from_file);
        let has_its_own = |id: &str| id == OPENCODE && opencode_model(opencode).is_some();
        for (id, entry) in entries {
            let first_listed = defaults_to_first_listed(&id) && !has_its_own(&id);
            self.add(id, entry, first_listed);
        }
        Ok(problems)
    }

    /// Adds an entry of the file. With `first_listed`, a session that leaves the model open
    /// runs on the first model the entry lists.
    fn add(&mut self, id: String, entry: AgentConfig, first_listed: bool) {
        let name = entry.name.unwrap_or_else(|| id.clone());
        let default_model = entry.models.first().filter(|_| first_listed).cloned();
        let info = AgentInfo {
            id: id.clone(),
            name,
            models: entry.models.clone(),
            default_model: default_model.clone(),
            // The protocol has no way to add to a turn under way, nor to let a call return
            // while its work goes on.
            hears_mid_turn: false,
            sends_to_background: false,
        };
        let program = PathBuf::from(&entry.command);
        let backend = AcpBackend {
            program: entry.command.into(),
            args: entry.args,
            env: entry.env.into_iter().collect(),
            default_model,
        };
        let agent = Agent {
            info,
            backend: Arc::new(backend),
            gets_shared_mcp_servers: entry.shared_mcp_servers.unwrap_or(true),
            from_file: true,
            program: Some(program),
        };
        self.0.insert(id, agent);
    }

    /// Registers an agent directly; how the tests add ones that are not in a file.
    pub(crate) fn insert(&mut self, info: AgentInfo, backend: Arc<dyn AgentBackend>) {
        let agent = Agent {
            info,
            backend,
            gets_shared_mcp_servers: true,
            from_file: false,
            program: None,
        };
        self.0.insert(agent.info.id.clone(), agent);
    }

    /// Whether agent `id` takes a message into the turn under way: see [`AgentInfo`].
    pub(crate) fn hears_mid_turn(&self, id: &str) -> bool {
        self.0
            .get(id)
            .is_some_and(|agent| agent.info.hears_mid_turn)
    }

    /// Whether agent `id` can send work a call waits on to the background: see [`AgentInfo`].
    pub(crate) fn sends_to_background(&self, id: &str) -> bool {
        self.0
            .get(id)
            .is_some_and(|agent| agent.info.sends_to_background)
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

    /// The agents a session can be started on here and now: those whose program is found, by
    /// its path or by its name in one of `dirs`. One that is configured but not installed
    /// could only fail, so it is not offered. Claude Code first, the rest by id.
    pub(crate) fn available(&self, dirs: &[PathBuf]) -> Vec<AgentInfo> {
        let installed = |info: &AgentInfo| {
            let program = self
                .0
                .get(&info.id)
                .and_then(|agent| agent.program.as_ref());
            program.is_none_or(|program| is_installed(program, dirs))
        };
        self.infos().into_iter().filter(installed).collect()
    }

    fn agent(&self, id: &str) -> Result<&Agent> {
        self.0
            .get(id)
            .ok_or_else(|| Error::AgentNotFound(id.to_string()))
    }

    /// What a session on this agent that leaves the model open runs on, if Swarmed decides it.
    pub(crate) fn default_model(&self, id: &str) -> Option<String> {
        self.0.get(id)?.info.default_model.clone()
    }

    /// The models agent `id` offers, as its menu lists them: none for an agent there is not.
    pub(crate) fn models(&self, id: &str) -> &[String] {
        self.0.get(id).map_or(&[], |agent| &agent.info.models)
    }

    /// Every model some agent offers, each once, in the order the agents list them.
    pub(crate) fn every_model(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let models = self.infos().into_iter().flat_map(|info| info.models);
        models.filter(|model| seen.insert(model.clone())).collect()
    }

    pub(crate) fn backend(&self, id: &str) -> Result<Arc<dyn AgentBackend>> {
        Ok(Arc::clone(&self.agent(id)?.backend))
    }

    /// Whether sessions on this agent are given the MCP servers of the other agents.
    pub(crate) fn gets_shared_mcp_servers(&self, id: &str) -> Result<bool> {
        Ok(self.agent(id)?.gets_shared_mcp_servers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(toml: &str) -> Result<Agents> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("agents.toml");
        std::fs::write(&path, toml)?;
        Agents::load(&path, dir.path())
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
    fn only_an_agent_whose_models_come_from_openrouter_defaults_to_the_first_listed() {
        let agents = load(
            "[opencode]\ncommand = \"opencode\"\nmodels = [\"openrouter/a/one\", \"openrouter/b/two\"]\n\
             [codex]\ncommand = \"codex-acp\"\nmodels = [\"gpt-x\", \"gpt-y\"]\n\
             [bare]\ncommand = \"opencode\"\n",
        )
        .expect("loads");
        let defaults: Vec<(String, Option<String>)> = agents
            .infos()
            .into_iter()
            .map(|info| (info.id, info.default_model))
            .collect();
        assert_eq!(
            defaults,
            [
                ("claude".to_string(), None),
                ("bare".to_string(), None),
                ("codex".to_string(), None),
                ("opencode".to_string(), Some("openrouter/a/one".to_string())),
            ]
        );
    }

    #[test]
    fn a_model_named_in_opencodes_own_configuration_stays_its_default() {
        let dir = tempfile::tempdir().expect("folder");
        let path = dir.path().join("agents.toml");
        std::fs::write(
            &path,
            "[opencode]\ncommand = \"opencode\"\nmodels = [\"openrouter/a/one\"]\n",
        )
        .expect("writes");
        let default_model = |agents: &Agents| agents.default_model("opencode");

        let mut agents = Agents::load(&path, dir.path()).expect("loads");
        assert_eq!(default_model(&agents), Some("openrouter/a/one".to_string()));

        // The user chose a model there: it is OpenCode's default, and Swarmed picks none.
        let own = "{ // mine\n \"model\": \"anthropic/claude\" }";
        std::fs::write(dir.path().join("opencode.jsonc"), own).expect("writes");
        agents.reload(&path, dir.path()).expect("reloads");
        assert_eq!(default_model(&agents), None);
    }

    #[test]
    fn every_agent_gets_the_shared_mcp_servers_unless_told_not_to() {
        // The setting's earlier name is read past, whatever it says.
        let agents = load(
            "[a]\ncommand = \"a\"\nclaude_code_mcp_servers = false\n\
             [b]\ncommand = \"b\"\nshared_mcp_servers = false\n",
        );
        let gets = |id: &str| {
            agents
                .as_ref()
                .ok()
                .and_then(|agents| agents.gets_shared_mcp_servers(id).ok())
        };
        assert_eq!(
            (gets("a"), gets("b"), gets("claude")),
            (Some(true), Some(false), Some(true))
        );
    }

    #[test]
    fn a_missing_file_means_only_claude_and_what_cannot_be_read_costs_only_itself() {
        let missing = Agents::load(
            Path::new("/nonexistent/agents.toml"),
            Path::new("/nonexistent"),
        )
        .expect("loads");
        assert_eq!(missing.infos().len(), 1);

        // A misspelt entry, a redefined Claude Code and a key from a newer Swarmed: the rest
        // is still offered, and each is said.
        let text = "[x]\ncomand = \"typo\"\n[claude]\ncommand = \"other\"\n\
                    [good]\ncommand = \"good\"\ncolour = \"red\"\n";
        let agents = load(text).expect("loads");
        let ids: Vec<String> = agents.infos().into_iter().map(|info| info.id).collect();
        assert_eq!(ids, ["claude", "good"]);
        assert!(agents.backend("claude").is_ok());
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("agents.toml");
        std::fs::write(&path, text).expect("writes");
        let problems = problems_in(&path).expect("reads");
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("`x` could not be read"))
        );
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("`good.colour`"))
        );
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("`claude` is built in"))
        );
    }

    #[test]
    fn a_file_that_is_not_toml_leaves_the_agents_as_they_were() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("agents.toml");
        std::fs::write(&path, "[a]\ncommand = \"a\"\n").expect("writes");
        let mut agents = Agents::load(&path, dir.path()).expect("loads");
        std::fs::write(&path, "[a\ncommand = \"a\"\n").expect("writes");
        let problems = agents.reload(&path, dir.path()).expect("reloads");
        assert!(problems[0].contains("not TOML (line 1"), "{problems:?}");
        assert_eq!(agents.infos().len(), 2);
        // Opened that way, Swarmed starts with Claude Code alone.
        assert_eq!(
            Agents::load(&path, dir.path())
                .expect("loads")
                .infos()
                .len(),
            1
        );
    }
}
