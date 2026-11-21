//! What Swarmed does with a message that is one of an agent's own `/` commands.
//!
//! In a terminal, many of Claude Code's commands open a screen: `/usage`, `/status`, `/model`.
//! Run as Swarmed runs it, Claude Code has no screen. Each command it has is therefore one of
//! four kinds here, in one table: sent on as a message, shown as a screen whose contents
//! Claude Code is asked for, put to the user as a question whose answer Swarmed applies, or
//! refused because Swarmed has its own way to do it. A command the table does not name is
//! sent on: a skill, a command of the user's own, or one that only opens a screen in the
//! terminal, which Claude Code answers itself, marked as such.

use std::collections::{BTreeMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::agent::{AgentCommand, AgentEvent, Asked, AskedOption, Auth, Effort, ScreenKind};
use crate::error::{Error, Result};
use crate::sessions::{Session, SessionSettings, SessionStatus, skill_called};
use crate::{CLAUDE, Core};

/// What Swarmed does with a command when it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    /// It is sent to the agent as a message: a skill, or a command the agent answers in words.
    Prompt,
    /// Swarmed asks the agent for the screen's contents and shows them.
    Screen,
    /// Swarmed puts the choices to the user and makes the change itself.
    Setting,
    /// It only means something in the agent's own terminal, and is not sent.
    Terminal,
}

/// A command or skill a message can call by starting with `/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SlashCommand {
    /// The name, without its slash.
    pub name: String,
    /// What Swarmed does with it.
    pub kind: CommandKind,
    /// For one Swarmed does not send on, what to do instead; empty for any other.
    pub note: String,
}

/// A setting a command changes, and so what Swarmed asks when the command is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Setting {
    Model,
    Effort,
    /// Claude Code's own settings, a few of which matter when it runs without its screen.
    Config,
}

/// What is done with one of Claude Code's commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handling {
    /// Sent on: Claude Code answers it in words, or it is a prompt.
    Send,
    Show(ScreenKind),
    Ask(Setting),
    /// Refused, with what to do in Swarmed instead.
    Terminal(&'static str),
}

use Handling::{Ask, Send, Show, Terminal};

const MODE_MENU: &str = "What an agent may do without asking is chosen in the session's mode menu.";
const FORK: &str = "A session is forked from one of its messages.";
const SIGN_IN: &str = "Swarmed handles no sign-in: run `claude` in a terminal for it.";
const CLOSE: &str = "A session is closed with the button in its header.";
const NEW_SESSION: &str = "For an empty conversation, start a new session in the task.";
const WORKTREE: &str = "A session works in its task's worktree and nowhere else.";
const IN_SWARMED: &str = "A session runs in Swarmed, and is not handed to another app.";

/// Claude Code's own commands, each in the one kind it is handled as.
///
/// A command Claude Code gains goes in here on purpose: the test below holds every command a
/// known version reports, and fails for one this table does not name. A command that only
/// opens a screen in the terminal needs no place here unless Swarmed has its own way to do it:
/// sent, it is answered by Claude Code itself, marked, and shown as [`AgentEvent::TerminalOnly`].
const CLAUDE_COMMANDS: &[(&str, Handling)] = &[
    // Screens Claude Code describes to a host on request.
    ("usage", Show(ScreenKind::Usage)),
    // Buying usage credits and setting their cap is left to the terminal: the figures are shown.
    ("usage-credits", Show(ScreenKind::Usage)),
    ("extra-usage", Show(ScreenKind::Usage)),
    ("cost", Show(ScreenKind::Cost)),
    ("status", Show(ScreenKind::Status)),
    ("context", Show(ScreenKind::Context)),
    ("memory", Show(ScreenKind::Memory)),
    ("skills", Show(ScreenKind::Skills)),
    ("hooks", Show(ScreenKind::Hooks)),
    ("mcp", Show(ScreenKind::Mcp)),
    // Settings, which Swarmed keeps or passes on.
    ("model", Ask(Setting::Model)),
    ("effort", Ask(Setting::Effort)),
    ("config", Ask(Setting::Config)),
    // Answered by Claude Code in words, or prompts of its own.
    ("advisor", Send),
    ("agents", Send),
    ("auto-mode-setup", Send),
    ("autocompact", Send),
    ("compact", Send),
    ("design-consent", Send),
    ("design-revoke", Send),
    ("focus", Send),
    ("goal", Send),
    ("heapdump", Send),
    ("import", Send),
    ("init", Send),
    ("insights", Send),
    ("list-agents", Send),
    ("output-style", Send),
    ("recap", Send),
    ("reload-plugins", Send),
    ("reload-skills", Send),
    ("review", Send),
    ("security-review", Send),
    ("skill-doctor", Send),
    ("team-onboarding", Send),
    ("ultrareview", Send),
    // Screens and switches of the terminal that Swarmed has its own way to do.
    ("clear", Terminal(NEW_SESSION)),
    // Where a session works, and what runs it, are Swarmed's: these are refused whether or not
    // Claude Code could do them without its screen, as it can `/add-dir` behind a switch.
    ("add-dir", Terminal(WORKTREE)),
    ("cd", Terminal(WORKTREE)),
    ("desktop", Terminal(IN_SWARMED)),
    ("remote-control", Terminal(IN_SWARMED)),
    ("teleport", Terminal(IN_SWARMED)),
    ("branch", Terminal(FORK)),
    (
        "color",
        Terminal("Colours are chosen in Swarmed's settings."),
    ),
    (
        "continue",
        Terminal("A closed session is reopened from its task."),
    ),
    ("diff", Terminal("A task's changes are in its Changes tab.")),
    ("exit", Terminal(CLOSE)),
    (
        "fast",
        Terminal("Fast mode is switched beside the session's model."),
    ),
    ("fork", Terminal(FORK)),
    (
        "help",
        Terminal("The message box lists the commands as you type `/`."),
    ),
    (
        "keybindings",
        Terminal("Swarmed's shortcuts are listed in its settings."),
    ),
    ("login", Terminal(SIGN_IN)),
    ("logout", Terminal(SIGN_IN)),
    ("permissions", Terminal(MODE_MENU)),
    ("plan", Terminal(MODE_MENU)),
    (
        "rename",
        Terminal("A session is renamed by pressing its title."),
    ),
    (
        "resume",
        Terminal("A closed session is reopened from its task."),
    ),
    ("rewind", Terminal(FORK)),
    (
        "statusline",
        Terminal("The status line is set in Swarmed's settings."),
    ),
    (
        "tasks",
        Terminal("What an agent left running is listed with its task."),
    ),
    (
        "theme",
        Terminal("Colours are chosen in Swarmed's settings."),
    ),
];

/// The other names Claude Code takes for commands in the table, each with the command it
/// stands for: `/new` clears a conversation as `/clear` does.
const CLAUDE_ALIASES: &[(&str, &str)] = &[
    ("allowed-tools", "permissions"),
    ("app", "desktop"),
    ("bashes", "tasks"),
    ("checkpoint", "rewind"),
    ("name", "rename"),
    ("new", "clear"),
    ("quit", "exit"),
    ("rc", "remote-control"),
    ("reset", "clear"),
    ("settings", "config"),
    ("stats", "usage"),
    ("tp", "teleport"),
    ("undo", "rewind"),
];

/// What is done with command `name` of `agent`, by its name or another it goes by, if the
/// agent's commands are known here.
fn handling(agent: &str, name: &str) -> Option<Handling> {
    if agent != CLAUDE {
        return None;
    }
    let alias = CLAUDE_ALIASES.iter().find(|(alias, _)| *alias == name);
    let name = alias.map_or(name, |(_, command)| *command);
    let known = CLAUDE_COMMANDS.iter().find(|(known, _)| *known == name);
    known.map(|(_, how)| *how)
}

/// Commands Swarmed shows a part of, whose screen in Claude Code's terminal does the rest:
/// `/usage-credits` buys credits and sets their cap, which Swarmed leaves to Claude Code.
const SHOWN_IN_PART: &[&str] = &["usage-credits"];

/// Whether Swarmed opens the agent's own screen for `name`, without its slash: a command of
/// Claude Code's that the table leaves to Claude Code, or one it shows only in part, by a name
/// that is only a name. The others the table names have a way of Swarmed's, or are refused, as
/// `/login` and `/cd` are.
pub(crate) fn opens_its_screen(agent: &str, name: &str) -> bool {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':');
    let named = name.starts_with(|c: char| c.is_ascii_alphanumeric()) && name.chars().all(plain);
    let left = handling(agent, name).is_none() || SHOWN_IN_PART.contains(&name);
    agent == CLAUDE && named && left
}

/// A message taken apart as a command: its name without the slash, and what follows it.
fn called(text: &str) -> Option<(&str, &str)> {
    let rest = text.trim().strip_prefix('/')?;
    let (name, argument) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    (!name.is_empty()).then_some((name, argument.trim()))
}

fn kind_of(how: Handling) -> (CommandKind, &'static str) {
    match how {
        Send => (CommandKind::Prompt, ""),
        Show(_) => (CommandKind::Screen, ""),
        Ask(_) => (CommandKind::Setting, ""),
        Terminal(instead) => (CommandKind::Terminal, instead),
    }
}

/// The commands a message box offers for `agent`: the `names` it reported and the skills
/// handed to it, each with what Swarmed does with it, then the screens and settings Swarmed
/// has for the agent that the agent did not list itself. Run without its screen, Claude Code
/// leaves out the commands that need one, such as `/status`, which Swarmed can still show.
pub(crate) fn offered(agent: &str, names: Vec<String>) -> Vec<SlashCommand> {
    let entry = |name: String, how: Option<Handling>| {
        let (kind, note) = how.map_or((CommandKind::Prompt, ""), kind_of);
        let note = note.to_string();
        SlashCommand { name, kind, note }
    };
    let listed: HashSet<String> = names.iter().cloned().collect();
    let own = CLAUDE_COMMANDS
        .iter()
        .filter(|(name, how)| {
            agent == CLAUDE && matches!(how, Show(_) | Ask(_)) && !listed.contains(*name)
        })
        .map(|(name, how)| entry((*name).to_string(), Some(*how)));
    let reported = names.into_iter().map(|name| {
        let how = handling(agent, &name);
        entry(name, how)
    });
    reported.chain(own).collect()
}

/// One of Claude Code's own settings that Swarmed asks about: the name `/config` knows it by,
/// a word for it, the question, and the answers with the value each stands for.
struct ConfigChoice {
    key: &'static str,
    header: &'static str,
    question: &'static str,
    answers: &'static [(&'static str, &'static str, &'static str)],
}

/// The answer that changes nothing: every question has it, so that one can be changed alone.
const UNCHANGED: &str = "Leave as it is";

/// The answer that puts a setting back to the agent's own choice.
const DEFAULT: &str = "Default";

/// The settings of Claude Code's `/config` that matter when it runs under Swarmed. The rest
/// are about its terminal screen. `/config key=value` reaches all of them.
const CONFIG: &[ConfigChoice] = &[
    ConfigChoice {
        key: "autoCompact",
        header: "Auto-compact",
        question: "Should Claude Code compact a conversation by itself when its context fills up?",
        answers: &[
            ("On", "true", "It sums up what was said and carries on."),
            ("Off", "false", "It stops when the context is full."),
        ],
    },
    ConfigChoice {
        key: "thinking",
        header: "Thinking",
        question: "Should Claude Code think before it answers?",
        answers: &[
            ("On", "true", ""),
            ("Off", "false", "Not every model can go without."),
        ],
    },
    ConfigChoice {
        key: "outputStyle",
        header: "Output style",
        question: "How should Claude Code write its answers?",
        answers: &[
            ("Default", "default", ""),
            ("Proactive", "Proactive", "Acts at once and asks little."),
            ("Concise", "Concise", "Short, with the result first."),
            ("Explanatory", "Explanatory", "Explains its choices."),
            (
                "Learning",
                "Learning",
                "Leaves small pieces for you to write.",
            ),
        ],
    },
];

fn option(label: &str, description: &str) -> AskedOption {
    AskedOption {
        label: label.to_string(),
        description: description.to_string(),
    }
}

fn asked(question: &str, header: &str, options: Vec<AskedOption>) -> Asked {
    Asked {
        question: question.to_string(),
        header: header.to_string(),
        options,
        multi_select: false,
    }
}

const MODEL_QUESTION: &str = "Which model should this session run on?";
const EFFORT_QUESTION: &str = "How hard should the model think in this session?";

/// What a setting's command asks. `models` are the ones worth offering for the agent.
fn questions(setting: Setting, models: &[String]) -> Vec<Asked> {
    match setting {
        Setting::Model => {
            let named = models.iter().map(|model| option(model, ""));
            let own = option(DEFAULT, "The agent's own choice.");
            let options = std::iter::once(own).chain(named).collect();
            vec![asked(MODEL_QUESTION, "Model", options)]
        }
        Setting::Effort => {
            let levels = [
                Effort::Low,
                Effort::Medium,
                Effort::High,
                Effort::Xhigh,
                Effort::Max,
            ];
            let named = levels.into_iter().map(|level| option(level.as_str(), ""));
            let own = option(DEFAULT, "The model's own choice.");
            let options = std::iter::once(own).chain(named).collect();
            vec![asked(EFFORT_QUESTION, "Effort", options)]
        }
        Setting::Config => CONFIG
            .iter()
            .map(|choice| {
                let answers = choice.answers.iter();
                let named = answers.map(|(label, _, about)| option(label, about));
                let options = std::iter::once(option(UNCHANGED, "")).chain(named);
                asked(choice.question, choice.header, options.collect())
            })
            .collect(),
    }
}

/// The `/config` message that makes the changes `answers` choose, by each question's text;
/// `None` if they leave everything as it is. An answer that is none of a question's own is
/// passed on as the user wrote it, for Claude Code to take or refuse.
fn config_message(answers: &BTreeMap<String, String>) -> Option<String> {
    let changes: Vec<String> = CONFIG
        .iter()
        .filter_map(|choice| {
            let answer = answers.get(choice.question)?.trim();
            let known = choice.answers.iter().find(|(label, _, _)| *label == answer);
            let value = known.map_or(answer, |(_, value, _)| value);
            let changed = !(value.is_empty() || answer == UNCHANGED);
            changed.then(|| format!("{}={value}", choice.key))
        })
        .collect();
    (!changes.is_empty()).then(|| format!("/config {}", changes.join(" ")))
}

/// The one thing `answer` names, unless it is the default: a model's name or an effort.
fn named(command: &str, answer: &str) -> Result<Option<String>> {
    let answer = answer.trim();
    if answer.is_empty() || answer.contains(char::is_whitespace) {
        return Err(Error::NotAChoice {
            command: command.to_string(),
            answer: answer.to_string(),
        });
    }
    let own = answer.eq_ignore_ascii_case(DEFAULT);
    Ok((!own).then(|| answer.to_string()))
}

/// The effort `level` names.
fn effort_named(level: &str) -> Result<Effort> {
    Effort::parse(&level.to_lowercase()).ok_or_else(|| Error::NotAChoice {
        command: "effort".to_string(),
        answer: level.to_string(),
    })
}

/// An id for a question no other of the session has: the command, then the time.
fn request_id(command: &str) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH);
    format!("{command}-{}", now.map_or(0, |since| since.as_nanos()))
}

/// The command a [`request_id`] was made for.
fn command_of(request_id: &str) -> &str {
    request_id
        .rsplit_once('-')
        .map_or("", |(command, _)| command)
}

/// What stands open under `request_id` among `events`: the questions, unless they were
/// answered already or never asked.
fn open_questions(events: &[AgentEvent], request_id: &str) -> Option<Vec<Asked>> {
    let about = |event: &&AgentEvent| match event {
        AgentEvent::CommandAsked { request_id: id, .. }
        | AgentEvent::CommandAnswered { request_id: id, .. } => id == request_id,
        _ => false,
    };
    match events.iter().rev().find(about)? {
        AgentEvent::CommandAsked { questions, .. } => Some(questions.clone()),
        _ => None,
    }
}

impl Core {
    /// Refuses `text` with [`Error::TerminalOnly`] if it is a command Swarmed does not send
    /// on. For a message about to wait in a queue: what waits there is sent as typed, and
    /// never comes by [`Core::command_taken`].
    pub(crate) fn may_wait(&self, id: i64, text: &str) -> Result<()> {
        let session = self.session(id)?;
        let Some((name, _)) = called(text) else {
            return Ok(());
        };
        match handling(&session.agent, name) {
            Some(Terminal(instead)) if !self.calls_skill(id, text) => Err(Error::TerminalOnly {
                command: name.to_string(),
                instead: instead.to_string(),
            }),
            _ => Ok(()),
        }
    }

    /// Does what `text` asks itself, if it is one of the agent's own commands that is not
    /// simply sent on, and says whether it did. With `auth` the session's agent is started
    /// where the command needs it; without, it must be running.
    ///
    /// A command Swarmed does not send on is refused with [`Error::TerminalOnly`].
    pub(crate) fn command_taken(&self, id: i64, text: &str, auth: Option<Auth>) -> Result<bool> {
        let session = self.session(id)?;
        let Some((name, argument)) = called(text) else {
            return Ok(false);
        };
        let how = match handling(&session.agent, name) {
            None | Some(Send) => return Ok(false),
            // A skill of the user's by a command's name is what they call: it is sent.
            Some(_) if self.calls_skill(id, text) => return Ok(false),
            Some(Terminal(instead)) => {
                return Err(Error::TerminalOnly {
                    command: name.to_string(),
                    instead: instead.to_string(),
                });
            }
            Some(how) => how,
        };
        match (how, argument.is_empty()) {
            (Show(kind), true) => self.show(id, kind, auth)?,
            // With something after it, Claude Code takes the command in words itself:
            // `/mcp reconnect docs`, `/config thinking=false`.
            (Show(_) | Ask(Setting::Config), false) => {
                self.deliver(id, text.trim(), auth)?;
            }
            (Ask(setting), true) => self.ask(&session, name, setting)?,
            (Ask(setting), false) => {
                let said = self.set_as_chosen(&session, setting, name, argument)?;
                self.live.note(&self.store, id, said_plainly(name, said))?;
            }
            (Send | Terminal(_), _) => return Ok(false),
        }
        Ok(true)
    }

    /// Passes the changes `answers` choose on to Claude Code, and says what came of it. They
    /// are sent before anything is said: a conversation must not hold that Claude Code was
    /// told what never left.
    fn tell_config(
        &self,
        id: i64,
        answers: &BTreeMap<String, String>,
        auth: Auth,
    ) -> Result<String> {
        let Some(message) = config_message(answers) else {
            return Ok(NOTHING_CHANGED.to_string());
        };
        Ok(match self.deliver(id, &message, Some(auth))? {
            Delivered::Sent => format!("Claude Code was told: {message}"),
            Delivered::Queued => format!("Claude Code is told once it is free: {message}"),
        })
    }

    /// Whether `text` calls a skill that Swarmed sends to the session's agent by name.
    fn calls_skill(&self, id: i64, text: &str) -> bool {
        skill_called(&self.store, &self.paths, id, text.trim()).is_some()
    }

    /// Asks the session's agent for one of its screens, which arrives as an event.
    fn show(&self, id: i64, kind: ScreenKind, auth: Option<Auth>) -> Result<()> {
        if let Some(auth) = auth {
            self.session_start(id, auth)?;
        }
        let commands = self.live.sender(id)?;
        commands
            .send(AgentCommand::Show(kind))
            .map_err(|_| Error::SessionNotRunning(id))
    }

    /// Sends `text` to the session's agent as it stands, or has it wait while the agent is at
    /// work: sent into a turn, it would take an open request away.
    fn deliver(&self, id: i64, text: &str, auth: Option<Auth>) -> Result<Delivered> {
        let status = self.session(id)?.status;
        if matches!(status, SessionStatus::Working | SessionStatus::Waiting) {
            self.session_enqueue(id, text)?;
            return Ok(Delivered::Queued);
        }
        if let Some(auth) = auth {
            self.session_start(id, auth)?;
        }
        self.session_send_as_typed(id, text)?;
        Ok(Delivered::Sent)
    }

    /// Puts the choices of a setting's command to the user, in the conversation.
    fn ask(&self, session: &Session, command: &str, setting: Setting) -> Result<()> {
        if self.store().session_is_closed(session.id)? {
            return Err(Error::SessionClosed(session.id));
        }
        let agents = self.agents_now().infos();
        let agent = agents.iter().find(|agent| agent.id == session.agent);
        let models = agent.map_or(&[][..], |agent| &agent.models[..]);
        let asked = AgentEvent::CommandAsked {
            request_id: request_id(command),
            command: command.to_string(),
            questions: questions(setting, models),
        };
        self.live.note(&self.store, session.id, asked)
    }

    /// Makes the change a setting's command stands for, and says what was done in a line.
    fn set_as_chosen(
        &self,
        session: &Session,
        setting: Setting,
        command: &str,
        answer: &str,
    ) -> Result<String> {
        let mut settings = SessionSettings {
            model: session.model.clone(),
            supervision: session.supervision,
            effort: session.effort,
            fast: session.fast,
        };
        let chosen = named(command, answer)?;
        let shown = chosen.clone().unwrap_or_else(|| "the default".to_string());
        let said = match setting {
            Setting::Model => {
                settings.model = chosen;
                format!("This session's model is now {shown}.")
            }
            Setting::Effort => {
                settings.effort = chosen.as_deref().map(effort_named).transpose()?;
                format!("This session's effort is now {shown}.")
            }
            Setting::Config => return Ok(String::new()),
        };
        self.session_configure(session.id, &settings)?;
        Ok(said)
    }

    /// Answers the questions a command put to the user, each by its text, and makes the
    /// change they choose. `None` puts the questions aside and changes nothing. Does nothing
    /// if they are no longer open. Must be called from within a tokio runtime.
    pub fn session_command_answer(
        &self,
        id: i64,
        request_id: &str,
        answers: Option<BTreeMap<String, String>>,
        auth: Auth,
    ) -> Result<()> {
        let session = self.session(id)?;
        let Some(questions) = open_questions(&self.session_events(id)?, request_id) else {
            return Ok(());
        };
        let command = command_of(request_id);
        let Some(Ask(setting)) = handling(&session.agent, command) else {
            return Ok(());
        };
        let answered = |said: String| AgentEvent::CommandAnswered {
            request_id: request_id.to_string(),
            said,
        };
        let Some(answers) = answers else {
            let aside = answered(NOTHING_CHANGED.to_string());
            return self.live.note(&self.store, id, aside);
        };
        if setting == Setting::Config {
            let said = self.tell_config(id, &answers, auth)?;
            return self.live.note(&self.store, id, answered(said));
        }
        let first = questions
            .first()
            .and_then(|asked| answers.get(&asked.question));
        let said =
            self.set_as_chosen(&session, setting, command, first.map_or("", String::as_str))?;
        self.live.note(&self.store, id, answered(said))
    }
}

const NOTHING_CHANGED: &str = "Nothing was changed.";

/// What became of something handed to a session's agent.
enum Delivered {
    /// It went to the agent.
    Sent,
    /// The agent is at work, and it waits in the queue.
    Queued,
}

/// A setting changed by a command that named its value: the question it would have asked is
/// recorded as answered at once, so the conversation says what was done.
fn said_plainly(command: &str, said: String) -> AgentEvent {
    AgentEvent::CommandAnswered {
        request_id: request_id(command),
        said,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command of its own that Claude Code 2.1.330 reports when it starts without its
    /// screen. A later version's new command is added here and to the table together.
    const REPORTED: &[&str] = &[
        "advisor",
        "agents",
        "auto-mode-setup",
        "autocompact",
        "clear",
        "color",
        "compact",
        "config",
        "output-style",
        "context",
        "effort",
        "fast",
        "focus",
        "heapdump",
        "init",
        "mcp",
        "import",
        "model",
        "reload-plugins",
        "reload-skills",
        "rename",
        "ultrareview",
        "security-review",
        "usage-credits",
        "extra-usage",
        "usage",
        "insights",
        "recap",
        "skill-doctor",
        "goal",
        "design-consent",
        "design-revoke",
        "list-agents",
        "team-onboarding",
    ];

    #[test]
    fn every_command_claude_code_reports_is_in_the_table_once() {
        for name in REPORTED {
            assert!(
                handling(CLAUDE, name).is_some(),
                "/{name} is in none of the four kinds"
            );
        }
        let mut names: Vec<&str> = CLAUDE_COMMANDS.iter().map(|(name, _)| *name).collect();
        let all = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all, "a command is in the table twice");
    }

    #[test]
    fn every_refused_command_says_what_to_do_instead() {
        for (name, how) in CLAUDE_COMMANDS {
            assert!(
                !matches!(how, Terminal(instead) if instead.is_empty()),
                "/{name}"
            );
        }
    }

    #[test]
    fn every_screen_has_its_command_and_no_screen_command_reaches_the_model() {
        for kind in ScreenKind::ALL {
            assert_eq!(handling(CLAUDE, kind.command()), Some(Show(kind)));
        }
        // What opens a screen in the terminal is never among what is sent on as a message.
        for screen in [
            "usage",
            "status",
            "config",
            "model",
            "memory",
            "hooks",
            "permissions",
        ] {
            assert_ne!(handling(CLAUDE, screen), Some(Send), "/{screen}");
        }
    }

    #[test]
    fn a_command_called_by_another_name_is_handled_as_itself() {
        for (alias, command) in CLAUDE_ALIASES {
            assert!(
                handling(CLAUDE, command).is_some(),
                "/{alias} stands for no command"
            );
            assert_eq!(
                handling(CLAUDE, alias),
                handling(CLAUDE, command),
                "/{alias}"
            );
            assert!(
                CLAUDE_COMMANDS.iter().all(|(name, _)| name != alias),
                "/{alias} is in the table as well"
            );
        }
        // Run under Swarmed, Claude Code would empty its conversation while the session went on
        // showing it: a new session is an empty one.
        for name in ["clear", "new", "reset"] {
            assert_eq!(
                handling(CLAUDE, name),
                Some(Terminal(NEW_SESSION)),
                "/{name}"
            );
        }
        // Where a session works, and what runs it, are never left to Claude Code.
        for name in [
            "cd",
            "add-dir",
            "remote-control",
            "rc",
            "teleport",
            "desktop",
        ] {
            assert!(
                matches!(handling(CLAUDE, name), Some(Terminal(note)) if !note.is_empty()),
                "/{name}"
            );
        }
        // Without its screen, Claude Code only says to run it in a terminal.
        assert_eq!(
            handling(CLAUDE, "usage-credits"),
            Some(Show(ScreenKind::Usage))
        );
        // A command that only opens a screen is Claude Code's to say so of, and one that has
        // come to work without a screen, as `/doctor` has, is not refused by an old guess.
        assert_eq!(handling(CLAUDE, "privacy-settings"), None);
        assert_eq!(handling(CLAUDE, "doctor"), None);
    }

    #[test]
    fn only_a_command_left_to_claude_code_opens_its_screen() {
        assert!(opens_its_screen(CLAUDE, "vim"));
        assert!(opens_its_screen(CLAUDE, "privacy-settings"));
        // Shown here in part: Claude Code's own screen buys the credits.
        assert!(opens_its_screen(CLAUDE, "usage-credits"));
        // Swarmed's own way, or refused: colours, sign-in, where the session works, what runs it.
        for name in [
            "model", "usage", "theme", "login", "cd", "add-dir", "clear", "new", "rc",
        ] {
            assert!(!opens_its_screen(CLAUDE, name), "/{name}");
        }
        // Only a name: nothing that would be read as an option or a path.
        for name in ["", "-p", "--resume", "../x", "theme dark", "the/me"] {
            assert!(!opens_its_screen(CLAUDE, name), "{name:?}");
        }
        assert!(!opens_its_screen("codex", "vim"));
    }

    #[test]
    fn other_agents_commands_and_unknown_names_are_sent_on() {
        assert_eq!(handling("codex", "usage"), None);
        assert_eq!(handling(CLAUDE, "my-own-skill"), None);
        assert_eq!(handling(CLAUDE, "plugin:review"), None);
    }

    #[test]
    fn a_message_is_a_command_by_its_first_word() {
        assert_eq!(called("/usage"), Some(("usage", "")));
        assert_eq!(called("  /model   opus \n"), Some(("model", "opus")));
        assert_eq!(
            called("/mcp reconnect docs"),
            Some(("mcp", "reconnect docs"))
        );
        assert_eq!(
            called("/config\nthinking=false"),
            Some(("config", "thinking=false"))
        );
        assert_eq!(called("look at /usage"), None);
        assert_eq!(called("/"), None);
        assert_eq!(called(""), None);
    }

    #[test]
    fn the_menu_says_what_becomes_of_each_command_and_adds_swarmeds_own_screens() {
        let reported = ["usage", "review", "my-skill", "rename"].map(String::from);
        let menu = offered(CLAUDE, reported.to_vec());
        let kind = |name: &str| {
            let found = menu.iter().find(|command| command.name == name);
            found.map(|command| command.kind)
        };
        assert_eq!(kind("usage"), Some(CommandKind::Screen));
        assert_eq!(kind("review"), Some(CommandKind::Prompt));
        assert_eq!(kind("my-skill"), Some(CommandKind::Prompt));
        assert_eq!(kind("rename"), Some(CommandKind::Terminal));
        // Not reported by Claude Code without its screen, and shown by Swarmed all the same.
        assert_eq!(kind("status"), Some(CommandKind::Screen));
        assert_eq!(kind("model"), Some(CommandKind::Setting));
        // What only works in a terminal is offered only where the agent offers it.
        assert_eq!(kind("login"), None);
        assert_eq!(
            menu.iter()
                .filter(|command| command.name == "usage")
                .count(),
            1
        );
        let rename = menu.iter().find(|command| command.name == "rename");
        assert_eq!(
            rename.map(|command| command.note.as_str()),
            Some("A session is renamed by pressing its title.")
        );
        // The names keep the order they came in, and the additions follow.
        assert_eq!(menu[0].name, "usage");
        assert_eq!(menu[3].name, "rename");

        // Another agent's commands are all its own to answer.
        let other = offered("codex", vec!["usage".to_string()]);
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].kind, CommandKind::Prompt);
    }

    #[test]
    fn a_settings_command_asks_with_the_default_first_and_the_models_of_the_agent() {
        let models = ["opus".to_string(), "sonnet".to_string()];
        let asked = questions(Setting::Model, &models);
        let labels: Vec<&str> = asked[0].options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Default", "opus", "sonnet"]);
        assert_eq!(asked[0].question, MODEL_QUESTION);

        let asked = questions(Setting::Effort, &[]);
        let labels: Vec<&str> = asked[0].options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Default", "low", "medium", "high", "xhigh", "max"]);

        let asked = questions(Setting::Config, &[]);
        assert_eq!(asked.len(), CONFIG.len());
        assert!(asked.iter().all(|one| one.options[0].label == UNCHANGED));
        assert!(asked.iter().all(|one| !one.multi_select));
        // The questions are told apart by their text, which is what answers are filed under.
        let mut texts: Vec<&str> = asked.iter().map(|one| one.question.as_str()).collect();
        texts.dedup();
        assert_eq!(texts.len(), CONFIG.len());
    }

    #[test]
    fn config_answers_become_one_message_of_only_what_changes() {
        let answers = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            let pairs = pairs.iter();
            pairs
                .map(|(q, a)| ((*q).to_string(), (*a).to_string()))
                .collect()
        };
        let [compact, thinking, style] =
            [CONFIG[0].question, CONFIG[1].question, CONFIG[2].question];
        let all = answers(&[(compact, "Off"), (thinking, UNCHANGED), (style, "Concise")]);
        assert_eq!(
            config_message(&all).as_deref(),
            Some("/config autoCompact=false outputStyle=Concise")
        );
        let none = answers(&[
            (compact, UNCHANGED),
            (thinking, UNCHANGED),
            (style, UNCHANGED),
        ]);
        assert_eq!(config_message(&none), None);
        assert_eq!(config_message(&answers(&[])), None);
        // A style of the user's own is passed on as they wrote it.
        let own = answers(&[(style, "Terse")]);
        assert_eq!(
            config_message(&own).as_deref(),
            Some("/config outputStyle=Terse")
        );
        // An answer to a question that was never asked changes nothing.
        assert_eq!(config_message(&answers(&[("Shall I?", "On")])), None);
    }

    #[test]
    fn an_answer_names_one_thing_or_the_default() {
        assert_eq!(
            named("model", " opus ").expect("a model"),
            Some("opus".to_string())
        );
        assert_eq!(named("model", "default").expect("the default"), None);
        assert_eq!(named("model", DEFAULT).expect("the default"), None);
        assert!(matches!(named("model", ""), Err(Error::NotAChoice { .. })));
        assert!(matches!(
            named("model", "two words"),
            Err(Error::NotAChoice { .. })
        ));
    }

    #[test]
    fn a_question_is_open_until_it_is_answered() {
        let id = request_id("model");
        assert_eq!(command_of(&id), "model");
        assert_eq!(command_of(&request_id("output-style")), "output-style");
        let asked = AgentEvent::CommandAsked {
            request_id: id.clone(),
            command: "model".to_string(),
            questions: questions(Setting::Model, &[]),
        };
        let answered = AgentEvent::CommandAnswered {
            request_id: id.clone(),
            said: String::new(),
        };
        assert!(open_questions(std::slice::from_ref(&asked), &id).is_some());
        assert_eq!(open_questions(&[asked.clone(), answered], &id), None);
        assert_eq!(open_questions(&[asked], "model-0"), None);
        assert_eq!(open_questions(&[], &id), None);
    }
}
