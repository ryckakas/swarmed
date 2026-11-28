# Swarmed

![Swarmed, get swarmed by agents: a bear in power armour facing a swarm](docs/cover.jpg)

**Run coding agents side by side, each on a branch and in a worktree of its own.**

Give Swarmed a git repository and say what you want done. It creates a branch and a worktree for
the task, sets it up with ports of its own, and starts an agent there: Claude Code, Codex,
OpenCode, Antigravity or any agent that speaks the Agent Client Protocol. Run several tasks at
once: each has its own worktree and ports, so they don't step on each other. A desktop app and a
`swarmed` command, over one Rust core.

[![Latest release](https://img.shields.io/github/v/release/ryckakas/swarmed)](https://github.com/ryckakas/swarmed/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/ryckakas/swarmed/ci.yml?branch=main&label=CI)](https://github.com/ryckakas/swarmed/actions/workflows/ci.yml?query=branch%3Amain)
[![CodeQL](https://img.shields.io/github/actions/workflow/status/ryckakas/swarmed/codeql.yml?branch=main&label=CodeQL)](https://github.com/ryckakas/swarmed/actions/workflows/codeql.yml?query=branch%3Amain)
[![crates.io](https://img.shields.io/crates/v/swarmed?label=crates.io)](https://crates.io/crates/swarmed)
![Rust 1.100+](https://img.shields.io/badge/rust-1.100%2B-CE422B)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

<img src="docs/images/demo.gif" alt="A new task started in plan mode and its plan approved; the agent answering with a chart shown inline, whose buttons redraw it; a diff in the task's panel; a permission request allowed; and the colour palettes switched in Settings">

## Why this one

- **Any agent, one pool.** Claude Code, Codex, OpenCode, Antigravity or any ACP agent, each on
  its own login, sharing one pool of MCP servers and skills.
- **Agents that delegate.** Give a session a role (architect, planner, implementer, reviewer)
  and it splits the work into sessions and tasks of their own, which report back to it.
- **Sessions survive restarts.** Quit or reboot and every session is still there: write to it
  and its agent carries on, or fork it onto another agent.
- **Answers you can see.** Agents answer with charts, tables and mock-ups, drawn in a sandbox
  right in the conversation, as are the apps MCP servers attach to their tools.
- **From issue to merged pull request.** Start a task from a GitHub issue, review its diff,
  commit, push, open the pull request and merge it, while Swarmed tells you when checks fail or
  a review comes in.
- **A browser per task.** Each task has a live Chrome beside its sessions, on a profile of its
  own, where agents' Playwright and DevTools tools act while you watch.
- **Cloud and phone.** Hand work to a Codex or Claude Code cloud agent, or carry a Claude Code
  session on from your phone with Remote Control.
- **Made yours.** Palettes, layouts, your own stylesheet, the status line, actions and rules,
  changed by hand, in files, from the command line or by asking an agent.
- **Scriptable.** The `swarmed` command drives the same core and data as the app, from a
  terminal, a script or an agent's shell.

There is more: chats with no repository, a diff and a rewind for every turn, usage and cost,
and the rest in **[Features](docs/features.md)**.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/app-dark.png">
  <img src="docs/images/app-light.png" alt="Swarmed with a session open: projects, tasks and sessions in the sidebar, a conversation in which Claude Code asks to push, and the task's panel with its pull request and checks">
</picture>

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/ryckakas/swarmed/main/install.sh | sh
```

puts Swarmed.app in `/Applications` (`~/Applications` when that cannot be written to) and links
the `swarmed` command into `~/.local/bin`. Or with Homebrew, which puts `swarmed` on the PATH as
well:

```sh
brew install --cask ryckakas/tap/swarmed
```

Swarmed then updates itself: it installs a new version when you quit, or from About at once
(Settings → About turns that off). Running the install script again updates it too.

`cargo install swarmed` builds the command alone. The releases are built for macOS so far, so
elsewhere that is how to get the command.

Swarmed is not signed with an Apple Developer ID yet. Installed with Homebrew or downloaded by
hand, macOS stops it the first time it opens: allow it under System Settings → Privacy & Security
→ Open Anyway.

## Get started

Swarmed runs agents you have installed and signed in to yourself; Claude Code is the default.
Open the app, add a repository, start a task and say what the agent should do. Or press New task
with no repository picked, which starts a chat. The app speaks
in swarm words, unless Icons and words in Settings → Appearance is set to Boring: adding a
repository is **Found a colony**, a new task **New brood**.

Or from a terminal:

```sh
swarmed project add ~/code/shop
swarmed task new shop fix-login
swarmed session run shop fix-login "Fix the login form's error message"
```

## Documentation

The documentation site is at **[swarmed.kauneckas.dev](https://swarmed.kauneckas.dev)**. It is
written from the files below, which can also be read here:

- [Features](docs/features.md): everything Swarmed does, in short
- [Projects and configuration](docs/configuration.md): hooks, ports, environments and where
  Swarmed keeps things
- [Coding agents](docs/coding-agents.md): adding agents, MCP servers, skills, and the tools
  Swarmed gives them
- [The `swarmed` command](docs/command.md)
- [Architecture and development](docs/architecture.md): how it is built, and building it
- [Security](SECURITY.md): what Swarmed keeps, what it hands on, and reporting a vulnerability
- [Changelog](CHANGELOG.md): what changed in each release

## License

MIT
