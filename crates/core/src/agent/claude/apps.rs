//! MCP Apps as Claude Code offers them to the program that runs it: which tools have an app
//! (`mcp_status`), the app of a call (`mcp_read_resource`), and a tool called for an app
//! (`mcp_call`).
//!
//! Only a Claude Code that says it can, in the capabilities of its init message, is asked:
//! [`READS`] for showing apps, [`CALLS`] for calling a tool for one. Of `mcp_status` only
//! each server's name and its tools' names and `_meta.ui` are read; the servers' definitions,
//! which hold their secrets, are not. An app is read once its call has a result that is not
//! an error, and reported as [`AgentEvent::App`] with that result, which an app is handed.
//! A tool called for an app is decided by Claude Code as the agent's own call of it would
//! be, and its result is kept out of the model's context (the user's choice).
//!
//! Tried with Claude Code 2.1.332.

use std::collections::{HashMap, HashSet, VecDeque};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value, json};

use super::{content_blocks, kind_of, str_of, strings};
use crate::agent::AgentEvent;
use crate::apps::{AppCsp, MAX_APP_BYTES, MAX_APP_MB, MAX_RESULT_BYTES};

/// The id of the request that asks which tools have an app.
const TOOLS_REQUEST: &str = "swarmed-apps";
/// What the id of a request that reads a call's app starts with; the call's id follows.
const READ_REQUEST: &str = "swarmed-app-read:";
/// What the id of a request that calls a tool for an app starts with; the call's number
/// follows.
const CALL_REQUEST: &str = "swarmed-app-call:";

/// What Claude Code says when it can say which tools have an app and read one.
const READS: [&str; 2] = ["mcp_read_resource_v1", "mcp_tool_ui_meta_v1"];

/// What Claude Code says when it can call a tool for an app as it would for the agent, asking
/// first where it would ask, and keep the result out of the model's context. One that does
/// not say so would run the tool without a check, so it is never asked.
const CALLS: [&str; 2] = [
    "mcp_call_check_permissions_v1",
    "mcp_call_result_not_for_model_v1",
];

/// What an app's document is, as a server gives it.
const APP_TYPE: &str = "text/html;profile=mcp-app";

/// How many of the agent's MCP calls are remembered until their result: a turn has a few
/// waiting at most.
const CALLS_KEPT: usize = 256;

/// An MCP tool as Claude Code reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tool {
    /// The server, as Claude Code names it.
    server: String,
    /// The tool, as its server names it.
    name: String,
    /// Its app, if it has one.
    uri: Option<String>,
    /// Who may call it (`model`, `app`), if its server said; anyone if not.
    visibility: Option<Vec<String>>,
}

impl Tool {
    /// Whether an app of its own server may call it.
    fn for_apps(&self) -> bool {
        self.visibility
            .as_ref()
            .is_none_or(|who| who.iter().any(|one| one == "app"))
    }
}

/// An MCP call of the agent's, until its result: the tool by the name the agent calls it, and
/// the sub-agent that made it, if one did.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Call {
    tool: String,
    parent: Option<String>,
}

/// A call whose app is to be read, with the result it is to be handed.
#[derive(Debug, Clone)]
struct Pending {
    call: Call,
    result: Option<Value>,
}

/// Whether the tools have been asked for and not reported yet.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Asking {
    /// Not asked for, or reported.
    #[default]
    No,
    /// Asked for once.
    Once,
    /// Asked for, and to be asked for again once reported: the servers changed meanwhile.
    Again,
}

/// What this Claude Code says it can do for apps.
#[derive(Debug, Default, Clone, Copy)]
struct Able {
    /// Say which tools have an app and read one.
    reads: bool,
    /// Call a tool for an app with its own check.
    calls: bool,
}

/// What is known of the MCP apps of one run of Claude Code.
#[derive(Debug, Default)]
pub(super) struct Apps {
    /// What it can do.
    able: Able,
    /// The MCP tools it last reported, by the name the agent calls each by.
    tools: HashMap<String, Tool>,
    /// The servers and their states when the tools were last asked for: asked again when
    /// they change, as a server that connects later brings tools of its own.
    servers_asked: Option<Vec<(String, String)>>,
    /// Whether the tools have been asked for and not reported yet.
    asking: Asking,
    /// The tools asked about since the agent called them unknown, so that one that stays
    /// unknown is not asked about at every call.
    unknown_asked: HashSet<String>,
    /// The agent's MCP calls that have no result yet, by id, oldest first.
    calls_made: VecDeque<(String, Call)>,
    /// Results of calls of tools not known yet, until the tools are reported.
    waiting: HashMap<String, Pending>,
    /// Calls whose app is being read, with the tool it is read for.
    reading: HashMap<String, (Tool, Pending)>,
    /// Tools called for apps and not answered yet, by the number of the call.
    in_flight: HashMap<u64, String>,
}

/// What follows from one message: requests to send, and events to report.
pub(super) type Step = (Vec<Value>, Vec<AgentEvent>);

impl Apps {
    /// Takes in one message of Claude Code's.
    pub(super) fn after(&mut self, message: &Value) -> Step {
        let replay = message.get("isReplay").and_then(Value::as_bool) == Some(true);
        match (kind_of(message, "type"), kind_of(message, "subtype")) {
            (Some("system"), Some("init")) => (self.on_init(message), vec![]),
            (Some("assistant"), _) => (self.on_calls(message), vec![]),
            (Some("user"), _) if !replay => (self.on_results(message), vec![]),
            (Some("control_response"), _) => self.on_answer(message),
            _ => (vec![], vec![]),
        }
    }

    /// What it can do, and the tools asked for if the servers changed since they last were.
    fn on_init(&mut self, message: &Value) -> Vec<Value> {
        let said = strings(message.get("capabilities"));
        let says = |all: &[&str]| all.iter().all(|one| said.iter().any(|it| it == one));
        self.able = Able {
            reads: says(&READS),
            calls: says(&CALLS),
        };
        if !self.able.reads {
            return vec![];
        }
        let listed = message.get("mcp_servers").and_then(Value::as_array);
        let mut servers: Vec<(String, String)> = listed
            .into_iter()
            .flatten()
            .map(|server| (str_of(server, "name"), str_of(server, "status")))
            .collect();
        servers.sort();
        if self.servers_asked.as_ref() == Some(&servers) {
            return vec![];
        }
        self.servers_asked = Some(servers);
        self.ask_tools()
    }

    /// The request for the tools, unless one is out already, in which case they are asked
    /// for again once it is answered.
    fn ask_tools(&mut self) -> Vec<Value> {
        if self.asking != Asking::No {
            self.asking = Asking::Again;
            return vec![];
        }
        self.asking = Asking::Once;
        vec![json!({
            "type": "control_request",
            "request_id": TOOLS_REQUEST,
            "request": { "subtype": "mcp_status" },
        })]
    }

    /// Remembers the agent's MCP calls, and asks for the tools when one is of a tool not
    /// known yet.
    fn on_calls(&mut self, message: &Value) -> Vec<Value> {
        let parent = kind_of(message, "parent_tool_use_id").map(String::from);
        content_blocks(message)
            .filter(|block| kind_of(block, "type") == Some("tool_use"))
            .map(|block| (str_of(block, "id"), str_of(block, "name")))
            .filter(|(_, name)| name.starts_with("mcp__"))
            .flat_map(|(id, tool)| self.remember(id, tool, parent.clone()))
            .collect()
    }

    /// Remembers call `id` of `tool` until its result, and asks for the tools if it is not
    /// known yet and has not been asked about since it was called.
    fn remember(&mut self, id: String, tool: String, parent: Option<String>) -> Vec<Value> {
        let unknown = !self.tools.contains_key(&tool);
        let ask = self.able.reads && unknown && self.unknown_asked.insert(tool.clone());
        self.calls_made.push_back((id, Call { tool, parent }));
        if self.calls_made.len() > CALLS_KEPT {
            self.calls_made.pop_front();
        }
        if ask { self.ask_tools() } else { vec![] }
    }

    /// The call of id `id`, which has its result: it is forgotten.
    fn take_call(&mut self, id: &str) -> Option<Call> {
        let at = self.calls_made.iter().position(|(made, _)| made == id)?;
        self.calls_made.remove(at).map(|(_, call)| call)
    }

    /// Reads the app of each call that has a result and a tool with an app, or waits for
    /// the tools to be known first.
    fn on_results(&mut self, message: &Value) -> Vec<Value> {
        let results: Vec<&Value> = content_blocks(message)
            .filter(|block| kind_of(block, "type") == Some("tool_result"))
            .collect();
        // The message's whole result is its one call's: of several, nobody can tell whose.
        let full = (results.len() == 1)
            .then(|| message.get("tool_use_result"))
            .flatten();
        results
            .into_iter()
            .filter_map(|block| self.on_result(block, full))
            .collect()
    }

    /// The request that reads the app of the call `block` is the result of, if it has one.
    /// A call that failed or was refused shows as it is, with no app.
    fn on_result(&mut self, block: &Value, full: Option<&Value>) -> Option<Value> {
        let id = str_of(block, "tool_use_id");
        let call = self.take_call(&id)?;
        if block.get("is_error").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        let pending = Pending {
            call,
            result: result_of(block, full),
        };
        let Some(tool) = self.tools.get(&pending.call.tool).cloned() else {
            self.wait_for_tools(id, pending);
            return None;
        };
        self.read(id, tool, pending)
    }

    /// Keeps a result of a tool not known yet until the tools are reported, if they have been
    /// asked for.
    fn wait_for_tools(&mut self, id: String, pending: Pending) {
        if self.asking != Asking::No {
            self.waiting.insert(id, pending);
        }
    }

    /// The request that reads the app of call `id`, if its tool is known now and has one.
    fn read_known(&mut self, id: String, pending: Pending) -> Option<Value> {
        let tool = self.tools.get(&pending.call.tool).cloned()?;
        self.read(id, tool, pending)
    }

    /// The request that reads the app of call `id`, if its tool has one.
    fn read(&mut self, id: String, tool: Tool, pending: Pending) -> Option<Value> {
        let uri = tool.uri.clone()?;
        let request = json!({
            "type": "control_request",
            "request_id": format!("{READ_REQUEST}{id}"),
            "request": { "subtype": "mcp_read_resource", "serverName": tool.server, "uri": uri },
        });
        self.reading.insert(id, (tool, pending));
        Some(request)
    }

    /// The answers to Swarmed's own requests about apps: the tools, an app, a tool's result.
    fn on_answer(&mut self, message: &Value) -> Step {
        let Some(response) = message.get("response") else {
            return (vec![], vec![]);
        };
        let id = kind_of(response, "request_id").unwrap_or_default();
        let body = (kind_of(response, "subtype") == Some("success"))
            .then(|| response.get("response"))
            .flatten();
        if id == TOOLS_REQUEST {
            return (self.on_tools(body), vec![]);
        }
        if let Some(call) = id.strip_prefix(READ_REQUEST) {
            let read = self.reading.remove(call);
            let app = read.and_then(|(tool, pending)| self.app(call, &tool, pending, body?));
            return (vec![], app.into_iter().collect());
        }
        if let Some(call) = id.strip_prefix(CALL_REQUEST).and_then(|n| n.parse().ok()) {
            self.in_flight.remove(&call);
            let said = kind_of(response, "error").unwrap_or("Claude Code gave no reason");
            let called = AgentEvent::AppCalled {
                call,
                result: body.cloned(),
                error: body.is_none().then(|| said.to_string()),
            };
            return (vec![], vec![called]);
        }
        (vec![], vec![])
    }

    /// Takes in the tools Claude Code reported, and reads the apps of the calls that waited
    /// for them.
    fn on_tools(&mut self, body: Option<&Value>) -> Vec<Value> {
        let again = self.asking == Asking::Again;
        self.asking = Asking::No;
        if let Some(body) = body {
            self.tools = tools_in(body);
        }
        let waiting = std::mem::take(&mut self.waiting);
        let mut asks: Vec<Value> = waiting
            .into_iter()
            .filter_map(|(id, pending)| self.read_known(id, pending))
            .collect();
        if again {
            asks.extend(self.ask_tools());
        }
        asks
    }

    /// The app of call `id` from the answer `body` to reading it, if it holds one.
    fn app(&self, id: &str, tool: &Tool, pending: Pending, body: &Value) -> Option<AgentEvent> {
        let items = body.get("contents").and_then(Value::as_array)?;
        let item = items
            .iter()
            .find(|item| is_app(kind_of(item, "mimeType").unwrap_or_default()))?;
        let meta = item.pointer("/_meta/ui").unwrap_or(&Value::Null);
        let (html, note) = match html_of(item) {
            Ok(html) => (Some(html), None),
            Err(why) => (None, Some(why)),
        };
        let mut callable: Vec<String> = self
            .tools
            .values()
            .filter(|other| other.server == tool.server && other.for_apps())
            .map(|other| other.name.clone())
            .collect();
        callable.sort();
        let permissions = meta.get("permissions").and_then(Value::as_object);
        let mut permissions: Vec<String> = permissions
            .into_iter()
            .flatten()
            .map(|(k, _)| k.clone())
            .collect();
        permissions.sort();
        let app = AgentEvent::App {
            tool_use_id: id.to_string(),
            server: tool.server.clone(),
            tool: tool.name.clone(),
            uri: str_of(item, "uri"),
            document: None,
            note,
            csp: AppCsp::declared(meta.get("csp").unwrap_or(&Value::Null)),
            permissions,
            prefers_border: meta.get("prefersBorder").and_then(Value::as_bool),
            callable,
            result: pending.result,
            html,
        };
        Some(match pending.call.parent {
            Some(parent) => AgentEvent::Sub {
                parent,
                event: Box::new(app),
            },
            None => app,
        })
    }

    /// The request that calls tool `tool` of server `server` for an app, or `None` where
    /// Claude Code cannot do it with its own check.
    pub(super) fn call(
        &mut self,
        call: u64,
        (server, tool): (&str, &str),
        arguments: &Value,
    ) -> Option<Value> {
        if !self.able.calls {
            return None;
        }
        let name = full_name(server, tool);
        self.in_flight.insert(call, name.clone());
        Some(json!({
            "type": "control_request",
            "request_id": format!("{CALL_REQUEST}{call}"),
            "request": {
                "subtype": "mcp_call",
                "tool": name,
                "arguments": arguments,
                // Decided as the agent's own call is, and handed back for the app alone.
                "check_permissions": true,
                "result_not_for_model": true,
                // The tool's own failure comes back as its result, which an app can show.
                "soft_error_result": true,
            },
        }))
    }

    /// Whether a permission request for tool `tool` of call `id` is for a call an app asked
    /// for: not a call the agent made, of a tool an app called and has no answer for yet.
    pub(super) fn asked_by_app(&self, id: &str, tool: &str) -> bool {
        let agents = self.calls_made.iter().any(|(made, _)| made == id);
        !agents && self.in_flight.values().any(|name| name == tool)
    }
}

/// A server's or a tool's name as it stands in the name the agent calls a tool by: what is
/// not a letter, a digit, `_` or `-` becomes `_`.
fn normalized(name: &str) -> String {
    let kept = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    name.chars()
        .map(|c| if kept(c) { c } else { '_' })
        .collect()
}

/// The name the agent calls tool `tool` of server `server` by.
fn full_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", normalized(server), normalized(tool))
}

/// The tools of an answer to `mcp_status`, by the name the agent calls each by. Only names
/// and `_meta.ui` are read.
fn tools_in(body: &Value) -> HashMap<String, Tool> {
    let servers = body.get("mcpServers").and_then(Value::as_array);
    servers.into_iter().flatten().flat_map(tools_of).collect()
}

/// The tools of one server of an answer to `mcp_status`.
fn tools_of(server: &Value) -> Vec<(String, Tool)> {
    let name = str_of(server, "name");
    let listed = server.get("tools").and_then(Value::as_array);
    let tools = listed.into_iter().flatten();
    tools.filter_map(|tool| tool_of(&name, tool)).collect()
}

/// One tool of server `server`, by the name the agent calls it by, with its app if it has one
/// and who may call it, if its server said.
fn tool_of(server: &str, tool: &Value) -> Option<(String, Tool)> {
    let name = str_of(tool, "name");
    if name.is_empty() || server.is_empty() {
        return None;
    }
    let ui = tool.pointer("/_meta/ui");
    // The older, flat key, which servers written before `ui` still use.
    let flat = tool
        .get("_meta")
        .and_then(|meta| kind_of(meta, "ui/resourceUri"));
    let uri = ui
        .and_then(|ui| kind_of(ui, "resourceUri"))
        .or(flat)
        .filter(|uri| uri.starts_with("ui://"))
        .map(String::from);
    let visibility = ui
        .and_then(|ui| ui.get("visibility"))
        .filter(|who| who.is_array())
        .map(|who| strings(Some(who)));
    let tool = Tool {
        server: server.to_string(),
        name,
        uri,
        visibility,
    };
    Some((full_name(server, &tool.name), tool))
}

/// Whether a resource of type `mime` is an app: HTML marked as one, or plain HTML.
fn is_app(mime: &str) -> bool {
    let bare: String = mime.chars().filter(|c| !c.is_whitespace()).collect();
    let bare = bare.to_ascii_lowercase();
    bare == APP_TYPE || bare == "text/html" || bare.starts_with("text/html;")
}

/// The HTML of a resource item, given as text or in base64, or why it is not kept.
fn html_of(item: &Value) -> Result<String, String> {
    let larger = || format!("The app is larger than {MAX_APP_MB} MB, so it was not kept.");
    if let Some(text) = kind_of(item, "text") {
        if text.len() > MAX_APP_BYTES {
            return Err(larger());
        }
        return Ok(text.to_string());
    }
    let blob = kind_of(item, "blob").unwrap_or_default().trim();
    // Base64 is a third longer than what it stands for: one far too long is not decoded.
    if blob.len() / 4 * 3 > MAX_APP_BYTES + 3 {
        return Err(larger());
    }
    let unread = || "The app could not be read.".to_string();
    let bytes = STANDARD.decode(blob).map_err(|_| unread())?;
    if bytes.len() > MAX_APP_BYTES {
        return Err(larger());
    }
    String::from_utf8(bytes).map_err(|_| unread())
}

/// A call's result as an app is handed it, MCP's `CallToolResult`: its text from the block
/// the model was given, and, where the message's whole result `full` says them, its
/// `structuredContent` and `_meta`. Left out where it is larger than an app is handed.
fn result_of(block: &Value, full: Option<&Value>) -> Option<Value> {
    let text = match block.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| kind_of(part, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    let mut result = Map::new();
    result.insert(
        "content".to_string(),
        json!([{ "type": "text", "text": text }]),
    );
    for key in ["structuredContent", "_meta"] {
        if let Some(value) = full.and_then(|full| full.get(key)).filter(|v| !v.is_null()) {
            result.insert(key.to_string(), value.clone());
        }
    }
    let size =
        |result: &Map<String, Value>| serde_json::to_string(result).map_or(usize::MAX, |s| s.len());
    if size(&result) > MAX_RESULT_BYTES {
        result.remove("structuredContent");
        result.remove("_meta");
    }
    (size(&result) <= MAX_RESULT_BYTES).then_some(Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claude Code's init message, as 2.1.332 sent it, cut to what is read here.
    fn init(capabilities: &[&str], servers: &Value) -> Value {
        json!({
            "type": "system", "subtype": "init", "session_id": "s1",
            "capabilities": capabilities, "mcp_servers": servers,
        })
    }

    const ABLE: [&str; 4] = [
        "mcp_read_resource_v1",
        "mcp_tool_ui_meta_v1",
        "mcp_call_check_permissions_v1",
        "mcp_call_result_not_for_model_v1",
    ];

    /// Claude Code's answer to `mcp_status`, as 2.1.332 sent it for a server with an app.
    fn status() -> Value {
        json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": TOOLS_REQUEST, "response": {
                "mcpServers": [{
                    "name": "demo", "status": "connected",
                    "serverInfo": { "name": "demo", "version": "1" },
                    "config": { "type": "stdio", "command": "python3", "env": { "TOKEN": "secret" } },
                    "tools": [
                        { "name": "chart", "annotations": { "readOnly": true },
                          "_meta": { "ui": { "resourceUri": "ui://demo/chart",
                                             "visibility": ["model", "app"] } } },
                        { "name": "refresh", "annotations": {},
                          "_meta": { "ui": { "resourceUri": "ui://demo/chart",
                                             "visibility": ["app"] } } },
                        { "name": "secret_admin",
                          "_meta": { "ui": { "visibility": ["model"] } } },
                        { "name": "plain" }
                    ]
                }, {
                    "name": "claude.ai Maps", "status": "connected",
                    "tools": [{ "name": "show map", "_meta": { "ui/resourceUri": "ui://maps/map" } }]
                }]
            } }
        })
    }

    fn call(id: &str, name: &str) -> Value {
        json!({
            "type": "assistant", "parent_tool_use_id": null,
            "message": { "content": [{ "type": "tool_use", "id": id, "name": name, "input": { "n": 3 } }] }
        })
    }

    /// A tool's result, as 2.1.332 sent it: the text the model was given, and the whole
    /// result beside it.
    fn result(id: &str, is_error: bool) -> Value {
        json!({
            "type": "user", "parent_tool_use_id": null,
            "message": { "role": "user", "content": [{
                "tool_use_id": id, "type": "tool_result", "content": "{\"bars\":[0,1,2]}",
                "is_error": is_error,
            }] },
            "tool_use_result": {
                "content": "{\"bars\":[0,1,2]}", "_meta": { "note": "hi" },
                "structuredContent": { "bars": [0, 1, 2] }
            }
        })
    }

    /// Claude Code's answer to `mcp_read_resource`, as 2.1.332 sent it.
    fn resource(id: &str, item: &Value) -> Value {
        json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": format!("{READ_REQUEST}{id}"),
                          "response": { "contents": [item] } }
        })
    }

    fn chart_item() -> Value {
        json!({
            "uri": "ui://demo/chart", "mimeType": "text/html;profile=mcp-app",
            "text": "<!doctype html><h1>Chart</h1>",
            "_meta": { "ui": {
                "csp": { "connectDomains": ["https://api.example.com"],
                         "resourceDomains": ["https://cdn.example.com", "http://bad.example.com"] },
                "permissions": { "camera": {}, "clipboardWrite": {} },
                "prefersBorder": true
            } }
        })
    }

    fn subtypes(asks: &[Value]) -> Vec<String> {
        asks.iter()
            .map(|ask| {
                ask.pointer("/request/subtype")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    }

    /// An `Apps` that knows the demo server's tools.
    fn known() -> Apps {
        let mut apps = Apps::default();
        let (asks, _) = apps.after(&init(
            &ABLE,
            &json!([{ "name": "demo", "status": "connected" }]),
        ));
        assert_eq!(subtypes(&asks), ["mcp_status"]);
        let (asks, events) = apps.after(&status());
        assert!(asks.is_empty() && events.is_empty(), "nothing is asked");
        apps
    }

    #[test]
    fn an_older_claude_code_is_asked_nothing() {
        let mut apps = Apps::default();
        let (asks, _) = apps.after(&init(
            &["interrupt_receipt_v1"],
            &json!([{ "name": "demo" }]),
        ));
        assert!(asks.is_empty(), "nothing is asked");
        let (asks, _) = apps.after(&call("toolu_1", "mcp__demo__chart"));
        assert!(asks.is_empty(), "no tools are asked for");
        let (asks, _) = apps.after(&result("toolu_1", false));
        assert!(asks.is_empty(), "no app is read");
        assert!(apps.call(1, ("demo", "chart"), &json!({})).is_none());
    }

    #[test]
    fn the_tools_are_asked_for_again_only_when_the_servers_change() {
        let mut apps = known();
        let same = init(&ABLE, &json!([{ "name": "demo", "status": "connected" }]));
        assert!(apps.after(&same).0.is_empty(), "nothing is asked");
        let more = init(
            &ABLE,
            &json!([{ "name": "demo", "status": "connected" },
                                       { "name": "late", "status": "connected" }]),
        );
        assert_eq!(subtypes(&apps.after(&more).0), ["mcp_status"]);
        // Asked again while asked: once more after the answer, not twice at once.
        let changed = init(&ABLE, &json!([{ "name": "late", "status": "failed" }]));
        assert!(apps.after(&changed).0.is_empty(), "nothing is asked");
        assert_eq!(subtypes(&apps.after(&status()).0), ["mcp_status"]);
    }

    #[test]
    fn only_names_and_the_apps_of_tools_are_read() {
        let tools = tools_in(&status()["response"]["response"]);
        let chart = &tools["mcp__demo__chart"];
        assert_eq!(chart.uri.as_deref(), Some("ui://demo/chart"));
        assert!(chart.for_apps());
        assert!(tools["mcp__demo__refresh"].for_apps());
        assert!(!tools["mcp__demo__secret_admin"].for_apps());
        assert!(
            tools["mcp__demo__plain"].for_apps(),
            "nothing said is anyone"
        );
        // The older flat key, and a server and a tool named with spaces and dots.
        let map = &tools["mcp__claude_ai_Maps__show_map"];
        assert_eq!(map.uri.as_deref(), Some("ui://maps/map"));
        assert_eq!(
            (map.server.as_str(), map.name.as_str()),
            ("claude.ai Maps", "show map")
        );
        // The server's definition, with its secrets, is not read.
        assert!(!format!("{tools:?}").contains("TOKEN"));
    }

    #[test]
    fn a_calls_app_is_read_once_it_has_a_result_and_reported_with_it() {
        let mut apps = known();
        assert!(
            apps.after(&call("toolu_1", "mcp__demo__chart"))
                .0
                .is_empty(),
            "nothing is asked"
        );
        let (asks, _) = apps.after(&result("toolu_1", false));
        assert_eq!(
            asks,
            [json!({
                "type": "control_request", "request_id": "swarmed-app-read:toolu_1",
                "request": { "subtype": "mcp_read_resource", "serverName": "demo",
                             "uri": "ui://demo/chart" },
            })]
        );
        let (_, events) = apps.after(&resource("toolu_1", &chart_item()));
        let [
            AgentEvent::App {
                tool_use_id,
                server,
                tool,
                uri,
                document,
                note,
                csp,
                permissions,
                prefers_border,
                callable,
                result,
                html,
            },
        ] = events.as_slice()
        else {
            panic!("not one app: {events:?}");
        };
        assert_eq!(
            (tool_use_id.as_str(), server.as_str(), tool.as_str()),
            ("toolu_1", "demo", "chart")
        );
        assert_eq!(uri, "ui://demo/chart");
        assert_eq!(html.as_deref(), Some("<!doctype html><h1>Chart</h1>"));
        assert_eq!((document, note), (&None, &None));
        assert_eq!(csp.connect, ["https://api.example.com"]);
        assert_eq!(
            csp.resources,
            ["https://cdn.example.com", "http://bad.example.com"]
        );
        assert_eq!(permissions, &["camera", "clipboardWrite"]);
        assert_eq!(*prefers_border, Some(true));
        assert_eq!(callable, &["chart", "plain", "refresh"]);
        assert_eq!(
            result.as_ref(),
            Some(&json!({
                "content": [{ "type": "text", "text": "{\"bars\":[0,1,2]}" }],
                "structuredContent": { "bars": [0, 1, 2] }, "_meta": { "note": "hi" },
            }))
        );
    }

    #[test]
    fn a_failed_call_a_tool_without_an_app_and_a_read_that_fails_show_no_app() {
        let mut apps = known();
        apps.after(&call("toolu_1", "mcp__demo__chart"));
        assert!(apps.after(&result("toolu_1", true)).0.is_empty(), "failed");
        apps.after(&call("toolu_2", "mcp__demo__plain"));
        assert!(apps.after(&result("toolu_2", false)).0.is_empty(), "no app");
        apps.after(&call("toolu_3", "Bash"));
        assert!(
            apps.after(&result("toolu_3", false)).0.is_empty(),
            "not MCP"
        );
        apps.after(&call("toolu_4", "mcp__demo__chart"));
        assert_eq!(apps.after(&result("toolu_4", false)).0.len(), 1);
        let refused = json!({ "type": "control_response", "response": {
            "subtype": "error", "request_id": "swarmed-app-read:toolu_4",
            "error": "mcp_read_resource: server demo is not connected" } });
        assert!(apps.after(&refused).1.is_empty(), "nothing is reported");
        // A resource that is not HTML is no app.
        apps.after(&call("toolu_5", "mcp__demo__chart"));
        apps.after(&result("toolu_5", false));
        let image = json!({ "uri": "ui://demo/chart", "mimeType": "image/png", "blob": "AAAA" });
        assert!(
            apps.after(&resource("toolu_5", &image)).1.is_empty(),
            "no app"
        );
    }

    #[test]
    fn a_call_of_a_tool_not_known_yet_waits_for_the_tools() {
        let mut apps = Apps::default();
        apps.after(&init(&ABLE, &json!([])));
        apps.after(&json!({ "type": "control_response", "response": {
            "subtype": "success", "request_id": TOOLS_REQUEST, "response": { "mcpServers": [] } } }));
        // A server that connected since: its tool is asked about once.
        assert_eq!(
            subtypes(&apps.after(&call("toolu_1", "mcp__demo__chart")).0),
            ["mcp_status"]
        );
        assert!(
            apps.after(&call("toolu_2", "mcp__demo__chart"))
                .0
                .is_empty(),
            "nothing is asked"
        );
        assert!(apps.after(&result("toolu_1", false)).0.is_empty(), "waits");
        let (asks, _) = apps.after(&status());
        assert_eq!(subtypes(&asks), ["mcp_read_resource"]);
        // One that stays unknown is not asked about again.
        apps.after(&call("toolu_3", "mcp__gone__tool"));
        assert!(
            apps.after(&call("toolu_4", "mcp__gone__tool")).0.is_empty(),
            "nothing is asked"
        );
    }

    #[test]
    fn a_sub_agents_app_is_its_own_and_an_app_too_large_is_named() {
        let mut apps = known();
        let mut by_sub = call("toolu_1", "mcp__demo__chart");
        by_sub["parent_tool_use_id"] = json!("agent_1");
        apps.after(&by_sub);
        apps.after(&result("toolu_1", false));
        let large = json!({ "uri": "ui://demo/chart", "mimeType": "text/html;profile=mcp-app",
                            "text": "x".repeat(MAX_APP_BYTES + 1) });
        let (_, events) = apps.after(&resource("toolu_1", &large));
        let [AgentEvent::Sub { parent, event }] = events.as_slice() else {
            panic!("not a sub-agent's: {events:?}");
        };
        assert_eq!(parent, "agent_1");
        let AgentEvent::App { html, note, .. } = event.as_ref() else {
            panic!("not an app");
        };
        assert!(html.is_none());
        assert_eq!(
            note.as_deref(),
            Some("The app is larger than 4 MB, so it was not kept.")
        );
    }

    #[test]
    fn an_app_given_in_base64_is_read() {
        let item = json!({ "mimeType": "text/html", "blob": STANDARD.encode("<p>hi</p>") });
        assert_eq!(html_of(&item), Ok("<p>hi</p>".to_string()));
        let broken = json!({ "blob": "not base64!" });
        assert_eq!(
            html_of(&broken),
            Err("The app could not be read.".to_string())
        );
        assert!(is_app("text/html; profile=mcp-app"));
        assert!(!is_app("text/plain"));
    }

    #[test]
    fn a_result_too_large_for_an_app_keeps_only_its_text() {
        let block =
            json!({ "content": [{ "type": "text", "text": "short" }, { "type": "image" }] });
        let full = json!({ "structuredContent": { "rows": "x".repeat(MAX_RESULT_BYTES) } });
        assert_eq!(
            result_of(&block, Some(&full)),
            Some(json!({ "content": [{ "type": "text", "text": "short" }] }))
        );
        let huge = json!({ "content": "y".repeat(MAX_RESULT_BYTES + 1) });
        assert_eq!(result_of(&huge, None), None);
    }

    #[test]
    fn a_tool_called_for_an_app_is_checked_as_the_agents_own_and_its_answer_reported() {
        let mut apps = known();
        let asked = apps.call(7, ("claude.ai Maps", "show map"), &json!({ "q": "x" }));
        assert_eq!(
            asked,
            Some(json!({
                "type": "control_request", "request_id": "swarmed-app-call:7",
                "request": { "subtype": "mcp_call", "tool": "mcp__claude_ai_Maps__show_map",
                             "arguments": { "q": "x" }, "check_permissions": true,
                             "result_not_for_model": true, "soft_error_result": true },
            }))
        );
        // Its permission request is the app's, not the agent's.
        assert!(apps.asked_by_app("toolu_mcp_call_1", "mcp__claude_ai_Maps__show_map"));
        apps.after(&call("toolu_9", "mcp__claude_ai_Maps__show_map"));
        assert!(!apps.asked_by_app("toolu_9", "mcp__claude_ai_Maps__show_map"));
        let answered = json!({ "type": "control_response", "response": {
            "subtype": "success", "request_id": "swarmed-app-call:7",
            "response": { "content": [{ "type": "text", "text": "Drew 2 bars." }],
                          "structuredContent": { "bars": [0, 1] } } } });
        let (_, events) = apps.after(&answered);
        assert_eq!(
            events,
            [AgentEvent::AppCalled {
                call: 7,
                result: Some(
                    json!({ "content": [{ "type": "text", "text": "Drew 2 bars." }],
                                     "structuredContent": { "bars": [0, 1] } })
                ),
                error: None,
            }]
        );
        assert!(!apps.asked_by_app("toolu_mcp_call_2", "mcp__claude_ai_Maps__show_map"));
        apps.call(8, ("demo", "chart"), &json!({}));
        let refused = json!({ "type": "control_response", "response": {
            "subtype": "error", "request_id": "swarmed-app-call:8",
            "error": "The tool did not run: the user said no" } });
        assert_eq!(
            apps.after(&refused).1,
            [AgentEvent::AppCalled {
                call: 8,
                result: None,
                error: Some("The tool did not run: the user said no".to_string()),
            }]
        );
    }
}
