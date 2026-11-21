# Architecture and development

[← Back to README](../README.md)

```text
crates/
├── core/   swarmed-core: projects, tasks, worktrees, agents, sessions, the store; no UI
├── cli/    swarmed: the command, a thin front end over the core
└── app/    swarmed-app: the desktop app, a Tauri shell over the core
ui/         the app's front end: React and TypeScript, built by Vite
```

## One core, two front ends

Everything Swarmed does is in `swarmed-core`. The command and the app only drive it: the app's
commands forward to the core and add nothing (`crates/app/src/commands.rs`), apart from what only
the window has, such as the count on its Dock icon.

The UI reaches the app through one interface, `Api` (`ui/src/api/client.ts`). `tauri.ts` calls
the app; `mock.ts` serves sample data, so `pnpm dev` runs the whole UI in a browser with a
scripted agent.

## Sessions

A session is one agent at work in a task's worktree. Agents sit behind one trait,
`AgentBackend` (`crates/core/src/agent/`):

| Backend | Speaks to |
| --- | --- |
| `ClaudeBackend` | the installed `claude`, over its line-delimited JSON (`stream-json`) |
| `AcpBackend` | any agent that speaks the [Agent Client Protocol](https://agentclientprotocol.com) |
| `FakeBackend` | nobody: a scripted agent for tests, and for `SWARMED_FAKE_AGENT=1` |

What an agent says becomes `AgentEvent`s. The core stores each one before publishing it on one
stream (`Core::subscribe`) for every session, which the front end reads. The pieces of a reply
still being written are only published, never stored: the whole reply follows them.

Events are kept as JSON in a SQLite database, `swarmed.db` in the data directory. Their
variants and fields are only ever added, never renamed, and schema changes are appended to
`MIGRATIONS` in `store.rs`, never edited.

## What runs where

- **Git** is the `git` program, never a library (`git.rs`). **GitHub** is the user's own `gh`.
  The core links no HTTP client.
- **Whatever Swarmed starts** (hooks, stacks, agents, the status line script) gets the `PATH` of
  the user's shell (`shellenv.rs`). An app opened from the Dock would otherwise have only the
  system's and miss Homebrew, Nix and version managers.
- **Swarmed's own tools for agents** are an MCP server named `swarmed`, served by the same
  program started with a hidden `tool-server` argument (`toolserver.rs`). The agent starts it;
  it reaches the running Swarmed over a socket in the system's temporary folder.
- **Two Swarmeds can share a data directory**, such as the installed app beside a development
  build (`presence.rs`). The first to arrive holds a lock on `run/first.lock`, and only it
  settles the sessions an earlier run left busy, and stops on quitting the stacks it did not
  start.

## HTML views

An agent can answer with HTML. Swarmed shows it in a sandboxed frame (`allow-scripts` only) loaded
from `swarmed-view://`, a scheme the app serves with a policy of its own (`VIEW_POLICY` in
`views.rs`): no requests, no forms, nothing but inline styles, scripts, and images and fonts
from `data:`. The app's own policy allows frames from that scheme and nowhere else. A link
pressed in a view is shown with its address and opened only when the user says so.

## Building from source

Needs Rust (the version in `rust-version` in `Cargo.toml`) and Node with Corepack.

```sh
corepack enable && pnpm install
pnpm app                        # the app, in development
pnpm dev                        # the UI alone in a browser, on sample data
pnpm app:build                  # Swarmed.app, in target/release/bundle/macos
cargo run -p swarmed -- --help  # the command
```

The app crate embeds `ui/dist` when it compiles, so run `pnpm build` once before `cargo clippy`
or `cargo test` on the whole workspace.

## Checks

CI runs these, and a pull request passes them locally first:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features
cargo deny check && cargo shear && typos && taplo fmt --check
zizmor .github && actionlint && shellcheck install.sh scripts/*.sh
gitleaks git --redact
pnpm lint && pnpm typecheck && pnpm complexity && pnpm test
```

CodeQL also scans every pull request and `main`, in each language it reads (`codeql.yml`).

The lint policy, which forbids `unsafe` and holds functions to a cognitive complexity of 15 in
Rust and TypeScript alike, is in [AGENTS.md](../AGENTS.md). [RELEASING.md](../RELEASING.md) says
how a release is made.
