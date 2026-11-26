# Coding agents

[← Back to README](../README.md)

Sessions run on Claude Code unless you pick another agent. Swarmed never signs in to an agent and
never reads the login it keeps: sign in with the agent's own command first, and Swarmed runs it
on that login.

## The agents Swarmed knows

| Agent | Runs | Installed from Settings → Agents with | Signing in |
| --- | --- | --- | --- |
| Claude Code | `claude` | `curl -fsSL https://claude.ai/install.sh \| bash` | run `claude` once and follow `/login` |
| Codex | `codex-acp` | `brew install codex-acp` | `codex login`, or `codex-acp login` |
| OpenCode | `opencode acp` | `brew install anomalyco/tap/opencode-v2` | none: it uses the API keys stored in Settings |
| Antigravity | `agy acp` | `curl -fsSL https://antigravity.google/cli/install.sh \| bash` | run `agy` once and sign in with Google |

Settings shows the command before running it. Installing Codex, OpenCode or Antigravity also
writes it into `agents.toml` with its full path and the models to offer: Codex's and
Antigravity's are asked of the agent, and OpenCode starts with a few, with OpenRouter's public
list to pick more from in Settings. `swarmed agent` does the same from a terminal.

An OpenCode session left on "Default model" runs on the first model listed for it, which the
model menu names. Left to itself OpenCode would run on the newest model it knows of, which
changes whenever one comes out. A `model` set in your own `opencode.json` is the exception: it
stays OpenCode's default, and Swarmed picks none. Swarmed reads that file when it starts and
whenever the agents in Settings change.

An agent other than Claude Code is offered for a session only while its program can be found: on
your shell's `PATH`, or in `/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin` or
`~/.opencode/bin`.

## Claude Code

Claude Code is built in. Swarmed runs your installed `claude` with your own login, settings and
plugins, and speaks to it in its line-delimited JSON. It gets none of the API keys stored in
Swarmed, and no variable from a task's environment whose name starts with `ANTHROPIC_` or
`CLAUDE`. From the command line, `--auth api-key` runs it on the `ANTHROPIC_API_KEY` of the shell
you run `swarmed` from, and `--auth inherit` passes that shell's environment on unchanged, for
Bedrock or Vertex.

### Usage limits and extra usage

The card that opens over the context figure in the status line shows the following for a Claude
Code session:

- **Usage limits:** how much of each window (5-hour, weekly, and a model's own weekly limit) is
  used, and when it resets.
- **Extra usage:** what is spent beyond your plan this month, against your monthly cap.
- **Spent in this session:** what the session's own turns cost.

The usage limits and the extra usage are your account's, not a session's, so every Claude Code
session shows the same: the newest figures any of them heard. A new session shows them before
its first turn, and they are kept when Swarmed is opened again. Every Claude Code session runs
on your own Claude Code sign-in, so they all report the same account. The one exception is a
session you run from the command line with `--auth inherit` in a shell set up for another
account: Swarmed cannot tell, and its figures show under every session too.

Swarmed only learns these figures from what Claude Code says while it works. It never asks for
them on a timer or when the card opens. So each says how old it is ("As of 14:48"), and they can
be out of date:

- The 5-hour and weekly limits are as Claude Code reported them after the last turn of any
  Claude Code session.
- A model's own weekly limit, such as Fable's, is named only when Claude Code is asked. After
  that, turns that use the model update it too: Claude Code reports it after such a turn
  without the model's name, and Swarmed shows it on the model's row. Until Claude Code has been
  asked once, that report shows as **Weekly model**. Turns that do not use the model leave it
  as it was. It is not extra usage, which is money against your monthly cap, however close
  the two percentages may be.
- A window whose reset time has passed is shown as reset, at 0%, until a report says how much
  of the new window is used. The warning on the coins goes at that time, with nothing asked.
- The extra usage is as it stood when Claude Code last started for a session, or last ran a
  turn on extra usage. Claude Code starts with a session's first message after Swarmed opens,
  or after the session was stopped.
- Usage from other devices or claude.ai shows up after the next turn of any Claude Code session.

For figures as they are now, press the coins at the top right of a Claude Code session. They
open an overview of the same figures, with a larger bar for each, and ask the session's Claude
Code for them as they open, starting it if it is not running; **Ask again** asks once more.
Claude Code keeps the figures it fetches from claude.ai for a minute and answers from them, in
every Claude Code on your machine, your terminal's included. So when it fetched them less than a
minute before you ask, the question waits until it would fetch them again, at most a minute,
and the overview counts down to it: what you get is fetched after you asked, such as right after
your usage was reset. While a question is under way the coins pulse, and the overview says it is
refreshing, with a bar that fills as the wait runs out and moves while Claude Code is asked. The
figures stay in full colour meanwhile: they are the latest any session heard, and the overview
says so. The answer lights them up for a moment, and the overview says when it came, to the
second, and whether the figures are new or the same as before.
Nothing is sent to the model, so it costs nothing, and nothing is added to the conversation.
Typing `/usage` in the session asks the same and shows the answer in the conversation. Either
way, what Claude Code answers is kept, and every session's card and coins show it. Other agents
report no usage, so their sessions have no coins and their card shows only what they spent.
`swarmed limits` prints the same figures, with how old each is, and asks nothing.

Once a window is 75% used, which is when Claude Code itself starts to warn, the coins carry a
small yellow triangle, red once a limit is used up, and their label names the window. The bars
are yellow and red likewise. Like the card, the triangle follows the figures last heard, so it
appears after the turn that passes 75%, in every Claude Code session, and goes when the window
resets.

While a Claude Code session is on screen, a notice at the top of the window says how far the
account's usage has come, with the window, how much of it is used and when it resets. It is
worked out from the same figures as the coins, the newest any session heard, so it is the same
in every session, and it shows one level, the highest any window has reached:

1. 50% of a window used;
2. 75% used;
3. 90% used;
4. **billed**: a limit is reached and extra usage can pay for the work, which goes on;
5. **stopped**: a limit is reached and nothing can pay, so Claude Code's requests are refused.
   For one model's own weekly limit, such as Fable's, only requests to that model are.

Whether extra usage can pay is what Claude Code last said of it: after a turn, or when asked,
as extra usage turned on and its monthly cap not reached. It need not have paid for a turn yet.
Once a turn has said it cannot, out of usage credits for instance, asking does not undo that,
since what asking gives is your setting: the next turn's word does.
A limit counts as reached once a report after a turn says so. The figure Claude Code gives when
asked is a whole percent, so 100% there may be 99.5%: the notice then says the limit is all but
used up, and nothing is limited yet.

Claude Code reports one window at a time, after each turn and often several times in one, so
nothing is said per report: a report only raises or lowers the level. Dismiss the notice and it
stays away in every session, and after Swarmed starts again, until a window reaches a higher
level, a window that has reset since climbs back to one, or a window first reported after you
dismissed it, such as one model's weekly limit, reaches one. The dismissal is kept on this Mac.

The conversation says nothing of usage, except where a limit stopped a turn with nothing to pay
past it: that turn keeps one line, which says why the agent stopped. Conversations recorded
before show no other usage lines either.

### When a limit stops a session

The work stays where the limit stopped it until a message goes to the session. Swarmed can send
that message for you when the limit resets. The line of the latest turn a limit stopped offers
**Continue at 14:05**, with the time the limit resets, for a weekly limit too. Pressed, the
session goes on by itself at that time:

- **What is sent:** "Continue where you stopped.", as if you had typed it, first in the
  session's queue, so that what you queued meanwhile goes after it. The message says it was sent
  by itself. Where two limits stopped the turn, it waits for the later reset.
- **What cancels it:** writing to the session, Stop, closing or deleting it, removing its task,
  or pressing the clock that shows the time beside the session in the sidebar, or **Cancel**
  under the limit's line.
- **Several sessions:** those one limit stopped go on a few seconds apart, not all at once.
- **When Swarmed is closed:** it is kept with the session, and one whose time came while
  Swarmed was closed is sent when the app next opens, unless the usage Swarmed last heard says a
  limit still refuses requests: it then waits for that one to reset. One model's weekly limit
  does not count for that, since it stops that model alone. Only the app sends it,
  not the command line, and with two apps open only the one opened first does.

**Continue after a limit** in Settings, under Storage and activity, does this for every turn a
limit stops, without asking. It is off unless you turn it on, and an agent cannot turn it on
for you.

Nothing asks Claude Code for your usage to do any of this: the reset time is the one Claude
Code gave when it refused the turn. If the limit has not reset after all, the turn is refused
again and says so, with the new time; with the setting on, the session waits for that one. A
reset time that has already passed sets nothing. Only Claude Code reports when a limit resets,
so other agents' sessions offer none of this. `swarmed session continue-at-reset <id>` sets it
from the command line, `--cancel` cancels it, and `swarmed session show` says when it goes.

**Usage warnings** in Settings, under Storage and activity, chooses which levels the notice
says:

- **Frequent**, the default: every level.
- **Rare**: from 90% used: 90%, billed and stopped.
- **None**: only stopped.

The coins and the card show the same whatever you choose. An agent can change it for you with
`configure`.

### Sending a command or sub-agent to the background

In Claude Code's own terminal, Ctrl+B moves a long command, or a sub-agent, that the turn waits
on into the background. In Swarmed, a running `Bash` or `Agent` call of a Claude Code session has
a **Send to background** button on its line while the session works. The call returns at once,
the turn goes on, and the command or sub-agent runs on: the call's line then says "moved to the
background", and the task panel's Background list shows it, with a way to stop it, until it ends.
A sub-agent sent there keeps the session's answer back until it is done, as one Claude Code
started in the background does.

If it had already ended, or was in the background already, nothing breaks: the line says it was
not sent. So does a Claude Code that refuses, such as one started with
`CLAUDE_CODE_DISABLE_BACKGROUND_TASKS`, or a version too old for it, which is then to be
updated. Work that a call waits on is not listed in the Background list until it moves there.
Other agents have no such thing, and their calls have no button. From the command line, type
`bg` and Enter while `swarmed session run` or `session send` shows a turn.

### Remote Control: carry on from your phone

A Claude Code session can be reached from claude.ai and from the Claude app on your phone. Press
the phone at the top right of the session and **Turn on**, or type `/rc` in it. The session is
then listed with your other sessions in the Claude app, and the panel gives its address on
claude.ai to open or copy. What you type there is answered by the same Claude Code, in the
task's worktree, and shows in the conversation here marked "Typed on claude.ai". Permission
requests can be answered on either side.

- Claude Code has to be signed in with a claude.ai subscription; an API key does not do. On a
  Team or Enterprise plan, an admin may have to allow Remote Control.
- While it is on, the conversation is kept on Anthropic's servers as well.
- It stays on until you turn it off (**Turn off**, or `/rc off`), close the session or remove
  its task. When
  Claude Code stops, for a change of effort or because Swarmed quit, it is turned on again as
  soon as Claude Code starts for the session, at the same place on claude.ai. When Swarmed
  starts, it starts Claude Code for every open session that had it on.
- The phone turns green while the session can be reached, and yellow while it connects or
  waits for Claude Code to run again.
- A permission mode changed on claude.ai is not shown here, and views show as code there.

Other agents have no Remote Control. `swarmed session remote-control <id>` does the same from the
command line for as long as it runs. With `--off` it keeps later runs of the agent from turning
it on; a session the app has running stays reachable until it is turned off there.

## Any other agent

Any agent that speaks the [Agent Client Protocol](https://agentclientprotocol.com) can be added
in `agents.toml` in the data directory:

```toml
[goose]
name = "Goose"            # shown in the agent menu; the id if left out
command = "goose"         # a program on the PATH, or a full path
args = ["acp"]
models = ["provider/model"]   # offered in the model menu

[goose.env]               # extra environment, over the stored API keys
# SOME_SETTING = "..."
```

A key Swarmed does not know, or an entry named `claude`, is an error that keeps Swarmed from
opening, so check the file after editing it. Swarmed reads it when it starts and whenever
Settings changes an agent, so restart it after editing by hand.

Agents other than Claude Code get the API keys stored in Settings → API keys (OpenRouter, OpenAI,
Anthropic, Google Gemini) as `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY` and
`GEMINI_API_KEY`. The keys live in the macOS Keychain, and Swarmed never shows them again.

## Supervision

| Setting | Claude Code runs in | Another agent |
| --- | --- | --- |
| Ask | `acceptEdits`: edits go through, everything else asks | its read-only, ask or default mode |
| Auto | `auto`: Claude Code decides what is safe | its agent or auto mode |
| Plan | `plan`: it reads, then presents a plan to approve | its plan, read-only, ask or default mode |

Swarmed picks the first of those modes the agent has; the app offers Plan for Claude Code alone.
An agent with neither an agent nor an auto mode has its permission requests allowed by Swarmed
in Auto, so it runs unsupervised there: use Ask for it. Permission requests and questions are
answered in the conversation. Approving a plan moves the
session to the default supervision from the settings, or to Ask if that default is Plan itself.
Models and effort are chosen per session, and can change mid-session.

## Taking a session back to before a message

**Edit from here** on a message you sent, or `/rewind` (also `/checkpoint` and `/undo`) in a
Claude Code session, takes the session back to just before that message and puts the message in
the box, for you to change and send again. Nothing is sent by itself. You choose:

- **Conversation only:** the files stay as they are now.
- **Conversation and files:** the worktree goes back to how it was before that message, from the
  checkpoint taken then (see [What a turn changed](configuration.md#what-a-turn-changed)).

What the message started, and everything after it, stays in the transcript, folded under a line
that says where the session was taken back to: costs and search still count it. It no longer
counts as the conversation: a fork of the session leaves it out.

| Agent | What it remembers afterwards |
| --- | --- |
| Claude Code | Resumes its own conversation where the turn before the message ended, and does not remember what came after. |
| Any other agent | Starts a new conversation, and is told what was said up to the message with your next one: what was said, not what it read or ran. |

Claude Code starts a new conversation the same way when there is no point to resume at: before
the first message, after a turn that failed before its first word, or in a conversation from
before Swarmed noted those points.

**What goes back with the files, and what does not:**

- Only the files of the worktree. **Commits stay on the branch**: the branch is never moved, so
  the files then differ from it, and resetting it is yours to do. The line says how many commits
  were made since. Nothing outside the worktree comes back either: a push, a pull request, a
  database, a package installed elsewhere.
- Files git ignores, and new files over 10 MB that checkpoints leave out, are neither restored
  nor deleted. One in the way of a file that would come back refuses the whole, and nothing is
  changed: it is in no checkpoint, so it could not be put back.
- If git cannot write a file part-way through (a full disk, a folder that takes no new file),
  what it had written is put as it was and nothing is taken back.
- The worktree as it was is kept first: **Put the files back** on the line restores it, as long
  as nothing changed the files since.
- The other sessions of the task are not told. Their agents find the files as they are when they
  next read them.

**Refused, with the reason, rather than tried:** while the session is at work, closed, run by
another Swarmed, or has something it left running. Taking the files back is refused as well while
any session of the task is at work or run elsewhere, anything its agents left running still
runs, its setup runs, or a rebase, merge, cherry-pick or revert is under way in the worktree; and
for a message no checkpoint was taken before (older than the latest 50 turns, sent before
checkpoints existed, copied into a fork, or in a chat). The conversation alone can still go back.

Swarmed uses its own checkpoints for every agent, not Claude Code's file checkpointing, which
does not undo what shell commands changed. From the command line:
`swarmed session rewind <id> <message> [--files]` (`--check` says what it would do) and
`swarmed session put-back <id>`.

## Sub-agents and work left running

Agents can start sub-agents for parts of their work, and leave commands running past a turn.
Swarmed lists a sub-agent under its session while it works: press it to read what it says and
does, apart from the conversation. The task panel's Background list shows the sub-agents and
commands left running, with a Stop button where the agent has a way to stop them.

| Agent | Sub-agents | Commands left running |
| --- | --- | --- |
| Claude Code | listed, opened, stopped | listed, stopped, output readable |
| Codex | listed, opened, stopped | not shown |
| OpenCode | listed, opened; cannot be stopped from Swarmed | listed; cannot be stopped from Swarmed |
| Antigravity | not shown apart | not shown |

- **Codex** does not show the call that starts a sub-agent, so Swarmed shows one for it,
  named after the sub-agent, with what it was asked. A sub-agent that Codex sets to work
  again is shown again. Codex reports commands left running only to JetBrains' own client,
  in a form that changes everything else it sends, so Swarmed does not ask for it.
- **OpenCode** offers no way to stop one sub-agent or one shell. It does not say when a shell
  ends either: a shell is shown running until OpenCode next takes something up by itself,
  which is what a shell's end wakes it for, or until it stops. With several shells running,
  all of them are then shown as ended.
- **Antigravity** reports neither in a form Swarmed can read: its sub-agents and long commands
  are tool calls in the conversation like any other.

A sub-agent's permission requests are answered in its session's conversation, which names the
sub-agent asking. While a sub-agent works on after its session's turn, the session waits to
tell you it is done until the sub-agent is too. A command left running does not hold it back:
a dev server may never end. Reading what a command printed is Claude Code's alone.

A session whose agent has something running in the background never looks merely idle in the
sidebar: its mark gets a ring, a clear one that breathes slowly while sub-agents are at work and
a faint one while only commands run. Pointed at, the mark says what runs, and a folded task or
repository shows the ring of what runs in it, below working and above idle. A command that runs
for good, such as a dev server, keeps its session marked for good: stop it from the Background
list once you no longer need it. The session is still idle, and a message goes at once.

## Screenshots and other images tools return

When a tool returns an image, such as a screenshot from Playwright MCP's
`browser_take_screenshot` or Chrome DevTools MCP's `take_screenshot`, or an image file the agent
read, Swarmed keeps it and shows it under the call. Press it to see it large, and **Open** to
show it in your system's viewer. The last image of a turn stands again at the end of the
agent's reply, so a run of folded tool calls still shows it; the images of a folded run also
stay in view under its line. With tool calls hidden, their images are hidden too.

- **Which agents:** Claude Code, and every ACP agent that puts an image in a tool call's
  content, as the protocol has it.
- **What is kept:** PNG, JPEG, GIF and WebP, up to 10 MB each and 8 in one result. Anything
  else is named in the result, as `[image not shown: …]`, and not kept.
- **Where:** in `attachments/<session id>/` in the data directory, beside the images pasted into
  the session, readable by you alone. They are deleted with the session, its task or its
  repository, and Settings' Storage and activity counts them.
- **Who sees them:** you. An image can show anything a page showed, a password included, so
  it is never logged, nor handed to another session, a fork, the cloud or search. A fork
  carries what was said, not what tools returned.

## The task's browser and your browser tools

Agents look at pages with the browser tools they already know: Playwright MCP, Chrome DevTools
MCP, or a script. Each of those launches a Chrome of its own, a window that takes your focus or
a headless one nobody sees, on a fresh profile signed in nowhere. In Swarmed they act in the
task's own browser instead ([The task's browser](configuration.md#the-tasks-browser)), which you
watch beside the conversation, on the task's profile, where what you signed in to stays signed
in. Swarmed adds no browser server you did not set up, and no browser tool of its own.

- **An address per session.** Each session's agent is started with `SWARMED_BROWSER`, an
  address on `127.0.0.1` that Swarmed listens at. The task's Chrome starts when a tool first
  connects to it, so a session that never browses starts nothing. A script connects with
  `chromium.connectOverCDP(process.env.SWARMED_BROWSER)` in Playwright, or
  `puppeteer.connect({ browserURL: process.env.SWARMED_BROWSER })`.
- **Your browser servers are pointed at it**, recognised by what they run, never by the name
  you gave them: Playwright MCP (`@playwright/mcp`, `playwright-mcp`, `mcp-server-playwright`)
  is given `--cdp-endpoint` with the address, Chrome DevTools MCP (`chrome-devtools-mcp`)
  `--browser-url`, and loses `--channel`, `--executable-path` and `--user-data-dir`, which it
  refuses beside it. Nothing else about them changes, and nothing is written into your
  configuration.
- **Every agent is told** that the task has a browser you see, to use it and never launch one
  of its own, to open a tab of its own rather than act in one already open, and which of its
  servers already act on it.
- **`show_browser`**, on Swarmed's own server, opens the Browser pane on an address, or on the
  task's running app, in a tab of the session's own, starting the browser if need be. It
  answers with the address, the tabs and which of them is the session's. It only shows:
  clicking, typing, reading and screenshots stay with the agent's own tools. The pane opens
  when the session on screen works in that task.
- **The pane follows the agents.** Each tab says which session's tools last acted in it, and
  when a session's tools go to work in another tab than the one shown, the pane goes there. A
  tab you went to yourself is left shown while the tools go on in the same one. Screenshots
  they take show in the conversation as any tool's images do.

Which servers are pointed:

| Where the server is set up | Claude Code | Codex, OpenCode, Antigravity and other ACP agents |
| --- | --- | --- |
| Claude Code: your servers, yours for the repository, the repository's `.mcp.json` ones you approved, your plugins' | pointed: Swarmed hands Claude Code a pointed copy under the same name, which takes the place of its own | pointed, as Swarmed hands them over |
| Codex's or OpenCode's own configuration | pointed, as Swarmed hands them over | for that agent itself: not pointed |
| Swarmed's own, in Settings → MCP servers | pointed | pointed |

- **Not pointed, left as they are:** a server that already says where its browser is
  (`--cdp-endpoint`, `--extension`, `--browser-url`, `--ws-endpoint`, `--auto-connect`, or
  `PLAYWRIGHT_MCP_CDP_ENDPOINT` in its variables); Playwright MCP on Firefox or WebKit
  (`--browser`, or `PLAYWRIGHT_MCP_BROWSER`); Playwright MCP standing in for a device
  (`--device`, `--mobile`, or their variables), which it refuses to do in a browser it
  connects to; a
  server reached over HTTP, which runs apart and takes no argument from Swarmed; other browser
  servers, such as Puppeteer's or Browserbase's; and a server of the repository's `.mcp.json`
  that Claude Code has not been allowed to start, which would start unasked if Swarmed handed
  it over.
- **An agent's own server stays its own.** Codex keeps a server of its own configuration even
  when Swarmed hands it one of the same name, so its own Playwright MCP opens its own browser.
  To have Codex act in the task's browser, take the server out of `~/.codex/config.toml` and
  keep it in Claude Code's configuration or in Swarmed's: Swarmed then hands Codex a pointed
  copy. The same holds for OpenCode.
- **Tried with** Claude Code 2.1.332, Playwright MCP 0.0.85 and Chrome DevTools MCP 1.12.0,
  Antigravity with the Playwright MCP Swarmed handed it, and codex-acp 2.7.0.

What a page says is data: browser tools hand page text to the agent as they always do, and
Swarmed adds nothing to it and logs none of it. A login you made in the task's browser can be
used by the task's agents; that is what lets them check pages behind it.

## MCP servers: one pool

When a session starts, Swarmed reads the MCP servers set up for Claude Code, Codex and OpenCode,
and in Swarmed itself, and gives the session's agent the ones it lacks by name, as they are
configured, secrets included. Nothing is copied into an agent's own files.

| From | Read from |
| --- | --- |
| Claude Code | `~/.claude.json` (your servers, and yours for the repository), the repository's `.mcp.json`, and your user-scope plugins |
| Codex | `~/.codex/config.toml` |
| OpenCode | `~/.config/opencode/opencode.json` or `opencode.jsonc` |
| Swarmed | `mcp.toml` in the data directory, set up in Settings → MCP servers |

- Claude Code, Codex and OpenCode are not given a server by a name they already have, switched
  on or off. The configuration of other agents is not read, so they get the whole pool.
- Servers set up in Swarmed go to every agent, Claude Code included, in place of any of the same
  name. Their secrets are kept in the Keychain, and `mcp.toml` holds only their names.
- A server from the repository's `.mcp.json` goes to other agents only after you approved it in
  Claude Code, in your own settings rather than a file the repository tracks, and never once you
  rejected it.
- An agent that cannot take HTTP servers gets only the ones it runs as programs.
- `shared_mcp_servers = false` on an entry in `agents.toml` gives that agent only Swarmed's own
  servers.

`swarmed mcp` lists the pool: the servers set up in Swarmed, Claude Code, Codex and OpenCode.

### Apps MCP servers attach to their tools

Some MCP servers attach an interactive app to a tool, following the MCP Apps standard: a map
for a geocoding tool, a chart for a query, a form to pick from. When the agent calls such a
tool, Swarmed reads the app from the server once the call has its result and shows it under the
call, with the server's name above it. The call itself keeps its arguments and result, so you
can read them when the app shows nothing.

- **Which agents:** Claude Code 2.1.332 or later, which says it can read an app and call a tool
  for one. Older versions and other agents show the call as before.
- **What an app may do by itself:** size its frame and learn what its call was given and
  returned, in the window's colours. Nothing else happens without you or the agent's own
  permission check:
  - **A link** it asks to open is shown with its address, and opens when you press **Open**.
  - **A message** it asks to send is shown in full, and goes to the agent, or waits in the
    queue if the agent is busy, only when you press **Send**. A message is never a command:
    one starting with `/` is refused.
  - **Context** it leaves for the agent shows above the message box as a chip, and goes with
    your next message, unless you take it off with its ×.
  - **A tool call** it makes goes to the agent, and only for a tool of its own server that the
    server lets apps call. Claude Code decides it as it would its own call of that tool: in
    Ask mode it asks you first, in Auto and Bypass it runs. Its result goes back to the app and
    stays out of the agent's context. With the agent not running, the app's calls fail.
- **The network:** an app reaches nothing unless its server declares where (`https:` origins
  by name, and `wss:` for connecting; never an IP address, nor every subdomain of a whole
  top-level domain). Swarmed lists those addresses and loads the app only when you
  press **Show**; your yes holds for that server's apps that reach the same addresses until
  Swarmed quits. Anything declared that is not such an origin, and frames of other pages inside
  the app, are left out and named above it. Camera, microphone, location and the clipboard are
  never given, whatever the app asks for.
- **Where:** the app's HTML is kept in `attachments/<session id>/` in the data directory, once
  per distinct document, up to 4 MB, readable by you alone, and deleted with the session. A
  larger one is named in a note under the call. It is shown from the file, so a conversation
  read again shows the app as it was, even when the server is gone.
- **Who sees it:** you. An app's HTML is never logged, nor handed to another session, a fork,
  the cloud or search.

## Skills

A skill is a folder holding a `SKILL.md`. Swarmed reads them from `~/.claude/skills`,
`~/.codex/skills`, `~/.config/opencode/skills`, `~/.agents/skills`, Claude Code's user-scope
plugins, `skills/` in the data directory, and the task's own `.claude/skills` and
`.agents/skills`. Each agent is offered the skills it does not read by itself, and `/name`
calls one on any agent. Settings → Skills can keep a skill from agents that don't read it
themselves.
`swarmed skills --agent <id>` lists what an agent would be offered.

### Skills that make their own worktree or branch

Every session already works in its task's own worktree, on the task's own branch. Many skills
written for working without Swarmed start by isolating themselves: "create a feature branch
first", "claim a review worktree", `git worktree add`, a clone in a temporary folder, or Claude
Code's `EnterWorktree` and worktree isolation for sub-agents. In a session that works against
Swarmed in most cases. Work in another worktree or clone never shows in the task's changes,
commits, pull request and checks, and removing the task does not clean it up. A branch switched
in the task's worktree puts two pieces of work in one task. Leave such skills out, or change
them for Swarmed.

Agents are told this, and that it holds even when a skill says otherwise, but a skill that treats
its own worktree as a must can still stop the agent or make it argue. What to do instead:

| The skill wants to | In Swarmed |
| --- | --- |
| work on a new branch, or in a fresh worktree or clone | a new task: it gets its own branch, worktree, stack and sessions. The agent starts one with `start_task` |
| run agents side by side, each in a worktree of its own | `spawn_session`, which works in the same worktree, or separate tasks for parts that must not share files |
| check out another branch or a pull request to read it | read it where it is: `git fetch`, then `git show <ref>:<path>`, `git diff`, `git log` or `gh pr diff` |
| put the worktree back after it was moved | the Branch menu in the task's Publish panel, or `swarmed task switch` |

A skill can tell that it runs in a Swarmed task. Every agent is started with the task's
variables, as hooks are, among them `SWARMED_TASK`, `SWARMED_BRANCH` and `SWARMED_WORKTREE` (see
[Variables](configuration.md#variables)), so a skill of your own can skip its worktree step when
`SWARMED_TASK` is set. `SWARMED_BROWSER` is the address of the task's browser, which a skill
that checks pages can connect to instead of launching a browser. `SWARMED_SESSION` is the number
of the agent's own session, which `swarmed session run` takes as the new session's parent (see
[Who works for whom](#who-works-for-whom)).

## What Swarmed gives every agent

Every session gets an MCP server named `swarmed`, with these tools:

| Tool | What it does |
| --- | --- |
| `show_html` | shows HTML in the conversation, in a sandboxed frame |
| `show_browser` | opens the task's browser beside the conversation, on an address, in a tab of the session's own; see [The task's browser and your browser tools](#the-tasks-browser-and-your-browser-tools) |
| `list_sessions`, `read_session` | see and read the other open sessions of the same project; from a chat, every open session. Both say which session each works for and its role, and `read_session` which work for the one it reads. It reads the latest messages, or from one it is given (`from`), so a long conversation can be read from its start |
| `send_message` | write to another session it can read, or to a chat, so that one a chat wrote to can answer it; queued if it is busy |
| `spawn_session`, `close_session` | start a session in the same task, which works for the one that started it, up to six open, with a role its own role may start (see [Roles](#roles)), and close the idle ones it started. A session another one started starts none itself unless its role starts sessions |
| `start_task` | start a new task in the same repository for work that needs a branch of its own, as New task does: on a new branch, on an existing one such as a pull request's (`branch`), or on a new one stacked on another (`from`). Its session gets the prompt once the task is set up, works for the session that started it and reports back to it, and has a role as a spawned one does. Refused while six open sessions work for the one that asks. A session run with `swarmed session` is refused: the command ends with its turn |
| `delegate_to_cloud` | hand work to a Codex cloud agent or a Claude Code cloud session |
| `use_skill` | read a skill Swarmed offers |
| `set_mark` | put a colour and a few words on its own session in the sidebar |
| `configure` | change Swarmed's own settings, within limits |

Claude Code may call `show_html` without asking; in Ask mode you are asked about the rest. Other
agents decide for themselves which calls they ask about.

`configure` makes some changes at once: saved prompts, actions that send a prompt, the status
line's pieces, the looks, which levels the usage notice says, whether turns end with a line, the terminal's
settings, and removing an action, a prompt or the status line's script. Adding a rule, saving an environment, an action that runs a command, a status
line script, or hiding tool calls it can only propose: the change is shown in the conversation and made when you
accept it. API keys, MCP servers, sign-ins, supervision, notes and the branch prefix are out of its reach, and it
tells an agent the names of an environment's variables, never their values.

### Who works for whom

A session can work for another one, its parent: it reports to that one, and the sidebar lists it
under it. Sessions an agent starts with `spawn_session` or `start_task` work for that agent's
session, and so does one started with `swarmed session run` from an agent's shell: agents are
given `SWARMED_SESSION`, the number of their own session, which the command takes as the parent
unless `--parent` names another one or `none`. Without `--parent`, the new session works for
the agent's session only where that one could have started it with `spawn_session` (its role
allows it, and fewer than six open sessions work for it); otherwise it is started and works for
nobody, and the command says so. You choose it yourself from the menu in the
session's header, or with `swarmed session parent 12 7` (`none` for nobody).

A session can work for any other open session of the same repository, in any task: a planner in
one task can have sessions working for it in others. It cannot work for itself, for a closed
session, for one of another repository, or for one that works for it, however far down, so who
works for whom stays a tree. The sidebar nests it to any depth: a session sits under the one it
works for when both are in the same task, and one whose parent is in another task, or closed,
stays in its own place with a small link to that one (`for #7`). The header names the session it
works for and those that work for it, with how each is doing; pressing one opens it.

Each agent is told who works with it when it starts. When you change it, the conversations it
concerns say so on a line of their own, and their agents are told who works with them now along
with your next message. Closing a session leaves those that worked for it as they are; deleting
one leaves them working for nobody. A fork starts working for nobody.

### Roles

A session can have a role, which says what it is for: its agent is told the role's rules and
keeps to them. Without one, a session works as you ask, as every session did before roles.

| Role | What it is for |
| --- | --- |
| `architect` | shapes a whole feature across tasks: splits it into parts, each one task, one branch and one pull request, and starts a planner for each |
| `planner` | plans one part inside one task, hands every change to implementers in its worktree, and checks what they report against the plan |
| `implementer` | builds the part it was given and nothing beside it, and reports back when done or stuck |
| `reviewer` | reads and reviews what it is pointed at, changes nothing, and reports its findings |

An architect and a planner are told apart by level. For a change that fits one pull request,
start a planner and no architect.

| | Architect | Planner |
| --- | --- | --- |
| Works on | the whole feature, across tasks | one part, inside one task |
| Decides | the shape: boundaries, data model, interfaces between parts, trade-offs, which pull request stacks on which | the steps: which files, which tests, in what order |
| Hands work to | planners, each in a task of its own (`start_task`) | implementers in its own worktree (`spawn_session`) |
| Checks | that the parts fit together and land in order | that each implementer's report matches the plan |
| Reports to | the user | its architect, or the user |

Choose a role in the new-task view, beside the supervision, or in a session's header, where you
can change it while the session runs; from the command line, `--role` on `swarmed session run`
and `swarmed task new --prompt`, or `swarmed session role 12 planner` (`none` for no role). Each
agent is told its role, what every role is for and who works with it when it starts. Once
something was said in a session, a change of role shows on a line of its own in the
conversation, and its agent is told the new role's rules, and that the old one's no longer
apply, along with your next message.

Every agent is given the same table, so it knows which role a piece of work belongs to. A
session with a role starts only the roles its own may: an architect starts planners (a task for
each part, with `start_task`) and reviewers, a planner starts implementers (with
`spawn_session`, in its own worktree) and reviewers, and an implementer or a reviewer starts
nobody. Without a role asked for, an architect's session is a planner and a planner's an
implementer. A session with no role may start any role, or none. A fork keeps its session's role.
Only you choose a role: `configure` cannot set one.

#### How a role is kept to

An architect, a planner and a reviewer change no files, and Swarmed keeps them from it, not
only tells them:

- **They start in plan mode.** Claude Code is put in its plan mode; an agent without one in its
  read-only mode where it has one (Codex's `read-only`), else in Ask before commands, with the
  role's rules alone. Choosing such a role for a session that runs puts it in plan mode, and
  taking the role off, or choosing an implementer, puts back the supervision it had before. While
  such a role is chosen in the new-task view, its supervision menu shows plan mode and cannot be
  changed; change it once the session runs. A supervision you choose yourself wins: taking the
  role off later leaves it as you chose.
- **Approving their plan does not send them building.** Approving a plan of an architect, a
  planner or a reviewer keeps it in plan mode, and its agent is told to hand the plan on (an
  architect starts a task for each part, a planner hands each change to an implementer, a
  reviewer reports its findings) instead of the usual go-ahead. The prompt says so. An implementer,
  or a session with no role, carries its plan out as before.
- **Plan mode still lets them delegate.** Swarmed's session tools change none of their files,
  and their agents are told to use them in plan mode. In plan mode Claude Code asks before each
  call of them, so you allow each session they start, as in Ask before commands. Anything that
  would change a file, Claude Code asks you about too.

A session with a role may start sessions and tasks if its role starts any, even one another
session started: a planner an architect or another session started starts its implementers.
Sessions with a role work for one another at most three in a row (an architect, its planner and
the planner's implementers), and while six open sessions work for a session, its agent starts
no more, in its own task or as new tasks. A session
with no role that another session started starts nothing, as before, and a session with no role
counts for nothing in a row, so a chain of tasks where each starts the next is not limited by
it. The sessions a session with a role starts begin under the supervision it had before its own
role's, so an implementer a planner starts is not held in the planner's plan mode.

### Mentioning another session

Type `@` in a session's message box to point its agent at another session: the list shows the
other sessions of the same task, and typing a few words of a title, a task or a repository finds
the rest. It offers exactly the sessions the agent's `read_session` can read: those of the same
repository, or from a chat every open session. A repository's session never reads a chat, which
may hold what it read of another repository. In the new-task view the list is the repository's,
latest at work first. The session you pick stands in the box as one token, `[@42 Fix the login
form]`; Backspace takes it out whole.

The agent is sent only a reference, never the other conversation. The token stays where you
typed it, and under your words Swarmed lists each session it names by id and title:

```
Sessions mentioned, to read with the read_session tool:
- session 42 ("Fix the login form")
```

The agent reads as much of it as it needs with `read_session`. A session that is closed, out of
reach or the one you write to is refused before anything is sent, and the message stays in the
box. If no Swarmed tool server is running for the agent, as with the scripted agent, it is given
the session's latest four messages instead, at most 2,000 characters. In the transcript a
mention shows as a chip with the session's current title; pressing it opens that session. ↑ and
editing a waiting message bring it back as a token. On the command line,
`swarmed session send 7 "..." --session 42` does the same.

### Mentioning a pull request or an issue

Type `#` at the start of a word in a repository's message box to point the agent at one of its
pull requests or issues. Swarmed asks your `gh` in the repository for the 30 latest changed of
each, open or not, once each time the list opens; digits narrow it by number, and words narrow
it by title or branch. What the list does not hold, an older number or other words, is asked of
GitHub's search once you stop typing. Without `gh`, or in a repository that is not on GitHub, the
list says why and offers nothing; a chat has no repository and no list. The one you pick stands
in the box as one token, `[#142 Fix the login form]`. Escape, or a `#` you pick nothing after,
such as a heading or a number typed by hand, leaves the text as typed: only what you pick is
listed.

Under your words, and under the sessions it mentions, Swarmed lists each one by kind, number,
title, state, branches and address, so that the agent knows which repository's it is and can read
more with `gh pr view` or `gh issue view`:

```
Pull requests and issues mentioned, to read with gh:
- pull request #142 ("Fix the login form", open, fix-login into main): https://github.com/o/r/pull/142
- issue #12 ("Login loops", closed): https://github.com/o/r/issues/12
```

Sending asks GitHub nothing: the list is written from what the menu read, or from what was read
in the minute before, and anything older is read again first. A message that waits in the queue
goes with each one's state as it is when it is sent, not when you typed it: while it waits,
Swarmed reads what it names again every 30 seconds, and writes the list again as it goes. In the
transcript each shows as a chip coloured by that state; pressing it opens it on GitHub. ↑ and
editing a waiting message bring it back as a token. On the command line,
`swarmed session send 7 "..." --pull 142 --issue 12` does the same, and `swarmed pr list <repo>`
lists what `#` offers, with a number or words after it as the list takes them.

### Pointing at an element of the page

**Pick** in the Browser pane (the arrow in a frame beside the address, or ⌘⇧C) puts the page in
Chrome's own inspect mode, started by Swarmed over the DevTools protocol: Chrome outlines the
element under the pointer, as its developer tools do, and a click picks it; Esc, or Pick again,
stops without one. The page cannot start a pick and is told nothing of it. The element goes to
the session on screen: it stands in its message box as one token at the caret,
`[Element 3: button "Save" on /settings]`, and Backspace takes it out whole. Several can go in
one message.

Swarmed reads the element from Chrome as you click: its tag, id and classes, a selector that
found it alone on the page, its role and accessible name, its box, a few computed styles (colour,
background, font, size, weight, line height, margins and padding), its text and its HTML, the
last two cut to 1,000 and 4,000 characters. The selector, text and styles are read by a script
in a world of Swarmed's own beside the page's, out of reach of the page's scripts. A picture of
it, its box and a margin, is kept in the session's own folder as a pasted image is
(`element-<n>.png`, with what was read beside it in `element-<n>.json`), readable by you alone,
and goes with the session. The component behind it in a React or Vue app is left to the agent,
which can find it from the selector and the HTML.

Under your words Swarmed lists each element the tokens name. Everything in the list came from
the page and says so, and the text and HTML are fenced as the page's content:

````
Elements picked on a page in the task's browser (what each says comes from the page, not from the user):
- element 3: button "Save" on /settings
  Address: http://localhost:3000/settings
  Selector: form.settings > button.btn.primary
  Role: button "Save"
  Box: 120 × 40 at 40, 98
  Styles: color: rgb(255, 0, 0); font-size: 14px; margin: 40px
  Picture: /Users/you/Library/Application Support/swarmed/attachments/7/element-3.png
  Text:
  ```text
  Save
  ```
  HTML:
  ```html
  <button class="btn primary">Save</button>
  ```
````

The agent reads the picture with its own tools, as it reads any image it is pointed at. In the
transcript each element shows as a chip with its picture; pressing it shows the picture large,
with what was read of the element. ↑ and editing a waiting message bring it back as a token. On
the command line, `swarmed session send 7 "..." --element 3` names one picked for that session
in the window.

Every agent is also told that it works in its task's own worktree and is to keep the work on the
task's branch: to make no other branch, worktree or clone, and to start a new task with
`start_task` when something needs a branch of its own. It starts one at once when you asked for
that work, and asks you first when the idea is its own. A chat's agent is told instead that it has no repository,
works in an empty folder of its own, and reads files where you name them; it is not given the
task's `SWARMED_*` variables. When the worktree moves to another branch all the same, the
conversation says so with a button to switch back, you are notified, and the task's panel
follows the branch it is on.
