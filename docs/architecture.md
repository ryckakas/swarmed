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
the window has, such as the count on its Dock icon and updating the app itself
(`crates/app/src/update.rs`).

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
- **What another Swarmed writes reaches the window** (`elsewhere.rs`): the command line, or a
  second app on the same data. The app looks every second. Another process's commit changes
  SQLite's `data_version`; the sessions' new events are then published in order as if the app
  had written them, the tree is read again when its rows changed or a stack started or
  stopped, and a configuration file changed on disk has the configuration read again. A model,
  effort or supervision another Swarmed stored for a session whose agent runs in the app, as
  `swarmed session set` does, is passed on to that agent as a change in the header would be.
- **A session's agent runs in one Swarmed at a time.** The one that starts it holds a lock on
  `run/sessions/<id>.lock` until the agent has gone; the system lets go of it when that
  process ends, however it ends. Another Swarmed refuses meanwhile to start, stop, close or
  delete the session or to remove its task, and a message queued for it there goes once that
  run is over.
- **The first app to arrive passes its place on.** With two apps open, the one left over takes
  `run/first.lock` once the first has quit: it then shows the notices of what the command line
  runs, stops every stack when it quits, and puts the sessions the first left busy back to
  idle.

## HTML views

An agent can answer with HTML. Swarmed shows it in a sandboxed frame (`allow-scripts` only) loaded
from `swarmed-view://`, a scheme the app serves with a policy of its own (`VIEW_POLICY` in
`views.rs`): no requests, no forms, nothing but inline styles, scripts, and images and fonts
from `data:`. The app's own policy allows frames from that scheme and nowhere else. A link
pressed in a view is shown with its address and opened only when the user says so.

## Math in replies

Formulas in agents' replies are drawn by KaTeX, whose script, stylesheet and fonts are bundled
into the app, so the window's policy is unchanged and nothing is fetched. KaTeX runs with
`trust` off: a formula can neither link nor load anything (`\href`, `\includegraphics`, `\html…`
are drawn as text), and its sizes are capped. What it draws is parsed in an inert template and
drawn by React, never set as HTML in the page, and a formula it cannot read is shown as written.
Which delimiters count is decided by one pure function, `marked` in `ui/src/math.ts`, which never
looks inside code.

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

CodeQL also scans `main` in each language it reads, and a pull request in the languages whose
files it changes (`codeql.yml`).

The lint policy, which forbids `unsafe` and holds functions to a cognitive complexity of 15 in
Rust and TypeScript alike, is in [AGENTS.md](../AGENTS.md). [RELEASING.md](../RELEASING.md) says
how a release is made.
