# AGENTS.md

Guidance for coding agents working in this repository.

## What this is

Swarmed runs coding agents side by side (Claude Code, and any agent that speaks the Agent Client
Protocol), each in its own git worktree with an isolated dev stack. It is a desktop app, built for
macOS so far, and a command over one Rust core. The plan, with its decision log and build steps,
is in `.plans/rust-agent-orchestrator/plan.md` (local, not committed). What users read is in
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

The Rust app crate embeds `ui/dist` at compile time, so `cargo clippy` and `cargo test` on the
whole workspace need that directory: run `pnpm build` once, or `mkdir -p ui/dist && touch
ui/dist/index.html`.

CI (`.github/workflows/ci.yml`) runs exactly these. Match them locally before pushing.

CI runs on GitHub's runners: the lint on Linux, and the tests on both macOS and Linux, so code
that only one of them compiles is checked on each. A self-hosted runner would run a stranger's
pull request on that machine, now that the repository is public.

A made-up secret in a test is marked `gitleaks:allow` on its line; one already in the history is
listed by its fingerprint in `.gitleaksignore`.

CodeQL (`.github/workflows/codeql.yml`) scans `main` in every language it reads that the
repository has, and a pull request only in the languages whose files it changes; there is no
schedule. A file in a language new to the repository that CodeQL reads goes into its list there,
with the patterns that name its files; one the list names must keep at least one file.

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
- **An ACP agent may carry on after its turn is over**, when a timer of its own goes off or
  something it left running ends. `acp.rs` treats that as a turn of the agent's own: it ends
  when the agent has said something and reports its usage, or has been silent for `QUIET`,
  and a stop from the user ends it at once, since the agent has nothing to answer one with.
- **A stack is also remembered on disk** (`run/stacks/<task>.pid`: its leading process and
  when that started). A Swarmed that ended without stopping its stacks finds them again, counts
  them as running and can stop them. Stopping asks the stack's process group, waits, then
  ends whatever was started from it and is still there: services often sit in groups of their
  own, where a signal to the stack's group does not reach them. When the process started is
  asked of `ps` with `LC_ALL=C` and `TZ=UTC`, so it reads the same to every Swarmed. A project
  runs no more stacks at once than its `max_stacks` (`start_stack` refuses the next).
- **Two Swarmeds can run on one data directory** (`presence.rs`): the installed app beside a
  development build, or the app beside a command. Only the first to arrive, known by a lock on
  `run/first.lock`, settles sessions when it starts and ends stacks it did not start when it
  quits (`Core::arrive`, `Core::leave`). Any other ends only its own (`stacks_stop_started`),
  which is all the command line ever does.
- **A session left busy by a Swarmed that ended mid-turn** is put right: the app settles all
  of them when it starts (`Core::arrive`), and Stop on one records its turn as stopped.
- **A stopped agent is given `GRACE` to go, then killed** (`sessions.rs`): closing its input
  ends most, and the rest must not write on into a worktree being removed. A backend returns
  the tasks that hold its process (`AgentHandle::tasks`); aborting them kills it. Removing a
  task waits until its agents are gone (`sessions_ended`); deleting a session kills at once.
  A run that went without a word gets its `Exited` from the core. What a run still says after
  another has taken its place changes nothing about the session, and its end is dropped.
- **Whatever Swarmed starts gets the `PATH` of the user's shell** (`shellenv.rs`). An app
  started from the Dock has only the system's, so the installed app asks the shell once, before
  the core opens; hooks, stacks, the status command and agents are then given it.
- **Antigravity** (`agy acp`, Google's agent, on the user's Google sign-in) is a known agent
  like Codex. It names its effort `thought_level` and its modes `default`, `auto` and `plan`,
  which `acp.rs` maps supervision onto. What it asks permission for is decided by its own
  settings as well: with `toolPermission` set to `always-proceed` there, it never asks.
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
- **A command that reads much or starts something runs off the main thread.** Tauri runs a
  command that is not `async` on the thread that also serves the window, so one that reads a
  whole conversation, searches, sums up costs or asks the keychain is marked
  `#[tauri::command(async)]`. Commands whose order matters (what is typed into a terminal,
  what is sent, queued or answered) stay as they are: the main thread runs them one by one,
  in the order they came.
- **`crates/app/src/tests.rs` calls every command the way the UI does,** by name with JSON
  arguments, on Tauri's mock runtime. Nobody can look inside the native window from CI, so a
  new or changed command gets a call there, with the argument names `ui/src/api/tauri.ts` sends.
- **The UI talks to the app through `Api`** (`ui/src/api/client.ts`). `tauri.ts` is the real one;
  `mock.ts` serves sample data when the UI runs in a plain browser. `types.ts` mirrors what the
  core serialises by hand, with the core's snake_case field names: change both together.
- **UI logic lives in pure functions** with tests: `tree.ts` for the sidebar, `transcript.ts`
  for turning a session's events into what the transcript shows. Components and the store stay
  thin.
- **The menu bar is the app's own** (`crates/app/src/menu.rs`): the system's items, and
  Swarmed's (About, Settings, Keyboard Shortcuts), whose ids the window hears as a `menu` event
  and acts on (`menuChosen` in `ui/src/store.ts`). A new item of Swarmed's gets its id in both.
  Keep the Edit menu's items: on a Mac, undo, copy and paste reach a text field through them.
  The version shown in the app is the app crate's, which is the workspace's.
- **Keyboard shortcuts are one table** (`ui/src/hotkeys.ts`), matched in `useHotkeys.ts` and
  listed in Settings from the same table. A new action gets its shortcut there, not a key
  handler of its own. Combinations macOS or the window already use are not taken.
- **A layout is a setting apart from palette and style** (`Settings::layout`, `data-layout` on
  the root). Classic is what `app.css` draws; `layouts.css` holds only what senior changes:
  the text and spacing tokens, quiet colours mixed from the palette's own, and fixed-size icons
  enlarged with `zoom` rather than given second sizes. Size new things with the tokens and they
  follow; a new fixed-size icon goes into the `zoom` lists there. Each layout remembers its own
  sidebar width (`sidebarOf`).
- **The custom layout is data, checked in the core** (`layouts.rs`, `layout.json` in the data
  directory): sizes, spacing, fonts and colours on top of classic or senior, never which parts
  the app has. `CustomLayout::parse` refuses the whole file with the reason if a number is out
  of range, a colour is not `#rrggbb`, or text would not stand out from the backgrounds given;
  a new field gets a range there, a line in `layout_schema`, and a line in the UI's
  `customLayout.ts`, which writes only values of a form it knows into the stylesheet it builds.
  Agents save it whole through `configure`'s `layout` kind, at once: it cannot hide or move
  anything, and it has to stay that way for that to be safe.
- **The terminal is xterm.js, drawn from the palette and the user's `terminal.json`**
  (`termconfig.rs`, checked like the custom layout; `terminalLook.ts` on the UI side). All
  sixteen colours come from the palette's tokens unless the configuration names its own, so
  they follow every palette and layout. Addresses open through `open_link`. Programs may write
  the clipboard (OSC 52) and are never given its contents: keep the provider write-only.
  Drawing on the graphics card (`gpu`) is off by default, because it has not been seen to draw
  right in the app's own window; check there before changing the default. The shell is started
  with `COLORTERM=truecolor` and `CLICOLOR=1`.
- **Closing a session keeps it; deleting one does not** (`session_close`, `session_delete`).
  A closed session is put away with its conversation and can be reopened; until it is, its
  agent cannot be started and nothing can be sent to it (`Error::SessionClosed`). A deleted one is
  gone from the database with its events and its spooled views; sessions it started stay, with
  nobody as their starter. `AgentEvent::Deleted` is published and never stored, since there is
  nothing left to store it in. Swarmed does not touch what an agent keeps of the conversation
  in its own folders. The close button always asks, and offers both.
- **"Open in Terminal" hands the worktree to a terminal application** (`open::in_terminal`,
  `Settings::terminal_app`) through macOS's `open -a`, whose exit says whether there is such an
  application. It is apart from the session's own terminal, which is xterm.js in the window.
- **A skill can be called by name, `/name`, on any agent** (`Paths::skill_called`, used by
  `send_text`). Claude Code answers to its own skills itself; for every other skill and agent
  Swarmed sends what the skill says with the rest of the message, and records the message as
  it was typed. `slash_commands` lists those names after the agent's own commands, which is
  all the menu has for agents that report none (every ACP agent so far). The skills lookups
  live on `Paths` so that a queued message, sent from a task without the core, gets the same.
- **The Files tab walks a task's worktree one folder at a time** (`files.rs`, `Files.tsx`,
  `files.ts`). A folder is read when it is unfolded, never the whole tree. Paths come from the
  front end and are refused unless they stay inside the worktree, by their wording
  (`stays_inside`) and again once links are followed (`files::inside`); a link to a
  folder is listed as a file, since it may lead out. A file is opened in the user's editor
  together with its worktree, so an editor with a window per folder shows it in that window.
  Swarmed shows no file's contents itself: that is the editor's job.
- **A place in a front end's list of events means nothing to the core.** That list also holds
  events that are only shown and never stored (`TextDelta`, `Queue`, `ConfigChanged`,
  `Deleted`), so it is longer than the core's. Anything a front end points at in a
  conversation is named by something both sides count alike: a fork is asked for "from
  message number n" (`session_fork`, `messageNumber`), never "from event n".
- **A session never changes its agent once it has begun; it is cloned instead**
  (`session_fork` with an agent). The clone is a new session in the same task on the other
  agent, given what was said as text with its first message, like any fork. It starts on that
  agent's own model and effort, and asks where the first one planned, since only Claude Code
  plans. The first session is untouched. What the first agent read or did is not carried
  over: only what was said.
- **Check UI changes in a browser** with `pnpm dev`. The mock agent answers plain text, asks
  permission for `run: <command>`, answers `show …` with an HTML view, and runs until stopped
  for `slow`; one sample session has 2,000 messages for checking scroll speed. The README's
  screenshot and GIF are of this sample data, never of anyone's real projects.
- **MCP servers are one pool for every agent** (`mcp_pool.rs`). When a session starts, each
  agent's servers are read from its own configuration (`mcp.rs` and `mcp_plugins.rs` for
  Claude Code and its plugins, `mcp_codex.rs`, `mcp_opencode.rs`), and the session's agent is
  given the ones it has none of by that name, switched on or off: an agent refuses a second
  server of a name, and one switched off there is off for a reason. Claude Code is given the
  others' too. A server from the repository's own `.mcp.json` is the exception: a branch can
  put anything there, so it goes to other agents only once the user approved it in Claude Code
  (in their own settings, never in a file the repository tracks) and never when rejected there
  (`claude_code_servers_to_share`). Claude Code is handed its servers in a file only the user
  can read, under `run/agents/<session>/`, never on its command line, where `ps` would show
  their secrets. Nothing is written into an agent's files. `shared_mcp_servers = false` in
  `agents.toml` turns it off for an agent. The definitions hold secrets: they are passed on
  and never logged, and their `Debug` output leaves values out. Keep it that way.
- **Questions an agent puts to the user are answered where they stand** (`questions.ts`,
  `QuestionPrompt.tsx`, `session_answer`). Claude Code asks them through its `AskUserQuestion`
  tool, which arrives as a permission request; answering allows it with the answers added to
  its input, each under its question's text, several joined by a comma. Allowing it without
  answers tells the agent nothing, so the UI never offers that. A number, the arrows, the
  space bar, Enter and a click all choose; one question with one answer is sent as it is
  chosen. Input the UI cannot show whole falls back to the plain allow-or-deny prompt.
- **A sub-agent is read from its session's events, never a session of its own**
  (`AgentEvent::Sub`, `subagents.ts`, `SubAgentPane.tsx`). Claude Code names, on each message
  of an agent it started, the tool call that started it; `claude.rs` wraps what such a message
  holds in `Sub` under that call, so it is stored with the session and stays out of the
  conversation, which is between the user and the session's agent. A sub-agent is at work
  from its `Agent` (earlier `Task`) call to that call's result, and the UI works that out
  from the events it hears: the sidebar lists the ones at work under their session and drops
  them when they are done, and pressing one shows its own conversation, read-only, in place
  of the session's. Nothing in the store or the database knows of sub-agents. Conversations
  recorded before this have a sub-agent's words among the agent's own.
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
- **What agents leave running** (`background.rs`) is read back from the sessions' histories:
  Claude Code reports a `task_started` and a `task_notification` for each background command
  or agent, which become `AgentEvent::Background`. Stopping one is the `stop_task` control
  request. What ran under an agent that is no longer running is listed as stopped. A stack's
  `start` command is spawned with `std::process`, never Tokio's: window commands that are not
  `async` run on a thread without a runtime, where a Tokio spawn aborts the app.
- **A snapshot is the files git ignores in one well set up task** (`snapshots.rs`), one per
  environment, copied with `cp -c` so that APFS clones them. A new task gets them put into its
  worktree before `setup` runs, never over a file that is already there. They may include the
  likes of `.env`: the folder is the user's alone and its contents are never logged.
- **Prompts the user keeps** (`templates.rs`) are a name and a text in `templates.toml` in the
  data directory. A front end puts one into the message box; the core never sends one by
  itself, and agents are not told about them. `{{something}}` marks a place to fill in.
- **Attaching a file is naming its path** (`attach.rs`). Nothing is uploaded or copied: the
  agent runs on this machine and reads the file where it is. `with_files` lists the paths
  under the message in one fixed form, which the UI reads back (`splitFiles`) to show them as
  files. The front end gets paths from the system's file picker and from files dropped on the
  window, never file contents.
- **Each session can have a terminal** (`terminal.rs`): the user's shell on a pseudo-terminal in
  the task's worktree, shown in place of the message box. Its output is published on its own
  channel (`Core::subscribe_terminals`) and not stored. What it printed since the agent's last
  turn is appended, as plain text, to the user's next message and recorded as `TerminalShared`.
  The agent cannot type into the terminal. Each piece of output says where it starts and
  `terminal_open` says where its replay ends: a front end listens first, then opens, and draws
  only what lies past that end, so nothing is lost or shown twice.
- **Agents may answer with HTML views**: a fenced block with the language `html-view`, which
  they are told about through `views::INSTRUCTIONS`. The UI wraps the fragment in a document
  (`ui/src/htmlview.ts`), hands it to the core (`views.rs`), and shows it in a sandboxed frame
  loaded from `swarmed-view://`, a scheme the app serves with a policy of its own
  (`view_response` in the app crate). Tauri counts that scheme as the app's own and checks
  the app's commands against no capability; a view cannot call one because the window's
  invoke key is given to the main frame only, the frame is sandboxed, and its policy lets it
  send no request. Never show agent HTML in the app's own origin, never loosen the app's policy
  for it, and never hand a frame the key.
  A view cannot open anything either. A navigation the window refuses is only refused
  (`navigation.rs`), and a link pressed in a view is shown by the host with its address and
  opened when the user says so there: a view's script could ask without a press, with anything
  in the address. A copy opened in the browser carries the policy as its very first tag.
  The instructions are deliberately firm that this is the way to show something and needs no
  tool: without that, agents build a file and drive a browser instead. An ordinary `html` block
  stays code, with a button to show it as a view.
- **A view's frame is as tall as its content, and the view never scrolls up and down inside
  it.** The script Swarmed adds to a view hides its vertical scrollbar and counts a horizontal
  one into the height it reports. Where scrollbars take up room (a Mac with a mouse), a
  scrollbar appearing made the view narrower, a view laid out by its width changed height, the
  frame followed, and the two flickered between two layouts for ever. `nextFrameSize` also
  holds a view that swings between two heights at the taller one. When the frame stops
  following a view (`holdsBack`), the host tells it so and it may scroll again. Test a change
  here in Safari with "Always show scroll bars": Chromium does not reproduce the loop.
- **Actions are buttons of the user's own** (`actions.rs`): a name, an optional icon, and either
  a prompt or a command. Those for every repository are in `actions.toml` in the data
  directory, a repository's own in `projects/<name>.actions.toml`; its own takes the place of a
  general one of the same name. The files are meant to be written by hand or by an agent
  (`configure` on the tool server, `swarmed action` on the command line), so an entry that is
  not a usable action is left out instead of failing the lot, and written back as it was when
  the file is saved. The core only keeps them. The UI
  (`ActionBar`) sends the prompt to the session on screen, or types the command into that
  session's terminal: a command is run where the user sees it and can answer it, never out of
  sight. However many there are, the first six are buttons and the rest a menu. They are
  shown in the order their files keep them, general ones first; `action_move` changes it. Do not add a
  built-in button for one person's workflow: that is what an action is for.
- **Agents change Swarmed's own configuration through one tool, `configure`** (`configure.rs`),
  in three kinds. Done at once: prompts, actions that send a prompt, the status line's pieces,
  appearance. Only proposed: rules, environments, an action's command, the status line's
  script. These outlive the session, so an agent that read something hostile could plant one;
  the change is shown in the conversation (`ConfigProposed`) and made when the user accepts it
  there (`Core::config_answer`), whatever the agent's own permission mode. Not reachable at
  all: API keys, MCP servers, sign-ins, supervision, notes. Keep it one tool: every tool's
  description is sent to every agent in every session. A new setting goes into one of the three
  kinds on purpose; when unsure, propose. An environment's variable values are never told to
  an agent, only their names. An agent may set variables and ports, one by one; the proposal
  shows the values that call gives and no others, because a conversation can be read by other
  agents. Listing the `environment` kind also returns the guide to hooks, `{{port.<name>}}`
  and the other variables (`ENVIRONMENT_GUIDE`): that is where an agent learns to keep tasks
  apart, so keep it true. An environment's `ports` add to the project's. Proposals are held in memory, so one from before a restart has
  to be asked for again. `ConfigChanged` is published and never stored: it tells front ends to
  read the settings again.
- **A mark is a colour and a few words on a session** (`marks.rs`), set by the agent with
  `set_mark`, by `swarmed session mark`, or taken off by pressing it in the sidebar. It says how
  the work stands, in the user's terms; the status says what the agent is doing, and the two
  never mix. It is cleared when the user writes to the session. Colours are names, drawn by
  each theme, so a mark never carries a colour value.
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
  a message in the other conversation, queued if that session is busy. Only open sessions of
  the caller's own project can be read or written to (`peer_in_reach`): a closed one would
  be started out of sight. A spawned session runs
  in its parent's task with its settings, cannot spawn itself, and a session can have
  `MAX_SPAWNED` open. The tools are not on the allow list: in ask mode the user is asked.
  A session can close only sessions it started, and only idle ones (`peer_close`).
- **A session's agent can be changed until it has begun** (`session_set_agent`): before its
  first message reaches an agent, which from then on holds the conversation. The model goes
  back to the new agent's default. The UI offers the agent menu for exactly that long.
- **A fork is a new session that starts with a copy of what was said** (`fork.rs`): the user's
  messages, the replies and the views, then a `Forked` event. "Fork from here" on a message
  takes that message and everything after it, and nothing from before; the first session
  keeps all of it, since its agent cannot be made to forget. No agent can
  be handed another's memory, so the fork's agent is given that conversation as text with the
  first message sent to it, whatever agent it is, and again with each message until a turn in
  the fork has ended well: an agent that died on the first one heard nothing. What is recorded is what the user typed.
- **A closed session is put away, not deleted.** Closing is an event, `Closed`, so every front
  end drops it from its lists. Its task counts its closed sessions, they can be listed
  (`sessions_closed`) and put back (`session_reopen`) with their conversation as it was.
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
- **ACP agents with an effort of their own say what it stands at** when a conversation opens,
  which is recorded as `Applied` too. Before a session's agent has said, a front end shows
  what the agent's default last came to (`effort.rs`, `Core::effort_default`): the latest
  `Applied` of a session on the same agent and model that chose no effort.
- **Whatever sends into a session queues when its agent is busy** (`sendOrQueue` in the UI):
  a message sent straight in would clear a request the agent waits on. A request takes the
  keyboard only when nothing has it (`focus.ts`), and the answer shortcuts never allow a
  question without answers (`pending.ts`).
- **Work goes to a cloud agent through the user's own `codex` or `claude`** (`cloud.rs`), which
  works on the repository as GitHub has it. From a session there are two ways to tell it what
  the session arrived at (`CloudDialog.tsx`). A brief: the session's agent is sent a message
  asking it to write one and hand it over with its `delegate_to_cloud` tool, in the
  conversation, where the user sees it. Or the conversation itself (`handoff.rs`,
  `cloud_delegate_conversation`): what was said and the plans the agent proposed, as text
  under what to do, its latest `MAX_HANDED` characters. Never what the agent read or ran, which
  may hold anything, and no file: the form says what of the task is not on GitHub.
- **Messages can wait in a queue** (`queue.rs`) while a session's agent is busy. The core sends
  the first one when a turn ends well, as if it had just been typed; a turn that was stopped
  or failed leaves the queue standing. The queue lives in memory, and its changes are published
  as `Queue` events that are never stored, like the pieces of a reply.
- **What is worth interrupting the user for is decided in the core** (`notices.rs`): a session
  that asks for a decision, finishes a turn, or fails. A front end shows it only while the user
  looks elsewhere; the app does so as a macOS notification (`crates/app/src/notify.rs`) and
  counts what waits on its icon. A notice names the tool and the agent's own summary, never the
  tool's arguments, which may hold anything.
- **Banners need the built app.** The system's notification centre only knows a bundle with an
  identity, which `pnpm app:build` makes (signed ad hoc, which is enough) and `pnpm app` does
  not. The bundle asks once for permission, shows banners as Swarmed, and a click opens the
  session (`notice-opened`). A development build falls back on showing them as the Terminal's,
  which newer systems may not show. `Swarmed.app --args notify-check`, started with `open`,
  says what the system allows and sends one banner. Whether this is the built app is decided
  by where the program lies (`Name.app/Contents/MacOS/`), not by its having a bundle
  identifier: the Terminal fallback makes the program answer to the Terminal's identifier,
  after which a development build looked like a bundle and its second notice crashed it.
- **Whether the user is looking** is asked of the window: focused, shown and not in the Dock.
  A window put away can still hold the focus. Closing the window hides Swarmed, on a Mac;
  ⌘Q quits it. The count on the icon needs the notification centre's leave too, which the
  bundle asks for with the rest. `SWARMED_NOTIFY_TRACE=<file>` notes what became of each notice.
- **Swarmed never handles Claude.ai credentials.** It only shapes the child's environment (`Auth`).
- **Session events are stored as JSON** (`AgentEvent`), so its variants and fields are only added,
  never renamed.
- **A task's changes are read from git on demand** (`changes.rs`): everything that differs from
  the merge base with the task's base ref, uncommitted and untracked files included. The view is
  read-only. The UI reads again when one of the task's agents finishes a tool call or a turn.
- **A task's pull request and checks are asked of the user's own `gh`** (`pulls.rs`), in the
  task's worktree, on demand. Swarmed holds no GitHub token. No `gh`, no pull request, or a
  repository that is not on GitHub all mean the same: the task has none, which is not an error.
- **A task's work leaves its worktree only when asked** (`publish.rs`): commit everything, push
  the branch, open a pull request, each a step of its own behind a button. Commits are made by
  the user's git as them, the pull request by their `gh`; nothing is added to either.
- **A pull request's title and description can be written for the user too**
  (`task_pull_message`), by the same writer as a commit message and under the same rules:
  shown the branch's commits and its diff against the base (three dots, so only what the
  branch itself changed), told that this is data and not instructions, and nothing is opened.
  A repository's own pull request template is handed on and filled in. The first line of the
  answer is the title. The writer is told not to claim anything was tested, since it cannot
  know.
- **A commit message can be written for the user** (`task_commit_message`), by the agent of the
  session on screen: for Claude Code the `Namer`'s small model, for any other agent one question
  in a conversation of its own (`asking.rs`), on the session's model, in no worktree, with every
  tool refused and the agent in its planning or read-only mode. One that has modes and neither
  of those is not asked: it would not ask before using a tool. New files are read as files
  only, a link never through to what it points at. Whoever writes is shown what git says changed, new files included, and the
  answer goes into the commit box to be read and changed. Nothing is committed by it.
- **Removing a project** (`project_remove`) removes its tasks as `task_remove` does, keeping
  their branches, then deletes what Swarmed holds about it: sessions, their history, the
  notes, its snapshots, environments and own actions. Its config file stays. The repository is not touched. Removing a task stops its sessions first. In the UI
  both ask first, and offer to go ahead regardless only after a refusal over unsaved work.
- **Only unsaved work stops a task's removal** (`task_remove`): changes not committed, commits
  not pushed, or a worktree git no longer knows, whose work cannot be checked. The stack is
  always stopped. A `stop` or `teardown` that fails, or outlasts `HOOK_LIMIT` and is then
  killed with what it started (`hooks::run_within`), a broken
  configuration, and a process that outlives being killed are passed over and returned as
  notes, which the UI shows once the dialog is gone. Before the worktree goes, whatever still
  runs in it is ended (`leftovers.rs`): a stack Swarmed never knew of, or one whose services
  outlived an earlier stop, would otherwise go on recreating files as they are deleted. A shell
  open in a terminal, what keeps terminals open (tmux and the like) and the processes of an
  application from outside the worktree are left alone. The worktree is then
  moved into `.removed` beside it, which is instant, and deleted by an `rm` of its own;
  `Core::arrive` deletes what one that was cut short left, and `usage.rs` counts what is
  still there. A removal under way is kept in the UI's store (`removals`), so a dialog closed
  and asked for again starts no second one. An environment that tasks use
  cannot be removed (`Error::EnvironmentInUse`).
- **Opening things elsewhere** (`open.rs`) starts the user's editor (`settings.toml`, VS Code by
  default) or the browser and never waits for them.
- **A hook's command takes each value as one word** (`Vars::render_for_shell`): a value with
  anything a shell would read is put in by the name of its `SWARMED_*` variable, never as text,
  so a branch name cannot run. Unknown-length lists from git are read NUL-separated (`-z`).
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
- A PR with a change a user would notice adds its line under `## [Unreleased]` in
  `CHANGELOG.md`, under Added, Changed, Fixed or Removed, written for the user. The release
  workflow publishes that section as the release's notes; `RELEASING.md` has the steps.
- No AI attribution anywhere: no `Co-Authored-By`, no "Generated with" lines, no session links.
- PR descriptions: a bold one-sentence summary, a `Risk:` line, then `## Problem`, `## Change`
  and a `## Verification` table.
