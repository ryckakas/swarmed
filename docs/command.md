# The `swarmed` command

[← Back to README](../README.md)

The command drives the same core as the app, on the same data, from a terminal or a script.
`swarmed help <command>` has every argument; this page is the map.

## A first task

```sh
swarmed project add ~/code/shop                       # register a git repository
swarmed task new shop fix-login                       # branch, worktree, setup hook
swarmed session run shop fix-login "Fix the login form's error message"
swarmed session send 12 "Add a test for it"           # the id `session run` printed
swarmed task commit shop fix-login -m "Fix the login form's error message"
swarmed task push shop fix-login
swarmed task pr shop fix-login                        # through your own gh
swarmed task rm shop fix-login                        # teardown hook, worktree removed
```

`task rm` refuses while the worktree has uncommitted changes or unpushed commits; `--force`
removes it anyway. A failing `teardown` does not stop it: what it had to pass over is printed.

`session run` and `session send` print the conversation as it happens and ask on the terminal
when the agent wants permission (`[y/N]`). Ctrl-C stops the turn, not the session. They exit
non-zero when the turn is stopped or fails, so a script can tell.

A session runs on Claude Code unless `--agent` names another (`swarmed agents` lists them).
`--ask`, `--auto` and `--plan` choose how it is supervised; without one, the settings' default
applies. `SWARMED_FAKE_AGENT=1` swaps in a scripted agent that spends no tokens.

## Commands

| Command | What it does |
| --- | --- |
| `project` | add, list and remove repositories; `project config` prints the path of a project's config file |
| `task` | create and remove tasks, rerun `setup`, run the `start` command, see changes, commit, push, open a pull request |
| `session` | run, continue, list, show, fork, rename, mark, close, reopen and delete sessions; change a session's model or supervision |
| `environment` | list a project's environments |
| `snapshot` | keep the files git ignores in a set-up task, for new tasks to start from |
| `agents`, `agent` | list the agents sessions can run on; install and configure the others |
| `key` | store, remove or list API keys for agents other than Claude Code |
| `mcp`, `skills` | list the MCP servers and skills agents are given |
| `rules` | print or replace the rules every agent is given |
| `note` | print or replace the note on a project or a task |
| `action` | manage the buttons of your own that send a prompt or run a command |
| `cloud` | hand a prompt to a Codex cloud agent or a Claude Code cloud session and list what was handed over; print or apply a Codex task's changes |
| `search` | find the sessions where something was said |
| `costs`, `analytics`, `usage` | what agents report their work cost, turns by day and model, what Swarmed takes up |
| `editor`, `auto-title`, `layout`, `terminal` | settings: the editor, naming sessions by a model, the custom layout, the terminal |

Commands that take a secret or a long text, `key set`, `note --set` and `rules --set`, read it
from standard input, so it stays out of the shell's history.
