# Projects and configuration

[← Back to README](../README.md)

A project is a git repository registered with Swarmed. A task is a branch of it, checked out in a
worktree of its own, with a stack: what the project's hooks set up and keep running there.

## Where things are

| What | Where |
| --- | --- |
| Configuration and state | `~/Library/Application Support/swarmed`, or `SWARMED_DATA_DIR` |
| Worktrees | `~/swarmed/<project>/<task>`, or under `SWARMED_WORKTREES_DIR` |

In the data directory:

| File | Holds |
| --- | --- |
| `swarmed.db` | projects, tasks, sessions and everything said in them (SQLite) |
| `settings.toml` | the settings, the rules every agent is given among them |
| `projects/<project>.toml` | a project's configuration, below |
| `projects/<project>.environments.toml` | its environments, readable by you alone |
| `agents.toml`, `mcp.toml`, `skills.toml` | agents, MCP servers and hidden skills: see [Coding agents](coding-agents.md) |
| `actions.toml`, `projects/<project>.actions.toml` | actions for every repository, and for one |
| `templates.toml` | saved prompts |
| `layout.json`, `terminal.json` | the custom layout and the terminal; `swarmed layout schema` and `swarmed terminal schema` print their JSON Schemas |
| `skills/` | skills of Swarmed's own: see [Coding agents](coding-agents.md) |
| `notes/` | notes on projects and tasks, readable by you alone |
| `snapshots/` | snapshots, below, readable by you alone |
| `spool/` | HTML views agents hand over through Swarmed's `show_html` tool |
| `run/` | stacks' logs and what Swarmed is running |

The app and the `swarmed` command write these. The TOML and JSON files may be edited by hand
as well.

## A project's configuration

`swarmed project add` writes a commented template, and `swarmed project config <project>` prints
where it is. It is read again whenever a task needs it, and a key Swarmed does not know is
refused rather than passed over.

```toml
port_step = 100

[ports]
web = 3000

[hooks]
setup = "cp {{repo}}/.env .env && npm ci"
start = "npm run dev -- --port {{port.web}}"

[env]
WEB_PORT = "{{port.web}}"
```

| Key | Default | Meaning |
| --- | --- | --- |
| `branch_pattern` | `{{user}}/{{task}}` | a new task's branch; may use `user`, `task` and `project` |
| `base_branch` | `origin`'s default branch, else `origin/main` or `origin/master` | the ref new tasks branch from |
| `ports` | none | named base ports |
| `port_step` | `100` | how far apart two tasks' ports are |
| `max_stacks` | `2` | how many of the project's stacks may run at once |
| `open_url` | the first local address `start` prints | what "Open in browser" opens |
| `hooks` | none | `setup`, `start`, `stop` and `teardown` commands |
| `env` | none | environment for the hooks and the task's agents |

## Hooks and the stack

Each hook runs with `sh -c` in the task's worktree, and an empty one is skipped.

1. A new task gets its branch and worktree, then its environment's snapshot, then `setup`. A task
   whose setup failed is kept, and setup can be run again.
2. When `setup` succeeds, `start` is started and kept running, with its output in a log. Starting
   one stack more than `max_stacks` is refused.
3. Stopping a stack runs `stop`, then ends whatever `start` started, including what it started
   in process groups of their own.
4. Removing a task stops its agents and its stack, runs `teardown`, ends whatever else still
   runs in the worktree, and removes the worktree. It is refused while the worktree has
   uncommitted changes or unpushed commits, or is one git no longer knows, whose work cannot
   be checked, unless forced; nothing else stops it. A `stop` or
   `teardown` that fails or takes longer than a minute is passed over, and so is a process that
   will not end, and you are told. The worktree is moved aside at once and deleted in the
   background, so a large one does not keep you waiting. Removed in the app, the task's local
   branch goes too, unless the task started from it or commits on it are on neither the base
   nor a remote: then it is kept, and you are told. From the command line the branch stays
   unless you pass `--delete-branch`. A branch on a remote is never touched.

Quitting the app ends its stacks' processes without running `stop`. From the command line,
`swarmed task new` and `swarmed task setup` end the stack again when they exit, and
`swarmed task start` keeps one running in the foreground.

## Variables

Hooks, `env` values and `open_url` may use `{{project}}`, `{{task}}`, `{{user}}`, `{{branch}}`,
`{{slot}}`, `{{worktree}}`, `{{repo}}` (the main checkout) and `{{port.<name>}}`. An unknown name
is an error.

In a hook, a value is always one word, whatever is in it: a path with a space needs no quotes,
and a branch's name is never read as a command. The same values are exported to hooks as
`SWARMED_PROJECT`, `SWARMED_TASK`, `SWARMED_USER`, `SWARMED_BRANCH`, `SWARMED_SLOT`,
`SWARMED_WORKTREE`, `SWARMED_REPO` and `SWARMED_PORT_<NAME>`.

The task's agents get `env` too, but not the `SWARMED_*` variables, and Claude Code gets no
variable whose name starts with `ANTHROPIC_` or `CLAUDE`.

## Ports

Each task has a slot: the lowest number from 1 that no other task of the project holds. A task's
port is `base + slot × port_step`, so with `web = 3000` the first task's is 3100 and the second's
3200. Slot 0, port 3000, is left to the main checkout.

## Environments

An environment is another way of setting a task up in the same project, such as with or without
Docker. It can have its own hooks and `open_url`, which take the place of the project's, and
ports and variables, which are added to the project's in place of any of the same name. A task
is created in one environment, or none, and keeps it. An environment cannot be removed while a
task still uses it.

Environments are edited in the app and kept in `projects/<project>.environments.toml`, which
Swarmed creates readable by you alone, since their variables may hold credentials. `swarmed environment <project>` lists
them.

## Snapshots

A snapshot keeps the files git ignores in one task whose setup went well, such as dependencies,
build output and `.env`. A new task of the same environment gets a copy before its `setup` runs,
so setup has less to do. Nothing already in the worktree is overwritten. Where the data directory
and the worktrees share an APFS volume, as they do by default, the copies are clones and take no
extra space until changed.

`swarmed snapshot save <project> <task>` takes one; there is one per environment.

## Settings

Set in the app's Settings, kept in `settings.toml`.

| Key | Default | Meaning |
| --- | --- | --- |
| `editor` | `code` | the command that opens a folder in your editor |
| `terminal_app` | `Terminal` | the application "Open in Terminal" uses |
| `default_supervision` | `auto` | how new sessions are supervised: `ask`, `auto` or `plan`; `bypass`, which allows everything, is not offered in Settings |
| `auto_title` | `true` | a model names each new session after its first message |
| `notifications` | `true` | notify when a session needs you, finishes or fails |
| `updates` | `automatic` | how Swarmed updates itself: `automatic` looks at launch and every few hours, downloads a new version and installs it when you quit; `when_asked` fetches nothing until About opens and installs only when you ask |
| `html_views` | `true` | agents may answer with HTML views |
| `rules` | none | rules every agent is given, in every session |
| `status_items`, `status_command` | branch, changes, model, context, cost | the status line: built-in pieces, or a script whose first line is shown |
| `theme`, `palette`, `style`, `layout`, `text_scale` | system, swarm, colony, classic, 100 | how the app looks |

## Environment variables

| Variable | Effect |
| --- | --- |
| `SWARMED_DATA_DIR`, `SWARMED_WORKTREES_DIR` | move the data directory and the worktrees |
| `SWARMED_CLAUDE_BIN` | the `claude` program sessions run, instead of the one on the `PATH` |
| `SWARMED_CLAUDE_CONFIG`, `SWARMED_CODEX_CONFIG`, `SWARMED_OPENCODE_CONFIG_DIR` | where Claude Code's, Codex's and OpenCode's configuration is read from |
| `SWARMED_FAKE_AGENT` | any value: a scripted agent instead of real ones, which spends no tokens |
| `SWARMED_NOTIFY_TRACE` | a file the app notes what became of each notification in |
