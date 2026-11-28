# Features

[← Back to README](../README.md)

Everything Swarmed does, in short, with a link to where each part is explained in full.

## Tasks and worktrees

- **A task is a branch, a worktree and a stack.** Your setup hook runs in each new worktree,
  your dev server keeps running beside it, and every task gets ports of its own, so two of them
  can run the same app at once. See [Hooks and the stack](configuration.md#hooks-and-the-stack)
  and [Ports](configuration.md#ports).
- **Environments and snapshots.** A repository can be set up more than one way, such as with or
  without Docker, and a new task starts from a copy of the files git ignores in one set up
  earlier, so even a large `node_modules` takes seconds. See
  [Environments](configuration.md#environments) and [Snapshots](configuration.md#snapshots).
- **Agents keep to their task's branch.** Work that needs a branch of its own goes into a new
  task the agent starts, so skills that make a worktree or branch of their own are best left
  out: [see why](coding-agents.md#skills-that-make-their-own-worktree-or-branch).
- **No files of its own in your repository.** Swarmed keeps its configuration and state in a
  folder of its own. The repository gains only the tasks' branches (and an empty first commit
  if it had none), and their worktrees live under `~/swarmed`, beside the chats' folders. See
  [Where things are](configuration.md#where-things-are).
- **Finished tasks are put away, not deleted.** Their agents and stack stop and the worktree,
  branch, notes and sessions stay, by hand or by themselves once the pull request merged or they
  sat idle, if you turn that on. If you turn that on too (`put_away_remove_days`), a task put
  away for a number of days is then removed, as a forced **Remove…** does. See
  [Finished tasks](configuration.md#finished-tasks).

## Agents

- **Any agent, on its own login.** Claude Code runs as you set it up, with your login, settings
  and plugins. Codex, OpenCode, Antigravity and any agent that speaks the Agent Client Protocol
  are installed and set up from Settings → Agents. Swarmed never signs you in to anything and
  keeps no GitHub token: it uses each agent's own login and your `gh`. See
  [The agents Swarmed knows](coding-agents.md#the-agents-swarmed-knows).
- **One pool of MCP servers and skills.** A server set up for Claude Code, Codex or OpenCode
  reaches the other agents too, and so does a skill, without copying anything into their
  configuration. See [MCP servers: one pool](coding-agents.md#mcp-servers-one-pool) and
  [Skills](coding-agents.md#skills).
- **repolore, built in.** [repolore](coding-agents.md#repolore-built-in), which searches your
  own GitHub history, can be built in: Swarmed installs it and gives every session one copy of
  it.
- **Supervision per session.** Ask, Auto or Plan, the model and its effort are set per session
  and can change mid-session. Permission requests, questions and plans are answered in the
  conversation. See [Supervision](coding-agents.md#supervision).

## Agents working together

- **Agents start sessions and tasks.** Every agent gets tools to list, read and message the
  other sessions, start new ones beside it or in a task of their own, and close them once they
  reported back. See
  [What Swarmed gives every agent](coding-agents.md#what-swarmed-gives-every-agent).
- **Who works for whom.** A session can work for another, nested to any depth and across tasks;
  the sidebar shows the tree and the header counts everyone below. See
  [Who works for whom](coding-agents.md#who-works-for-whom).
- **Roles.** Architect, planner, implementer or reviewer: an architect splits a feature into
  tasks, a planner hands each change to implementers, a reviewer reports or gets its findings
  fixed. Each role can have a model and effort of its own. See [Roles](coding-agents.md#roles)
  and [Role defaults](configuration.md#role-defaults).
- **One prompt, several agents.** Send the same prompt to several agents or models at once, each
  in a task of its own, and compare what two tasks changed.
- **Cloud and phone.** Hand work to a Codex cloud agent or a Claude Code cloud session, from a
  session too, as a brief its agent writes or with the whole conversation. Carry a Claude Code
  session on from your phone with
  [Remote Control](coding-agents.md#remote-control-carry-on-from-your-phone).

## Sessions

- **Sessions outlive a restart.** Every session is kept on disk with its whole conversation, and
  its task's worktree stays where it was. Write to a session afterwards and its agent carries
  on where it left off: Claude Code, Codex and OpenCode pick up their own conversation.
- **Fork from any message.** The fork starts with the conversation up to there, on the same
  agent or another, so an agent that cannot resume can carry on all the same.
- **See what one turn changed.** The Changes tab shows the whole task or one turn of a session,
  edits made by shell commands included. See
  [What a turn changed](configuration.md#what-a-turn-changed).
- **Take a turn back.** A session goes back to before one of your messages, its files too if
  you choose, and the message waits in the box to be changed and sent again. See
  [Taking a session back](coding-agents.md#taking-a-session-back-to-before-a-message).
- **Messages queue while the agent works.** Send one now, or tell Claude Code without stopping
  it; ↑ brings back what you sent.
- **Attach, paste and quote.** Pick or drop files, paste a screenshot, or quote part of a reply
  and write what you make of it under it.
- **Mentions.** Type `@` to point the agent at another session, or `#` at a pull request or an
  issue. See [Mentioning another session](coding-agents.md#mentioning-another-session) and
  [Mentioning a pull request or an issue](coding-agents.md#mentioning-a-pull-request-or-an-issue).
- **A terminal in each session.** What you ran there since the agent's last turn goes with your
  next message, so it can act on a command it asked for without you pasting anything.
- **Work left running is never hidden.** Sub-agents and commands an agent left in the background
  are listed, readable and stoppable, and a running command or sub-agent can be sent to the
  background as Ctrl+B does in Claude Code. See
  [Sub-agents and work left running](coding-agents.md#sub-agents-and-work-left-running).
- **Search, notifications and an inbox.** Search what was said in any session; unread answers are
  marked, and notifications and a count on the Dock icon say when a session needs you. ⌘I lists
  everything that waits.
- **Snooze a session until a time.** ⌘⇧D, the clock on a session's row or in its header: in an
  hour, this evening, tomorrow morning, next Monday or a time of your own. Until then it waits
  in its task's Snoozed fold, out of the list, the inbox and the Dock's count, and its agent is
  not stopped. It comes back early when you write to it, it asks for something or it fails, and
  on time by itself; `swarmed session snooze` and `wake` do the same from the command line.

## Answers you can see

- **HTML in the conversation.** An agent can answer with a chart, a table or a mock-up, shown in
  a sandbox with no way out to the app or the network. See [HTML views](architecture.md#html-views).
- **Apps from MCP servers.** Apps that MCP servers attach to their tools show under the call,
  sandboxed the same way. See
  [Apps MCP servers attach to their tools](coding-agents.md#apps-mcp-servers-attach-to-their-tools).
- **Images tools return.** A screenshot from a browser tool, or an image the agent read, shows
  under the call. See
  [Screenshots and other images tools
  return](coding-agents.md#screenshots-and-other-images-tools-return).
- **Math and code.** Formulas are drawn by KaTeX, which comes with the app, and every code block
  and tool call can be copied with one press. See
  [Math in replies](architecture.md#math-in-replies).

## A browser per task

- **A Chrome of each task's own.** Shown live beside its sessions: click and type in its page,
  read its console. Its profile is the task's own, so what you sign in to there stays with the
  task and never reaches another's. See [The task's browser](configuration.md#the-tasks-browser).
- **Agents browse where you watch them.** Agents' own browser tools, Playwright MCP and Chrome
  DevTools MCP, act in it instead of opening windows. See
  [The task's browser and your browser
  tools](coding-agents.md#the-tasks-browser-and-your-browser-tools).
- **Point at an element.** Pick one on the page to point the agent at exactly that one, with a
  picture of it. See
  [Pointing at an element of the page](coding-agents.md#pointing-at-an-element-of-the-page).

## From issue to pull request

- **Start a task from a GitHub issue,** read with its comments, and the pull request Swarmed
  writes closes it. See [Issues](configuration.md#issues).
- **The task panel.** The task's diff, comments on lines sent back to the agent, its files, a
  commit whose message the agent writes, push, and a pull request whose title and description
  are written for you, with its checks.
- **Swarmed follows the pull request.** Failed checks, a review, a conflict or a merge each show
  as a line in the conversation and a notification, and a failure goes to the agent with one
  press. See [Pull requests](configuration.md#pull-requests).
- **Merge from the panel,** now, or once its checks pass with auto-merge.

## Chats

- **Chats, with no repository.** New task starts a chat unless you pick a repository: for a
  question, a repository you have not added, or several agents' opinions on one thing. Each
  chat gets an empty folder of its own and is listed under General.
- **A chat reaches your repositories.** It can read and message their sessions, which can answer
  it, and when it turns into work, it carries on in a new task of a repository with the whole
  conversation.

## Usage and cost

- **Your Claude Code usage.** The coins show your usage limits and extra usage as they are when
  you ask, and warn before a limit is reached. See
  [Usage limits and extra usage](coding-agents.md#usage-limits-and-extra-usage).
- **Go on when a limit resets.** A session a usage limit stopped can carry on by itself once it
  resets. See [When a limit stops a session](coding-agents.md#when-a-limit-stops-a-session).
- **Tokens per turn.** A Claude Code turn counts the tokens it uses while it runs, and Claude
  Code, Codex, OpenCode and Antigravity say what a turn used when it ends. See
  [The tokens a turn uses](coding-agents.md#the-tokens-a-turn-uses).
- **What it costs.** What the agents report their work cost, by day and by task, turns by day and
  model in charts, and what Swarmed takes up on disk.

## Made yours

- **Looks.** Six palettes, swarm words or plain ones, the classic, senior and custom layouts with
  sizes, fonts and colours of your own, and a stylesheet of your own for the rest. See
  [The custom layout](configuration.md#the-custom-layout) and
  [Your own stylesheet](configuration.md#your-own-stylesheet).
- **Your own setup.** The terminal and the status line, buttons that send a prompt or run a
  command, saved prompts, rules for every agent, notes on a repository and a task, and keyboard
  shortcuts. See [Settings](configuration.md#settings).
- **By hand, in files, or by an agent.** Change them in the app, in their files or with
  `swarmed`, or ask an agent: it changes the looks at once, apart from your stylesheet, which
  only you change, and proposes rules, environments and commands for your yes.

## The `swarmed` command

- **The same core and data as the app,** from a terminal, a script or an agent's shell. What it
  changes shows in a running app within about a second. See [The `swarmed` command](command.md).
