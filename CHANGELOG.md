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

- **A usage limit whose share is not a whole number is no longer missing** from the context
  card and the coins' warning, where `/usage` already showed it.
- **A weekly limit that names no model is no longer shown as a second "Weekly".**
- **Choices of `/config` made while the agent is at work say that Claude Code is told once it
  is free,** and nothing says it was told when the choices could not be passed on.
- **A skill of your own named like one of Claude Code's commands,** such as `status` or `diff`,
  **is called by `/name` again** instead of showing that command's screen or being refused.
- **A question of `/model`, `/effort` or `/config` left unanswered no longer takes the keyboard
  when its session is opened again,** where a digit typed into the empty message box chose an
  answer. It still takes it right after you send the command.
- **A stack that would not end when Swarmed quits is found again by the next one,** as it
  already was after stopping a single stack.

### Changed

- **`/usage` no longer names your plan.** What it shows is kept with the conversation, and the
  plan's name is not something Swarmed keeps.
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
- **The plain icons and words are called Boring** in Settings → Appearance. Nothing else about
  them changed, and a setting saved before keeps them.

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

[Unreleased]: https://github.com/ryckakas/swarmed/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ryckakas/swarmed/releases/tag/v0.1.0
