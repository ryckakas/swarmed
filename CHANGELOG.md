# Changelog

[← Back to README](README.md)

Notable changes per release, in the format of [Keep a Changelog](https://keepachangelog.com).
This project follows [Semantic Versioning](https://semver.org). While it is pre-1.0, a
backwards-compatible change is a patch bump (0.1.0 → 0.1.1), and a change that breaks a `swarmed`
command, a file in the data directory or `swarmed-core`'s public API moves to the next minor
(0.1.x → 0.2.0).

The release workflow publishes the section matching a tag as that release's notes, so an entry
here is what a user reads on the GitHub release page. Each pull request with a change a user would
notice adds its line under Unreleased.

## [Unreleased]

### Added

- **A waiting message can be sent now.** Each message in the queue has an arrow beside it.
  While the agent works, pressing it stops the turn, as Stop does, and sends that message as
  soon as the turn has ended, ahead of the others. While the agent is not working, it sends
  that message at once. The rest of the queue follows as before, each when a turn ends well.
- **A task's branches, to choose between.** Once a task's worktree has been on more than one
  branch, the task panel says which one it is on, that the changes, commits and pull request
  shown are that branch's, and lets you put the worktree on any of the others; what is not
  committed comes along unless it conflicts. A rebase under way is named, and nothing is
  pushed or switched until it is over. `swarmed task switch` does the same from the command
  line.
- **Agents keep to their task's branch and worktree.** Every agent is now told that it
  already works in a worktree of its own, not to switch it to another branch or make another
  branch, worktree or clone, even when a skill says to, and to ask you for a new task when
  something needs a branch of its own. Asked to anyway, it says so first.
- **`swarmed-core`: `Unpublished` has `branch`, `branches` and `rebasing`** and is no longer
  `Copy`, and `Core::task_switch` puts a task's worktree on another of its branches.

### Changed

- **Stopping a turn sends the next waiting message.** Stop, or Esc, used to leave the queue
  standing until "Send the next one now" was pressed. A turn that fails still leaves it for
  you to send.

### Fixed

- **"This session runs on it now: its turns are billed" goes once the limits say so.** After a
  usage window reset, refreshing with the coins showed the window back within its limit, but
  the extra usage section kept saying the turns were billed until the next message. Limits
  Claude Code is asked for after a turn's report now decide it.
- **A long branch name no longer squeezes the cost off the status line.** Every piece of the
  line shrank alike, so beside a long branch the cost and the time worked were cut to `$…` and
  `1…`. The figures (changes, context, cost, time worked) now keep their width, the branch,
  model, agent and worktree share what is left, and hovering a piece shows all of it.
- **A task follows its worktree to another branch.** When an agent, or you, switched a
  task's worktree to another branch, the task panel showed the first branch's pull request
  beside the other branch's work, and Push pushed the first branch, so the new commits never
  left the machine. Push, the pull request, its checks and a cloud agent's starting point are
  now those of the branch the worktree is on.

## [0.2.2] - 2026-11-22

### Added

- **Swarmed updates itself.** About shows whether a newer version is out, and **Update and
  restart** installs it. With **Update automatically** on, as it is unless turned off under
  Settings → About, Swarmed looks at launch and every few hours, downloads a new version, and
  installs it when you quit; a dot on the version in the corner says one is ready. Off, nothing
  is fetched until About opens, and nothing is installed unless you ask. An update is installed
  only if it is signed with the release key, for the version it claims to be; one that could not
  be put in says why at the next launch. An app built from the sources is never updated. This version still has to be installed by hand: the
  ones after it update from here.
- **`swarmed-core`: `Settings` has `updates`,** an `UpdateMode`: `automatic` or `when_asked`.
- **A swarm dictionary,** in Settings just before About, in the swarm's styles. It lists every
  swarm word with the plain word it stands for and what that is, and you can search it. It is
  made from the words the app uses, so it always matches them.

### Changed

- **The swarm's styles reach further.** Settings is the Evolution Chamber and the keyboard
  shortcuts are Reflexes. The version is an evolution: `e0.2.1` in the corner, "Evolution 0.2.1"
  in About. Untitled sessions, empty and loading sessions, forks, changes, files, sub-agents,
  actions, the inbox, search and the activity charts now take the swarm's words where they still
  had the plain ones. True swarm goes all the way. Repositories and tasks are colonies and
  broods, a worktree is a cell, a branch a strain and the base branch the mother strain.
  Committing is sealing, pushing is releasing, and a pull request is an offering. Agents are
  species, models minds, MCP servers symbionts, skills genes, prompts pheromones, and the rules
  are the hive law. Questions, errors and setting hints stay plain, and so does everything an
  agent is given. The plain style reads as before.

### Fixed

- **`url` 2.5.9, for RUSTSEC-2026-0592:** in 2.5.8, `Url::origin()` on a URL nested in many
  `blob:` prefixes overflowed the stack and ended the process. Swarmed calls it itself only in
  a build run from the sources, but Tauri and its web view's crates use `url` as well.

- **An OpenCode session left on "Default model" runs on the first model listed for OpenCode
  in Settings,** and the model menu names it. It used to run on OpenCode's own default,
  whichever model OpenRouter added last, which changes without notice and may be one the
  account's allowed providers don't serve: a first message could fail with "No allowed
  providers are available for the selected model". The start of the session also named a
  model other than the one the message ran on. A `model` set in your own `opencode.json`
  stays OpenCode's default.
- **`swarmed-core`: `AgentInfo` and `AcpBackend` have `default_model`,** what a session that
  leaves the model open runs on where Swarmed chooses it; `None` leaves it to the agent.

## [0.2.1] - 2026-11-21

### Changed

- **Removing a task in the app deletes its local branch too,** unless the task started from
  that branch, or commits on it are on neither the base nor a remote, which a forced removal
  can leave: then the branch is kept, and the removal says so. A branch on a remote is never
  touched. `swarmed task rm` keeps the branch as before, unless given `--delete-branch`.
- **`swarmed-core`: `NewTaskOptions` has `free_name`,** which steps past a taken name to
  `name-2` and so on, and `RemoveOptions::delete_branch` keeps a branch whose commits exist
  nowhere else. `Error` has the variants `BranchExists`, `TaskFolderExists`,
  `NoFreeTaskName` and `Migration`.

### Fixed

- **A new session no longer shows the conversation of one culled before it.** A deleted
  session's number could be given to the next new one, which then showed what the window still
  kept of the old conversation. Numbers are now never given twice, and what is kept of a culled
  session, or of a removed task's sessions, is forgotten.
- **A new task's name no longer collides with a branch.** A name made up from the prompt now
  steps past the names of removed tasks and branches here or on a remote, to `name-2` and so
  on, where creating the task used to fail with "a branch named … already exists".
- **Removing a task also deletes what its sessions left:** messages still waiting to be sent,
  views nobody read, and the files handed to their agents, which can hold MCP servers' secrets.

## [0.2.0] - 2026-11-21

### Added

- **A Claude Code command whose screen exists only in its own terminal opens there in one
  click.** The line that says so, under `/vim` or `/privacy-settings`, has "Open in Claude
  Code". It starts Claude Code on that command in the session's terminal, in place of the shell
  until you quit it or go back to the shell. It runs with your own sign-in, in a conversation of
  its own, and nothing it shows is sent to the agent. The usage screen has the same button for
  `/usage-credits`, where Claude Code buys usage credits and sets their monthly cap. An
  action's command still goes to the shell, which is shown again for it.
- **The coins warn before a limit is reached.** Once a usage window is 75% used, as Claude Code
  starts to warn, they carry a small yellow triangle, red once a limit is used up, and the bars
  in the card and the overview turn yellow and red likewise. Extra usage's bar is grey now, so
  that yellow always means a warning.
- **The coins at the top right of a Claude Code session show your usage as it is now.** They
  open an overview of the plan's limits, extra usage and what the session spent, with a bar for
  each that carries its share, and ask Claude Code for the figures as they open, with nothing
  sent to the model and nothing added to the conversation. The context card keeps the figures
  last heard, from a turn, the coins or `/usage`, now with a thin bar under each limit.
- **Claude Code's own commands do in Swarmed what they do in a terminal.** `/usage`, `/status`,
  `/context`, `/cost`, `/memory`, `/skills`, `/hooks` and `/mcp` show their screen in the
  conversation, with your plan's limits as bars, instead of a block of text or "isn't available
  in this environment". `/model`, `/effort` and `/config` ask where the conversation stands and
  are set by your answer; `/model opus` sets it at once. None of them is sent to the model or
  interrupts a turn. The list after `/` says which commands are shown here, which ask here,
  and which are not sent, with what to do instead; those cannot wait in the queue either.
  Any other command that only opens a screen in Claude Code's terminal, such as `/vim`, is
  said to, instead of "isn't available in this environment" as if the agent had answered.
  `/usage-credits` shows your usage and extra usage, and says that credits are bought in the
  terminal. `/clear`, `/new` and `/reset` are not sent: Claude Code would forget the
  conversation while the session went on showing it, so they point to a new session. `/cd`
  and `/add-dir` are refused, since a session works in its task's worktree, and so are
  `/remote-control`, `/teleport` and `/desktop`, which would hand the session elsewhere.
- **The context window's card shows your extra usage:** what is spent beyond your plan this
  month, billed at API prices, against the monthly cap you set, and when this session's turns
  run on it. When a plan's limit is reached and Claude Code goes on with extra usage, the
  conversation says so, once, instead of "Usage limit reached".
- **The context window's card shows your usage limits and what the session has spent.** For
  Claude Code, each usage window (5-hour, weekly and the like) with how much is used and when it
  resets; for any agent that reports a cost, what the session has spent in all, and whether that
  is billed or covered by a subscription. Agents over the Agent Client Protocol report no limits.

### Changed

- **`swarmed-core`: `Core::task_remove` and `Core::project_remove` return what was passed
  over** (`Vec<String>`) instead of nothing; `Core::session_send` and `Core::session_message`
  do Claude Code's own commands themselves instead of sending them; and `AgentEvent`,
  `AgentCommand` and `Error` have new variants, which a `match` over them has to cover.
- **`swarmed-core`: `Core::slash_commands` returns each command with what is done with it**
  (`SlashCommand`: its name, its kind and a note) instead of only its name.
- **Removing a task no longer waits for its worktree to be deleted, and only unsaved work stops
  it.** The worktree is moved aside and deleted in the background; what is not deleted yet, or
  could not be, is counted in Settings under what Swarmed takes up. A `stop` or `teardown` that
  fails or takes over a minute is passed over, and what was passed over is shown afterwards.
  The dialog's "Keep it" button is now "Close", and closing it does not stop a removal under
  way.
- **An environment cannot be removed while a task still uses it.**
- **The button that ends a session is the last in the session's header,** at the far right, apart
  from the ones that carry the session on.
- **The task panel opens a task in your terminal or editor from two icons** beside the one that
  hides the panel. Which terminal and editor they open is chosen in Settings, so the panel no
  longer has a menu of editors of its own.
- **The swarm's icons and words are called True swarm and Weak swarm** in Settings →
  Appearance: True swarm, which a new installation starts in, renames the tree too, and Weak
  swarm keeps the plain names there. Nothing else about them changed, and a setting saved
  before keeps its choice.
- **The plain icons and words are called Boring** in Settings → Appearance. Nothing else about
  them changed, and a setting saved before keeps them.

### Fixed

- **What a program prints as it ends in a session's terminal is no longer lost.** On a Mac, the
  last lines of one that printed and ended at once could go missing.
- **What sessions cost is no longer counted many times over.** Claude Code reports the
  conversation's running total after every turn and keeps it when a session is resumed, but
  the costs in Settings, the turn statistics and the context card's "spent here" started it
  over at every turn. So they added each turn's whole running total again, and a session that
  had spent $1.09 could show $6.47. A total that goes down, as after `/clear`, still counts whole.
- **A task whose removal failed halfway can be removed.** "Remove anyway" now deletes what is
  left of its worktree, where it used to fail every time once git had lost track of it.
- **Removing a task ends what still runs in its worktree,** such as a stack started by hand in
  its terminal, before deleting it. Such a stack used to keep running, recreating files as they
  were deleted, which made the removal fail.
- **The button that takes a cloud task off the list shows on hover and asks first.** It used to
  be invisible, and one click took the task off at once.
- **A usage warning from Claude Code no longer reads as a limit reached.** A window filling up
  is said as a warning, with how much of it is used, once rather than after every turn. When a
  window resets on another day, the day is given as well as the time.
- **A stack that would not end when Swarmed quits is found again by the next one,** as it
  already was after stopping a single stack.

## [0.1.0] - 2026-11-21

The first release: a macOS app and a `swarmed` command over one Rust core. Swarmed is not signed
with an Apple Developer ID yet, so macOS asks before opening it the first time when it comes
through Homebrew or a browser.

### Added

- **A task is a branch in a worktree of its own.** Add a git repository, say what you want done,
  and Swarmed creates the branch and a worktree under `~/swarmed/<project>/<task>`, runs the
  repository's setup hook, and starts an agent there. Nothing is written into the repository
  itself.
- **Any agent.** Claude Code runs with its own login and configuration; Codex, OpenCode,
  Antigravity and any agent that speaks the Agent Client Protocol are set up from Settings →
  Agents or in `agents.toml`. Swarmed never signs you in to an agent and keeps no GitHub token: it
  uses each agent's own login and your `gh`. A new session's agent can be picked until its first
  message, and a conversation can be cloned onto another agent.
- **The conversation.** Replies stream as they are written, with syntax highlighting, and HTML an
  agent hands over is shown in a sandbox on an origin of its own. The transcript says what the
  agent is doing and how long each turn took. Messages queue while the agent works; files can be
  picked or dropped on the window; commands, skills and saved prompts are offered after a slash.
- **Supervision.** Permission requests are answered in the conversation, and an agent's questions
  by arrow keys, number or click. Auto mode, plan mode, the model and its effort are set per
  session and can change mid-session.
- **Sessions.** Rename them by hand or by a model, fork one from any message into a new session
  that holds that message and what followed, close, archive, reopen or delete them, and search
  what was said in any of them. Unread answers are marked, and notifications and a count on the
  Dock icon say when a session needs you, finishes or fails; ⌘I lists everything that waits.
- **Sessions working together.** Agents get tools to see, read, message, start and close other
  sessions, and spawned sessions are listed under the one that started them. Claude Code's
  sub-agents appear under their session while they work. One prompt can go to several agents at
  once, and work can be handed to a Codex cloud agent or a Claude Code cloud session: from a
  session too, as a brief its agent writes or with the whole conversation.
- **The task panel.** Changed files with their diffs, comments on lines sent to a session, a Files
  tab, commit with a message the session's agent writes, push, and a pull request whose title and
  description are written for you, with its checks summed up in a line that unfolds into the list,
  failures first. Two tasks' changes can be compared, and a worktree opened in the editor, the
  browser or the terminal application.
- **A terminal in each session**, whose output the agent sees, configured in JSON; processes a
  task's agents left running are listed and can be stopped.
- **Environments.** Named environments per repository with their ports and variables, a start
  command that keeps running beside the setup hook, and snapshots of the files git ignores, so a
  new task starts set up like an earlier one.
- **One pool of MCP servers and skills.** The servers configured for Claude Code (its plugins'
  too), Codex and OpenCode, and Swarmed's own, go to every agent that lacks one by that name, read
  when a session starts (a repository's own `.mcp.json` only once approved in Claude Code); skills from every agent and from the task's repository likewise.
- **Your own setup.** Rules every agent is given, notes on a repository and on a task, actions of
  your own, a status line from built-in pieces or a script, keyboard shortcuts, and API keys kept
  in the macOS Keychain for agents that need them. Agents can propose configuration changes;
  rules, environments and commands change only after your yes.
- **Looks.** Six colour palettes, Swarm among them with its own words and marks, the classic,
  senior and custom layouts, and the text size.
- **What it costs.** What the agents report their work cost, by day and by task; turns by day and
  model in charts; and what Swarmed takes up on disk and is using right now.
- **About, and a menu bar of its own.** Swarmed's menu has About, Settings and Keyboard
  Shortcuts. The About view, also in Settings and behind the version in the sidebar's top corner,
  says what Swarmed is and which version is installed, and links to its source.
- **The `swarmed` command** for repositories, tasks, sessions, agents, keys and snapshots from a
  terminal. `SWARMED_FAKE_AGENT=1` swaps in a scripted agent that spends no tokens.
- **Installing:** an install script, a Homebrew cask that also puts `swarmed` on the PATH, and
  `cargo install swarmed` for the command alone.

[Unreleased]: https://github.com/ryckakas/swarmed/compare/v0.2.2...HEAD
[0.2.2]: https://github.com/ryckakas/swarmed/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/ryckakas/swarmed/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/ryckakas/swarmed/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ryckakas/swarmed/releases/tag/v0.1.0
