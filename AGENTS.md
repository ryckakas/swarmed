# AGENTS.md

Guidance for coding agents working in this repository.

## What this is

Swarmed runs several Claude Code sessions in parallel, each in its own git worktree with a fully
isolated dev stack. It is a personal macOS tool with a Rust core. The plan, with its decision log
and build steps, is in `.plans/rust-agent-orchestrator/plan.md` (local, not committed).

## Commands

```bash
cargo fmt --all -- --check                          # formatting (rustfmt.toml: 100 cols)
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features
cargo deny check                                    # advisories, licenses, duplicates: deny.toml
cargo shear                                         # dependencies nothing uses
typos                                               # spelling: _typos.toml
taplo fmt --check                                   # TOML formatting: .taplo.toml
zizmor .github                                      # workflow security
actionlint                                          # workflows, shellcheck on run: blocks too

pnpm install                                        # once; pnpm comes from Corepack
pnpm lint                                           # Biome: format and lint (biome.json)
pnpm typecheck                                      # tsc, strict
pnpm complexity                                     # bonsai-lint ui/src, threshold 15
pnpm test                                           # Vitest
pnpm build                                          # type check, then the UI into ui/dist
pnpm dev                                            # the UI alone in a browser, on sample data
pnpm app                                            # the desktop app (tauri dev)
```

The Rust app crate embeds `ui/dist` at compile time, so `cargo clippy` and `cargo test` on the
whole workspace need that directory: run `pnpm build` once, or `mkdir -p ui/dist && touch
ui/dist/index.html`.

CI (`.github/workflows/ci.yml`) runs exactly these. Match them locally before pushing.

Where CI runs is the repository variable `CI_RUNNER`. Unset, the jobs run on GitHub's runners.
Set to a self-hosted runner's label (`gh variable set CI_RUNNER --body swarmed-mac`), they run on
that machine with the tools installed there, and the Linux test run is left out.

## Architecture

```
crates/
├── core/   swarmed-core: projects, tasks, worktrees, hooks, sessions, store. No UI dependency.
├── cli/    the `swarmed` binary, a thin front end over the core.
└── app/    swarmed-app: the Tauri shell. Commands forward to the core; events are passed on.
ui/         the front end: React and TypeScript, built by Vite from the repository root.
```

- **Everything lands in `core` first** and is driven from the CLI before any UI touches it. The
  core exposes typed operations and knows nothing about its front ends.
- **Nothing is written into a user's repository.** Config and state live in the data directory
  (`Paths`); worktrees live under `~/swarmed/<project>/<task>`. The one exception: a repository
  with no commits is given an empty first commit when its first task is created, because a
  branch needs a commit to start from. Its files and index are not touched.
- **Agents sit behind `AgentBackend`** (`agent/`), and a session records which agent runs it.
  `agents.rs` holds the registry: Claude Code, plus the ACP agents listed in the user's
  `agents.toml`. `AcpBackend` (`agent/acp.rs`) speaks the Agent Client Protocol to any of them;
  its tests run against a scripted agent in `tests/fixtures/fake_acp_agent.py`. Anything that
  only Claude Code has (allow rules, appended instructions, its config) must not be assumed for
  other agents. `ClaudeBackend` drives the installed `claude`
  CLI over its line-delimited JSON protocol, which has no specification: `agent/claude.rs` is the
  only file that knows its shapes, and it must tolerate unknown messages. `FakeBackend` is the
  scripted agent the tests use; `SWARMED_FAKE_AGENT=1` selects it in the CLI.
- **Known agents can be set up from the settings** (`agent_setup.rs`): found on the machine,
  installed with Homebrew by a command the user is shown first, and written into `agents.toml`
  with their whole path and the models to offer. The registry is behind a lock and read again
  after every change, so nothing needs a restart. An agent is only offered for a session while
  its program is found on the machine: configured but not installed, it is in the settings and
  not in the agent menu. The list of OpenRouter's models is fetched
  with `curl` from its public address, with no key and nothing of the user's in the request.
- **What Swarmed takes up** (`usage.rs`) is measured on request: the database and notes by
  their files, each worktree with `du`, and memory and processor time with `ps`, summed over
  Swarmed's own process and everything started from it. An agent's share is the tree under the
  process its backend reported when it started.
- **Running sessions live in the core** (`sessions.rs`). Front ends call `session_start`,
  `session_send` and the like by session id and read every session's events from one stream,
  `Core::subscribe`. Events are stored before they are published, except the pieces of a reply
  still being written (`TextDelta`), which are only published: the whole reply follows.
- **The app crate holds no logic.** A command in `crates/app/src/commands.rs` calls one core
  function. If the UI needs something new, it is added to the core and tested there first.
- **`crates/app/src/tests.rs` calls every command the way the UI does,** by name with JSON
  arguments, on Tauri's mock runtime. Nobody can look inside the native window from CI, so a
  new or changed command gets a call there, with the argument names `ui/src/api/tauri.ts` sends.
- **The UI talks to the app through `Api`** (`ui/src/api/client.ts`). `tauri.ts` is the real one;
  `mock.ts` serves sample data when the UI runs in a plain browser. `types.ts` mirrors what the
  core serialises by hand, with the core's snake_case field names: change both together.
- **UI logic lives in pure functions** with tests: `tree.ts` for the sidebar, `transcript.ts`
  for turning a session's events into what the transcript shows. Components and the store stay
  thin.
- **Keyboard shortcuts are one table** (`ui/src/hotkeys.ts`), matched in `useHotkeys.ts` and
  listed in Settings from the same table. A new action gets its shortcut there, not a key
  handler of its own. Combinations macOS or the window already use are not taken.
- **Check UI changes in a browser** with `pnpm dev`. The mock agent answers plain text, asks
  permission for `run: <command>`, and runs until stopped for `slow`; one sample session has
  2,000 messages for checking scroll speed.
- **MCP servers are read from Claude Code's configuration** (`mcp.rs`) and handed to every agent
  that does not read it itself. The definitions hold secrets: they are passed on and never
  logged, and their `Debug` output leaves values out. Keep it that way.
- **A session's title** starts as the first line of its first message. A `Namer` (`ClaudeNamer`:
  one call to Claude Code's smallest model, with none of the user's settings, tools or MCP
  servers) then suggests a better one, unless `auto_title` is off or the user renamed the session
  first. Every change of title is an event, `Renamed`, so all front ends follow.
- **A new task is named before it exists** (`naming.rs`): the same `Namer` is asked, with a
  short timeout, and its answer is both the first session's title and, as lower-case words
  joined by dashes, the task's name. With no answer the front end names the task after the
  prompt. A task is never renamed later: its folder and branch are where the agent works.
- **A session is unread** (`Session::unread`) from when its agent ends a turn, or stops with an
  error, until a front end calls `session_mark_read` or the user writes to it. It is a flag
  beside the status, not a status: the UI shows it for idle sessions only, since busy, waiting
  and failed already say more. The UI marks the session on screen as read when its answer
  arrives, if the window has focus, and otherwise when the window gets it.
- **MCP servers can be set up in Swarmed** (`mcp_own.rs`), in `mcp.toml` in the data
  directory, and are given to every agent it runs, Claude Code included, beside Claude Code's
  own. A server's secrets are named in the file and kept in the keychain under
  `mcp:<server>:<name>`; a front end can set one and never read it. Claude Code's servers are
  shown from its configuration and from what it last reported, and never written.
- **Each session can have a terminal** (`terminal.rs`): the user's shell on a pseudo-terminal in
  the task's worktree, shown in place of the message box. Its output is published on its own
  channel (`Core::subscribe_terminals`) and not stored. What it printed since the agent's last
  turn is appended, as plain text, to the user's next message and recorded as `TerminalShared`.
  The agent cannot type into the terminal.
- **Agents may answer with HTML views**: a fenced block with the language `html-view`, which
  they are told about through `views::INSTRUCTIONS`. The UI wraps the fragment in a document
  (`ui/src/htmlview.ts`), hands it to the core (`views.rs`), and shows it in a sandboxed frame
  loaded from `swarmed-view://`, a scheme the app serves with a policy of its own
  (`view_response` in the app crate). That origin has no capability, so it cannot call a
  command. Never show agent HTML in the app's own origin, and never loosen the app's policy
  for it.
  The instructions are deliberately firm that this is the way to show something and needs no
  tool: without that, agents build a file and drive a browser instead. An ordinary `html` block
  stays code, with a button to show it as a view.
- **Swarmed serves one tool of its own to every agent** (`toolserver.rs`): `show_html`, on an
  MCP server named `swarmed`. A front end's executable is that server when started with
  `tool-server`, and tells the core so with `set_tool_server`. The tool does nothing: its call
  is in the conversation, where the UI shows the HTML like an `html-view` block. It is there
  because agents asked for something visual look for a tool.
- **A view reaches the conversation from the tool, not from the call.** The tool server
  leaves the HTML in the session's own folder (`Paths::view_spool`, named to it by an
  environment variable), and the core records it as a `View` event when the next tool result or
  turn end arrives. Agents wrap their calls differently: directly, from a script, from a
  subagent. None of that matters this way. Reading the HTML out of the call's own shape is only
  a fallback for conversations recorded before.
- **Sessions can see, read, message and start each other** (`peers.rs`), through four tools
  of Swarmed's own server. The server is a process the agent starts, so it asks the core over
  a socket in the system's temporary folder (`Paths::tools_socket`; short, since a socket's
  path is limited), which a front end serves with `serve_session_tools`. A message arrives as
  a message in the other conversation, queued if that session is busy. A spawned session runs
  in its parent's task with its settings, cannot spawn itself, and a session can have
  `MAX_SPAWNED` open. The tools are not on the allow list: in ask mode the user is asked.
- **Agents other than Claude Code get Swarmed's guidance with their first message**, as a
  second block after the user's words: the protocol has no place for a client's instructions.
- A view still being written is held back and stood in for (`splitPendingView`), so it appears
  whole instead of as streaming source.
- **The message box handles a few keys itself** (`ui/src/editing.ts`): Home and End move within
  the line, and ⌘⌫ at the very start empties the box.
- **The status line** under the message box is drawn by the UI from built-in pieces
  (`ui/src/statusline.ts`) chosen in the settings. A `status_command` replaces them: the core
  runs it (`statusline.rs`) with the session as JSON on standard input and shows its first line.
- **API keys live in the system's keychain** (`secrets.rs`), under names from a fixed list of
  providers. They are written by a front end and afterwards only read to fill the environment
  of an agent other than Claude Code. They are never returned, logged or written to Swarmed's
  files, and `SecretEnv` prints names only. Keep it that way.
- **Notes** (`notes.rs`) are the user's own: one text file per project and per task, under
  `notes/` in the data directory, readable by the user alone. They may hold credentials, so
  they are never logged and never handed to an agent, and a removed task takes its note along.
- **Plan mode** is a value of `Supervision`. Claude Code is started in, or switched to, its own
  plan mode, and presents its plan by asking to use `PLAN_TOOL`. Allowing that request approves
  the plan: the core then puts the session back to how new sessions are supervised and tells the
  agent, which by itself would fall back to asking about everything. Every change of
  supervision is an event, `SupervisionChanged`. Other agents have no plan mode and ask instead.
- **The model and effort actually in use** are asked of Claude Code (`get_settings`) when it
  starts and after each turn, and recorded as `Applied`. Only those two values are read: the
  rest of that answer is the user's whole configuration and is never kept or logged.
- **Messages can wait in a queue** (`queue.rs`) while a session's agent is busy. The core sends
  the first one when a turn ends well, as if it had just been typed; a turn that was stopped
  or failed leaves the queue standing. The queue lives in memory, and its changes are published
  as `Queue` events that are never stored, like the pieces of a reply.
- **Swarmed never handles Claude.ai credentials.** It only shapes the child's environment (`Auth`).
- **Session events are stored as JSON** (`AgentEvent`), so its variants and fields are only added,
  never renamed.
- **A task's changes are read from git on demand** (`changes.rs`): everything that differs from
  the merge base with the task's base ref, uncommitted and untracked files included. The view is
  read-only. The UI reads again when one of the task's agents finishes a tool call or a turn.
- **Removing a project** (`project_remove`) removes its tasks as `task_remove` does, keeping
  their branches, then deletes what Swarmed holds about it: sessions, their history and the
  notes. The repository is not touched. Removing a task stops its sessions first. In the UI
  both ask first, and offer to go ahead regardless only after a refusal over unsaved work.
- **Opening things elsewhere** (`open.rs`) starts the user's editor (`settings.toml`, VS Code by
  default) or the browser and never waits for them.
- **Git is used by shelling out** to the `git` binary (`git.rs`), not through a library.
- **The store is SQLite** (`store.rs`). Schema changes are appended to `MIGRATIONS` and never
  edited afterwards.

## Lint policy

`unsafe_code` is forbidden. Clippy `pedantic` and `cargo` warn, and CI turns warnings into errors.
`excessive_nesting` (threshold 3), `too_many_lines` and `cognitive_complexity` (threshold 15) are
**deny**: restructure the code instead of loosening them. An `#[allow]` needs a `reason = "…"`.

- Public items are documented; the first paragraph is one short sentence.
- Private items get a comment only for the "why".
- No `unwrap()`; tests use `expect("…")`.
- No `println!`: it panics on a closed pipe. The CLI writes through its `say` helper.
- Tauri's dependency tree is exempt from the duplicate-version rules, in `deny.toml` and with one
  `#![allow]` in the app crate. Nothing else is.

The UI is held to the same bar: Biome's recommended rules plus cognitive complexity at 15, no
`any`, no non-null assertions, no default exports and no `console`; TypeScript in strict mode with
`noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`; and bonsai-lint over `ui/src`.

## Conventions

- Rust edition 2024, current stable toolchain (`rust-version` in `Cargo.toml`).
- Dependencies are declared once in `[workspace.dependencies]` and inherited by the crates.
- Every action in the workflows is pinned to a commit, with its version in a comment.
- Commit and PR titles use conventional prefixes (`feat:`, `fix:`, `chore:`).
- No AI attribution anywhere: no `Co-Authored-By`, no "Generated with" lines, no session links.
- PR descriptions: a bold one-sentence summary, a `Risk:` line, then `## Problem`, `## Change`
  and a `## Verification` table.
