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

Of `cargo deny check`, the advisories fail no run: one is published against crates already in the
lockfile, whatever a pull request changed. The Dependencies job shows it in a step of its own.

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
- **A chat is a session with no repository** (`chats.rs`, `ui/src/chats.ts`, #357). The chats
  are the tasks of one project, `CHATS` (`.chats`), made the first time `task_new` is asked for
  one there. No repository can take that name (`validate_name` wants a letter or digit first),
  and nothing keys on what the window calls them (General, a word of the style), so a
  repository called `general` is a repository like any other. `Project::chats` marks it.
  - **A folder, not a worktree.** Each chat has an empty folder under
    `~/swarmed/.chats/<chat>`, no branch and no setup. Its agent is told so
    (`chats::INSTRUCTIONS` in place of the worktree guidance) and gets no `SWARMED_*` task
    variables. `git::head_now` answers only for a folder with a `.git` of its own, so a chat
    inside a home folder kept in git never reports a move.
  - **What only a repository has is refused, not tried.** `Project::repo` is the repository's
    path or `Error::NoRepository`; changes, publishing, pull requests, snapshots and the cloud
    look their task up with `repo_task`, and cleanup leaves the chats out of git. Use
    `repo()` for any new git call on a project's path, never `repo_path` itself.
  - **Removal** (`chat_remove`) takes the folder, and without `force` refuses one that holds
    anything (`TaskNotClean`), since an agent's files there exist nowhere else.
  - **Reach.** From a chat, `list_sessions` and the other peer tools reach every open
    session. A repository's sessions list and read their own, and can write to a chat, so one
    a chat wrote to can answer it. They cannot list or read one (`Access`): a chat may hold
    what it read of another repository.
  - **Carrying on in a repository** is a task there and `session_fork_into`: the whole
    conversation, the same agent and title; the chat stays.
  - **In the UI** New task starts on No repository; a repository's plus preselects it. The chats
    are drawn above the repositories, apart from their order (`project_move` skips them).
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
  A sub-agent's run that starts or ends after the turn, or a request it makes then, counts
  the same, so that the session is told it is done.
- **What Swarmed cannot understand in its configuration costs only itself** (`lenient.rs`,
  #300). Two Swarmeds of different versions can share one data directory, and one key the
  older does not know must not stop it.
  - **Not refused whole:** the files are read with `serde_ignored`, not `deny_unknown_fields`.
    A key a struct does not know is left out at any depth and named.
  - **A value that cannot be read** costs the smallest part it is in: a setting's key at its
    default (`lenient::fields`), or one entry left out of `agents.toml`, `mcp.toml` or a
    repository's environments (`lenient::entries`).
  - **A file that is not TOML** gives the defaults where there are safe ones (settings, agents,
    MCP servers, skills) and is kept as `<file>.unreadable` before it is written over. A
    repository's configuration or environments stay an error: there is no telling what their
    hooks were.
  - **Said, never in the way:** `Core::config_problems` names each problem with its file. The
    window shows them in its problem banner and the command line on stderr.
  - **Settings keep unknown keys:** saving the settings writes keys this version does not know
    back as they were.
  - **New configuration files** are read the same way. A new struct for one gets no
    `deny_unknown_fields`.
- **A stack is also remembered on disk** (`run/stacks/<task>.pid`: its leading process and
  when that started). A Swarmed that ended without stopping its stacks finds them again, counts
  them as running and can stop them. Stopping asks the stack's process group, waits, then
  ends whatever was started from it and is still there: services often sit in groups of their
  own, where a signal to the stack's group does not reach them. When the process started is
  asked of `ps` with `LC_ALL=C` and `TZ=UTC`, so it reads the same to every Swarmed. A stack is
  refused (`start_stack`) while as many of the project's stacks run as `max_stacks` allows: the
  task's environment's, if it sets one, else the project's.
- **Each task can have a Chrome of its own** (`browser.rs`, `BrowserPane.tsx`, `useBrowser.ts`,
  `browser.ts`, #382), shown live in the Browser pane beside the conversation. The pane's
  hooks are in `useBrowser.ts` and only act; what they decide is in `browser.ts`, with tests.
  - **One process per task**, on the Chrome, Chromium, Edge or Brave found (`browser/find.rs`)
    or `Settings::browser_program`, headless, on `browsers/<task>/profile` (0700), so logins
    outlast restarts and stay the task's. Headless, it draws at `RENDER_SCALE`
    (`--force-device-scale-factor`, `process.rs`): it draws one pixel to a point whatever
    device scale is emulated, which a Retina pane enlarged into a blur. Deleted with the task (`browser_forget`), listed by
    the clean-up when its task is gone. A missing browser is `Error::BrowserMissing`, and the
    pane offers Homebrew's `google-chrome` cask (Chromium's was disabled on 2026-09-01).
  - **Swarmed's connection is a WebSocket to the port** Chrome writes into
    `DevToolsActivePort` (`--remote-debugging-port=0`, on 127.0.0.1), with a client written
    here for loopback only (`browser/ws.rs`), not a pipe: agents' tools need the port anyway
    (#384), and a Chrome an earlier Swarmed started can only be reached by it. Never pass
    `--remote-allow-origins`. Over it, the DevTools protocol (`browser/cdp.rs`): commands with
    numbers, events, one session per page (`flatten`).
  - **What the pane shows is pure** (`browser/page.rs`: tabs, console, dialogs from events;
    `browser/input.rs`: clicks, keys, the address bar), tested against made-up events; the rest
    against a stand-in Chrome (`tests/fixtures/fake_chrome.py`), and against a real one with
    `cargo test --test browser -- --ignored`.
  - **Pictures** (`Page.startScreencast`) are asked for only while the pane shows a tab, at
    most `FRAMES_A_SECOND`, acknowledged at that pace, and published on their own channel
    (`Core::subscribe_browsers`, `BrowserEvent`), never stored. A page's script can change its
    title without Chrome saying so, so the shown tab's info is read again with its pictures,
    at most once a second. Chrome refuses a second start on a session (158 does), so `cast`
    remembers what it asked (`Cast`), leaves the same asked again alone, and stops before it
    starts otherwise, under one lock (`casting`).
  - **Two qualities** (`browser/pace.rs`): `STILL` for a page that stands still, `MOVING`
    once pictures come close together (`Motion`), and `STILL` again after `SETTLE` without
    one (`recast`, which starts them again: a start sends one picture, the sharp one). Never
    a screenshot for it: Chrome draws the page again for one, which reads as moving, and the
    stills set each other off.
  - **Or a window of its own** (`Settings::browser_display`, `BrowserDisplay::Window`):
    started without `--headless`, the forced scale or a window size, so Chrome draws it and
    keeps where the user put it. Which way a Chrome runs is read from it (`windowed`: a
    headless one names itself `HeadlessChrome`), so one an earlier Swarmed started counts as
    it is, and `BrowserState::windowed` says so. A windowed one gets no emulation and no
    pictures (`cast` does nothing); the pane lists its tabs, runs its toolbar and console,
    passes on no input, and offers `browser_raise` (`Target.activateTarget`), which only the
    user asks for: a session's tools acting in a tab never take the focus. It is never
    stopped for sitting unused (`stopped_unused`). The setting applies as a Chrome starts; the
    pane offers to start one again that runs the other way (`restartTo`).
  - **Chrome's blank page is never painted headless**, so its pictures are black: the pane
    draws a tab on `about:blank` as white (`blankPage`) and no override of the background
    changes that. A stop keeps the open web pages (`browser/reopen.rs`, `tabs.json`, 0600,
    at most 20, `http` and `https` only, read back with the same check and used up as they
    are: only a Swarmed connected to it keeps them, so one stopped otherwise, as the command
    line's `--stop` does, must not bring back an older stop's) and a start opens
    them over the connection, never on the command line, then closes the blank tab it
    started on unless it was used meanwhile (`unused`: the browser can be reached while they
    open). A stopped Chrome's connection can end after a new one started in its place:
    `gone` then says nothing, and a record is deleted only for the Chrome it names
    (`process::forget`).
  - **Input goes in order**: the mouse, keys and text commands are not `async` in the app and
    send without waiting (`Cdp::send`). ⌘V is the window's: Chrome's clipboard is its own.
  - **Remembered on disk** as a stack is (`run/browsers/<task>.pid`), counted in
    `TaskOverview::browser` (the sidebar's mark: never hide running work) and watched by
    `elsewhere.rs`. A start takes `run/browsers/<task>.lock`, so two Swarmeds connect to one
    Chrome rather than start two. **Stopped** with `Browser.close`, then its process group as a
    stack's: when its task is put away or removed, by `leave` (the first Swarmed every one,
    any other those it started), and by `follow_browsers` (the house only) after
    `browser_idle_minutes` unused, not while the pane shows it. The house also connects to one
    another Swarmed left, so that its unused time counts. The command line's runs on after it.
  - **Agents drive it with their own browser tools** (#384, `browser/agents.rs`), never a
    tool of Swarmed's: Playwright MCP and Chrome DevTools MCP, recognised by what they run
    (`browser/servers.rs`), are given `--cdp-endpoint` or `--browser-url`. Claude Code gets a
    pointed copy of each of its own (`Paths::claude_code_own_servers`: approved ones and
    plugins' `plugin:<plugin>:<server>`) in its `--mcp-config` file, whose server of a name takes
    the place of its own (tried with 2.1.332); never one of the repository's it would ask
    about first. Other agents' servers are pointed as Swarmed hands them over; their own
    configuration is not touched (codex-acp keeps its own of a name). Nothing the user did not
    set up is added.
  - **Each session has an address** (`browser/address.rs`, `SWARMED_BROWSER`): a listener on
    `127.0.0.1` from when its agent starts, made once a front end serves agents' tools
    (`serve_session_tools` calls `browsers_listen`). The first connection starts the task's
    Chrome (`browser_of`), so a session that never browses starts none. Bytes pass unchanged:
    Chrome builds the addresses it hands out from the `Host` sent, so they lead back here.
    `Sec-WebSocket-Extensions` is dropped so messages stay readable, and a request with an
    `Origin`, a `Sec-Fetch-Site` other than `none` or another host is refused before
    anything starts. Short messages are read on the way (`ws::Unframer`, `watch.rs`, pure)
    for the tab a session's tools act in: `Browser::acted` marks it (`BrowserTab::acted_by`)
    and the pane, on screen, goes to it when another tab was acted in before. Nothing read is
    kept or logged.
  - **`show_browser`** (`browser_show_for`) opens the pane on an address or the stack, in the
    session's own tab (the one it acted in last, else an unused empty one, else a new one), and
    publishes `BrowserEvent::Show`, which opens the pane where the session on screen is in
    that task. Not on Claude Code's allow list: it navigates with the task's logins.
  - **Picking an element** (`browser/pick.rs`, `elements.ts`, #383): `browser_pick` puts the
    tab shown into Chrome's inspect mode (`Overlay.setInspectMode`) for the session on screen
    and waits; only an `Overlay.inspectNodeRequested` on Swarmed's own session of that tab,
    while that pick waits (`Browser::picking`), is taken. `browser_pick_end`, another pick,
    another tab shown, the pane hidden or the connection gone end it with nothing. What is read
    (DOM, `Accessibility`, box, and a script in an isolated world for the selector, text and
    styles) is kept as `attachments/<session>/element-<n>.json` beside its picture
    (`element-<n>.png`, `Page.captureScreenshot` clipped to the box and a margin, checked as a
    pasted image is) and never logged. A message names picks by number (`Attached::elements`);
    `message_for` lists them after the pull requests and before the files (`with_elements`),
    every line after an entry's first set in by two spaces and its text and HTML fenced with
    more backticks than they hold, so page content can neither close its fence nor pass for an
    entry or a field. The UI reads the list back only up to the fenced part (`splitElements`).
    The token, `[Element 3: label]`, follows the `@` and `#` pattern; the pane puts it at the
    caret of the session's box through the store (`placePick`, `pickWaiting`). A fork copies a
    pick's description with its picture. The mock picks from its drawn page.
- **Two Swarmeds can run on one data directory** (`presence.rs`): the installed app beside a
  development build, or the app beside a command. Only the first to arrive, known by a lock on
  `run/first.lock`, settles sessions when it starts and ends stacks it did not start when it
  quits (`Core::arrive`, `Core::leave`). Any other ends only its own (`stacks_stop_started`),
  which is all the command line ever does.
- **What another process writes reaches the window through its own core** (`elsewhere.rs`).
  Each process publishes only on its own stream, so the app runs `follow_others`, which calls
  `Core::notice_others` every second:
  - **Events:** those above the highest id dealt with (`Written::mark`) are published in order
    as `Live::note` would, flagged `SessionEvent::elsewhere`. Each event this process stored
    is noted in `Store::keep` and left out, so nothing is published twice. The last event of
    each session in a look, and an answer handed back, carry the status and unread state of
    the session's row; the others, what `SessionStatus::after` makes of them.
  - **The tree:** after another connection committed (`PRAGMA data_version`), the rows of
    projects, tasks, sessions and cloud tasks are read again; if they changed, or a stack's
    record under `run/stacks` did, `AgentEvent::TreeChanged` (session id 0, never stored) has
    front ends read the tree again.
  - **Configuration files:** a `.toml` or `.json` in the data directory or `projects/` that
    changed on disk is published as `ConfigChanged` with the kind `file` on session id 0, and
    a changed `agents.toml` is read again. Own writes count too: the UI keeps what it reads
    again when it is the same (`unlessSame`), and says the same problems once.
  - **A history read** (`session_events`) passes on what is above the mark first, under the
    same lock as a look (`Elsewhere::passing`), and reads only up to it (`events_upto`):
    everything it returns has been published, so a front end that reads it and adds what it
    hears shows nothing twice. A look that fails still publishes the events it read.
  - **Notices:** only the first Swarmed to arrive shows those of events from elsewhere, and
    not while a Swarmed that shows its own runs the session (`shows_notices_of`). An app that
    arrived second takes the house at its next look once the first has quit
    (`house_if_free`), and from then on also stops every stack when it quits. It settles
    then what the first left busy, as `arrive` would have (`sessions_settle`, which passes
    over what runs here or elsewhere): the first, opened again, finds the house kept and
    settles nothing.
  The first look takes stock and publishes no tree or file change. `swarmed events` drives
  the same from the command line.
- **A session's agent runs in one process at a time** (`claim_run`). `launch` takes a lock on
  `run/sessions/<id>.lock`, under the lock on the running sessions, and `Live::forget` lets go
  of it there once no run of the session is left. The system lets go of it when the process
  ends, so a crashed one blocks nothing. Another process refuses to start the session
  (`Error::RunsElsewhere`), to interrupt, close or delete it, to answer its requests, and to
  remove its task or repository, even by force; a message queued for it there goes once the
  lock is free (`Elsewhere::waiting`). The lock file is deleted with its session only while
  no other process holds it (`forget_run_lock`). `sessions_settle` passes over the
  sessions run elsewhere. The lock is unlocked before it is closed (`let_go_run`): a process
  another thread forks meanwhile holds the file open until it execs, and closing alone would
  leave it locked that long. A dropped `Core` lets go of its locks, as its process would.
- **A session left busy by a Swarmed that ended mid-turn** is put right: the app settles all
  of them when it starts (`Core::arrive`), and Stop on one records its turn as stopped. One
  whose agent another Swarmed runs is not left over, and is left as it is.
  An agent that went with its Swarmed never reported its end, so `arrive` also records one
  (`background_settle`) for each session whose history has something left in the background
  and no end after it: a front end reading that history would take a sub-agent left there to
  be at work for good.
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
- **What Swarmed takes up** (`usage.rs`) is measured on request: the database, notes and
  sessions' images by their files, each worktree with `du`, and memory and processor time with `ps`, summed over
  Swarmed's own process and everything started from it. An agent's share is the tree under the
  process its backend reported when it started.
- **What removed tasks leave behind is cleaned up on request** (`cleanup.rs`): removed
  worktrees not deleted yet (in `.removed`), the branches of removed tasks, stale worktrees
  (folders in a project's place under the worktrees root that no task owns, and git's records
  of worktrees whose folder is gone), snapshots, and the checkpoints and browser profiles of
  tasks that are gone. `Core::cleanup` lists them, each with a
  key; `cleanup_clean` deletes one kind, by keys or every item not at risk, after listing again,
  so only what is still listed goes. A record is listed only while nothing is at its path:
  git calls one prunable when the folder's `.git` file is missing, which a folder made again
  in its place also is, and no folder is ever deleted for an item listed as a record. A
  front end that showed a list says which keys it showed as at risk (`shown_at_risk`): one at
  risk now and not shown so is kept, since the list may be old. A repository that cannot be
  asked lists no branches and stops no other kind. Never listed: a current task's worktree or branch (its
  own, the others its worktree was seen on, its base), a task being made (`names_claimed`),
  anything a worktree of the repository has checked out, the repositories themselves and any
  folder with a repository of its own, a remote branch, and a branch Swarmed did not make
  (`tasks.branch_made`, written since; a task made before counts as its removal did: its
  branch unless it started from it). An item that would lose work found nowhere else (commits
  on neither the base nor a remote, uncommitted changes, commits on no branch, or one git could
  not check) is `at_risk`: it goes only when named, and the UI lists it apart, unticked, to be
  ticked one by one. A snapshot is forgotten through `snapshot_remove`, under its lock. The
  window's Usage tab and `swarmed cleanup` drive it; the window measures usage again after.
- **Running sessions live in the core** (`sessions.rs`). Front ends call `session_start`,
  `session_send` and the like by session id and read every session's events from one stream,
  `Core::subscribe`, which also carries what other processes wrote (`elsewhere.rs`). Events are stored before they are published, except the pieces of a reply
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
- **How tool calls show is the user's setting** (`Settings::tool_calls`): all, folded or
  hidden, applied after `toItems` by one pure function (`shownTools`). A call that talks to the
  user (a plan, questions, a view) shows whatever it says, and so does everything that is not a
  tool call, such as a permission request. Hidden puts a larger animation (`Swarming`) under
  the activity line while the agent works, so that a quiet transcript does not look stuck.
- **The menu bar is the app's own** (`crates/app/src/menu.rs`): the system's items, and
  Swarmed's (About, Settings, Keyboard Shortcuts), whose ids the window hears as a `menu` event
  and acts on (`menuChosen` in `ui/src/store.ts`). A new item of Swarmed's gets its id in both.
  Keep the Edit menu's items: on a Mac, undo, copy and paste reach a text field through them.
  The version shown in the app is the app crate's, which is the workspace's.
- **Keyboard shortcuts are one table** (`ui/src/hotkeys.ts`), matched in `useHotkeys.ts` and
  listed in Settings from the same table. A new action gets its shortcut there, not a key
  handler of its own. Combinations macOS or the window already use are not taken.
- **Every dialog is built with `Dialog`** (`ui/src/components/Dialog.tsx`), the one file with a
  `<dialog>` element, which `dialogs.test.ts` holds it to. It opens modal and gives the same
  three parts: a head with the title and ×, a body that alone scrolls, and a footer with the
  way out first and the main button last. Width is a size by name (`small`, `medium`, `large`,
  `wide`); spacing, radius and colours come from the tokens, so every layout applies. A click
  beside one closes it only when the dialog says nothing would be lost (`closeOnBackdrop`), and
  `busy` disables its actions but never the way out. Those two decisions and where the
  keyboard starts are pure functions (`ui/src/dialogs.ts`), tested there. A dialog opened from another keeps Esc to
  itself. A new dialog takes these props, not a frame or a class of its own.
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
  A key it does not know is refused on saving (`parse`) and only left out on reading
  (`CustomLayout::read`, through `serde_ignored`, colour names included), where
  `config_problems` names it: a newer Swarmed's key must not cost an older one the layout.
  The sidebar's levels (`sidebar`: repository, task, session, sub-agent) are drawn from
  `--sidebar-<level>-size`, `-weight`, `-colour` and `-background`, which only the custom
  stylesheet sets; each rule's fallback is what classic and senior draw, so keep it so.
  The core checks a level's colour against its own background only, so a session row that
  is pointed at, open, waiting or unread keeps to that pair (`sessionStates` in
  `customLayout.ts`: the background tinted with the colour) and never takes the palette's.
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
- **The sidebar's order is the user's, kept in the core** (`order.rs`): a `position` on
  projects, tasks, sessions and cloud tasks, which their lists follow. New rows go at the end,
  a cloud task at the top. A row moves `by` places among its siblings only, never to another
  parent: a session within its task, and one that works for another open session of its task
  among the others that work for that one, at any depth (`session_siblings`, as
  `nestedSessions` lists them). Rows
  not listed among the others, a closed session, an archived task or one put away (which the
  sidebar lists apart, in Done), keep their places among them and are not counted in a move,
  so one put back comes back where it was: the core counts exactly the siblings the sidebar
  draws together, or a drag lands short. Sub-agents have no order: nothing stores them. A move
  is published as `AgentEvent::Reordered` with session id 0, which no session has, and never
  stored; front ends read the tree again on it. Several reads of the tree are often under way
  at once and the core answers each on a thread of its own, so the store takes an answer only
  if none asked for later was taken (`treeReads`): an older one would put a row back where it
  was before a move or a pin (#407). The UI drags rows with pointer events, not HTML drag and
  drop, which would fight the window's dropping of files to attach (`useRowDrag`,
  `ordering.ts`), and ⌥⌘↑/↓ move the focused row or the open session.
  A repository, a task or a session can be pinned (`pinned`, `project_pin` and the like):
  lists order by `pinned DESC` first, a move stays among the rows pinned as it is (each pin
  state is a drag group of its own in the UI), pinning puts a row last among the pinned and
  unpinning first among the others, and a closed session or a task put away keeps its pin:
  pinning or moving either is refused (`Error::SessionClosed`, `Error::TaskPutAway`). A
  pin is published as a move is. Cloud tasks and the chats among the repositories are not
  pinned.
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
- **A command whose screen Claude Code has only in its terminal can be opened there, in the
  session's terminal, apart from its shell** (`terminal.rs`, `Program::Claude`,
  `Core::claude_terminal_open`). The transcript's line for `AgentEvent::TerminalOnly` has the
  button. It is a pseudo-terminal of its own (`claude_terminals`), never the shell's:
  - **Nothing it prints reaches the agent.** Only the shells' output is shared
    (`take_unshared`), and a screen such as `/usage-credits` shows a balance and card digits.
  - **It runs the same `claude` as the session**, from `AgentBackend::interactive`, in the
    task's worktree, with the shell's `PATH` and none of this process's `ANTHROPIC_*` or
    `CLAUDE*` variables, so it signs in as the user's own does. Swarmed handles no sign-in.
  - **It opens only a command the table leaves to Claude Code**, by a plain name in lower
    case (`commands::opens_its_screen`): never `/login`, `/cd` or one Swarmed does its own way. The
    exception is `/usage-credits` (`SHOWN_IN_PART`): Swarmed shows the figures, and the button
    under the usage screen opens Claude Code's own to buy credits or set their cap.
  - **It is a new conversation, not the session's.** Resuming the session's own would put two
    Claude Codes on one conversation.
  - **One per session.** Opened on the same command, the running one is shown again; on
    another, it is replaced, and what the replaced one still prints is dropped at the source.
    When it ends, its output says `ended` and the UI shows the shell again. A new worktree
    gets Claude Code's folder-trust question first, which is the user's to answer.
  - **An action's command makes it give way.** `runInTerminal` shows the shell again and the
    command is typed at once; Claude Code keeps running and is shown again when opened on
    the same command. A command held back until the shell is next shown would run unseen.
- **Each start of a terminal's program is a run with a number** (`TerminalOpened::run`,
  `TerminalOutput::run`), the shell's and Claude Code's alike. A piece of a run that another
  took the place of can already be on its way when the new one is opened, so a front end
  draws and ends only by pieces of the run it opened (`heard` in `terminalOutput.ts`).
- **A terminal reads all its program printed before it says the program ended.** `pump` keeps
  a hold of its own on the program's side of the pseudo-terminal: macOS throws away what is
  unread when the last holder lets go, and a program that prints and ends at once, with why
  it could not start for one, would end without a word. Held open, that side never reports
  its end, so `feed` waits for the program and tells `pump`, which reads on until nothing
  has come for `LAST_WORDS`.
- **Claude Code's own `/` commands are each one of six kinds, in one table**
  (`commands.rs`, `CLAUDE_COMMANDS`). In a terminal many of them open a screen; run as Swarmed
  runs it, Claude Code has none. A *screen* (`/usage`, `/status`, `/context`, `/cost`,
  `/memory`, `/skills`, `/hooks`, `/mcp`) is asked of Claude Code with one control request
  (`agent/claude/screens.rs`, the only place that knows their shapes) and arrives as
  `AgentEvent::Screen`: headed rows, a measure with its share. A *setting* (`/model`,
  `/effort`, `/config`) is put to the user as questions, `CommandAsked`, in the shape of an
  agent's own questions, and the answer (`session_command_answer`) is applied by the core:
  model and effort with `session_configure`, Claude Code's settings by sending it
  `/config key=value`, which it takes in words. One Swarmed does itself with nothing to ask
  is *own* (`/remote-control`, `/rc`, and `/rc off`): see Remote Control below. It is sent
  at once, and refused in the queue, where it would reach Claude Code as typed. A *choice*
  (`/rewind`, `/checkpoint`, `/undo`) is one the window opens a dialog of Swarmed's own for
  (`CommandKind::Choice`, `opensChoice` in `slash.ts`); as text, from the command line or the
  queue, it is refused like the next kind, with what to do instead. A command
  Swarmed has its own way to do
  (`/rename`, `/fork`, `/clear`) is refused (`Error::TerminalOnly`) with what to do instead,
  under any of its names (`CLAUDE_ALIASES`: `/new` is `/clear`). So is one that would move
  the session out of its worktree or hand it to something else (`/cd`, `/add-dir`,
  `/teleport`), named in the table even where Claude Code would refuse it itself: it
  can do `/add-dir` without a screen behind a switch. A refused command cannot wait in the
  queue either (`may_wait`). Anything else, and any name the table does not hold, is a
  message. That includes the commands that only open a screen in the terminal (`/vim`,
  `/privacy-settings`): they need no row. Claude Code answers them itself, with no model,
  and marks the answer `local_command_outcome: unavailable_headless`, which
  `agent/claude.rs` turns into `AgentEvent::TerminalOnly` instead of a reply. Neither a
  screen, a setting nor a refusal starts a turn or records a message. A new command of Claude Code's goes into the table on purpose:
  the test there lists what a known version reports. With words after it a screen's command
  is Claude Code's to take (`/mcp reconnect docs`), and waits in the queue if the agent is
  busy. What waits in the queue is sent as typed. The UI draws a screen as HTML it builds
  itself (`screens.ts`, every word escaped) in the same sandboxed frame as a view, and sends
  such commands at once instead of queueing them (`takenBySwarmed`). A screen is the user's:
  nothing that hands a conversation on (forks, peers, the cloud, search) reads it. Of
  `mcp_status` only names and states are read, never the definitions, which hold secrets.
- **A Claude Code session can be reached from claude.ai and the Claude app**
  (`remote.rs`, Remote Control). Claude Code offers a host that runs it without its screen a
  `remote_control` control request, as the Agent SDK's `enableRemoteControl` does, which
  `agent/claude.rs` sends and whose answer and `bridge_state` reports it reads into
  `AgentEvent::RemoteControl`. Swarmed handles no sign-in for it: Claude Code uses its own,
  which has to be a claude.ai subscription, and the conversation is kept on Anthropic's
  servers while it is on, which the panel that turns it on says.
  - **What is typed there is the user's.** Claude Code says each message back as it takes it
    in. One Swarmed did not send (`Live::sent`, the latest `SENT_KEPT`) nor tell, while
    Remote Control is on in that run, is recorded as `UserMessage` with `remote`, which a
    message typed here never has. Claude Code's own records of a command are not.
  - **The wish outlives the agent.** The session's latest `RemoteControl` says whether it is
    wanted (`RemoteState::wanted`). An agent started for a session that wants it turns it on
    (`remote::resume`, from `launch`), attached to the same session on claude.ai
    (`reattach_session_id`, kept by `keep_session_on_exit`) unless the latest report is a
    failure. An agent that goes leaves it `Paused`; the first Swarmed to arrive starts the
    agents of the sessions that want it (`remote_controls_resume`). Closing a session turns
    it off, and so does removing its task. Only the user turns it on: `/rc` takes nothing
    after it but `on` or `off`, and a prompt an agent starts a session with (`peer_spawn`)
    is sent as typed, never read as a command. What a run still reports of it after
    another took its place is dropped (`dropped_when_late`). The core fills in the address and id on each report from the one before, since
    Claude Code names them only when it is turned on, and drops a report that repeats the last.
  - **Changes made there are not all heard here.** A permission mode changed on claude.ai is
    not reported; the model is, after the turn. Views show as code there.
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
  A changed file's line in the Changes tab opens it the same way, from a button beside the one
  that folds its diff (`openable`: never a deleted file's, nor a cloud task's diff).
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
  permission for `run: <command>`, answers `show …` with an HTML view, ends its turn with a
  weekly usage warning for `warn <percent>` (resetting in that many seconds with
  `warn <percent> <seconds>`), ends its turn refused by a 5-hour limit that resets that many
  seconds later for `limit <seconds>`, runs until stopped for `slow`, runs a
  command its turn waits on for `long: <command>`, until it is sent to the background or
  stopped, renames the session as another Swarmed would for `elsewhere: <title>`, saying
  only `tree_changed`, checks a page in a browser for `screenshot`, whose last call returns
  an image, calls an MCP tool whose server attached an app for `app` (`mockApps.ts`: a chart
  that asks to connect first, refreshes through a tool call, asks to send a message, leaves a
  context and asks to open a link), and for `browse` opens the task's browser with `show_browser` and types and clicks
  in a tab of its own there, which the pane goes to; one sample session has 2,000 messages for
  checking scroll speed.
  The pull request of `fix-login-redirect` has a check running that fails 15 seconds after the
  window opens, as the core's follower would say it (`pull_read`, `pull_changed`). Its
  required check holds a merge back (`mockMerging.ts`): Force merge is refused in GitHub's
  words, auto-merge can be turned on, and Bypass requirements merges it.
  Removing the sample task `invoice-export` is refused over unsaved work, and `shop-142` over
  a session another Swarmed runs. The second session of `invoice-export` is idle with a
  sub-agent at work and a dev server left running; stopping them from the Background list
  changes its mark. A session can be taken back with Edit from here once idle
  (`mockRewind.ts`); a busy one is refused, and so are the files of one whose task has a session
  at work, as in `fix-login-redirect`. A new task's setup is quick in storefront, takes half a
  minute in bonsai-lint, and in fsum is killed the first time and succeeds when run again
  (`mockSetups.ts`). Storefront and bonsai-lint have GitHub issues, `shop-142` was started for
  #142, #428's text holds HTML GitHub draws (`<details>`, `<kbd>`) and HTML it does not, an
  HTML comment, an image and a view's block, and one of its comments is hidden on GitHub, and
  fsum is not on GitHub, so its list says why (`mockIssues.ts`). bonsai-lint's `rule-engine`
  holds an architect with sessions working for it, three levels deep, and `rule-docs` a
  planner that works for it from another task (`mockParents.ts`), each with a role
  (`mockRoles.ts`). The task whose stack runs has
  its browser running too (`mockBrowser.ts`), drawn on a canvas as a made-up shop that shows
  clicks and typing, and whose parts Pick outlines and picks; an address with `console` logs a line, `alert` opens a dialog, `refused`
  fails, and a browser program with `missing` in it shows the offer to install one. With
  `browser_display` set to window, a browser started from then on shows the window panel, and
  one already running offers to start again in a window. The README's
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
  A question can be skipped on its own (`skip`, the S key): it counts as settled, choosing
  or typing an answer un-skips it, and it reaches the agent under its text as `SKIPPED_ONE`,
  never left out, which could read as no answer. Every question skipped is Skip all, which
  denies the call with `SKIPPED`. Both say not to choose for the user: an agent that hears
  no answer otherwise picks one. A command's questions (`/config`) leave a skipped one out
  of the answers (`commandAnswersOf`), and the core leaves that setting as it is.
- **A sub-agent is read from its session's events, never a session of its own**
  (`AgentEvent::Sub`, `subagents.ts`, `SubAgentPane.tsx`). Claude Code names, on each message
  of an agent it started, the tool call that started it; `claude.rs` wraps what such a message
  holds in `Sub` under that call, so it is stored with the session and stays out of the
  conversation, which is between the user and the session's agent. A sub-agent is at work
  from its `Agent` (earlier `Task`) call to that call's result, and the UI works that out
  from the events it hears (`atWork`, `working`): the sidebar lists the ones at work under
  their session and drops them when they are done, and pressing one shows its own
  conversation, read-only, in place of the session's. One in the background (Claude Code's
  default now) gets its call's result at once and works on past the turn: its
  `AgentEvent::Background` report names the call (`tool_use_id`), which keeps it at work,
  listed under the session even while it is idle, until the report of its end or the
  agent's exit, in whichever order the result and the report came. One sent there while its
  call waited on it is reported the same way when it moves. Its answer is that end
  report's summary, and its row in the Background list opens it. Nothing in the store or the database knows of sub-agents. Conversations
  recorded before this have a sub-agent's words among the agent's own.
- **ACP agents' sub-agents are shown as Claude Code's are** (`agent/acp/children.rs`). A
  sub-agent there is a session of its own, which `acp.rs` asks for at `initialize`: the
  protocol's unstable `subagents` (Codex sends `subagent_update` on the session, then the
  child's updates and permission requests under the child's `sessionId`) and OpenCode's
  `_meta["opencode/child-session-updates"]` (`opencode/session/child_update` notifications;
  without it, a child's updates come on the session's own stream marked with
  `_meta["opencode/child-session"]`). Whatever a child sends is `Sub` under the call that
  shows its run, never the agent's own: its words, its tool calls, and its usage, which is
  not the session's. Each run is reported as `local_agent` work in the background, from the
  agent's word that it runs to its word that it is idle or done, so the UI follows it as it
  does a Claude Code sub-agent in the background, and its last words are its answer. OpenCode's
  `subagent` (earlier `task`) call is that run's call, shown as `Agent` with its `agent` as
  `subagent_type`; Codex hides its call, so one is made up (`Agent`, the child's session id,
  then `~2` and on for later runs) and returns at once. A run Codex says started is shown
  once what it was asked is known (`session_message`), or once it does something. A child's
  permission request is the session's own, with `description` naming the child. Stopping one
  cancels its session (Codex); OpenCode has no way, so its runs are `unstoppable`. Antigravity
  reports nothing of the kind, so its sub-agents are plain tool calls.
- **A session's title** starts as the first line of its first message. A `Namer` (`ClaudeNamer`:
  one call to Claude Code's smallest model, with none of the user's settings, tools, MCP
  servers, skills or commands) then suggests a better one, unless `auto_title` is off or the
  user renamed the session first. Every change of title is an event, `Renamed`, so all front
  ends follow. A message that calls a skill, `/name …`, is named as the request it stands
  for: the namer is given the skill's description and what follows it, never the text as
  typed, which Claude Code would run (`naming_prompt`). The description is given only when
  one skill answers to the name, as the agent would pick it (`skill_description`): several
  skills of a name that say different things, or a name Claude Code has a command by, are
  given by name and arguments alone. An answer that is no title, such as
  the `<invoke_…>` tag a model without tools gives, is dropped (`title_of`).
- **A new task is named before it exists** (`naming.rs`): the same `Namer` is asked, with a
  short timeout, and its answer is both the first session's title and, as lower-case words
  joined by dashes, the task's name. With no answer the front end names the task after the
  prompt. A task is never renamed later: its folder and branch are where the agent works.
  The core has the last word on a name it makes up (`NewTaskOptions::free_name`, which the app
  sets): it steps on to `name-2` and so on past any name a task of the project ever had, a
  folder in the way, a branch here or on a remote (one `for-each-ref` after the fetch), and a
  name another task is being created under, and the task it returns has the name taken. A name
  is claimed (`names_claimed`) from when it is chosen until its task is written down, so tasks
  started at once with the same prompt, each named alike, get `name`, `name-2` and so on
  instead of all but one failing in git. A name the user chose, as the command line takes it,
  is refused instead (`BranchExists`, `TaskFolderExists`, `TaskExists`). Removing a task
  deletes its local branch (`delete_branch`, which the app sets) unless the task started from
  it, a task that is still there is based on it (`based_on`: one stacked on it with `from`
  reads its changes against it), or commits on it are on neither the base nor a remote; a branch git will not check or
  delete is kept and noted, since the task is gone by then. A remote branch is never touched.
  What a task's sessions left (waiting messages, unread views, private files) is deleted only
  once the removal has gone through (`sessions_forgotten`), never by one that was refused.
- **A new task's branch starts with the user's branch prefix** (`prefix.rs`,
  `Settings::branch_prefix`): the default `branch_pattern` is `{{prefix}}/{{task}}`, and
  `{{prefix}}` is the prefix without the slashes at its ends, or the login (`{{user}}`) when
  it is empty. It belongs to the pattern alone; hooks have `{{branch}}`. `set_settings` checks
  a prefix that changed with git's own rules (`git check-ref-format --branch`, on
  `<prefix>/fix-login`) and refuses it with git's reason; one already kept is not checked
  again, so one written by hand cannot stop every other setting from being saved.
  `branch_example` shows the branch a prefix gives, for the settings to preview. A repository
  config whose pattern is `{{user}}/{{task}}`, which the template wrote before, is read as the
  default; any other is the user's and wins. The template leaves the pattern out, since an
  older Swarmed on the same data directory knows no `{{prefix}}`.
- **A session is unread** (`Session::unread`) from when its agent ends a turn, or stops with an
  error, until a front end calls `session_mark_read` or the user writes to it. It is a flag
  beside the status, not a status: the UI shows it for idle sessions only, since busy, waiting
  and failed already say more. The UI marks the session on screen as read when its answer
  arrives, if the window has focus, and otherwise when the window gets it. The core decides,
  and says so with every event (`SessionEvent::unread`, like `status`), which the UI
  follows. A turn that ends while the agent has sub-agents at work in the background
  (`local_agent`, `remote_agent`, followed in `Live` from the `Background` reports and
  forgotten when the agent exits or starts again) hands nothing back yet: the session goes
  idle, since it can take a message, but is not unread and gets no "Finished" notice. The
  turn the agent starts to read their reports does, and so does the answer to a later
  message of the user's (`Live::answering`): only the turn that started them and turns of
  the agent's own are held back. A command left running holds nothing up, and what a
  sub-agent says never makes its session busy.
- **MCP servers can be set up in Swarmed** (`mcp_own.rs`), in `mcp.toml` in the data
  directory, and are given to every agent it runs, Claude Code included, beside Claude Code's
  own. A server's secrets are named in the file and kept in the keychain under
  `mcp:<server>:<name>`; a front end can set one and never read it. Claude Code's servers are
  shown from its configuration and from what it last reported, and never written.
- **What agents leave running** (`background.rs`) is read back from the sessions' histories:
  Claude Code reports a `task_started` and a `task_notification` for each background command
  or agent, which become `AgentEvent::Background`. Stopping one is the `stop_task` control
  request. What ran under an agent that is no longer running is listed as stopped, and so is
  what an earlier run of the agent left once that run's `Exited` is recorded: the agent
  started again knows nothing of it. One run by another Swarmed (`agents_alive`) runs.
  - **Each session counts what runs** (`BackgroundCount`, #420): sub-agents and commands
    apart, kept on its row (`background_agents`, `background_commands`) and worked out again
    from its history whenever a `Background` or an `Exited` is stored (`background_after`),
    here or by a Swarmed that only passes it on, in case the one that stored it did not know
    the count. It rides on those events (`SessionEvent::background`). It is a flag beside
    the status, never one: the session stays idle. The UI rings an idle mark with it
    (`backgroundShown`), and folded headings add it up, ranked below working. A Swarmed
    that ends takes its agents along and stores no `Exited`, as the command line does after
    each turn: the house's look records one for each session that still counts something
    and whose agent runs nowhere (`background_settle_gone`), and publishes it.
  - **OpenCode's shells** (`agent/acp/shells.rs`): a call that ends with
    `rawOutput.metadata.status: "running"` and a `shellID` left one running, reported as
    `local_bash`. Nothing over ACP tells its end, so every shell reported running is taken to
    have ended when the agent next takes something up by itself (what a shell's end wakes it
    for), or when it goes. Like Claude Code's, a command left running holds nothing up.
  - **What cannot be stopped is marked** (`unstoppable` on the report, `stoppable` on
    `BackgroundTask`): `session_stop_background` refuses it (`Error::Unstoppable`) and the
    Background list offers no Stop. Reading output stays Claude Code's: ACP agents name no file.
  - **Codex's work left running is not followed.** codex-acp reports it only to a client that
    declares JetBrains' AIR, which changes the shape of everything else it sends; it is not
    declared. ACP v2's `state_update.background` and `terminal/cancel` are the way, once
    codex-acp sends them to a client like Swarmed.
- **What a command printed beside its call is its result** (`agent/acp/printed.rs`). Codex
  streams a command's output in `_meta.terminal_output_delta` and ends the call with no
  content; the pieces are kept by call (the latest 64 KB) and stand in for content that is
  missing, before `rawOutput`.
- **Work a call waits on is not work left running** (`claude.rs`, `Pending`). Claude Code
  reports a `Bash` command or sub-agent its call waits on with `task_started` and
  `is_backgrounded: false`; `claude.rs` keeps that report to itself, says nothing of its end
  there, and tells it as `Background` (with `moved`) only when a `task_updated` moves it to
  the background. Such work can be sent there, as Ctrl+B does in Claude Code's terminal, by
  `session_send_to_background` (the `background_tasks` control request, with the call's
  `tool_use_id` or for every call): the call returns, the turn goes on, and the work is
  listed until it ends. Claude Code's answer arrives as `AgentEvent::SentToBackground`, which
  the core hands to whoever asked (`Live::await_sent`) and neither stores nor publishes; only
  Claude Code can (`AgentInfo::sends_to_background`), and the core refuses for the others.
  The transcript offers it on a running `Bash`, `Agent` or `Task` call of such an agent while
  it works (`mayGoToBackground`), each call listed in an opened run of folded calls
  included, and says quietly when it did not go; `bg` typed during
  `swarmed session send` does the same for every call. The command line reads what is typed
  on a thread of its own (`Typed`), never with Tokio's stdin: a read of a terminal cannot be
  given up, and the runtime would wait for it before the command could end. A stack's
  `start` command is spawned with `std::process`, never Tokio's: window commands that are not
  `async` run on a thread without a runtime, where a Tokio spawn aborts the app.
- **What a background command printed can be read** (`session_background_output`), from the
  file Claude Code writes it to: named by `output_file` in its end report, and for one still
  running by the `Bash` call's result ("Output is being written to: …", read in `claude.rs`
  as one more running report; other wording only means no output until it ends). Only a file
  the session's own agent named is read, and only once it is, links followed, a regular
  `.output` file under `/tmp/claude-<uid>` (the owner of the data directory). At most the last
  256 KB, or what came since an offset, so a view that follows it adds and never redraws. It
  can hold anything, like terminal output: shown to the user, never logged, stored or handed
  to another session, a fork, the cloud or search. The UI draws it in a read-only xterm built
  as the session's terminal is, and reads on only while it is open and the command runs,
  one read at a time. Nothing in it writes the clipboard (`built`'s `shows`): OSC 52 is for
  programs the user runs, not for what an agent's command printed.
- **A snapshot is the files git ignores in one well set up task** (`snapshots.rs`), one per
  environment. Each top-level path is cloned whole with one `fclonefileat` (`clone.rs`, through
  `rustix`, since unsafe code is forbidden here): a 275,000-file `node_modules` in 6 s against
  48 s for `cp -c`, which clones file by file and is what a path falls back to where a whole
  clone cannot be made. Starting from a snapshot, and the first one Swarmed takes itself, say
  each step in the setup output, which the session's setup panel shows as it goes; one taken
  by hand (`snapshot_save`) says nothing. A new task gets them put into its
  worktree before `setup` runs, never over a file that is already there. They may include the
  likes of `.env`: the folder is the user's alone and its contents are never logged. The first
  task set up well in an environment without one is kept as its snapshot (`snapshot_first`,
  from `run_setup`), before its stack starts writing logs and caches beside what setup made.
  A task whose setup is run again (`task_setup`) is kept only if it was never set up well
  before: one that was has been lived in.
  One build per environment at a time: `building_lock` holds a file lock beside the folder, so
  parallel first setups, and the app beside the `swarmed` command, do not build in one place.
  Forgetting a snapshot takes the same lock and is refused while one is built, which would
  put it back; with the lock, it also deletes what a build that was cut short left beside it.
- **A repository's default environment is kept in the database** (`default_environment` on
  `projects`, `Core::environment_default`), not in its config or environments file: a
  Swarmed sharing the data directory that does not know the key would refuse the file, as
  0.3.0 does, or name it as a problem every time it starts, while a column it does not know
  it never sees. A new task is asked for with an `EnvironmentChoice` (the default, the
  repository's own setup, or one by name) and the core settles the default as the task is
  made, so the window and the command line get the same. A default whose environment is gone
  is none; removing that environment clears it. Agents cannot set it: `configure` has no way
  to.
- **Prompts the user keeps** (`templates.rs`) are a name and a text in `templates.toml` in the
  data directory. A front end puts one into the message box; the core never sends one by
  itself, and agents are not told about them. `{{something}}` marks a place to fill in.
- **Attaching a file is naming its path** (`attach.rs`). Nothing is uploaded or copied: the
  agent runs on this machine and reads the file where it is. `with_files` lists the paths
  under the message in one fixed form, which the UI reads back (`splitFiles`) to show them as
  files. The front end gets paths from the system's file picker and from files dropped on the
  window, never file contents.
- **A message can mention another session** (`mentions.rs`, `ui/src/mentions.ts`, #380). `@`
  in the message box opens a list read from the core (`session_reachable`, or
  `project_reachable` in the new-task view), which is what `read_session` can read: the reach
  rules stay in `peers.rs` alone (`Reach`, `in_reach`). The choice stands in the box as a token,
  `[@42 Fix the login form]`, which stays in the text as typed, so the draft, the queue and recall
  carry it as text. The front end sends the ids its tokens name beside the files (`Attached`);
  the core refuses one out of reach, closed or the session itself (`Error::CannotMention`)
  before anything is sent, and lists the rest under the words and before the files in a fixed
  form (`MENTIONED_SESSIONS`), which `splitMentions` reads back for chips and `mentionsAgain`
  for tokens. Only a reference is sent; with no tool server (`tool_server` unset) a short
  digest of each takes its place. A waiting message is edited with its files and sessions
  apart (`session_queue_edit_message`), so the core writes the list again. The pattern (a menu
  opened by a sign, a token, a list under the message, read back) is meant to be reused, and
  `#` reuses it: `mentionQuery` takes the sign, `usePicking` the list's keys, and only the
  list of the sign nearer the caret opens (`signAt`).
- **A message can mention a pull request or an issue** (`github_mentions.rs`,
  `ui/src/githubMentions.ts`, #371), by `#` in a repository's message box. They share
  GitHub's one count, so a token is `[#142 Fix the login form]` whichever it is.
  - **Listed** by `project_github_items` through the user's `gh` in the repository: the
    latest changed of each kind (`GITHUB_LISTED`), one by number (`gh pr view`, else
    `gh issue view`, which also answers for a pull request but without its branches), or
    GitHub's search for words. No `gh`, not on GitHub or signed out is an empty
    list that says why, as `issues.rs` does; issues turned off still lists the pull requests.
    The UI asks once per opening, filters as the user types (`githubMatches`), and asks again
    with the query only when that finds nothing, after a pause. Chats have none: `repo()`.
  - **Sending asks the network nothing.** What was read is kept in `Live::github` (`Known`,
    by address, with the repositories that asked); `Attached::github` names numbers, and
    `message_for` lists them from that, after the sessions and before the files, refusing a
    number not read (`Error::CannotMentionOnGitHub`). A front end calls
    `project_github_read` first (`githubRead` in `sending.ts`), which reads only what is not
    from the last minute; the command line's `--pull` and `--issue` do the same.
  - **A waiting message goes with the state as it is when sent** (the user's choice). While
    one with a list waits, one follower (`github_follow_waiting`, started by whatever queues
    such a message) reads each address it names again every `FRESH_EVERY` until none waits;
    `send_next` and `session_tell_now` write the list again from that (`fresh_list`) as the
    message goes. The queue's own text is never changed by it.
  - **Read back** by `splitGitHub` (the list must run to the end of what is left once the files
    are off), as chips coloured by state that open the address with `open_link`, and as tokens
    again by `typedAgain`, which handles both lists. A number typed by hand is never looked up.
- **A pasted image is the one part of a message Swarmed keeps** (`pasted.rs`, `images.ts`,
  #378), since it has no path to name. Until the message goes it is in the draft, in the
  window's memory (`images` of the `Draft`, in base64). Sending, queueing, saving an edited
  waiting message or starting a task hands it to the core (`session_images_keep`,
  `FirstSession::images`), which checks it by its first bytes (PNG, JPEG, GIF or WebP, at most
  `MAX_IMAGE_MB`; anything else is refused with what it is, never converted: there is no image
  library) and writes it into `attachments/<session>/` in the data directory, `0700` with files
  `0600`, under a name of its own (`pasted-<n>.<ext>`, never over a file). Its path is then
  attached like any file, so every agent reads it with its own tools; no agent is sent image
  content, and `UserMessage` stays text. A message's files in that folder show as images
  (`isPastedImage`), read through `session_image`, which reads only a file right in the
  session's folder, links followed, and answers a `data:` address, the only kind the window's
  policy loads. The folder goes with the session for good (`forget_session_files`,
  `session_delete`); closing keeps it. A fork copies the images its messages name into its own
  folder and has them name the copies (`images_forked`), so deleting the first session costs it
  nothing.
- **An image a tool returns is kept beside the pasted ones** (`returned.rs`, `ToolImages.tsx`,
  #428). A backend reads each image of a tool result (`Taking`: Claude Code's
  `source.base64`, ACP's `image` content) into `ToolResult::returned`, which is never stored or
  published and whose `Debug` names no bytes; checked as a pasted one is, at most
  `MAX_RETURNED_MB` and `MAX_PER_RESULT`, with `[image]` in `content` where it was, or why not.
  The core writes them into the session's `attachments/` folder as `tool-<call>-<n>.<ext>`
  before the event is stored (`returned::kept`, from `take` in `sessions.rs`) and lists the
  paths in `images`. The window reads them through `session_image` and holds only smaller
  copies (`keptThumbnail`); the turn's last one also stands at the end of its reply
  (`last-image`), and hidden tool calls hide them. Shown to the user only, like a pasted one.
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
- **MCP servers' apps are shown under their calls** (MCP Apps, #385: `apps.rs`,
  `agent/claude/apps.rs`, `mcpApps.ts`, `McpAppView.tsx`). An app is a third party's HTML, the
  server author's, and gets every rule a view has and more.
  - **Read through Claude Code only**, as it offers its host (tried with 2.1.332): only a
    Claude Code whose init message says `mcp_read_resource_v1` and `mcp_tool_ui_meta_v1` is
    asked. `mcp_status` is asked when it starts and when the servers it lists change, and of
    its answer only names and `_meta.ui` are read (`tools_in`), never a server's definition.
    A call of a tool with a `ui://` resource that ends without an error has the resource read
    (`mcp_read_resource`), and the app is reported as `AgentEvent::App` with the call's whole
    result (`structuredContent` from the message's `tool_use_result`), cut at
    `MAX_RESULT_BYTES`. A sub-agent's app is its own (`Sub`). Other agents report nothing of
    the kind.
  - **Kept once per session**: `apps::kept` writes the HTML into the session's attachments as
    `app-<hash>.html`, never over another file, at most `MAX_APP_MB`, and the event names the
    file; `html` is never stored or published. Forks, peers, the cloud and search pick events
    by kind and never carry one.
  - **Served from a host of its own** (`swarmed-view://app-<id>/`, `view_response` routes by
    host) under the policy `app_policy` builds from the declared `csp`: an `https:` origin
    (`wss:` too for connecting) by name, with at most a `*.` subdomain of a domain of two
    labels or more, no IP address, no path, at most `MAX_DOMAINS` of a kind; anything else is
    refused and named. `frame-src` is always `'none'`: the
    window's navigation guard is asked about every frame, so a nested page could not load
    without loosening it for the window. Views keep `VIEW_POLICY` unchanged. The frame is
    `allow-scripts` alone (the user's choice): no origin to keep storage in.
  - **The user says yes before an app that reaches anything loads** (`Consent`), and the yes
    holds per server and the addresses it reaches until Swarmed quits (`appsAllowed`, in
    memory, the user's choice). No device is granted.
  - **The bridge decides in `mcpApps.ts`**, pure and tested; `McpAppView.tsx` only acts.
    Messages are taken only from the app's own frame. The handshake and sizing go by
    themselves; the call's input and result are sent after `initialized`, again if the app
    loads anew. A link and `ui/message` wait for the user each time, and a message starting
    with `/` is refused: an app sends words, never a command or a skill.
    `ui/update-model-context` is kept per session (`appContexts`) as a chip and goes with the
    next typed message, never a command, in `<app_context>` tags the transcript folds and
    recall leaves out; a waiting message being edited keeps them. A tag of that name inside an
    app's text is renamed (`inBlock`), or the app could close its block and pass what follows
    off as the user's words. The transcript draws its
    items again with each event, so the bridge keeps the item in a ref and listens once per
    call: a listener tied to the item would tear the app down at every event.
  - **An app's tool calls go through the agent** (`Core::session_app_call`): only a tool of
    the app's own server in the event's `callable` (its visibility leaves `app` in), sent as
    `mcp_call` with `check_permissions` and `result_not_for_model` (the user's choice), so
    Claude Code decides it as the agent's own call of that tool and its permission request
    arrives as any other, marked as the app's (`APP_ASKS`). A Claude Code that does not say
    `mcp_call_check_permissions_v1` would run the tool unchecked, so it is not sent one: the
    writing side answers the call at once (`CANNOT_CALL`) instead. The
    answer is `AgentEvent::AppCalled`, handed to whoever waits (`AppCalls`) and never stored
    or published. While a call waits the app is told it still runs (`notifications/progress`),
    so the user's decision outlasts its own timeout.
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
  appearance (with `limit_warnings`, which levels the usage notice at the top of the window
  says: a limit that stops requests is said whatever it is; and `turn_end_lines`, whether
  turns end with a line: one that failed or was stopped keeps it whatever it is). Only
  proposed: rules, environments, an action's command, the status line's script, hiding tool
  calls (`tool_calls` to `hidden`; all and folded are appearance like the rest). These
  outlive the session, so an agent that read something hostile could plant one;
  the change is shown in the conversation (`ConfigProposed`) and made when the user accepts it
  there (`Core::config_answer`), whatever the agent's own permission mode. Not reachable at
  all: API keys, MCP servers, sign-ins, supervision, notes, the branch prefix (set once by the
  user, and not worth a line in a description every agent is sent). Keep it one tool: every tool's
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
- **A highlight is the user's colour on a task or a session, where it is** (`highlights.rs`,
  `highlights.ts`, `Highlight.tsx`, #398). A column of its own on both (`highlight`, a
  `MarkColour` name), set by `session_highlight` and `task_highlight` (by a colour's word
  through `session_highlight_named` and `task_highlight_named`, which is all a front end
  calls) and published as `TreeChanged`, never stored as an event. Nothing but the user clears it: writing, closing and
  reopening keep it, a fork starts without one, and neither `set_mark` nor `configure` reaches
  it. It is drawn as a bar at the row's right edge (an `::after`, since styles draw the open
  row's edge with a shadow, and the left edge is the waiting bar), never a tint, so the row's
  other states keep their look. Marks and highlights read their colour from one name each,
  `var(--green, var(--moss))` and so on, on the element: the custom layout's `green` and the
  others change both. The palette's open row is in the store (`highlighting`), which the
  shortcut sets and which unfolds what hides the row (`foldsShowing`).
- **Swarmed serves tools of its own to every agent** (`toolserver.rs`): `show_html`, on an
  MCP server named `swarmed`. A front end's executable is that server when started with
  `tool-server`, and tells the core so with `set_tool_server`. The tool does nothing: its call
  is in the conversation, where the UI shows the HTML like an `html-view` block. It is there
  because agents asked for something visual look for a tool. `show_browser` beside it is there
  for the same reason: an agent asked to look at a page should find the task's browser, not
  launch one. The core answers it, as it does the session tools.
- **A view reaches the conversation from the tool, not from the call.** The tool server
  leaves the HTML in the session's own folder (`Paths::view_spool`, named to it by an
  environment variable), and the core records it as a `View` event when the next tool result or
  turn end arrives. Agents wrap their calls differently: directly, from a script, from a
  subagent. None of that matters this way. Reading the HTML out of the call's own shape is only
  a fallback for conversations recorded before.
- **Sessions can see, read, message and start each other** (`peers.rs`), through tools
  of Swarmed's own server. The server is a process the agent starts, so it asks the core over
  a socket in the system's temporary folder (`Paths::tools_socket`; short, since a socket's
  path is limited), which a front end serves with `serve_session_tools`. A message arrives as
  a message in the other conversation, queued if that session is busy. Only open sessions of
  the caller's own project can be read or written to (`peer_in_reach`), and from a chat those
  of every project (see the chats above): a closed one would be started out of sight. A spawned session runs
  in its parent's task with its settings, cannot spawn itself (`Session::spawned`) unless its
  role starts sessions (see roles below). While `MAX_SPAWNED` open sessions work for a
  session, however each came to, its agent starts no more, with `spawn_session` or
  `start_task` (`room_for_another`); the user can still have more work for it. The tools are
  not on the allow list: in ask mode the user is asked. A session can close only sessions it spawned, and only idle ones
  (`peer_close`).
- **Who works for whom is a tree of parents** (`parents.rs`, #459). `Session::parent`, kept
  in the `spawned_by` column, is whom a session works for: set by `spawn_session`,
  `start_task` and `swarmed session run` (`--parent`, or `SWARMED_SESSION`, which agents of a
  task are given), or by the user (`session_set_parent`, the header's menu,
  `swarmed session parent`). It can be any open session of the same repository, in any task,
  never itself, a closed one or one below it (`Error::CannotWorkFor`). `Session::spawned`
  says apart that an agent started it in its own worktree, which is what keeps it from
  spawning, keeps its first message out of recall and its turns out of notices; the user's
  choice never changes it.
  - **A change** is `AgentEvent::ParentChanged`, recorded in the child and in its old and new
    parent, and `TreeChanged`. Each of their agents is told who works with it now with its
    next message (`parents::regrouped`, read from the history, so a change another process
    made counts too), and every agent at its start in its guidance.
  - **A session an agent starts** gets its parent from its start and records nothing
    (`session_parent_from_start`, and so `spawn_session` and `start_task`): its starter
    knows, and its own agent's guidance says so. One started from an agent's shell
    (`SWARMED_SESSION`) is that agent's doing, so it works for it only where `spawn_session`
    would have been allowed (`may_start`, `room_for_another`); refused, the session is made
    all the same and works for nobody. `--parent` is the user's word and is checked as the
    user's choice is.
  - **Shown:** the sidebar nests to any depth (`nestedSessions`, `parentHere`; `order.rs`
    counts the same siblings), and one whose parent is elsewhere shows `for #N`
    (`parentElsewhere`). `list_sessions` and `read_session` say it. Deleting a parent leaves
    its children with none; closing leaves them as they are.
- **A session's role says what it is for** (`roles.rs`, `roles.ts`, `Role.tsx`, #459). A role
  is data (`Role`: name, one-line description, instructions, starting supervision, whether it
  changes files, which roles it may start, its default child), so that roles of the user's own
  can come; four are built in (`ROLES`: architect, planner, implementer, reviewer), and no
  role is how every session worked before. `Session::role` is the name, in the `role` column;
  one no built-in has (a newer Swarmed's) counts as none where rules apply.
  - **Chosen at start** (`SessionOptions::role`: the new-task view, `--role`, a fork keeps it)
    and **changed** by the user only (`session_set_role`, the header's menu,
    `swarmed session role`), never through `configure`. Before anything was said nothing is
    recorded: the agent starts with it. After, `AgentEvent::RoleChanged`, shown in the
    transcript, and its agent is told with the next message (`roles::changed` against what it
    last heard, `roles::note`).
  - **Told:** `roles::section` is one part of the guidance: the role's rules, every role with
    the architect and planner table (`TABLE`), and who works with it (folding in part of
    `parents.rs`); with no role, only who works with it. `docs/coding-agents.md` holds the
    same roles and table, and a test keeps the two together: change both.
  - **What it may start** (`Core::may_start`): `spawn_session` and `start_task` take a
    `role`, default to the caller's role's child, and refuse one it may not start
    (`roles::of_child`); a role that starts nothing starts no session, and a session with no
    role starts any. A role that starts sessions may even when another session spawned it,
    while fewer than `MAX_DEPTH` (3) sessions with a role work one for another in a row up from
    it (`roles_in_a_row`); a session with no role breaks the row, so chains of tasks are not
    capped. A spawned session with no role still starts nothing (`Session::spawned`).
  - **Kept to, where Swarmed can** (#459 part 3): a role that changes no files holds the
    session in `Plan` (`Role::supervision`), which `acp.rs` maps onto an agent's plan or
    read-only mode, else its asking one. At start (`Core::session_insert`, `roles::starts_in`)
    and on a change (`roles::held`), what the role took the place of is kept in
    `supervision_before_role` and put back when the role goes; the user's own change
    (`session_configure`, not `supervise`, which the core uses) forgets it, so their choice
    wins. A session a role-holder starts begins from that kept supervision
    (`supervision_without_role`), so an implementer is never held in its planner's plan mode.
    A same-agent fork copies it; a clone onto another agent starts anew under the role.
  - **Approving its plan keeps it planning:** for a role that changes no files, `decide`
    answers `PLAN_TOOL` with a refusal whose reason is `roles::approval` (the role's
    `approved`: how it hands the plan on), recorded as allowed, and the session stays in
    `Plan`. Allowing would end Claude Code's plan mode with its own "You can now start coding".
    Tried with Claude Code 2.1.332: in plan mode it asks before an MCP tool and runs it once
    allowed, and the model calls one only when told it may, which the guidance
    (`KEPT_FROM_CHANGES`) does; an edit in plan mode is asked about, not refused.
  - **Shown:** a short chip on its sidebar row, gone when the row has no room for it (a
    container query), and the menu in the header and the new-task view. `list_sessions`,
    `read_session` and `swarmed session list` (`role <name>` last) say it.
- **A session starts a task for work that needs a branch of its own** (`start_task`,
  `peers/tasks.rs`). Without a tool for it, an agent asked for such work finds no way to make
  a task and switches its own worktree instead: `swarmed task new` is known only to an agent
  that has read this repository. It is the window's New task
  (`task_start`): the name gives way as a made-up one does, the setup runs in the process that
  serves the tools and shows in the setup panel, and the prompt goes once it finished well,
  so the session is the app's from the start. Its `branch` checks out an existing branch, a
  pull request's; its `from` starts a new branch from another (`NewTaskOptions::from`, a
  branch only, here or on a remote, which becomes the task's base and so its pull request's).
  The first message is the starter's words, marked as `send_message` marks them (`left_by`),
  with a note to report back; the session works for its starter but is not spawned
  (`Session::spawned` stays false), so it can spawn and start tasks itself, as the next link
  of a chain of pull requests does. A
  title left out is the prompt's, never the mark's, and the namer names it from the prompt.
  Refused to a spawned session and to a chat, and by a process that ends with its turn, as
  the command line's does, which would take the setup along: only one that stays answers it
  (`Core::set_stays`, which the app sets). `TreeChanged` is published, since front ends
  learn of a task their own process made only when told. Not on the allow list.
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
- **A session can be taken back to before one of its messages** (`rewind.rs`, `rewind.ts`,
  `RewindDialog.tsx`, #364): Edit from here, `/rewind`, `swarmed session rewind`.
  - **Nothing is deleted.** `AgentEvent::Rewound { to }` says that message `to` (numbered as a
    fork numbers them, every message ever sent counted) and what followed, up to the event, no
    longer count (`rewind::dropped`, `counted`); one inside a later one's part goes with it.
    Forks, carried text, a hand-over to the cloud and what another session reads of it
    (`said_in`) read `counted`; costs, search and the event list read everything.
    The transcript folds the part under the line (`RewoundItem`).
  - **Claude Code resumes** at its own id of the last message of the turn before
    (`--resume-session-at`, which takes a message's `uuid`). `claude.rs` reports it at each
    turn's end, before `TurnDone`, as `AgentEvent::ResumePoint` (stored, never shown), and a
    launch passes it (`rewind::resume_at`) until a turn after the rewind reports its own. With
    no such point after the message before, and for every other agent, `agent_session_id` is
    cleared and the agent is told what was said, as a fork's is (`rewind::starts_anew`).
  - **Files** go back with `checkpoints::restore`: the worktree taken into a scratch index,
    then `git read-tree -m -u <now> <checkpoint>`. Git refuses to overwrite an untracked file
    and changes nothing then, but writes over one it ignores without a word, so anything on
    disk where a file would be added that the tree taken has not got is refused first
    (`nothing_in_the_way`): it could not be put back. Git writes file by file under a limit of its own
    (`WRITE_LIMIT`), and one that gives up part-way has what it wrote put as it was
    (`left_at`); where that fails too, the tree taken is kept against the tree the files are
    left at (`Stopped::left`), so that they can still be put back. The tree taken is kept in `rewinds`, the
    session's latest only, and `session_files_put_back` restores it only while the worktree is
    still at the tree it was taken back to. Commits stay: `checkpoints.head` says how many
    were made since. Claude Code's own `rewind_files` is never used.
  - **Refused, not tried**, as `Error::Rewind` with the reason: the session busy, closed, run
    elsewhere or with work left running; for files also any session of the task busy or run
    elsewhere, anything left running, its setup, a rebase or merge under way, no checkpoint.
    While one runs, `Live::taking_back` holds every message to the task's agents
    (`send_text`), and the agent is stopped and waited for (`session_ended`) before anything is
    changed.
  - **The message goes back into the draft** (`rewindSession`, `draftOf`, `withEdited`), with
    what was typed kept after it, and is never sent by itself.
- **A closed session is put away, not deleted.** Closing is an event, `Closed`, so every front
  end drops it from its lists. Its task counts its closed sessions, they can be listed
  (`sessions_closed`) and put back (`session_reopen`) with their conversation as it was.
- **Agents other than Claude Code get Swarmed's guidance with their first message**, as a
  second block after the user's words: the protocol has no place for a client's instructions.
- A view still being written is held back and stood in for (`splitPendingView`), so it appears
  whole instead of as streaming source.
- **What is typed and not sent is a draft in the UI's store** (`drafts` per session,
  `newTaskDrafts` per repository, `Draft` in `sending.ts`), never in the component, which
  goes when another session is opened, and never on disk. A failed send puts it back into its
  own session's draft, wherever the user is by then. A draft goes with its session when that is
  closed or forgotten, and with its repository.
- **Where a transcript was left is kept by row, not by pixel** (`scrollSpot.ts`,
  `useScrollSpot.ts`, `scrolls` in the store). Rows out of view have only an estimated
  height, so a `scrollTop` would land elsewhere: the spot is the item at the top of the view
  (`data-key`) and how far down it the view was, put back once that row is measured, or
  following at the bottom. It is kept per session and per sub-agent when the transcript goes,
  in memory only, and forgotten with the session as its draft is. A move of the view the hook
  did not make (the user's, or the browser's own anchoring) is taken in before the view is put
  anywhere, even when rows grew in the same frame and its scroll event is still to come
  (`placeFor`): a spot left stale would put the view back. A scroll up stops following at
  once, or each piece of a reply would pull the view back down.
- **The message box handles a few keys itself** (`ui/src/editing.ts`): Home and End move within
  the line, and ⌘⌫ at the very start empties the box. ↑ at the very start and ↓ at the very end
  go through what was sent to the session (`ui/src/recall.ts`): its `user_message` events as
  typed, files apart, never a message another session left, nor the first message of a session
  another one spawned (`spawned`), which that session's agent wrote. Not on the first and
  last line: a long line the box wraps is several rows, and the arrows have to move between
  them. Where it stands is in the store (`recalls`), since it holds what was typed before ↑,
  and goes with the session as its draft does. The command list and a question that has the
  keyboard take the arrows first.
- **A large paste and a quote of a reply stand in the message box as tokens** (`pastes.ts`,
  `quotes.ts`, #379), all in the UI: the core sends the text as typed. What a token stands for
  is in the window's memory until its message goes, and the draft holds only the token. Sent, it
  gives way to its text in numbered tags (`<pasted_text_N>`, `<quoted_reply_N>`), which
  `messageParts` finds again for the transcript, and a message brought back (↑, or a waiting one
  opened to edit) has them as tokens again (`tokensAgain`). Only the agent's own replies in the
  session's transcript are quoted from (`data-quote-into`): not the user's messages, nor a
  sub-agent's pane, whose words are not in the agent's conversation.
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
  agent, which by itself would fall back to asking about everything. A session whose role
  changes no files stays in plan mode and is told to hand the plan on instead (see roles). Every change of
  supervision is an event, `SupervisionChanged`. Other agents have no plan mode and ask instead.
- **The model and effort actually in use** are asked of Claude Code (`get_settings`) when it
  starts and after each turn, and recorded as `Applied`. Only those two values are read: the
  rest of that answer is the user's whole configuration and is never kept or logged.
- **Usage is asked of Claude Code** (`get_usage`, which it marks experimental) when it starts,
  whenever a usage report says a turn ran on extra usage, and when the user asks: the coins
  in a session's header (`UsageButton`, `session_usage_refresh`, `AgentCommand::AskUsage`)
  or `/usage`. Every answer is read into `UsageLimits` (the plan's limits as `/usage` lists
  them) and `ExtraUsage` (what is spent beyond the plan this billing period, against the
  monthly cap); `/usage` also shows its screen. Claude Code reads them from claude.ai with
  its own sign-in. Only those figures are read, a shape it does not know is no answer, and
  `skip_behaviors` spares the scan of a week of local transcripts. The rest of the answer,
  the plan and what the account's sessions cost, is not kept, passed on or logged. Nothing
  asks for usage on its own, on a timer or when a figure is looked at, and
  `docs/coding-agents.md` says how fresh the figures are because of that. It was the user's
  choice, so that looking costs nothing. Keep it so. A UI timer that only draws again at a
  window's reset time asks nothing, and is how a warning goes when its window resets.
  Claude Code answers `get_usage` from its own copy for a minute after fetching it, shared by
  every Claude Code on the machine through its config file (`cachedUsageUtilization`), so
  `session_usage_refresh` holds the question back until that copy is a minute old
  (`usage_copy.rs`, `UsageAsk`), and the answer is fetched after the user asked. Only
  `fetchedAtMs` is read of that file for it. Asking shows at every step (`UsageOverview`,
  `askedLine`, `askedDetail`, `waitedShare`): a countdown and a bar while held back, a moving
  bar while asked, and the answer's time to the second with whether it changed the figures
  (`figuresKey`), since an answer the same as before otherwise looks like nothing happened.
  The figures last heard are never dimmed meanwhile (the user's choice): the line above them
  says they are refreshing and what they are until then.
- **Usage is the account's, and every Claude Code session shows the same**
  (`account_usage.rs`, `Core::account_usage`, `swarmed limits`). The core folds each
  `RateLimit`, `UsageLimits` and `ExtraUsage` a Claude Code session records into one
  `AccountUsage`: each window, by its kind (`session` is `five_hour`, `weekly_all` is
  `seven_day`, one model's weekly limit is `seven_day_<model>`), keeps the newest figure any
  report gave it, since the report after a turn leaves a model's limit out; an answer to
  asking lists every limit, so one an earlier answer gave that it does not is dropped. A
  report's `seven_day_overage_included` (`MODEL_WEEKLY`, "weekly model") is not extra usage
  but one model's weekly limit without its name: Claude Code shows it as that model's, and
  its figures follow the asked `weekly_scoped` one to the percent. It is folded into the one
  model's window known (`fold_model_weekly`, newer figure wins), stays apart while there is
  none or several, and like every model's limit refuses that model alone (`one_models`).
  Whether extra usage can pay past a limit reached (`ExtraPays`, `extra_can_pay`, #435) is the
  newest word on it: a report's `overageStatus` (`rejected` when it cannot, such as
  `out_of_credits`), else `overageEnabled`, or the asked extra usage (on, cap not reached);
  without any, a turn billed until its reset. An answer never raises a report's refusal
  (`hear_pays_asked`, `ExtraPays::asked`): its `is_enabled` is the setting, not known to turn
  off when credits run out, and Claude Code is asked at every start; a later report does. Billed and stopped follow it, not whether the
  latest turn was billed. A window is reached (`window_reached`, `limitLevel`) by a report's
  `rejected` or a report's utilization of 1 or more, never by an answer's percent alone,
  which is rounded; an answer at 100 keeps a report's `rejected` until the reset
  (`keep_refusal`). It is
  read lazily from the newest reports in the database, and published apart as
  `AgentEvent::AccountUsage` on session id 0, never stored, whenever it changes. There is one
  for all: `claude` cannot be redefined in `agents.toml` and runs on the user's own sign-in,
  so every session reports one account, and another agent's reports are not taken in. The UI
  keeps it in the store (`accountUsage`) and shows it only for Claude Code sessions; a window
  past its `resets_at` counts as reset, at 0%, until a report says otherwise
  (`accountWindows`), and `useAccountUsage` draws again at the next reset. Each figure says
  how old it is.
  Reports are folded by agent, not by sign-in: a session the command line runs with
  `--auth inherit` on another account's sign-in is taken as the same account.
- **A session a usage limit stopped can go on by itself when the limit resets**
  (`continuation.rs`, #374). A continuation is a time on the session (`continue_at`), kept in
  the database since a reset can be days away, and recorded as `AgentEvent::Continuation`.
  - **Set** by `session_continue_at_reset`, from the refusals (`RateLimit` reached, nothing
    paying) recorded since the session's latest message, at the latest of their resets; or
    by `Settings::continue_after_limit` as a refusal is heard (`after_refusal`, from
    `on_event`). A reset not later than now sets nothing, so a stale figure cannot loop.
    Agents cannot reach the setting through `configure`: it sends messages by itself.
  - **Cancelled** by any `UserMessage` (`Store::record` clears the column, and the UI's
    `applySessionEvent` mirrors it, as it does marks), Stop (`session_interrupt`), closing,
    removing the task, and `session_continue_cancel`.
  - **Sent** by the Swarmed that keeps house only (`continue_due`, which the app's
    `follow_resets` calls every second, pausing `APART` after each session it continued):
    taken off first, then `CONTINUE_MESSAGE` goes first in the queue as a `QueuedMessage` with
    `at_reset`, and is recorded as `UserMessage` with `at_reset`. One whose agent another
    Swarmed runs goes when that run is over (`queued_for`). If the account's usage as last
    heard says a limit still refuses requests, every due one waits for that reset instead;
    one model's weekly limit is not such a limit (`one_models`), since it stops that model
    alone. One is taken off, or put off, only if it is still due by then
    (`move_if_due`), so a cancel that came since the look is not undone. A waiting message
    the user changes is theirs: `Queues::edit` takes `at_reset` off it.
  - **Asks nothing:** the timer only sends the user's chosen message at a time a report
    already gave. Only Claude Code reports resets, so other agents' refusals offer nothing.
- **ACP agents with an effort of their own say what it stands at** when a conversation opens,
  which is recorded as `Applied` too. Before a session's agent has said, a front end shows
  what the agent's default last came to (`effort.rs`, `Core::effort_default`): the latest
  `Applied` of a session on the same agent and model that chose no effort.
- **Whatever sends into a session queues when its agent is busy** (`sendOrQueue` in the UI):
  a message sent straight in would clear a request the agent waits on. A request takes the
  keyboard only when nothing has it (`focus.ts`); the question of a command the user has just
  sent (`commandSent.ts`) also takes it from an empty message box, and one drawn again later
  does not. The answer shortcuts never allow a
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
  the first one when a turn ends well or the user stops it (`session_interrupt`), as if it had
  just been typed; a turn that failed, or an agent that was stopped, leaves the queue standing.
  The queue lives in memory, and its changes are published as `Queue` events that are never
  stored, like the pieces of a reply. An answer about a queue (the read when a session opens,
  the answer to an edit) can reach the window after an event published later, so the UI takes
  it through `queueAnswer` in the store, never `setQueue`: it is passed over if a queue event
  of the session was heard since it was asked for (`queuesHeard`, `queuesAnswered` in
  `queue.ts`).
- **A waiting message open for editing is held** (`session_queue_hold`), and the edit box
  renews the hold every few seconds. While the first message is held, nothing sends from the
  queue by itself, whatever the path (a turn's end, Stop, Send now, Send the next one): the
  send is owed (`Queues::owe`) and goes when the hold is let go, by saving
  (`session_queue_edit`, with the new text) or giving the edit up (`session_queue_let_go`,
  as it was). A hold nobody renews lapses after `HOLD_FOR`, and a task started when the send
  was put off (`wait_for_edit`) sends it then, so a window that went away mid-edit cannot
  leave the queue stuck. A held message further back holds nothing up.
- **A waiting message can be sent at once** (`session_send_now`): it is put first and the turn
  under way is stopped as Stop stops it, so it goes when that turn has ended. Never straight
  into a busy agent, which would take an open request away and is not something every agent
  can take. A stopped turn is marked in `Queues::pressing`, under the same hold of that lock
  as the interrupt goes out with, and the lock is held while a turn's end is dealt with
  (`end_turn`) and while a message is sent at once, so a stop that crosses the turn's own end
  sends one message, not two. It is taken before the message is put first, so a message that
  such an end has already sent is left alone, and so is its turn. What is typed can go the same way in one step
  (`session_message_now`, ⌘⇧Enter, `send-now` in the shortcut table): put first and sent
  now, never queued and then sent, between which a turn could end. The message box acts on
  that shortcut itself, where the text is; the window's listener leaves it alone.
- **Claude Code can be told a waiting message mid-turn** (`session_tell_now`, **Tell now** on
  the message), for an agent whose `AgentInfo::hears_mid_turn` says so: only Claude Code. As
  tried with 2.1.330, a `user` line written while a turn runs is taken into that turn at its
  next step (after the tool under way), answered in the same turn, and an open `can_use_tool`
  request stays open. Started with `--replay-user-messages`, Claude Code says each message
  back (`isReplay`) as it takes it in, which `claude.rs` reads as `AgentEvent::Heard`, never
  stored or published. A told message is recorded as the user's (`UserMessage`) when it is
  heard, so it stands where the agent heard it; every other message said back was recorded
  when it was sent and is dropped (`heard_as`). A skill Claude Code would not know by name
  is told as what it says, like any message that calls one (`Told`: what was sent is what is
  said back, what was typed is what is recorded). One told and never heard, because the agent
  ended or another run took its place, waits in the queue again (`wait_again`): recording it
  would show as said what the agent never got. ACP agents cannot add to a turn and refuse it
  (`Error::HearsNoMessageMidTurn`); between turns it is sent as any message.
- **What is worth interrupting the user for is decided in the core** (`notices.rs`): a session
  that asks for a decision, finishes a turn, fails, or moves its task's worktree to another
  branch, and a task's pull request that changed (`PullChanged`, named by task and number). A front end shows it only while the user
  looks elsewhere; the app does so as a macOS notification (`crates/app/src/notify.rs`) and
  counts what waits on its icon. A notice names the tool and the agent's own summary, never the
  tool's arguments, which may hold anything.
- **The app updates itself, and that is the one thing the app crate decides**
  (`crates/app/src/update.rs`, `tauri-plugin-updater`): the updater is the window's, and the
  core links no HTTP client. The setting is the core's (`Settings::updates`, `UpdateMode`).
  What it decides is in pure functions tested there (`step`, `progress_step`); add to those
  rather than to the code around them.
  - **Nothing is installed while Swarmed runs.** The checked archive waits in memory and goes
    in on `RunEvent::Exit` (`install_ready`): agents start Swarmed's own executable to reach
    its tools, and must not get a newer one than the app they talk to.
  - **Only a release updates itself** (`from_sources`): `scripts/build-release.sh` sets
    `SWARMED_RELEASE` for the build, and anything built without it, `pnpm app:build` and
    `pnpm app:install` included, looks only when About opens and installs nothing.
  - **Each look asks again**, and a newer release takes the place of the archive held. With
    `when_asked`, nothing is downloaded or installed unasked: an archive downloaded while
    updating was automatic is put away by the next look and passed over at quit.
  - **A version that could not be put in is remembered** with the reason in
    `run/update-refused`, shown with the version at the next launch and not downloaded again
    unasked. Asking for it tries again.
  - **Only an archive signed with the key built in is installed**, for the version it was
    signed as (`requireSignedVersion`). `build-release.sh` fails if the archive it built is
    signed with another key. The plugin's own commands are in no capability: the window
    reaches updating through `update_state`, `update_check` and `update_restart` alone.
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
- **What one turn changed is read from checkpoints** (`checkpoints.rs`, `checkpoints/trees.rs`,
  `turnChanges.ts`, #363). A checkpoint is the worktree as a git tree, kept in a bare repository
  of the task's own under the data directory (`Paths::checkpoints`, by task id), named by a row
  of `checkpoints` (session, message number as a fork counts them, `before` or `after`).
  - **Taken** in `send_text` once the message is recorded and before it goes (every path but a
    message told mid-turn, which joins the turn), and in `on_event` on `TurnDone` before the end
    is told, for the turn whose `before` this process took (`Checkpoints::under_way`). A failed
    one keeps nothing back: that turn is only not listed. Chats take none.
  - **Nothing is written into the repository.** Git runs on a copy of the worktree's index with
    `GIT_OBJECT_DIRECTORY` on the store and the repository's objects as an alternate; hooks,
    filters (Git LFS writes into `.git/lfs` when it cleans), the file-system monitor and a split
    index are switched off through `GIT_CONFIG_*`. A test holds the repository's `.git` to the
    byte. An untracked file over `MAX_UNTRACKED_BYTES` is left out by literal pathspec and said.
    Every git call has `LIMIT`, after which it is killed: a message never waits longer.
  - **Kept:** the latest `KEPT_TURNS` turns per session; older rows are dropped as a `before` is
    kept, and the store pruned on a thread of its own (`git prune` with the kept trees as heads,
    `GRACE` for objects a checkpoint being taken has not named yet). A task's removal deletes
    them; usage counts them, and a clean-up lists a store whose task is gone.
  - **Read:** a turn ends at its `after`, else the next turn's `before`, else now (a tree taken
    and not kept). Who else was at work is the task's other sessions whose turns overlap.
- **A task's pull request and checks are asked of the user's own `gh`** (`pulls.rs`), in the
  task's worktree. Swarmed holds no GitHub token. No `gh`, no pull request, or a
  repository that is not on GitHub all mean the same: the task has none, which is not an error.
  `gh pr view` reports every check run on the head commit, runs a newer one replaced included,
  so only each check's latest run is kept (`latest`, by workflow and job name), as GitHub's own
  page and `gh pr checks` show them. A run is replaced only by one that started after it
  finished, or when it was cancelled. What `gh pr view` prints has no event, so a job run for
  both `push` and `pull_request` on one commit, under way at the same time, shows twice:
  folded into one, a run that passed would hide one that failed.
- **The core follows the pull requests of tasks, and says what changed once** (`following.rs`,
  #366). The window never asks `gh` on a timer: badges and the panel show `pulls` in the store,
  filled from the tree (`TaskOverview::pull`) and from `AgentEvent::PullRead` (session id 0,
  never stored); opening the task panel asks once (`task_pull`).
  - **One list per repository per round** (`follow_pulls`, `Core::pulls_follow`), by the
    Swarmed that keeps house only: `gh pr list --author @me`, matched to tasks by the branch
    their worktree is on. A followed one off the list (merged, closed, someone else's) is read
    on its own; reviews and comments only when `updatedAt` moved. A task with no reading that
    is not on the list has its branch asked about once per process (`look_at_unlisted`): one
    merged, closed or someone else's before it was ever read would have no badge until its
    panel was opened. Paced by `RUNNING`, `OPEN`
    and `QUIET`; a repository with nothing open is listed at the first round and when a turn
    ends or a worktree moves in one of its tasks (`wake_on_turns`). A failing `gh` backs off.
  - **The last reading is kept with the task** (`tasks.pull`, JSON of `Seen`, fields only
    added) and compared with by the pure `step`: the first reading of a pull request takes
    stock and says nothing. A reading replaces only the one it was compared with
    (`task_pull_swap`), so two processes say a change once. Every way of reading
    (`task_pull`, `task_pull_look`, the follower) goes through `pull_take_in`.
  - **What changed is `AgentEvent::PullChanged`** with a `PullChange`, recorded in the open
    session the user wrote to last, else the latest (`session_for_news`), and worded by
    `notice_for`. A review or comment is told by who, never what: the bodies are never read
    into anything, and the user's own (their `gh api user` login, or `viewerDidAuthor`) are
    not news.
  - **Stops** once merged or closed (kept, so the badge says so), for a removed task, and when
    the worktree is on another branch (the reading is let go). On no branch it is left as is.
  - **Send the failure to the agent** (`task_pull_failure`) builds a message with the tail of
    each failed Actions job's log (`gh run view --log-failed`) in tags marked as output, which
    the UI puts into the message box (`draftFailure`). Nothing reaches an agent unasked.
- **A task's pull request is merged by the user's `gh`, at the head they saw** (`merging.rs`,
  #367, `MergeDialog.tsx`, `merging.ts`).
  - **`task_pull_merge`** reads the pull request again and refuses one that is not open, a
    draft, or whose head is not the one the front end passes; then `gh pr merge` with the
    method and `--match-head-commit`. Never `--delete-branch`: `gh` would delete the local
    branch and move the worktree off it. The reading after it goes through `pull_take_in`, so
    the merge is recorded once and no follower says it again.
  - **Ways** (`MergeWhen`): `now` is refused while a check failed and points to `force`, which
    leaves it to GitHub's rules; `bypass` is `--admin`, which the UI offers to admins only;
    `auto` is GitHub's own auto-merge, refused where `merge_state` says GitHub would merge it
    now, since `gh --auto` then merges at once. `task_pull_auto_merge_off` takes it back.
  - **What came of it** is read from the pull request after: merged, `queued` (still open after
    a merge that is not auto-merge: a merge queue took it), auto-merge on, or `asked` when it
    could not be read back. GitHub's refusals are given in its words (`gh_words`); the UI names
    `gh`'s `--auto` and `--admin` hints as its own controls (`mergeProblem`).
  - **Rules** (`project_merge_rules`): one GraphQL query per repository per process, kept in
    memory, since `gh repo view` cannot say whether auto-merge is allowed. The method last
    merged with is kept in `projects.merge_method`, as the default environment is.
- **A finished task is put away, not removed** (`put_away.rs`, #375). `task_done` stops its
  agents and its stack, closes its sessions only with `Settings::put_away_closes_sessions`, and
  marks it done (`tasks.done_at`, `done_why`), which front ends list in a folded Done group
  (`putAway.ts`). Nothing is deleted. Sessions left open get Remote Control turned off and
  their continuation cancelled, since either would start the agent again by itself.
  - **Coming back.** `task_undone`, and anything that works on it: `session_reopen`,
    `session_start` (which a message does) and `stack_start` call `bring_back`, which keeps
    when (`tasks.back_at`). Both ways publish `AgentEvent::TreeChanged`.
  - **By itself only where the user turned a rule on** (`put_away_merged`, `put_away_idle`,
    each with its days; off by default). The house looks once a minute (`follow_put_away`,
    `tasks_put_away_due`); the decision is the pure `due`. Merged counts from GitHub's
    `mergedAt` (`Pull::merged_at`), idle from the latest of creation, the task's last event
    and `back_at`; a task brought back after its merge is not put away for it again. Never
    while work goes on (`works_on`): a session busy, waiting, run elsewhere or with a
    continuation, work left running in the background, or a setup. Agents cannot turn a rule
    on through `configure`: it stops agents by itself.
  - **Offered, never done unasked,** once its pull request merged: in the panel
    (`PutAwayPart`), on the latest merged line of the conversation, and in the merge dialog's
    done view. The Remove dialog offers it instead of removal.
  - **Not the clean-up's.** A done task is a current task to `cleanup.rs`; removal stays
    `task_remove`'s.
- **A repository's GitHub issues are read through the user's `gh`, only when asked**
  (`issues.rs`, #370, `IssuesDialog.tsx`, `issues.ts`).
  - **Listed** by `project_issues` with an `IssueFilter` (whose, open or closed, GitHub search
    words, a limit), made into `gh issue list` arguments by the pure `list_args` (the words as
    one `--search=` argument, so a leading dash is never an option) and put in order by
    `ordered`, the latest changed first. No `gh`, no GitHub remote, issues turned off or no
    sign-in is an empty `IssueList` whose `unavailable` says why in `gh`'s words, never an
    error, as a task without a pull request is none. `project_issue` reads one whole.
  - **What an issue says is anyone's to write:** the core hands it on as text and leaves out
    the words of a comment GitHub hides (`hidden`). The window draws it with
    `UntrustedMarkdown` only, in the window and never in a view or a frame, so that it looks
    as it does on GitHub: its HTML is read (`rehype-raw`), cleaned with `ISSUE_SCHEMA`
    (`issueHtml.ts`: `rehype-sanitize`'s default, which follows GitHub's, narrowed to what
    Markdown makes and HTML that only formats, such as `<details>` and `<kbd>`, with no
    `style`, `class` but a code block's language, `id`, handlers or addresses off the web or
    mail), and only then coloured, so the colours' classes stay. An image is a link rather
    than loaded from wherever it points: nothing may make the window fetch an address.
    The agent reads the whole text, so nothing of it is hidden from the user unsaid: **Show
    source** shows it as written, and `issueLeavesOut` says when drawing leaves anything out
    (an HTML comment, which GitHub hides too, what cleaning drops, HTML it cannot read whole,
    an attribute's value that is neither drawn nor does anything, a title, a definition
    nothing uses, words after a code block's language, a table's cells beyond its head's, a
    link with no words). A link's address and a reference's label are not drawn either and do
    not count: the link shows that there is one. It reads the HTML Markdown leaves as written against
    the same schema and counts anything it does not read as kept; HTML in code is text and
    counts for nothing. A change to the schema needs no change there, but for an attribute
    kept with any value, which gets its line in `SHAPES`: without one it keeps no value. The command line strips
    control characters before printing it (`printable`).
  - **Start a task** fills the new-task view of the repository with `issuePrompt` (title and
    number first, the address, and a line asking the agent to read it with `gh`; never the
    body) and links it there (`newTaskIssues` in the store, a chip that takes it off). Sent,
    `TaskStart::issue` reaches `NewTaskOptions::issue`, which the task records in
    `tasks.issue` (`Task::issue`); a chat never has one.
  - **Linked:** the sidebar shows the number beside the task, which opens the Issues view on
    it; the panel reads the issue when it opens (`task_issue`), never on a timer.
    `task_pull_draft` and `task_pull_message` end the description with `Closes #N`
    (`with_closes`, kept when the text closes it already); the writer is told the branch
    closes it among what it is shown. Opening the pull request adds nothing, so the user can
    take the line out.
- **A task's work leaves its worktree only when asked** (`publish.rs`): commit everything, push
  the branch, open a pull request, each a step of its own behind a button. Commits are made by
  the user's git as them, the pull request by their `gh`; nothing is added to either.
- **A task begins on one branch, and what Swarmed shows and publishes follows the branch its
  worktree is on** (`git::current_branch`), since an agent or the user can switch it. Push, the
  pull request and its checks, what is not published, the changes and the branch a cloud agent
  starts from are all that branch's, never `task.branch` in its place: publishing the recorded
  branch beside another's work looked as if it had worked. Each other branch the worktree is
  seen on is remembered (`task_branches`, written when `task_unpublished` reads), and
  `Unpublished::branches` lists those still there, the task's own first. With more than one,
  the Publish panel says which it is on and offers the others (`task_switch`, which takes only
  a branch of that list and lets git carry or refuse what is not committed). On no branch
  only committing is left, and a rebase under way (`Unpublished::rebasing`) is named and
  waited for: git switches nothing in the middle of one. `task.branch` itself never changes;
  the task panel's heading, the status line and removal's branch deletion still mean it.
  Following the worktree is the fallback, not the way: every agent is told, in Swarmed's
  guidance whatever the settings (`publish::worktree_instructions`), that it already works in
  an isolated worktree, to keep to the task's branch, to make no other branch, worktree or
  clone even when a skill says to (skills written without Swarmed often do), how to read
  another branch without checking it out, and to put work that needs another branch in a new
  task it starts with `start_task`: at once when the user asked for that work, after asking
  when it is only its own idea. For parts that can run beside it, it is pointed at
  `spawn_session`, which shares the worktree. Agents
  are started with the task's `SWARMED_*` variables, as hooks are (`Core::task_env`), so a
  skill can tell it runs in a task and skip its own worktree step; `docs/coding-agents.md`
  tells users which skills to leave out, and has to agree with what agents are told.
- **A move of a task's worktree is watched for** (`notice_moved`, `moved_since` in
  `sessions.rs`). After every tool result and turn end the core asks git which branch the
  worktree is on (`git::head_now`, without waiting on a runtime: it is a few milliseconds and
  must come before the next event) and compares it with where that worktree was last seen.
  A move is recorded once, as `AgentEvent::WorktreeMoved`, in the session whose agent was at
  work; it is worth a notice, and the transcript offers to switch back. Its branch is
  remembered for the task there and then, not only when the panel reads. A move the agent did
  not see (the user's switch from Swarmed or by hand, another session's) is told to it with
  the user's next message and recorded with `between_turns`; `task_switch` notes its own
  switch so that it is not reported as a move. Both maps live in memory (`Live`): after a
  restart the first look sets where things stand, and only the task's own worktree is read,
  so a checkout in another folder is never a move.
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
  not pushed, or a worktree git no longer knows, whose work cannot be checked. And, whatever
  `force` says, a session of it whose agent another Swarmed runs (`Error::RunsElsewhere`): the
  dialog offers no "Remove anyway" for that (`anywayHelps`). The stack is
  always stopped. A `stop` or `teardown` that fails, or outlasts `HOOK_LIMIT` and is then
  killed with what it started (`hooks::run_within`), a broken
  configuration, and a process that outlives being killed are passed over and returned as
  notes, which the UI shows once the dialog is gone. Before the worktree goes, whatever still
  runs in it is ended (`leftovers.rs`): a stack Swarmed never knew of, or one whose services
  outlived an earlier stop, would otherwise go on recreating files as they are deleted. A shell
  open in a terminal, what keeps terminals open (tmux and the like) and the processes of an
  application from outside the worktree are left alone. The worktree is then
  moved into `.removed` beside it, which is instant, and deleted by an `rm` of its own;
  `Core::arrive` deletes what one that was cut short left, `usage.rs` counts what is still
  there, and a clean-up deletes it on request (`cleanup.rs`), making a folder some tool left
  without write permission writable first. A removal under way is kept in the UI's store (`removals`), so a dialog closed
  and asked for again starts no second one. An environment that tasks use
  cannot be removed (`Error::EnvironmentInUse`).
- **A task's `setup` is stopped on purpose, never found and killed** (`setups.rs`, #360). From
  when the task is written down until its hook and what follows it are done, its setup has a
  place in `Core::setups` (`setup_begin`, refused while one runs), and the hook runs in a
  process group of its own (`hooks::run_in_group`). A removal stops it before anything else
  (`setup_stop_for_removal`): its group is ended as a stack is (`stacks::end_group`: asked,
  `STOP_GRACE`, killed, then what it started that set itself apart), and the removal waits
  until the setup is over, so no stack is started behind it. `end_in` would otherwise find
  the hook in the worktree and kill it, reported as `HookFailed`, and could not find what it
  ran elsewhere, which held its output open. What ran the setup ends as
  `Error::SetupStopped`: for a removal (`SetupStop::Removed`) it writes no status and the UI
  says nothing (`removedWhileSetUp`); asked (`setups_stop_all`: quitting, Ctrl-C on the
  command line, and every setup begun after it) leaves the task `failed`, to set up again. A
  stop that came after the hook had ended leaves the hook's own end standing
  (`Setup::interrupted`). The setups are this process's own: a removal from another Swarmed
  finds the hook among what runs in the worktree, as before.
- **A setup says how it goes, and the user can act on it** (`setups.rs`, #359). On the
  registry above:
  - **Progress is published, never stored:** `Setup::say` writes each line to
    `run/setups/<task>.log` (0600, begun anew by each setup, deleted with the task) and keeps it
    for the next report; a reporter task publishes `AgentEvent::Setup` on session id 0 every
    `REPORT_EVERY`, and a step change (`SetupStep`) or the end (`SetupEnd`, the failure in words
    from `failure_said`) at once. Each report says where in the log its output lies (`from`,
    `to`), and `task_setup_log` reads only what was published for a setup under way, so the UI
    puts the log and the reports together without twice (`readFrom`, `heard` in `setup.ts`).
  - **Who sets it up is a lock** (`run/setups/<task>.lock`, held while it runs): the overview's
    `setting_up` says `here`, `elsewhere`, or `interrupted` for a `pending` task nobody holds.
  - **`max_setups`** (config or environment, 0 or unset: none) holds a setup at `Waiting` until
    fewer of the project's hooks run here (`setup_place`, woken by `places`). Counted in this
    process only.
  - **The user stops one** with `task_setup_stop` (`SetupStop::User`, the task left `failed`).
- **A task started from a prompt has its session before its setup** (`starting.rs`,
  `Core::task_start`, which takes `&Arc<Self>` and spawns the setup). The session is made when
  the task is written down, its prompt queued (`keep_for_setup`) and kept in
  `sessions.setup_prompt` until it goes, so a restart loses nothing (`queue_kept_again` puts it
  back on reading the queue). While the task is set up its sessions' queues send nothing by
  themselves (`held_for_setup`, `send_next_by_itself`); a setup that finishes well with an
  `Auth` sends them (`send_waiting`); one that does not leaves them, for `task_setup` with an
  `Auth` or `task_start_anyway`. The UI queues what is typed in such a session (`standing`).
  A kept prompt another Swarmed sent meanwhile is dropped from this one's queue when its turn
  comes, not sent again (`kept_went_elsewhere`). The kept prompt is in memory and in the
  database, and is changed and looked at only through the `Queues` methods that hold the lock
  on `kept` around both (`keep`, `keep_again`, `unkeep`, `kept_went_elsewhere`, `kept_edited`):
  a look between the two would put a prompt that has gone back in the queue. The lock on `kept`
  is taken before the store's and the queue lists', never after.
  The new-task view only names the task and shows fetching and making the worktree
  (`creating` in the store, by repository); the session's `SetupPanel` shows the rest.
- **Opening things elsewhere** (`open.rs`) starts the user's editor (`settings.toml`, VS Code by
  default) or the browser and never waits for them.
- **A hook's command takes each value as one word** (`Vars::render_for_shell`): a value with
  anything a shell would read is put in by the name of its `SWARMED_*` variable, never as text,
  so a branch name cannot run. Unknown-length lists from git are read NUL-separated (`-z`).
- **Git is used by shelling out** to the `git` binary (`git.rs`), not through a library.
  A command that writes or reads a repository's worktree records (`worktree add`, `remove`,
  `prune`, `list`, `branch -D`) goes through `on_worktrees`, one at a time per repository: git
  writes a new worktree's records file by file, and another git reading them meanwhile dies.
  A task's `switch` reads them too, and takes the same lock (`git::switch`).
- **The store is SQLite** (`store.rs`). Schema changes are appended to `MIGRATIONS` and never
  edited afterwards.
- **No id is given twice: every table's integer id is `INTEGER PRIMARY KEY AUTOINCREMENT`.**
  Without it, SQLite gives a deleted last row's id to the next row, and what a front end or
  the core keeps by id (a transcript, a terminal, files) turns up in the new one. A new table
  is declared with it; a test holds every table to it. The tables made before were remade
  with it by `rebuild_with_autoincrement`, the one migration that is not SQL alone: SQLite
  only takes the keyword when a table is created, and remaking one needs foreign keys off,
  which a transaction cannot switch. A culled session, or a removed task's sessions, are also
  forgotten by the UI (`forgotten` in `store.ts`).
- **The centre never holds on to a session that is gone.** Every way the session on screen goes
  (closed, deleted, its task or repository removed, here, in another window or by an agent)
  passes through `leaving` in `store.ts`, before anything of it is forgotten: the new-task view
  of its repository opens. A removal leaves it when it starts (`leaveForRemoval`), since the
  task panel would otherwise read a worktree that is being taken away.

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

- Rust edition 2024, on the current stable toolchain, pinned to its exact version in
  `rust-toolchain.toml`, which every workflow installs from (`rustup toolchain install`) and
  Dependabot moves to each new release. `rust-version` in `Cargo.toml` is the oldest it builds
  with.
- Dependencies are declared once in `[workspace.dependencies]` and inherited by the crates.
- `swarmed-core` makes no promise about its API: it is on crates.io only for `swarmed`, which
  requires it at exactly the workspace's version (`=X.Y.Z`, moved by `release-prep.sh`). A
  version number follows what a release does to the `swarmed` command and the data directory
  alone (`CHANGELOG.md`), so a change to the core's public items needs no care for semver.
- Everything the workflows run is pinned, so a run uses only what a reviewed change chose:
  every action to a commit, with its version in a comment; a binary downloaded by hand to its
  version and SHA-256; a tool `taiki-e/install-action` installs to its version; Node to
  `.node-version`; pnpm to its version and hash in `package.json`'s `packageManager`. Every
  cargo command runs `--locked` and every `pnpm install` `--frozen-lockfile`, so the lockfiles
  are what is built. Dependabot moves the actions, the crates, the npm packages and Rust; the
  tools, Node and pnpm are moved by hand.
- Commit and PR titles use conventional prefixes (`feat:`, `fix:`, `chore:`).
- A PR with a change a user would notice adds its line under `## [Unreleased]` in
  `CHANGELOG.md`, under Added, Changed, Fixed or Removed, written for the user. The release
  workflow publishes that section as the release's notes; `RELEASING.md` has the steps.
- No AI attribution anywhere: no `Co-Authored-By`, no "Generated with" lines, no session links.
- PR descriptions: a bold one-sentence summary, a `Risk:` line, then `## Problem`, `## Change`
  and a `## Verification` table.
