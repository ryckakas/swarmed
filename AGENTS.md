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
  (`Paths`); worktrees live under `~/swarmed/<project>/<task>`.
- **Agents sit behind `AgentBackend`** (`agent/`), and a session records which agent runs it.
  `agents.rs` holds the registry: Claude Code, plus the ACP agents listed in the user's
  `agents.toml`. `AcpBackend` (`agent/acp.rs`) speaks the Agent Client Protocol to any of them;
  its tests run against a scripted agent in `tests/fixtures/fake_acp_agent.py`. Anything that
  only Claude Code has (allow rules, appended instructions, its config) must not be assumed for
  other agents. `ClaudeBackend` drives the installed `claude`
  CLI over its line-delimited JSON protocol, which has no specification: `agent/claude.rs` is the
  only file that knows its shapes, and it must tolerate unknown messages. `FakeBackend` is the
  scripted agent the tests use; `SWARMED_FAKE_AGENT=1` selects it in the CLI.
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
- **Check UI changes in a browser** with `pnpm dev`. The mock agent answers plain text, asks
  permission for `run: <command>`, and runs until stopped for `slow`; one sample session has
  2,000 messages for checking scroll speed.
- **MCP servers are read from Claude Code's configuration** (`mcp.rs`) and handed to every agent
  that does not read it itself. The definitions hold secrets: they are passed on and never
  logged, and their `Debug` output leaves values out. Keep it that way.
- **Swarmed never handles Claude.ai credentials.** It only shapes the child's environment (`Auth`).
- **Session events are stored as JSON** (`AgentEvent`), so its variants and fields are only added,
  never renamed.
- **A task's changes are read from git on demand** (`changes.rs`): everything that differs from
  the merge base with the task's base ref, uncommitted and untracked files included. The view is
  read-only. The UI reads again when one of the task's agents finishes a tool call or a turn.
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
