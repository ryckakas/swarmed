# Coding agents

[← Back to README](../README.md)

Sessions run on Claude Code unless you pick another agent. Swarmed never signs in to an agent and
never reads the login it keeps: sign in with the agent's own command first, and Swarmed runs it
on that login.

## The agents Swarmed knows

| Agent | Runs | Installed from Settings → Agents with | Signing in |
| --- | --- | --- | --- |
| Claude Code | `claude` | `curl -fsSL https://claude.ai/install.sh \| bash` | run `claude` once and follow `/login` |
| Codex | `codex-acp` | `brew install codex-acp` | `codex login`, or `codex-acp login` |
| OpenCode | `opencode acp` | `brew install anomalyco/tap/opencode-v2` | none: it uses the API keys stored in Settings |
| Antigravity | `agy acp` | `curl -fsSL https://antigravity.google/cli/install.sh \| bash` | run `agy` once and sign in with Google |

Settings shows the command before running it. Installing Codex, OpenCode or Antigravity also
writes it into `agents.toml` with its full path and the models to offer: Codex's and
Antigravity's are asked of the agent, and OpenCode starts with a few, with OpenRouter's public
list to pick more from in Settings. `swarmed agent` does the same from a terminal.

An agent other than Claude Code is offered for a session only while its program can be found: on
your shell's `PATH`, or in `/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin` or
`~/.opencode/bin`.

## Claude Code

Claude Code is built in. Swarmed runs your installed `claude` with your own login, settings and
plugins, and speaks to it in its line-delimited JSON. It gets none of the API keys stored in
Swarmed, and no variable from a task's environment whose name starts with `ANTHROPIC_` or
`CLAUDE`. From the command line, `--auth api-key` runs it on the `ANTHROPIC_API_KEY` of the shell
you run `swarmed` from, and `--auth inherit` passes that shell's environment on unchanged, for
Bedrock or Vertex.

### Usage limits and extra usage

The card that opens over the context figure in the status line shows the following for a Claude
Code session:

- **Usage limits:** how much of each window (5-hour, weekly) is used, and when it resets.
- **Extra usage:** what is spent beyond your plan this month, against your monthly cap.
- **Spent in this session:** what the session's own turns cost.

Swarmed only learns these figures from what Claude Code says while it works for a session. It
never asks for them on a timer or when the card opens. So they can be out of date:

- The usage limits are as Claude Code reported them at the end of the session's last turn.
- The extra usage is as it stood when Claude Code last started for the session, or last ran a
  turn on extra usage. Claude Code starts with the session's first message after Swarmed opens,
  or after the session was stopped. A session that has not done either since this was added
  shows no extra usage.
- Usage from other sessions, other devices or claude.ai shows up after the session's next turn.

For figures as they are now, press the coins at the top right of a Claude Code session. They
open an overview of the same figures, with a larger bar for each, and ask the session's Claude
Code for them as they open, starting it if it is not running; **Ask again** asks once more.
Nothing is sent to the model, so it costs nothing, and nothing is added to the conversation.
Typing `/usage` in the session asks the same and shows the answer in the conversation. Either
way, what Claude Code answers is kept, so the card shows it too until a later turn reports
newer figures. Other agents report no usage, so their sessions have no coins.

## Any other agent

Any agent that speaks the [Agent Client Protocol](https://agentclientprotocol.com) can be added
in `agents.toml` in the data directory:

```toml
[goose]
name = "Goose"            # shown in the agent menu; the id if left out
command = "goose"         # a program on the PATH, or a full path
args = ["acp"]
models = ["provider/model"]   # offered in the model menu

[goose.env]               # extra environment, over the stored API keys
# SOME_SETTING = "..."
```

A key Swarmed does not know, or an entry named `claude`, is an error that keeps Swarmed from
opening, so check the file after editing it. Swarmed reads it when it starts and whenever
Settings changes an agent, so restart it after editing by hand.

Agents other than Claude Code get the API keys stored in Settings → API keys (OpenRouter, OpenAI,
Anthropic, Google Gemini) as `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY` and
`GEMINI_API_KEY`. The keys live in the macOS Keychain, and Swarmed never shows them again.

## Supervision

| Setting | Claude Code runs in | Another agent |
| --- | --- | --- |
| Ask | `acceptEdits`: edits go through, everything else asks | its read-only, ask or default mode |
| Auto | `auto`: Claude Code decides what is safe | its agent or auto mode |
| Plan | `plan`: it reads, then presents a plan to approve | its plan, read-only, ask or default mode |

Swarmed picks the first of those modes the agent has; the app offers Plan for Claude Code alone.
An agent with neither an agent nor an auto mode has its permission requests allowed by Swarmed
in Auto, so it runs unsupervised there: use Ask for it. Permission requests and questions are
answered in the conversation. Approving a plan moves the
session to the default supervision from the settings, or to Ask if that default is Plan itself.
Models and effort are chosen per session, and can change mid-session.

## MCP servers: one pool

When a session starts, Swarmed reads the MCP servers set up for Claude Code, Codex and OpenCode,
and in Swarmed itself, and gives the session's agent the ones it lacks by name, as they are
configured, secrets included. Nothing is copied into an agent's own files.

| From | Read from |
| --- | --- |
| Claude Code | `~/.claude.json` (your servers, and yours for the repository), the repository's `.mcp.json`, and your user-scope plugins |
| Codex | `~/.codex/config.toml` |
| OpenCode | `~/.config/opencode/opencode.json` or `opencode.jsonc` |
| Swarmed | `mcp.toml` in the data directory, set up in Settings → MCP servers |

- Claude Code, Codex and OpenCode are not given a server by a name they already have, switched
  on or off. The configuration of other agents is not read, so they get the whole pool.
- Servers set up in Swarmed go to every agent, Claude Code included, in place of any of the same
  name. Their secrets are kept in the Keychain, and `mcp.toml` holds only their names.
- A server from the repository's `.mcp.json` goes to other agents only after you approved it in
  Claude Code, in your own settings rather than a file the repository tracks, and never once you
  rejected it.
- An agent that cannot take HTTP servers gets only the ones it runs as programs.
- `shared_mcp_servers = false` on an entry in `agents.toml` gives that agent only Swarmed's own
  servers.

`swarmed mcp` lists the pool: the servers set up in Swarmed, Claude Code, Codex and OpenCode.

## Skills

A skill is a folder holding a `SKILL.md`. Swarmed reads them from `~/.claude/skills`,
`~/.codex/skills`, `~/.config/opencode/skills`, `~/.agents/skills`, Claude Code's user-scope
plugins, `skills/` in the data directory, and the task's own `.claude/skills` and
`.agents/skills`. Each agent is offered the skills it does not read by itself, and `/name`
calls one on any agent. Settings → Skills can keep a skill from agents that don't read it
themselves.
`swarmed skills --agent <id>` lists what an agent would be offered.

## What Swarmed gives every agent

Every session gets an MCP server named `swarmed`, with these tools:

| Tool | What it does |
| --- | --- |
| `show_html` | shows HTML in the conversation, in a sandboxed frame |
| `list_sessions`, `read_session` | see and read the other open sessions of the same project |
| `send_message` | write to another session; queued if it is busy |
| `spawn_session`, `close_session` | start a session in the same task, up to six, and close the idle ones it started |
| `delegate_to_cloud` | hand work to a Codex cloud agent or a Claude Code cloud session |
| `use_skill` | read a skill Swarmed offers |
| `set_mark` | put a colour and a few words on its own session in the sidebar |
| `configure` | change Swarmed's own settings, within limits |

Claude Code may call `show_html` without asking; in Ask mode you are asked about the rest. Other
agents decide for themselves which calls they ask about.

`configure` makes some changes at once: saved prompts, actions that send a prompt, the status
line's pieces, the looks, the terminal's settings, and removing an action, a prompt or the status
line's script. Adding a rule, saving an environment, an action that runs a command, or a status
line script it can only propose: the change is shown in the conversation and made when you
accept it. API keys, MCP servers, sign-ins, supervision and notes are out of its reach, and it
tells an agent the names of an environment's variables, never their values.
