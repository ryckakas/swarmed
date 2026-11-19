# Swarmed

Parallel Claude Code sessions, each in its own git worktree with a fully isolated dev stack.
A personal tool with a Rust core; the desktop app (Tauri) comes later.

Status: early. The core and a CLI exist; see `.plans/` locally for the plan.

## Layout

- `crates/core` — `swarmed-core`, the headless core. No UI dependency.
- `crates/cli` — the `swarmed` command, a thin front end over the core.

## Try it

```sh
cargo run -p swarmed-cli -- project add /path/to/repo
$EDITOR "$(cargo run -q -p swarmed-cli -- project config <project>)"
cargo run -p swarmed-cli -- task new <project> <task>
cargo run -p swarmed-cli -- task list <project>
cargo run -p swarmed-cli -- task rm <project> <task>
```

Worktrees are created under `~/swarmed/<project>/<task>`. Config and state live in
`~/Library/Application Support/swarmed/`; nothing is written into your repositories.
`SWARMED_DATA_DIR` and `SWARMED_WORKTREES_DIR` override both locations.

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

`CLAUDE.md` lists the remaining checks CI runs (docs, cargo-deny, cargo-shear, typos, taplo,
zizmor, actionlint) and the lint policy.
