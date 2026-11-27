//! Swarmed's own tools for agents, served over the Model Context Protocol on standard input and
//! output.
//!
//! Each front end's executable becomes this server when started with [`TOOL_SERVER_ARG`], and
//! every agent is given it under the name [`SERVER_NAME`]. [`SHOW_HTML`] does nothing here: its
//! call is part of the conversation, which is where a front end finds the HTML and shows it. It
//! exists because an agent asked for something visual goes looking for a tool, and should find
//! one that puts the result in the conversation. [`SHOW_BROWSER`] beside it opens the task's
//! browser for the user, for the same reason: an agent asked to look at a page should find the
//! browser the user sees, not launch one. The rest are answered by the core, `start_task`
//! among them: work that needs a branch of its own goes into a task of its own.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::agent::Effort;

/// The argument that makes a front end's executable serve the tools instead of starting up.
pub const TOOL_SERVER_ARG: &str = "tool-server";
/// The name the server is given to agents under.
pub const SERVER_NAME: &str = "swarmed";
/// The tool that shows HTML to the user in the conversation. Its input has the HTML as `html`.
pub const SHOW_HTML: &str = "show_html";
/// The tool that opens the task's browser beside the conversation.
///
/// Answered by the core, as the session tools are, but not at once: the browser may have to start
/// first. Never on Claude Code's allow list: it navigates with the task's logins.
pub const SHOW_BROWSER: &str = "show_browser";
/// The tool that hands work to a cloud agent. Answered by the core, as the session tools are,
/// but not at once: the provider is asked first.
pub(crate) const DELEGATE_TO_CLOUD: &str = "delegate_to_cloud";
/// The tool that starts a new task for work that needs a branch of its own. Answered by the
/// core, as the session tools are, but not at once: the repository is fetched first.
pub(crate) const START_TASK: &str = "start_task";
/// That tool as Claude Code names it: by server, then by tool.
pub(crate) const SHOW_HTML_FOR_CLAUDE: &str = "mcp__swarmed__show_html";

/// The environment variable that tells the server where to leave the views it is given. The
/// core sets it to a folder of the session's own, so a view reaches the right conversation
/// whichever way the agent came to call the tool: directly, from a script, or from a subagent.
pub(crate) const VIEW_SPOOL_ENV: &str = "SWARMED_VIEW_SPOOL";
const VIEW_EXTENSION: &str = "html";
/// The environment variables that tell the server which session it serves, and where the core
/// that session belongs to is listening.
pub const SESSION_ENV: &str = "SWARMED_SESSION";
pub(crate) const SOCKET_ENV: &str = "SWARMED_SOCKET";

/// What ties this server to the core: the session whose agent started it, and how to reach
/// the core on its behalf.
#[derive(Debug, Default)]
struct Link {
    /// Where views are left for the core to pick up; nowhere, if the core named no place.
    spool: Option<PathBuf>,
    /// The socket the core answers session tools on.
    socket: PathBuf,
    /// The session this server's agent runs.
    session: i64,
}

/// The protocol revision answered with when the agent names none.
const PROTOCOL: &str = "2025-06-18";
const METHOD_NOT_FOUND: i64 = -32601;

fn tools() -> Value {
    let mut tools = local_tools();
    if let Some(tools) = tools.as_array_mut() {
        tools.push(cloud_tool());
        tools.push(skill_tool());
        tools.push(mark_tool());
        tools.push(configure_tool());
    }
    tools
}

/// Whether the server serves a tool called `name`.
#[cfg(test)]
pub(crate) fn serves(name: &str) -> bool {
    let tools = tools();
    let mut all = tools.as_array().into_iter().flatten();
    all.any(|tool| tool["name"] == name)
}

/// The tools about what happens on this machine: showing views, and the other sessions.
fn local_tools() -> Value {
    json!([{
        "name": SHOW_HTML,
        "title": "Show HTML in the conversation",
        "description": "Shows HTML to the user at once, inline in the conversation, in a \
            sandboxed frame. This is the tool for rendering, previewing or displaying anything \
            visual or interactive: a chart, a diagram, a dashboard, a sortable table, a mock-up. \
            Use it instead of writing an HTML file, starting a server, opening a browser or \
            taking screenshots. Pass one self-contained document or fragment: inline CSS and \
            JavaScript only, no network (no CDN scripts, web fonts or remote images), data \
            embedded. The app's colours are available as the CSS variables --text, --muted, \
            --background, --line and --accent. Writing the same HTML in your reply as a fenced \
            code block with the language html-view does exactly the same, without a tool call.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "html": {
                    "type": "string",
                    "description": "The HTML to show: a fragment or a whole document.",
                },
                "title": {
                    "type": "string",
                    "description": "A few words saying what the view is.",
                },
            },
            "required": ["html"],
        },
    },
    show_browser_tool(),
    start_task_tool(),
    {
        "name": "list_sessions",
        "title": "List the user's other agent sessions",
        "description": "Lists the agent sessions the user has open in this project, yours \
            included: each one's id, whether it is idle, working or waiting for the user, the \
            task (worktree) it is in, the session it works for, its role, the model and effort \
            it runs on, and its title. Use it to see who else is at work, and who works for \
            you, before reading from or writing to another session.",
        "inputSchema": { "type": "object", "properties": {} },
    },
    read_session_tool(),
    {
        "name": "send_message",
        "title": "Send a message to another session",
        "description": "Leaves another session a message, which arrives in its conversation \
            marked as coming from you: at once if that session is idle, else when its current \
            turn ends. Its agent then acts on it. Say what you need or what you found in full: \
            the other session does not see your conversation.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session_id": { "type": "integer", "description": "The session to write to." },
                "text": { "type": "string", "description": "The message." },
            },
            "required": ["session_id", "text"],
        },
    }, {
        "name": "spawn_session",
        "title": "Start a new session beside you",
        "description": "Starts a new agent session in your own task and worktree, on your \
            agent and settings unless you choose its model and effort, and gives it your \
            prompt. It works at the same time as \
            you and on the same files and branch, so give it a part of the work that does not \
            collide with yours, and everything it needs to know: it starts with no knowledge \
            of your conversation. It works for you, and reports back to you with a message \
            when it is done. A session that was itself spawned cannot spawn, and you can have \
            a few open at a time. For work on another branch, use start_task instead.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "prompt": { "type": "string", "description": "What the new session is to do." },
                "title": { "type": "string", "description": "A few words naming it." },
                "role": role_argument(),
                "model": model_argument(),
                "effort": effort_argument(),
            },
            "required": ["prompt"],
        },
    }, {
        "name": "close_session",
        "title": "Close a session you started",
        "description": "Closes a session that you started with spawn_session or start_task and \
            that works for you, once it is idle: it leaves the sidebar and its agent is \
            stopped. Its conversation is kept and the user can reopen it; a task's worktree, \
            branch and pull request stay. Use it when such a session is done and you have \
            what you need from it, so finished helpers do not pile up and you have room to \
            start more. You cannot close a session you did not start, one that is still \
            working, or one whose task is still being set up.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session_id": { "type": "integer", "description": "The session to close." },
            },
            "required": ["session_id"],
        },
    }])
}

/// The role a session an agent starts is given, as `spawn_session` and `start_task` take it.
fn role_argument() -> Value {
    let names: Vec<&str> = crate::ROLES.iter().map(|role| role.name).collect();
    json!({
        "type": "string",
        "enum": names,
        "description": "What the new session is for. Left out, the role yours starts by \
            default, or none if you have no role. A role may start only some roles.",
    })
}

/// The model a session an agent starts runs on, as `spawn_session` and `start_task` take it.
fn model_argument() -> Value {
    json!({
        "type": "string",
        "description": "The model it runs on, one your agent offers. Left out, its role's \
            default in the user's settings, or yours.",
    })
}

/// How hard a session an agent starts thinks, as `spawn_session` and `start_task` take it.
fn effort_argument() -> Value {
    json!({
        "type": "string",
        "enum": Effort::ALL.map(Effort::as_str),
        "description": "How hard it thinks. Left out, its role's default in the user's \
            settings, or yours.",
    })
}

/// The tool that starts a new task, with a branch, worktree and session of its own.
fn start_task_tool() -> Value {
    let text = |description: &str| json!({ "type": "string", "description": description });
    json!({
        "name": START_TASK,
        "title": "Start a new task on a branch of its own",
        "description": "Starts a new task in this repository, as the user's New task does: a \
            branch and worktree of its own, set up as the repository says, and a new agent \
            session on your agent and settings unless you choose its model and effort, which \
            gets your prompt once the setup is \
            done, works for you, and reports back to you with a message when it is done. The \
            user sees it in the sidebar and can watch and answer it. Use it for work that \
            needs a branch of its own: a separate change, a fix to another pull request (pass \
            its `branch`), the next pull request of a stack (pass `from`). Never switch your \
            own worktree to another branch for such work. Start one when the user asks for \
            that work; when it is only your own idea, ask first. The new session knows \
            nothing of your conversation, so the prompt has to say everything it needs. For a \
            part of your own task's work, on your branch, use spawn_session instead.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": text("The task's name: lower-case words joined by dashes, such as \
                    fix-login-redirect. A name that is taken gets a number."),
                "prompt": text("What the new session is to do."),
                "title": text("A few words naming the new session."),
                "branch": text("An existing branch to check out instead of making a new one, \
                    such as a pull request's head branch."),
                "from": text("A branch to start the new branch from instead of the \
                    repository's base, such as the head branch of the pull request this one \
                    stacks on. It becomes the task's base."),
                "role": role_argument(),
                "model": model_argument(),
                "effort": effort_argument(),
            },
            "required": ["name", "prompt"],
        },
    })
}

/// The tool that opens the task's browser for the user.
fn show_browser_tool() -> Value {
    json!({
        "name": SHOW_BROWSER,
        "title": "Show the task's browser beside the conversation",
        "description": "Opens the task's own browser where the user sees it, beside the \
            conversation: a Chrome with a profile of the task's own, so what the user signed in \
            to there stays signed in. Pass an address to open it on, or none for the task's \
            running app. It opens in a tab of yours, starting the browser if it is not running, \
            and answers with its address for browser tools, its tabs, and how to act in yours. \
            Use it instead of launching a browser or opening a window of your own. It only \
            shows: clicking, typing, reading the page and screenshots are for browser tools \
            connected to its address (Playwright MCP, Chrome DevTools MCP), or for a script \
            that connects to it; the address is in SWARMED_BROWSER.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Where to go, such as http://localhost:3000/cart; the task's \
                        running app if left out.",
                },
            },
        },
    })
}

/// The tool an agent reads another session with: its latest messages, or from one on.
fn read_session_tool() -> Value {
    json!({
        "name": "read_session",
        "title": "Read another session's latest messages",
        "description": "Returns what was last said in another session, by the user and by \
            its agent, oldest first, after who works with it. Tool calls and their output are \
            left out.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session_id": { "type": "integer", "description": "The session to read." },
                "messages": {
                    "type": "integer",
                    "description": "How many messages; 12 if left out.",
                },
                "from": {
                    "type": "integer",
                    "description": "The message to start at, 1 being the first; the latest \
                        ones if left out.",
                },
            },
            "required": ["session_id"],
        },
    })
}

/// The tool an agent changes Swarmed's own configuration with.
fn configure_tool() -> Value {
    let text = |description: &str| json!({ "type": "string", "description": description });
    json!({
        "name": "configure",
        "title": "Change Swarmed's own settings",
        "description": "Reads or changes the configuration of Swarmed, the app the user runs \
            you in. Use it when the user asks for it: \"keep that as a prompt\", \"add a button \
            that starts the dev server\", \"add a rule: never use em dashes\". Choose a `kind` \
            and an `op` (list, save, remove). \
            prompt: a text kept under `name` (lower-case, dashes) that the user sends again by \
            typing /name; give `name` and `text`. \
            action: a button in the task panel that sends `text` to the agent or runs \
            `command` in the session's terminal; give `name`, optionally `icon`, and one of \
            the two; `everywhere` keeps it for every repository instead of this one. \
            rule: one line every agent is told to keep to; give `text`. \
            status_line: the pieces under the message box, as `items` in order (branch, \
            changes, model, context, cost, agent, worktree, duration), or a script as `command`. \
            appearance: `theme`, `palette`, `style`, `layout`, `text_size`, `tool_calls` (hiding \
            them is only proposed), `turn_end_lines` (true or false), `limit_warnings` (the \
            usage notice at the top of the window: frequent, rare, none), `math` (which \
            formulas are drawn: off, standard, with_single_dollars). \
            layout: the user's custom layout, one JSON object of sizes, spacing, fonts and \
            colours; list it to get its schema, save the whole of it as `json`. Saving \
            switches the app to it; what cannot be read, such as text too close to its \
            background, is refused with the reason. \
            terminal: how the terminal looks and behaves (font, size, cursor, scrollback, \
            colours and more), one JSON object; list it for its schema, save the whole of it \
            as `json`. \
            environment: how a task's stack is set up, started and stopped in this repository; \
            give `name` and any of `setup`, `start`, `stop`, `teardown`, `open_url`, `ports`, \
            `variables`. When asked to set a repository up, read how it starts (README, \
            package scripts, compose files, Makefile), list this kind first for the rules \
            and the variables that keep tasks apart, try the commands, then save. \
            Rules, environments, a command for an action and a script for the status line are \
            not made at once: they are proposed to the user, who accepts or declines them in \
            the conversation. API keys, MCP servers, sign-ins, permissions and notes cannot be \
            changed this way. List before you change something you have not seen.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": [
                        "prompt", "action", "rule", "status_line", "appearance", "layout",
                        "terminal", "environment",
                    ],
                },
                "op": { "type": "string", "enum": ["list", "save", "remove"] },
                "name": text("The name of the prompt, action or environment."),
                "text": text("A prompt's text, an action's prompt, or a rule."),
                "command": text("An action's shell command, or the status line's script."),
                "icon": text("One emoji shown before an action's name."),
                "everywhere": {
                    "type": "boolean",
                    "description": "An action for every repository instead of this one.",
                },
                "items": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "The status line's pieces, in order.",
                },
                "theme": text("system, dark or light."),
                "palette": text("dusk, graphite, forest, plum, ember or swarm."),
                "style": text("plain, swarm or colony."),
                "layout": text("classic, senior for larger text and rows, or custom."),
                "json": {
                    "type": "object",
                    "description": "The whole custom layout, for kind layout, or the whole terminal configuration, for kind terminal.",
                },
                "text_size": { "type": "integer", "description": "In percent, 80 to 150." },
                "setup": text("An environment's command run once after a worktree is created."),
                "start": text("An environment's command that starts what keeps running."),
                "stop": text("An environment's command run when its stack is stopped."),
                "teardown": text("An environment's command run before a worktree is removed."),
                "open_url": text("Where an environment's running stack is reached."),
                "ports": {
                    "type": "object",
                    "additionalProperties": { "type": "integer" },
                    "description": "An environment's base ports by name, used as {{port.<name>}}; 0 removes one.",
                },
                "variables": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "An environment's variables by name; an empty text removes one. \
                        Their values are shown to the user and never read back to you.",
                },
            },
            "required": ["kind", "op"],
        },
    })
}

/// The tool an agent marks its own session with.
fn mark_tool() -> Value {
    json!({
        "name": "set_mark",
        "title": "Mark your session in the sidebar",
        "description": "Puts a colour and a few words on your own session in the user's \
            sidebar, so they see at a glance how its work stands without opening it: green \
            for a check that passed, red for one that failed, yellow for something they \
            should look at. Use it when the user, a skill or a prompt asks for a mark, not as \
            a matter of course. The mark stays until you set it again or the user writes to \
            you; the colour `none` takes it off.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "colour": {
                    "type": "string",
                    "enum": ["green", "blue", "yellow", "red", "purple", "none"],
                    "description": "The colour of the mark, or `none` to take it off.",
                },
                "label": {
                    "type": "string",
                    "description": "A few words shown beside it, such as `e2e passed`. At \
                        most 40 characters; may be left out.",
                },
            },
            "required": ["colour"],
        },
    })
}

/// The tool that hands over a skill the user has with another agent or with Swarmed.
fn skill_tool() -> Value {
    json!({
        "name": "use_skill",
        "title": "Use one of the user's skills",
        "description": "Gives you the instructions of a skill named in the list of skills \
            from Swarmed you were given at the start: skills the user keeps with another \
            agent or in Swarmed, which you do not have by yourself. Call it before doing work \
            the skill covers, then follow what it says. Its answer ends with the folder its \
            other files are in.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "The skill's name, as listed." },
            },
            "required": ["name"],
        },
    })
}

/// The tool that hands work to a cloud agent.
fn cloud_tool() -> Value {
    json!({
    "name": DELEGATE_TO_CLOUD,
    "title": "Hand work to a cloud agent",
    "description": "Hands a self-contained piece of work to an agent that runs in the \
        cloud (Codex's, or Claude Code's if `provider` says so), on its own copy of this \
        repository taken from GitHub. Use it for work that \
        can go on without this machine and without you, when the user asks for something \
        to be delegated or run in the cloud. The cloud agent sees only what has been \
        pushed: commits and changes that exist only in this worktree are not there, so \
        push first if it needs them. It starts with no knowledge of your conversation, so \
        the prompt has to say everything. It does not report back to you: you get the \
        address where the user follows it. Refused if the repository is not on GitHub, \
        has no cloud environment yet, or has not been trusted by the user; pass that on \
        to the user as it is.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "prompt": { "type": "string", "description": "What the cloud agent is to do." },
            "provider": {
                "type": "string",
                "enum": ["codex", "claude"],
                "description": "Whose cloud: `codex` (the default) or `claude`.",
            },
        },
        "required": ["prompt"],
    },
    })
}

/// Hands the HTML of a `show_html` call to the core, by leaving it as a file in `spool`. The
/// core picks it up when the call's result arrives. Files are named so that they sort in the
/// order they were written, and appear whole: written under another name, then renamed.
///
/// The view comes from the tool, not from the call: agents call it directly, from a script or
/// from a sub-agent, and the tool gets the HTML whichever way. Reading it out of the call's
/// own shape (`shownHtml` in the UI) is only for conversations recorded before.
fn deliver(spool: &Path, html: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(spool)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let name = format!("{:020}-{}", now.as_nanos(), std::process::id());
    let unfinished = spool.join(format!("{name}.part"));
    std::fs::write(&unfinished, html)?;
    std::fs::rename(unfinished, spool.join(format!("{name}.{VIEW_EXTENSION}")))
}

/// Asks the core, which this server's agent belongs to, to carry out a session tool. The
/// answer is what the agent is told, and whether it is an error.
fn ask_core(link: &Link, tool: &str, arguments: &Value) -> std::io::Result<(String, bool)> {
    let mut stream = UnixStream::connect(&link.socket)?;
    let request = json!({ "session": link.session, "tool": tool, "arguments": arguments });
    writeln!(stream, "{request}")?;
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer)?;
    let reply: Value = serde_json::from_str(&answer).unwrap_or_default();
    let text = reply
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let ok = reply.get("ok").and_then(Value::as_bool).unwrap_or(false);
    Ok((text.to_string(), !ok))
}

/// Shows a view: leaves its HTML for the core and says whether that worked. A call with no
/// HTML in it shows nothing, and the agent is told so instead of that it was shown.
fn show_html(params: &Value, link: &Link) -> (String, bool) {
    let html = params.pointer("/arguments/html").and_then(Value::as_str);
    let Some(html) = html.filter(|html| !html.trim().is_empty()) else {
        let empty = "The `html` was empty, so nothing was shown. Call again with the HTML to show.";
        return (empty.to_string(), true);
    };
    let delivered = link
        .spool
        .as_deref()
        .is_none_or(|spool| deliver(spool, html).is_ok());
    if delivered {
        let shown = "Shown to the user in the conversation. Do not repeat the HTML in your reply.";
        return (shown.to_string(), false);
    }
    let lost = "The view could not be handed to Swarmed, so the user does not see it.";
    (lost.to_string(), true)
}

/// What calling a tool returns to the agent.
fn call(params: &Value, link: &Link) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = params.get("arguments").unwrap_or(&Value::Null);
    let (text, is_error) = if name == SHOW_HTML {
        show_html(params, link)
    } else {
        ask_core(link, name, arguments).unwrap_or_else(|_| {
            let down = "Swarmed is not answering, so this tool is not available right now.";
            (down.to_string(), true)
        })
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

/// The answer to one request, or `None` for a notification, which gets none.
fn answer(message: &Value, link: &Link) -> Option<Value> {
    let id = message.get("id")?;
    let params = message.get("params").unwrap_or(&Value::Null);
    let result = match message.get("method").and_then(Value::as_str)? {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            json!({
                "protocolVersion": asked.unwrap_or(PROTOCOL),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => call(params, link),
        method => {
            let error = json!({ "code": METHOD_NOT_FOUND, "message": format!("no `{method}`") });
            return Some(json!({ "jsonrpc": "2.0", "id": id, "error": error }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Serves the tools: one JSON message per line in, one per line out, until `input` ends.
///
/// A line that is not JSON is passed over, as the protocol has no one to report it to. Views
/// are handed to the core through the folder its environment names, when the core set one.
pub fn serve_tools(input: impl BufRead, output: impl Write) -> std::io::Result<()> {
    let link = Link {
        spool: std::env::var_os(VIEW_SPOOL_ENV).map(PathBuf::from),
        socket: std::env::var_os(SOCKET_ENV).map_or_else(PathBuf::new, PathBuf::from),
        session: std::env::var(SESSION_ENV)
            .ok()
            .and_then(|id| id.parse().ok())
            .unwrap_or(0),
    };
    serve(input, output, &link)
}

fn serve(input: impl BufRead, mut output: impl Write, link: &Link) -> std::io::Result<()> {
    for line in input.lines() {
        let message = serde_json::from_str::<Value>(&line?).ok();
        if let Some(reply) = message.as_ref().and_then(|message| answer(message, link)) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// The views left in `spool` since it was last emptied, oldest first. Each is removed as it is
/// read, so it is handed over once. One with nothing in it is no view, and is only removed.
pub(crate) fn take_views(spool: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(spool) else {
        return Vec::new();
    };
    let finished = |path: &PathBuf| path.extension().is_some_and(|ext| ext == VIEW_EXTENSION);
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(finished)
        .collect();
    files.sort();
    let take = |file: PathBuf| {
        let html = std::fs::read_to_string(&file).ok();
        std::fs::remove_file(&file).ok();
        html.filter(|html| !html.trim().is_empty())
    };
    files.into_iter().filter_map(take).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the server writes for these lines, parsed.
    fn replies(lines: &[Value]) -> Vec<Value> {
        let lines: Vec<String> = lines.iter().map(Value::to_string).collect();
        let input = lines.join("\n");
        let mut output = Vec::new();
        serve(input.as_bytes(), &mut output, &Link::default()).expect("serves");
        let text = String::from_utf8(output).expect("text");
        text.lines()
            .map(|line| serde_json::from_str(line).expect("json"))
            .collect()
    }

    #[test]
    fn an_agent_can_connect_list_the_tool_and_call_it() {
        let request = |id: u64, method: &str, params: Value| json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let said = replies(&[
            request(1, "initialize", json!({ "protocolVersion": "2025-03-26" })),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            request(2, "tools/list", json!({})),
            request(
                3,
                "tools/call",
                json!({ "name": SHOW_HTML, "arguments": { "html": "<p>hi</p>" } }),
            ),
            request(4, "tools/call", json!({ "name": "other" })),
            request(5, "resources/list", json!({})),
        ]);
        // The notification gets no answer, so five replies for six lines.
        assert_eq!(said.len(), 5);
        assert_eq!(said[0]["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(said[0]["result"]["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(said[1]["result"]["tools"][0]["name"], SHOW_HTML);
        assert_eq!(said[1]["result"]["tools"][1]["name"], SHOW_BROWSER);
        assert_eq!(
            said[1]["result"]["tools"][0]["inputSchema"]["required"],
            json!(["html"])
        );
        assert_eq!(said[2]["result"]["isError"], false);
        assert_eq!(said[3]["result"]["isError"], true);
        assert_eq!(said[4]["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn a_line_that_is_not_json_is_passed_over() {
        let mut output = Vec::new();
        let input = "not json\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
        serve(input.as_bytes(), &mut output, &Link::default()).expect("serves");
        let text = String::from_utf8(output).expect("text");
        assert_eq!(text.lines().count(), 1);
        assert_eq!(
            SHOW_HTML_FOR_CLAUDE,
            format!("mcp__{SERVER_NAME}__{SHOW_HTML}")
        );
    }

    #[test]
    fn a_shown_view_is_left_for_the_core_once_and_in_order() {
        let dir = tempfile::tempdir().expect("temp dir");
        let spool = dir.path().join("spool");
        let show = |id: u64, html: &str| {
            json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": SHOW_HTML, "arguments": { "html": html } } })
        };
        let input = format!(
            "{}\n{}\n",
            show(1, "<p>first</p>"),
            show(2, "<p>second</p>")
        );
        let mut output = Vec::new();
        let link = Link {
            spool: Some(spool.clone()),
            ..Link::default()
        };
        serve(input.as_bytes(), &mut output, &link).expect("serves");
        assert_eq!(
            String::from_utf8(output)
                .expect("text")
                .matches("Shown to")
                .count(),
            2
        );

        // A file still being written is not a view yet.
        std::fs::write(spool.join("0-1.part"), "<p>half").expect("writes");
        assert_eq!(take_views(&spool), ["<p>first</p>", "<p>second</p>"]);
        assert_eq!(take_views(&spool), Vec::<String>::new());
        assert_eq!(take_views(&dir.path().join("none")), Vec::<String>::new());
    }

    #[test]
    fn a_view_with_nothing_in_it_is_refused_and_never_shown() {
        let dir = tempfile::tempdir().expect("temp dir");
        let spool = dir.path().join("spool");
        let link = Link {
            spool: Some(spool.clone()),
            ..Link::default()
        };
        for arguments in [json!({ "html": "" }), json!({ "html": " \n\t" }), json!({})] {
            let params = json!({ "name": SHOW_HTML, "arguments": arguments });
            let (said, is_error) = show_html(&params, &link);
            assert!(is_error, "{arguments}");
            assert!(said.contains("empty"), "{said}");
        }
        assert_eq!(take_views(&spool), Vec::<String>::new());

        // One that got into the folder some other way is not a view either, and is cleared.
        std::fs::create_dir_all(&spool).expect("creates");
        let blank = spool.join(format!("1-1.{VIEW_EXTENSION}"));
        std::fs::write(&blank, "  \n").expect("writes");
        std::fs::write(spool.join(format!("2-1.{VIEW_EXTENSION}")), "<p>hi</p>").expect("writes");
        assert_eq!(take_views(&spool), ["<p>hi</p>"]);
        assert!(!blank.exists());
    }
}
