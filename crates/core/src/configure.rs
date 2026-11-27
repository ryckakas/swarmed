//! Swarmed's own configuration, as an agent may change it: the `configure` tool.
//!
//! A user should be able to say "keep that as a prompt" or "add a rule" in a session and have
//! it done. What an agent can reach this way comes in three kinds.
//!
//! - **Done at once**: prompts, actions that send a prompt, what the status line shows, and
//!   the app's appearance. The worst a mistake does there is undone in a moment.
//! - **Proposed to the user**: rules, environments, actions that run a command, the status
//!   line's script, and hiding tool calls. These outlive the session: a rule is given to every
//!   agent from then on, a command runs later, and with tool calls hidden no conversation
//!   shows what its agent does. An agent that read something hostile on a page or in a
//!   repository could plant any of them. So the change is only shown in the conversation, exactly
//!   as it would be made, and made when the user accepts it there, whatever the agent's own
//!   permissions are.
//! - **Not reachable at all**: API keys, MCP servers and their secrets, signing in, how much
//!   an agent may do without asking, and notes. An agent must not widen what it may do, and
//!   has no business with secrets. Nor the branch prefix: the user sets it once, and it is not
//!   worth a line in a description every agent is sent. Nor how many sessions may work for one
//!   (`max_workers`): it caps how many agents one session keeps running, and so what it can
//!   spend, and an agent must not raise its own ceiling.
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
use crate::layouts::layout_schema;
use crate::settings::{Layout, LimitWarnings, MAX_RULES_BYTES, MathShown, Settings, ToolCalls};
use crate::store::Project;
use crate::termconfig::terminal_schema;
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
        /// The variables this change itself sets or removes, as the user is shown them.
        /// Those the environment already had stay out of the conversation, where another
        /// agent could read them.
        variables: String,
    },
    /// An environment taken away.
    EnvironmentRemove { project: String, name: String },
    /// Tool calls left out of every conversation.
    ToolCallsHidden,
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
            Self::ToolCallsHidden => "appearance",
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
            Self::ToolCallsHidden => "Hide tool calls in every conversation?".to_string(),
        }
    }

    /// The change itself, as it would be made: what the user agrees to.
    fn detail(&self) -> String {
        match self {
            Self::RuleAdd(rule) | Self::RuleRemove(rule) => rule.clone(),
            Self::ActionSave { action, .. } => action.command.clone(),
            Self::StatusCommand(command) => command.clone(),
            Self::EnvironmentSave {
                environment,
                variables,
                ..
            } => {
                let lines = [
                    line("setup", &environment.setup),
                    line("start", &environment.start),
                    line("stop", &environment.stop),
                    line("teardown", &environment.teardown),
                    line("address", &environment.open_url),
                    line("ports", &pairs(&environment.ports)),
                    line("variables", variables),
                ];
                lines.into_iter().flatten().collect::<Vec<_>>().join("\n")
            }
            Self::EnvironmentRemove { .. } => String::new(),
            Self::ToolCallsHidden => HIDDEN_CALLS.to_string(),
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

/// What the user is told that hiding tool calls does.
const HIDDEN_CALLS: &str = "What agents run, read and change is left out of every conversation, \
    this one included. Permission requests, questions, plans and views still show. Settings, \
    Appearance, Tool calls puts them back.";

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

    /// The entries of an object given under `key`; none if it is not given.
    fn entries(&self, key: &str) -> Result<Vec<(&String, &Value)>> {
        match self.0.get(key).filter(|given| !given.is_null()) {
            None => Ok(vec![]),
            Some(Value::Object(entries)) => Ok(entries.iter().collect()),
            Some(_) => Err(refused(format!("`{key}` is an object of names and values"))),
        }
    }

    /// The ports given, by name: a base number, or nothing for one to remove.
    fn ports(&self) -> Result<Vec<(String, Option<u16>)>> {
        let port = |(name, value): (&String, &Value)| {
            let base = value.as_u64().and_then(|base| u16::try_from(base).ok());
            let base =
                base.ok_or_else(|| refused(format!("port `{name}` is a number up to 65535")))?;
            Ok((name.clone(), (base != 0).then_some(base)))
        };
        self.entries("ports")?.into_iter().map(port).collect()
    }

    /// The variables given, by name: a value, or nothing for one to remove.
    fn variables(&self) -> Result<Vec<(String, Option<String>)>> {
        let variable = |(name, value): (&String, &Value)| {
            let text = value
                .as_str()
                .ok_or_else(|| refused(format!("variable `{name}` is a text")))?;
            Ok((name.clone(), (!text.is_empty()).then(|| text.to_string())))
        };
        self.entries("variables")?
            .into_iter()
            .map(variable)
            .collect()
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
    pub(crate) fn project_of_session(&self, from: i64) -> Result<Project> {
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
                ..
            } => self.environment_save(project, name, environment),
            Change::EnvironmentRemove { project, name } => self.environment_remove(project, name),
            Change::ToolCallsHidden => {
                let mut settings = self.settings()?;
                settings.tool_calls = ToolCalls::Hidden;
                self.set_settings(&settings)
            }
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

    fn configure_appearance(&self, from: i64, op: &str, asked: &Asked<'_>) -> Result<String> {
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
        // Done at once like the rest: it only changes which levels the usage notice says, and
        // a limit that stops requests is said whatever it is.
        if let Some(warnings) = asked.choice::<LimitWarnings>("limit_warnings")? {
            settings.limit_warnings = warnings;
        }
        // Done at once too: a turn that failed or was stopped keeps its line whatever it is.
        if let Some(lines) = asked.choice("turn_end_lines")? {
            settings.turn_end_lines = lines;
        }
        // Done at once too: it changes only how formulas in replies are drawn, never what an
        // agent is told or what the user sees of its work.
        if let Some(math) = asked.choice::<MathShown>("math")? {
            settings.math = math;
        }
        let tool_calls: Option<ToolCalls> = asked.choice("tool_calls")?;
        // Hiding what agents do is the one part of the looks the user has to agree to: an
        // agent could hide its own calls with it, in every session from then on.
        let hides =
            tool_calls == Some(ToolCalls::Hidden) && settings.tool_calls != ToolCalls::Hidden;
        if let Some(tool_calls) = tool_calls.filter(|_| !hides) {
            settings.tool_calls = tool_calls;
        }
        self.set_settings(&settings)?;
        if hides {
            // The rest of what was asked is made, which the proposal's answer does not say.
            self.config_changed(from, "appearance")?;
            return self.propose(from, Change::ToolCallsHidden);
        }
        Ok(format!("Changed. {}", appearance_of(&settings)))
    }

    /// The custom layout: listed with its schema, or replaced as a whole and switched to.
    fn configure_layout(&self, op: &str, asked: &Asked<'_>) -> Result<String> {
        if op == "list" {
            let current = serde_json::to_string(&self.custom_layout()?).unwrap_or_default();
            return Ok(format!(
                "The custom layout now: {current}\nIts JSON Schema: {}",
                layout_schema()
            ));
        }
        if op != "save" {
            return Err(refused(
                "the custom layout can be listed and saved; save {} to empty it",
            ));
        }
        let Some(given) = asked.0.get("json").filter(|given| !given.is_null()) else {
            return Err(refused("give the whole layout as `json`"));
        };
        let text = given
            .as_str()
            .map_or_else(|| given.to_string(), str::to_string);
        let kept = self.set_custom_layout(&text)?;
        let mut settings = self.settings()?;
        settings.layout = Layout::Custom;
        self.set_settings(&settings)?;
        let kept = serde_json::to_string(&kept).unwrap_or_default();
        Ok(format!("Kept, and the app is drawn in it now: {kept}"))
    }

    /// The terminal's configuration: listed with its schema, or replaced as a whole.
    fn configure_terminal(&self, op: &str, asked: &Asked<'_>) -> Result<String> {
        if op == "list" {
            let current = serde_json::to_string(&self.terminal_config()?).unwrap_or_default();
            return Ok(format!(
                "The terminal's configuration now: {current}\nIts JSON Schema: {}",
                terminal_schema()
            ));
        }
        if op != "save" {
            return Err(refused(
                "the terminal's configuration can be listed and saved; save {} to empty it",
            ));
        }
        let Some(given) = asked.0.get("json").filter(|given| !given.is_null()) else {
            return Err(refused("give the whole configuration as `json`"));
        };
        let text = given
            .as_str()
            .map_or_else(|| given.to_string(), str::to_string);
        let kept = serde_json::to_string(&self.set_terminal_config(&text)?).unwrap_or_default();
        Ok(format!("Kept; terminals are redrawn in it: {kept}"))
    }

    fn configure_environment(&self, from: i64, op: &str, asked: &Asked<'_>) -> Result<String> {
        let project = self.project_of_session(from)?.name;
        let environments = self.environments(&project)?;
        if op == "list" {
            let lines: Vec<String> = environments
                .iter()
                .map(|(name, environment)| environment_line(name, environment))
                .collect();
            let own = project_line(&self.project_config(&project)?);
            let listed = listed(&lines, "This repository has no environments.");
            return Ok(format!("{own}\n{listed}\n{ENVIRONMENT_GUIDE}"));
        }
        let name = asked.word("name").to_string();
        crate::paths::validate_name(&name)?;
        if op != "save" && !environments.contains_key(&name) {
            return Err(refused(format!("there is no environment `{name}`")));
        }
        if op != "save" {
            return self.propose(from, Change::EnvironmentRemove { project, name });
        }
        // What the call does not name stays as it is, ports and variables one by one.
        let mut environment = environments.get(&name).cloned().unwrap_or_default();
        merge(&mut environment.ports, asked.ports()?);
        let given = asked.variables()?;
        let variables = shown(&given);
        merge(&mut environment.env, given);
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
            variables,
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
            "appearance" => self.configure_appearance(from, op, &asked),
            "layout" => self.configure_layout(op, &asked),
            "terminal" => self.configure_terminal(op, &asked),
            "environment" => self.configure_environment(from, op, &asked),
            _ => Err(refused(
                "`kind` is one of prompt, action, rule, status_line, appearance, layout, \
                 terminal, environment. \
                 API keys, MCP servers, sign-ins, permissions, notes and how many sessions \
                 may work for one cannot be changed by an agent.",
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
/// What an agent has to know to write an environment that works for several tasks at once.
const ENVIRONMENT_GUIDE: &str = "How an environment works: `setup` runs once after a task's \
    worktree is made and has to finish (install, build, seed); `start` is started after it and \
    left running (the dev stack, in the foreground); `stop` runs before what `start` left \
    running is ended; `teardown` runs before the worktree is removed. Each runs with `sh -c` in \
    the task's worktree. Several tasks run side by side, so nothing may use a fixed port or a \
    fixed name: declare each port under `ports` with its base number and write \
    `{{port.<name>}}` wherever it is needed, in commands, variables and `open_url`; a task \
    gets base + slot * step. Also usable: {{project}} {{task}} {{branch}} {{slot}} \
    {{worktree}} {{repo}} {{user}}; the same reach commands as SWARMED_PROJECT, SWARMED_TASK, \
    SWARMED_SLOT, SWARMED_WORKTREE, SWARMED_REPO, SWARMED_BRANCH and SWARMED_PORT_<NAME>. \
    `variables` are exported to the commands and to the agents working in the task. Saving \
    changes only what you name; a port given as 0 or a variable given as an empty text is \
    removed. A task takes its environment when it is started, so try the commands in your own \
    worktree first, with the numbers a task would get, and propose what worked.";

/// `map` as `name=value` pairs in a line.
fn pairs<V: std::fmt::Display>(map: &std::collections::BTreeMap<String, V>) -> String {
    let pairs: Vec<String> = map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    pairs.join(", ")
}

/// The variables a call gives, as the user is shown them: each with its value, or as removed.
fn shown(given: &[(String, Option<String>)]) -> String {
    let one = |(name, value): &(String, Option<String>)| match value {
        Some(value) => format!("{name}={value}"),
        None => format!("{name} removed"),
    };
    given.iter().map(one).collect::<Vec<_>>().join(", ")
}

/// Puts `given` into `kept`: each entry replaces the one of its name, and `None` removes it.
fn merge<V>(kept: &mut std::collections::BTreeMap<String, V>, given: Vec<(String, Option<V>)>) {
    for (name, value) in given {
        match value {
            Some(value) => kept.insert(name, value),
            None => kept.remove(&name),
        };
    }
}

/// What the repository's own config sets up, which every environment starts from.
fn project_line(config: &crate::config::ProjectConfig) -> String {
    let variables: Vec<&str> = config.env.keys().map(String::as_str).collect();
    let parts = [
        line("setup", &config.hooks.setup),
        line("start", &config.hooks.start),
        line("stop", &config.hooks.stop),
        line("teardown", &config.hooks.teardown),
        line("address", &config.open_url),
        line("ports", &pairs(&config.ports)),
        line("variables (values withheld)", &variables.join(", ")),
    ];
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    format!(
        "The repository's own config, which an environment adds to (port step {}): {}",
        config.port_step,
        listed(&parts, "nothing set").replace('\n', "; ")
    )
}

fn environment_line(name: &str, environment: &Environment) -> String {
    let variables: Vec<&str> = environment.env.keys().map(String::as_str).collect();
    let parts = [
        line("setup", &environment.setup),
        line("start", &environment.start),
        line("stop", &environment.stop),
        line("teardown", &environment.teardown),
        line("address", &environment.open_url),
        line("ports", &pairs(&environment.ports)),
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
    let tool_calls = word(serde_json::to_value(settings.tool_calls).unwrap_or_default());
    let warnings = word(serde_json::to_value(settings.limit_warnings).unwrap_or_default());
    let math = word(serde_json::to_value(settings.math).unwrap_or_default());
    format!(
        "theme {theme} (system, dark, light); palette {palette} (dusk, graphite, forest, plum, \
         ember, swarm); style {style} (plain, swarm, colony); layout {layout} (classic, senior, custom: \
         see kind layout); \
         text_size {} (80 to 150); tool_calls {tool_calls} (all, folded, hidden); \
         turn_end_lines {} (true or false: a line where each turn ends); \
         limit_warnings {warnings} (which levels the usage notice at the top of the window says; \
         frequent: from 50% used; rare: from 90% used; none: only requests stopped); \
         math {math} (which formulas in replies are drawn as math; off; standard: $$…$$, \\(…\\), \
         \\[…\\] and fenced math blocks; with_single_dollars: $…$ too)",
        settings.text_scale, settings.turn_end_lines
    )
}
