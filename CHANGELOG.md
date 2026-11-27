# Changelog

[← Back to README](README.md)

Notable changes per release, in the format of [Keep a Changelog](https://keepachangelog.com).
This project follows [Semantic Versioning](https://semver.org). While it is pre-1.0, a
backwards-compatible change is a patch bump (0.1.0 → 0.1.1), and a change that breaks a `swarmed`
command or a file in the data directory moves to the next minor (0.1.x → 0.2.0). `swarmed-core`
is published only for the `swarmed` command, which requires the one of its own version exactly,
and makes no promise about its API: any release may change it.

The release workflow publishes the section matching a tag as that release's notes, so an entry
here is what a user reads on the GitHub release page. Each pull request with a change a user would
notice adds its line under Unreleased.

## [Unreleased]

### Added

- **repolore, built in.** Swarmed can run [repolore](https://github.com/ryckakas/repolore),
  which searches your own pull requests, reviews, issues and docs on GitHub, as its own MCP
  server: Settings → MCP servers, under **Swarmed built-in**, or `swarmed mcp builtin`. It is off
  until you turn it on. Swarmed installs it, shows the command first, keeps its settings and
  index in its own data folder, and gives it to every session, whichever agent runs it. Turning
  it on copies the setup you already have, with its repositories and index, or starts fresh;
  your own setup is never changed. So that only one repolore holds its model at a time, it waits
  until repolore is registered nowhere else and your old server has stopped, and lists what is
  left: each registration with its own remove command, shown before it runs, and the idle
  sessions still on the old server, to restart. **Open repolore** opens its own page for
  everything else, a session's start refreshes its index when one is due, and once it has taken
  over, your old setup can go to the Trash. It needs repolore 0.5.1 or newer.

## [0.4.11] - 2026-11-27

### Added

- **A stylesheet of your own.** For what the custom layout has no field for, such as a wider
  conversation or rounder corners, turn on **Your stylesheet** in Settings, under Appearance:
  `style.css` in Swarmed's data folder, applied after the app's own styles so that it wins.
  Write it in Settings, where saving applies it at once, open it in your editor, or ask an agent
  to write it. Since CSS can hide anything in the window, a Deny button included, a change made
  to the file outside Settings is shown to you, line by line, and applied only once you press
  Apply, and Swarmed's own tools give agents no way around that. It loads nothing from elsewhere:
  no web fonts, no remote images. The app's colour and size variables stay from version to
  version; its class names may not. If a stylesheet ever hides the switch that turns it off, hold
  ⌥ while Swarmed starts, or run `swarmed layout stylesheet off`.
- **See how many tokens a turn is using while it runs.** Beside what a Claude Code session is
  doing, its turn counts the tokens used so far, with an arrow for which way they go: up while a
  request is read, down while its answer comes in. Only what the model reads anew and what it
  writes counts, not the conversation it reads again from the cache with every request. A
  sub-agent counts on its own, in its own pane. The line where a turn ends says what it used
  ("Turn finished in 12s, 3.4k tokens"), so does `swarmed session send`, and the status line
  has a new **Tokens** piece that adds up the session's turns. Other agents report nothing of
  the kind while they work, so they show none.

### Changed

- **Everything a sidebar row shows stays put under the pointer.** A pinned pin, a note that holds
  something, a task's browser globe, a session's mark, whom it works for and its agent's icon no
  longer slide aside as you point at the row to press them. The buttons that only show then (the
  highlight, the cloud, an empty note, a pin not pinned) now come right after the name and take
  their room from it, and the pin sits between them and what the row always shows, so pressing
  it leaves it where it is.

## [0.4.10] - 2026-11-27

### Added

- **See everyone who works for a session, all the way down.** The session's header counts every
  session that works for it, however far down and in whichever task, on one line by how they
  are doing ("9 working for it: 1 needs you, 2 working, 6 idle · 2 closed"). Press it to unfold
  their tree: nested as they work for one another, each with how it is doing and, in another
  task, that task's name and pull request badge. Press a line to open the session, or the badge
  to open the pull request. The closed ones are folded below, each with Reopen, so a finished
  chain stays readable.
- **Close a session with every session that works for it.** When others work for the session you
  close, however far down and in whichever task, the close dialog lists them with how each is
  doing and offers to close them too, unticked unless you tick it. Putting a task away asks the
  same about sessions of other tasks that work for its sessions; their tasks stay as they are.
  If another Swarmed runs any of them, nothing is closed. From the command line,
  `swarmed session close <id> --with-workers` and `swarmed task done <project> <task>
  --close-workers`.
- **How many sessions may work for one is yours to set.** Settings → Sessions → Workers per
  session (`max_workers`) sets how many open sessions may work for one session before its agent
  starts no more, from 1 to 20; it was a fixed six, which stays the default. When an agent is
  refused one more, it is told the setting's name, so it can tell you what to change. Agents
  cannot change it themselves.
- **Agents choose the model and effort of the sessions they start.** `spawn_session` and
  `start_task` take a `model` and an `effort`: a model the agent offers and an effort it has,
  or the agent is told the list to choose from. The session starts on them from its first turn,
  however quick its setup, and the starting agent and `list_sessions` say what each session
  runs on. Left out, a session runs on its role's defaults, else on its starter's, as before.
- **Each role can have a model and effort of its own.** Settings → Sessions → Role defaults
  (`role_defaults`) starts every new session with a role on them, such as a reviewer on Fable
  at medium effort, whoever starts it: you or an agent. A model or effort chosen for the
  session wins. Agents cannot change these.
- **Code blocks and tool calls can be copied with one press.** Pointing at a code block in an
  agent's reply, a plan, a sub-agent's conversation or an issue shows a copy button in its top
  right corner, which the keyboard reaches with Tab too. It copies the code as written: no
  fence, no language name, no colours, and no newline left at its end. An opened tool call's
  input and result have the same button. It says **Copied** for a moment, or that the copy did
  not go through, and pressing it selects nothing, so the Quote button stays away.

### Changed

- **What runs moves.** A session with work left in the background no longer shows a ring that
  stands nearly still: while sub-agents are at work, a small grub for each (up to three) circles
  its mark, or an arc in the plain style, and while only commands run, a faint dashed thread
  creeps round it, quieter. A pull request's badge has a light passing across it while its
  checks run, a task's browser badge turns slowly while its browser runs, and a sub-agent at
  work in the sidebar is a tiny crawling grub. With reduced motion turned on in the system, each
  stands still as before.
- **A task folds by itself once its last open session is closed.** Closing or deleting the
  last open session of a task, yourself, by an agent or from another window, folds the task in
  the sidebar instead of leaving "No sessions" and the closed line under it. Unfold it to see
  its closed sessions and reopen one; it stays unfolded until its last open session goes
  again. A task being set up is left as it is. An unfolded task with only closed sessions no
  longer says "No sessions" above them.
- **A task's pull request and issue badges stay put under the pointer.** They now come after the
  task's note and the buttons that only show while the row is pointed at, right before its +, so
  they no longer slide away as you move to press them.

### Fixed

- **`swarmed session set` reaches an agent the app runs.** A model, effort or supervision set
  from the command line for a session whose agent runs in the app is passed on to that agent
  within a second, as a change in the session's header would be, and the command says whether it
  was passed on or is taken when the agent next starts.
- **Tables in replies no longer break short words in the middle.** A column of a table in an
  agent's reply, a plan or an issue is at least as wide as its longest word, so a heading such
  as `Task` or a name in code such as `own-links` breaks only at a space or a hyphen. A table
  wider than the conversation, such as one with a long path in it, scrolls sideways on its own,
  and the conversation does not. A long address in a sentence still breaks at the edge of the
  line.
- **A session's agent icon is the last thing on its row in the sidebar.** It comes after whom
  the session works for ("for #123"), its mark and its time to go on by itself, so the icons of
  a task's sessions stand in one column, and a role sits right beside "for #123". A press on the
  icon still opens the session.
- **A narrow sidebar no longer scrolls sideways, and every name keeps a few letters.** The
  buttons a row shows only under the pointer (pin, note, cloud, issues, environments) take no
  room until it is pointed at or reached with Tab, in classic as in senior. The narrower the
  sidebar, the shorter a row draws what follows its name, step by step: a pull request's number
  without its #, then no task icon, then an issue's ring without its number, then a pinned pin
  and a filled note that wait for the pointer; a session's mark shortens its words, and its time
  to go on shows only its clock. Pointed at in a sidebar narrower than its default, a task's row
  with badges leaves out its cloud and then an empty note, and a session's row beside a mark,
  its time to go on or "for #123" leaves out its pin, unless it is pinned, and then its
  highlight. Tab still reaches each of them, and so do shortcuts: ⌘D pins, ⌘⇧H highlights, ⌘⇧N
  opens the task's note, and the session's header hands work to the cloud. Every + stays in
  one column, and a pinned session's pin now stands before its agent icon, so the icons do too.
- **A long waiting message no longer takes the whole conversation.** A message waiting in the
  queue shows its first three lines, with an ellipsis when there is more, and its files and
  buttons stay beside it. Press it to see and edit the whole text, in a box that scrolls inside
  itself. The queue shows about three messages, then scrolls under its heading, so the
  transcript and the message box stay within reach whatever waits.

## [0.4.9] - 2026-11-27

### Added

- **Math in agents' replies is drawn as math.** A formula an agent writes in `$$…$$`, `\(…\)`,
  `\[…\]` or a fenced `math` block is drawn by KaTeX, which comes with the app, so nothing is
  fetched from the network. A single `$` stays a dollar sign, so replies about prices and shell
  variables (`$5 and $10`, `$HOME`) read as before; Settings → Sessions → Math also draws `$…$`
  where it reads as math, or turns math off. Code is never drawn as math, a formula KaTeX cannot
  read shows as written, and copying a formula gives its TeX.
- **Links in your own messages open.** An address you wrote in a message (`https://`, `http://`
  or one starting with `www.`) is a link in the conversation and in the queue of waiting
  messages, and opens in your browser when pressed, as a link in an agent's reply does.
  Punctuation that ends the sentence after it, and a bracket closed around it, are not part of
  the address.
- **Code and paths in your own messages look like code.** What you wrote between backticks is
  drawn as code, without the backticks, as in an agent's reply, and so is a path: absolute,
  from your home (`~/`), or relative with a folder and a file name (`ui/src/setup.ts`), with a
  line and column after it if you wrote them. A path in the task's worktree, or in a chat's
  folder, opens in your editor when pressed, at the top of the file. The rest of what you
  write stays as you typed it: a heading, emphasis or a list is not drawn as Markdown.

### Changed

- **An architect or a planner leaves plan mode once you approve its plan.** It goes on in Auto
  mode, so starting, following and closing the sessions that carry the plan out no longer asks
  you about each step; it is still told to hand the plan on and change no files itself. A
  reviewer stays in plan mode. See
  [How a role is kept to](docs/coding-agents.md#how-a-role-is-kept-to).
- **A reviewer can hand fixes to implementers.** Asked to get its findings fixed as well, a
  reviewer starts implementers for them with `spawn_session`, and reports what was found and
  fixed.

### Fixed

- **Sessions with a role no longer ask you before each session tool.** Listing, reading and
  messaging other sessions and closing them go through without a question, in plan mode too, so
  a reviewer can report its findings. Starting sessions and tasks does as well once the session
  is out of plan mode, so a planner whose plan you approved starts its implementers unasked; in
  plan mode you are still asked about each start, since what is started changes files. Allow
  rules of your own could not do this: in plan mode Claude Code asks before every MCP tool that
  is not read-only, whatever they say. An agent that names these tools otherwise than Claude
  Code does may still ask.
- **Agents close the sessions they start with `start_task`.** An architect or a planner that
  hands work to an implementer in a task of its own can close that session with
  `close_session` once it has reported back, as it can a session it spawned beside itself, and
  so makes room to start more. The task, its worktree, branch and pull request stay. A session
  you started stays yours to close, even one you set to work for an agent, and one whose task
  is still being set up is not closed.
- **A setup with nothing to run no longer flashes its panel.** Starting a task on the
  repository's own setup, where there is nothing to set up, showed the setup panel and the
  sidebar's setup line for a moment before they went again. A setup now shows only once it has
  run for most of a second, and at once if it fails or is stopped; a longer one shows as before,
  with how long it has run counted from its start. What you type meanwhile still waits for it.
- **A closed pull request no longer looks like a draft.** Its number is struck through on the
  task's badge in the sidebar, in the task panel, and where a message or the `#` menu names a
  closed pull request or issue. It stays grey: red still means a failed check.
- **In the senior layout, a task's + lines up with the others.** The button that starts a
  session on a task's row sat about 4px right of the + on the chats and repository headings, so
  the column of + buttons broke at every task. They are in one column now, as in classic, and
  stay so at any icon size a custom layout sets.
- **A highlighted session's title has room again in a narrow sidebar.** The drop that opens a
  row's colours stayed in view on a highlighted task or session, so in a narrow sidebar a
  session with a mark as well had no room left for its title. The drop now shows only while
  you point at the row, reach it with the keyboard or have its colours open, in every layout,
  and takes no room otherwise: the bar at the row's right edge shows its colour. In classic,
  every task and session row has that room for its name again, highlighted or not.
- **Purple marks and highlights no longer look red.** Purple was half the palette's link colour
  and half red, so in the swarm palette, whose link colour is magenta, it came out next to red,
  and in the others a greyish mauve. Each palette now has a purple of its own, in dark and in
  light, which stands apart from its red, its blue and its link colour, and a custom layout's
  `purple` still takes its place.

## [0.4.8] - 2026-11-26

### Added

- **Agents start tasks.** Ask an agent for work that needs a branch of its own, such as a fix to
  another pull request or the next pull request of a stack, and it starts a new task for it
  with Swarmed's `start_task` tool instead of switching its own worktree to another branch. The
  task is set up as New task sets one up, its session starts on the prompt once the setup is
  done, and it reports back to the session that started it. `swarmed task new --from <branch>`
  starts a task's branch from another branch the same way. See
  [What Swarmed gives every agent](docs/coding-agents.md#what-swarmed-gives-every-agent).

- **Who works for whom.** A session can work for another session of the same repository, in
  any task: it reports to that one, and the sidebar lists it under it, nested to any depth, or
  with a `for #7` link when that one is in another task. Sessions an agent starts with
  `spawn_session` or `start_task`, or with `swarmed session run` from its shell, work for that
  agent's session. Choose it yourself from the menu in a session's header, which also shows the
  session it works for and those that work for it, or with `swarmed session parent` and
  `swarmed session run --parent`. Agents are told who works with them when they start, and
  again with the next message when you change it. `list_sessions` and `read_session` say it
  too, and `swarmed session list` adds it at the end of each line. See
  [Who works for whom](docs/coding-agents.md#who-works-for-whom).

- **Roles.** Give a session a role, architect, planner, implementer or reviewer, in the
  new-task view or the session's header, and its agent keeps to the role's rules: a planner
  plans and hands each change to implementers, an architect splits a feature into tasks with a
  planner in each. Change it while the session runs and the agent is told with your next
  message; the conversation shows where. Sessions an agent starts get a role its own may start,
  and the sidebar shows each session's role. `swarmed session role` and `--role` do the same
  from the command line. See [Roles](docs/coding-agents.md#roles).

- **Roles are kept to.** An architect, a planner or a reviewer starts in plan mode, or in its
  agent's read-only mode, and taking the role off puts back the supervision it had. Approving
  its plan keeps it there and has it hand the plan on, to planners or implementers, instead of
  building it itself. A planner another session started can start its implementers, up to three
  sessions with a role in a row. See
  [How a role is kept to](docs/coding-agents.md#how-a-role-is-kept-to).

## [0.4.7] - 2026-11-26

### Added

- **Apps from MCP servers.** When Claude Code calls a tool whose MCP server attaches an app to
  it (MCP Apps), such as a chart, a map or a form, Swarmed shows the app under the call. It runs
  sandboxed, from an origin of its own, and reaches the network only where its server declared,
  after you press Show. A link or a message it wants to send waits for you, and context it
  leaves for the agent shows as a chip above the message box, to go with your next message; a
  tool of its own server it calls goes through the agent, which decides it as its own call.
  See [Apps MCP servers attach to their tools](docs/coding-agents.md#apps-mcp-servers-attach-to-their-tools).
- **Highlight a task or a session in a colour of your own, where it is.** The drop beside a
  row's pin, or ⌘⇧H on the focused row or the open session, opens a palette of the five colours
  marks use, and None. A highlighted row keeps its place and gets a bar of its colour at its
  right edge, so what it is open, unread or waiting for still shows as before; the session's
  header and the inbox show a dot of it. Writing to the session, closing and reopening it and
  restarting keep it; a fork starts without one, and agents cannot set one. A custom layout can
  give each of the five colours its own value (`green`, `blue`, `yellow`, `red`, `purple`, in
  dark and in light), which marks follow too. `swarmed session highlight <id> <colour>` and
  `swarmed task highlight <project> <task> <colour>` do the same, and `--off` takes it off.
- **Open a changed file in your editor from the Changes tab.** Pointing at a file's line in the
  task panel's Changes tab shows a button at its end that opens the file in the editor from
  the settings, in its worktree's window, as the Files tab does. Its diff stays as it was. A
  deleted file has nothing to open, so its line has no button.

### Changed

- **A session with work left in the background no longer looks idle.** Its status mark in the
  sidebar gets a ring: a clear one that breathes slowly while sub-agents are at work, and a
  faint one while only commands it left running run, such as a dev server. Pointed at, the mark
  says what runs ("2 sub-agents, 1 command in the background"), and so does the session's
  header. A folded task or repository shows the ring too, ranked below working and above idle.
  The session stays idle all the same, and takes your messages at once. It reads the same
  after a restart and in a second Swarmed, and `swarmed session show` says it as well.

### Fixed

- **A row pinned stays above the rows not pinned.** A repository, task or session pinned
  again after you moved it could stay where it was, below rows not pinned, until the sidebar
  was next drawn: an older reading of the list could arrive after the newer one and take its
  place. The sidebar now keeps the newest.
- **Dragging a task among the others goes where you drop it.** A repository with tasks put
  away counted them, though they are listed apart in Done, so a task dragged or moved with
  ⌥⌘↑/↓ could stop short or not move at all. A task brought back still comes back where it
  was.
- **The + for a new chat lines up with the others.** On the chats' heading in the sidebar it
  sat further right than the + on each repository's heading.
- **A sub-agent's long prompt or answer no longer pushes its conversation out of reach.** In a
  sub-agent's pane, What it was asked and what it handed back can each be folded, and opened
  they scroll inside a height of their own, giving way where the window is short, so the
  conversation between them always keeps its room. In the task panel's Background list, a long
  summary of a finished sub-agent shows its first lines, with Show all to read the rest in a
  box that scrolls.

## [0.4.6] - 2026-11-25

### Added

- **Point the agent at an element of the page.** **Pick** in the Browser pane, or ⌘⇧C, puts
  the page in Chrome's own inspect mode: the element under the pointer is outlined, and a click
  puts it into the message box as a token, `[Element 3: button "Save" on /settings]`. Sent, the
  agent gets its address, a selector that finds it, its role and name, a few computed styles,
  its text and HTML marked as the page's content, and a picture of it kept with the session.
  Several can go in one message; the transcript shows each with its picture, which opens
  large. Esc stops picking. See
  [Pointing at an element of the page](docs/coding-agents.md#pointing-at-an-element-of-the-page).

- **Agents browse in the task's browser, where you watch them.** Your own Playwright MCP and
  Chrome DevTools MCP servers are pointed at the task's Chrome, so an agent that checks a page,
  or a skill that says to use them, acts in the Browser pane beside the conversation, on the
  task's profile, instead of opening a window or a fresh profile of its own. Each session has
  an address of the task's browser in `SWARMED_BROWSER`, which also starts it, so a session
  that never browses starts nothing. Every agent is told to use it, and the new
  `show_browser` tool opens the pane on an address, in a tab of the session's own. The pane
  marks the tabs agents acted in, says which session acted last, and goes to the tab an agent
  works in. Claude Code's own servers and those Swarmed hands other agents are covered; a
  server another agent keeps in its own configuration is not. See
  [The task's browser and your browser tools](docs/coding-agents.md#the-tasks-browser-and-your-browser-tools).

- **A Chrome of each task's own, live beside its sessions.** The globe in a session's header
  opens the **Browser** pane: the task's page, drawn live, with tabs, an address bar, back,
  forward, reload and its console. Click, scroll and type in it as in any browser, paste with
  ⌘V; a dialog the page opens is answered there. It runs on the Google Chrome, Chromium,
  Microsoft Edge or Brave you have, without a window, on a profile of the task's own, so a
  login there stays with the task across restarts and never mixes with your own browser's or
  another task's. It opens on the task's dev server when that runs, and stops when the task is
  put away or removed, when Swarmed quits, and after 30 minutes unused (a setting). A task
  whose browser runs is marked in the sidebar. No browser installed: the pane offers to
  install Google Chrome with Homebrew, showing the command first. `swarmed task browser`
  starts it and prints where its DevTools endpoint answers, for tools that drive a browser.
  The page is drawn at your screen's density, lighter while it scrolls and sharp once it
  stops; started again, it opens the pages it had open. Each task remembers whether its pane
  is open, so going to another task's session neither opens its pane nor starts its Chrome.
  To have Chrome draw the page itself, **Settings → Sessions → Browser → Show it** puts it in
  a window of its own beside Swarmed, where agents' browser tools act all the same.
  See [The task's browser](docs/configuration.md#the-tasks-browser).

- **See the images a tool returns.** A screenshot from a browser tool, or an image file the
  agent read, used to show as `[image]`; it now shows under the call, large when pressed, with
  **Open** to see it in your system's viewer. The last image of a turn also stands at the end
  of the agent's reply, so it shows with folded tool calls. Claude Code and ACP agents that
  send images in their tool calls are covered; PNG, JPEG, GIF and WebP up to 10 MB, at most 8
  per result. The images are kept with the session, for you alone, and deleted with it; see
  [Screenshots and other images tools return](docs/coding-agents.md#screenshots-and-other-images-tools-return).

### Fixed

- **Tasks started at once in one repository are all created.** Now and then one of them
  failed with git's `failed to read .git/worktrees/<name>/commondir`: git writes a new
  worktree's records one file at a time, and another git reading them meanwhile gave up.
  Swarmed now runs its worktree commands on a repository one at a time.
- **The coins now get your usage as it is when you ask.** Claude Code answers from its own
  copy of the figures for a minute after it fetched them, in every Claude Code on your
  machine, so opening the coins or pressing Ask again in that minute changed nothing, even
  right after your usage was reset. The question now waits until Claude Code fetches the
  figures again, and the overview counts down to it. Every step shows: the coins pulse and the
  overview says it is refreshing, with a bar, while the figures last heard stay in full colour,
  and the answer lights them up and says when it came, to the second, and whether anything
  changed.
- **The usage notice no longer says requests are refused when they are not.** With extra
  usage turned on and under its cap, a limit reached is billed, not refused, even before a turn
  has been billed: the notice said refused whenever the latest turn had not run on extra
  usage. A figure of 100% from asking, which may be 99.5%, no longer counts as a limit reached
  until a turn says so, and a model's own weekly limit says it stops that model alone.
- **One row per usage limit.** The row called "weekly, extra usage included" was not extra
  usage: it was a model's own weekly limit, such as Fable's, as Claude Code reports it after a
  turn, shown beside that model's own row and left behind when the two differed. It is now
  shown on the model's row. Before Claude Code has named the model, it shows as "Weekly model".
  That limit used up no longer holds back sessions set to go on when a limit resets, since it
  stops only that model.

## [0.4.5] - 2026-11-25

### Added

- **See what one turn changed.** Swarmed now keeps the task's worktree as it was before each
  message went to the agent and when its turn ended. The Changes tab can show the whole task,
  as before, or one turn of the session on screen: the files it changed, edits made by shell
  commands included, each with its diff, and the other sessions that were at work at the same
  time. `swarmed session turns` and `swarmed session turn-diff` show the same in the terminal.
  The latest 50 turns of each session are kept, outside your repository and without the files
  it ignores; see [What a turn changed](docs/configuration.md#what-a-turn-changed).
- **Mention another session with `@`.** Type `@` in the message box to point the agent at
  another session: the list shows the sessions of the same task, and typing finds the rest it
  can read. The agent is given the session's id and title and reads what it needs with
  `read_session`, which can now also read a long conversation from its start. A session it
  cannot read is refused before anything is sent. A sent mention shows as a chip that opens the
  session, and `swarmed session send --session <id>` does the same from the terminal; see
  [Mentioning another session](docs/coding-agents.md#mentioning-another-session).
- **Mention a pull request or an issue with `#`.** Type `#` in a repository's message box to
  pick one of its pull requests or issues: the list shows the latest changed, digits narrow it
  by number and words search GitHub. The agent is given each one's number, title, state,
  branches and address, to read more with `gh`. A message waiting in the queue goes with each
  one's state as it is when it is sent. A sent mention shows as a chip coloured by its state
  that opens it on GitHub; `#` with nothing picked stays text. From the terminal,
  `swarmed session send --pull <n>` and `--issue <n>` do the same, and `swarmed pr list` lists
  what `#` offers; see
  [Mentioning a pull request or an issue](docs/coding-agents.md#mentioning-a-pull-request-or-an-issue).
- **Edit from here: take a session back to before one of your messages.** Beside Fork from here,
  and as `/rewind` (`/checkpoint`, `/undo`) in a Claude Code session, which no longer refuses
  it. Choose the conversation only, or the files too: the worktree goes back to the checkpoint
  taken before that message, and **Put the files back** undoes that while nothing changed them
  since. The message goes back into the box to edit and send again. Claude Code resumes its own
  conversation there and forgets what came after; other agents start a new one told what was
  said up to it. What was taken back stays in the transcript, folded. Commits stay on the
  branch, and it is refused, with the reason, while an agent could write into the worktree or a
  rebase or merge is under way. `swarmed session rewind` and `swarmed session put-back` do the
  same from the terminal; see
  [Taking a session back](docs/coding-agents.md#taking-a-session-back-to-before-a-message).

## [0.4.4] - 2026-11-25

### Added

- **Swarmed follows your tasks' pull requests and tells you what changed.** Checks that failed,
  all checks passing, a review or a comment from someone else (who, never what they wrote), a
  branch that conflicts with its base, a merge and a close each show as one line in the
  conversation of the task you wrote to last, and as a notification while you look elsewhere.
  What changed while Swarmed was closed is said at the next start, once. Where checks failed,
  **Send the failure to the agent** puts them, with the last lines of each failed job's log,
  into the message box for you to read and send. `swarmed task pull --follow` prints each change
  in the terminal. Configuration's [Pull requests](docs/configuration.md#pull-requests) says what
  Swarmed asks GitHub and how often.
- **Merge a task's pull request from its panel.** **Merge…** opens a dialog with where it goes,
  the commit it is at, its checks and its review, and the methods the repository allows, starting
  on the one you used there last. Its button merges the way its menu chose: **Merge**, which
  points you to **Force merge** while a check failed, and **Bypass requirements** if you
  administer the repository. **Enable auto-merge** has GitHub merge it once its requirements are
  met. Only the commit you saw is merged, no branch is deleted, GitHub's refusals are shown in
  its words and a merge queue is named as one. `swarmed task merge` does the same in the
  terminal.
- **Put finished tasks away.** **Put away** in a task's panel, or **Put away instead** when you
  remove it, stops its agents and its stack and lists it in a folded **Done** group at the end of
  its repository, with nothing deleted: the worktree, the branch, your notes and its sessions
  stay. Once its pull request merged, the panel, the line that says so and the merge dialog
  offer it. **Bring back** lists it with the others again, and so does working on it.
  Settings can put tasks away by themselves, each rule off until you turn it on: a set number of
  days after their pull request merged, or after their sessions sat idle that long, never while
  one of them is at work. Whether putting away also closes the sessions is a setting of its
  own. `swarmed task done` and `swarmed task undone` do the same in the terminal.
- **Start a task from a GitHub issue.** The issue button on a repository's row, or ⌘⇧I, lists
  its issues through your own `gh`: everyone's or those assigned to you, opened by you or
  mentioning you, open or closed, and words to look for. Pick one to read it with its comments,
  drawn as GitHub draws them: folded `<details>`, `<kbd>` keys and the other HTML GitHub allows
  to format text, never a script, a style or a form, and images as links, never loaded.
  **Show source** shows the text as written, which is what the agent reads, and the issue says
  when the drawing leaves part of it out. **Start a task** opens New task with a prompt that
  names the issue and asks the agent to read it, for you to read and send. The task remembers
  its issue: the sidebar shows its number, the panel its title and whether it is still open,
  and the pull request text Swarmed writes ends with `Closes #N`, so merging closes the issue.
  Without `gh`, or off GitHub, the list says why. `swarmed issue list`, `swarmed issue show`
  and `swarmed task new --issue` do the same in the terminal.

### Changed

- **Pull request badges no longer ask GitHub each on a timer.** One list of open pull requests
  per repository serves every badge and panel, asked only by the first Swarmed on the data
  directory, sooner while checks run and less often once nothing moves; a repository without a
  pull request open is not asked at all. A task whose pull request is not on that list (merged,
  closed or someone else's) is asked about once when the app starts.

### Fixed

- **A task's first prompt no longer shows as waiting after it was sent.** In a repository with
  no setup, or a quick one, the prompt went to the agent and also stood first in the session's
  queue, to be sent a second time when the turn ended.
- **Run setup again no longer freezes Swarmed** for a task that has a session whose first
  prompt is not waiting in its queue: one already sent, or one a restart took out of it.

## [0.4.3] - 2026-11-24

### Added

- **Paste an image, such as a screenshot, into a message box to attach it.** It shows above the
  text with a × to take it off, in a session's box, the new-task view and a waiting message
  being edited. Sent, it is kept in the session's own folder in Swarmed's data directory,
  readable by you alone, and the agent is given its path. Sent messages show their pasted
  images, which open in Preview when pressed. PNG, JPEG, GIF and WebP up to 20 MB can be
  pasted; anything else, such as TIFF, is refused with the reason. The images are deleted with
  the session, and a fork gets copies of its own.
- **Quote part of an agent's reply into your message, and write what you make of it under the
  quote.** Select words in one reply and press **Quote** beside them, or ⌘⇧L. The quote stands
  in the message box as one token, `[Quote #1: "the first words…"]`, which Backspace and Delete
  take out whole; a message can hold several. The agent is sent the quoted words in numbered
  tags, with your words after them, and the transcript shows them as a quote. A message brought
  back with ↑, or a waiting message opened to edit, has its quotes and large pastes as tokens
  again, no longer pasted out in full.
- **Pin a repository, a task or a session to the top of its list in the sidebar.** Press the pin
  that shows when you point at a row, or ⌘D for the focused row or the open session. A pinned
  row stays above the rows beside it that are not pinned, keeps its pin, and moves only among
  the pinned ones; unpinned, it goes back to the top of the others. A closed session keeps its
  pin for when it is reopened. On the command line: `swarmed project pin`, `swarmed task pin`
  and `swarmed session pin`, and `unpin` for each.
- **A Claude Code session a usage limit stopped can go on by itself when the limit resets.**
  The line that says the limit stopped the turn offers **Continue at 14:05**; pressed, the
  session is sent "Continue where you stopped." at that time, ahead of anything you queued
  meanwhile, and the message says it was sent by itself. A small clock beside the session in
  the sidebar shows the time; pressing it cancels, and so do writing to the session, Stop and
  closing it. **Continue after a limit** in Settings, under Storage and activity, does this for
  every refused turn. Sessions one limit stopped go on a few seconds apart. It is kept when
  Swarmed quits, and one whose time came while Swarmed was closed is sent when the app next
  opens. Nothing asks Claude Code for usage to do it. On the command line:
  `swarmed session continue-at-reset <id>`, `--cancel`, and `swarmed session show` names it.

### Fixed

- **What waits for a task's setup is sent once, and all of it.** A message typed into a session
  made while its task was still being set up stayed waiting after the setup finished. A first
  prompt that `swarmed task setup` or `swarmed task start-anyway` sent while the app was open
  was sent a second time by the app after the session's next turn.
- **Run setup again says why it could not start**, where the button seemed to do nothing: a
  configuration that cannot be read, or a setup already under way.
- **`swarmed task new --prompt` and `swarmed task setup` end when the prompt could not be sent**
  once setup finished, and say that it still waits, instead of waiting for a turn that never
  began.
- **A setup's output no longer loses lines in the panel** when a step changed just as it printed.

## [0.4.2] - 2026-11-24

### Added

- **Chats, with no repository.** New task now starts a chat unless you pick a repository, for a
  question, a repository you have not added, or a folder that is no repository at all. A
  repository's plus still starts a task in it. With no repository there is no branch or
  environment to pick, and the extra agents stay: each gets the same prompt in a chat of its
  own, to compare their answers. Each chat has an empty folder of its own under
  `~/swarmed/.chats` and is listed under General, above the repositories; a repository called
  `general` is a repository like any other. A chat's agent can read and message the sessions in
  every repository, and they can answer it, without reading the chat themselves. When a chat
  turns into work, Carry on in a repository starts a task there with the whole conversation,
  and the chat stays as it was. On the command line a chat is a task of `.chats`, and
  `swarmed session fork --into` carries one on.

- **A task being set up says so, and its prompt is never lost.** A new task's first session is
  there at once, with your prompt waiting until `setup` succeeds, instead of a task with
  nothing spawned yet. The sidebar shows how long setup has run and what it printed last, and
  the session shows each step, what `setup` prints and how long it has taken, also after you
  open something else and come back; a stop button ends it. A setup that fails says why in
  words, signal 9 for what it usually means, and offers Run setup again, Start anyway, Show log
  and Remove task. What a setup printed is kept in a log readable by you alone. `max_setups`
  in a repository's config or environment limits how many setups run at once; the others wait
  and say so. A task whose Swarmed quit mid-setup shows as interrupted. On the command line:
  `swarmed task new --prompt`, `task start-anyway` and `task setup-log`.

### Fixed

- **Removing a task while its `setup` runs no longer reports a failed setup.** Swarmed used to
  find the hook among what runs in the worktree and kill it three seconds later, and then
  said ``the `setup` hook failed with signal: 9 (SIGKILL)``. Removing the task now stops its
  setup on purpose before anything else, with everything the hook started, also what works
  outside the worktree, and gives it time to wind down first, as devenv and the like do; the
  task then goes without a word about its setup. Quitting the app stops a setup under way
  too, and so does Ctrl-C during `swarmed task new` or `swarmed task setup`; the task is then
  left as not set up, to run setup again. What a stack or a removal leaves running is given
  ten seconds to stop, not three, before it is killed.

## [0.4.1] - 2026-11-24

### Added

- **Each session keeps its place, and a button takes you back to the latest message.** Scroll
  up in a session, open another and come back: it is where you left it, on the same message,
  and it stays there as new ones arrive. One you left at the bottom comes back at the bottom,
  with what arrived meanwhile, and keeps following; so does a sub-agent's conversation. While a
  conversation is scrolled up, a round button with a downward arrow shows over its bottom edge,
  above the message box and any waiting messages, when the pointer is over it or it has the
  keyboard: press it to go to the latest message and follow again. Places are not kept after
  Swarmed quits.
- **`swarmed events` prints what happens in every session as it happens**, whichever Swarmed
  does it: the app, another command or this one. Ctrl-C ends it.
- **`swarmed limits` prints your account's usage limits and extra usage** as Claude Code last
  reported them in any session, with how old each figure is. Nothing is asked for it.
- **Codex and OpenCode sub-agents show as Claude Code's do.** While one works it is listed
  under its session; press it to read what it says and does, apart from the conversation, and
  the Background list shows it until it is done, with what it handed back. Its permission
  requests are answered in the session, which names the sub-agent asking. A Codex sub-agent
  can be stopped there; OpenCode has no way to stop one. Shells OpenCode leaves running are in
  the Background list too, without a Stop button, since OpenCode cannot stop one shell, and
  shown running until OpenCode next takes something up by itself, since it does not say when
  a shell ends. `docs/coding-agents.md` lists what each agent shows; Antigravity reports
  neither.
- **Clean up what removed tasks leave behind.** Settings, under Storage and activity, lists
  worktrees of removed tasks not deleted yet, branches kept when their tasks were removed,
  stale worktrees no task owns, and snapshots, each with how many and their size. Each button
  lists exactly what it deletes and asks first; a branch with commits found nowhere else, or a
  folder with changes never committed, is listed apart and goes only if you tick it on its own.
  Nothing of a current task, no branch on a remote and no repository's own checkout is ever
  touched, and the figures above are measured again afterwards. `swarmed cleanup` does the same
  from the command line.
- **A branch prefix of your own.** **Branch prefix** in Settings, under Sessions, is what every
  new task's branch starts with, before a `/` and the task's name: `jd` gives `jd/fix-login`,
  and `jd/feat` gives `jd/feat/fix-login`. Settings shows the branch it gives as you type, drops
  slashes at its ends, and refuses a prefix git would not take in a branch's name, with git's
  reason. Left empty, branches start with your login as before. It applies to every repository
  that keeps the default `branch_pattern`, including the ones added before; a repository with a
  pattern of its own keeps it, and can use `{{prefix}}` in it. `swarmed branch-prefix` shows or
  sets it from the command line.
- **Send a running command or sub-agent to the background, as Ctrl+B does in Claude Code.** A
  `Bash` or `Agent` call that a Claude Code session waits on has a **Send to background** button
  on its line. The call returns at once, the turn goes on, and the work runs on in the
  Background list until it ends; the call's line says it moved. If it had ended already, or
  Claude Code would not, the line says so quietly. `bg` typed while `swarmed session send` runs
  a turn does the same.

- **Remote Control: carry on with a Claude Code session from your phone.** Press the phone at
  the top right of a Claude Code session, or type `/rc`, and the session can be reached from
  claude.ai and the Claude app. What you type there is answered in the same worktree and shows
  in the conversation, marked "Typed on claude.ai". It stays on, at the same place on
  claude.ai, when Claude Code or Swarmed starts again, until you turn it off or close the
  session. It needs Claude Code signed in with a claude.ai subscription, and the conversation is
  kept on Anthropic's servers while it is on. `swarmed session remote-control <id>` does the
  same from the command line.
- **Fold a task in the sidebar.** Press a task's name, or Enter or Space on it, to put its
  sessions and their sub-agents away and keep the other tasks in view, as a repository folds
  its tasks. A folded task still shows how many sessions it has, what the most pressing of them
  needs and whether one has an answer you have not read, and keeps its buttons. It unfolds by
  itself when you open one of its sessions from anywhere else, or start a new one in it.
  Swarmed remembers which tasks are folded until you unfold or remove them.
- **Set the sidebar's levels apart in a custom layout.** A `sidebar` section in `layout.json`
  gives repositories, tasks, sessions and sub-agents each their own text size, weight, and
  colour and background in dark and in light, without changing the rest of the window. A level
  you leave out looks as it does now, and the repository's name now grows with the text sizes
  like everything else. Colours that would be hard to read are refused with the reason. Ask an
  agent to change the layout, or edit it in Settings.
- **One usage notice at the top of the window, in place of lines in the conversation.**
  Claude Code reports each usage window in turn, several times a turn, and the conversation
  repeated two lines with every turn once a limit was near. Now a notice at the top of the
  window, while a Claude Code session is on screen, shows the highest level the account has
  reached: 50%, 75% or 90% of a window used, a limit reached with extra usage paying, or a
  limit that stops requests, with the window and when it resets. It is the same in every
  session. Dismiss it and it stays away in all of them, and after a restart, until a higher
  level is reached, its window resets and climbs back, or a limit first reported since, such as
  one model's weekly limit, reaches a level. The conversation keeps one line only
  where a limit stopped a turn. **Usage warnings** in Settings, under Storage and activity,
  picks the levels: every one (Frequent), from 90% (Rare), or only requests stopped (None).
  An agent can change it for you too.

### Changed

- **`swarmed-core` makes no promise about its API.** It is published so that `swarmed` can be
  installed from crates.io, and each `swarmed` now requires the `swarmed-core` of its own
  version exactly. A release changes its version by what it does to the `swarmed` command and
  the data directory alone, so a patch can change the core's API, as this one does: code of
  your own on `swarmed-core` should require one version exactly.
- **A conversation says what its agent loaded once, and ends only turns that showed something.**
  Claude Code says which model, sign-in, MCP servers and skills it has at the start of every
  turn, and each one drew the same line again; now only the first is drawn, at the top, and
  `swarmed session show` prints it once too. A turn the agent takes by itself that shows
  nothing and spends nothing more, such as one reading a sub-agent's report that came to
  nothing, no longer draws an end line either. **Turn end lines** in Settings, beside Tool
  calls, turns the end lines off; a turn that failed or was stopped keeps its line. An agent
  can change it for you too.
- **Tasks, sessions and cloud tasks are dragged by a grip.** Point at one in the sidebar and
  six dots show before it: drag those to move it among its own kind. Pressing anywhere else on
  the row opens it as before, a click that slips no longer moves it, and dragging across it no
  longer selects its text. The grip's tooltip names the shortcuts that still move the focused
  row from the keyboard. A repository is still dragged by its name.
- **Every dialog has the same head, body and buttons.** Its title and a × are at the top, its
  buttons at the bottom, and only what lies between them scrolls, so a long diff, form or list
  never takes the buttons out of view. Esc, the × and Close or Cancel close any of them, and so
  does a click beside one where nothing would be lost: never with something typed or ticked in
  it, nor while it is at work. The keyboard starts on the first field or the main button; a
  question whose answer cannot be undone starts on keeping things as they are. The inbox,
  search, About and the keyboard shortcuts now have a Close button too.

### Fixed

- **Scrolling up while an answer is being written stays where you scrolled.** A scroll made as
  a piece of the answer arrived was taken back to the bottom, and so was any scroll up that had
  not yet gone 80 pixels, so a slow scroll or a short turn of the wheel could not get away from
  the bottom until the answer was done. Now the first pixels of a scroll up stop following.
- **The window shows what the `swarmed` command or a second Swarmed changes.** A session
  renamed, marked, closed, reopened, deleted, moved or set from the command line, a task or
  session made or removed there, and an action or setting written there or by hand, used to
  show only after a restart, and a session the command ran never moved on screen. Now each
  shows within about a second, and a session run elsewhere follows live, with its status,
  unread state, conversation and notices. A session's agent runs in one place at a time:
  sending to one that another Swarmed runs says so instead of starting a second agent on the
  same conversation, and so does closing or deleting it, or removing its task, while it runs
  there. A message queued for it goes once that run is over.
- **Every Claude Code session shows the same usage, and the newest.** The coins' warning, the
  usage limits and the extra usage were what each session heard after its own last turn, so
  one session warned while another did not, a new session showed nothing, a window that had
  reset still showed as used, and a model's own weekly limit, such as Fable's, was dropped by
  the next turn's report. They are now your account's, as any Claude Code session last heard
  them: a new session shows them before its first turn, a model's limit stays until it resets
  or is asked again, a window past its reset time shows as reset and its warning goes then,
  and each figure says how old it is. Nothing more is asked of Claude Code for it.
- **A Codex command's output shows under its call.** Codex sends what a command prints beside
  the call, and its calls ended with nothing to show.
- **An OpenCode sub-agent's words no longer join the agent's reply.** What a sub-agent said
  during the turn that started it was taken for the agent's own, and its tool calls were shown
  in the conversation.
- **What a Claude Code turn waits on is no longer listed as left running.** Claude Code reports
  a command or sub-agent its call waits on as it starts, and Swarmed took each such report for
  work left in the background: it showed in the Background list, and a sub-agent could keep
  the session's answer back. It is now listed only once it moves to the background.
- **A custom layout with a key Swarmed does not know is still drawn.** One key a newer Swarmed
  wrote into `layout.json` made an older one beside it refuse the whole layout. The key is now
  left out and named with the other configuration problems, and the rest of the layout is drawn.
  Saving a layout with a key Swarmed does not know is still refused, so a misspelt one is caught.
- **The sidebar's + and × line up with the icons beside them.** They were letters, which sat
  where the font put them: off the centre line of the cloud, environments and note, and larger
  and heavier. They are now drawn like those icons, at the same size and weight, in every
  layout and font. Every icon on a row is also centred in the square that lights up under the
  pointer, and the cloud on a repository or task is no longer drawn larger than its neighbours
  in the senior layout or a custom one that enlarges icons.
- **The calls inside an opened run of folded tool calls show whether each is open.** With tool
  calls folded, every call listed in an opened run had its arrow pointing down, open or not,
  and it did not change when the call was pressed. Each call's arrow now points right while it
  is closed and down while it is open. While the run is still at work, a call in it that failed
  no longer pulses as if running, and the run's count of failed calls is red as it is once the
  run is done.
- **A merged pull request no longer looks like a draft.** Its number in the sidebar was drawn
  faint, as a draft with no checks is, and on a badge that small the two looked the same. A
  merged pull request is now lilac, as GitHub draws it in purple, in the sidebar and in the
  task panel, in every palette, light and dark. A draft stays grey until its checks say
  otherwise, and a closed one stays faint with a dashed border.

## [0.4.0] - 2026-11-23

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
- **`swarmed-core`: what code built against 0.3 has to change**, hence 0.4.0.
  `NewTaskOptions::environment` is an `EnvironmentChoice` rather than an `Option<&str>`
  (`Named` for one environment, `Own` for the repository's own setup, and `Default`, the
  default, for the repository's default environment), so `NewTaskOptions` is no longer `Copy`. `Environment` has
  a `max_stacks` field, and `Error` a `NoRoomForAStack` variant, so a struct literal of the
  one or an exhaustive `match` on the other needs them.

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

[Unreleased]: https://github.com/ryckakas/swarmed/compare/v0.4.11...HEAD
[0.4.11]: https://github.com/ryckakas/swarmed/compare/v0.4.10...v0.4.11
[0.4.10]: https://github.com/ryckakas/swarmed/compare/v0.4.9...v0.4.10
[0.4.9]: https://github.com/ryckakas/swarmed/compare/v0.4.8...v0.4.9
[0.4.8]: https://github.com/ryckakas/swarmed/compare/v0.4.7...v0.4.8
[0.4.7]: https://github.com/ryckakas/swarmed/compare/v0.4.6...v0.4.7
[0.4.6]: https://github.com/ryckakas/swarmed/compare/v0.4.5...v0.4.6
[0.4.5]: https://github.com/ryckakas/swarmed/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/ryckakas/swarmed/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/ryckakas/swarmed/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/ryckakas/swarmed/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/ryckakas/swarmed/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/ryckakas/swarmed/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/ryckakas/swarmed/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/ryckakas/swarmed/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/ryckakas/swarmed/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/ryckakas/swarmed/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ryckakas/swarmed/releases/tag/v0.1.0
