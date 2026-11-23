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

- **A repository can have a default environment.** Choose it in the Environments dialog, and
  every new task in that repository is set up in it: the new-task view's menu opens on it, and
  `swarmed task new` uses it when no `--environment` is given. You can still pick another
  environment, or the repository's own setup, for a single task. `swarmed environment
  <project> --default <name>` sets it from the command line. The menu's entry for no
  environment is now called "Repository's own setup", since "Default setup" no longer meant
  the default.
- **An environment can say how many stacks run at once.** "Stacks at once" in its form in the
  Environments dialog is how many of the repository's stacks may be running for one of that
  environment's tasks to start its own, in place of the repository's `max_stacks`. Left empty,
  the repository's limit applies.

### Changed

- **A new repository runs up to five stacks at once, up from two.** A repository added before
  keeps the `max_stacks` its config file says.
- **The Environments dialog keeps its buttons in view.** Only what lies between its heading and
  its buttons scrolls, so Save and Done no longer scroll away under a long form.
- **Swarmed takes an environment's snapshot itself.** Until now new tasks set up from nothing
  until a snapshot was taken by hand, which was easy to miss. Now the first task whose setup
  succeeds in an environment without a snapshot is kept as its snapshot, before its stack
  starts, and the next tasks start from it. Its setup output says so, and that task waits
  once for the copy. **Take from…** and `swarmed snapshot save` still replace it, such as
  after a dependency update. Two snapshots of one environment are no longer built at once,
  whether by tasks set up side by side or by the app and the `swarmed` command.
- **Starting from a snapshot, or taking one, takes seconds, and a new task says what it is
  doing.** Each folder is now cloned whole at once instead of file by file: a `node_modules` of
  275,000 files takes about 6 seconds instead of 48. Until now the new-task view said only
  "Setting up…" while a task started from a snapshot or kept its environment's first one; now
  it says which part is being put in place ("3 of 87, node_modules"), and the setup output
  says how long the copy took. Where a folder cannot be cloned whole, such as on another file
  system, it is copied as before.

### Fixed

- **A setting Swarmed cannot understand no longer stops it.** One key in `settings.toml` that
  the app did not know, such as one a newer Swarmed wrote beside an older one, kept the whole
  app from working. It now uses every setting it understands and names the rest in the window,
  and the command line notes them. A value it cannot take leaves only that setting at its
  default, and a key it does not know is kept in the file when the settings are saved. A file
  that is not TOML at all gives the defaults and is kept as `settings.toml.unreadable` before
  anything is saved over it.
- **The same holds for agents, MCP servers, skills and a repository's configuration.** One key
  Swarmed did not know in `agents.toml` kept the app from opening, and one in `mcp.toml` kept
  every agent from starting. In a repository's configuration or environments, it kept that
  repository's agents from starting. Each now costs only itself: a key it does not know is left
  out, an agent, server or environment that cannot be read is left out, and a setting that
  cannot be read is at its default. Each is named with its file. A `skills.toml` that could not
  be read used to offer every hidden skill again without a word, and the next change wiped what
  it held. Now that is said, and the file is kept as `skills.toml.unreadable`.
- **A snapshot can be taken where `.gitignore` ignores what is in a folder.** For a rule like
  `/vendor/*`, git names the folder and then each thing in it, and the snapshot copied both: each
  package a second time, into the copy already made. That nested the packages inside themselves,
  and failed with "Not a directory" at one holding a file of its own name, as
  `phpstan/phpstan` does. Each file is now copied once. A snapshot taken before this holds
  those extra copies until it is taken again.
- **What one task is doing no longer shows on another's panel.** Stopping or starting a stack
  and then opening a session in another task showed that task's button as "Stopping…" or
  "Starting…" too, with the first task's log, while its own stack was untouched. A commit
  message typed for one task appeared in the next one's box as well. Each task's panel now
  keeps its own: the stack's button says what is being done to that task's stack alone, also
  when the task is opened again meanwhile, and each task keeps its own unsent commit message.

## [0.3.0] - 2026-11-22

### Added

- **↑ in the message box brings back what you sent.** As in a terminal, ↑ goes back through
  the messages sent to the session, newest first, and ↓ comes forward again to what you had
  typed. They do so only with the caret at the very start or the very end of the box, so a
  message of several lines, or one long enough to wrap, stays editable: one more press gets
  there from the first or the last row. A message comes back as you typed it, `/code-review 123` rather than
  what the skill expanded to, with its attached files. Editing it makes a new message to send.
- **Put the sidebar in your own order.** Drag a repository, a task, a session or a cloud task
  by its row to another place among its own kind, or move the focused row, or the open session,
  with ⌥⌘↑ and ⌥⌘↓. Each stays with what it belongs to: a session in its task, one another
  session started among those that session started. The order is kept by Swarmed, so every
  window and `swarmed project move`, `task move`, `session move` and `cloud move` share it.
  Sub-agents come and go as they work and are not moved. A newly added repository now goes at
  the end instead of in alphabetical order.
- **Tool calls are folded, and can be shown or hidden.** A new setting under Sessions, right
  after "New sessions start in", Tool calls: Folded, now the default, where calls in a row
  become one line that counts them and names the last one, and opens to list them all; All,
  each on a line of its own as before; or Hidden, where they are left out and a larger
  animation shows while the agent works, so a long run of them does not look like a stuck
  session. Permission
  requests, questions, plans and views always show. An agent can change it with the rest of
  the appearance, except that it can only propose hiding them: that is done when you accept
  it in the conversation.
- **`swarmed-core`: `Core::project_move`, `task_move`, `session_move` and `cloud_task_move`**
  move a row among its siblings; `AgentEvent` has `Reordered`, `Error` has `CloudTaskNotFound`,
  and `Settings` has `tool_calls`, a `ToolCalls`: `all`, `folded` or `hidden`.
- **Tell Claude Code a waiting message without stopping it.** While Claude Code works, a
  waiting message has **Tell now** beside it: Claude Code takes it in at its next step, after
  the tool it is running, and goes on with it in the same turn, so a hint no longer costs the
  turn. The message shows in the conversation where Claude Code heard it; one it had not
  taken in when it stopped waits in the queue again. Other agents cannot take a message
  mid-turn, so they keep sending it now, which stops the turn.
- **⌘⇧Enter sends what is typed now.** While the agent works, ⌘⇧Enter in the message box stops
  its turn and sends the message, with its files, ahead of anything waiting; while it is idle
  it sends as Enter does. It is listed with the other shortcuts in Settings.
- **See what a background command printed.** A command an agent left running in the
  background can be opened from the task's Background list, read-only, in the terminal's
  colours, and followed while it runs. What Claude Code has cleared since it last started is
  said to be gone.
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
- **Agents know the task they work in.** Every agent is now started with the task's
  `SWARMED_*` variables, as hooks are: `SWARMED_TASK`, `SWARMED_BRANCH`, `SWARMED_WORKTREE` and
  the rest. A skill of your own can skip its worktree step when `SWARMED_TASK` is set, and
  Coding agents in the docs says which skills to leave out and what to do instead.
- **A task's worktree moving is noticed while the agent works.** When an agent, or something
  it ran, switches the task's worktree to another branch, the conversation says so right
  away, with a button to switch back, and you get a notification. A move the agent did not
  see, such as your own switch from the task panel, is told to it with your next message.
- **Agents keep to their task's branch and worktree.** Every agent is now told that it
  already works in a worktree of its own, not to switch it to another branch or make another
  branch, worktree or clone, even when a skill says to, and to ask you for a new task when
  something needs a branch of its own. Asked to anyway, it says so first.
- **`swarmed-core`: `Unpublished` has `branch`, `branches` and `rebasing`** and is no longer
  `Copy`, and `Core::task_switch` puts a task's worktree on another of its branches.

### Changed

- **Text size is right under Icons and words** in Appearance, before the layouts.
- **Skip one question, not all of them.** When an agent asks several questions at once, each
  can be skipped on its own, with its button or the S key, and the rest answered; Skip all is
  still there. The agent is told plainly that a skipped question was skipped and not to choose
  for you, and so is it when you skip them all. A skipped setting of a command like `/config`
  is left as it is.
- **Stopping a turn sends the next waiting message.** Stop, or Esc, used to leave the queue
  standing until "Send the next one now" was pressed. Stopping again stops that message's
  turn and sends the one after it. A turn that fails still leaves the queue for you to send.

### Fixed

- **An unsent message is no longer lost when another session is opened.** Each session keeps
  what was typed in its message box, attached files included, until it is sent. So does the
  new-task box, for each repository. Drafts are kept only while the window is open, and nothing
  unsent is written to disk. Closing or deleting a session, removing its task or removing a
  repository drops its draft.
- **Closing, deleting or removing the session on screen opens a new task in its repository.**
  Removing its task left it showing "Waking the spawn…" and an error in the task panel while
  the worktree went, and closing or deleting it could open the new-task view on another
  repository. The new-task view of the session's own repository now opens at once, as soon as
  a removal starts. For a removed repository, the next repository's opens instead. The same
  happens when another window or an agent closes or deletes it.
- **A session started with a skill gets a real title.** A first message such as
  `/code-review 123` used to title the session `<invoke_skill_or_bash_placeholder>`, and a new
  task started with it got that as its name, folder and branch. It is now named after what the
  skill is for, "Review PR 123", and an answer that is no title falls back to the prompt.
- **Tasks started at the same moment no longer clash by name.** Starting several tasks at once
  with the same prompt, such as five `/code-review 123`, named them all alike, and all but one
  failed because git refused the second branch of that name. Each now gets a name of its own:
  `code-review-pr-123`, `code-review-pr-123-2` and so on.
- **A session waiting on background sub-agents is neither "finished" nor "working".** When an
  agent ended its turn to wait for sub-agents it left working in the background, the session
  was marked unread with a "Finished" notice, and then showed as working whenever they wrote,
  so what you typed waited in the queue. It now goes idle and takes your messages, and is
  marked unread and notified once the agent hands their results back.
- **A sub-agent working in the background stays in the sidebar.** Claude Code now starts
  sub-agents in the background, and they dropped out of the sidebar at once, though they went
  on working past the turn. They stay listed under their session, even while it is idle,
  until they report their end, and the Background list opens one to show what it says and
  does.
- **A waiting message is no longer sent while you edit it.** If the agent's turn ended while a
  waiting message was open for editing, its old text was sent and what you were typing was
  lost. It now waits until you finish: saved, it goes with the new text; given up with
  Escape, as it was. The queue's heading says so meanwhile, and an edit left open in a window
  that went away holds nothing up for more than a few seconds.
- **A check a newer run replaced no longer shows as failed.** When a workflow run was cancelled
  because a newer run of the same workflow took its place, or a failed job was run again, the
  task's pull request listed the check twice, the old run as failed. Only each check's latest
  run is shown now, as on GitHub's own page. Two runs of a job under way at the same time,
  for a push and for the pull request, are both shown.
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

[Unreleased]: https://github.com/ryckakas/swarmed/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/ryckakas/swarmed/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/ryckakas/swarmed/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/ryckakas/swarmed/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/ryckakas/swarmed/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ryckakas/swarmed/releases/tag/v0.1.0
