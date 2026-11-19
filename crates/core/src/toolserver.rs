//! Swarmed's own tools for agents, served over the Model Context Protocol on standard input and
//! output.
//!
//! Each front end's executable becomes this server when started with [`TOOL_SERVER_ARG`], and
//! every agent is given it under the name [`SERVER_NAME`]. The one tool, [`SHOW_HTML`], does
//! nothing here: its call is part of the conversation, which is where a front end finds the HTML
//! and shows it. It exists because an agent asked for something visual goes looking for a tool,
//! and should find one that puts the result in the conversation.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// The argument that makes a front end's executable serve the tools instead of starting up.
pub const TOOL_SERVER_ARG: &str = "tool-server";
/// The name the server is given to agents under.
pub const SERVER_NAME: &str = "swarmed";
/// The tool that shows HTML to the user in the conversation. Its input has the HTML as `html`.
pub const SHOW_HTML: &str = "show_html";
/// That tool as Claude Code names it: by server, then by tool.
pub(crate) const SHOW_HTML_FOR_CLAUDE: &str = "mcp__swarmed__show_html";

/// The environment variable that tells the server where to leave the views it is given. The
/// core sets it to a folder of the session's own, so a view reaches the right conversation
/// whichever way the agent came to call the tool: directly, from a script, or from a subagent.
pub(crate) const VIEW_SPOOL_ENV: &str = "SWARMED_VIEW_SPOOL";
const VIEW_EXTENSION: &str = "html";
/// The environment variables that tell the server which session it serves, and where the core
/// that session belongs to is listening.
pub(crate) const SESSION_ENV: &str = "SWARMED_SESSION";
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
    }, {
        "name": "list_sessions",
        "title": "List the user's other agent sessions",
        "description": "Lists the agent sessions the user has open in this project, yours \
            included: each one's id, whether it is idle, working or waiting for the user, the \
            task (worktree) it is in, and its title. Use it to see who else is at work before \
            reading from or writing to another session.",
        "inputSchema": { "type": "object", "properties": {} },
    }, {
        "name": "read_session",
        "title": "Read another session's latest messages",
        "description": "Returns what was last said in another session, by the user and by \
            its agent, oldest first. Tool calls and their output are left out.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session_id": { "type": "integer", "description": "The session to read." },
                "messages": {
                    "type": "integer",
                    "description": "How many of its latest messages; 12 if left out.",
                },
            },
            "required": ["session_id"],
        },
    }, {
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
        "description": "Starts a new agent session in your own task and worktree, on the same \
            agent and settings as you, and gives it your prompt. It works at the same time as \
            you and on the same files, so give it a part of the work that does not collide \
            with yours, and everything it needs to know: it starts with no knowledge of your \
            conversation. It reports back to you with a message when it is done. A session \
            that was itself spawned cannot spawn, and you can have a few open at a time.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "prompt": { "type": "string", "description": "What the new session is to do." },
                "title": { "type": "string", "description": "A few words naming it." },
            },
            "required": ["prompt"],
        },
    }])
}

/// Hands the HTML of a `show_html` call to the core, by leaving it as a file in `spool`. The
/// core picks it up when the call's result arrives. Files are named so that they sort in the
/// order they were written, and appear whole: written under another name, then renamed.
fn deliver(spool: &Path, params: &Value) -> std::io::Result<()> {
    let html = params.pointer("/arguments/html").and_then(Value::as_str);
    let Some(html) = html else {
        return Ok(());
    };
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

/// Shows a view: leaves its HTML for the core and says whether that worked.
fn show_html(params: &Value, link: &Link) -> (String, bool) {
    let delivered = link
        .spool
        .as_deref()
        .is_none_or(|spool| deliver(spool, params).is_ok());
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
/// read, so it is handed over once.
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
        html
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
}
