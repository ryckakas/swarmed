//! A task's own Chrome, against a stand-in that speaks the DevTools protocol over a WebSocket
//! (`fixtures/fake_chrome.py`): started on a profile of the task's own, shown as pictures,
//! driven by the user's clicks and keys, found again by the next Swarmed, and stopped with its
//! task.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use common::{core_with_project, fixture};
use swarmed_core::{
    BrowserDisplay, BrowserEvent, BrowserGo, BrowserState, CleanupKind, Core, KeyInput, KeyKind,
    Modifiers, MouseInput, MouseKind, NewTaskOptions, RemoveOptions, Settings, Viewport, hooks,
};
use tokio::sync::broadcast::Receiver;

const VIEW: Viewport = Viewport {
    width: 640,
    height: 480,
    scale: 1.0,
};

/// Ends every browser written down in the fixture's data when dropped: a test that fails
/// before it stops its browser must leave no Chrome running, since Chrome is in a process
/// group of its own and outlives the test.
struct EndsBrowsers(PathBuf);

impl Drop for EndsBrowsers {
    fn drop(&mut self) {
        let Ok(records) = std::fs::read_dir(self.0.join("run/browsers")) else {
            return;
        };
        let pid = |path: PathBuf| {
            std::fs::read_to_string(path)
                .ok()?
                .lines()
                .next()?
                .parse()
                .ok()
        };
        for leader in records.flatten().filter_map(|record| pid(record.path())) {
            let group: u32 = leader;
            let killed = std::process::Command::new("kill")
                .args(["-KILL", "--", &format!("-{group}")])
                .status();
            killed.ok();
        }
    }
}

fn ends_browsers(fx: &common::Fixture) -> EndsBrowsers {
    EndsBrowsers(fx.paths.data_dir.clone())
}

/// The stand-in Chrome, installed where the settings point the browser.
fn chrome(fx: &common::Fixture) -> PathBuf {
    let root = fx.paths.data_dir.parent().expect("has a parent");
    let program = root.join("fake-chrome");
    if !program.exists() {
        common::executable(&program, include_str!("fixtures/fake_chrome.py"));
    }
    program
}

/// A core with task `one`, whose browser runs on the stand-in.
async fn core_with_task(fx: &common::Fixture) -> Core {
    let core = core_with_project(fx).await;
    point_at_chrome(&core, fx);
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core
}

fn point_at_chrome(core: &Core, fx: &common::Fixture) {
    let settings = Settings {
        browser_program: chrome(fx).to_string_lossy().into_owned(),
        ..Settings::default()
    };
    core.set_settings(&settings).expect("test setup");
}

fn task_id(core: &Core) -> i64 {
    core.task("demo", "one").expect("there").1.id
}

fn record(fx: &common::Fixture, task: i64) -> PathBuf {
    fx.paths
        .data_dir
        .join("run/browsers")
        .join(format!("{task}.pid"))
}

/// The process written down for the task's browser.
fn pid(fx: &common::Fixture, task: i64) -> u32 {
    let written = std::fs::read_to_string(record(fx, task)).expect("written down");
    written
        .lines()
        .next()
        .expect("a line")
        .parse()
        .expect("a number")
}

fn alive(pid: u32) -> bool {
    let asked = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps runs");
    let state = String::from_utf8_lossy(&asked.stdout);
    !state.trim().is_empty() && !state.trim().starts_with('Z')
}

/// The next event of the browsers that `wanted` keeps, failing instead of hanging.
async fn heard<T>(
    bus: &mut Receiver<BrowserEvent>,
    wanted: impl Fn(BrowserEvent) -> Option<T>,
) -> T {
    tokio::time::timeout(Duration::from_secs(10), first_kept(bus, wanted))
        .await
        .expect("heard in time")
}

async fn first_kept<T>(
    bus: &mut Receiver<BrowserEvent>,
    wanted: impl Fn(BrowserEvent) -> Option<T>,
) -> T {
    loop {
        let event = bus.recv().await.expect("the channel stays open");
        if let Some(found) = wanted(event) {
            return found;
        }
    }
}

/// The next state published in which `wanted` holds.
async fn state_where(
    bus: &mut Receiver<BrowserEvent>,
    wanted: impl Fn(&BrowserState) -> bool,
) -> BrowserState {
    heard(bus, |event| match event {
        BrowserEvent::State(state) if wanted(&state) => Some(state),
        _ => None,
    })
    .await
}

/// What a picture of the stand-in says: which one it is, and its size.
fn picture(data: &str) -> String {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .expect("base64");
    String::from_utf8(bytes).expect("text")
}

/// What the stand-in was told to do with the mouse and keys, in order.
async fn inputs(profile: &Path, count: usize) -> Vec<serde_json::Value> {
    noted(profile, "fake-input", count).await
}

/// What the stand-in noted under `name`, once it has noted at least `count` things.
async fn noted(profile: &Path, name: &str, count: usize) -> Vec<serde_json::Value> {
    let file = profile.join(name);
    for _ in 0..100 {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if text.lines().count() >= count {
            let line = |line: &str| serde_json::from_str(line).expect("JSON");
            return text.lines().map(line).collect();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the stand-in was not told {count} things");
}

#[tokio::test]
async fn a_tasks_chrome_starts_on_a_profile_of_its_own_and_says_where_it_listens() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    assert!(!core.browser_state(task).expect("reads").running);

    let state = core.browser_open(task).await.expect("starts");
    assert!(state.running);
    let endpoint = state.endpoint.clone().expect("an endpoint");
    assert!(endpoint.starts_with("http://127.0.0.1:"), "{endpoint}");
    let websocket = state.websocket.clone().expect("a websocket");
    assert!(websocket.contains("/devtools/browser/"), "{websocket}");
    assert_eq!(state.tabs.len(), 1);
    assert_eq!(state.tabs[0].url, "about:blank");
    assert_eq!(state.name.as_deref(), Some("fake-chrome"));

    let folder = fx.paths.browser(task);
    let mode = std::fs::metadata(&folder)
        .expect("made")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700, "the profile is the user's alone");
    let downloads = std::fs::read_to_string(folder.join("profile/fake-downloads")).expect("told");
    assert!(downloads.contains(&*state.downloads.to_string_lossy()));
    assert!(state.downloads.starts_with(&folder));

    assert!(alive(pid(&fx, task)));
    let overview = core.overview().expect("reads");
    assert!(overview[0].tasks[0].browser, "the sidebar says it runs");
    // Opening it again finds the same one.
    let again = core.browser_open(task).await.expect("opens");
    assert_eq!(again.endpoint, state.endpoint);
    core.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn the_pane_gets_pictures_of_the_tab_shown_at_its_size_and_none_once_hidden() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();

    let state = core.browser_show(task, None, VIEW).await.expect("shows");
    let shown = state.shown.clone().expect("the first tab is shown");
    assert_eq!(Some(&shown), state.tabs.first().map(|tab| &tab.id));
    let frame = |event| match event {
        BrowserEvent::Frame {
            tab,
            data,
            width,
            height,
            ..
        } => Some((tab, picture(&data), width, height)),
        _ => None,
    };
    let first = heard(&mut bus, frame).await;
    assert_eq!(first, (shown.clone(), "frame 1 640x480".into(), 640, 480));
    // The next comes once the first was acknowledged, at the pace allowed.
    let second = heard(&mut bus, frame).await;
    assert_eq!(second.1, "frame 2 640x480");

    core.browser_hide(task).await.expect("hides");
    while bus.try_recv().is_ok() {}
    tokio::time::sleep(Duration::from_millis(400)).await;
    let mut late = 0;
    while let Ok(event) = bus.try_recv() {
        late += usize::from(matches!(event, BrowserEvent::Frame { .. }));
    }
    assert!(late <= 1, "at most one already on its way, not {late}");
    core.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn the_tab_shown_shown_again_gets_pictures_at_the_new_size() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();
    let size = |event| match event {
        BrowserEvent::Frame { width, height, .. } => Some((width, height)),
        _ => None,
    };
    core.browser_show(task, None, VIEW).await.expect("shows");
    assert_eq!(heard(&mut bus, size).await, (640, 480));

    // The pane measured again: the same tab, at its new size. Chrome refuses to start
    // pictures a second time, so they are stopped first.
    let wider = Viewport { width: 900, ..VIEW };
    core.browser_show(task, None, wider)
        .await
        .expect("shows the same tab again");
    let resized = heard(&mut bus, |event| {
        size(event).filter(|&(width, _)| width == 900)
    });
    assert_eq!(resized.await, (900, 480));

    // Two at once, as the pane opening and a session's tools acting can be.
    let (one, two) = tokio::join!(
        core.browser_show(task, None, VIEW),
        core.browser_show(task, None, wider),
    );
    one.expect("the first shows");
    two.expect("the second shows");
    core.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn a_moving_page_is_sent_at_a_lower_quality_and_a_still_one_sharp_again() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();
    core.browser_show(task, None, VIEW).await.expect("shows");
    // The stand-in sends forty pictures, as fast as they are acknowledged, then none: a page
    // that moves, then stands still. It is started sharp, again lower as pictures come close
    // together, and sharp again once they stop, which sends the one picture that stays. A
    // pause on a busy machine is a page that stood still for that long, so it may go between
    // the two more than once while they come.
    let profile = fx.paths.browser(task).join("profile");
    let casts = casts_settled(&profile).await;
    let qualities: Vec<_> = casts.iter().map(|cast| cast["quality"].clone()).collect();
    assert_eq!(qualities.first(), Some(&90.into()), "{qualities:?}");
    assert!(qualities.contains(&60.into()), "{qualities:?}");
    assert_eq!(qualities.last(), Some(&90.into()), "{qualities:?}");
    let last = casts.last().map(|cast| cast["frame"].clone());
    let last = last.and_then(|frame| frame.as_u64()).expect("a number");
    let sharp = heard(&mut bus, |event| match event {
        BrowserEvent::Frame { data, .. } => {
            Some(picture(&data)).filter(|p| p.starts_with(&format!("frame {last} ")))
        }
        _ => None,
    });
    assert_eq!(
        sharp.await,
        format!("frame {last} 640x480"),
        "the sharp one after the forty"
    );
    core.browser_stop(task).await.expect("stops");
}

/// How long the starts of pictures must not change to count as settled.
const CASTS_SETTLE: Duration = Duration::from_millis(600);

/// The starts of pictures the stand-in noted, once some were lower, the last is sharp, and no
/// other came for [`CASTS_SETTLE`]: standing still, the page is not started again.
async fn casts_settled(profile: &Path) -> Vec<serde_json::Value> {
    let quality = |cast: &serde_json::Value| cast["quality"].as_u64();
    let (mut seen, mut since) = (Vec::new(), tokio::time::Instant::now());
    for _ in 0..200 {
        let casts = noted(profile, "fake-cast", 3).await;
        let moved = casts.iter().any(|cast| quality(cast) == Some(60));
        let sharp = casts.last().and_then(quality) == Some(90);
        let still = since.elapsed() >= CASTS_SETTLE;
        if casts != seen {
            (seen, since) = (casts, tokio::time::Instant::now());
        } else if moved && sharp && still {
            return casts;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the pictures were started again and again: {seen:?}");
}

#[tokio::test]
async fn a_chrome_with_a_window_is_drawn_by_chrome_and_raised_only_when_asked() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let settings = Settings {
        browser_display: BrowserDisplay::Window,
        ..core.settings().expect("reads")
    };
    core.set_settings(&settings).expect("test setup");
    let state = core.browser_show(task, None, VIEW).await.expect("shows");
    assert!(state.windowed, "started with a window of its own");
    let tab = state.shown.expect("a tab is shown");
    // Chrome draws it: no pictures, and no size set on its page.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let profile = fx.paths.browser(task).join("profile");
    assert!(!profile.join("fake-cast").exists(), "no pictures asked for");
    core.browser_raise(task, None).await.expect("raises");
    let raised = noted(&profile, "fake-activate", 1).await;
    assert_eq!(raised, [serde_json::json!(tab)]);
    core.browser_stop(task).await.expect("stops");

    // Started again with the pane chosen, it runs without one.
    let pane = Settings {
        browser_display: BrowserDisplay::Pane,
        ..settings
    };
    core.set_settings(&pane).expect("saves");
    let state = core.browser_show(task, None, VIEW).await.expect("shows");
    assert!(!state.windowed);
    core.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn a_chrome_started_again_opens_the_tabs_it_had_when_it_was_stopped() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();
    core.browser_open(task).await.expect("starts");
    core.browser_tab_new(task, Some("example.com"))
        .await
        .expect("opens a tab");
    core.browser_tab_new(task, Some("localhost:3000/cart"))
        .await
        .expect("opens a tab");
    state_where(&mut bus, |state| state.tabs.len() == 3).await;
    core.browser_stop(task).await.expect("stops");
    let kept = fx.paths.browser(task).join("tabs.json");
    let mode = std::fs::metadata(&kept).expect("kept").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "readable by the user alone");

    // Its web pages come back, and the blank page it started on goes. What was kept is used
    // up.
    let state = core
        .browser_show(task, None, VIEW)
        .await
        .expect("starts again");
    let urls: Vec<&str> = state.tabs.iter().map(|tab| tab.url.as_str()).collect();
    assert_eq!(urls, ["https://example.com", "http://localhost:3000/cart"]);
    assert_eq!(state.shown.as_ref(), state.tabs.first().map(|tab| &tab.id));
    assert!(!kept.exists(), "used up");

    // Stopped by a Swarmed not connected to it, as `swarmed task browser --stop` does, it
    // keeps nothing, and the pages of the stop before do not come back.
    let elsewhere = another_core(&fx);
    elsewhere.browser_stop(task).await.expect("stops");
    let state = elsewhere.browser_open(task).await.expect("starts again");
    let urls: Vec<&str> = state.tabs.iter().map(|tab| tab.url.as_str()).collect();
    assert_eq!(urls, ["about:blank"]);
    elsewhere.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn the_blank_tab_is_kept_when_it_was_used_while_the_kept_tabs_opened() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();
    core.browser_open(task).await.expect("starts");
    core.browser_tab_new(task, Some("example.com"))
        .await
        .expect("opens a tab");
    state_where(&mut bus, |state| state.tabs.len() == 2).await;
    core.browser_stop(task).await.expect("stops");

    // Started again, its kept tab takes a while to open, and meanwhile the blank tab it
    // started on is sent somewhere, as an agent's tools or the address bar would.
    let profile = fx.paths.browser(task).join("profile");
    std::fs::write(profile.join("fake-slow-open"), "").expect("test setup");
    let mut bus = core.subscribe_browsers();
    let using = blank_tab_used(&core, task, &mut bus);
    let (started, used) = tokio::join!(core.browser_open(task), using);
    started.expect("starts again");
    let state = core.browser_state(task).expect("reads");
    let urls: Vec<(&str, &str)> = state
        .tabs
        .iter()
        .map(|tab| (tab.id.as_str(), tab.url.as_str()))
        .collect();
    assert!(
        urls.contains(&(used.as_str(), "http://localhost:3000/form")),
        "the tab that was used stays: {urls:?}"
    );
    assert!(urls.iter().any(|(_, url)| *url == "https://example.com"));
    core.browser_stop(task).await.expect("stops");
}

/// The blank tab of `state`, if it has one.
fn blank_tab(state: &BrowserState) -> Option<String> {
    let found = state.tabs.iter().find(|tab| tab.url == "about:blank");
    found.map(|tab| tab.id.clone())
}

/// Sends the blank tab of the task's browser somewhere as soon as it is told of and can be
/// spoken to. Returns its id.
async fn blank_tab_used(core: &Core, task: i64, bus: &mut Receiver<BrowserEvent>) -> String {
    let started = state_where(bus, |state| blank_tab(state).is_some()).await;
    let tab = blank_tab(&started).expect("the blank tab");
    let form = BrowserGo::Address("localhost:3000/form".into());
    for _ in 0..100 {
        if core.browser_go(task, &tab, &form).await.is_ok() {
            return tab;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the blank tab could not be sent anywhere");
}

#[tokio::test]
async fn clicks_keys_and_text_reach_the_tab_shown_in_the_order_they_came() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let mouse = |kind| MouseInput {
        kind,
        x: 12.0,
        y: 34.0,
        button: "left".into(),
        buttons: 1,
        click_count: 1,
        delta_x: 0.0,
        delta_y: 0.0,
        modifiers: Modifiers::default(),
    };
    let refused = core.browser_mouse(task, &mouse(MouseKind::Down));
    assert!(refused.is_err(), "nothing runs to click on yet");

    core.browser_show(task, None, VIEW).await.expect("shows");
    core.browser_mouse(task, &mouse(MouseKind::Down))
        .expect("sent");
    core.browser_mouse(task, &mouse(MouseKind::Up))
        .expect("sent");
    let key = KeyInput {
        kind: KeyKind::Down,
        key: "a".into(),
        code: "KeyA".into(),
        key_code: 65,
        modifiers: Modifiers::default(),
        repeat: false,
    };
    core.browser_key(task, &key).expect("sent");
    core.browser_text(task, "pasted words").expect("sent");

    let told = inputs(&fx.paths.browser(task).join("profile"), 4).await;
    let said: Vec<(&str, &str)> = told
        .iter()
        .map(|input| {
            let params = &input["params"];
            let what = params["type"].as_str().or_else(|| params["text"].as_str());
            (
                input["method"].as_str().unwrap_or_default(),
                what.unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            ("Input.dispatchMouseEvent", "mousePressed"),
            ("Input.dispatchMouseEvent", "mouseReleased"),
            ("Input.dispatchKeyEvent", "keyDown"),
            ("Input.insertText", "pasted words"),
        ]
    );
    assert_eq!(told[0]["params"]["x"], 12.0);
    assert_eq!(told[2]["params"]["text"], "a");
    core.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn a_tab_goes_where_it_is_sent_back_again_and_keeps_what_it_logged() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();
    let state = core.browser_show(task, None, VIEW).await.expect("shows");
    let tab = state.shown.expect("shown");

    let address = BrowserGo::Address("localhost:3000/console".into());
    core.browser_go(task, &tab, &address).await.expect("goes");
    let there = "http://localhost:3000/console";
    // What the page logged is said as it comes; where its history stands, once read.
    let line = heard(&mut bus, |event| match event {
        BrowserEvent::Console { line, .. } => Some(line),
        _ => None,
    })
    .await;
    assert_eq!(line.text, format!("hello from {there}"));
    let state = state_where(&mut bus, |state| {
        state.tabs.iter().any(|t| t.url == there && t.can_go_back)
    })
    .await;
    assert_eq!(state.tabs[0].title, format!("Page at {there}"));
    let kept = core.browser_console(task, &tab).expect("reads");
    assert_eq!(kept, [line]);

    core.browser_go(task, &tab, &BrowserGo::Back)
        .await
        .expect("goes back");
    state_where(&mut bus, |state| {
        state
            .tabs
            .iter()
            .any(|t| t.url == "about:blank" && t.can_go_forward)
    })
    .await;

    let nowhere = BrowserGo::Address("localhost:9/refused".into());
    let refused = core
        .browser_go(task, &tab, &nowhere)
        .await
        .expect_err("refused");
    assert!(
        refused.to_string().contains("ERR_CONNECTION_REFUSED"),
        "{refused}"
    );
    core.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn tabs_are_opened_and_closed_and_a_closed_tab_is_no_longer_shown() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();
    core.browser_open(task).await.expect("starts");

    let opened = core
        .browser_tab_new(task, Some("example.com"))
        .await
        .expect("opens a tab");
    let state = state_where(&mut bus, |state| state.tabs.len() == 2).await;
    assert_eq!(state.tabs[1].id, opened);
    assert_eq!(state.tabs[1].url, "https://example.com");
    let shown = core
        .browser_show(task, Some(&opened), VIEW)
        .await
        .expect("shows");
    assert_eq!(shown.shown.as_deref(), Some(opened.as_str()));

    // Hidden and shown again, the pane is on the tab it was on, not back on the first.
    core.browser_hide(task).await.expect("hides");
    let again = core.browser_show(task, None, VIEW).await.expect("shows");
    assert_eq!(again.shown.as_deref(), Some(opened.as_str()));

    core.browser_tab_close(task, &opened).await.expect("closes");
    let state = state_where(&mut bus, |state| state.tabs.len() == 1).await;
    assert_eq!(state.shown, None);
    let gone = core.browser_show(task, Some(&opened), VIEW).await;
    assert!(gone.is_err(), "a closed tab cannot be shown");
    core.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn a_chrome_that_cannot_start_says_why_and_leaves_nothing_written_down() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let profile = fx.paths.browser(task).join("profile");
    std::fs::create_dir_all(&profile).expect("test setup");
    std::fs::write(profile.join("fake-fail"), "").expect("test setup");

    let refused = core.browser_open(task).await.expect_err("does not start");
    let said = refused.to_string();
    assert!(said.contains("ended before it was ready"), "{said}");
    assert!(said.contains("the profile is in use"), "{said}");
    assert!(!record(&fx, task).exists());
    assert!(!core.browser_state(task).expect("reads").running);
}

#[tokio::test]
async fn a_chrome_given_up_on_while_it_starts_is_written_down_and_can_be_stopped() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    let profile = fx.paths.browser(task).join("profile");
    std::fs::create_dir_all(&profile).expect("test setup");
    std::fs::write(profile.join("fake-slow"), "").expect("test setup");

    // Whoever asked goes before it listens, as a Swarmed that quits or a call that is dropped.
    let opening = tokio::time::timeout(Duration::from_millis(500), core.browser_open(task));
    assert!(opening.await.is_err(), "it was not ready yet");
    let process = pid(&fx, task);
    assert!(alive(process));

    core.browser_stop(task).await.expect("stops");
    assert!(!alive(process));
    assert!(!record(&fx, task).exists());
}

#[tokio::test]
async fn without_any_browser_the_pane_is_told_none_was_found() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let settings = Settings {
        browser_program: "/nowhere/chrome".into(),
        ..Settings::default()
    };
    core.set_settings(&settings).expect("test setup");
    let setup = core.browser_setup().expect("looks");
    assert_eq!((setup.name, setup.chosen), (None, true));
    let missing = core.browser_open(task_id(&core)).await.expect_err("none");
    assert!(
        matches!(missing, swarmed_core::Error::BrowserMissing),
        "{missing}"
    );
}

#[tokio::test]
async fn a_chrome_left_running_is_found_again_by_the_next_swarmed_and_stopped_there() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let first = core_with_task(&fx).await;
    let task = task_id(&first);
    let started = first.browser_open(task).await.expect("starts");
    let process = pid(&fx, task);
    // Gone without stopping it, as a Swarmed that crashed.
    drop(first);

    let next = Core::with_backend(
        fx.paths.clone(),
        std::sync::Arc::new(swarmed_core::agent::FakeBackend::default()),
    )
    .expect("opens");
    assert!(next.browser_state(task).expect("reads").running);
    assert!(next.overview().expect("reads")[0].tasks[0].browser);
    let found = next.browser_open(task).await.expect("connects");
    assert_eq!(
        found.endpoint, started.endpoint,
        "the same Chrome, not a second"
    );
    assert_eq!(pid(&fx, task), process);

    // A third finds it too, and stops it without ever connecting.
    let third = Core::with_backend(
        fx.paths.clone(),
        std::sync::Arc::new(swarmed_core::agent::FakeBackend::default()),
    )
    .expect("opens");
    third.browser_stop(task).await.expect("stops");
    assert!(!alive(process));
    assert!(!record(&fx, task).exists());
    assert!(!next.browser_state(task).expect("reads").running);
}

fn another_core(fx: &common::Fixture) -> Core {
    Core::with_backend(
        fx.paths.clone(),
        std::sync::Arc::new(swarmed_core::agent::FakeBackend::default()),
    )
    .expect("opens")
}

#[tokio::test]
async fn looking_after_the_browsers_starts_none_for_a_chrome_that_went_with_its_swarmed() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    core.arrive().expect("arrives");
    let task = task_id(&core);
    // Written down by a Swarmed that crashed, of a Chrome that is no longer there.
    let written = record(&fx, task);
    std::fs::create_dir_all(written.parent().expect("has a parent")).expect("test setup");
    std::fs::write(&written, "999999\nMon Jan  1 00:00:00 2001\n").expect("test setup");

    core.browsers_look().await.expect("looks");
    assert!(!core.browser_state(task).expect("reads").running);
    assert!(!written.exists(), "what was written down is forgotten");
    assert!(
        !fx.paths.browser(task).join("profile").exists(),
        "no Chrome was started"
    );
}

#[tokio::test]
async fn looking_after_the_browsers_connects_to_one_left_running_and_stops_none_shown() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let first = core_with_task(&fx).await;
    let task = task_id(&first);
    first.browser_open(task).await.expect("starts");
    let process = pid(&fx, task);
    drop(first);

    let next = another_core(&fx);
    next.arrive().expect("arrives");
    let mut bus = next.subscribe_browsers();
    next.browsers_look().await.expect("looks");
    // Connected to: its tabs are told of, which only a connection hears.
    let state = state_where(&mut bus, |state| state.running).await;
    assert_eq!(state.tabs.len(), 1);
    assert_eq!(pid(&fx, task), process, "the same Chrome, not a second");

    // Never stopped for sitting unused where the setting is 0.
    let settings = Settings {
        browser_idle_minutes: 0,
        ..next.settings().expect("reads")
    };
    next.set_settings(&settings).expect("saves");
    next.browsers_look().await.expect("looks");
    assert!(alive(process));
    next.browser_stop(task).await.expect("stops");
    assert!(!alive(process));
}

#[tokio::test]
async fn two_swarmeds_opening_a_tasks_browser_at_once_start_one_chrome() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let one = core_with_task(&fx).await;
    let two = another_core(&fx);
    let task = task_id(&one);

    let (first, second) = tokio::join!(one.browser_open(task), two.browser_open(task));
    let (first, second) = (first.expect("opens"), second.expect("opens"));
    assert_eq!(first.endpoint, second.endpoint, "the same Chrome");
    let process = pid(&fx, task);
    one.browser_stop(task).await.expect("stops");
    assert!(!alive(process));
}

#[tokio::test]
async fn putting_the_task_away_stops_its_chrome_and_keeps_what_was_signed_in_to() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    core.browser_open(task).await.expect("starts");
    let process = pid(&fx, task);

    core.task_done("demo", "one", &hooks::discard)
        .await
        .expect("puts away");
    assert!(!alive(process));
    assert!(!core.browser_state(task).expect("reads").running);
    assert!(fx.paths.browser(task).join("profile").exists());
}

#[tokio::test]
async fn removing_the_task_stops_its_chrome_and_deletes_its_profile() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let task = task_id(&core);
    core.browser_open(task).await.expect("starts");
    let process = pid(&fx, task);

    let options = RemoveOptions {
        force: true,
        delete_branch: false,
    };
    let notes = core
        .task_remove("demo", "one", options, &hooks::discard)
        .await
        .expect("removes");
    assert!(notes.is_empty(), "{notes:?}");
    assert!(!alive(process));
    assert!(!fx.paths.browser(task).exists());
    assert!(!record(&fx, task).exists());
}

#[tokio::test]
async fn quitting_stops_every_browser_the_first_swarmed_finds() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    core.arrive().expect("arrives");
    let task = task_id(&core);
    core.browser_open(task).await.expect("starts");
    let process = pid(&fx, task);
    core.leave().await;
    assert!(!alive(process));
    assert!(!record(&fx, task).exists());
}

#[tokio::test]
async fn the_cleanup_lists_the_browser_of_a_task_that_is_gone_and_no_other() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_task(&fx).await;
    let current = fx.paths.browser(task_id(&core));
    std::fs::create_dir_all(current.join("profile")).expect("test setup");
    let orphan = fx.paths.browser(999);
    std::fs::create_dir_all(orphan.join("profile")).expect("test setup");
    std::fs::write(orphan.join("profile/Cookies"), "signed in").expect("test setup");

    let items = core
        .cleanup_items(CleanupKind::Browsers)
        .await
        .expect("lists");
    let names: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, [orphan.to_string_lossy()]);
    assert!(!items[0].at_risk);
    let cleaned = core
        .cleanup_clean(CleanupKind::Browsers, None, None)
        .await
        .expect("cleans");
    assert_eq!(cleaned.deleted.len(), 1);
    assert!(!orphan.exists());
    assert!(current.exists());
}

/// The same against the Chrome, Chromium, Edge or Brave installed on this machine, which CI
/// has not got: `cargo test --test browser -- --ignored`.
#[tokio::test]
#[ignore = "needs a real Chrome on the machine"]
async fn with_a_real_chrome_a_page_is_drawn_typed_into_and_logs() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();
    let state = core.browser_show(task, None, VIEW).await.expect("shows");
    let tab = state.shown.expect("shown");
    let page = "data:text/html,<input autofocus oninput=\"document.title=this.value\">\
                <script>console.warn('ready')</script>";
    let address = BrowserGo::Address(page.into());
    core.browser_go(task, &tab, &address).await.expect("goes");
    let line = heard(&mut bus, |event| match event {
        BrowserEvent::Console { line, .. } => Some(line),
        _ => None,
    })
    .await;
    assert_eq!(line.text, "ready");
    let jpeg = heard(&mut bus, |event| match event {
        BrowserEvent::Frame { data, width, .. } => Some((data, width)),
        _ => None,
    })
    .await;
    assert!(jpeg.0.starts_with("/9j/"), "a JPEG");
    assert_eq!(jpeg.1, VIEW.width);
    // Into the field, as the user would: a click first.
    for kind in [MouseKind::Down, MouseKind::Up] {
        let click = MouseInput {
            kind,
            x: 20.0,
            y: 15.0,
            button: "left".into(),
            buttons: u32::from(kind == MouseKind::Down),
            click_count: 1,
            delta_x: 0.0,
            delta_y: 0.0,
            modifiers: Modifiers::default(),
        };
        core.browser_mouse(task, &click).expect("clicks");
    }
    core.browser_text(task, "typed").expect("types");
    state_where(&mut bus, |state| {
        state.tabs.iter().any(|t| t.title == "typed")
    })
    .await;
    core.browser_stop(task).await.expect("stops");
}

/// On a screen with two pixels to each point, a real Chrome draws the page at that density:
/// pictures drawn at one point to a pixel are blurred twice over when the pane enlarges them.
#[tokio::test]
#[ignore = "needs a real Chrome on the machine"]
async fn with_a_real_chrome_pictures_have_the_screens_pixels() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();
    let retina = Viewport { scale: 2.0, ..VIEW };
    let state = core.browser_show(task, None, retina).await.expect("shows");
    let tab = state.shown.expect("shown");
    let page = "data:text/html,<p style=\"font:13px system-ui\">The quick brown fox</p>";
    let address = BrowserGo::Address(page.into());
    core.browser_go(task, &tab, &address).await.expect("goes");
    assert_eq!(settled(&mut bus).await, (1280, 960));
    // The still once the page stopped is the last: taking it sets off no more pictures.
    let after = settled_count(&mut bus).await;
    assert_eq!(after, 0, "pictures after the still");
    // On a screen of one pixel to a point, Chrome makes them smaller: none wider than asked.
    core.browser_show(task, None, VIEW).await.expect("shows");
    assert_eq!(settled(&mut bus).await, (640, 480));
    core.browser_stop(task).await.expect("stops");
}

/// The last state and how many pictures came in three seconds.
async fn heard_after(bus: &mut Receiver<BrowserEvent>) -> (Option<BrowserState>, usize) {
    let (mut last_state, mut pictures) = (None, 0);
    let until = tokio::time::Instant::now() + Duration::from_secs(3);
    while let Ok(Ok(event)) = tokio::time::timeout_at(until, bus.recv()).await {
        pictures += usize::from(matches!(event, BrowserEvent::Frame { .. }));
        if let BrowserEvent::State(state) = event {
            last_state = Some(state);
        }
    }
    (last_state, pictures)
}

/// How many pictures came in a second.
async fn settled_count(bus: &mut Receiver<BrowserEvent>) -> usize {
    let until = tokio::time::Instant::now() + Duration::from_secs(1);
    let mut count = 0;
    while let Ok(Ok(event)) = tokio::time::timeout_at(until, bus.recv()).await {
        count += usize::from(matches!(event, BrowserEvent::Frame { .. }));
    }
    count
}

/// The size of the last picture heard within two seconds: the first pictures after a change
/// can be of the page before it took its size.
async fn settled(bus: &mut Receiver<BrowserEvent>) -> (u16, u16) {
    let mut sizes = Vec::new();
    let until = tokio::time::Instant::now() + Duration::from_secs(2);
    while let Ok(Ok(event)) = tokio::time::timeout_at(until, bus.recv()).await {
        if let BrowserEvent::Frame { data, .. } = event {
            sizes.push(jpeg_size(&data));
        }
    }
    sizes.last().copied().expect("pictures came")
}

/// A JPEG's own width and height, from its start-of-frame marker.
fn jpeg_size(data: &str) -> (u16, u16) {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .expect("base64");
    let mut at = 2;
    while at + 9 < bytes.len() {
        let marker = bytes[at + 1];
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        if (0xC0..=0xC3).contains(&marker) {
            let height = u16::from_be_bytes([bytes[at + 5], bytes[at + 6]]);
            let width = u16::from_be_bytes([bytes[at + 7], bytes[at + 8]]);
            return (width, height);
        }
        at += 2 + length;
    }
    panic!("no size in the JPEG");
}

/// A real Chrome with a window of its own says so, and its window can be brought forward. It
/// opens a window on the screen for a moment: `cargo test --test browser -- --ignored window`.
#[tokio::test]
#[ignore = "needs a real Chrome on the machine, and opens a window on the screen"]
async fn with_a_real_chrome_in_a_window_it_says_so_and_is_brought_forward() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let task = task_id(&core);
    let settings = Settings {
        browser_display: BrowserDisplay::Window,
        ..core.settings().expect("reads")
    };
    core.set_settings(&settings).expect("test setup");
    let state = core.browser_show(task, None, VIEW).await.expect("shows");
    assert!(
        state.windowed,
        "a Chrome with a window does not name itself HeadlessChrome"
    );
    core.browser_raise(task, None)
        .await
        .expect("brought forward");
    core.browser_stop(task).await.expect("stops");
}

/// Stopped and started again, a real Chrome is shown again, and stays shown: what the stopped
/// one says as it goes is not taken for the new one.
#[tokio::test]
#[ignore = "needs a real Chrome on the machine"]
async fn with_a_real_chrome_stopped_and_started_again_it_is_shown_again() {
    let fx = fixture();
    let _ends = ends_browsers(&fx);
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let task = task_id(&core);
    let mut bus = core.subscribe_browsers();
    let shown = core.browser_show(task, None, VIEW).await.expect("shows");
    let tab = shown.shown.expect("a tab");
    // Nothing listens there: Chrome shows its error page, at that address.
    let away = BrowserGo::Address("http://127.0.0.1:9/away".into());
    core.browser_go(task, &tab, &away).await.ok();
    state_where(&mut bus, |state| {
        state.tabs.iter().any(|tab| tab.url.ends_with("/away"))
    })
    .await;
    core.browser_stop(task).await.expect("stops");
    let again = core
        .browser_show(task, None, VIEW)
        .await
        .expect("shows again");
    assert!(again.running && again.shown.is_some(), "{again:?}");
    let urls: Vec<&str> = again.tabs.iter().map(|tab| tab.url.as_str()).collect();
    assert_eq!(
        urls,
        ["http://127.0.0.1:9/away"],
        "its tab again, not a blank page"
    );
    // What comes after: pictures of it, and no word that it stopped.
    let (last_state, pictures) = heard_after(&mut bus).await;
    assert!(pictures > 0, "no pictures after starting again");
    if let Some(state) = last_state {
        assert!(state.running && state.shown.is_some(), "{state:?}");
    }
    let record = fx
        .paths
        .data_dir
        .join("run/browsers")
        .join(format!("{task}.pid"));
    assert!(record.exists(), "the new Chrome is still written down");
    core.browser_stop(task).await.expect("stops");
}
