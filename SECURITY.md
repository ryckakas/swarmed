# Security policy

## Supported versions

Only the latest release gets security fixes. Swarmed is pre-1.0, so a fix ships as a new release
rather than a backport; update to pick it up (run the install script again, or
`brew upgrade --cask swarmed`).

## Reporting a vulnerability

Report it privately through GitHub:
**[Report a vulnerability](https://github.com/ryckakas/swarmed/security/advisories/new)**.
Please don't open a public issue, pull request or discussion for it.

A useful report says which version and install source you used (the install script, Homebrew,
Cargo or a build from source), which agent the session ran on, what you did, and what happened.

The report stays private while it is investigated and fixed. Once a fixed release is out, the
advisory is published with credit to you, unless you'd rather not be named.

## What Swarmed promises

- **It never signs you in and never reads the logins agents keep.** Each agent uses its own.
  GitHub is reached through your own `gh`; Swarmed holds no token.
- **API keys, and the secrets of MCP servers set up in Swarmed, live in the macOS Keychain.**
  Swarmed never shows them again and never logs them. API keys go to agents other than Claude
  Code, and a server's secrets to that server.
- **MCP servers are handed on as they are configured.** To share servers between agents, Swarmed
  reads Claude Code's, Codex's and OpenCode's MCP configuration, secrets in it included, and
  passes each server to the agents of your sessions. Claude Code gets its servers in a file only
  you can read, removed when the agent ends (after a crash, when its session next starts), never
  on a command line other processes can see.
- **Notes stay out of agents' hands.** Notes, and the environment files Swarmed creates, are
  readable by you alone, and notes are never handed to an agent. An environment's variables are
  set in its tasks' processes, agents included, but Swarmed's own tools tell an agent only their
  names, so their values stay out of conversations. Snapshots, which may hold `.env` files, sit
  in folders only you can open.
- **A pasted image is the one thing of a message Swarmed keeps.** Attached files are named by
  their paths and never copied. An image pasted into a message has no path, so it is written
  into `attachments/<session>/` in the data directory, readable by you alone, and its path is
  attached. It is deleted when the session is deleted or its task removed; closing a session
  keeps it. Nothing pasted is written into a repository or a worktree.
- **An image a tool returns is shown to you and nobody else.** A screenshot can show anything a
  page showed, a password included. Images a session's tools return are kept beside its pasted
  images, readable by you alone, deleted with them, and never logged, handed to another
  session, a fork or a cloud agent, or searched.
- **A task's browser runs in Chrome, never in Swarmed's window.** The window only draws
  pictures Chrome sends of the page, so no page can reach Swarmed's commands. Chrome's DevTools
  endpoint listens on `127.0.0.1` only, on a port Chrome picks: other programs you run can reach
  it, as they can any local dev server, but web pages cannot, since Swarmed never lets an origin
  in (`--remote-allow-origins`). Its profile, which holds what you signed in to there, is the
  task's own, readable by you alone and deleted with the task. What a page shows or logs is
  shown to you and never logged.
- **Agents' browser tools reach the task's browser through an address of their session's
  own.** Swarmed listens at it on `127.0.0.1` only, on a port the system picks, and passes what
  comes on to Chrome unchanged. A request a web page made is refused before Chrome is even
  started: one with an `Origin`, one a browser marks as coming from a site, or one that names a
  host other than this machine. On the way Swarmed reads only which tab a tool acts in, and
  keeps or logs nothing of what passes. A login you made in the task's browser can be used by
  the task's agents, as anything else in its profile can: that is what lets them check pages
  behind it. Agents cannot change any of this through `configure`.
- **An agent's HTML cannot reach the app.** Views are shown in a sandboxed frame on an origin of
  their own, under a policy that lets them make no request. A link in a view opens only when you
  press Open.
- **What an agent may change in Swarmed is limited.** Its `configure` tool cannot reach API keys,
  MCP servers, sign-ins, supervision or notes. A rule, an environment, an action that runs a
  command and a status line script are added, and tool calls are hidden, only when you accept
  its proposal.
- **A value in a hook is one word.** A branch or task name cannot run as a command.
- **A notification never carries the input of a Claude Code tool.** It names the tool with the
  agent's own summary, or gives the first line of a finished answer. Other agents name their tool
  calls themselves, and a notice shows that name as given.
- **Swarmed makes no network requests of its own** apart from fetching OpenRouter's public model
  list, with nothing of yours in it, running an agent's or a browser's installer when you ask
  for one, checking whether a stack answers on this machine, and looking for a new version of
  itself. That last one fetches `latest.json` from this repository's latest GitHub release, at
  launch and every few hours while updating automatically is on, and otherwise only when About opens; the
  request names the updater, not you or your version. When a newer version is out, the archive
  that file names is downloaded too: by itself while updating automatically is on, and otherwise
  when you ask for the update. There is no telemetry. The programs it
  runs for you, such as the agents, `git`, `gh` and a task's Chrome, make their own.
- **An update is installed only if it is signed with the release key,** whose public half is
  built into the app, and only for the version it was signed as: neither a changed archive nor
  an older release presented as newer goes in. It is installed as Swarmed quits, never while it
  runs. Only a release updates itself: an app you built from the sources is left as you built
  it.

Swarmed does not sandbox the agents. An agent runs as you, in its task's worktree, and what it may
do is decided by the session's supervision and the agent itself.

## What counts

Anything that breaks one of the promises above is in scope, for example:

- An agent's output, a repository's content or a branch name causing code to run, or a secret
  reaching a log, a file, a notification or an agent the promises above keep it from.
- An HTML view reaching the app, the network or your files.
- A problem in a release artefact or the way it is built and published: the app, the install
  script, the Homebrew cask or the crates.

An agent doing what its supervision allowed is not a Swarmed vulnerability. A vulnerability in a
dependency that Swarmed doesn't actually reach is better raised as a normal issue.
