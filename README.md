# Swarmed

![Swarmed, a Rust agent orchestrator: a bear in power armour facing a swarm](docs/cover.jpg)

Parallel Claude Code sessions, each in its own git worktree with a fully isolated dev stack.
A personal tool with a Rust core; the desktop app (Tauri) comes later.

Status: early. The core and CLI work. In the desktop app you can start sessions, talk to the
agent, answer its permission requests and stop a turn. Add a repository from the sidebar, then
start a task by typing what you want done and choosing the repository, branch, supervision and
model under the message box. There is no view of the changes yet.

## Layout

- `crates/core` — `swarmed-core`, the headless core. No UI dependency.
- `crates/cli` — the `swarmed` command, a thin front end over the core.
- `crates/app` — `swarmed-app`, the desktop app: a Tauri shell over the core.
- `ui` — the app's front end, in React and TypeScript.

## Try it

```sh
cargo run -p swarmed-cli -- project add /path/to/repo
$EDITOR "$(cargo run -q -p swarmed-cli -- project config <project>)"
cargo run -p swarmed-cli -- task new <project> <task>
cargo run -p swarmed-cli -- task list <project>
cargo run -p swarmed-cli -- session run <project> <task> "<prompt>"
cargo run -p swarmed-cli -- session send <session-id> "<prompt>"
cargo run -p swarmed-cli -- task rm <project> <task>
```

Sessions run your installed `claude` with its own login and configuration. `--auto` uses Claude
Code's auto mode; without it, file edits go through and other actions ask. Set
`SWARMED_FAKE_AGENT=1` to try the commands against a scripted agent that spends no tokens.

Worktrees are created under `~/swarmed/<project>/<task>`. Config and state live in
`~/Library/Application Support/swarmed/`; nothing is written into your repositories.
`SWARMED_DATA_DIR` and `SWARMED_WORKTREES_DIR` override both locations.

## The desktop app

```sh
corepack enable && pnpm install
pnpm app            # builds and opens the app
pnpm dev            # the UI alone in a browser, on sample data
```

## Other agents

Sessions run on Claude Code unless told otherwise. Any agent that speaks the
[Agent Client Protocol](https://agentclientprotocol.com) can be added in `agents.toml` in the data
directory (`swarmed agents` prints its path):

```toml
[opencode]
name = "OpenCode"
command = "opencode"
args = ["acp"]
models = ["openrouter/openai/gpt-5.6"]   # offered in the model menu

[opencode.env]                           # optional extra environment for the agent
# OPENROUTER_API_KEY = "..."
```

OpenCode can also be installed and given its models from Settings → Agents, without editing
the file.

The agent keeps its own credentials: sign in with the agent's own command first (for OpenCode,
`opencode auth login`). It then appears in the app's agent menu and as `--agent opencode` in the
CLI.

What such a session gets:

- **MCP servers:** one pool for every agent. When a session starts, Swarmed reads the servers
  configured for Claude Code (your own, the repository's `.mcp.json`, yours for that
  repository, and the ones its plugins bring), for Codex and for OpenCode, and hands the
  session's agent the ones it does not have by that name. Nothing is copied into an agent's
  files. Set `shared_mcp_servers = false` on an agent to turn that off. HTTP servers only go
  to agents that accept them, and a server you signed in to in a browser stays signed in only
  in the agent you did that in.
- **Skills:** nothing to do for agents that read `~/.claude/skills` and `.claude/skills`
  themselves, which OpenCode and Goose do.
- **Not available:** claude.ai connectors, which are signed in through your Anthropic account,
  and Claude Code's hooks.

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

`CLAUDE.md` lists the remaining checks CI runs (docs, cargo-deny, cargo-shear, typos, taplo,
zizmor, actionlint) and the lint policy.
