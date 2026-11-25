//! Picking an element of a page in the task's browser and pointing the agent at it (#383),
//! against the stand-in Chrome (`fixtures/fake_chrome.py`): Chrome's inspect mode is started on
//! the tab shown, a click picks an element, which is read and kept with its picture in the
//! session's folder, and a message names it by its number.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{core_with_project, fixture};
use swarmed_core::{
    Attached, BrowserGo, Core, ELEMENTS_PICKED, Error, Modifiers, MouseInput, MouseKind,
    NewTaskOptions, SessionOptions, Settings, Viewport, hooks,
};

const VIEW: Viewport = Viewport {
    width: 800,
    height: 600,
    scale: 2.0,
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
        let pid = |path: PathBuf| -> Option<u32> {
            std::fs::read_to_string(path)
                .ok()?
                .lines()
                .next()?
                .parse()
                .ok()
        };
        for leader in records.flatten().filter_map(|record| pid(record.path())) {
            let killed = std::process::Command::new("kill")
                .args(["-KILL", "--", &format!("-{leader}")])
                .status();
            killed.ok();
        }
    }
}

/// A core with task `one`, whose browser runs on the stand-in, or on the Chrome installed
/// with `real`; and a session of the task.
async fn core_with_session(fx: &common::Fixture, real: bool) -> (Core, i64, i64) {
    let core = core_with_project(fx).await;
    if !real {
        let root = fx.paths.data_dir.parent().expect("has a parent");
        let program = root.join("fake-chrome");
        common::executable(&program, include_str!("fixtures/fake_chrome.py"));
        let settings = Settings {
            browser_program: program.to_string_lossy().into_owned(),
            ..Settings::default()
        };
        core.set_settings(&settings).expect("test setup");
    }
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let task = core.task("demo", "one").expect("there").1.id;
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    (core, task, session.id)
}

fn mouse(kind: MouseKind, x: f64, y: f64) -> MouseInput {
    MouseInput {
        kind,
        x,
        y,
        button: if kind == MouseKind::Move {
            "none"
        } else {
            "left"
        }
        .into(),
        buttons: u32::from(kind == MouseKind::Down),
        click_count: u32::from(kind != MouseKind::Move),
        delta_x: 0.0,
        delta_y: 0.0,
        modifiers: Modifiers::default(),
    }
}

/// The lines the stand-in wrote into `name` in its profile, once there are `count`.
async fn noted(profile: &Path, name: &str, count: usize) -> Vec<serde_json::Value> {
    let file = profile.join(name);
    for _ in 0..200 {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if text.lines().count() >= count {
            let line = |line: &str| serde_json::from_str(line).expect("JSON");
            return text.lines().map(line).collect();
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the stand-in did not note {count} lines in {name}");
}

/// Clicks at `x`, `y` once the stand-in is in inspect mode as many times as `inspecting` says.
async fn click_once_inspecting(core: &Core, task: i64, profile: &Path, inspecting: usize) {
    noted(profile, "fake-inspect", inspecting).await;
    core.browser_mouse(task, &mouse(MouseKind::Down, 60.0, 110.0))
        .expect("clicks");
    core.browser_mouse(task, &mouse(MouseKind::Up, 60.0, 110.0))
        .expect("clicks");
}

#[tokio::test]
async fn a_click_in_inspect_mode_picks_an_element_kept_with_its_picture_for_the_session() {
    let fx = fixture();
    let _ends = EndsBrowsers(fx.paths.data_dir.clone());
    let (core, task, session) = core_with_session(&fx, false).await;
    let state = core.browser_show(task, None, VIEW).await.expect("shows");
    let tab = state.shown.expect("shown");
    let settings = BrowserGo::Address("localhost:3000/settings".into());
    core.browser_go(task, &tab, &settings).await.expect("goes");
    let profile = fx.paths.browser(task).join("profile");

    let (picked, ()) = tokio::join!(
        core.browser_pick(task, session),
        click_once_inspecting(&core, task, &profile, 1)
    );
    let element = picked.expect("picks").expect("an element");
    assert_eq!(element.number, 1);
    assert_eq!(element.label, "button \"Save\" on /settings");
    assert_eq!(element.url, "http://localhost:3000/settings");
    assert_eq!(
        (element.tag.as_str(), element.element_id.as_deref()),
        ("button", Some("save"))
    );
    assert_eq!(element.classes, ["btn", "primary"]);
    assert_eq!(element.selector, "#save");
    assert_eq!(
        (element.role.as_deref(), element.name.as_deref()),
        (Some("button"), Some("Save"))
    );
    assert_eq!(element.text, "Save");
    assert!(element.html.starts_with("<button id=\"save\""));
    let styles: Vec<String> = element
        .styles
        .iter()
        .map(|style| format!("{}: {}", style.property, style.value))
        .collect();
    assert_eq!(styles, ["color: rgb(255, 0, 0)", "font-size: 14px"]);
    let bounds = element.bounds.expect("a box");
    assert_eq!(
        [bounds.x, bounds.y, bounds.width, bounds.height],
        [40.0, 100.0, 120.0, 40.0]
    );

    // The picture is the session's, beside what was read, for the user alone.
    let folder = fx.paths.attachments(session);
    assert_eq!(element.picture, Some(folder.join("element-1.png")));
    let picture = std::fs::read(folder.join("element-1.png")).expect("kept");
    assert!(picture.starts_with(b"\x89PNG"));
    let mode = |path: &Path| std::fs::metadata(path).expect("there").permissions().mode();
    assert_eq!(mode(&folder.join("element-1.json")) & 0o777, 0o600);
    assert_eq!(mode(&folder) & 0o777, 0o700);

    // Inspect mode went on for the pick and off before the picture; the script ran in a world
    // of Swarmed's own; the picture is the box and a margin, where the page is scrolled to.
    let modes = noted(&profile, "fake-inspect", 2).await;
    assert_eq!(modes, ["searchForNode", "none"]);
    let read = noted(&profile, "fake-pick", 3).await;
    assert_eq!(read[0]["world"], "swarmed-pick");
    assert_eq!(read[1]["resolved in"], 7);
    let clip = &read[2]["clip"];
    assert_eq!(
        [&clip["x"], &clip["y"], &clip["width"], &clip["height"]],
        [28.0, 388.0, 144.0, 64.0]
    );
    core.browser_stop(task).await.expect("stops");
}

#[tokio::test]
async fn a_message_names_picks_by_number_and_lists_them_under_its_words() {
    let fx = fixture();
    let _ends = EndsBrowsers(fx.paths.data_dir.clone());
    let (core, task, session) = core_with_session(&fx, false).await;
    core.browser_show(task, None, VIEW).await.expect("shows");
    let profile = fx.paths.browser(task).join("profile");
    for picks in 1..=2 {
        let (picked, ()) = tokio::join!(
            core.browser_pick(task, session),
            click_once_inspecting(&core, task, &profile, picks * 2 - 1)
        );
        assert_eq!(picked.expect("picks").expect("picked").number, picks as u64);
    }
    core.browser_stop(task).await.expect("stops");

    let attached = Attached {
        elements: vec![2, 1, 2],
        ..Attached::default()
    };
    let typed = "make [Element 2: button \"Save\" on about:blank] like [Element 1: button]";
    let queue = core
        .session_enqueue_message(session, typed, &attached)
        .expect("queues");
    let text = &queue[0].text;
    let (words, list) = text.split_once("\n\n").expect("a list");
    assert_eq!(words, typed);
    let entries: Vec<&str> = list.lines().filter(|line| line.starts_with("- ")).collect();
    assert_eq!(
        entries,
        [
            "- element 2: button \"Save\" on about:blank",
            "- element 1: button \"Save\" on about:blank"
        ]
    );
    assert!(list.starts_with(ELEMENTS_PICKED));
    let picture = fx.paths.attachments(session).join("element-2.png");
    assert!(list.contains(&format!("  Picture: {}", picture.display())));
    assert!(list.contains("  HTML:\n  ```html\n  <button id=\"save\""));

    // One never picked for this session is refused before anything is sent.
    let unknown = Attached {
        elements: vec![9],
        ..Attached::default()
    };
    let refused = core.session_enqueue_message(session, "this", &unknown);
    assert!(
        matches!(refused, Err(Error::NoSuchElement(9))),
        "{refused:?}"
    );
    let other = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    let elsewhere = core.session_enqueue_message(other.id, "this", &attached);
    assert!(matches!(elsewhere, Err(Error::NoSuchElement(2))));
    let kept = core.session_elements(session, &[1]).expect("reads");
    assert_eq!(kept[0].selector, "#save");
}

#[tokio::test]
async fn picking_ends_without_a_pick_when_asked_or_when_another_tab_is_shown() {
    let fx = fixture();
    let _ends = EndsBrowsers(fx.paths.data_dir.clone());
    let (core, task, session) = core_with_session(&fx, false).await;
    core.browser_show(task, None, VIEW).await.expect("shows");
    let profile = fx.paths.browser(task).join("profile");

    let end = async {
        noted(&profile, "fake-inspect", 1).await;
        core.browser_pick_end(task).expect("ends");
    };
    let (picked, ()) = tokio::join!(core.browser_pick(task, session), end);
    assert_eq!(picked.expect("ended"), None);
    assert_eq!(
        noted(&profile, "fake-inspect", 2).await,
        ["searchForNode", "none"]
    );

    let other = core.browser_tab_new(task, None).await.expect("opens");
    let away = async {
        noted(&profile, "fake-inspect", 3).await;
        core.browser_show(task, Some(&other), VIEW)
            .await
            .expect("shows");
    };
    let (picked, ()) = tokio::join!(core.browser_pick(task, session), away);
    assert_eq!(picked.expect("ended"), None);
    assert!(
        !fx.paths
            .attachments(session)
            .join("element-1.json")
            .exists()
    );

    // A pick goes only to a session of the task, and only where the browser runs.
    core.task_new("demo", "two", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let elsewhere = core
        .session_new("demo", "two", &SessionOptions::default())
        .expect("creates");
    let refused = core.browser_pick(task, elsewhere.id).await;
    let why = refused.expect_err("another task's session").to_string();
    assert!(why.contains("does not work in its task"), "{why}");
    core.browser_stop(task).await.expect("stops");
    let stopped = core.browser_pick(task, session).await;
    assert!(
        matches!(stopped, Err(Error::BrowserNotRunning(_))),
        "{stopped:?}"
    );
}

/// Clicks the real page's Save button once inspect mode has had time to start: past the first
/// button, inside its margin.
async fn click_save(core: &Core, task: i64) {
    tokio::time::sleep(Duration::from_millis(500)).await;
    for kind in [MouseKind::Move, MouseKind::Down, MouseKind::Up] {
        core.browser_mouse(task, &mouse(kind, 180.0, 90.0))
            .expect("clicks");
    }
}

/// The same against the Chrome, Chromium, Edge or Brave installed on this machine, which CI
/// has not got: `cargo test --test pick -- --ignored`.
#[tokio::test]
#[ignore = "needs a real Chrome on the machine"]
async fn with_a_real_chrome_an_element_is_picked_read_and_pictured() {
    let fx = fixture();
    let _ends = EndsBrowsers(fx.paths.data_dir.clone());
    let (core, task, session) = core_with_session(&fx, true).await;
    let state = core.browser_show(task, None, VIEW).await.expect("shows");
    let tab = state.shown.expect("shown");
    let page = "data:text/html,<body style=\"margin:0\"><main><p>Hi</p>\
                <form class=\"settings\"><button class=\"btn\">Back</button>\
                <button class=\"btn primary\" style=\"margin:40px;width:120px;height:40px;\
                color:rgb(255,0,0)\">Save</button></form></main></body>";
    let address = BrowserGo::Address(page.into());
    core.browser_go(task, &tab, &address).await.expect("goes");
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (picked, ()) = tokio::join!(core.browser_pick(task, session), click_save(&core, task));
    let element = picked.expect("picks").expect("an element");
    assert_eq!(element.tag, "button");
    assert_eq!(element.name.as_deref(), Some("Save"));
    assert_eq!(element.role.as_deref(), Some("button"));
    assert_eq!(element.text, "Save");
    assert!(
        element.selector.ends_with("button.btn.primary"),
        "{}",
        element.selector
    );
    let color = element
        .styles
        .iter()
        .find(|style| style.property == "color");
    assert_eq!(
        color.map(|style| style.value.as_str()),
        Some("rgb(255, 0, 0)")
    );
    let picture = std::fs::read(element.picture.expect("a picture")).expect("kept");
    assert!(picture.starts_with(b"\x89PNG"));
    core.browser_stop(task).await.expect("stops");
}
