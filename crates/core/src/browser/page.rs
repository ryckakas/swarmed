//! What the Browser pane shows of a task's Chrome: its tabs, and what each one logged.
//!
//! Pure: Chrome's events go in, and what changed comes out, for the connection to act on
//! (attach to a new page, read its history again, publish a picture). Nothing here talks to
//! Chrome or keeps time.

use std::collections::{HashMap, VecDeque};

use serde::Serialize;
use serde_json::Value;

use super::cdp::Event;

/// How many lines of what a page logged are kept per tab; older ones are dropped.
pub(crate) const CONSOLE_LINES: usize = 500;

/// The longest a line of the console is kept, in bytes.
const CONSOLE_LINE_BYTES: usize = 10_000;

/// A page open in a task's Chrome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowserTab {
    /// Chrome's id for it.
    pub id: String,
    /// The page's title; its address until it has one.
    pub title: String,
    /// Where it is.
    pub url: String,
    /// Whether it is loading.
    pub loading: bool,
    /// Whether it has a page to go back to.
    pub can_go_back: bool,
    /// Whether it has a page to go forward to.
    pub can_go_forward: bool,
    /// A dialog the page opened (`alert`, `confirm`, `prompt`), which holds it until answered.
    pub dialog: Option<PageDialog>,
}

/// A dialog a page opened, waiting for an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PageDialog {
    /// `alert`, `confirm`, `prompt` or `beforeunload`.
    pub kind: String,
    /// What it says.
    pub message: String,
    /// What a `prompt` offers as the answer.
    pub default_prompt: String,
}

/// How much a line of the console matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleLevel {
    /// `console.debug`, and what Chrome calls verbose.
    Debug,
    /// `console.log`, `console.info` and the rest.
    Info,
    /// `console.warn`.
    Warning,
    /// `console.error`, a failed `console.assert`, an uncaught error, a crash.
    Error,
}

/// One line of what a page logged, or of an error it did not catch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConsoleLine {
    /// Its place among every line of the task's Chrome, from 1: a front end that read the
    /// lines kept takes only those with a higher number from what it hears.
    pub n: u64,
    /// How much it matters.
    pub level: ConsoleLevel,
    /// What it says.
    pub text: String,
    /// Where it was said, as `address:line`, where Chrome says.
    pub source: Option<String>,
    /// When, in milliseconds since 1970.
    pub at: i64,
}

/// What changed with an event, for the connection to act on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Change {
    /// The tabs are different: what the pane shows is to be published again.
    Tabs,
    /// A new page, to attach to.
    Attach(String),
    /// This tab's history is to be read again: where back and forward lead.
    History(String),
    /// A line this tab logged.
    Console(String, ConsoleLine),
    /// A picture of this tab, to be published if it is the one shown, then acknowledged.
    Frame(Frame),
    /// Someone used the browser: it does not count as unused.
    Used,
}

/// A picture of a page, as Chrome sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) tab: String,
    /// The session it came on, to acknowledge it on.
    pub(crate) session: String,
    /// A JPEG, in base64.
    pub(crate) data: String,
    /// The page's size, in CSS pixels.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// What Chrome wants back to send the next one.
    pub(crate) ack: i64,
}

/// The tabs of one Chrome, what they logged, and which one the pane shows.
#[derive(Debug, Default)]
pub(crate) struct Pane {
    tabs: Vec<BrowserTab>,
    console: HashMap<String, VecDeque<ConsoleLine>>,
    /// Each page's session, by the page.
    sessions: HashMap<String, String>,
    shown: Option<String>,
    lines: u64,
}

impl Pane {
    /// The tabs, in the order they were opened.
    pub(crate) fn tabs(&self) -> Vec<BrowserTab> {
        self.tabs.clone()
    }

    /// The tab the pane shows, if it shows one.
    pub(crate) fn shown(&self) -> Option<&str> {
        self.shown.as_deref()
    }

    /// Shows `tab`, or none. Says whether there is such a tab.
    pub(crate) fn show(&mut self, tab: Option<&str>) -> bool {
        match tab {
            Some(tab) if !self.has(tab) => false,
            _ => {
                self.shown = tab.map(String::from);
                true
            }
        }
    }

    /// Whether there is a tab `id`.
    pub(crate) fn has(&self, id: &str) -> bool {
        self.tabs.iter().any(|tab| tab.id == id)
    }

    /// The session commands for `tab` go on, once Swarmed has attached to it.
    pub(crate) fn session_of(&self, tab: &str) -> Option<String> {
        self.sessions.get(tab).cloned()
    }

    /// Notes the session Chrome gave on attaching to `tab`.
    pub(crate) fn attached(&mut self, tab: &str, session: &str) {
        if self.has(tab) {
            self.sessions.insert(tab.to_string(), session.to_string());
        }
    }

    /// The lines `tab` logged that are kept, oldest first.
    pub(crate) fn console(&self, tab: &str) -> Vec<ConsoleLine> {
        self.console
            .get(tab)
            .map_or_default(|lines| lines.iter().cloned().collect())
    }

    /// Notes where `tab`'s history stands. Says whether that changed anything.
    pub(crate) fn history(&mut self, tab: &str, current: u64, entries: u64) -> bool {
        let Some(found) = self.tab_mut(tab) else {
            return false;
        };
        let (back, forward) = (current > 0, current + 1 < entries);
        let changed = (found.can_go_back, found.can_go_forward) != (back, forward);
        found.can_go_back = back;
        found.can_go_forward = forward;
        changed
    }

    /// Takes in one event, and says what changed.
    pub(crate) fn apply(&mut self, event: &Event) -> Vec<Change> {
        let params = &event.params;
        match event.method.as_str() {
            "Target.targetCreated" => self.created(&params["targetInfo"]),
            "Target.targetInfoChanged" => self.info_changed(&params["targetInfo"]),
            "Target.targetDestroyed" => self.destroyed(text(params, "targetId")),
            "Target.detachedFromTarget" => {
                self.sessions
                    .retain(|_, session| session != text(params, "sessionId"));
                vec![]
            }
            _ => match self.tab_of(event) {
                Some(tab) => self.on_page(&tab, event),
                None => vec![],
            },
        }
    }

    /// The tab an event of a page's session belongs to.
    fn tab_of(&self, event: &Event) -> Option<String> {
        let session = event.session.as_deref()?;
        let (tab, _) = self.sessions.iter().find(|(_, of)| *of == session)?;
        Some(tab.clone())
    }

    fn on_page(&mut self, tab: &str, event: &Event) -> Vec<Change> {
        let params = &event.params;
        match event.method.as_str() {
            "Page.frameStartedLoading" => self.loading(tab, params, true),
            "Page.frameStoppedLoading" => self.loading(tab, params, false),
            "Page.frameNavigated" if params["frame"].get("parentId").is_none() => {
                vec![Change::History(tab.to_string()), Change::Used]
            }
            "Page.navigatedWithinDocument" if text(params, "frameId") == tab => {
                vec![Change::History(tab.to_string())]
            }
            "Page.screencastFrame" => frame(tab, event).into_iter().collect(),
            "Page.javascriptDialogOpening" => self.dialog(tab, Some(dialog_of(params))),
            "Page.javascriptDialogClosed" => self.dialog(tab, None),
            "Inspector.targetCrashed" => {
                let crashed = (ConsoleLevel::Error, "The page crashed.".to_string(), None);
                self.logged(tab, crashed, now_of(params))
            }
            method => match said(method, params) {
                Some(line) => self.logged(tab, line, now_of(params)),
                None => vec![],
            },
        }
    }

    fn created(&mut self, info: &Value) -> Vec<Change> {
        let id = text(info, "targetId");
        if text(info, "type") != "page" || id.is_empty() || self.has(id) {
            return vec![];
        }
        self.tabs.push(BrowserTab {
            id: id.to_string(),
            title: title_of(info),
            url: text(info, "url").to_string(),
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            dialog: None,
        });
        vec![Change::Tabs, Change::Attach(id.to_string()), Change::Used]
    }

    fn info_changed(&mut self, info: &Value) -> Vec<Change> {
        let Some(tab) = self.tab_mut(text(info, "targetId")) else {
            return vec![];
        };
        let (title, url) = (title_of(info), text(info, "url").to_string());
        if tab.title == title && tab.url == url {
            return vec![];
        }
        tab.title = title;
        tab.url = url;
        vec![Change::Tabs, Change::Used]
    }

    fn destroyed(&mut self, id: &str) -> Vec<Change> {
        if !self.has(id) {
            return vec![];
        }
        self.tabs.retain(|tab| tab.id != id);
        self.console.remove(id);
        self.sessions.remove(id);
        if self.shown.as_deref() == Some(id) {
            self.shown = None;
        }
        vec![Change::Tabs, Change::Used]
    }

    fn loading(&mut self, tab: &str, params: &Value, loading: bool) -> Vec<Change> {
        // A page's own frame has the page's id; frames inside it load on their own.
        if text(params, "frameId") != tab {
            return vec![];
        }
        match self.tab_mut(tab) {
            Some(found) if found.loading != loading => {
                found.loading = loading;
                vec![Change::Tabs]
            }
            _ => vec![],
        }
    }

    fn dialog(&mut self, tab: &str, dialog: Option<PageDialog>) -> Vec<Change> {
        let Some(found) = self.tab_mut(tab) else {
            return vec![];
        };
        found.dialog = dialog;
        vec![Change::Tabs, Change::Used]
    }

    /// Keeps a line `tab` logged, and says it.
    fn logged(&mut self, tab: &str, line: Said, at: i64) -> Vec<Change> {
        let (level, text, source) = line;
        self.lines += 1;
        let line = ConsoleLine {
            n: self.lines,
            level,
            text: cut(text),
            source,
            at,
        };
        let kept = self.console.entry(tab.to_string()).or_default();
        kept.push_back(line.clone());
        while kept.len() > CONSOLE_LINES {
            kept.pop_front();
        }
        vec![Change::Console(tab.to_string(), line)]
    }

    fn tab_mut(&mut self, id: &str) -> Option<&mut BrowserTab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }
}

/// A line of the console before it has its place: how much it matters, what it says, where.
type Said = (ConsoleLevel, String, Option<String>);

/// What an event of the `Runtime` or `Log` domain logged, if it logged anything.
fn said(method: &str, params: &Value) -> Option<Said> {
    match method {
        "Runtime.consoleAPICalled" => {
            let args = params["args"].as_array().map_or_default(Vec::as_slice);
            let words: Vec<String> = args.iter().map(object_text).collect();
            let frame = &params["stackTrace"]["callFrames"][0];
            Some((
                level_of_call(text(params, "type")),
                words.join(" "),
                source(frame),
            ))
        }
        "Runtime.exceptionThrown" => {
            let details = &params["exceptionDetails"];
            let thrown = details["exception"]["description"]
                .as_str()
                .unwrap_or_else(|| text(details, "text"));
            Some((ConsoleLevel::Error, thrown.to_string(), source(details)))
        }
        "Log.entryAdded" => {
            let entry = &params["entry"];
            let level = level_of_entry(text(entry, "level"));
            Some((level, text(entry, "text").to_string(), source(entry)))
        }
        _ => None,
    }
}

/// A value a page logged, as the console would write it: plain values as they are, objects
/// by Chrome's description of them.
fn object_text(object: &Value) -> String {
    match object.get("value") {
        Some(Value::String(text)) => text.clone(),
        Some(value) => value.to_string(),
        None => ["unserializableValue", "description", "type"]
            .iter()
            .find_map(|key| object.get(key)?.as_str())
            .unwrap_or_default()
            .to_string(),
    }
}

fn level_of_call(kind: &str) -> ConsoleLevel {
    match kind {
        "debug" => ConsoleLevel::Debug,
        "warning" => ConsoleLevel::Warning,
        "error" | "assert" => ConsoleLevel::Error,
        _ => ConsoleLevel::Info,
    }
}

fn level_of_entry(level: &str) -> ConsoleLevel {
    match level {
        "verbose" => ConsoleLevel::Debug,
        "warning" => ConsoleLevel::Warning,
        "error" => ConsoleLevel::Error,
        _ => ConsoleLevel::Info,
    }
}

/// `address:line` of something with Chrome's `url` and `lineNumber`, which counts from 0.
fn source(at: &Value) -> Option<String> {
    let url = text(at, "url");
    if url.is_empty() {
        return None;
    }
    Some(match at["lineNumber"].as_u64() {
        Some(line) => format!("{url}:{}", line + 1),
        None => url.to_string(),
    })
}

/// When an event happened, in milliseconds since 1970, as Chrome said; 0 where it did not.
#[allow(
    clippy::cast_possible_truncation,
    reason = "milliseconds since 1970 fit an i64 for a long while yet"
)]
fn now_of(params: &Value) -> i64 {
    let at = params["timestamp"]
        .as_f64()
        .or_else(|| params["entry"]["timestamp"].as_f64())
        .unwrap_or_default();
    at as i64
}

fn dialog_of(params: &Value) -> PageDialog {
    PageDialog {
        kind: text(params, "type").to_string(),
        message: text(params, "message").to_string(),
        default_prompt: text(params, "defaultPrompt").to_string(),
    }
}

fn frame(tab: &str, event: &Event) -> Option<Change> {
    let params = &event.params;
    let size = |key: &str| {
        params["metadata"][key]
            .as_f64()
            .and_then(|pixels| format!("{pixels:.0}").parse().ok())
            .unwrap_or_default()
    };
    Some(Change::Frame(Frame {
        tab: tab.to_string(),
        session: event.session.clone()?,
        data: text(params, "data").to_string(),
        width: size("deviceWidth"),
        height: size("deviceHeight"),
        ack: params["sessionId"].as_i64()?,
    }))
}

/// A page's title, or its address while it has none.
fn title_of(info: &Value) -> String {
    let title = text(info, "title");
    if title.is_empty() {
        text(info, "url").to_string()
    } else {
        title.to_string()
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// `text` cut to what a line keeps, on a character's edge.
fn cut(mut text: String) -> String {
    if text.len() > CONSOLE_LINE_BYTES {
        let mut end = CONSOLE_LINE_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// What an event that changes nothing changes.
    const NONE: [Change; 0] = [];

    fn event(method: &str, params: Value) -> Event {
        Event {
            session: None,
            method: method.into(),
            params,
        }
    }

    fn on(session: &str, method: &str, params: Value) -> Event {
        Event {
            session: Some(session.into()),
            ..event(method, params)
        }
    }

    fn page(id: &str, url: &str, title: &str) -> Value {
        json!({ "targetInfo": { "targetId": id, "type": "page", "url": url, "title": title } })
    }

    /// A pane with one tab, `T`, attached on session `S`.
    fn one_tab() -> Pane {
        let mut pane = Pane::default();
        pane.apply(&event("Target.targetCreated", page("T", "about:blank", "")));
        pane.attached("T", "S");
        pane
    }

    #[test]
    fn a_new_page_is_a_tab_to_attach_to_and_anything_else_is_not() {
        let mut pane = Pane::default();
        let changes = pane.apply(&event("Target.targetCreated", page("T", "about:blank", "")));
        assert_eq!(
            changes,
            [Change::Tabs, Change::Attach("T".into()), Change::Used]
        );
        assert_eq!(pane.tabs()[0].title, "about:blank");
        let worker = json!({ "targetInfo": { "targetId": "W", "type": "service_worker" } });
        assert_eq!(pane.apply(&event("Target.targetCreated", worker)), NONE);
        // Chrome tells of the pages it has when asked to, and again of new ones: once each.
        assert_eq!(
            pane.apply(&event("Target.targetCreated", page("T", "x", ""))),
            NONE
        );
        assert_eq!(pane.tabs().len(), 1);
    }

    #[test]
    fn a_tab_follows_its_title_and_address_and_goes_with_its_page() {
        let mut pane = one_tab();
        assert!(pane.show(Some("T")));
        let changed = page("T", "http://localhost:3000/", "Shop");
        assert_eq!(
            pane.apply(&event("Target.targetInfoChanged", changed.clone())),
            [Change::Tabs, Change::Used]
        );
        assert_eq!(
            pane.apply(&event("Target.targetInfoChanged", changed)),
            NONE
        );
        assert_eq!(pane.tabs()[0].title, "Shop");
        let gone = json!({ "targetId": "T" });
        pane.apply(&event("Target.targetDestroyed", gone));
        assert_eq!(pane.tabs(), Vec::<BrowserTab>::new());
        assert_eq!(pane.shown(), None);
        assert_eq!(pane.session_of("T"), None);
    }

    #[test]
    fn only_the_pages_own_frame_says_whether_it_is_loading() {
        let mut pane = one_tab();
        let inner = json!({ "frameId": "an-iframe" });
        assert_eq!(
            pane.apply(&on("S", "Page.frameStartedLoading", inner)),
            NONE
        );
        let own = json!({ "frameId": "T" });
        assert_eq!(
            pane.apply(&on("S", "Page.frameStartedLoading", own.clone())),
            [Change::Tabs]
        );
        assert!(pane.tabs()[0].loading);
        pane.apply(&on("S", "Page.frameStoppedLoading", own));
        assert!(!pane.tabs()[0].loading);
    }

    #[test]
    fn a_navigation_of_the_page_itself_reads_its_history_again() {
        let mut pane = one_tab();
        let top = json!({ "frame": { "id": "T", "url": "http://localhost:3000/" } });
        assert_eq!(
            pane.apply(&on("S", "Page.frameNavigated", top)),
            [Change::History("T".into()), Change::Used]
        );
        let inner = json!({ "frame": { "id": "F", "parentId": "T" } });
        assert_eq!(pane.apply(&on("S", "Page.frameNavigated", inner)), NONE);
        assert!(pane.history("T", 1, 2));
        let tab = &pane.tabs()[0];
        assert!(tab.can_go_back && !tab.can_go_forward);
        assert!(!pane.history("T", 1, 2));
    }

    #[test]
    fn what_a_page_logs_is_kept_per_tab_numbered_and_worded_as_the_console_would() {
        let mut pane = one_tab();
        let call = json!({
            "type": "warning",
            "args": [{ "type": "string", "value": "low on" }, { "type": "number", "value": 3 },
                     { "type": "object", "description": "Array(2)" }],
            "timestamp": 1_700_000_000_123.4,
            "stackTrace": { "callFrames": [{ "url": "http://localhost:3000/app.js", "lineNumber": 9 }] },
        });
        let changes = pane.apply(&on("S", "Runtime.consoleAPICalled", call));
        let line = ConsoleLine {
            n: 1,
            level: ConsoleLevel::Warning,
            text: "low on 3 Array(2)".into(),
            source: Some("http://localhost:3000/app.js:10".into()),
            at: 1_700_000_000_123,
        };
        assert_eq!(changes, [Change::Console("T".into(), line.clone())]);
        let thrown = json!({ "timestamp": 1.0, "exceptionDetails": {
            "text": "Uncaught", "exception": { "description": "TypeError: x is undefined" } } });
        pane.apply(&on("S", "Runtime.exceptionThrown", thrown));
        let console = pane.console("T");
        assert_eq!(console[0], line);
        assert_eq!(console[1].text, "TypeError: x is undefined");
        assert_eq!((console[1].n, console[1].level), (2, ConsoleLevel::Error));
        assert_eq!(pane.console("another tab"), Vec::<ConsoleLine>::new());
    }

    #[test]
    fn the_console_keeps_only_its_latest_lines_and_cuts_long_ones() {
        let mut pane = one_tab();
        for n in 0..=CONSOLE_LINES {
            let entry = json!({ "entry": { "level": "info", "text": format!("line {n}") } });
            pane.apply(&on("S", "Log.entryAdded", entry));
        }
        let console = pane.console("T");
        assert_eq!(console.len(), CONSOLE_LINES);
        assert_eq!(console[0].text, "line 1");
        let long =
            json!({ "type": "log", "args": [{ "type": "string", "value": "é".repeat(9_000) }] });
        pane.apply(&on("S", "Runtime.consoleAPICalled", long));
        let last = pane.console("T").pop().expect("kept");
        assert!(last.text.len() <= CONSOLE_LINE_BYTES + '…'.len_utf8());
        assert!(last.text.ends_with('…'));
    }

    #[test]
    fn a_dialog_holds_its_tab_until_it_closes() {
        let mut pane = one_tab();
        let opening = json!({ "type": "confirm", "message": "Delete it?", "defaultPrompt": "" });
        pane.apply(&on("S", "Page.javascriptDialogOpening", opening));
        let dialog = pane.tabs()[0].dialog.clone().expect("open");
        assert_eq!(
            (dialog.kind.as_str(), dialog.message.as_str()),
            ("confirm", "Delete it?")
        );
        pane.apply(&on(
            "S",
            "Page.javascriptDialogClosed",
            json!({ "result": true }),
        ));
        assert_eq!(pane.tabs()[0].dialog, None);
    }

    #[test]
    fn a_picture_says_its_page_its_size_and_what_to_acknowledge() {
        let mut pane = one_tab();
        let picture = json!({ "data": "/9j/", "sessionId": 4,
            "metadata": { "deviceWidth": 800.0, "deviceHeight": 600.4 } });
        let changes = pane.apply(&on("S", "Page.screencastFrame", picture));
        assert_eq!(
            changes,
            [Change::Frame(Frame {
                tab: "T".into(),
                session: "S".into(),
                data: "/9j/".into(),
                width: 800,
                height: 600,
                ack: 4,
            })]
        );
        // An event of a session nobody attached to belongs to no tab.
        let stray = json!({ "data": "", "sessionId": 1, "metadata": {} });
        assert_eq!(pane.apply(&on("X", "Page.screencastFrame", stray)), NONE);
    }

    #[test]
    fn only_an_open_tab_can_be_shown_and_showing_none_always_works() {
        let mut pane = one_tab();
        assert!(!pane.show(Some("nowhere")));
        assert!(pane.show(Some("T")));
        assert_eq!(pane.shown(), Some("T"));
        assert!(pane.show(None));
        assert_eq!(pane.shown(), None);
    }
}
