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
swarmed issue list shop --mine                        # its GitHub issues assigned to you
swarmed issue show shop 12                            # one whole, with its comments
swarmed task new shop fix-login --issue 12            # branch, worktree, setup hook; for issue #12
swarmed session run shop fix-login "Fix the login form's error message"
swarmed session send 12 "Add a test for it"           # the id `session run` printed
swarmed session send 12 "Do as it did" --session 9    # points the agent at session 9 to read
swarmed session send 12 "Review it" --pull 142        # and at pull request #142 (or --issue 12)
swarmed session send 12 "Make it blue" --element 3    # and at element 3 picked in the task's browser
swarmed pr list shop login                            # the pull requests and issues # offers
swarmed session turns 12                              # each turn with the files it changed
swarmed session turn-diff 12 1                        # what the second message's turn changed
swarmed task commit shop fix-login -m "Fix the login form's error message"
swarmed task push shop fix-login
swarmed task switch shop fix-login                    # back to its own branch, or to one named
swarmed task pr shop fix-login                        # through your own gh
swarmed task pull shop fix-login --follow             # its checks, reviews and merge as they come
swarmed task merge shop fix-login --squash            # or --auto, to merge once checks pass
swarmed task browser shop fix-login                   # the task's own Chrome: where its DevTools endpoint answers
swarmed task done shop fix-login                      # put away: agents and stack stopped, nothing deleted
swarmed task undone shop fix-login                    # back with the others
swarmed task rm shop fix-login                        # teardown hook, worktree removed
```

`issue list` lists a repository's open issues through your own `gh`, the latest changed first:
`--mine` those assigned to you, `--created` those you opened, `--mentioned` those that mention
you, `--closed` closed ones, `--search` words in GitHub's search syntax (`label:bug login`) and
`--limit` how many. Without `gh`, or for a repository that is not on GitHub, it lists nothing and
says why. `issue show` prints one whole, with its comments, leaving out those GitHub hides.
`task new --issue 12` records that the task is for issue #12: the pull request text `task pr`
and `task pr-message` write ends with `Closes #12`, so merging it closes the issue.

`task merge` merges the pull request at the commit it just read: one pushed since is refused.
Without a method it takes the one last used for the repository. It refuses while a check failed;
`--force` merges anyway where no rule of the repository requires that check, `--admin` merges
past the repository's requirements, as only its admins can, and `--auto` has GitHub merge it
once they are met (`--cancel-auto` takes that back). No branch is deleted: the task's goes with
`task rm --delete-branch`.

`task browser` starts the task's own Chrome, or finds the one that runs, and prints where its
DevTools endpoint answers (`http://127.0.0.1:<port>` and the browser's WebSocket), its tabs and
where its downloads go. It runs on after the command ends, until `--stop`, the task is put away
or removed, or the app stops it unused; see
[The task's browser](configuration.md#the-tasks-browser).

`task done` puts a finished task away: its agents and its stack are stopped and `task list`
says `done`; its sessions are closed too if `put_away_closes_sessions` is set. Starting one of
its agents, reopening one of its sessions or starting its stack brings it back, as `task undone`
does. The rules that put tasks away by themselves run in the app only.

`task rm` refuses while the worktree has uncommitted changes or unpushed commits, or is one git
no longer knows; `--force` removes it anyway. A failing `teardown` does not stop it: what it had
to pass over is printed.

`task new --branch <branch>` checks out an existing branch, here or on a remote, instead of
making one. `task new --from <branch>` makes the new branch from that branch instead of the
base, which it then is for the task: its changes and its pull request are against it, for work
stacked on another pull request. Agents do the same with Swarmed's `start_task` tool, in the app: a session run from the
command line is refused it, since the command ends with its turn and the task's setup with it.

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
`branches`, `stale`, `snapshots`, `checkpoints` and `browsers`.

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
| `project` | add, list, remove, move and pin repositories; `project config` prints the path of a project's config file |
| `task` | create, remove, move, pin and highlight tasks, rerun `setup`, run the `start` command, see changes, commit, push, open a pull request, follow it (`task pull --follow`) and merge it (`task merge`) |
| `issue` | list a repository's GitHub issues with filters, or print one with its comments, through your own `gh` |
| `session` | run, continue, list, show, fork, rename, mark, move, pin, highlight, close, reopen and delete sessions; list a session's turns with the files each changed (`turns`) and print one turn's diff (`turn-diff`); take a session back to before one of its messages, its files too with `--files` (`rewind`), and put those files back (`put-back`); change a session's model or supervision; let a Claude Code session be reached from claude.ai (`remote-control`); let one a usage limit stopped go on by itself when the limit resets (`continue-at-reset`, which the app sends) |
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
| `cleanup` | list or delete what removed tasks left behind, and snapshots: removed worktrees, branches of removed tasks, stale worktrees, checkpoints and browser profiles of tasks that are gone |
| `editor`, `auto-title`, `branch-prefix`, `layout`, `terminal` | settings: the editor, naming sessions by a model, what new tasks' branches start with, the custom layout, the terminal |

Commands that take a secret or a long text, `key set`, `note --set` and `rules --set`, read it
from standard input, so it stays out of the shell's history.
