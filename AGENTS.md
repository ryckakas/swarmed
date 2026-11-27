# AGENTS.md

Guidance for coding agents working in this repository.

## What this is

Swarmed runs coding agents side by side (Claude Code, and any agent that speaks the Agent Client
Protocol), each in its own git worktree with an isolated dev stack: a desktop app, built for macOS
so far, and a command, over one Rust core. The plan and its decision log are in
`.plans/rust-agent-orchestrator/plan.md` (local, not committed). What users read is in
`README.md` and `docs/`: a change that makes a claim there untrue updates it.

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
shellcheck install.sh scripts/*.sh                  # shell scripts
gitleaks git --redact                               # secrets, in the whole history: .gitleaksignore

pnpm install                                        # once; pnpm comes from Corepack
pnpm lint                                           # Biome: format and lint (biome.json)
pnpm typecheck                                      # tsc, strict
pnpm complexity                                     # bonsai-lint ui/src, threshold 15
pnpm test                                           # Vitest
pnpm build                                          # type check, then the UI into ui/dist
pnpm dev                                            # the UI alone in a browser, on sample data
pnpm app                                            # the desktop app (tauri dev)
pnpm app:build                                      # Swarmed.app, in target/release/bundle/macos
pnpm app:install                                    # the same, copied into /Applications
pnpm app:dmg                                        # a disk image of it, for handing on
scripts/build-release.sh                            # what a release ships: RELEASING.md
```

- **`ui/dist` first.** The app crate embeds it at compile time: before `cargo clippy` or
  `cargo test` on the workspace, run `pnpm build` once, or `mkdir -p ui/dist && touch
  ui/dist/index.html`.
- **CI runs exactly these** (`.github/workflows/ci.yml`): match them locally before pushing. The
  lint runs on Linux and the tests on macOS and Linux, so code must compile on both. Only GitHub's
  runners: the repository is public, and a self-hosted one would run a stranger's pull request.
  `cargo deny`'s advisories fail no run (one is published against crates already locked).
- **Secrets in tests:** a made-up one is marked `gitleaks:allow` on its line; one already in the
  history is listed by its fingerprint in `.gitleaksignore`.
- **CodeQL** (`codeql.yml`) scans `main` in every language the repository has, and a pull request
  in the languages it changes. A file in a language new to the repository goes into its list
  there, with the patterns that name its files; a language listed keeps at least one file.

## Architecture

```
crates/
├── core/   swarmed-core: projects, tasks, worktrees, hooks, sessions, store. No UI dependency.
├── cli/    the `swarmed` binary, a thin front end over the core.
└── app/    swarmed-app: the Tauri shell. Commands forward to the core; events are passed on.
ui/         the front end: React and TypeScript, built by Vite from the repository root.
```

Each feature's mechanics, and the reasons behind them, are in its own module's doc comment (the
`//!` at the top of a Rust file, the comment above a UI function or component): read it before
changing the feature, and keep it true. What follows is what holds across features.

### Where things go

- **Core first.** Everything lands in `swarmed-core`, as typed operations that know nothing of
  their front ends, and is driven from the CLI before any UI touches it.
- **The app crate holds no logic.** A command in `crates/app/src/commands.rs` calls one core
  function; anything new is added to the core and tested there first. A command that reads much
  or waits runs off the main thread (`#[tauri::command(async)]`); one whose order matters stays
  on it. A command that is not `async` runs on a thread with no Tokio runtime: nothing it reaches
  may spawn with Tokio, which aborts the app; use `std::process` (`stacks.rs`, `mcp.rs`).
- **`crates/app/src/tests.rs` calls every command** as the UI does: a new or changed one gets a
  call there, with the argument names `ui/src/api/tauri.ts` sends.
- **Updating is the one thing the app crate decides** (`crates/app/src/update.rs`): the core
  links no HTTP client, and the updater plugin's own commands are in no capability.
- **Nothing is written into a user's repository.** Config and state live in the data directory
  (`Paths`), worktrees under `~/swarmed/<project>/<task>`. The one exception: an empty first
  commit for a repository with none (`git::ensure_first_commit`). Checkpoints write nothing there
  either (`checkpoints/trees.rs`; a test holds `.git` to the byte).
- **Chats have no repository** (`chats.rs`). Make any git call on a project's path through
  `Project::repo()`, never `repo_path`; what only a repository has is refused with
  `Error::NoRepository`.
- **What a task keeps on disk** goes with its removal, counts in `usage.rs`, and is listed by
  `cleanup.rs` once its task is gone, as browser profiles and checkpoints are.

### Data and compatibility

- **Session events are stored as JSON** (`AgentEvent`): variants and fields are only added, never
  renamed. Front ends act on a session by its id and hear every session's events on one stream,
  `Core::subscribe`. Events are stored before they are published, except those only shown
  (`TextDelta` and the like); one published on session id 0 is never stored. So a front end's
  list is longer than the core's: point into a conversation by message number (`session_fork`),
  never by event index.
- **The store is SQLite** (`store.rs`): schema changes are appended to `MIGRATIONS`, never
  edited. Every table's integer id is `INTEGER PRIMARY KEY AUTOINCREMENT`, so no id is given
  twice; a test holds every table to it.
- **What a version cannot read costs only itself** (`lenient.rs`). Swarmeds of different versions
  share one data directory: configuration is read with `serde_ignored`, never
  `deny_unknown_fields`, so an unknown key or a bad value costs only its key or entry and is named
  in `Core::config_problems`. Saving the settings writes unknown keys back. A new configuration
  file is read the same way. A value an older Swarmed must not trip on goes in a database column,
  not a config file (`environments.rs`: the default environment).

### Several Swarmeds on one data directory

- **The house** (`presence.rs`, `run/first.lock`): only the first Swarmed to arrive settles busy
  sessions as it starts, stops every stack and browser as it quits, shows notices of what others
  run and runs the house's followers. Any other, and the command line always, stops only what it
  started. A second app takes the house once the first quits.
- **Hearing others** (`elsewhere.rs`): each process publishes only on its own stream. A window
  learns of another's work only from what that one stored (events, the tree's rows, run records,
  configuration files), through `Core::notice_others`.
- **One process per session's agent** (`run/sessions/<id>.lock`, `claim_run`): starting,
  stopping, closing, deleting or answering a session, and removing its task or repository even by
  force, is refused with `Error::RunsElsewhere` while another process runs it
  (`none_run_elsewhere`, `or_elsewhere`).
- **Lock order:** the store is never held while the lock on the running sessions is taken
  (`Core::settings_told`); the lock on `Queues::kept` is taken before the store's, never after.

### Agents

- **Claude Code is one agent of several.** Agents sit behind `AgentBackend` (`agent/`). Never
  assume for ACP agents what only Claude Code has (allow rules, appended instructions, its
  config). `agent/claude.rs` alone knows Claude Code's JSON shapes, and tolerates unknown
  messages. `SWARMED_FAKE_AGENT=1` runs the scripted `FakeBackend`.
- **Guidance has one way in** (`Core::guidance`, `SessionSpec::append_system_prompt`): Claude
  Code takes it as its system prompt, an ACP agent with its first message after each start.
- **A `UserMessage` is what the user typed** (`send_text` in `sessions.rs`). What Swarmed adds
  for the agent (an earlier conversation after a fork or rewind, terminal output, a skill's
  text, a change of role or parent) goes to the agent only.
- **Whatever Swarmed starts** (hooks, stacks, the status command, agents) gets the `PATH` of the
  user's shell (`shellenv.rs`).
- **Few tools, general ones** (`toolserver.rs`): every tool's description is sent to every agent
  in every session. Widen a tool there is before adding one. There is no browser tool of
  Swarmed's: agents drive a task's Chrome with their own Playwright or Chrome DevTools MCP,
  pointed at it (`browser/agents.rs`); `show_browser` only opens the pane.
- **Configuration through `configure` alone, in three kinds** (`configure.rs`): done at once,
  proposed to the user, not reachable. A new setting goes into one on purpose; when unsure,
  propose. Out of reach: what widens what an agent may do or spend, what acts by itself (sending
  messages, stopping agents: `continue_after_limit`, the put-away rules), and what holds secrets.
  An environment's variable values are never told to an agent, only their names.
  `ENVIRONMENT_GUIDE` stays true as hooks and their variables change.
- **Only `show_html` is on Claude Code's allow list** (`spec_for` in `sessions.rs`). Session
  tools, `start_task` and `show_browser` are asked about; a session with a role uses its session
  tools unasked, but for a start in plan mode (`roles::uses_unasked`).
- **Who may reach, start and close whom is checked in one place:** `Reach` and `in_reach` in
  `peers.rs` (what `@` lists is what `read_session` can read; from a chat every open session, and
  a repository's sessions may write to a chat but never list or read one), `may_start` and
  `room_for_another` (`Settings::max_workers`), `peer_close`, and `parents.rs`. A new way to start
  or reach a session goes through them. A message never starts a closed one.
- **Roles are told and documented alike** (`roles.rs`): `docs/coding-agents.md` holds the same
  roles and table, and a test keeps them together: change both. Only the user sets a role.
- **Agents keep to the task's branch.** Every agent is told it already works in an isolated
  worktree, to make no other branch, worktree or clone, and to start a task for work that needs
  one (`publish::worktree_instructions`). `docs/coding-agents.md` has to agree.
- **Sub-agents are only events** (`AgentEvent::Sub`, read in `subagents.ts`): no table or row
  holds one. ACP agents' are shown the same way (`agent/acp/children.rs`).
- **A rewind deletes nothing** (`rewind.rs`): whatever hands a conversation on (forks, carried
  text, the cloud, `said_in`) reads `rewind::counted`; costs, search and the event list read
  everything.

### Secrets and what is the user's alone

- **API keys stay in the keychain** (`secrets.rs`): never returned, logged or written to
  Swarmed's files, and `Debug` prints names only. Keep it that way.
- **MCP server definitions hold secrets** (`mcp.rs`, `mcp_pool.rs`, `mcp_own.rs`): passed on,
  never logged, and `McpServer`'s `Debug` leaves values out. Secrets of Swarmed's own servers are
  in the keychain: a front end sets one and never reads it. Claude Code gets its servers in a file
  only the user can read (`run/agents/<session>/`), never on its command line, where `ps` shows
  them. Nothing is written into an agent's own files.
- **A repository's `.mcp.json` is the branch author's, not the user's**: other agents get its
  servers only once the user approved them in Claude Code (`claude_code_servers_to_share`).
- **Notes may hold credentials** (`notes.rs`): never logged, never handed to an agent.
- **Swarmed never handles Claude.ai credentials.** It only shapes the child's environment
  (`Auth`).
- **Of Claude Code's `get_settings` and `get_usage` answers only the model, the effort and the
  usage figures are read** (`agent/claude.rs`): the rest is never kept, passed on or logged.
- **Nothing asks for usage on its own**, on a timer or when a figure is looked at: only the user
  does (their choice, so that looking costs nothing; keep it so). `docs/coding-agents.md` says
  how fresh the figures are because of it (`account_usage.rs`).
- **Only the user's eyes:** Claude Code's screens (`AgentEvent::Screen`), the Claude Code opened
  in a session's terminal, what a background command printed (`background.rs`), an image a tool
  returned (`returned.rs`) and a snapshot (`snapshots.rs`, which may hold `.env`) are never
  logged, or handed to an agent, another session, a fork, the cloud or search.
- **The user's terminal is the user's** (`terminal.rs`): its output is never stored, an agent
  cannot type into it, and an action's command is typed there, where the user sees it. No
  terminal gives a program the clipboard's contents: OSC 52 only writes (`TerminalBox.tsx`).
- **A path from a front end** is refused unless it stays inside the worktree, by its wording
  (`stays_inside`) and once links are followed (`files::inside`).
- **Attaching a file is naming its path** (`attach.rs`): nothing is uploaded or copied. A pasted
  image is the one part kept, as a file in the session's `attachments/` (`pasted.rs`), then
  attached by path: no agent is sent image content, and `UserMessage` stays text.

### Untrusted content

- **Agent HTML never runs as the app.** A view (an `html-view` block or `show_html`) is shown
  only in a sandboxed frame from `swarmed-view://`, under its own policy (`views.rs`,
  `view_response` in the app crate). Tauri checks no capability for that scheme: never show agent
  HTML in the app's own origin, never loosen the app's policy for it, never hand a frame the
  invoke key.
- **An MCP app is a third party's HTML** (`apps.rs`, `mcpApps.ts`): every rule of a view and
  more. What it may do is decided in `mcpApps.ts`; anything else is refused.
- **What others wrote on GitHub is anyone's.** An issue is drawn only with `UntrustedMarkdown`,
  in the window, never in a view or a frame. Nothing may make the window fetch an address: it
  loads images only from `data:` addresses (`session_image`, `Markdown.tsx`, `issueHtml.ts`). A
  review or comment is told by who, never what.
- **A custom layout or `terminal.json` holds values, never structure** (`layouts.rs`,
  `termconfig.rs`): agents save a layout at once only because it can hide or move nothing.

### Git, GitHub and publishing

- **Git by shelling out** (`git.rs`), never a library. A command on a repository's worktree
  records (`worktree add`, `remove`, `prune`, `list`, `branch -D`, `switch`) goes through
  `on_worktrees`. A list of unknown length is read NUL-separated (`-z`).
- **A hook's command takes each value as one word** (`Vars::render_for_shell`): a value a shell
  would read goes in as its `SWARMED_*` variable, never as text.
- **The user's own `gh`, no token.** Pull requests, checks, merges and issues are asked of the
  user's `gh`; Swarmed holds no GitHub token. No `gh`, not on GitHub or signed out means none,
  said why, never an error (`pulls.rs`, `issues.rs`). Nothing asks `gh` on a timer but the
  house's follower (`following.rs`); every reading goes through `pull_take_in`, so a change is
  said once.
- **Work leaves the worktree only when asked** (`publish.rs`): commit, push and pull request are
  each a step the user takes, by their own git and `gh`, as them. Nothing is added to either.
- **Follow the branch the worktree is on** (`git::current_branch`): what is shown, pushed and
  opened is that branch's, never `task.branch` in its place, which never changes. A task's name
  is its folder and branch, never changed (`naming.rs`). A remote branch is never deleted or
  touched.

### The UI

- **The UI reaches the app only through `Api`** (`ui/src/api/client.ts`; `mock.ts` for
  `pnpm dev`). `types.ts` mirrors what the core serialises by hand, in its snake_case: change
  both together.
- **UI logic lives in pure, tested functions** (`tree.ts`, `transcript.ts`); components and the
  store stay thin.
- **One place each:** a shortcut in the `ui/src/hotkeys.ts` table, never a key handler of its
  own; every dialog is `Dialog` (`Dialog.tsx`, the one `<dialog>`, as `dialogs.test.ts` checks),
  never a frame or class of its own.
- **Size things with the tokens**, so every layout follows; a new fixed-size icon goes into the
  `zoom` lists of `ui/src/styles/layouts.css`.
- **Nothing shown moves when the pointer comes** (the user's rule). What shows only under the
  pointer or with the keyboard is laid over what is there, or takes its room from text that gives
  way, never from beside something that shows anyway. Hover changes colour and opacity; only what
  the user asks for (a fold, a menu) moves things. A new sidebar icon goes into `offered` or
  `shown` of `RowParts` (`Sidebar.tsx`).
- **Running work is never hidden, and is drawn moving**: still means nothing runs. A running
  stack or browser is marked in the sidebar (`TaskOverview`); work left in the background keeps
  a session idle but marked (`inBackground.ts`, `StatusMark.tsx`).
- **Unread and the background count are flags beside the status, never statuses**
  (`Session::unread`, `BackgroundCount`). The core decides them and says so on its events
  (`SessionEvent::unread`, `SessionEvent::background`); the UI follows. A mark is the agent's
  word and a highlight the user's (`marks.rs`, `highlights.rs`): colours are names each theme
  draws, never values; a mark never stands in for the status, and no agent sets a highlight.
- **Figures being refreshed are never dimmed** (the user's choice): a line beside them says so.
- **Unsent text and per-session UI state are in the store** (`ui/src/store.ts`): drafts, recall,
  scroll spots and the like, in memory only. Never in a component, which goes when another
  session opens, and never on disk. A new kind goes with its session in `forgotten` and `closed`.
- **Whatever sends into a session queues while its agent is busy** (`sendOrQueue` in
  `ui/src/actions.ts`): sent straight in, it would take away a request the agent waits on. What
  appears by itself takes the keyboard only while nothing has it (`focus.ts`).
- **An answer from the core can arrive after an event published later.** Store state that is
  both read and published takes an answer only if nothing was heard since it was asked
  (`queueAnswer`, `treeReads`); the tree is read only through `readTree`.
- **Core and sidebar count the same siblings** (`session_siblings` in `order.rs`,
  `nestedSessions` in `tree.ts`), or a drag lands short.
- **The centre never holds on to a session that is gone.** Every way the session on screen goes
  (closed, deleted, its task or repository removed, here, elsewhere or by an agent) passes
  through `leaving` in `store.ts` before anything of it is forgotten.
- **References under a message follow one pattern** (`mentions.ts`, `githubMentions.ts`,
  `attach.rs`): a menu opened by a sign, a token in the text, a fixed list under the words that
  front ends read back. A new kind reuses it. Sending asks the network nothing.
- **No built-in button for one person's workflow**: that is what an action is for
  (`actions.rs`).
- **Check UI changes in a browser** with `pnpm dev`: sample data and a scripted agent. Every
  trigger word and sample state is listed at the top of `ui/src/api/mock.ts`, with details in
  the `mock*.ts` beside it. The README's screenshot and GIF are of this sample data, never of
  anyone's real projects.

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

- Rust edition 2024, on the stable toolchain pinned exactly in `rust-toolchain.toml`, which every
  workflow installs from and Dependabot moves. `rust-version` in `Cargo.toml` is the oldest it
  builds with.
- Dependencies are declared once in `[workspace.dependencies]` and inherited by the crates.
- `swarmed-core` makes no promise about its API: it is on crates.io only for `swarmed`, which
  requires it at exactly the workspace's version (`=X.Y.Z`, moved by `release-prep.sh`). A version
  number follows what a release does to the `swarmed` command and the data directory alone.
- Everything the workflows run is pinned, so a run uses only what a reviewed change chose: an
  action to a commit with its version in a comment, a downloaded binary to its version and
  SHA-256, an `install-action` tool to its version, Node to `.node-version`, pnpm to its version
  and hash in `packageManager`. Cargo runs `--locked` and `pnpm install` `--frozen-lockfile`.
  Dependabot moves the actions, crates, npm packages and Rust; tools, Node and pnpm move by hand.
- Commit and PR titles use conventional prefixes (`feat:`, `fix:`, `chore:`).
- A PR with a change a user would notice adds its line under `## [Unreleased]` in
  `CHANGELOG.md`, under Added, Changed, Fixed or Removed, written for the user: the release
  workflow publishes that section as the release's notes (`RELEASING.md`).
- No AI attribution anywhere: no `Co-Authored-By`, no "Generated with" lines, no session links.
- PR descriptions: a bold one-sentence summary, a `Risk:` line, then `## Problem`, `## Change`
  and a `## Verification` table.
