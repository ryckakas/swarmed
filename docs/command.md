# The `swarmed` command

[← Back to README](../README.md)

The command drives the same core as the app, on the same data, from a terminal or a script.
`swarmed help <command>` has every argument; this page is the map.

What it changes shows in a running app within about a second: a session renamed, marked,
closed or started, a task made or removed, an action added, and the conversation of a session
it runs, as it goes. A session's agent runs in one place at a time: one the app runs is refused
here, and the other way round, until it is stopped there or that Swarmed quits. So is closing or
deleting it, and removing its task or repository. A message queued in the app for a
session the command runs goes once the command is done. `swarmed events` prints what happens in
every session, whichever Swarmed does it.

## A first task

```sh
swarmed project add ~/code/shop                       # register a git repository
swarmed task new shop fix-login                       # branch, worktree, setup hook
swarmed session run shop fix-login "Fix the login form's error message"
swarmed session send 12 "Add a test for it"           # the id `session run` printed
swarmed task commit shop fix-login -m "Fix the login form's error message"
swarmed task push shop fix-login
swarmed task switch shop fix-login                    # back to its own branch, or to one named
swarmed task pr shop fix-login                        # through your own gh
swarmed task rm shop fix-login                        # teardown hook, worktree removed
```

`task rm` refuses while the worktree has uncommitted changes or unpushed commits, or is one git
no longer knows; `--force` removes it anyway. A failing `teardown` does not stop it: what it had
to pass over is printed.

`task new --prompt "…"` starts the task's first session with it, as the app does: the session
is made at once, the prompt waits while `setup` runs, and goes once it succeeds, after which the
turn is shown as `session run` shows it (`--agent` and `--model` choose the session's). A setup
that fails leaves the prompt waiting: `task setup` runs setup again and then sends it,
`task start-anyway` sends it now, and `task setup-log` prints what the last setup printed.

A chat, a session with no repository, is a task of the project `.chats`, which no repository can
be called. It has an empty folder of its own and no branch; the chats appear in `project list`
once the first is made. `task rm` refuses while its folder holds anything, unless `--force`.
`session fork --into` carries a chat on in a task of a repository:

```sh
swarmed task new .chats monads                        # a folder of its own, no repository
swarmed session run .chats monads "What is a monad, in plain words?"
swarmed task new shop monads                          # when it turns into work
swarmed session fork 14 --into shop monads            # the whole conversation, in that task
```

`swarmed cleanup` lists what removed tasks left behind, and the snapshots, one to a line: its
kind, size, key, task and why it is there. `swarmed cleanup branches --delete` deletes every
branch of a removed task that loses no work; one marked `at risk` goes only when its key is
given, as in `swarmed cleanup branches --delete shop:jd/spike`. The kinds are `removed`,
`branches`, `stale` and `snapshots`.

`session run` and `session send` print the conversation as it happens and ask on the terminal
when the agent wants permission (`[y/N]`). Ctrl-C stops the turn, not the session. Typing `bg`
and Enter sends a command or sub-agent that Claude Code waits on to the background, as Ctrl+B
does in its own terminal: the turn goes on, and `task background` lists it while it runs. They
exit non-zero when the turn is stopped or fails, so a script can tell.

A session runs on Claude Code unless `--agent` names another (`swarmed agents` lists them).
`--ask`, `--auto` and `--plan` choose how it is supervised; without one, the settings' default
applies. `SWARMED_FAKE_AGENT=1` swaps in a scripted agent that spends no tokens.

## Commands

| Command | What it does |
| --- | --- |
| `project` | add, list, remove and move repositories; `project config` prints the path of a project's config file |
| `task` | create, remove and move tasks, rerun `setup`, run the `start` command, see changes, commit, push, open a pull request |
| `session` | run, continue, list, show, fork, rename, mark, move, close, reopen and delete sessions; change a session's model or supervision; let a Claude Code session be reached from claude.ai (`remote-control`) |
| `environment` | list a project's environments; choose the default one new tasks are set up in |
| `snapshot` | keep the files git ignores in a set-up task, for new tasks to start from |
| `agents`, `agent` | list the agents sessions can run on; install and configure the others |
| `key` | store, remove or list API keys for agents other than Claude Code |
| `mcp`, `skills` | list the MCP servers and skills agents are given |
| `rules` | print or replace the rules every agent is given |
| `note` | print or replace the note on a project or a task |
| `action` | manage the buttons of your own that send a prompt or run a command |
| `cloud` | hand a prompt to a Codex cloud agent or a Claude Code cloud session and list what was handed over; move one among the others; print or apply a Codex task's changes |
| `search` | find the sessions where something was said |
| `events` | print what happens in every session as it happens, whichever Swarmed does it, until Ctrl-C |
| `costs`, `analytics`, `usage` | what agents report their work cost, turns by day and model, what Swarmed takes up |
| `limits` | your plan's usage limits and extra usage as Claude Code last reported them in any session, with how old each figure is; asks nothing |
| `cleanup` | list or delete what removed tasks left behind, and snapshots: removed worktrees, branches of removed tasks, stale worktrees |
| `editor`, `auto-title`, `branch-prefix`, `layout`, `terminal` | settings: the editor, naming sessions by a model, what new tasks' branches start with, the custom layout, the terminal |

Commands that take a secret or a long text, `key set`, `note --set` and `rules --set`, read it
from standard input, so it stays out of the shell's history.
