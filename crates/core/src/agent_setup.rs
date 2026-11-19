//! Setting up agents other than Claude Code: seeing whether one is installed, installing it,
//! and writing its entry in `agents.toml`, so that nobody has to edit that file by hand.
//!
//! Only agents listed in [`KNOWN`] can be set up from here. Each is installed with Homebrew,
//! by a command a front end shows the user before it is run.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio::process::Command;
use toml::{Table, Value};

use crate::Core;
use crate::agents::Agents;
use crate::error::{Error, Result};
use crate::hooks::{self, OutputSink};

/// An agent Swarmed knows how to install and start.
struct Known {
    id: &'static str,
    name: &'static str,
    /// One sentence on what it is for.
    about: &'static str,
    /// The executable to look for.
    program: &'static str,
    /// The arguments that put it into ACP mode.
    args: &'static [&'static str],
    /// The Homebrew formula that installs it.
    formula: &'static str,
    /// The models offered for it until the user chooses their own.
    models: &'static [&'static str],
}

const KNOWN: &[Known] = &[Known {
    id: "opencode",
    name: "OpenCode",
    about: "An open-source coding agent that runs models from OpenRouter and other providers.",
    program: "opencode",
    args: &["acp"],
    formula: "anomalyco/tap/opencode-v2",
    models: &[
        "openrouter/openai/gpt-6.2-sol",
        "openrouter/google/gemini-3.8-flash",
        "openrouter/anthropic/claude-sonnet-5.5",
    ],
}];

/// Where OpenRouter lists its models. Public: asking needs no key and sends nothing of ours.
const OPENROUTER_MODELS: &str = "https://openrouter.ai/api/v1/models";

/// An agent that can be set up, and how far along it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentSetup {
    /// What sessions refer to the agent by.
    pub id: String,
    /// The name to show.
    pub name: String,
    /// One sentence on what it is for.
    pub about: String,
    /// Where its executable was found, if it is installed.
    pub installed: Option<PathBuf>,
    /// Whether `agents.toml` has an entry for it, so sessions can run on it.
    pub configured: bool,
    /// The models offered for it.
    pub models: Vec<String>,
    /// The command that installs it, to show before it is run.
    pub install_command: String,
}

fn known(id: &str) -> Result<&'static Known> {
    KNOWN
        .iter()
        .find(|agent| agent.id == id)
        .ok_or_else(|| Error::AgentNotFound(id.to_string()))
}

/// The first of `dirs` that holds an executable file called `program`.
fn find_program(program: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Where programs are looked for: the `PATH`, then the places installers use that a desktop
/// app's shorter `PATH` often lacks.
pub(crate) fn default_tool_dirs() -> Vec<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    if let Some(home) = std::env::home_dir() {
        dirs.extend([home.join(".local/bin"), home.join(".opencode/bin")]);
    }
    dirs
}

/// `agents.toml` as a table; empty if there is no such file yet.
fn read_config(path: &Path) -> Result<Table> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Table::new()),
        Err(e) => return Err(e.into()),
    };
    toml::from_str(&text).map_err(|e| Error::Config {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().cloned().map(Value::String).collect())
}

/// The model ids in OpenRouter's answer, each with the prefix agents name the provider by.
fn openrouter_ids(answer: &str) -> Vec<String> {
    let parsed: serde_json::Value = serde_json::from_str(answer).unwrap_or_default();
    let models = parsed.get("data").and_then(serde_json::Value::as_array);
    let id = |model: &serde_json::Value| Some(format!("openrouter/{}", model.get("id")?.as_str()?));
    models.map_or_else(Vec::new, |models| models.iter().filter_map(id).collect())
}

/// Every model OpenRouter offers right now, named the way agents take them, for choosing from.
/// Asks OpenRouter's public list, with `curl`.
pub async fn openrouter_models() -> Result<Vec<String>> {
    let output = Command::new("curl")
        .args(["-fsS", "--max-time", "15", OPENROUTER_MODELS])
        .output()
        .await?;
    let models = openrouter_ids(&String::from_utf8_lossy(&output.stdout));
    if !output.status.success() || models.is_empty() {
        return Err(Error::Install {
            agent: "OpenRouter".to_string(),
            reason: "its list of models could not be read".to_string(),
        });
    }
    Ok(models)
}

impl Core {
    /// The agents that can be set up from here, with what is done for each.
    pub fn agent_setups(&self) -> Result<Vec<AgentSetup>> {
        let config = read_config(&self.paths.agents_config())?;
        Ok(KNOWN
            .iter()
            .map(|agent| self.setup_of(agent, &config))
            .collect())
    }

    fn setup_of(&self, agent: &Known, config: &Table) -> AgentSetup {
        let entry = config.get(agent.id).and_then(Value::as_table);
        let models = entry.and_then(|entry| entry.get("models")?.as_array().cloned());
        let named = |models: Vec<Value>| {
            let name = |model: &Value| model.as_str().map(String::from);
            models.iter().filter_map(name).collect::<Vec<String>>()
        };
        AgentSetup {
            id: agent.id.to_string(),
            name: agent.name.to_string(),
            about: agent.about.to_string(),
            installed: find_program(agent.program, &self.tool_dirs),
            configured: entry.is_some(),
            models: models.map_or_else(Vec::new, named),
            install_command: format!("brew install {}", agent.formula),
        }
    }

    /// Installs a known agent with Homebrew, forwarding what the installer prints to `sink`,
    /// then writes its entry in `agents.toml` so sessions can run on it at once.
    pub async fn agent_install(&self, id: &str, sink: OutputSink<'_>) -> Result<AgentSetup> {
        let agent = known(id)?;
        let failed = |reason: String| Error::Install {
            agent: agent.name.to_string(),
            reason,
        };
        let brew = find_program("brew", &self.tool_dirs).ok_or_else(|| {
            failed("Homebrew was not found; install the agent yourself, then come back".into())
        })?;
        let command = format!("'{}' install {}", brew.display(), agent.formula);
        hooks::run("install", &command, &std::env::temp_dir(), &[], sink)
            .await
            .map_err(|e| failed(e.to_string()))?;
        if find_program(agent.program, &self.tool_dirs).is_none() {
            return Err(failed(format!(
                "the installer finished, but `{}` is nowhere to be found",
                agent.program
            )));
        }
        self.agent_configure(id, None)
    }

    /// Writes a known agent's entry in `agents.toml` and puts it to use: where its executable
    /// is, how to start it, and the models to offer, `models` if given, else the ones it has,
    /// else a few to start with. Anything else in the file is kept, except comments.
    pub fn agent_configure(&self, id: &str, models: Option<Vec<String>>) -> Result<AgentSetup> {
        let agent = known(id)?;
        let path = self.paths.agents_config();
        let mut config = read_config(&path)?;
        let program = find_program(agent.program, &self.tool_dirs);
        // The whole path, since an app started from the Dock has a shorter `PATH` than a shell.
        let command = program.map_or_else(
            || agent.program.to_string(),
            |found| found.to_string_lossy().into_owned(),
        );
        let entry = config
            .entry(agent.id)
            .or_insert_with(|| Value::Table(Table::new()));
        let Some(entry) = entry.as_table_mut() else {
            return Err(Error::Config {
                path,
                message: format!("`{}` is not a table", agent.id),
            });
        };
        let args: Vec<String> = agent.args.iter().map(ToString::to_string).collect();
        let starting: Vec<String> = agent.models.iter().map(ToString::to_string).collect();
        entry.insert("name".into(), Value::String(agent.name.to_string()));
        entry.insert("command".into(), Value::String(command));
        entry.insert("args".into(), strings(&args));
        match models {
            Some(models) => drop(entry.insert("models".into(), strings(&models))),
            None => drop(entry.entry("models").or_insert_with(|| strings(&starting))),
        }
        self.write_agents(&path, &config)?;
        let setup = self.setup_of(agent, &config);
        Ok(setup)
    }

    /// Saves `agents.toml` and makes the running core use it.
    fn write_agents(&self, path: &Path, config: &Table) -> Result<()> {
        let text = toml::to_string(config).map_err(|e| Error::Config {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let header = "# Agents Swarmed can run besides Claude Code. Written from Settings; \
                      editing by hand is fine.\n\n";
        std::fs::write(path, format!("{header}{text}"))?;
        self.reload_agents()
    }

    /// Reads `agents.toml` again, so a change to it takes effect without a restart.
    pub(crate) fn reload_agents(&self) -> Result<()> {
        let mut agents = self
            .agents
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Agents::reload(&mut agents, &self.paths.agents_config())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_is_found_in_the_first_directory_that_has_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (first, second) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir_all(&first).expect("dir");
        std::fs::create_dir_all(&second).expect("dir");
        std::fs::write(second.join("opencode"), "").expect("file");
        let dirs = [first.clone(), second.clone()];
        assert_eq!(
            find_program("opencode", &dirs),
            Some(second.join("opencode"))
        );
        assert_eq!(find_program("goose", &dirs), None);
        // A directory of that name is not a program.
        std::fs::create_dir(first.join("opencode")).expect("dir");
        assert_eq!(
            find_program("opencode", &dirs),
            Some(second.join("opencode"))
        );
    }

    #[test]
    fn openrouter_models_are_named_with_their_provider_prefix() {
        let answer = r#"{"data":[{"id":"openai/gpt-6.2-sol","name":"x"},{"id":"x-ai/grok-4.7"},{"name":"no id"}]}"#;
        assert_eq!(
            openrouter_ids(answer),
            ["openrouter/openai/gpt-6.2-sol", "openrouter/x-ai/grok-4.7"]
        );
        assert_eq!(openrouter_ids("not json"), Vec::<String>::new());
    }
}
