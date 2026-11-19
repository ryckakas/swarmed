// The shapes swarmed-core serialises. Field names are the core's, so they stay snake_case.

export type Supervision = "ask" | "auto" | "bypass";
export type SessionStatus = "idle" | "working" | "waiting" | "errored";
export type SetupStatus = "pending" | "ok" | "failed";

export interface Session {
    id: number;
    task_id: number;
    /** The id of the agent that runs the session. */
    agent: string;
    title: string;
    agent_session_id: string | null;
    model: string | null;
    supervision: Supervision;
    status: SessionStatus;
}

export interface Task {
    id: number;
    project_id: number;
    name: string;
    branch: string;
    base_ref: string;
    worktree_path: string;
    slot: number;
    setup: SetupStatus;
    archived: boolean;
    sessions: Session[];
    /** Ids of the sessions whose agent is running. */
    running: number[];
}

export interface Project {
    id: number;
    name: string;
    repo_path: string;
    tasks: Task[];
}

export interface McpServerInfo {
    name: string;
    status: string;
}

export type AgentEvent =
    | { type: "user_message"; text: string }
    | {
          type: "init";
          agent_session_id: string;
          model: string;
          permission_mode: string;
          api_key_source: string;
          mcp_servers: McpServerInfo[];
          skills: number;
          /** The commands and skills a message can call with `/`, without the slash. */
          slash_commands: string[];
      }
    | { type: "text"; text: string }
    /** A piece of a reply still being written; the whole reply follows as `text`. */
    | { type: "text_delta"; text: string }
    | { type: "tool_use"; id: string; name: string; input: unknown }
    | { type: "tool_result"; tool_use_id: string; content: string; is_error: boolean }
    | {
          type: "permission_request";
          request_id: string;
          tool_name: string;
          input: unknown;
          description: string | null;
      }
    | { type: "permission_answered"; request_id: string; allowed: boolean }
    | { type: "permission_withdrawn"; request_id: string }
    | { type: "rate_limit"; status: string; resets_at: number | null; raw: unknown }
    | { type: "turn_done"; is_error: boolean; result: string | null; cost_usd: number | null }
    | { type: "exited"; code: number | null; stderr: string }
    | { type: "other"; raw: unknown };

export interface SessionEvent {
    session_id: number;
    /** The session's status after this event, if the event changed it. */
    status: SessionStatus | null;
    event: AgentEvent;
}

/** One line a hook printed, with the task it belongs to. */
export interface HookOutput {
    project: string;
    task: string;
    line: string;
}

/** What a new task in a project can start from. */
export interface BranchChoices {
    /** The ref a new branch would be created from. */
    base: string;
    /** Existing branches a task could check out instead. */
    existing: string[];
}

/** An agent sessions can run on. */
export interface AgentInfo {
    id: string;
    name: string;
    /** Model names worth offering for it. */
    models: string[];
}

/** How a new session should run. */
/** What can be changed about a session after it was created. */
export interface SessionSettings {
    /** A model name the agent accepts, or `null` for its default. */
    model: string | null;
    supervision: Supervision;
}

export interface SessionSetup {
    /** An agent id, or `null` for Claude Code. */
    agent: string | null;
    /** A model name the agent accepts, or `null` for its default. */
    model: string | null;
    supervision: Supervision;
}
