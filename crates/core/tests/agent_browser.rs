//! Agents' own browser tools on the task's Chrome (#384), against the stand-in Chrome
//! (`fixtures/fake_chrome.py`): each session's address starts the browser on the first
//! connection and leads back to itself, the pane follows the tab a session's tools act in and
//! names the session, `show_browser` opens a tab of the session's own, and the user's browser
//! servers are handed to the agent pointed at the address.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use common::{core_with_project, fixture, turn};
use serde_json::{Value, json};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    BrowserEvent, BrowserState, Core, NewTaskOptions, SessionOptions, Settings, Viewport, hooks,
    serve_session_tools,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UnixStream};
use tokio::sync::broadcast::Receiver;

const VIEW: Viewport = Viewport {
    width: 640,
    height: 480,
    scale: 1.0,
};

/// A core with task `one`, whose browser runs on the stand-in, serving agents' tools.
async fn serving(fx: &common::Fixture) -> Arc<Core> {
    let core = core_with_project(fx).await;
    let root = fx.paths.data_dir.parent().expect("has a parent");
    let program = root.join("fake-chrome");
    common::executable(&program, include_str!("fixtures/fake_chrome.py"));
    let settings = Settings {
        browser_program: program.to_string_lossy().into_owned(),
        ..Settings::default()
    };
    core.set_settings(&settings).expect("test setup");
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let core = Arc::new(core);
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    for _ in 0..1000 {
        if socket.exists() {
            return core;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("nothing listens on {}", socket.display());
}

fn task_id(core: &Core) -> i64 {
    core.task("demo", "one").expect("there").1.id
}

fn started(core: &Core) -> i64 {
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    session.id
}

/// What the agent of `session` answers `text` with.
async fn said(core: &Core, session: i64, text: &str) -> String {
    let mut bus = core.subscribe();
    core.session_send(session, text).expect("sends");
    let events = turn(&mut bus, session).await;
    let words = events.into_iter().filter_map(|event| match event {
        AgentEvent::Text { text } => Some(text),
        _ => None,
    });
    words.collect()
}

/// The port of `http://127.0.0.1:<port>`.
fn port_of(address: &str) -> u16 {
    let port = address
        .strip_prefix("http://127.0.0.1:")
        .expect("an address");
    port.parse().expect("a port")
}

/// A plain request to the address, as a tool makes it, and the whole answer.
async fn request(port: u16, headers: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connects");
    let head = format!(
        "GET /json/version HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{headers}Connection: close\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).await.expect("writes");
    let mut answer = String::new();
    let read = stream.read_to_string(&mut answer);
    tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .expect("answered in time")
        .expect("reads");
    answer
}

/// A browser tool's WebSocket to the address, as Playwright's or Puppeteer's is.
struct Tool {
    stream: TcpStream,
}

impl Tool {
    async fn open(port: u16, path: &str) -> Self {
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connects");
        let head = format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\nSec-WebSocket-Extensions: permessage-deflate\r\n\r\n"
        );
        stream.write_all(head.as_bytes()).await.expect("writes");
        let mut answer = Vec::new();
        while !answer.ends_with(b"\r\n\r\n") {
            answer.push(stream.read_u8().await.expect("reads"));
        }
        let answer = String::from_utf8_lossy(&answer).into_owned();
        assert!(answer.contains(" 101 "), "{answer}");
        assert!(!answer.to_lowercase().contains("deflate"), "{answer}");
        Self { stream }
    }

    /// Sends a command, masked as a client's are, and waits for its answer.
    async fn call(&mut self, command: Value) -> Value {
        let id = command["id"].clone();
        let payload = command.to_string().into_bytes();
        let mask = [3u8, 1, 4, 1];
        let mut frame = vec![0x81];
        let length = u16::try_from(payload.len()).expect("short");
        if length < 126 {
            frame.push(0x80 | u8::try_from(length).expect("short"));
        } else {
            frame.push(0x80 | 0x7E);
            frame.extend(length.to_be_bytes());
        }
        frame.extend(mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(at, byte)| byte ^ mask[at % 4]),
        );
        self.stream.write_all(&frame).await.expect("writes");
        let mut answer = self.message().await;
        while answer["id"] != id {
            answer = self.message().await;
        }
        answer
    }

    async fn message(&mut self) -> Value {
        let head = [self.read_u8().await, self.read_u8().await];
        let length = match head[1] & 0x7F {
            126 => usize::from(self.stream.read_u16().await.expect("reads")),
            127 => usize::try_from(self.stream.read_u64().await.expect("reads")).expect("fits"),
            short => usize::from(short),
        };
        let mut payload = vec![0; length];
        self.stream.read_exact(&mut payload).await.expect("reads");
        serde_json::from_slice(&payload).expect("JSON")
    }

    async fn read_u8(&mut self) -> u8 {
        let read = self.stream.read_u8();
        tokio::time::timeout(Duration::from_secs(10), read)
            .await
            .expect("in time")
            .expect("reads")
    }
}

/// The next state published in which `wanted` holds.
async fn state_where(
    bus: &mut Receiver<BrowserEvent>,
    wanted: impl Fn(&BrowserState) -> bool,
) -> BrowserState {
    tokio::time::timeout(Duration::from_secs(10), next_state(bus, wanted))
        .await
        .expect("heard in time")
}

async fn next_state(
    bus: &mut Receiver<BrowserEvent>,
    wanted: impl Fn(&BrowserState) -> bool,
) -> BrowserState {
    loop {
        if let BrowserEvent::State(state) = bus.recv().await.expect("open")
            && wanted(&state)
        {
            return state;
        }
    }
}

/// The next request to show the browser: who asked, and on which tab.
async fn next_show(bus: &mut Receiver<BrowserEvent>) -> (i64, Option<String>) {
    loop {
        if let BrowserEvent::Show {
            session_id, tab, ..
        } = bus.recv().await.expect("open")
        {
            return (session_id, tab);
        }
    }
}

/// A tool of Swarmed's called by `session`'s agent, as its tool server calls it.
async fn tool(socket: &PathBuf, session: i64, name: &str, arguments: Value) -> (bool, String) {
    let mut stream = UnixStream::connect(socket).await.expect("connects");
    let request = json!({ "session": session, "tool": name, "arguments": arguments });
    let line = format!("{request}\n");
    stream.write_all(line.as_bytes()).await.expect("writes");
    let mut answer = String::new();
    BufReader::new(stream)
        .read_line(&mut answer)
        .await
        .expect("reads");
    let answer: Value = serde_json::from_str(&answer).expect("JSON");
    let text = answer["text"].as_str().unwrap_or_default().to_string();
    (answer["ok"] == true, text)
}

#[tokio::test]
async fn an_agent_that_never_browses_starts_no_chrome_and_its_address_starts_one() {
    let fx = fixture();
    let core = serving(&fx).await;
    let session = started(&core);
    let address = said(&core, session, "env: SWARMED_BROWSER").await;
    let port = port_of(&address);
    // The agent runs, its address listens, and nothing has started a browser.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let task = task_id(&core);
    assert!(!core.browser_state(task).expect("reads").running);

    // A web page is turned away before anything starts.
    let from_a_page = request(port, "Origin: https://evil.test\r\n").await;
    assert!(from_a_page.starts_with("HTTP/1.1 403"), "{from_a_page}");
    assert!(!core.browser_state(task).expect("reads").running);

    // A tool on this machine starts the browser, and is told addresses that lead back here.
    let answer = request(port, "").await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let back = format!("\"webSocketDebuggerUrl\": \"ws://127.0.0.1:{port}/devtools/browser/");
    assert!(answer.contains(&back), "{answer}");
    assert!(core.browser_state(task).expect("reads").running);
    // The same address, however often it is asked for.
    assert_eq!(said(&core, session, "env: SWARMED_BROWSER").await, address);
    core.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn the_pane_follows_the_tab_a_sessions_tools_act_in_and_names_the_session() {
    let fx = fixture();
    let core = serving(&fx).await;
    let session = started(&core);
    let port = port_of(&said(&core, session, "env: SWARMED_BROWSER").await);
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();
    let shown = core.browser_show(task, None, VIEW).await.expect("shows");
    let first = shown.shown.clone().expect("a tab");
    let other = core
        .browser_tab_new(task, Some("http://localhost:3000/cart"))
        .await
        .expect("opens");
    state_where(&mut bus, |state| state.tabs.len() == 2).await;

    let version = request(port, "").await;
    let path = version
        .split("/devtools/browser/")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a path");
    let mut tool = Tool::open(port, &format!("/devtools/browser/{path}")).await;
    let attach = json!({ "id": 1, "method": "Target.attachToTarget",
        "params": { "targetId": other, "flatten": true } });
    let attached = tool.call(attach).await;
    let on = attached["result"]["sessionId"].clone();
    let go = json!({ "id": 2, "sessionId": on, "method": "Page.navigate",
        "params": { "url": "http://localhost:3000/checkout" } });
    tool.call(go).await;

    let followed = state_where(&mut bus, |state| state.shown.as_ref() == Some(&other)).await;
    let acted = |id: &str| {
        followed
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.acted_by)
    };
    assert_eq!(acted(&other), Some(Some(session)));
    assert_eq!(acted(&first), Some(None));
    core.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn show_browser_opens_a_tab_of_the_sessions_own_and_says_how_to_act_on_it() {
    let fx = fixture();
    let core = serving(&fx).await;
    let session = started(&core);
    let address = said(&core, session, "env: SWARMED_BROWSER").await;
    let socket = core.paths().tools_socket();
    let mut bus = core.subscribe_browsers();

    let asked = json!({ "url": "localhost:3000/products" });
    let (ok, answer) = tool(&socket, session, "show_browser", asked).await;
    assert!(ok, "{answer}");
    assert!(
        answer.contains("on http://localhost:3000/products (tab "),
        "{answer}"
    );
    assert!(
        answer.contains("http://localhost:3000/products (yours)"),
        "{answer}"
    );
    assert!(
        answer.contains(&format!("Its address: {address}")),
        "{answer}"
    );
    let (asked_by, tab) = tokio::time::timeout(Duration::from_secs(10), next_show(&mut bus))
        .await
        .expect("in time");
    assert_eq!(asked_by, session);
    let tab = tab.expect("a tab");

    // Asked again, the session's own tab goes where it is sent: no second tab.
    let again = json!({ "url": "http://localhost:3000/cart" });
    let (ok, answer) = tool(&socket, session, "show_browser", again).await;
    assert!(ok, "{answer}");
    assert!(answer.contains(&format!("http://localhost:3000/cart (tab {tab})")));
    let state = core.browser_state(task_id(&core)).expect("reads");
    let own = state
        .tabs
        .iter()
        .filter(|known| known.acted_by == Some(session))
        .count();
    assert_eq!(own, 1, "{state:?}");
    core.browser_stop(task_id(&core)).await.expect("stops");
}

#[tokio::test]
async fn claude_codes_own_browser_servers_are_handed_back_pointed_at_the_address() {
    let fx = fixture();
    let config = json!({ "mcpServers": {
        "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] },
        "devtools": { "command": "npx", "args": ["-y", "chrome-devtools-mcp@latest", "--channel", "canary"] },
        "firefox": { "command": "npx", "args": ["@playwright/mcp", "--browser", "firefox"] },
        "files": { "command": "npx", "args": ["files"] },
    }});
    std::fs::write(&fx.paths.claude_config, config.to_string()).expect("writes");
    let core = serving(&fx).await;
    let (_, task) = core.task("demo", "one").expect("there");
    // The repository's own, which Claude Code asks about first: not handed over.
    let repository =
        json!({ "mcpServers": { "team": { "command": "npx", "args": ["@playwright/mcp"] } } });
    std::fs::write(task.worktree_path.join(".mcp.json"), repository.to_string()).expect("writes");
    let session = started(&core);
    let address = said(&core, session, "env: SWARMED_BROWSER").await;

    assert_eq!(
        said(&core, session, "mcp: playwright").await,
        format!("npx @playwright/mcp@latest --cdp-endpoint {address}")
    );
    assert_eq!(
        said(&core, session, "mcp: devtools").await,
        format!("npx -y chrome-devtools-mcp@latest --browser-url={address}")
    );
    for left in ["firefox", "files", "team"] {
        assert_eq!(said(&core, session, &format!("mcp: {left}")).await, "none");
    }
    // Pointing a server starts nothing.
    assert!(!core.browser_state(task.id).expect("reads").running);
}
