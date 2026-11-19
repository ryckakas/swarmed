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
```

CI (`.github/workflows/ci.yml`) runs exactly these. Match them locally before pushing.

## Architecture

```
crates/
├── core/   swarmed-core: projects, tasks, worktrees, hooks, sessions, store. No UI dependency.
└── cli/    the `swarmed` binary, a thin front end over the core.
```

- **Everything lands in `core` first** and is driven from the CLI before any UI touches it. The
  core exposes typed operations and knows nothing about its front ends.
- **Nothing is written into a user's repository.** Config and state live in the data directory
  (`Paths`); worktrees live under `~/swarmed/<project>/<task>`.
- **Agents sit behind `AgentBackend`** (`agent/`). `ClaudeBackend` drives the installed `claude`
  CLI over its line-delimited JSON protocol, which has no specification: `agent/claude.rs` is the
  only file that knows its shapes, and it must tolerate unknown messages. `FakeBackend` is the
  scripted agent the tests use; `SWARMED_FAKE_AGENT=1` selects it in the CLI.
- **Running sessions live in the core** (`sessions.rs`). Front ends call `session_start`,
  `session_send` and the like by session id and read every session's events from one stream,
  `Core::subscribe`. Events are stored before they are published.
- **Swarmed never handles Claude.ai credentials.** It only shapes the child's environment (`Auth`).
- **Session events are stored as JSON** (`AgentEvent`), so its variants and fields are only added,
  never renamed.
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

## Conventions

- Rust edition 2024, current stable toolchain (`rust-version` in `Cargo.toml`).
- Dependencies are declared once in `[workspace.dependencies]` and inherited by the crates.
- Every action in the workflows is pinned to a commit, with its version in a comment.
- Commit and PR titles use conventional prefixes (`feat:`, `fix:`, `chore:`).
- No AI attribution anywhere: no `Co-Authored-By`, no "Generated with" lines, no session links.
- PR descriptions: a bold one-sentence summary, a `Risk:` line, then `## Problem`, `## Change`
  and a `## Verification` table.
