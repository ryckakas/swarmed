//! Swarmed's own tools for agents, served over the Model Context Protocol on standard input and
//! output.
//!
//! Each front end's executable becomes this server when started with [`TOOL_SERVER_ARG`], and
//! every agent is given it under the name [`SERVER_NAME`]. The one tool, [`SHOW_HTML`], does
//! nothing here: its call is part of the conversation, which is where a front end finds the HTML
//! and shows it. It exists because an agent asked for something visual goes looking for a tool,
//! and should find one that puts the result in the conversation.

use std::io::{BufRead, Write};
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

/// What calling a tool returns to the agent.
fn call(params: &Value, spool: Option<&Path>) -> Value {
    let known = params.get("name").and_then(Value::as_str) == Some(SHOW_HTML);
    let delivered = match spool.filter(|_| known) {
        Some(spool) => deliver(spool, params).is_ok(),
        None => true,
    };
    let text = match (known, delivered) {
        (false, _) => "Swarmed has no such tool.",
        (true, true) => {
            "Shown to the user in the conversation. Do not repeat the HTML in your reply."
        }
        (true, false) => "The view could not be handed to Swarmed, so the user does not see it.",
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": !(known && delivered) })
}

/// The answer to one request, or `None` for a notification, which gets none.
fn answer(message: &Value, spool: Option<&Path>) -> Option<Value> {
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
        "tools/call" => call(params, spool),
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
    let spool = std::env::var_os(VIEW_SPOOL_ENV).map(PathBuf::from);
    serve(input, output, spool.as_deref())
}

fn serve(input: impl BufRead, mut output: impl Write, spool: Option<&Path>) -> std::io::Result<()> {
    for line in input.lines() {
        let message = serde_json::from_str::<Value>(&line?).ok();
        if let Some(reply) = message.as_ref().and_then(|message| answer(message, spool)) {
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
        serve(input.as_bytes(), &mut output, None).expect("serves");
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
        serve(input.as_bytes(), &mut output, None).expect("serves");
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
        serve(input.as_bytes(), &mut output, Some(&spool)).expect("serves");
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
