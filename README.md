# Swarmed

Parallel Claude Code sessions, each in its own git worktree with a fully isolated dev stack.
A personal tool with a Rust core; the desktop app (Tauri) comes later.

Status: early. Only the workspace and its checks exist so far.

## Layout

- `crates/core` — `swarmed-core`, the headless core. No UI dependency.
- `crates/cli` — the `swarmed` command, a thin front end over the core.

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

`CLAUDE.md` lists the remaining checks CI runs (docs, cargo-deny, cargo-shear, typos, taplo,
zizmor, actionlint) and the lint policy.
