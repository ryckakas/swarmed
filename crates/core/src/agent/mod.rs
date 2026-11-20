//! The interface between the core and whatever runs an agent session.
//!
//! The rest of the core only sees [`AgentEvent`]s coming out and [`AgentCommand`]s going in.
//! [`ClaudeBackend`] drives the Claude Code CLI, [`AcpBackend`] any agent that speaks the Agent
//! Client Protocol, and [`FakeBackend`] is a scripted stand-in for tests and front-end work.

mod acp;
mod claude;
mod fake;

use std::fmt;
use std::path::PathBuf;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

pub use self::acp::AcpBackend;
pub use self::claude::{ClaudeBackend, ClaudeNamer};
pub use self::fake::FakeBackend;
use crate::error::Result;
use crate::mcp::McpServer;
use crate::queue::QueuedMessage;
use crate::secrets::SecretEnv;

/// How much the agent may do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Supervision {
    /// File edits go through; shell commands ask unless an allow rule permits them.
    #[default]
    Ask,
    /// Claude Code's auto mode decides which actions are safe.
    Auto,
    /// Nothing asks. Never a default.
    Bypass,
    /// The agent only reads and plans. It changes nothing until the user approves its plan,
    /// which it presents by asking to use [`PLAN_TOOL`].
    Plan,
}

/// The tool an agent in plan mode asks to use when its plan is ready. Allowing it approves the
/// plan, which is the `plan` of its input.
pub const PLAN_TOOL: &str = "ExitPlanMode";

impl Supervision {
    /// The value stored in the database and shown to the user.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Auto => "auto",
            Self::Bypass => "bypass",
            Self::Plan => "plan",
        }
    }

    /// The inverse of [`Supervision::as_str`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ask" => Some(Self::Ask),
            "auto" => Some(Self::Auto),
            "bypass" => Some(Self::Bypass),
            "plan" => Some(Self::Plan),
            _ => None,
        }
    }
}

/// How hard the model thinks before it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    /// The least thinking: quickest and cheapest.
    Low,
    /// A middle course.
    Medium,
    /// More thinking.
    High,
    /// More still.
    Xhigh,
    /// As much as the model can.
    Max,
}

impl Effort {
    /// The value stored in the database and passed to the agent.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    /// The inverse of [`Effort::as_str`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Low, Self::Medium, Self::High, Self::Xhigh, Self::Max]
            .into_iter()
            .find(|effort| effort.as_str() == s)
    }
}

/// Where the agent's credentials come from. Swarmed only shapes the child's environment.
#[derive(Clone, Default)]
pub enum Auth {
    /// The CLI's own stored login. Credential variables are removed from the child's environment.
    #[default]
    Subscription,
    /// This key is passed to the child as `ANTHROPIC_API_KEY`.
    ApiKey(String),
    /// The parent environment is passed through unchanged (Bedrock, Vertex, custom setups).
    Inherit,
}

impl fmt::Debug for Auth {
    /// Never prints the key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Subscription => "Subscription",
            Self::ApiKey(_) => "ApiKey(<redacted>)",
            Self::Inherit => "Inherit",
        })
    }
}

/// Everything needed to start, or resume, one agent session.
#[derive(Debug, Clone, Default)]
pub struct SessionSpec {
    /// The task's worktree.
    pub cwd: PathBuf,
    /// The model to use; `None` leaves the agent's own default.
    pub model: Option<String>,
    /// How much the agent may do without asking.
    pub supervision: Supervision,
    /// How hard the model thinks; `None` leaves the agent's own default.
    pub effort: Option<Effort>,
    /// Where the credentials come from.
    pub auth: Auth,
    /// The agent's own session id from an earlier run, to continue that conversation.
    pub resume: Option<String>,
    /// Tool rules that never prompt, in Claude Code's rule syntax.
    pub allowed_tools: Vec<String>,
    /// Tool rules that are always refused.
    pub disallowed_tools: Vec<String>,
    /// Extra instructions appended to the agent's system prompt.
    pub append_system_prompt: Option<String>,
    /// MCP servers to give the agent, on top of any it finds in its own configuration.
    pub mcp_servers: Vec<McpServer>,
    /// The user's API keys, for agents that bring no login of their own. Claude Code is never
    /// given these: its credentials are chosen by [`SessionSpec::auth`].
    pub env: SecretEnv,
}

/// One MCP server a session loaded, and whether it connected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerInfo {
    /// The server's configured name.
    pub name: String,
    /// The agent's word for its state, such as `connected`, `failed` or `needs-auth`.
    pub status: String,
}

/// One part of what fills an agent's context window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCategory {
    /// What the part is, as the agent names it: "Messages", "Skills", "System prompt".
    pub name: String,
    /// How many tokens it takes.
    pub tokens: u64,
    /// The agent's word for what kind of part it is: `used`, `free`, `buffer` or `deferred`.
    pub kind: String,
}

/// What an agent session reports.
///
/// Events are stored as JSON, so variants and fields are only ever added, never renamed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// The message the user sent. Recorded by the core, not produced by a backend.
    UserMessage {
        /// The message as typed.
        text: String,
    },
    /// The agent process is up.
    Init {
        /// The agent's own id for the conversation; what [`SessionSpec::resume`] takes.
        agent_session_id: String,
        /// The model the session runs on.
        model: String,
        /// The agent's name for its permission mode.
        permission_mode: String,
        /// Where the agent found its credentials; `none` means a stored subscription login.
        api_key_source: String,
        /// Every MCP server the session loaded.
        mcp_servers: Vec<McpServerInfo>,
        /// How many skills the session loaded.
        skills: usize,
        /// The commands and skills a message can call by starting with `/`, without the slash.
        #[serde(default)]
        slash_commands: Vec<String>,
    },
    /// A piece of the agent's reply.
    Text {
        /// Markdown text.
        text: String,
    },
    /// A piece of a reply that is still being written. It is published as it arrives and
    /// never stored: the whole reply follows as one [`AgentEvent::Text`].
    TextDelta {
        /// The next few characters.
        text: String,
    },
    /// The agent calls a tool.
    ToolUse {
        /// Matches the later [`AgentEvent::ToolResult`].
        id: String,
        /// The tool's name.
        name: String,
        /// The tool's arguments.
        input: Value,
    },
    /// A tool call finished.
    ToolResult {
        /// The [`AgentEvent::ToolUse`] this answers.
        tool_use_id: String,
        /// The result, flattened to text.
        content: String,
        /// Whether the tool reported a failure.
        is_error: bool,
    },
    /// The agent wants to do something that needs a decision.
    PermissionRequest {
        /// What [`AgentCommand::Allow`] and [`AgentCommand::Deny`] refer to.
        request_id: String,
        /// The tool that wants to run.
        tool_name: String,
        /// The tool's arguments.
        input: Value,
        /// The agent's own summary of the action, if it gave one.
        description: Option<String>,
    },
    /// The user decided a [`AgentEvent::PermissionRequest`]. Recorded by the core, not produced by
    /// a backend.
    PermissionAnswered {
        /// The request that was decided.
        request_id: String,
        /// Whether it was allowed.
        allowed: bool,
    },
    /// The agent no longer needs a decision on a [`AgentEvent::PermissionRequest`].
    PermissionWithdrawn {
        /// The request that was taken back.
        request_id: String,
    },
    /// The session got a new title, from the user or from a [`Namer`]. Recorded by the core.
    Renamed {
        /// The new title.
        title: String,
    },
    /// What happened in the session's terminal since the agent's last turn, which the agent was
    /// given along with the user's message. Recorded by the core.
    TerminalShared {
        /// The terminal's output as plain text.
        text: String,
    },
    /// The session's supervision changed, by the user's choice or because a plan was approved.
    /// Recorded by the core.
    SupervisionChanged {
        /// What it is from now on.
        supervision: Supervision,
    },
    /// The session was closed, by the user or by the session that started it. Recorded by the
    /// core, so that every front end takes it out of its lists.
    Closed,
    /// HTML the agent showed the user through Swarmed's own tool. Recorded by the core, which
    /// gets it from the tool and not from the shape of the agent's call.
    View {
        /// The document or fragment to show.
        html: String,
    },
    /// What the agent says it actually runs with, which settings left at their default do not
    /// tell.
    Applied {
        /// The model in use.
        model: String,
        /// How hard it thinks, if the agent has such a setting.
        effort: Option<String>,
    },
    /// What the agent expects the user to say next, offered after a turn. It stands until the
    /// conversation moves on.
    Suggestion {
        /// The message, ready to be sent as it is.
        text: String,
    },
    /// What the session's queue of waiting messages now holds. Published by the core whenever
    /// it changes, and never stored.
    Queue {
        /// The waiting messages, first to be sent first.
        messages: Vec<QueuedMessage>,
    },
    /// How full the agent's context window is, reported after a turn.
    ContextUsage {
        /// Tokens in use.
        used: u64,
        /// The size of the window.
        max: u64,
        /// What the tokens are spent on, if the agent says. Empty otherwise.
        categories: Vec<ContextCategory>,
    },
    /// Usage-limit state.
    RateLimit {
        /// `allowed`, or the agent's word for a limit that was hit.
        status: String,
        /// When the current window resets, as a Unix timestamp.
        resets_at: Option<i64>,
        /// The full report, including utilisation per window.
        raw: Value,
    },
    /// The turn ended and the agent waits for the next message.
    TurnDone {
        /// Whether the turn failed or was interrupted.
        is_error: bool,
        /// The agent's closing summary, if any.
        result: Option<String>,
        /// What the agent process has cost since it started, in US dollars. A resumed session
        /// starts counting again.
        cost_usd: Option<f64>,
        /// How long the turn took, from the message that started it to here, in
        /// milliseconds. Measured by Swarmed, so it is the same for every agent; absent from
        /// turns recorded before it was.
        #[serde(default)]
        duration_ms: Option<u64>,
    },
    /// The agent process ended. Nothing follows.
    Exited {
        /// The exit code, if the process was not killed by a signal.
        code: Option<i32>,
        /// The end of what the process wrote to stderr.
        stderr: String,
    },
    /// The session was forked from another: what stands before this is a copy of what was
    /// said there. Recorded by the core, not produced by a backend.
    Forked {
        /// The session it was forked from.
        from: i64,
    },
    /// Something this version does not understand; kept so nothing is lost silently.
    Other {
        /// The message as received.
        raw: Value,
    },
}

/// What a front end can tell a running session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCommand {
    /// Start a turn with this message.
    UserMessage(String),
    /// Let a pending [`AgentEvent::PermissionRequest`] go ahead.
    Allow {
        /// The request being answered.
        request_id: String,
    },
    /// Refuse a pending [`AgentEvent::PermissionRequest`].
    Deny {
        /// The request being answered.
        request_id: String,
        /// Shown to the agent, so it can try something else.
        reason: String,
    },
    /// Stop the current turn; the session stays usable.
    Interrupt,
    /// Run on this model from the next turn on; `None` is the agent's default.
    SetModel(Option<String>),
    /// Change how much the agent may do without asking, from now on.
    SetSupervision(Supervision),
}

/// A running session. Dropping `commands` ends it.
#[derive(Debug)]
pub struct AgentHandle {
    /// Everything the session reports, ending with [`AgentEvent::Exited`].
    pub events: mpsc::UnboundedReceiver<AgentEvent>,
    /// The way to talk to the session.
    pub commands: mpsc::UnboundedSender<AgentCommand>,
    /// The agent's process, if it is one, for seeing what it takes.
    pub pid: Option<u32>,
}

/// What a [`Namer`] returns: a title, or `None` if it could not think of one in time.
pub type Naming = Pin<Box<dyn Future<Output = Option<String>> + Send>>;

/// Something that can give a conversation a short title from its first message.
pub trait Namer: fmt::Debug + Send + Sync {
    /// A title of a few words for a conversation that starts with `prompt`.
    fn name(&self, prompt: String) -> Naming;
}

/// Something that can run agent sessions.
pub trait AgentBackend: fmt::Debug + Send + Sync {
    /// Starts a session. Must be called from within a tokio runtime.
    fn start(&self, spec: SessionSpec) -> Result<AgentHandle>;
}
