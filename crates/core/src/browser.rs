//! A Chrome for each task, shown live beside its sessions (#382).
//!
//! - **One Chrome per task**, on a profile of the task's own in the data directory
//!   (`browsers/<task>/profile`), readable by the user alone: cookies, storage and logins are
//!   the task's, never the user's own browser's nor another task's, and they outlast restarts.
//!   One process per task rather than one Chrome with a context per task, whose cookies would
//!   go with every restart. Deleted with its task; the clean-up lists one whose task is gone.
//! - **Without a window.** Its DevTools endpoint listens on a port Chrome picks, on
//!   `127.0.0.1`, and Swarmed connects to that port with a WebSocket of its own (`ws.rs`). The
//!   port and not a pipe: it is how agents' own browser tools reach it (#384), and the only
//!   way back to a Chrome an earlier Swarmed started, which a pipe would not outlive. No
//!   `--remote-allow-origins` is ever passed, so a web page cannot reach the endpoint.
//! - **Started when used**: when the pane opens, or by `swarmed task browser`. It opens on the
//!   task's stack, if that answers. Written down under `run/browsers/` as a stack is, and
//!   **stopped** when its task is put away or removed, when Swarmed quits (the first Swarmed to
//!   arrive stops every one, any other those it started), and after sitting unused for as long
//!   as the settings say, which the first Swarmed looks at ([`follow_browsers`]).
//! - **The pane draws pictures.** Chrome sends pictures of the tab shown, only while the pane
//!   is on screen (`Page.startScreencast`), at most [`FRAMES_A_SECOND`] a second. They are
//!   published on a channel of their own ([`Core::subscribe_browsers`]) and never stored. The
//!   user's clicks and keys go back as input events. No page ever runs in Swarmed's window.
//! - **What a page shows or logs** is the user's: shown in the pane, never logged.
//! - **Agents act on it with their own browser tools** (#384), through an address of their
//!   session's own that starts it on the first connection (`address.rs`, `agents.rs`). The pane
//!   names the session that last acted in each tab, and follows the tab acted in.

mod address;
mod agents;
mod cdp;
mod find;
mod input;
mod page;
mod pick;
mod process;
mod servers;
mod watch;
mod ws;

use std::collections::HashMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc};

pub(crate) use agents::{AgentBrowser, BROWSER_ENV, instructions};
pub use find::BrowserSetup;
pub use input::{KeyInput, KeyKind, Modifiers, MouseInput, MouseKind};
pub use page::{BrowserTab, ConsoleLevel, ConsoleLine, PageDialog};
pub(crate) use pick::descriptions_beside;
pub use pick::{
    ELEMENTS_PICKED, ElementBox, ElementStyle, MAX_HTML_CHARS, MAX_TEXT_CHARS, PickedElement,
    with_elements,
};

use crate::agent::AgentEvent;
use crate::error::{Error, Result};
use crate::hooks::{self, OutputSink};
use crate::sessions::Live;
use crate::stacks::StackState;
use crate::store::Task;
use crate::{Core, lock};
use cdp::{Cdp, Event};
use page::{Change, Frame, Pane};

/// The most pictures of a page sent to the window in a second.
pub const FRAMES_A_SECOND: u32 = 15;

/// The JPEG quality of those pictures, from 0 to 100.
const QUALITY: u8 = 70;

/// How many events a slow window may fall behind; pictures it missed are not sent again.
const BUFFER: usize = 64;

/// How often [`follow_browsers`] looks for browsers that sat unused.
const LOOK_EVERY: Duration = Duration::from_secs(30);

/// How long a start waits for another Swarmed that is starting the same task's Chrome.
const START_WAIT: Duration = Duration::from_secs(40);

/// How long a Chrome whose connection ended is given to end too before it counts as running.
const ENDING: Duration = Duration::from_secs(2);

/// How often the shown tab's title is read again while its pictures come: Chrome does not say
/// when a page's script changes it.
const TITLE_EVERY: Duration = Duration::from_secs(1);

/// The largest side of a page the pane may ask for, in CSS pixels.
const MAX_SIDE: u32 = 4096;

/// The most minutes the settings may leave an unused browser running.
pub(crate) const MAX_IDLE_MINUTES: u32 = 24 * 60;

/// What the pane shows of a task's Chrome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowserState {
    /// The task it belongs to.
    pub task_id: i64,
    /// Whether it runs.
    pub running: bool,
    /// Which browser it is, such as "Google Chrome", once it runs here.
    pub name: Option<String>,
    /// Where its DevTools endpoint answers, as `http://127.0.0.1:<port>`, for tools that take
    /// Chrome's address.
    pub endpoint: Option<String>,
    /// The WebSocket of the browser as a whole, for tools that take that instead.
    pub websocket: Option<String>,
    /// Its tabs, in the order they were opened.
    pub tabs: Vec<BrowserTab>,
    /// The tab the pane shows.
    pub shown: Option<String>,
    /// Where what its pages download goes.
    pub downloads: PathBuf,
}

/// What front ends follow of the tasks' browsers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserEvent {
    /// A picture of the tab the pane shows.
    Frame {
        /// The task whose browser it is.
        task_id: i64,
        /// The tab.
        tab: String,
        /// A JPEG, in base64.
        data: String,
        /// The page's size, in CSS pixels.
        width: u32,
        /// The page's size, in CSS pixels.
        height: u32,
    },
    /// What the pane shows changed: a tab, the tab shown, or the browser started or stopped.
    State(BrowserState),
    /// A line a tab logged.
    Console {
        /// The task whose browser it is.
        task_id: i64,
        /// The tab.
        tab: String,
        /// The line.
        line: ConsoleLine,
    },
    /// A session's agent asked for the browser to be shown to the user (`show_browser`): on
    /// this tab, the session's own, if it has one.
    Show {
        /// The task whose browser it is.
        task_id: i64,
        /// The session that asked.
        session_id: i64,
        /// The tab to show.
        tab: Option<String>,
    },
}

/// The room the pane gives a page.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    /// In CSS pixels.
    pub width: u32,
    /// In CSS pixels.
    pub height: u32,
    /// How many of the screen's pixels make one CSS pixel: 2 on a Retina display.
    pub scale: f64,
}

impl Viewport {
    fn within_limits(self) -> Self {
        let scale = if self.scale.is_finite() {
            self.scale.clamp(1.0, 3.0)
        } else {
            1.0
        };
        Self {
            width: self.width.clamp(100, MAX_SIDE),
            height: self.height.clamp(100, MAX_SIDE),
            scale,
        }
    }

    /// A side in the screen's pixels, which pictures are asked for at.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a side and a scale within their limits give a few thousand pixels"
    )]
    fn pixels(self, side: u32) -> u32 {
        (f64::from(side) * self.scale).round() as u32
    }
}

/// Where a tab is to go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserGo {
    /// To what was typed into the address bar.
    Address(String),
    /// Back a page.
    Back,
    /// Forward a page.
    Forward,
    /// Load the page again.
    Reload,
    /// Stop loading.
    Stop,
}

/// The tasks' browsers this process is connected to, and the channel what they show goes on.
#[derive(Debug)]
pub(crate) struct Browsers {
    connected: Mutex<HashMap<i64, Arc<Browser>>>,
    /// One start at a time per task, in this process.
    gates: Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
    bus: broadcast::Sender<BrowserEvent>,
    /// The sessions' addresses this process listens at, by session.
    addresses: Mutex<HashMap<i64, address::Address>>,
    /// The core a connection to an address starts a browser with, once a front end serves
    /// agents' tools; until then sessions get no address.
    home: OnceLock<Weak<Core>>,
}

impl Default for Browsers {
    fn default() -> Self {
        Self {
            connected: Mutex::default(),
            gates: Mutex::default(),
            bus: broadcast::Sender::new(BUFFER),
            addresses: Mutex::default(),
            home: OnceLock::new(),
        }
    }
}

impl Browsers {
    /// The task's browser, while the connection to it stands.
    fn get(&self, task: i64) -> Option<Arc<Browser>> {
        let connected = lock(&self.connected);
        connected
            .get(&task)
            .filter(|browser| browser.cdp.is_open())
            .cloned()
    }

    fn gate(&self, task: i64) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(lock(&self.gates).entry(task).or_default())
    }

    fn take(&self, task: i64) -> Option<Arc<Browser>> {
        lock(&self.connected).remove(&task)
    }

    /// Forgets `browser` if it is still the one known for its task.
    fn forget(&self, browser: &Arc<Browser>) {
        let mut connected = lock(&self.connected);
        if connected
            .get(&browser.task)
            .is_some_and(|known| Arc::ptr_eq(known, browser))
        {
            connected.remove(&browser.task);
        }
    }

    fn publish(&self, event: BrowserEvent) {
        // An error only means nobody is subscribed right now.
        self.bus.send(event).ok();
    }
}

/// A task's Chrome this process is connected to.
#[derive(Debug)]
struct Browser {
    task: i64,
    pid: u32,
    /// The process, if this Swarmed started it.
    child: Mutex<Option<std::process::Child>>,
    name: String,
    port: u16,
    path: String,
    downloads: PathBuf,
    cdp: Arc<Cdp>,
    pane: Mutex<Pane>,
    /// When it was last used: by the user, or by a page navigating.
    used: Mutex<Instant>,
    /// The room the pane gives pages, once it has said.
    viewport: Mutex<Option<Viewport>>,
    /// When the next picture may be asked for.
    next_frame: Mutex<Instant>,
    /// When the shown tab's title was last read again.
    title_read: Mutex<Instant>,
    /// The tab a session's tools last acted in, which the pane on screen goes to.
    followed: Mutex<Option<String>>,
    /// A pick under way in the tab shown, waiting for the user's click (`pick.rs`).
    picking: Mutex<Option<pick::Picking>>,
}

/// A Chrome that runs, as it is connected to.
struct Found {
    pid: u32,
    child: Option<std::process::Child>,
    name: String,
    port: u16,
    path: String,
    downloads: PathBuf,
}

impl Browser {
    fn new(task: i64, found: Found, cdp: Arc<Cdp>) -> Self {
        Self {
            task,
            pid: found.pid,
            child: Mutex::new(found.child),
            name: found.name,
            port: found.port,
            path: found.path,
            downloads: found.downloads,
            cdp,
            pane: Mutex::default(),
            used: Mutex::new(Instant::now()),
            viewport: Mutex::default(),
            next_frame: Mutex::new(Instant::now()),
            title_read: Mutex::new(Instant::now()),
            followed: Mutex::default(),
            picking: Mutex::default(),
        }
    }

    fn state(&self) -> BrowserState {
        let pane = lock(&self.pane);
        BrowserState {
            task_id: self.task,
            running: self.cdp.is_open(),
            name: Some(self.name.clone()),
            endpoint: Some(format!("http://127.0.0.1:{}", self.port)),
            websocket: Some(format!("ws://127.0.0.1:{}{}", self.port, self.path)),
            tabs: pane.tabs(),
            shown: pane.shown().map(String::from),
            downloads: self.downloads.clone(),
        }
    }

    fn touch(&self) {
        *lock(&self.used) = Instant::now();
    }

    fn idle_for(&self) -> Duration {
        lock(&self.used).elapsed()
    }

    fn shown(&self) -> Option<String> {
        lock(&self.pane).shown().map(String::from)
    }

    /// Shows `tab`: when `None`, the one shown, else the one shown when the pane was hidden,
    /// else the first. Returns the tab shown before and the one shown now; `None` if there is
    /// no such tab.
    fn shows(&self, tab: Option<&str>) -> Option<(Option<String>, Option<String>)> {
        let mut pane = lock(&self.pane);
        let before = pane.shown().map(String::from);
        let left = pane.left().map(String::from);
        let first = pane.tabs().first().map(|tab| tab.id.clone());
        let wanted = tab
            .map(String::from)
            .or_else(|| before.clone())
            .or(left)
            .or(first);
        pane.show(wanted.as_deref()).then_some((before, wanted))
    }

    fn session_of(&self, tab: &str) -> Option<String> {
        lock(&self.pane).session_of(tab)
    }

    /// Asks the tab for pictures, at the size the pane gives it. Nothing yet for a tab not
    /// attached to: attaching asks once it is.
    async fn cast(&self, tab: &str) -> std::result::Result<(), String> {
        let (Some(session), Some(view)) = (self.session_of(tab), *lock(&self.viewport)) else {
            return Ok(());
        };
        let metrics = json!({
            "width": view.width,
            "height": view.height,
            "deviceScaleFactor": view.scale,
            "mobile": false,
        });
        let on = Some(session.as_str());
        self.cdp
            .call(on, "Emulation.setDeviceMetricsOverride", metrics)
            .await?;
        self.cdp.call(on, "Page.bringToFront", json!({})).await?;
        let pictures = json!({
            "format": "jpeg",
            "quality": QUALITY,
            "maxWidth": view.pixels(view.width),
            "maxHeight": view.pixels(view.height),
            "everyNthFrame": 1,
        });
        self.cdp.call(on, "Page.startScreencast", pictures).await?;
        Ok(())
    }

    /// Stops the tab's pictures.
    async fn uncast(&self, tab: &str) {
        if let Some(session) = self.session_of(tab) {
            let stop = self
                .cdp
                .call(Some(&session), "Page.stopScreencast", json!({}));
            stop.await.ok();
        }
    }

    /// Publishes a picture if its tab is the one shown, and asks for the next one once the
    /// pace allows.
    fn framed(self: &Arc<Self>, browsers: &Arc<Browsers>, frame: Frame) {
        let shown = self.shown().as_deref() == Some(frame.tab.as_str());
        let wait = {
            let mut next = lock(&self.next_frame);
            let now = Instant::now();
            let wait = next.saturating_duration_since(now);
            *next = now + wait + Duration::from_secs(1) / FRAMES_A_SECOND;
            wait
        };
        let (session, ack) = (frame.session, frame.ack);
        if shown && self.title_due() {
            let read = read_info(Arc::clone(self), Arc::clone(browsers), frame.tab.clone());
            drop(tokio::spawn(read));
        }
        if shown {
            browsers.publish(BrowserEvent::Frame {
                task_id: self.task,
                tab: frame.tab,
                data: frame.data,
                width: frame.width,
                height: frame.height,
            });
        }
        let browser = Arc::clone(self);
        drop(tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            let acked = json!({ "sessionId": ack });
            browser
                .cdp
                .send(Some(&session), "Page.screencastFrameAck", acked);
        }));
    }
}

impl Browser {
    /// Session `session`'s tools acted in `tab`: the tab says so, and the pane, while it is on
    /// screen, goes to it if another tab was acted in before. A tab the user went to since is
    /// left shown while the tools go on acting in the same one.
    fn acted(self: &Arc<Self>, browsers: &Arc<Browsers>, tab: &str, session: i64) {
        self.touch();
        if lock(&self.pane).acted(tab, session) {
            browsers.publish(BrowserEvent::State(self.state()));
        }
        let moved = {
            let mut followed = lock(&self.followed);
            let moved = followed.as_deref() != Some(tab);
            *followed = Some(tab.to_string());
            moved
        };
        if moved {
            self.follow(browsers, tab);
        }
    }

    /// Shows `tab` in the pane, if the pane is on screen on another tab and Chrome has told
    /// of this one; a tab told of later is gone to as it is attached to.
    fn follow(self: &Arc<Self>, browsers: &Arc<Browsers>, tab: &str) {
        let shown = self.shown();
        let has = lock(&self.pane).has(tab);
        if shown.is_none() || shown.as_deref() == Some(tab) || !has {
            return;
        }
        let going = go_to(Arc::clone(self), Arc::clone(browsers), tab.to_string());
        drop(tokio::spawn(going));
    }

    /// Whether the shown tab's title is to be read again now, which it then counts as.
    fn title_due(&self) -> bool {
        let mut read = lock(&self.title_read);
        let due = read.elapsed() >= TITLE_EVERY;
        if due {
            *read = Instant::now();
        }
        due
    }
}

/// Shows `tab` in the pane in place of the tab shown, and says so.
async fn go_to(browser: Arc<Browser>, browsers: Arc<Browsers>, tab: String) {
    let Some((before, now)) = browser.shows(Some(&tab)) else {
        return;
    };
    if let Some(before) = before.filter(|before| Some(before) != now.as_ref()) {
        // A pick goes with the tab it was started in.
        browser.end_pick();
        browser.uncast(&before).await;
    }
    browser.cast(&tab).await.ok();
    browsers.publish(BrowserEvent::State(browser.state()));
}

/// Reads a tab's title and address again, as if Chrome had said they changed.
async fn read_info(browser: Arc<Browser>, browsers: Arc<Browsers>, tab: String) {
    let asked = json!({ "targetId": tab });
    let Ok(answer) = browser.cdp.call(None, "Target.getTargetInfo", asked).await else {
        return;
    };
    let changed = Event {
        session: None,
        method: "Target.targetInfoChanged".to_string(),
        params: json!({ "targetInfo": answer["targetInfo"] }),
    };
    take_in(&browser, &browsers, &changed);
}

/// What the events of one Chrome reach beyond it.
struct Ends {
    browsers: Arc<Browsers>,
    live: Arc<Live>,
    record: PathBuf,
}

/// Takes in what Chrome says, until the connection ends.
async fn pump(browser: Arc<Browser>, mut events: mpsc::UnboundedReceiver<Event>, ends: Ends) {
    while let Some(event) = events.recv().await {
        take_in(&browser, &ends.browsers, &event);
    }
    gone(&browser, &ends).await;
}

/// Takes in one event: the pane follows it, and what it changed is acted on.
fn take_in(browser: &Arc<Browser>, browsers: &Arc<Browsers>, event: &Event) {
    let changes = lock(&browser.pane).apply(event);
    for change in changes {
        act(browser, browsers, change);
    }
}

fn act(browser: &Arc<Browser>, browsers: &Arc<Browsers>, change: Change) {
    match change {
        Change::Tabs => {
            browser.pick_tab_gone();
            browsers.publish(BrowserEvent::State(browser.state()));
        }
        Change::Attach(tab) => {
            let attaching = attach(Arc::clone(browser), Arc::clone(browsers), tab);
            drop(tokio::spawn(attaching));
        }
        Change::History(tab) => {
            let (browser, browsers) = (Arc::clone(browser), Arc::clone(browsers));
            drop(tokio::spawn(async move {
                history(&browser, &browsers, &tab).await;
            }));
        }
        Change::Console(tab, line) => browsers.publish(BrowserEvent::Console {
            task_id: browser.task,
            tab,
            line,
        }),
        Change::Frame(frame) => browser.framed(browsers, frame),
        Change::Inspected(tab, node) => browser.inspected(&tab, node),
        Change::Used => browser.touch(),
    }
}

/// Attaches to a new tab, so that what it logs, how it loads and its pictures are heard.
async fn attach(browser: Arc<Browser>, browsers: Arc<Browsers>, tab: String) {
    let asked = json!({ "targetId": tab, "flatten": true });
    let Ok(answer) = browser.cdp.call(None, "Target.attachToTarget", asked).await else {
        return;
    };
    let Some(session) = answer["sessionId"].as_str() else {
        return;
    };
    lock(&browser.pane).attached(&tab, session);
    for domain in ["Page.enable", "Runtime.enable", "Log.enable"] {
        browser
            .cdp
            .call(Some(session), domain, json!({}))
            .await
            .ok();
    }
    history(&browser, &browsers, &tab).await;
    if browser.shown().as_deref() == Some(tab.as_str()) {
        browser.cast(&tab).await.ok();
    } else if lock(&browser.followed).as_deref() == Some(tab.as_str()) {
        // A session's tools acted in it before Chrome told of it.
        browser.follow(&browsers, &tab);
    }
}

/// Reads again where the tab's history stands, for back and forward.
async fn history(browser: &Browser, browsers: &Browsers, tab: &str) {
    let Some(session) = browser.session_of(tab) else {
        return;
    };
    let read = browser
        .cdp
        .call(Some(&session), "Page.getNavigationHistory", json!({}));
    let Ok(answer) = read.await else {
        return;
    };
    let current = answer["currentIndex"].as_u64().unwrap_or_default();
    let entries = answer["entries"].as_array().map_or(0, Vec::len) as u64;
    if lock(&browser.pane).history(tab, current, entries) {
        browsers.publish(BrowserEvent::State(browser.state()));
    }
}

/// The connection ended: the browser is forgotten if Chrome ended with it, and every front end
/// is told.
async fn gone(browser: &Arc<Browser>, ends: &Ends) {
    browser.pick_forget();
    let child = lock(&browser.child).take();
    let ended = process::ended_within(browser.pid, child, ENDING).await;
    if let Some(child) = ended {
        // Still running: kept, so that it is stopped with the others.
        *lock(&browser.child) = child;
    } else {
        ends.browsers.forget(browser);
        std::fs::remove_file(&ends.record).ok();
    }
    let mut state = browser.state();
    state.running = false;
    state.tabs.clear();
    state.shown = None;
    ends.browsers.publish(BrowserEvent::State(state));
    ends.live.publish_apart(AgentEvent::TreeChanged);
}

/// Goes to `typed`. Chrome's words when it cannot, such as `net::ERR_CONNECTION_REFUSED`.
async fn navigate(cdp: &Cdp, session: &str, typed: &str) -> std::result::Result<Value, String> {
    let url = json!({ "url": input::address_of(typed) });
    let answer = cdp.call(Some(session), "Page.navigate", url).await?;
    match answer["errorText"].as_str() {
        Some(error) if !error.is_empty() => Err(error.to_string()),
        _ => Ok(answer),
    }
}

/// Goes `by` pages back or forward in the tab's history.
async fn step(cdp: &Cdp, session: &str, by: i64) -> std::result::Result<Value, String> {
    let history = cdp
        .call(Some(session), "Page.getNavigationHistory", json!({}))
        .await?;
    let current = history["currentIndex"].as_i64().unwrap_or_default();
    let at = usize::try_from(current + by).map_err(|_| "there is no page there".to_string())?;
    let entry = history["entries"][at]["id"]
        .as_i64()
        .ok_or_else(|| "there is no page there".to_string())?;
    let to = json!({ "entryId": entry });
    cdp.call(Some(session), "Page.navigateToHistoryEntry", to)
        .await
}

impl Core {
    /// The browser a task's Chrome runs on, or what would install one.
    pub fn browser_setup(&self) -> Result<BrowserSetup> {
        let chosen = self.settings()?.browser_program;
        let found = find::find(&chosen, &find::app_folders(), &self.tool_dirs);
        let (name, path) = found.unzip();
        Ok(BrowserSetup {
            name,
            path,
            install_command: find::install_shown(&self.tool_dirs),
            chosen: !chosen.trim().is_empty(),
        })
    }

    /// Installs Google Chrome, forwarding what the installer prints to `sink`. The command is
    /// [`BrowserSetup::install_command`], which a front end shows the user first.
    pub async fn browser_install(&self, sink: OutputSink<'_>) -> Result<BrowserSetup> {
        let failed = |reason: String| Error::Install {
            agent: "a browser".to_string(),
            reason,
        };
        let command = find::install_command(&self.tool_dirs).ok_or_else(|| {
            failed(
                "Homebrew was not found: install Google Chrome, Chromium, Microsoft Edge or \
                 Brave yourself, then come back"
                    .to_string(),
            )
        })?;
        hooks::run("install", &command, &std::env::temp_dir(), &[], sink)
            .await
            .map_err(|error| failed(error.to_string()))?;
        let setup = self.browser_setup()?;
        if setup.name.is_none() {
            return Err(failed(
                "the installer finished, but no browser is to be found".to_string(),
            ));
        }
        Ok(setup)
    }

    /// The task's browser as the pane shows it, without starting it.
    pub fn browser_state(&self, task_id: i64) -> Result<BrowserState> {
        let task = self.browser_task(task_id)?;
        Ok(match self.browsers.get(task.id) {
            Some(browser) => browser.state(),
            None => self.stopped_state(task.id),
        })
    }

    /// Starts the task's Chrome, or connects to the one that runs, and says how it stands.
    pub async fn browser_open(&self, task_id: i64) -> Result<BrowserState> {
        let browser = self.browser_of(task_id).await?;
        browser.touch();
        Ok(browser.state())
    }

    /// Stops the task's Chrome, wherever it was started. Its profile stays.
    pub async fn browser_stop(&self, task_id: i64) -> Result<()> {
        match self.stop_browser(task_id).await {
            Some(note) => Err(self.browser_error(task_id, note)),
            None => Ok(()),
        }
    }

    /// Shows `tab` in the pane (when `None`: the one shown, else the one it was hidden on,
    /// else the first), at the size of `viewport`: its pictures are asked for from now on, and
    /// another tab's stop. Starts the task's Chrome if it does not run.
    pub async fn browser_show(
        &self,
        task_id: i64,
        tab: Option<&str>,
        viewport: Viewport,
    ) -> Result<BrowserState> {
        let browser = self.browser_of(task_id).await?;
        browser.touch();
        *lock(&browser.viewport) = Some(viewport.within_limits());
        let (before, now) = browser
            .shows(tab)
            .ok_or_else(|| self.browser_error(task_id, "it has no such tab".to_string()))?;
        if let Some(before) = before.filter(|before| Some(before) != now.as_ref()) {
            browser.end_pick();
            browser.uncast(&before).await;
        }
        if let Some(now) = &now {
            let cast = browser.cast(now).await;
            cast.map_err(|reason| self.browser_error(task_id, reason))?;
        }
        let state = browser.state();
        self.browsers.publish(BrowserEvent::State(state.clone()));
        Ok(state)
    }

    /// The pane is off screen: no more pictures are asked for.
    pub async fn browser_hide(&self, task_id: i64) -> Result<()> {
        let Some(browser) = self.browsers.get(task_id) else {
            return Ok(());
        };
        browser.end_pick();
        let shown = browser.shown();
        lock(&browser.pane).show(None);
        if let Some(tab) = shown {
            browser.uncast(&tab).await;
        }
        Ok(())
    }

    /// Passes what the mouse did on the picture on to the tab shown. Sent in the order front
    /// ends call this, without waiting for Chrome.
    pub fn browser_mouse(&self, task_id: i64, input: &MouseInput) -> Result<()> {
        self.to_shown(task_id, "Input.dispatchMouseEvent", input::mouse(input))
    }

    /// Passes a key on to the tab shown, as [`Core::browser_mouse`] does a click.
    pub fn browser_key(&self, task_id: i64, input: &KeyInput) -> Result<()> {
        self.to_shown(task_id, "Input.dispatchKeyEvent", input::key(input))
    }

    /// Types `text` into the tab shown, where its focus is: what the user pasted, or what an
    /// input method composed.
    pub fn browser_text(&self, task_id: i64, text: &str) -> Result<()> {
        self.to_shown(task_id, "Input.insertText", json!({ "text": text }))
    }

    fn to_shown(&self, task_id: i64, method: &str, params: Value) -> Result<()> {
        let browser = self.connected_browser(task_id)?;
        let session = browser.shown().and_then(|tab| browser.session_of(&tab));
        let session =
            session.ok_or_else(|| self.browser_error(task_id, "it shows no tab".to_string()))?;
        browser.touch();
        browser.cdp.send(Some(&session), method, params);
        Ok(())
    }

    /// Sends `tab` somewhere: to an address, back, forward, again, or stops it loading.
    pub async fn browser_go(&self, task_id: i64, tab: &str, go: &BrowserGo) -> Result<()> {
        let browser = self.connected_browser(task_id)?;
        browser.touch();
        let session = self.tab_session(&browser, tab)?;
        let (cdp, on) = (&browser.cdp, Some(session.as_str()));
        let done = match go {
            BrowserGo::Address(typed) => navigate(cdp, &session, typed).await,
            BrowserGo::Back => step(cdp, &session, -1).await,
            BrowserGo::Forward => step(cdp, &session, 1).await,
            BrowserGo::Reload => cdp.call(on, "Page.reload", json!({})).await,
            BrowserGo::Stop => cdp.call(on, "Page.stopLoading", json!({})).await,
        };
        done.map(drop)
            .map_err(|reason| self.browser_error(task_id, reason))
    }

    /// Opens a tab on `url`, or an empty one, starting the task's Chrome if it does not run.
    /// Returns the tab's id.
    pub async fn browser_tab_new(&self, task_id: i64, url: Option<&str>) -> Result<String> {
        let browser = self.browser_of(task_id).await?;
        browser.touch();
        let url = url.map_or_else(|| "about:blank".to_string(), input::address_of);
        let created = browser
            .cdp
            .call(None, "Target.createTarget", json!({ "url": url }))
            .await
            .map_err(|reason| self.browser_error(task_id, reason))?;
        let tab = created["targetId"].as_str().unwrap_or_default().to_string();
        Ok(tab)
    }

    /// Closes a tab.
    pub async fn browser_tab_close(&self, task_id: i64, tab: &str) -> Result<()> {
        let browser = self.connected_browser(task_id)?;
        browser.touch();
        let close = json!({ "targetId": tab });
        browser
            .cdp
            .call(None, "Target.closeTarget", close)
            .await
            .map(drop)
            .map_err(|reason| self.browser_error(task_id, reason))
    }

    /// Answers the dialog a tab opened: accepted or not, with `text` for a `prompt`.
    pub async fn browser_dialog(
        &self,
        task_id: i64,
        tab: &str,
        accept: bool,
        text: Option<&str>,
    ) -> Result<()> {
        let browser = self.connected_browser(task_id)?;
        browser.touch();
        let session = self.tab_session(&browser, tab)?;
        let mut answer = json!({ "accept": accept });
        if let Some(text) = text {
            answer["promptText"] = json!(text);
        }
        let answered = browser
            .cdp
            .call(Some(&session), "Page.handleJavaScriptDialog", answer);
        answered
            .await
            .map(drop)
            .map_err(|reason| self.browser_error(task_id, reason))
    }

    /// What a tab logged that is kept, oldest first. None while the browser does not run here.
    pub fn browser_console(&self, task_id: i64, tab: &str) -> Result<Vec<ConsoleLine>> {
        self.browser_task(task_id)?;
        Ok(self
            .browsers
            .get(task_id)
            .map_or_default(|browser| lock(&browser.pane).console(tab)))
    }

    /// The pictures, tabs and console lines of every task's browser, from now on.
    #[must_use]
    pub fn subscribe_browsers(&self) -> broadcast::Receiver<BrowserEvent> {
        self.browsers.bus.subscribe()
    }

    /// Whether the task's Chrome runs: one this process is connected to, or one written down
    /// that is still that process.
    pub(crate) fn browser_running(&self, task: i64) -> bool {
        self.browsers.get(task).is_some() || process::kept(&self.browser_record(task)).is_some()
    }

    /// Stops the task's Chrome and deletes its profile, for a task that goes. Returns what did
    /// not work, to be said.
    pub(crate) async fn browser_forget(&self, task: i64) -> Vec<String> {
        lock(&self.browsers.addresses).retain(|_, address| address.task != task);
        let mut notes: Vec<String> = self.stop_browser(task).await.into_iter().collect();
        let folder = self.paths.browser(task);
        if let Err(error) = std::fs::remove_dir_all(&folder)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            notes.push(format!(
                "its browser's profile could not be deleted: {error}"
            ));
        }
        notes
    }

    /// Stops the task's Chrome, wherever it was started: asked to close if this process is
    /// connected to it, else ended as a stack is. Says why not if it would not end.
    pub(crate) async fn stop_browser(&self, task: i64) -> Option<String> {
        let record = self.browser_record(task);
        let (pid, child, asked) = match self.browsers.take(task) {
            Some(browser) => {
                browser.cdp.send(None, "Browser.close", json!({}));
                (browser.pid, lock(&browser.child).take(), true)
            }
            None => (process::kept(&record)?, None, false),
        };
        let gone = process::end(pid, child, asked).await;
        if gone {
            std::fs::remove_file(&record).ok();
        }
        self.browsers
            .publish(BrowserEvent::State(self.stopped_state(task)));
        self.live.publish_apart(AgentEvent::TreeChanged);
        (!gone).then(|| format!("its browser (process {pid}) would not end, and is left running"))
    }

    /// Stops the browsers this process started, and no others.
    pub async fn browsers_stop_started(&self) {
        let started: Vec<i64> = lock(&self.browsers.connected)
            .values()
            .filter(|browser| lock(&browser.child).is_some())
            .map(|browser| browser.task)
            .collect();
        for task in started {
            self.stop_browser(task).await;
        }
    }

    /// Stops every task's browser, for when the first Swarmed to arrive quits: those it is
    /// connected to, and those written down by any Swarmed.
    pub async fn browsers_stop_all(&self) {
        let mut tasks: Vec<i64> = lock(&self.browsers.connected).keys().copied().collect();
        tasks.extend(self.kept_browsers());
        tasks.sort_unstable();
        tasks.dedup();
        for task in tasks {
            self.stop_browser(task).await;
        }
    }

    /// Looks after the browsers, for the Swarmed that keeps house: one another Swarmed left
    /// running is connected to, so that its time unused counts from now; one whose task is
    /// gone is stopped; and one that sat unused longer than the settings allow is stopped,
    /// unless the pane shows it.
    pub async fn browsers_look(&self) -> Result<()> {
        if lock(&self.house).is_none() {
            return Ok(());
        }
        let found: Vec<i64> = self.kept_browsers();
        for task in found
            .into_iter()
            .filter(|task| self.browsers.get(*task).is_none())
        {
            self.adopt_browser(task).await;
        }
        let minutes = self.settings()?.browser_idle_minutes;
        if minutes == 0 {
            return Ok(());
        }
        let limit = Duration::from_secs(u64::from(minutes) * 60);
        let unused: Vec<i64> = lock(&self.browsers.connected)
            .values()
            .filter(|browser| browser.shown().is_none() && browser.idle_for() >= limit)
            .map(|browser| browser.task)
            .collect();
        for task in unused {
            self.stop_browser(task).await;
        }
        Ok(())
    }

    /// Connects to a browser another Swarmed left running, so that its time unused counts from
    /// now; stops it if its task is gone.
    async fn adopt_browser(&self, task: i64) {
        if self.browser_task(task).is_ok() {
            self.browser_at(task, Absent::Left).await.ok();
        } else {
            self.stop_browser(task).await;
        }
    }

    /// The task's browser, started or connected to if it is not yet.
    async fn browser_of(&self, task_id: i64) -> Result<Arc<Browser>> {
        self.browser_at(task_id, Absent::Started)
            .await?
            .ok_or_else(|| self.browser_error(task_id, "it does not run".to_string()))
    }

    /// The task's browser, connected to if it runs. One that does not run, or cannot be
    /// reached, is started only when `absent` says so: looking after the browsers must start
    /// none, and what a Swarmed that crashed wrote down is of a Chrome that went with it.
    async fn browser_at(&self, task_id: i64, absent: Absent) -> Result<Option<Arc<Browser>>> {
        if let Some(browser) = self.browsers.get(task_id) {
            return Ok(Some(browser));
        }
        let task = self.browser_task(task_id)?;
        let gate = self.browsers.gate(task.id);
        let _one_at_a_time = gate.lock().await;
        if let Some(browser) = self.browsers.get(task.id) {
            return Ok(Some(browser));
        }
        // Another Swarmed may be starting it too: one waits for the other, then connects.
        let record = self.browser_record(task.id);
        let _starting = start_lock(&record).await?.ok_or_else(|| {
            self.browser_error(task.id, "another Swarmed is still starting it".to_string())
        })?;
        let browser = match (process::kept(&record), absent) {
            (Some(pid), Absent::Started) => self.reconnect_or_replace(&task, pid).await?,
            (Some(pid), Absent::Left) => self.reconnect(&task, pid).await?,
            (None, Absent::Started) => self.launch(&task).await?,
            (None, Absent::Left) => return Ok(None),
        };
        self.live.publish_apart(AgentEvent::TreeChanged);
        Ok(Some(browser))
    }

    /// Connects to the task's Chrome that runs; one that cannot be reached is no use to anyone,
    /// and a new one takes its place.
    async fn reconnect_or_replace(&self, task: &Task, pid: u32) -> Result<Arc<Browser>> {
        if let Ok(browser) = self.reconnect(task, pid).await {
            return Ok(browser);
        }
        process::end(pid, None, false).await;
        self.launch(task).await
    }

    /// Starts the task's Chrome.
    async fn launch(&self, task: &Task) -> Result<Arc<Browser>> {
        let chosen = self.settings()?.browser_program;
        let (name, program) = find::find(&chosen, &find::app_folders(), &self.tool_dirs)
            .ok_or(Error::BrowserMissing)?;
        let folder = self.paths.browser(task.id);
        let (profile, downloads) = (folder.join("profile"), folder.join("downloads"));
        crate::pasted::private_folder(&profile)?;
        crate::pasted::private_folder(&downloads)?;
        let page = self.start_page(task);
        let output = folder.join(process::OUTPUT);
        let record = self.browser_record(task.id);
        let started = process::start(&program, &profile, &output, page.as_deref(), &record)
            .await
            .map_err(|reason| self.browser_error(task.id, reason))?;
        let pid = started.child.id();
        let found = Found {
            pid,
            child: Some(started.child),
            name,
            port: started.port,
            path: started.path,
            downloads,
        };
        // A task that was put away is worked on again.
        self.bring_back(task.id)?;
        self.connect(task, found).await
    }

    /// Connects to the task's Chrome that another Swarmed, or an earlier one, started.
    async fn reconnect(&self, task: &Task, pid: u32) -> Result<Arc<Browser>> {
        let folder = self.paths.browser(task.id);
        let (port, path) = process::endpoint_in(&folder.join("profile")).ok_or_else(|| {
            self.browser_error(task.id, "it runs, but did not say where".to_string())
        })?;
        let chosen = self.settings()?.browser_program;
        let found = find::find(&chosen, &find::app_folders(), &self.tool_dirs);
        let found = Found {
            pid,
            child: None,
            name: found.map_or_else(|| "Chrome".to_string(), |(name, _)| name),
            port,
            path,
            downloads: folder.join("downloads"),
        };
        self.connect(task, found).await
    }

    /// Connects to a Chrome that runs, and has it say what it has.
    async fn connect(&self, task: &Task, found: Found) -> Result<Arc<Browser>> {
        let connected = Cdp::connect(found.port, &found.path).await;
        let (cdp, events) = connected.map_err(|error| {
            self.browser_error(task.id, format!("it could not be reached: {error}"))
        })?;
        let browser = Arc::new(Browser::new(task.id, found, cdp));
        lock(&self.browsers.connected).insert(task.id, Arc::clone(&browser));
        let ends = Ends {
            browsers: Arc::clone(&self.browsers),
            live: Arc::clone(&self.live),
            record: self.browser_record(task.id),
        };
        drop(tokio::spawn(pump(Arc::clone(&browser), events, ends)));
        if let Err(reason) = self.set_up(&browser).await {
            browser.cdp.close();
            self.browsers.forget(&browser);
            return Err(self.browser_error(task.id, reason));
        }
        self.browsers.publish(BrowserEvent::State(browser.state()));
        Ok(browser)
    }

    /// Has a Chrome just connected to download into the task's folder, tell of its pages from
    /// now on, and say which it has now.
    async fn set_up(&self, browser: &Arc<Browser>) -> std::result::Result<(), String> {
        let downloads = json!({
            "behavior": "allow",
            "downloadPath": browser.downloads,
            "eventsEnabled": false,
        });
        let cdp = &browser.cdp;
        cdp.call(None, "Browser.setDownloadBehavior", downloads)
            .await?;
        cdp.call(
            None,
            "Target.setDiscoverTargets",
            json!({ "discover": true }),
        )
        .await?;
        // Taken in at once, rather than when the events about them come: whoever opened the
        // browser goes on to show one of them.
        let targets = cdp.call(None, "Target.getTargets", json!({})).await?;
        for info in targets["targetInfos"].as_array().into_iter().flatten() {
            let created = Event {
                session: None,
                method: "Target.targetCreated".to_string(),
                params: json!({ "targetInfo": info }),
            };
            take_in(browser, &self.browsers, &created);
        }
        Ok(())
    }

    /// The address a new Chrome of the task opens on: its stack's, if that answers.
    fn start_page(&self, task: &Task) -> Option<String> {
        let project = self.store().project_by_id(task.project_id).ok()??;
        if project.chats {
            return None;
        }
        let config = self.config_in(&project.name, task.environment.as_deref());
        let reach = self.reach(&project, &config.ok()?, task);
        let answers = reach.state == Some(StackState::Running);
        answers.then(|| reach.urls.into_iter().next()).flatten()
    }

    fn connected_browser(&self, task_id: i64) -> Result<Arc<Browser>> {
        self.browsers
            .get(task_id)
            .ok_or_else(|| Error::BrowserNotRunning(self.task_name(task_id)))
    }

    fn tab_session(&self, browser: &Browser, tab: &str) -> Result<String> {
        browser
            .session_of(tab)
            .ok_or_else(|| self.browser_error(browser.task, format!("it has no tab {tab}")))
    }

    /// A task that is not removed, by its id.
    fn browser_task(&self, task_id: i64) -> Result<Task> {
        self.store()
            .task_by_id(task_id)?
            .filter(|task| !task.archived)
            .ok_or_else(|| Error::Browser {
                task: format!("task {task_id}"),
                reason: "there is no such task".to_string(),
            })
    }

    fn stopped_state(&self, task: i64) -> BrowserState {
        BrowserState {
            task_id: task,
            running: self.browser_running(task),
            name: None,
            endpoint: None,
            websocket: None,
            tabs: vec![],
            shown: None,
            downloads: self.paths.browser(task).join("downloads"),
        }
    }

    fn browser_error(&self, task: i64, reason: String) -> Error {
        Error::Browser {
            task: self.task_name(task),
            reason,
        }
    }

    /// The task's name, for what is said about it; its id where it has none.
    fn task_name(&self, task: i64) -> String {
        match self.store().task_by_id(task) {
            Ok(Some(found)) => found.name,
            _ => format!("task {task}"),
        }
    }

    fn browser_record(&self, task: i64) -> PathBuf {
        records(&self.paths.data_dir).join(format!("{task}.pid"))
    }

    /// The tasks a browser is written down for.
    fn kept_browsers(&self) -> Vec<i64> {
        let Ok(entries) = std::fs::read_dir(records(&self.paths.data_dir)) else {
            return vec![];
        };
        let task = |entry: std::fs::DirEntry| {
            let name = entry.file_name();
            name.to_string_lossy().strip_suffix(".pid")?.parse().ok()
        };
        entries.flatten().filter_map(task).collect()
    }
}

/// What to do about a task's browser that does not run.
#[derive(Clone, Copy)]
enum Absent {
    /// Start one: somebody asked for the browser.
    Started,
    /// Leave it so: nobody asked.
    Left,
}

/// Waits until no other Swarmed is starting the task's Chrome, whose record is `record`, and
/// holds that place; `None` if the other took too long.
async fn start_lock(record: &Path) -> std::io::Result<Option<File>> {
    if let Some(folder) = record.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let file = File::create(record.with_extension("lock"))?;
    let until = Instant::now() + START_WAIT;
    loop {
        if holds(&file)? {
            return Ok(Some(file));
        }
        if Instant::now() >= until {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Whether this process holds the lock on `file` now, taking it if it is free.
fn holds(file: &File) -> std::io::Result<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(TryLockError::WouldBlock) => Ok(false),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// Where the record of each running browser is, in the data directory.
pub(crate) fn records(data_dir: &Path) -> PathBuf {
    data_dir.join("run").join("browsers")
}

/// Stops the browsers that sat unused, for as long as the core is there. Only the first
/// Swarmed to arrive on the data does.
pub async fn follow_browsers(core: Weak<Core>) {
    let mut looks = tokio::time::interval(LOOK_EVERY);
    looks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        looks.tick().await;
        let Some(core) = core.upgrade() else {
            return;
        };
        // A look that failed is made again at the next.
        core.browsers_look().await.ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_room_the_pane_asks_for_is_kept_within_limits() {
        let asked = Viewport {
            width: 20,
            height: 90_000,
            scale: f64::NAN,
        };
        let kept = asked.within_limits();
        assert_eq!((kept.width, kept.height, kept.scale), (100, MAX_SIDE, 1.0));
        let retina = Viewport {
            width: 800,
            height: 600,
            scale: 2.0,
        };
        assert_eq!(retina.pixels(retina.width), 1600);
    }
}
