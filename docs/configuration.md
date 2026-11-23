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

### What Swarmed cannot understand

One key Swarmed cannot understand never stops it, whether it is misspelt or written by a newer version of Swarmed. It works with the rest, and names each problem with its file in the window, when the window opens and whenever the configuration changes. The command line notes them on stderr.

| File | A key it does not know | A value it cannot take | Not TOML at all |
| --- | --- | --- | --- |
| `settings.toml` | left out, and written back as it was when the settings are saved | that setting is at its default | every setting is at its default, and the file is kept as `settings.toml.unreadable` before the settings are saved over it |
| `agents.toml` | left out of that agent | that agent is not offered | none of its agents is offered; Claude Code is |
| `mcp.toml` | left out of that server | that server is not given to agents | none of its servers is given, and the file is kept as `mcp.toml.unreadable` before a server is saved over it |
| `skills.toml` | left out | every skill is offered | every skill is offered, and the file is kept as `skills.toml.unreadable` before a skill is shared or kept back |
| `projects/<project>.toml` | left out | that key is at its default | an error: the project's tasks cannot be set up, started or removed with their hooks, which there is no telling |
| `projects/<project>.environments.toml` | left out of that environment | that environment is left out | an error for tasks in its environments |

`templates.toml`, the actions, `layout.json` and `terminal.json` affect only their own part: the saved prompts, the action buttons, the custom layout and the terminal. A key `layout.json` does not know is left out and named like the others above; see [The custom layout](#the-custom-layout).

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
| `branch_pattern` | `{{prefix}}/{{task}}` | a new task's branch; may use `prefix`, `user`, `task` and `project`: see [A task's branch](#a-tasks-branch) |
| `base_branch` | `origin`'s default branch, else `origin/main` or `origin/master` | the ref new tasks branch from |
| `ports` | none | named base ports |
| `port_step` | `100` | how far apart two tasks' ports are |
| `max_stacks` | `5` | how many of the project's stacks may run at once |
| `open_url` | the first local address `start` prints | what "Open in browser" opens |
| `hooks` | none | `setup`, `start`, `stop` and `teardown` commands |
| `env` | none | environment for the hooks and the task's agents |

### A task's branch

A new task's branch is its branch prefix, a `/` and the task's name, such as `jd/fix-login`. Set
the prefix under **Branch prefix** in Settings, Sessions, or with `swarmed branch-prefix jd`.
Empty, it is your login (`{{user}}`), as it always was. Slashes at its ends are dropped and
those inside stay: `jd/feat` gives `jd/feat/fix-login`. Settings shows the branch it gives as
you type, and a prefix git would not take in a branch's name is refused with git's reason.

The prefix is `{{prefix}}` in `branch_pattern`. A repository whose config sets a pattern of its
own keeps it, and may use `{{prefix}}` there too. A config that says `{{user}}/{{task}}`, which
Swarmed wrote into every new config before there was a prefix, is read as the default, so the
prefix applies to it. A branch you name or pick when you start a task is used as it is, and a
name Swarmed makes up steps on to `name-2` and so on after the prefix.

## Hooks and the stack

Each hook runs with `sh -c` in the task's worktree, and an empty one is skipped.

1. A new task gets its branch and worktree, then its environment's snapshot, then `setup`. A task
   whose setup failed is kept, and setup can be run again. When `setup` succeeds and the
   environment has no snapshot yet, the task is kept as its snapshot.
2. When `setup` succeeds, `start` is started and kept running, with its output in a log. Starting
   one stack more than `max_stacks` is refused: the task's environment's, if it sets one.
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
is an error. `{{prefix}}` is for `branch_pattern` alone: a hook has the task's `{{branch}}`.

In a hook, a value is always one word, whatever is in it: a path with a space needs no quotes,
and a branch's name is never read as a command. The same values are exported to hooks as
`SWARMED_PROJECT`, `SWARMED_TASK`, `SWARMED_USER`, `SWARMED_BRANCH`, `SWARMED_SLOT`,
`SWARMED_WORKTREE`, `SWARMED_REPO` and `SWARMED_PORT_<NAME>`.

The task's agents get `env` and the same `SWARMED_*` variables, so a skill can tell that it runs
in a task's own worktree. Claude Code gets no variable whose name starts with `ANTHROPIC_` or
`CLAUDE`.

## Ports

Each task has a slot: the lowest number from 1 that no other task of the project holds. A task's
port is `base + slot × port_step`, so with `web = 3000` the first task's is 3100 and the second's
3200. Slot 0, port 3000, is left to the main checkout.

## Environments

An environment is another way of setting a task up in the same project, such as with or without
Docker. It can have its own hooks and `open_url`, which take the place of the project's, and
ports and variables, which are added to the project's in place of any of the same name. Its own
`max_stacks`, "Stacks at once" in the app, is how many of the project's stacks, in any
environment, may be running for one of its tasks to start its own. A task
is created in one environment, or none, and keeps it. An environment cannot be removed while a
task still uses it.

One environment can be the project's default. A new task is set up in it unless another, or the
project's own setup, is chosen when the task is started: in the new-task view's menu, which
opens on the default, or with `--environment` or `--no-environment` on `swarmed task new`.
Choose it in the app's Environments dialog or with `swarmed environment <project> --default
<name>`; `--no-default` goes back to the project's own setup. Tasks already made keep the
environment they were set up in. Removing the default environment leaves the project without one.

Environments are edited in the app and kept in `projects/<project>.environments.toml`, which
Swarmed creates readable by you alone, since their variables may hold credentials. `swarmed environment <project>` lists
them.

## Snapshots

A snapshot keeps the files git ignores in one task whose setup went well, such as dependencies,
build output and `.env`. A new task of the same environment gets a copy before its `setup` runs,
so setup has less to do. Nothing already in the worktree is overwritten. Where the data directory
and the worktrees share an APFS volume, as they do by default, the copies are clones and take no
extra space until changed, and each folder is cloned whole at once, so even a large
`node_modules` takes seconds. The setup output says which part is being copied, and how long
it took.

There is one per environment, and Swarmed takes it itself: the first task whose `setup` succeeds
in an environment with no snapshot is kept as its snapshot, before the task's stack starts, so
that its logs and caches are not in it. That copy runs once, and the task waits for it. The
next tasks start from it. A task whose `setup` is run again is kept only if it had not
succeeded before: one that had has been worked in since. Once what setup makes has changed, such as after a dependency update,
take it again from a task set up since, in the app's Environments dialog or with
`swarmed snapshot save <project> <task>`; forgetting it has the next task set up well kept
instead.

## Settings

Set in the app's Settings, kept in `settings.toml`.

| Key | Default | Meaning |
| --- | --- | --- |
| `editor` | `code` | the command that opens a folder in your editor |
| `terminal_app` | `Terminal` | the application "Open in Terminal" uses |
| `default_supervision` | `auto` | how new sessions are supervised: `ask`, `auto` or `plan`; `bypass`, which allows everything, is not offered in Settings |
| `auto_title` | `true` | a model names each new session after its first message |
| `notifications` | `true` | notify when a session needs you, finishes, fails or moves its worktree to another branch |
| `updates` | `automatic` | how Swarmed updates itself: `automatic` looks at launch and every few hours, downloads a new version and installs it when you quit; `when_asked` fetches nothing until About opens and installs only when you ask |
| `html_views` | `true` | agents may answer with HTML views |
| `tool_calls` | `folded` | how agents' tool calls show in a conversation: `all`, each on its own line; `folded`, the calls that follow each other in one line that counts them and names the last; `hidden`, none, with a larger animation while the agent works. Permission requests, questions, plans and views always show |
| `limit_warnings` | `frequent` | what a conversation says when Claude Code warns that a usage window is filling up: `frequent`, each warning, once for as long as the same window keeps warning; `rare`, only once a window is at least 75% used, once; `none`, no warnings. A limit reached and work that goes on with extra usage are always said, and the usage coins show the same whatever it is |
| `rules` | none | rules every agent is given, in every session |
| `status_items`, `status_command` | branch, changes, model, context, cost | the status line: built-in pieces, or a script whose first line is shown |
| `theme`, `palette`, `style`, `layout`, `text_scale` | system, swarm, colony, classic, 100 | how the app looks |

A key Swarmed does not know stays in `settings.toml` when the settings are saved, so an older Swarmed beside a newer one does not erase that one's choices. What else happens to what it cannot understand is in [What Swarmed cannot understand](#what-swarmed-cannot-understand).

### The custom layout

`layout.json` holds the custom layout, which Settings shows as JSON and an agent can write for you. It starts from classic or senior (`base`) and changes only how things look: the five text sizes, line height, spacing, icons, the heights of rows and controls, the two fonts, the palette's colours in dark and in light, and the sidebar's levels. `swarmed layout schema` prints every field with its range.

`sidebar` sets the four levels of the sidebar apart from each other and from the rest of the app: `repository`, `task`, `session` and `sub_agent`. Each can take a text `size` in pixels (9 to 40), a `weight` from 400 to 700, and for `dark` and `light` a `colour` for its text and a `background` for its rows:

```json
{
  "base": "classic",
  "sidebar": {
    "repository": { "size": 16 },
    "session": { "size": 14, "weight": 500,
      "dark": { "colour": "#e8e4f0", "background": "#24202c" } }
  }
}
```

A level you leave out is drawn as its base draws it, and the repository's name grows and shrinks with the text sizes like the rest. A session's colour is used whatever the session is doing; one that waits on you or has an answer you have not read is still drawn heavier. Where a session is selected, pointed at or waits on you, its row keeps the background that says so.

Swarmed refuses a layout it could not draw readably, with the reason, and keeps the one before. Text has to stand out from its background by 4.5 to 1 (3 to 1 for sub-agents, which are drawn quieter). A level that gives a colour has to give its background too, or its form a `panel`, which the sidebar is; one that gives a background has to give its colour. The palette's own colours are only known to the window, so nothing could be checked against them.

A key the layout does not know, misspelt or written by a newer Swarmed, is left out when `layout.json` is read and named with the other problems, and the rest of the layout is drawn. Saving a layout with one is refused, so a misspelt key is caught when you save it.

## Environment variables

| Variable | Effect |
| --- | --- |
| `SWARMED_DATA_DIR`, `SWARMED_WORKTREES_DIR` | move the data directory and the worktrees |
| `SWARMED_CLAUDE_BIN` | the `claude` program sessions run, instead of the one on the `PATH` |
| `SWARMED_CLAUDE_CONFIG`, `SWARMED_CODEX_CONFIG`, `SWARMED_OPENCODE_CONFIG_DIR` | where Claude Code's, Codex's and OpenCode's configuration is read from |
| `SWARMED_FAKE_AGENT` | any value: a scripted agent instead of real ones, which spends no tokens |
| `SWARMED_NOTIFY_TRACE` | a file the app notes what became of each notification in |
