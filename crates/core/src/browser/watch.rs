//! Which tab a session's browser tools act in, read from the messages of one connection.
//!
//! Pure: what the tools send and what Chrome answers go in, and the tab acted in comes out.
//! A command names a tab by the session the tool attached to it with, which Chrome says when
//! it attaches (`Target.attachedToTarget`) or answers an attach. What counts as acting is what
//! moves or shows the page: going somewhere, input, a picture taken, a tab brought forward or
//! opened. Reading the page does not, or the pane would jump to every tab a tool looks at as
//! it connects.

use std::collections::HashMap;

use serde_json::Value;

/// The commands that act on a page.
const ACTING: [&str; 7] = [
    "Page.navigate",
    "Page.reload",
    "Page.navigateToHistoryEntry",
    "Page.bringToFront",
    "Page.captureScreenshot",
    "Page.handleJavaScriptDialog",
    "DOM.setFileInputFiles",
];

/// How many questions are kept waiting for their answer at most, should Chrome not answer.
const MAX_ASKED: usize = 1024;

/// What one connection's tools did, as far as it says which tab they act in.
#[derive(Debug, Default)]
pub(crate) struct Watch {
    /// The page each of the tool's sessions is attached to.
    pages: HashMap<String, String>,
    /// What the tool asked that the answer says more about, by its session and id.
    asked: HashMap<(String, u64), Asked>,
}

#[derive(Debug)]
enum Asked {
    /// To attach to this page: the answer names the session.
    Attach(String),
    /// To open a tab: the answer names it.
    Open,
}

impl Watch {
    /// A message the tool sent: the tab it acts in, if it acts in one.
    pub(crate) fn sent(&mut self, message: &Value) -> Option<String> {
        let method = message.get("method")?.as_str()?;
        let session = text(message, "sessionId");
        let params = &message["params"];
        let id = message.get("id").and_then(Value::as_u64);
        match method {
            "Target.attachToTarget" => {
                self.ask(
                    session,
                    id?,
                    Asked::Attach(text(params, "targetId").to_string()),
                );
                None
            }
            "Target.createTarget" => {
                self.ask(session, id?, Asked::Open);
                None
            }
            "Target.activateTarget" => Some(text(params, "targetId").to_string()),
            _ if ACTING.contains(&method) || method.starts_with("Input.") => {
                self.pages.get(session).cloned()
            }
            _ => None,
        }
    }

    /// A message Chrome sent the tool: the tab it opened, if this answers its asking for one.
    pub(crate) fn heard(&mut self, message: &Value) -> Option<String> {
        let params = &message["params"];
        match message.get("method").and_then(Value::as_str) {
            Some("Target.attachedToTarget") => {
                let info = &params["targetInfo"];
                let page = (text(info, "type") == "page").then(|| text(info, "targetId"));
                self.note(text(params, "sessionId"), page.unwrap_or_default());
                None
            }
            Some("Target.detachedFromTarget") => {
                self.pages.remove(text(params, "sessionId"));
                None
            }
            Some(_) => None,
            None => self.answered(message),
        }
    }

    /// Whether a message from Chrome is worth reading: an answer to something asked, or news
    /// of a session. Most of what Chrome sends is neither.
    pub(crate) fn wants(&self, bytes: &[u8]) -> bool {
        let answer = !self.asked.is_empty() && bytes.starts_with(br#"{"id":"#);
        answer
            || contains(bytes, b"\"Target.attachedToTarget\"")
            || contains(bytes, b"\"Target.detachedFromTarget\"")
    }

    /// Notes that the tool's `session` stands for `page`.
    fn note(&mut self, session: &str, page: &str) {
        if !session.is_empty() && !page.is_empty() {
            self.pages.insert(session.to_string(), page.to_string());
        }
    }

    fn ask(&mut self, session: &str, id: u64, asked: Asked) {
        if self.asked.len() < MAX_ASKED {
            self.asked.insert((session.to_string(), id), asked);
        }
    }

    fn answered(&mut self, message: &Value) -> Option<String> {
        let id = message.get("id")?.as_u64()?;
        let key = (text(message, "sessionId").to_string(), id);
        let result = &message["result"];
        match self.asked.remove(&key)? {
            Asked::Attach(page) => {
                self.note(text(result, "sessionId"), &page);
                None
            }
            Asked::Open => Some(text(result, "targetId").to_string()).filter(|tab| !tab.is_empty()),
        }
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn attached(watch: &mut Watch, session: &str, page: &str) {
        let event = json!({ "method": "Target.attachedToTarget", "params": {
            "sessionId": session, "targetInfo": { "targetId": page, "type": "page" } } });
        assert_eq!(watch.heard(&event), None);
    }

    #[test]
    fn a_command_on_a_page_the_tool_attached_to_acts_in_that_tab() {
        let mut watch = Watch::default();
        attached(&mut watch, "S1", "T1");
        let go = json!({ "id": 7, "sessionId": "S1", "method": "Page.navigate",
            "params": { "url": "http://localhost:3000/" } });
        assert_eq!(watch.sent(&go), Some("T1".into()));
        let click = json!({ "id": 8, "sessionId": "S1", "method": "Input.dispatchMouseEvent" });
        assert_eq!(watch.sent(&click), Some("T1".into()));
        // Reading the page is no acting, and a session nobody heard of belongs to no tab.
        let read = json!({ "id": 9, "sessionId": "S1", "method": "Runtime.evaluate" });
        assert_eq!(watch.sent(&read), None);
        let stray = json!({ "id": 10, "sessionId": "S9", "method": "Page.navigate" });
        assert_eq!(watch.sent(&stray), None);
        // Detached, the session no longer stands for the page.
        let gone =
            json!({ "method": "Target.detachedFromTarget", "params": { "sessionId": "S1" } });
        watch.heard(&gone);
        assert_eq!(watch.sent(&go), None);
    }

    #[test]
    fn an_attach_asked_for_is_known_from_its_answer() {
        let mut watch = Watch::default();
        let attach = json!({ "id": 3, "method": "Target.attachToTarget",
            "params": { "targetId": "T2", "flatten": true } });
        assert_eq!(watch.sent(&attach), None);
        let answer = br#"{"id":3,"result":{"sessionId":"S2"}}"#;
        assert!(watch.wants(answer));
        let answer: Value = serde_json::from_slice(answer).expect("json");
        assert_eq!(watch.heard(&answer), None);
        let shot = json!({ "id": 4, "sessionId": "S2", "method": "Page.captureScreenshot" });
        assert_eq!(watch.sent(&shot), Some("T2".into()));
    }

    #[test]
    fn a_tab_the_tool_opens_or_brings_forward_is_acted_in() {
        let mut watch = Watch::default();
        let open =
            json!({ "id": 5, "method": "Target.createTarget", "params": { "url": "about:blank" } });
        assert_eq!(watch.sent(&open), None);
        let opened = json!({ "id": 5, "result": { "targetId": "T3" } });
        assert_eq!(watch.heard(&opened), Some("T3".into()));
        // Answered once: the same id again is someone else's.
        assert_eq!(watch.heard(&opened), None);
        let forward =
            json!({ "id": 6, "method": "Target.activateTarget", "params": { "targetId": "T4" } });
        assert_eq!(watch.sent(&forward), Some("T4".into()));
    }

    #[test]
    fn most_of_what_chrome_says_is_not_read() {
        let watch = Watch::default();
        assert!(!watch.wants(br#"{"method":"Network.dataReceived","params":{}}"#));
        assert!(!watch.wants(br#"{"id":1,"result":{}}"#));
        assert!(watch.wants(br#"{"method":"Target.attachedToTarget","params":{}}"#));
    }
}
