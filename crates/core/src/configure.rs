//! Swarmed's own configuration, as an agent may change it: the `configure` tool.
//!
//! A user should be able to say "keep that as a prompt" or "add a rule" in a session and have
//! it done. What an agent can reach this way comes in three kinds.
//!
//! - **Done at once**: prompts, actions that send a prompt, what the status line shows, and
//!   the app's appearance. The worst a mistake does there is undone in a moment.
//! - **Proposed to the user**: rules, environments, actions that run a command, and the status
//!   line's script. These outlive the session: a rule is given to every agent from then on,
//!   and a command runs later. An agent that read something hostile on a page or in a
//!   repository could plant either. So the change is only shown in the conversation, exactly
//!   as it would be made, and made when the user accepts it there, whatever the agent's own
//!   permissions are.
//! - **Not reachable at all**: API keys, MCP servers and their secrets, signing in, how much
//!   an agent may do without asking, and notes. An agent must not widen what it may do, and
//!   has no business with secrets.
//!
//! The values of an environment's variables are never told to an agent, only their names.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::actions::Action;
use crate::agent::AgentEvent;
use crate::environments::Environment;
use crate::error::Error;
use crate::settings::{MAX_RULES_BYTES, Settings};
use crate::store::Project;
use crate::{Core, Result, lock};

/// A change an agent asked for that the user has to agree to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    /// One more rule for every agent.
    RuleAdd(String),
    /// One rule fewer.
    RuleRemove(String),
    /// A button that runs a command, for one repository or for all.
    ActionSave {
        project: Option<String>,
        action: Action,
    },
    /// The script that writes the status line.
    StatusCommand(String),
    /// An environment of a repository, new or changed.
    EnvironmentSave {
        project: String,
        name: String,
        environment: Environment,
    },
    /// An environment taken away.
    EnvironmentRemove { project: String, name: String },
}

/// One line of a proposal's detail, left out when there is nothing to say.
fn line(label: &str, value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| format!("{label}: {}", value.trim()))
}

impl Change {
    /// The part of the configuration it changes, as front ends are told.
    const fn kind(&self) -> &'static str {
        match self {
            Self::RuleAdd(_) | Self::RuleRemove(_) => "rule",
            Self::ActionSave { .. } => "action",
            Self::StatusCommand(_) => "status_line",
            Self::EnvironmentSave { .. } | Self::EnvironmentRemove { .. } => "environment",
        }
    }

    /// What the user is asked, in a line.
    fn title(&self) -> String {
        match self {
            Self::RuleAdd(_) => "Add this rule for every agent?".to_string(),
            Self::RuleRemove(_) => "Remove this rule?".to_string(),
            Self::ActionSave { project, action } => {
                let place = project
                    .as_deref()
                    .map_or_else(|| "every repository".to_string(), ToString::to_string);
                format!(
                    "Add the button `{}` to {place}? It runs a command.",
                    action.name
                )
            }
            Self::StatusCommand(_) => "Let this script write the status line?".to_string(),
            Self::EnvironmentSave { project, name, .. } => {
                format!("Save the environment `{name}` of {project}?")
            }
            Self::EnvironmentRemove { project, name } => {
                format!("Remove the environment `{name}` of {project}?")
            }
        }
    }

    /// The change itself, as it would be made: what the user agrees to.
    fn detail(&self) -> String {
        match self {
            Self::RuleAdd(rule) | Self::RuleRemove(rule) => rule.clone(),
            Self::ActionSave { action, .. } => action.command.clone(),
            Self::StatusCommand(command) => command.clone(),
            Self::EnvironmentSave { environment, .. } => {
                let lines = [
                    line("setup", &environment.setup),
                    line("start", &environment.start),
                    line("stop", &environment.stop),
                    line("teardown", &environment.teardown),
                    line("address", &environment.open_url),
                ];
                lines.into_iter().flatten().collect::<Vec<_>>().join("\n")
            }
            Self::EnvironmentRemove { .. } => String::new(),
        }
    }
}

/// The changes agents asked for that are waiting for the user. They are kept while the front
/// end runs; one left over from an earlier run has to be asked for again.
#[derive(Debug, Default)]
pub(crate) struct Proposals {
    next: AtomicU64,
    open: Mutex<HashMap<String, (i64, Change)>>,
}

fn refused(why: impl Into<String>) -> Error {
    Error::Configure(why.into())
}

/// What the agent is told once a change is waiting for the user.
const PROPOSED: &str = "Proposed to the user. It is NOT in effect: Swarmed shows it in this \
    conversation, and it is made only if they accept it there. Say that it waits for their \
    approval; do not say it is done.";

/// The arguments of one call.
struct Asked<'a>(&'a Value);

impl Asked<'_> {
    /// A text argument, without the space around it; empty if it was not given.
    fn word(&self, key: &str) -> &str {
        let value = self.0.get(key).and_then(Value::as_str);
        value.unwrap_or_default().trim()
    }

    fn has(&self, key: &str) -> bool {
        self.0.get(key).is_some_and(|value| !value.is_null())
    }

    fn flag(&self, key: &str) -> bool {
        self.0.get(key).and_then(Value::as_bool) == Some(true)
    }

    /// An argument that names one of a fixed set, such as a palette; `None` if not given.
    fn choice<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let Some(value) = self.0.get(key).filter(|value| !value.is_null()) else {
            return Ok(None);
        };
        let lowered = value.as_str().map(|word| word.trim().to_lowercase());
        let value = lowered.map_or_else(|| value.clone(), Value::String);
        serde_json::from_value(value)
            .map(Some)
            .map_err(|error| refused(format!("`{key}`: {error}")))
    }
}

/// The rules as the lines they are, without blank ones.
fn rule_lines(rules: &str) -> Vec<&str> {
    let lines = rules.lines().map(str::trim);
    lines.filter(|rule| !rule.is_empty()).collect()
}

impl Core {
    /// The repository that session `from` works in.
    fn project_of_session(&self, from: i64) -> Result<Project> {
        let session = self.session(from)?;
        let task = self
            .store()
            .task_by_id(session.task_id)?
            .ok_or(Error::SessionNotFound(from))?;
        let project = self.store().project_by_id(task.project_id)?;
        project.ok_or(Error::SessionNotFound(from))
    }

    /// Tells every front end that the configuration of `kind` is no longer what they read.
    fn config_changed(&self, from: i64, kind: &str) -> Result<()> {
        let changed = AgentEvent::ConfigChanged {
            kind: kind.to_string(),
        };
        self.live.note(&self.store, from, changed)
    }

    /// Shows `change` to the user in the conversation of session `from` and keeps it until
    /// they answer.
    fn propose(&self, from: i64, change: Change) -> Result<String> {
        let number = self.proposals.next.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("config-{}-{number}", std::process::id());
        let proposed = AgentEvent::ConfigProposed {
            request_id: request_id.clone(),
            title: change.title(),
            detail: change.detail(),
        };
        lock(&self.proposals.open).insert(request_id, (from, change));
        self.live.note(&self.store, from, proposed)?;
        Ok(PROPOSED.to_string())
    }

    /// Makes a change the user agreed to.
    fn apply(&self, change: &Change) -> Result<()> {
        match change {
            Change::RuleAdd(rule) => {
                let mut settings = self.settings()?;
                let mut rules = rule_lines(&settings.rules);
                rules.push(rule);
                settings.rules = rules.join("\n");
                self.set_settings(&settings)
            }
            Change::RuleRemove(rule) => {
                let mut settings = self.settings()?;
                let kept: Vec<&str> = rule_lines(&settings.rules)
                    .into_iter()
                    .filter(|kept| kept != rule)
                    .collect();
                settings.rules = kept.join("\n");
                self.set_settings(&settings)
            }
            Change::ActionSave { project, action } => {
                self.action_save(project.as_deref(), action).map(drop)
            }
            Change::StatusCommand(command) => {
                let mut settings = self.settings()?;
                settings.status_command.clone_from(command);
                self.set_settings(&settings)
            }
            Change::EnvironmentSave {
                project,
                name,
                environment,
            } => self.environment_save(project, name, environment),
            Change::EnvironmentRemove { project, name } => self.environment_remove(project, name),
        }
    }

    /// The user's answer to a change an agent proposed: it is made if they accepted it, and
    /// dropped either way. The conversation it was proposed in is told.
    pub fn config_answer(&self, request_id: &str, accepted: bool) -> Result<()> {
        let waiting = lock(&self.proposals.open).get(request_id).cloned();
        let (from, change) = waiting.ok_or_else(|| {
            refused("that proposal is no longer open; ask the agent to propose it again")
        })?;
        if accepted {
            self.apply(&change)?;
        }
        lock(&self.proposals.open).remove(request_id);
        let answered = AgentEvent::ConfigAnswered {
            request_id: request_id.to_string(),
            accepted,
        };
        self.live.note(&self.store, from, answered)?;
        if accepted {
            self.config_changed(from, change.kind())?;
        }
        Ok(())
    }

    fn configure_prompt(&self, op: &str, asked: &Asked<'_>) -> Result<String> {
        let name = asked.word("name");
        match op {
            "list" => {
                let kept = self.templates()?;
                let lines: Vec<String> = kept
                    .iter()
                    .map(|prompt| format!("/{}: {}", prompt.name, prompt.text))
                    .collect();
                Ok(listed(&lines, "No prompts are kept."))
            }
            "save" => {
                self.template_save(name, asked.word("text"))?;
                Ok(format!(
                    "Kept the prompt. The user sends it by typing /{name}."
                ))
            }
            _ => {
                self.template_remove(name)?;
                Ok(format!("The prompt `{name}` is gone."))
            }
        }
    }

    fn configure_action(&self, from: i64, op: &str, asked: &Asked<'_>) -> Result<String> {
        let project = self.project_of_session(from)?.name;
        let scope = (!asked.flag("everywhere")).then_some(project.as_str());
        match op {
            "list" => {
                let offered = self.actions(Some(&project))?;
                let lines: Vec<String> = offered.iter().map(action_line).collect();
                Ok(listed(&lines, "The user has no actions."))
            }
            "save" => self.configure_action_save(from, scope, asked),
            _ => {
                self.action_remove(scope, asked.word("name"))?;
                Ok(format!("The action `{}` is gone.", asked.word("name")))
            }
        }
    }

    /// Keeps an action that sends a prompt, or proposes one that runs a command.
    fn configure_action_save(
        &self,
        from: i64,
        scope: Option<&str>,
        asked: &Asked<'_>,
    ) -> Result<String> {
        let action = Action {
            name: asked.word("name").to_string(),
            icon: asked.word("icon").to_string(),
            prompt: asked.word("text").to_string(),
            command: asked.word("command").to_string(),
            of_project: false,
        };
        if action.command.is_empty() {
            self.action_save(scope, &action)?;
            return Ok(format!(
                "Kept the action `{}`. Its button is in the task panel.",
                action.name
            ));
        }
        // Checked now, so the user is not asked about something that cannot be kept.
        let action = crate::actions::checked(&action)?;
        let project = scope.map(str::to_string);
        self.propose(from, Change::ActionSave { project, action })
    }

    fn configure_rule(&self, from: i64, op: &str, asked: &Asked<'_>) -> Result<String> {
        let settings = self.settings()?;
        let rules = rule_lines(&settings.rules);
        let rule = asked.word("text");
        if op == "list" {
            let lines: Vec<String> = rules.iter().map(|rule| format!("- {rule}")).collect();
            return Ok(listed(&lines, "The user has set no rules."));
        }
        if rule.is_empty() || rule.contains('\n') {
            return Err(refused("a rule is one line of text, given as `text`"));
        }
        let known = rules.contains(&rule);
        let room = settings.rules.len() + rule.len() < MAX_RULES_BYTES;
        match (op, known) {
            ("save", true) => Ok("That rule is already set.".to_string()),
            ("save", false) if !room => Err(Error::RulesTooLong(MAX_RULES_BYTES)),
            ("save", false) => self.propose(from, Change::RuleAdd(rule.to_string())),
            (_, true) => self.propose(from, Change::RuleRemove(rule.to_string())),
            (_, false) => Err(refused(
                "there is no such rule; list them and give one exactly as it is written",
            )),
        }
    }

    fn configure_status_line(&self, from: i64, op: &str, asked: &Asked<'_>) -> Result<String> {
        let mut settings = self.settings()?;
        if op == "list" {
            let items = serde_json::to_value(&settings.status_items).unwrap_or_default();
            let script = match settings.status_command.trim() {
                "" => "No script is set.".to_string(),
                script => format!("A script of the user's writes it instead: {script}"),
            };
            return Ok(format!("Shown, in this order: {items}. {script}"));
        }
        // A script for the line is the one part of it the user has to agree to.
        let command = asked.word("command");
        if op == "save" && !command.is_empty() {
            return self.propose(from, Change::StatusCommand(command.to_string()));
        }
        if op == "save" {
            let items = asked.choice("items")?;
            settings.status_items =
                items.ok_or_else(|| refused("give `items`, the pieces to show, in order"))?;
        } else {
            // Without its script the line is made of the built-in pieces again.
            settings.status_command = String::new();
        }
        self.set_settings(&settings)?;
        Ok("The status line is changed.".to_string())
    }

    fn configure_appearance(&self, op: &str, asked: &Asked<'_>) -> Result<String> {
        let mut settings = self.settings()?;
        if op == "list" {
            return Ok(appearance_of(&settings));
        }
        if op != "save" {
            return Err(refused("the appearance can be listed and saved"));
        }
        if let Some(theme) = asked.choice("theme")? {
            settings.theme = theme;
        }
        if let Some(palette) = asked.choice("palette")? {
            settings.palette = palette;
        }
        if let Some(style) = asked.choice("style")? {
            settings.style = style;
        }
        if let Some(layout) = asked.choice("layout")? {
            settings.layout = layout;
        }
        if let Some(scale) = asked.choice("text_size")? {
            settings.text_scale = scale;
        }
        self.set_settings(&settings)?;
        Ok(format!("Changed. {}", appearance_of(&settings)))
    }

    fn configure_environment(&self, from: i64, op: &str, asked: &Asked<'_>) -> Result<String> {
        let project = self.project_of_session(from)?.name;
        let environments = self.environments(&project)?;
        if op == "list" {
            let lines: Vec<String> = environments
                .iter()
                .map(|(name, environment)| environment_line(name, environment))
                .collect();
            return Ok(listed(&lines, "This repository has no environments."));
        }
        let name = asked.word("name").to_string();
        crate::paths::validate_name(&name)?;
        if op != "save" && !environments.contains_key(&name) {
            return Err(refused(format!("there is no environment `{name}`")));
        }
        if op != "save" {
            return self.propose(from, Change::EnvironmentRemove { project, name });
        }
        // What the call does not name stays as it is; the variables are never touched.
        let mut environment = environments.get(&name).cloned().unwrap_or_default();
        let parts = [
            ("setup", &mut environment.setup),
            ("start", &mut environment.start),
            ("stop", &mut environment.stop),
            ("teardown", &mut environment.teardown),
            ("open_url", &mut environment.open_url),
        ];
        for (key, part) in parts.into_iter().filter(|(key, _)| asked.has(key)) {
            *part = asked.word(key).to_string();
        }
        let change = Change::EnvironmentSave {
            project,
            name,
            environment,
        };
        self.propose(from, change)
    }

    /// The `configure` tool, called by session `from`. The answer is text for the agent.
    pub(crate) fn peer_configure(&self, from: i64, arguments: &Value) -> Result<String> {
        let asked = Asked(arguments);
        let op = asked.word("op");
        if !["list", "save", "remove"].contains(&op) {
            return Err(refused("`op` is one of list, save, remove"));
        }
        let kind = asked.word("kind");
        let done = match kind {
            "prompt" => self.configure_prompt(op, &asked),
            "action" => self.configure_action(from, op, &asked),
            "rule" => self.configure_rule(from, op, &asked),
            "status_line" => self.configure_status_line(from, op, &asked),
            "appearance" => self.configure_appearance(op, &asked),
            "environment" => self.configure_environment(from, op, &asked),
            _ => Err(refused(
                "`kind` is one of prompt, action, rule, status_line, appearance, environment. \
                 API keys, MCP servers, sign-ins, permissions and notes cannot be changed by \
                 an agent.",
            )),
        }?;
        // A proposal changes nothing yet; and a list never does.
        if op != "list" && done != PROPOSED {
            self.config_changed(from, kind)?;
        }
        Ok(done)
    }
}

/// `lines` as one text, or `none` if there are none.
fn listed(lines: &[String], none: &str) -> String {
    if lines.is_empty() {
        none.to_string()
    } else {
        lines.join("\n")
    }
}

fn action_line(action: &Action) -> String {
    let place = if action.of_project {
        "this repository"
    } else {
        "every repository"
    };
    let does = if action.command.is_empty() {
        format!("sends: {}", action.prompt)
    } else {
        format!("runs: {}", action.command)
    };
    format!("{} ({place}) {does}", action.name)
}

/// An environment for an agent to read: its commands, and the names of its variables alone.
fn environment_line(name: &str, environment: &Environment) -> String {
    let variables: Vec<&str> = environment.env.keys().map(String::as_str).collect();
    let parts = [
        line("setup", &environment.setup),
        line("start", &environment.start),
        line("stop", &environment.stop),
        line("teardown", &environment.teardown),
        line("address", &environment.open_url),
        line("variables (values withheld)", &variables.join(", ")),
    ];
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    format!(
        "{name}: {}",
        listed(&parts, "nothing of its own").replace('\n', "; ")
    )
}

fn appearance_of(settings: &Settings) -> String {
    let word = |value: Value| value.as_str().unwrap_or_default().to_string();
    let theme = word(serde_json::to_value(settings.theme).unwrap_or_default());
    let palette = word(serde_json::to_value(settings.palette).unwrap_or_default());
    let style = word(serde_json::to_value(settings.style).unwrap_or_default());
    let layout = word(serde_json::to_value(settings.layout).unwrap_or_default());
    format!(
        "theme {theme} (system, dark, light); palette {palette} (dusk, graphite, forest, plum, \
         ember, swarm); style {style} (plain, swarm, colony); layout {layout} (classic, senior); \
         text_size {} (80 to 150)",
        settings.text_scale
    )
}
