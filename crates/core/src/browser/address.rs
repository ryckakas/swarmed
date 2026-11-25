//! The address a session's browser tools reach the task's Chrome at (#384).
//!
//! Each session is given one of its own, on `127.0.0.1` at a port the system picks, as
//! `SWARMED_BROWSER`, and its browser servers are pointed at it (`servers.rs`). Swarmed holds
//! it from when the agent starts, so the address is there before Chrome is: the first
//! connection starts the task's Chrome, and a session that never browses starts nothing.
//!
//! What passes is passed on unchanged, both ways. Chrome builds the addresses it hands out
//! (`webSocketDebuggerUrl` in `/json/version`) from the `Host` the tool sent, so they lead
//! back here. On the way, a WebSocket's short messages are read (`watch.rs`) for the tab the
//! session's tools act in, which the pane follows and names. Nothing read is kept or logged.
//!
//! A request a web page made is refused before anything starts: one with an `Origin`, or that
//! a browser says came from a site (`Sec-Fetch-Site`), or that names a host other than this
//! machine. Chrome refuses those itself, but only once it runs.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use super::watch::Watch;
use super::ws::Unframer;
use super::{Browser, Browsers};
use crate::{Core, lock};

/// The longest a request's head may be.
const MAX_HEAD: usize = 16 * 1024;

/// How long a tool is given to say what it wants once it connected.
const HEAD_WAIT: Duration = Duration::from_secs(10);

/// How much is passed on at a time.
const CHUNK: usize = 64 * 1024;

/// One session's address, listened on while it is kept.
#[derive(Debug)]
pub(crate) struct Address {
    pub(crate) port: u16,
    pub(crate) task: i64,
    listening: JoinHandle<()>,
}

impl Drop for Address {
    fn drop(&mut self) {
        self.listening.abort();
    }
}

/// The address `http://127.0.0.1:<port>`.
pub(crate) fn endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// Listens for session `session`'s tools at a port of its own, starting the Chrome of task
/// `task` on the first connection. Needs a Tokio runtime.
pub(crate) fn listen(home: Weak<Core>, task: i64, session: i64) -> std::io::Result<Address> {
    tokio::runtime::Handle::try_current().map_err(std::io::Error::other)?;
    let bound = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    bound.set_nonblocking(true)?;
    let port = bound.local_addr()?.port();
    let listener = TcpListener::from_std(bound)?;
    let listening = tokio::spawn(accept(listener, home, task, session));
    Ok(Address {
        port,
        task,
        listening,
    })
}

async fn accept(listener: TcpListener, home: Weak<Core>, task: i64, session: i64) {
    loop {
        let Ok((client, _)) = listener.accept().await else {
            // Out of files for a moment, most likely: the next try may do.
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        if home.strong_count() == 0 {
            return;
        }
        drop(tokio::spawn(pass_on(
            Weak::clone(&home),
            client,
            task,
            session,
        )));
    }
}

/// Passes one connection on to the task's Chrome, started for it if need be.
async fn pass_on(home: Weak<Core>, mut client: TcpStream, task: i64, session: i64) {
    client.set_nodelay(true).ok();
    let read = tokio::time::timeout(HEAD_WAIT, read_head(&mut client)).await;
    let Ok(Ok(Some((head, rest)))) = read else {
        return;
    };
    if let Some(why) = refused(&head) {
        answer(&mut client, "403 Forbidden", why).await;
        return;
    }
    let Some(core) = home.upgrade() else {
        return;
    };
    let browser = match core.browser_of(task).await {
        Ok(browser) => browser,
        Err(error) => {
            answer(&mut client, "503 Service Unavailable", &error.to_string()).await;
            return;
        }
    };
    let browsers = Arc::clone(&core.browsers);
    // Not held while the connection lasts, which may be as long as the agent runs.
    drop(core);
    browser.touch();
    let Ok(mut chrome) = TcpStream::connect(("127.0.0.1", browser.port)).await else {
        answer(
            &mut client,
            "502 Bad Gateway",
            "the task's browser did not answer",
        )
        .await;
        return;
    };
    chrome.set_nodelay(true).ok();
    let sent = chrome.write_all(&passed_on(&head)).await;
    if sent.is_err() || chrome.write_all(&rest).await.is_err() {
        return;
    }
    if is_upgrade(&head) {
        let ends = Ends {
            browser,
            browsers,
            session,
            watch: Mutex::default(),
        };
        relay(client, chrome, &rest, &ends).await;
    } else {
        tokio::io::copy_bidirectional(&mut client, &mut chrome)
            .await
            .ok();
    }
}

/// Reads a request's head: the head, up to and with its blank line, and what came after it.
/// `None` if the connection ended first.
async fn read_head(client: &mut TcpStream) -> std::io::Result<Option<(Vec<u8>, Vec<u8>)>> {
    let mut read = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = read.windows(4).position(|four| four == b"\r\n\r\n") {
            let rest = read.split_off(end + 4);
            return Ok(Some((read, rest)));
        }
        if read.len() > MAX_HEAD {
            return Ok(None);
        }
        let got = client.read(&mut chunk).await?;
        if got == 0 {
            return Ok(None);
        }
        read.extend_from_slice(&chunk[..got]);
    }
}

/// The value of header `name` in a request's head, if it has one.
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

/// Why a request is refused, if it is: it came from a web page, or names another host.
pub(crate) fn refused(head: &[u8]) -> Option<&'static str> {
    let head = String::from_utf8_lossy(head);
    let from_a_page = "a web page cannot reach the task's browser";
    if header(&head, "origin").is_some() {
        return Some(from_a_page);
    }
    if header(&head, "sec-fetch-site").is_some_and(|site| site != "none") {
        return Some(from_a_page);
    }
    let host = header(&head, "host").unwrap_or_default();
    let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    let here = ["127.0.0.1", "localhost"].contains(&name);
    (!here).then_some("the task's browser answers only for 127.0.0.1")
}

/// Whether a request asks to speak WebSocket.
fn is_upgrade(head: &[u8]) -> bool {
    let head = String::from_utf8_lossy(head);
    header(&head, "upgrade").is_some_and(|upgrade| upgrade.eq_ignore_ascii_case("websocket"))
}

/// The head as it goes on to Chrome: without asking for compressed messages, which could not
/// be read on the way.
fn passed_on(head: &[u8]) -> Vec<u8> {
    let head = String::from_utf8_lossy(head);
    let kept: Vec<&str> = head
        .split("\r\n")
        .filter(|line| {
            let name = line.split_once(':').map_or("", |(name, _)| name.trim());
            !name.eq_ignore_ascii_case("sec-websocket-extensions")
        })
        .collect();
    kept.join("\r\n").into_bytes()
}

/// Answers a request with `status` and a line saying why, and ends the connection.
async fn answer(client: &mut TcpStream, status: &str, why: &str) {
    let body = format!("{why}\n");
    let said = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    client.write_all(said.as_bytes()).await.ok();
    client.shutdown().await.ok();
}

/// What the messages of one WebSocket connection reach.
struct Ends {
    browser: Arc<Browser>,
    browsers: Arc<Browsers>,
    session: i64,
    watch: Mutex<Watch>,
}

/// Which way a message went.
#[derive(Clone, Copy)]
enum Way {
    /// From the session's tool to Chrome.
    Sent,
    /// From Chrome to the tool.
    Heard,
}

impl Ends {
    fn read(&self, way: Way, message: &[u8]) {
        let tab = match way {
            Way::Sent => self.sent(message),
            Way::Heard => self.heard(message),
        };
        if let Some(tab) = tab {
            self.browser.acted(&self.browsers, &tab, self.session);
        }
    }

    /// The tab a message the tool sent acts in. The tool sending anything is a use.
    fn sent(&self, message: &[u8]) -> Option<String> {
        self.browser.touch();
        let message: Value = serde_json::from_slice(message).ok()?;
        lock(&self.watch).sent(&message)
    }

    /// The tab a message Chrome sent the tool says it opened. Only what may say so is read.
    fn heard(&self, message: &[u8]) -> Option<String> {
        let mut watch = lock(&self.watch);
        let message: Value = watch
            .wants(message)
            .then(|| serde_json::from_slice(message).ok())??;
        watch.heard(&message)
    }
}

/// Passes a WebSocket connection on both ways until either side ends it.
async fn relay(client: TcpStream, chrome: TcpStream, early: &[u8], ends: &Ends) {
    let (from_client, to_client) = client.into_split();
    let (from_chrome, to_chrome) = chrome.into_split();
    let mut sent = Unframer::default();
    for message in sent.push(early) {
        ends.read(Way::Sent, &message);
    }
    tokio::join!(
        carry(from_client, to_chrome, sent, Way::Sent, ends),
        carry(
            from_chrome,
            to_client,
            Unframer::after_head(),
            Way::Heard,
            ends
        ),
    );
}

/// Passes what one side says on to the other, reading the messages on the way, until it ends.
async fn carry(
    mut from: OwnedReadHalf,
    mut to: OwnedWriteHalf,
    mut unframer: Unframer,
    way: Way,
    ends: &Ends,
) {
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let read = match from.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        if to.write_all(&chunk[..read]).await.is_err() {
            break;
        }
        for message in unframer.push(&chunk[..read]) {
            ends.read(way, &message);
        }
    }
    to.shutdown().await.ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(lines: &[&str]) -> Vec<u8> {
        format!("{}\r\n\r\n", lines.join("\r\n")).into_bytes()
    }

    #[test]
    fn a_tool_on_this_machine_is_let_through_and_a_web_page_is_not() {
        let tool = head(&[
            "GET /json/version HTTP/1.1",
            "host: 127.0.0.1:50123",
            "accept: */*",
        ]);
        assert_eq!(refused(&tool), None);
        let by_name = head(&["GET /json/version HTTP/1.1", "Host: localhost:50123"]);
        assert_eq!(refused(&by_name), None);
        let page = head(&[
            "GET /json HTTP/1.1",
            "Host: 127.0.0.1:50123",
            "Origin: https://evil.test",
        ]);
        assert!(refused(&page).is_some());
        let no_cors = head(&[
            "GET /json HTTP/1.1",
            "Host: 127.0.0.1:1",
            "Sec-Fetch-Site: cross-site",
        ]);
        assert!(refused(&no_cors).is_some());
        let typed = head(&[
            "GET /json HTTP/1.1",
            "Host: 127.0.0.1:1",
            "Sec-Fetch-Site: none",
        ]);
        assert_eq!(refused(&typed), None);
        let rebound = head(&["GET /json HTTP/1.1", "Host: evil.test:50123"]);
        assert!(refused(&rebound).is_some());
    }

    #[test]
    fn a_websocket_is_asked_for_without_compression() {
        let asked = head(&[
            "GET /devtools/browser/x HTTP/1.1",
            "Host: 127.0.0.1:50123",
            "Upgrade: websocket",
            "Sec-WebSocket-Extensions: permessage-deflate; client_max_window_bits",
        ]);
        assert!(is_upgrade(&asked));
        let passed = String::from_utf8(passed_on(&asked)).expect("text");
        assert!(!passed.to_lowercase().contains("permessage"), "{passed}");
        assert!(passed.ends_with("Upgrade: websocket\r\n\r\n"), "{passed:?}");
        assert!(!is_upgrade(&head(&[
            "GET /json/version HTTP/1.1",
            "Host: 127.0.0.1:1"
        ])));
    }
}
