//! Calls the app's commands the way the UI does, by name with JSON arguments, on Tauri's mock
//! runtime. The argument names here are the ones `ui/src/api/tauri.ts` sends: if a command's
//! parameters are renamed, this is where it shows.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use swarmed_core::agent::FakeBackend;
use swarmed_core::{Core, Paths};
use tauri::http::HeaderMap;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tempfile::TempDir;

use crate::with_core;

struct App {
    _dir: TempDir,
    repo: std::path::PathBuf,
    window: WebviewWindow<MockRuntime>,
    /// The page the commands are called from; Tauri checks it against what it serves.
    origin: Url,
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

/// The app on the mock runtime, over a core with the scripted agent and one git repository.
fn app() -> App {
    // Nothing listens on the discard port: a look for a new version fails at once.
    app_updating_from("https://127.0.0.1:9/latest.json")
}

/// The same app, looking for new versions of itself at `endpoint`.
fn app_updating_from(endpoint: &str) -> App {
    app_built(endpoint, |_| {})
}

/// The app, its core changed by `change` first.
fn app_built(endpoint: &str, change: impl FnOnce(&mut Core)) -> App {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("temp dir resolves");
    let repo = root.join("demo");
    std::fs::create_dir(&repo).expect("repo dir");
    git(&repo, &["init", "--quiet", "-b", "main"]);
    // Commits Swarmed makes for the user are the user's git's: give it a name, as a user has.
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    std::fs::write(repo.join("README.md"), "demo\n").expect("readme");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--quiet", "-m", "init"]);

    let paths = Paths {
        data_dir: root.join("data"),
        worktrees_root: root.join("worktrees"),
        claude_config: root.join("claude.json"),
        codex_config: root.join("codex.toml"),
        opencode_config_dir: root.join("opencode"),
    };
    let mut core = Core::with_backend(paths, Arc::new(FakeBackend::default())).expect("core opens");
    change(&mut core);
    let mut context = mock_context(noop_assets());
    let updater = json!({ "pubkey": "none", "endpoints": [endpoint] });
    context
        .config_mut()
        .plugins
        .0
        .insert("updater".to_string(), updater);
    let plugins = mock_builder().plugin(tauri_plugin_updater::Builder::new().build());
    let app = with_core(plugins, Arc::new(core))
        .build(context)
        .expect("app builds");
    let window = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .expect("window builds");
    // The address Tauri serves the app from; a call from anywhere else counts as remote and is
    // refused. It differs by platform.
    let local = if cfg!(windows) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };
    let origin = local.parse().expect("url parses");
    App {
        _dir: dir,
        repo,
        window,
        origin,
    }
}

impl App {
    /// A call of a command from the page at `url`, carrying `key` as the window's key.
    fn request(command: &str, arguments: Value, url: Url, key: &str) -> InvokeRequest {
        InvokeRequest {
            cmd: command.to_string(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url,
            body: InvokeBody::Json(arguments),
            headers: HeaderMap::default(),
            invoke_key: key.to_string(),
        }
    }

    /// Invokes a command as the UI would; `Err` holds the error the UI would be shown.
    fn invoke(&self, command: &str, arguments: Value) -> Result<Value, Value> {
        self.invoke_from(&self.origin, command, arguments)
    }

    /// Invokes a command as a page at `url` would, with the window's key.
    fn invoke_from(&self, url: &Url, command: &str, arguments: Value) -> Result<Value, Value> {
        let request = Self::request(command, arguments, url.clone(), INVOKE_KEY);
        let body = get_ipc_response(&self.window, request)?;
        body.deserialize::<Value>()
            .map_err(|e| json!(e.to_string()))
    }

    /// Whether a call of `command` carrying `key` is answered at all. A command that runs off
    /// the main thread answers a moment later, so it is waited for; a call that is refused
    /// over its key is never answered, however long that wait.
    fn answers(&self, command: &str, key: &str) -> bool {
        let request = Self::request(command, json!({}), self.origin.clone(), key);
        let (answered, answer) = std::sync::mpsc::channel::<bool>();
        let window: &tauri::Webview<MockRuntime> = self.window.as_ref();
        window.clone().on_message(
            request,
            Box::new(move |_webview, _command, response, _callback, _error| {
                answered
                    .send(matches!(response, InvokeResponse::Ok(_)))
                    .ok();
            }),
        );
        answer
            .recv_timeout(Duration::from_millis(500))
            .unwrap_or(false)
    }

    /// The folder Swarmed keeps its own files in, beside the repository.
    fn data_dir(&self) -> std::path::PathBuf {
        self.repo.with_file_name("data")
    }

    /// Calls `command` without waiting for its answer, which comes on the receiver: for one that
    /// waits on what the test does next, as a pick waits for the click.
    fn invoke_waiting(
        &self,
        command: &str,
        arguments: Value,
    ) -> std::sync::mpsc::Receiver<Result<Value, Value>> {
        let request = Self::request(command, arguments, self.origin.clone(), INVOKE_KEY);
        let (answered, answer) = std::sync::mpsc::channel();
        let window: &tauri::Webview<MockRuntime> = self.window.as_ref();
        window.clone().on_message(
            request,
            Box::new(move |_webview, _command, response, _callback, _error| {
                let value = match response {
                    InvokeResponse::Ok(body) => body
                        .deserialize::<Value>()
                        .map_err(|e| json!(e.to_string())),
                    InvokeResponse::Err(error) => Err(error.0),
                };
                answered.send(value).ok();
            }),
        );
        answer
    }

    fn ok(&self, command: &str, arguments: Value) -> Value {
        self.invoke(command, arguments)
            .unwrap_or_else(|error| panic!("`{command}` failed: {error}"))
    }

    /// Waits for the session to report an event of this type and returns it.
    fn wait_for_event(&self, id: &Value, kind: &str) -> Value {
        wait_for(self, id, kind, |e| e["type"] == kind)
    }

    /// Waits for the session's agent to say exactly `text` and returns that event.
    fn wait_for_text(&self, id: &Value, text: &str) -> Value {
        wait_for(self, id, text, |e| e["type"] == "text" && e["text"] == text)
    }
}

/// How long [`wait_for`] waits. Generous, since the whole suite runs at once: stopping a turn
/// and sending the next message took longer than two seconds under that load.
const WAIT_LIMIT: Duration = Duration::from_secs(10);

/// Asks session `id`'s agent for the account's usage, as the coins do: the answer is the
/// session's, and what it says is the account's usage for every session.
fn usage_asked(app: &App, id: &Value) {
    // Claude Code keeps no copy of the figures here, so the question goes at once.
    let ask = app.ok("session_usage_refresh", json!({ "id": id }));
    assert_eq!(ask, json!({ "waits_until": null, "copy_fetched_at": null }));
    let usage = app.wait_for_event(id, "usage_limits");
    assert_eq!(usage["limits"][0]["kind"], "session");
    let account = app.ok("account_usage", json!({}));
    assert_eq!(account["windows"][0]["kind"], "five_hour");
    assert_eq!(account["windows"][0]["used"], 0.36);
}

/// Lets session `id`, which a usage limit stops, go on by itself when the limit resets, as the
/// limit's line in the transcript does, and cancels that, as the clock in the sidebar does.
fn continued_at_reset(app: &App, id: &Value) {
    app.ok("session_send", json!({ "id": id, "text": "refuse 3600" }));
    let refused = |e: &Value| e["type"] == "turn_done" && e["result"] == "usage limit reached";
    wait_for(app, id, "the refused turn's end", refused);
    let at = app.ok("session_continue_at_reset", json!({ "id": id }));
    let set = app.wait_for_event(id, "continuation");
    assert_eq!(set["at"], at);
    let continue_at = |app: &App| {
        let tasks = &app.ok("overview", json!({}))[0]["tasks"];
        let sessions = tasks
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|task| task["sessions"].as_array().cloned().unwrap_or_default());
        let session = sessions.into_iter().find(|session| &session["id"] == id);
        session.map(|session| session["continue_at"].clone())
    };
    assert_eq!(continue_at(app), Some(at));
    app.ok("session_continue_cancel", json!({ "id": id }));
    assert_eq!(continue_at(app), Some(json!(null)));
}

/// Turns Remote Control of session `id` on, as the session's header does, and off again.
fn remote_control(app: &App, id: &Value) {
    app.ok("session_remote_control", json!({ "id": id, "on": true }));
    let on = |e: &Value| e["type"] == "remote_control" && e["state"] == "on";
    let reached = wait_for(app, id, "Remote Control's address", on);
    let url = reached["url"].as_str().unwrap_or_default();
    assert!(url.starts_with("https://claude.ai/code/"), "{reached}");
    app.ok("session_remote_control", json!({ "id": id, "on": false }));
    let off = |e: &Value| e["type"] == "remote_control" && e["state"] == "off";
    wait_for(app, id, "Remote Control off", off);
}

/// Waits until the turn in which session `id`'s agent said `text` has ended. A message sent
/// before that goes into a turn under way, which the window never does: it queues it.
fn wait_for_end_of(app: &App, id: &Value, text: &str) {
    let deadline = Instant::now() + WAIT_LIMIT;
    while Instant::now() < deadline {
        let events = app.ok("session_events", json!({ "id": id }));
        let events = events.as_array().cloned().unwrap_or_default();
        let said = events
            .iter()
            .rposition(|e| e["type"] == "text" && e["text"] == text);
        let ended = said.is_some_and(|at| events[at..].iter().any(|e| e["type"] == "turn_done"));
        if ended {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the turn of session {id} that said `{text}` never ended");
}

fn wait_for(app: &App, id: &Value, what: &str, wanted: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + WAIT_LIMIT;
    while Instant::now() < deadline {
        let events = app.ok("session_events", json!({ "id": id }));
        let found = events
            .as_array()
            .into_iter()
            .flatten()
            .rev()
            .find(|e| wanted(e));
        if let Some(event) = found {
            return event.clone();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("session {id} never reported `{what}`");
}

#[test]
fn the_ui_can_go_from_an_empty_app_to_an_answered_permission_request() {
    let app = app();
    assert_eq!(app.ok("overview", json!({})), json!([]));
    assert_eq!(app.ok("agents", json!({}))[0]["id"], "claude");

    let project = app.ok("project_add", json!({ "path": app.repo, "name": null }));
    assert_eq!(project["name"], "demo");
    git(&app.repo, &["branch", "feature/existing"]);
    let choices = app.ok("branch_choices", json!({ "project": "demo" }));
    assert_eq!(
        choices,
        json!({ "base": "main", "existing": ["feature/existing"] })
    );
    let on_branch = json!({ "project": "demo", "name": "two", "branch": "feature/existing" });
    assert_eq!(app.ok("task_new", on_branch)["branch"], "feature/existing");
    let task = app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    assert_eq!(
        (&task["name"], &task["setup"]),
        (&json!("one"), &json!("ok"))
    );
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "ask", "effort": null } }),
    );
    let id = &session["id"];

    app.ok("session_send", json!({ "id": id, "text": "run: ls" }));
    let request = app.wait_for_event(id, "permission_request");
    let overview = app.ok("overview", json!({}));
    assert_eq!(overview[0]["tasks"][1]["running"], json!([id]));
    assert_eq!(overview[0]["tasks"][1]["sessions"][0]["title"], "run: ls");

    // Each command comes with what Swarmed does with it when it is sent.
    for asked_for in [json!({ "id": id }), json!({ "id": null })] {
        let commands = app.ok("slash_commands", asked_for);
        let listed = commands.as_array().expect("a list");
        let shown = json!({ "name": "context", "kind": "screen", "note": "" });
        let sent = json!({ "name": "review", "kind": "prompt", "note": "" });
        assert!(
            listed.contains(&shown) && listed.contains(&sent),
            "{commands}"
        );
    }
    app.ok(
        "session_allow",
        json!({ "id": id, "requestId": request["request_id"] }),
    );
    let result = app.wait_for_event(id, "tool_result");
    assert_eq!(result["is_error"], false);
    app.wait_for_event(id, "turn_done");

    // The finished turn is unread until the UI says the user has seen it.
    let unread =
        |app: &App| app.ok("overview", json!({}))[0]["tasks"][1]["sessions"][0]["unread"].clone();
    assert_eq!(unread(&app), true);
    app.ok("session_mark_read", json!({ "id": id }));
    assert_eq!(unread(&app), false);
}

/// The first bytes of a PNG, which is all the core looks at, in base64.
const PNG: &str = "iVBORw0KGgo=";

#[test]
fn the_ui_can_paste_an_image_into_a_message_and_show_it_again() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "ask", "effort": null } }),
    );
    let id = &session["id"];
    let root = app.ok("attachments_root", json!({}));
    let root = Path::new(root.as_str().expect("a path"));
    let folder = root.join(id.to_string());

    let kept = app.ok("session_images_keep", json!({ "id": id, "images": [PNG] }));
    let path = kept[0].as_str().expect("a path").to_string();
    assert_eq!(Path::new(&path), folder.join("pasted-1.png"));
    let shown = app.ok("session_image", json!({ "id": id, "path": path }));
    assert_eq!(shown, json!(format!("data:image/png;base64,{PNG}")));
    // A format agents do not read is refused, saying what it is; so is reading elsewhere.
    let tiff = app.invoke(
        "session_images_keep",
        json!({ "id": id, "images": ["SUkqAA=="] }),
    );
    assert_eq!(
        tiff,
        Err(json!(
            "a TIFF image cannot be attached: agents read only PNG, JPEG, GIF and WebP images"
        ))
    );
    let readme = app.repo.join("README.md");
    for command in ["session_image", "session_image_open"] {
        let outside = app.invoke(command, json!({ "id": id, "path": readme }));
        assert!(outside.is_err(), "{command}: {outside:?}");
    }

    // Its path is attached like any file's.
    let sent = json!({ "id": id, "text": "look", "files": [path] });
    app.ok("session_send", sent);
    let message = app.wait_for_event(id, "user_message");
    assert_eq!(
        message["text"],
        format!("look\n\nAttached files:\n- {path}")
    );
    // The folder goes with the session, when it goes for good.
    app.ok("session_close", json!({ "id": id, "withWorkers": false }));
    assert!(folder.exists());
    app.ok("session_delete", json!({ "id": id }));
    assert!(!folder.exists());
}

#[test]
fn the_ui_can_show_the_image_a_tool_returned_from_the_path_on_its_result() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "auto", "effort": null } }),
    );
    let id = &session["id"];
    app.ok("session_send", json!({ "id": id, "text": "screenshot" }));
    let result = app.wait_for_event(id, "tool_result");
    assert_eq!(result["content"], "Took the viewport screenshot\n[image]");
    // The paths alone: what the image shows never reaches the window but by asking for it.
    assert_eq!(result.get("returned"), None);
    let path = result["images"][0].as_str().expect("a path").to_string();
    let shown = app.ok("session_image", json!({ "id": id, "path": path }));
    assert!(
        shown
            .as_str()
            .expect("an address")
            .starts_with("data:image/png;base64,")
    );
    let usage = app.ok("usage", json!({}));
    assert!(usage["images_bytes"].as_u64().expect("a size") > 0);
}

/// Closed, a session is still kept; deleted, it is gone for good.
fn deleted_for_good(app: &App, id: &Value) {
    assert_eq!(app.ok("overview", json!({}))[0]["tasks"][0]["closed"], 1);
    app.ok("session_delete", json!({ "id": id }));
    assert_eq!(app.ok("overview", json!({}))[0]["tasks"][0]["closed"], 0);
    assert!(app.invoke("session_events", json!({ "id": id })).is_err());
}

/// Calls the commands of a Claude Code opened in the session's terminal, as the UI does. The
/// fake agent's screen is `echo`, which ends by itself: typed into, it may be gone already.
fn claude_code_in_a_terminal(app: &App, id: &Value, size: &Value) {
    let mut open = size.clone();
    open["command"] = json!("vim");
    let opened = app.ok("claude_terminal_open", open.clone());
    assert_eq!((&opened["replay"], &opened["end"]), (&json!(""), &json!(0)));
    let gone = Err(json!(format!("session {id} has no terminal open")));
    for (command, arguments) in [
        ("claude_terminal_write", json!({ "id": id, "data": "q" })),
        ("claude_terminal_resize", size.clone()),
    ] {
        let answer = app.invoke(command, arguments);
        assert!(answer.is_ok() || answer == gone, "{command}: {answer:?}");
    }
    app.ok("claude_terminal_close", json!({ "id": id }));
    // One Swarmed refuses is not opened this way either.
    open["command"] = json!("cd");
    assert_eq!(
        app.invoke("claude_terminal_open", open),
        Err(json!(
            "/cd is not opened in Claude Code's own terminal from Swarmed"
        ))
    );
}

#[test]
fn the_ui_can_put_projects_tasks_and_sessions_in_order() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    for name in ["one", "two"] {
        let task = json!({ "project": "demo", "name": name, "branch": null });
        app.ok("task_new", task);
    }
    let tasks = |app: &App| -> Vec<Value> {
        let overview = app.ok("overview", json!({}));
        let tasks = overview[0]["tasks"].as_array().cloned().unwrap_or_default();
        tasks.iter().map(|task| task["name"].clone()).collect()
    };
    app.ok(
        "task_move",
        json!({ "project": "demo", "task": "two", "by": -1 }),
    );
    assert_eq!(tasks(&app), [json!("two"), json!("one")]);
    // One repository has nowhere to go, which is no error.
    app.ok("project_move", json!({ "project": "demo", "by": 1 }));

    let how = json!({ "model": null, "supervision": "auto", "effort": null });
    let new = |app: &App| {
        let options = json!({ "project": "demo", "task": "one", "agent": null, "how": how });
        app.ok("session_new", options)["id"].clone()
    };
    let (first, second) = (new(&app), new(&app));
    app.ok("session_move", json!({ "id": second, "by": -1 }));
    let overview = app.ok("overview", json!({}));
    let one = &overview[0]["tasks"][1]["sessions"];
    assert_eq!((&one[0]["id"], &one[1]["id"]), (&second, &first));
    app.invoke("cloud_task_move", json!({ "id": 999, "by": 1 }))
        .expect_err("no such cloud task");
}

#[test]
fn the_ui_can_pin_projects_tasks_and_sessions() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    for name in ["one", "two"] {
        let task = json!({ "project": "demo", "name": name, "branch": null });
        app.ok("task_new", task);
    }
    app.ok("project_pin", json!({ "project": "demo", "pinned": true }));
    app.ok(
        "task_pin",
        json!({ "project": "demo", "task": "two", "pinned": true }),
    );
    let how = json!({ "model": null, "supervision": "auto", "effort": null });
    let options = json!({ "project": "demo", "task": "two", "agent": null, "how": how });
    let (first, second) = (
        app.ok("session_new", options.clone())["id"].clone(),
        app.ok("session_new", options)["id"].clone(),
    );
    app.ok("session_pin", json!({ "id": second, "pinned": true }));
    let overview = app.ok("overview", json!({}));
    let demo = &overview[0];
    assert_eq!(demo["pinned"], json!(true));
    let two = &demo["tasks"][0];
    assert_eq!(
        (&two["name"], &two["pinned"]),
        (&json!("two"), &json!(true))
    );
    let sessions = &two["sessions"];
    assert_eq!((&sessions[0]["id"], &sessions[1]["id"]), (&second, &first));
    assert_eq!(sessions[0]["pinned"], json!(true));
    app.ok("session_pin", json!({ "id": second, "pinned": false }));
    app.invoke("session_pin", json!({ "id": 999, "pinned": true }))
        .expect_err("no such session");
}

#[test]
fn the_ui_can_highlight_tasks_and_sessions_where_they_are() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let how = json!({ "model": null, "supervision": "auto", "effort": null });
    let options = json!({ "project": "demo", "task": "one", "agent": null, "how": how });
    let (first, second) = (
        app.ok("session_new", options.clone())["id"].clone(),
        app.ok("session_new", options)["id"].clone(),
    );
    app.ok(
        "task_highlight",
        json!({ "project": "demo", "task": "one", "colour": "purple" }),
    );
    app.ok(
        "session_highlight",
        json!({ "id": second, "colour": "blue" }),
    );
    let one = &app.ok("overview", json!({}))[0]["tasks"][0];
    assert_eq!(one["highlight"], json!("purple"));
    let sessions = &one["sessions"];
    // Highlighted, it stays where it was.
    assert_eq!((&sessions[0]["id"], &sessions[1]["id"]), (&first, &second));
    assert_eq!(
        (&sessions[0]["highlight"], &sessions[1]["highlight"]),
        (&json!(null), &json!("blue"))
    );
    app.ok("session_highlight", json!({ "id": second, "colour": null }));
    app.ok(
        "task_highlight",
        json!({ "project": "demo", "task": "one", "colour": "none" }),
    );
    let one = &app.ok("overview", json!({}))[0]["tasks"][0];
    assert_eq!(
        (&one["highlight"], &one["sessions"][1]["highlight"]),
        (&json!(null), &json!(null))
    );
    app.invoke(
        "session_highlight",
        json!({ "id": second, "colour": "teal" }),
    )
    .expect_err("not a colour");
    app.invoke("session_highlight", json!({ "id": 999, "colour": "red" }))
        .expect_err("no such session");
}

#[test]
fn the_ui_can_have_a_session_work_for_another_across_tasks() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    for name in ["one", "two"] {
        app.ok(
            "task_new",
            json!({ "project": "demo", "name": name, "branch": null }),
        );
    }
    let how = json!({ "model": null, "supervision": "auto", "effort": null });
    let new = |task: &str| {
        let options = json!({ "project": "demo", "task": task, "agent": null, "how": how });
        app.ok("session_new", options)["id"].clone()
    };
    let (planner, implementer) = (new("one"), new("two"));
    let set = app.ok(
        "session_set_parent",
        json!({ "id": implementer, "parent": planner }),
    );
    assert_eq!(set["parent"], planner);
    let two = &app.ok("overview", json!({}))[0]["tasks"][1];
    assert_eq!(two["sessions"][0]["parent"], planner);
    assert_eq!(two["sessions"][0]["spawned"], json!(false));
    app.invoke(
        "session_set_parent",
        json!({ "id": planner, "parent": implementer }),
    )
    .expect_err("a session cannot work for one that works for it");
    let unset = app.ok(
        "session_set_parent",
        json!({ "id": implementer, "parent": null }),
    );
    assert_eq!(unset["parent"], json!(null));
}

#[test]
fn the_ui_can_close_a_session_with_those_that_work_for_it_and_put_a_task_away_with_them() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    for name in ["one", "two"] {
        app.ok(
            "task_new",
            json!({ "project": "demo", "name": name, "branch": null }),
        );
    }
    let how = json!({ "model": null, "supervision": "auto", "effort": null });
    let new = |task: &str| {
        let options = json!({ "project": "demo", "task": task, "agent": null, "how": how });
        app.ok("session_new", options)["id"].clone()
    };
    // An architect in one, a planner for it in two, an implementer for that in one.
    let (architect, planner, implementer) = (new("one"), new("two"), new("one"));
    for (id, parent) in [(&planner, &architect), (&implementer, &planner)] {
        app.ok("session_set_parent", json!({ "id": id, "parent": parent }));
    }

    let under = app.ok(
        "session_descendants",
        json!({ "id": architect, "closed": false }),
    );
    let listed: Vec<_> = under
        .as_array()
        .expect("a list")
        .iter()
        .map(|one| {
            (
                one["session"]["id"].clone(),
                one["depth"].clone(),
                one["closed"].clone(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (planner.clone(), json!(1), json!(false)),
            (implementer.clone(), json!(2), json!(false))
        ]
    );
    let elsewhere = app.ok(
        "task_descendants_elsewhere",
        json!({ "project": "demo", "name": "one" }),
    );
    assert_eq!(elsewhere[0]["session"]["id"], planner);
    assert_eq!(elsewhere.as_array().map(Vec::len), Some(1));

    // Closed alone, the planner leaves the implementer open; with them, the deepest first.
    let closed = app.ok(
        "session_close",
        json!({ "id": planner, "withWorkers": false }),
    );
    assert_eq!(closed, json!([planner]));
    let left = app.ok(
        "session_descendants",
        json!({ "id": architect, "closed": true }),
    );
    assert_eq!(left[0]["closed"], json!(true));
    assert_eq!(left[1]["session"]["id"], implementer);
    let closed = app.ok(
        "session_close",
        json!({ "id": architect, "withWorkers": true }),
    );
    assert_eq!(closed, json!([implementer, architect]));

    let done = json!({ "project": "demo", "name": "two", "closeWorkers": true });
    assert_eq!(app.ok("task_done", done), json!([]));
}

#[test]
fn the_ui_can_give_a_session_a_role_at_its_start_and_after() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let roles = app.ok("roles", json!({}));
    let names: Vec<&str> = roles
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|role| role["name"].as_str())
        .collect();
    assert_eq!(names, ["architect", "planner", "implementer", "reviewer"]);
    assert_eq!(roles[1]["starts"], json!(["implementer", "reviewer"]));
    assert_eq!(roles[1]["child"], "implementer");
    assert_eq!(roles[1]["changes_files"], false);
    assert_eq!(roles[1]["approved_supervision"], "auto");
    assert_eq!(roles[3]["approved_supervision"], json!(null));
    assert!(roles[1]["description"].is_string());

    let how = json!({ "model": null, "supervision": "auto", "effort": null });
    let options = json!({
        "project": "demo", "task": "one", "agent": null, "role": "planner", "how": how,
    });
    let session = app.ok("session_new", options);
    assert_eq!(session["role"], "planner");
    let id = &session["id"];
    let changed = app.ok("session_set_role", json!({ "id": id, "role": "reviewer" }));
    assert_eq!(changed["role"], "reviewer");
    let tree = app.ok("overview", json!({}));
    assert_eq!(tree[0]["tasks"][0]["sessions"][0]["role"], "reviewer");
    app.invoke("session_set_role", json!({ "id": id, "role": "boss" }))
        .expect_err("no such role");
    let off = app.ok("session_set_role", json!({ "id": id, "role": null }));
    assert_eq!(off["role"], json!(null));

    // A task's first session is given one as it starts.
    let start = json!({
        "branch": null, "environment": null, "agent": null, "role": "architect",
        "how": { "model": null, "supervision": "plan", "effort": null, "fast": false },
        "title": null, "prompt": "Shape the export", "files": [], "images": [],
    });
    let started = app.ok(
        "task_start",
        json!({ "project": "demo", "name": "export", "start": start }),
    );
    assert_eq!(started["session"]["role"], "architect");
}

#[test]
fn the_ui_can_put_a_task_away_and_bring_it_back() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let which = json!({ "project": "demo", "name": "one" });
    let done = json!({ "project": "demo", "name": "one", "closeWorkers": false });
    assert_eq!(app.ok("task_done", done), json!([]));
    let one = &app.ok("overview", json!({}))[0]["tasks"][0];
    assert!(one["done_at"].is_i64(), "{one}");
    assert_eq!(one["done_why"], json!("hand"));
    app.ok("task_undone", which);
    let one = &app.ok("overview", json!({}))[0]["tasks"][0];
    assert_eq!(
        (&one["done_at"], &one["done_why"]),
        (&json!(null), &json!(null))
    );
    app.invoke(
        "task_done",
        json!({ "project": "demo", "name": "nope", "closeWorkers": false }),
    )
    .expect_err("no such task");
}

#[test]
fn the_ui_can_deny_interrupt_and_remove() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "auto", "effort": "high" } }),
    );
    let id = &session["id"];
    assert_eq!(session["supervision"], "auto");
    // Before anything is said the agent can still be chosen; one that is not set up cannot.
    let same = app.ok("session_set_agent", json!({ "id": id, "agent": "claude" }));
    assert_eq!(same["agent"], "claude");
    let unknown = json!({ "id": id, "agent": "nope" });
    app.invoke("session_set_agent", unknown)
        .expect_err("not an agent");

    app.ok("session_send", json!({ "id": id, "text": "run: rm -rf /" }));
    let request = app.wait_for_event(id, "permission_request");
    let deny = json!({ "id": id, "requestId": request["request_id"], "reason": "no" });
    app.ok("session_deny", deny);
    assert_eq!(app.wait_for_event(id, "tool_result")["is_error"], true);
    app.wait_for_event(id, "turn_done");

    app.ok("session_send", json!({ "id": id, "text": "slow" }));
    app.wait_for_event(id, "text");
    assert_eq!(session["effort"], "high");
    let how = json!({ "model": "opus", "supervision": "ask", "effort": "high", "fast": true });
    let settings = json!({ "id": id, "how": how });
    let changed = app.ok("session_configure", settings);
    assert_eq!(
        (&changed["model"], &changed["supervision"], &changed["fast"]),
        (&json!("opus"), &json!("ask"), &json!(true))
    );
    app.ok("session_interrupt", json!({ "id": id }));
    let stopped = |e: &Value| e["type"] == "turn_done" && e["is_error"] == true;
    wait_for(&app, id, "the interrupted turn's end", stopped);
    assert_eq!(app.ok("status_line", json!({ "id": id })), json!(null));
    usage_asked(&app, id);
    remote_control(&app, id);
    continued_at_reset(&app, id);
    let size = json!({ "id": id, "cols": 80, "rows": 24 });
    assert_eq!(
        app.ok("terminal_open", size.clone()),
        json!({ "replay": "", "end": 0, "run": 0 })
    );
    app.ok("terminal_write", json!({ "id": id, "data": "true\n" }));
    app.ok("terminal_resize", size.clone());
    claude_code_in_a_terminal(&app, id, &size);
    let renamed = app.ok(
        "session_rename",
        json!({ "id": id, "title": "  Slow test  " }),
    );
    assert_eq!(renamed["title"], "Slow test");
    assert_eq!(app.wait_for_event(id, "renamed")["title"], "Slow test");
    assert_eq!(
        app.invoke("session_rename", json!({ "id": id, "title": " " })),
        Err(json!("a session title cannot be empty"))
    );
    app.ok("session_close", json!({ "id": id, "withWorkers": false }));
    assert_eq!(
        app.ok("overview", json!({}))[0]["tasks"][0]["sessions"],
        json!([])
    );
    deleted_for_good(&app, id);
    // The UI has no command that closes a terminal: it goes with its session.
    assert_eq!(
        app.invoke("terminal_write", json!({ "id": id, "data": "ls\n" })),
        Err(json!(format!("session {id} has no terminal open")))
    );

    let remove = json!({ "project": "demo", "name": "one", "force": false, "deleteBranch": true });
    app.ok("task_remove", remove);
    assert_eq!(
        app.ok("overview", json!({}))[0]["tasks"][0]["archived"],
        true
    );
}

#[test]
fn the_ui_can_start_a_task_from_a_prompt_and_act_on_its_setup() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    // A setup that fails until the worktree has a file called `go`.
    let config = app.data_dir().join("projects").join("demo.toml");
    std::fs::write(&config, "[hooks]\nsetup = \"echo checking; test -f go\"\n").expect("config");
    let start = json!({
        "branch": null, "environment": null, "agent": null,
        "how": { "model": null, "supervision": "auto", "effort": null, "fast": false },
        "title": "Fix the login", "prompt": "fix the login", "files": [], "images": [PNG],
    });
    let started = app.ok(
        "task_start",
        json!({ "project": "demo", "name": "login", "start": start }),
    );
    let (task, session) = (&started["task"], &started["session"]);
    assert_eq!(task["name"], "login");
    assert_eq!(session["title"], "Fix the login");
    let target = json!({ "project": "demo", "name": "login" });
    let deadline = Instant::now() + WAIT_LIMIT;
    while app.ok("overview", json!({}))[0]["tasks"][0]["setup"] != "failed" {
        assert!(Instant::now() < deadline, "the setup never failed");
        std::thread::sleep(Duration::from_millis(10));
    }
    let failed = app.ok("overview", json!({}))[0]["tasks"][0].clone();
    assert!(
        failed["setup_reason"]
            .as_str()
            .is_some_and(|why| why.contains("exited with status 1"))
    );
    assert_eq!(failed["setting_up"], json!(null));
    assert_eq!(failed["sessions"][0]["waits_for_setup"], true);
    let log = app.ok("task_setup_log", target.clone());
    assert!(
        log["text"]
            .as_str()
            .is_some_and(|text| text.starts_with("checking\n"))
    );
    // Nothing runs to be stopped.
    app.ok("task_setup_stop", target.clone());

    // Set up again, it finishes well and sends what waited.
    let worktree = Path::new(task["worktree_path"].as_str().expect("a path"));
    std::fs::write(worktree.join("go"), "").expect("writes");
    assert_eq!(app.ok("task_setup", target.clone())["setup"], "ok");
    let id = &session["id"];
    let sent = app.wait_for_event(id, "user_message");
    // The image pasted into the prompt is the session's own, attached as a file.
    let image = app
        .data_dir()
        .join(format!("attachments/{id}/pasted-1.png"));
    assert_eq!(
        sent["text"],
        format!("fix the login\n\nAttached files:\n- {}", image.display())
    );
    let nothing = app
        .invoke("task_start_anyway", target)
        .expect_err("nothing waits");
    assert_eq!(
        nothing,
        json!("nothing of `login`'s sessions waits for its setup")
    );
}

#[test]
fn the_ui_can_list_issues_and_start_a_task_for_one() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    // The test repository has no remote: whether `gh` is installed or not, the list is empty
    // and says why, rather than failing. The filter is the one the window sends.
    let filter = json!({ "who": "assigned", "state": "open", "search": "login", "limit": 50 });
    let listed = app.ok(
        "project_issues",
        json!({ "project": "demo", "filter": filter }),
    );
    assert_eq!(listed["issues"], json!([]));
    assert!(listed["unavailable"].is_string(), "{listed}");
    let unread = app
        .invoke("project_issue", json!({ "project": "demo", "number": 12 }))
        .expect_err("nothing on GitHub to read");
    assert!(!unread.to_string().contains("invalid args"), "{unread}");

    // A task started for an issue records it, and the tree says so.
    let start = json!({
        "branch": null, "environment": null, "agent": null,
        "how": { "model": null, "supervision": "auto", "effort": null, "fast": false },
        "title": null, "prompt": "Work on issue #12", "files": [], "images": [], "issue": 12,
    });
    let started = app.ok(
        "task_start",
        json!({ "project": "demo", "name": "login", "start": start }),
    );
    assert_eq!(started["task"]["issue"], 12);
    assert_eq!(app.ok("overview", json!({}))[0]["tasks"][0]["issue"], 12);
    let target = json!({ "project": "demo", "name": "login" });
    let issue = app.invoke("task_issue", target).expect_err("not on GitHub");
    assert!(!issue.to_string().contains("invalid args"), "{issue}");
    // A task started for none has none, and nothing is asked.
    let task = json!({ "project": "demo", "name": "other", "branch": null });
    assert_eq!(app.ok("task_new", task)["issue"], json!(null));
    let other = json!({ "project": "demo", "name": "other" });
    assert_eq!(app.ok("task_issue", other), json!(null));
}

#[test]
fn the_ui_can_commit_a_tasks_work_and_is_told_what_cannot_be_published() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let made = app.ok("overview", json!({}))[0]["tasks"][0].clone();
    let branch = made["branch"].clone();
    // Never pushed, on the task's own branch, the only one its worktree has been on.
    let unpublished = |changed: u32, unpushed: u32| {
        json!({
            "changed": changed, "unpushed": unpushed, "pushed_before": false,
            "branch": branch, "branches": [branch], "rebasing": false,
        })
    };
    let worktree = Path::new(made["worktree_path"].as_str().expect("a path"));
    std::fs::write(worktree.join("notes.txt"), "one\n").expect("writes");
    let target = json!({ "project": "demo", "name": "one" });
    // A branch that was never pushed has no pull request, and opening it says so.
    assert_eq!(app.ok("task_pull", target.clone()), json!(null));
    let opening = json!({ "project": "demo", "name": "one", "check": null });
    let refused = app
        .invoke("task_pull_open", opening)
        .expect_err("nothing to open");
    assert_eq!(refused, json!("`one` has no pull request"));
    let failure = app
        .invoke("task_pull_failure", target.clone())
        .expect_err("no checks to send");
    assert_eq!(failure, json!("`one` has no pull request"));
    // The file written above is not committed yet; committing it leaves one commit to push.
    assert_eq!(
        app.ok("task_unpublished", target.clone()),
        unpublished(1, 0)
    );
    // No session has run, so nothing is known of what an agent's default effort comes to.
    let effort = json!({ "agent": "claude", "model": null });
    assert_eq!(app.ok("effort_default", effort), json!(null));
    // Nothing in this app can write: the answer is a reason, not a hang.
    let unwritten = app
        .invoke(
            "task_commit_message",
            json!({ "project": "demo", "name": "one", "session": null }),
        )
        .expect_err("no namer");
    assert_eq!(unwritten, json!("nothing here can write a commit message"));
    // A pull request is described from the branch's commits, and there are none yet.
    let describe = json!({ "project": "demo", "name": "one", "session": null });
    let nothing = app.invoke("task_pull_message", describe.clone());
    assert_eq!(
        nothing.expect_err("no commits"),
        json!("there is nothing to describe yet: the branch has no commits of its own")
    );
    let commit = json!({ "project": "demo", "name": "one", "message": "feat: notes" });
    assert_eq!(app.ok("task_commit", commit), json!(null));
    assert_eq!(
        app.ok("task_unpublished", target.clone()),
        unpublished(0, 1)
    );
    assert_eq!(
        app.ok("task_pull_draft", target.clone()),
        json!({ "title": "feat: notes", "body": "" })
    );
    // With a commit to describe, it comes down to the same as the commit message: no writer.
    let unwritten = app.invoke("task_pull_message", describe);
    assert_eq!(
        unwritten.expect_err("no namer"),
        json!("nothing here can write a pull request")
    );
    // The worktree is on the task's own branch, so putting it there changes nothing.
    let own = json!({ "project": "demo", "name": "one", "branch": branch });
    assert_eq!(app.ok("task_switch", own), json!(null));
    // The test repository has no remote: pushing and opening a pull request both say why not.
    app.invoke("task_push", target.clone())
        .expect_err("nowhere to push to");
    let create = json!({
        "project": "demo", "name": "one", "asDraft": true,
        "draft": { "title": " ", "body": "" },
    });
    let untitled = app.invoke("task_pull_create", create).expect_err("refused");
    assert_eq!(untitled, json!("a pull request needs a title"));
}

#[test]
fn the_ui_can_ask_to_merge_a_tasks_pull_request() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let target = json!({ "project": "demo", "name": "one" });
    // With no remote there is nothing on GitHub to merge, whether `gh` is installed or not;
    // the arguments are the ones the window sends.
    let merge = json!({
        "project": "demo", "name": "one", "method": "squash", "when": "now",
        "head": "0123456789abcdef",
    });
    let unmerged = app
        .invoke("task_pull_merge", merge)
        .expect_err("no pull request");
    assert!(!unmerged.to_string().contains("invalid args"), "{unmerged}");
    let off = app.invoke("task_pull_auto_merge_off", target.clone());
    let off = off.expect_err("no pull request");
    assert!(!off.to_string().contains("invalid args"), "{off}");
    let rules = app.invoke("project_merge_rules", json!({ "project": "demo" }));
    let rules = rules.expect_err("no repository on GitHub");
    assert!(!rules.to_string().contains("invalid args"), "{rules}");
}

/// A task's files are listed folder by folder, and nothing above its worktree is.
fn files_are_listed_within_the_worktree(app: &App) {
    let top = app.ok(
        "task_files",
        json!({ "project": "demo", "name": "one", "dir": "" }),
    );
    let listed = top["entries"].as_array().map_or(0, Vec::len);
    assert!(listed > 0, "{top}");
    let above = json!({ "project": "demo", "name": "one", "dir": "../.." });
    assert!(app.invoke("task_files", above).is_err());
    let missing = json!({ "project": "demo", "name": "one", "path": "no/such.file" });
    assert!(app.invoke("task_open_file", missing).is_err());
}

#[test]
fn a_settings_file_with_a_key_the_app_does_not_know_still_opens_and_says_so() {
    let app = app();
    std::fs::create_dir_all(app.data_dir()).expect("data dir");
    let written = "editor = \"zed\"\ncoming_later = true\n";
    std::fs::write(app.data_dir().join("settings.toml"), written).expect("writes");
    assert_eq!(app.ok("settings", json!({}))["editor"], json!("zed"));
    let problems = app.ok("config_problems", json!({}));
    let said = problems[0].as_str().expect("one problem, in words");
    assert!(said.starts_with("settings.toml: `coming_later`"), "{said}");
    assert_eq!(problems.as_array().map(Vec::len), Some(1));
}

#[test]
fn a_layout_with_a_key_the_app_does_not_know_is_drawn_and_says_so() {
    let app = app();
    std::fs::create_dir_all(app.data_dir()).expect("data dir");
    let written = r#"{ "icons": 120, "sidebar": { "task": { "size": 12, "italic": true } } }"#;
    std::fs::write(app.data_dir().join("layout.json"), written).expect("writes");
    let kept = json!({ "base": "classic", "text": {}, "icons": 120,
        "sidebar": { "task": { "size": 12 } } });
    assert_eq!(app.ok("custom_layout", json!({})), kept);
    let problems = app.ok("config_problems", json!({}));
    let said = problems[0].as_str().expect("one problem, in words");
    assert!(
        said.starts_with("layout.json: `sidebar.task.italic`"),
        "{said}"
    );
    // Saved, a layout is held to what this version knows.
    let refused = app.invoke("custom_layout_save", json!({ "text": written }));
    let said = refused.expect_err("refused");
    assert!(
        said.as_str().is_some_and(|said| said.contains("italic")),
        "{said}"
    );
    let level =
        json!({ "text": "{ \"sidebar\": { \"session\": { \"size\": 15, \"weight\": 600 } } }" });
    let saved = app.ok("custom_layout_save", level);
    assert_eq!(
        saved["sidebar"]["session"],
        json!({ "size": 15, "weight": 600 })
    );
}

#[test]
fn the_ui_can_choose_how_often_usage_warnings_are_said() {
    let app = app();
    let mut settings = app.ok("settings", json!({}));
    assert_eq!(settings["limit_warnings"], "frequent");
    settings["limit_warnings"] = json!("none");
    app.ok("set_settings", json!({ "settings": settings }));
    assert_eq!(app.ok("settings", json!({}))["limit_warnings"], "none");
}

#[test]
fn the_ui_can_set_a_branch_prefix_and_see_the_branch_it_gives() {
    let app = app();
    let shown = app.ok("branch_example", json!({ "prefix": "/jd/feat/" }));
    assert_eq!(shown, "jd/feat/fix-login");
    let refused = app.invoke("branch_example", json!({ "prefix": "a b" }));
    let said = refused.expect_err("refused");
    let reason = |said: &str| said.contains("'a b/fix-login' is not a valid branch name");
    assert!(said.as_str().is_some_and(reason), "{said}");

    let mut settings = app.ok("settings", json!({}));
    settings["branch_prefix"] = json!("a..b");
    let refused = app.invoke("set_settings", json!({ "settings": settings.clone() }));
    assert!(refused.is_err(), "{refused:?}");
    settings["branch_prefix"] = json!("jd/");
    app.ok("set_settings", json!({ "settings": settings }));
    assert_eq!(app.ok("settings", json!({}))["branch_prefix"], "jd");

    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    assert_eq!(app.ok("task_new", task)["branch"], "jd/one");
}

#[test]
fn the_ui_can_list_a_sessions_turns_and_read_what_one_changed() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "auto", "effort": null } }),
    );
    let id = &session["id"];
    app.ok(
        "session_send",
        json!({ "id": id, "text": "write: notes.txt = one" }),
    );
    app.wait_for_event(id, "turn_done");

    let turns = app.ok("session_turns", json!({ "id": id }));
    assert_eq!(turns[0]["message"], 0);
    assert_eq!(turns[0]["words"], "write: notes.txt = one");
    assert_eq!(turns[0]["ended"], true);
    assert_eq!(turns[0]["beside"], json!([]));
    let changes = app.ok("session_turn_changes", json!({ "id": id, "message": 0 }));
    assert_eq!(
        changes,
        json!({
            "files": [{ "path": "notes.txt", "kind": "added", "added": 1, "removed": 0 }],
            "skipped": [],
            "ended": true,
        })
    );
    let file = json!({ "id": id, "message": 0, "path": "notes.txt" });
    let diff = app.ok("session_turn_diff", file);
    assert!(
        diff["text"]
            .as_str()
            .is_some_and(|text| text.ends_with("+one\n")),
        "{diff}"
    );
}

#[test]
fn the_ui_can_take_a_session_back_with_its_files_and_put_them_back() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let worktree = std::path::PathBuf::from(task["worktree_path"].as_str().expect("a path"));
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "auto", "effort": null } }),
    );
    let id = &session["id"];
    for (ended, text) in ["write: notes.txt = one", "write: notes.txt = two"]
        .into_iter()
        .enumerate()
    {
        app.ok("session_send", json!({ "id": id, "text": text }));
        let turns = |e: &Value| {
            e.as_array()
                .map(|all| all.iter().filter(|e| e["type"] == "turn_done").count())
        };
        let deadline = Instant::now() + WAIT_LIMIT;
        while turns(&app.ok("session_events", json!({ "id": id }))) != Some(ended + 1) {
            assert!(Instant::now() < deadline, "turn {ended} never ended");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    let check = app.ok("session_rewind_check", json!({ "id": id, "message": 1 }));
    assert_eq!(check["refused"], json!(null));
    assert_eq!(check["files_refused"], json!(null));
    assert_eq!(check["resumes"], true);
    assert_eq!(
        check["files"],
        json!([{ "path": "notes.txt", "kind": "modified", "added": 1, "removed": 1 }])
    );
    let done = app.ok(
        "session_rewind",
        json!({ "id": id, "message": 1, "files": true }),
    );
    assert_eq!(
        done,
        json!({ "to": 1, "files": true, "resumes": true, "commits_since": 0 })
    );
    let notes = || std::fs::read_to_string(worktree.join("notes.txt")).expect("reads");
    assert_eq!(notes(), "one\n");
    let events = app.ok("session_events", json!({ "id": id }));
    let last = events
        .as_array()
        .and_then(|events| events.last())
        .expect("one");
    assert_eq!(last["type"], "rewound");
    assert_eq!(last["to"], 1);

    app.ok("session_files_put_back", json!({ "id": id }));
    assert_eq!(notes(), "two\n");
    let again = app.invoke("session_files_put_back", json!({ "id": id }));
    assert!(again.is_err(), "{again:?}");
}

#[test]
fn the_ui_can_choose_the_editor_and_is_told_what_cannot_be_opened() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    assert_eq!(
        app.ok("overview", json!({}))[0]["tasks"][0]["url"],
        json!(null)
    );
    assert_eq!(
        app.ok("settings", json!({})),
        json!({
            "editor": "code", "terminal_app": "Terminal", "auto_title": true, "theme": "system", "palette": "swarm",
            "style": "colony", "layout": "classic",
            "text_scale": 100,
            "html_views": true, "tool_calls": "folded", "turn_end_lines": true, "math": "standard", "limit_warnings": "frequent", "continue_after_limit": false, "notifications": true, "default_supervision": "auto",
            "max_workers": 6,
            "status_items": ["branch", "changes", "model", "context", "cost"],
            "status_command": "", "rules": "", "updates": "automatic", "branch_prefix": "",
            "put_away_merged": false, "put_away_merged_days": 1, "put_away_idle": false,
            "put_away_idle_days": 14, "put_away_closes_sessions": false,
            "browser_program": "", "browser_idle_minutes": 30, "browser_display": "pane",
        })
    );

    let worktree = app.ok("overview", json!({}))[0]["tasks"][0]["worktree_path"].clone();
    let worktree = Path::new(worktree.as_str().expect("a path"));
    std::fs::write(worktree.join("notes.txt"), "one\n").expect("writes");
    let target = json!({ "project": "demo", "name": "one" });
    let added = json!([{ "path": "notes.txt", "kind": "added", "added": 1, "removed": 0 }]);
    assert_eq!(app.ok("task_changes", target), added);
    let file = json!({ "project": "demo", "name": "one", "path": "notes.txt" });
    let diff = app.ok("task_diff", file);
    assert_eq!(diff["truncated"], false);
    assert!(
        diff["text"]
            .as_str()
            .is_some_and(|text| text.ends_with("+one\n"))
    );

    let chosen = json!({ "settings": {
        "editor": "swarmed-no-such-editor", "terminal_app": "Swarmed No Such Terminal", "auto_title": false, "theme": "light",
        "palette": "plum", "style": "plain", "layout": "senior", "text_scale": 110,
        "html_views": false, "notifications": false, "default_supervision": "ask", "status_items": ["cost"], "status_command": "",
        "rules": "Keep answers short.",
    } });
    app.ok("set_settings", chosen);
    assert_eq!(
        app.ok("custom_layout", json!({})),
        json!({ "base": "classic", "text": {} })
    );
    let written = json!({ "text": "{ \"base\": \"senior\", \"text\": { \"sm\": 17 } }" });
    let kept = json!({ "base": "senior", "text": { "sm": 17 } });
    assert_eq!(app.ok("custom_layout_save", written), kept);
    assert_eq!(app.ok("custom_layout", json!({})), kept);
    assert_eq!(app.ok("terminal_config", json!({})), json!({}));
    let written = json!({ "text": "{ \"cursor\": \"bar\", \"font_size\": 14 }" });
    let kept = json!({ "cursor": "bar", "font_size": 14 });
    assert_eq!(app.ok("terminal_config_save", written), kept);
    assert_eq!(app.ok("terminal_config", json!({})), kept);
    let target = json!({ "project": "demo", "name": "one" });
    files_are_listed_within_the_worktree(&app);
    let no_terminal = app.invoke("task_open_terminal", target.clone());
    let said = no_terminal.expect_err("no such terminal");
    let named = |m: &str| m.starts_with("could not open Swarmed No Such Terminal");
    assert!(said.as_str().is_some_and(named), "{said}");
    let failed = app.invoke("task_open_editor", target.clone());
    let message = failed.expect_err("no such editor");
    assert!(
        message
            .as_str()
            .is_some_and(|m| m.starts_with("could not open swarmed-no-such-editor")),
        "{message}"
    );
    assert_eq!(
        app.invoke("task_open_url", target),
        Err(json!("project `demo` has no `open_url` in its config"))
    );
}

#[test]
fn the_ui_sets_how_many_sessions_may_work_for_one_within_what_the_core_takes() {
    let app = app();
    assert_eq!(app.ok("settings", json!({}))["max_workers"], 6);
    let mut settings = app.ok("settings", json!({}));
    settings["max_workers"] = json!(9);
    app.ok("set_settings", json!({ "settings": settings.clone() }));
    assert_eq!(app.ok("settings", json!({}))["max_workers"], 9);
    settings["max_workers"] = json!(21);
    assert_eq!(
        app.invoke("set_settings", json!({ "settings": settings })),
        Err(json!(
            "how many open sessions may work for one session is from 1 to 20, not 21"
        ))
    );
    assert_eq!(app.ok("settings", json!({}))["max_workers"], 9);
}

#[test]
fn the_ui_sets_a_role_s_default_model_and_effort_and_is_told_what_no_agent_offers() {
    let app = app();
    let mut settings = app.ok("settings", json!({}));
    // None yet, and none is left out.
    assert_eq!(settings.get("role_defaults"), None);
    let reviewer = json!({ "reviewer": { "model": "fable", "effort": "medium" } });
    settings["role_defaults"] = reviewer.clone();
    app.ok("set_settings", json!({ "settings": settings.clone() }));
    assert_eq!(app.ok("settings", json!({}))["role_defaults"], reviewer);

    // As the window sends one it cleared: a model left out, the effort kept.
    settings["role_defaults"] = json!({ "reviewer": { "model": null, "effort": "low" } });
    app.ok("set_settings", json!({ "settings": settings.clone() }));
    assert_eq!(
        app.ok("settings", json!({}))["role_defaults"],
        json!({ "reviewer": { "effort": "low" } })
    );

    settings["role_defaults"] = json!({ "reviewer": { "model": "gpt-x" } });
    let refused = app.invoke("set_settings", json!({ "settings": settings.clone() }));
    let said = refused.expect_err("refused");
    let said = said.as_str().expect("words");
    assert!(
        said.starts_with("no agent offers the model `gpt-x` chosen for the reviewer's sessions"),
        "{said}"
    );
    settings["role_defaults"] = json!({ "critic": { "effort": "low" } });
    let refused = app.invoke("set_settings", json!({ "settings": settings }));
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(
        app.ok("settings", json!({}))["role_defaults"],
        json!({ "reviewer": { "effort": "low" } })
    );
}

#[test]
fn the_ui_can_list_what_was_left_running_and_cannot_stop_what_no_agent_runs() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let target = json!({ "project": "demo", "name": "one" });
    assert_eq!(app.invoke("task_background", target), Ok(json!([])));
    let stop = json!({ "id": 999, "taskId": "b1" });
    assert!(app.invoke("session_stop_background", stop).is_err());
    let send = json!({ "id": 999, "toolUseId": "toolu_1" });
    assert!(app.invoke("session_send_to_background", send).is_err());
    // Output is read only of what a session's own agent named: nothing here did.
    let output = json!({ "id": 999, "taskId": "b1", "from": null });
    assert!(app.invoke("session_background_output", output).is_err());
}

#[test]
fn nothing_is_known_of_updates_until_asked_and_a_build_from_the_sources_is_not_updated() {
    let app = app();
    assert_eq!(
        app.invoke("update_state", json!({})),
        Ok(json!({ "state": "unknown" }))
    );
    let refused = app.invoke("update_restart", json!({}));
    let message = refused.expect_err("refused");
    assert!(
        message
            .as_str()
            .is_some_and(|m| m.contains("not updated: install a release")),
        "{message}"
    );
}

/// Answers every request on a port of this machine with `body`, as JSON, and returns its
/// address. It goes with the test's process.
fn serving(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("its address");
    std::thread::spawn(move || {
        for mut asked in listener.incoming().flatten() {
            // What is asked does not matter, only that it was read to the end of its headers.
            let lines = BufReader::new(&asked).lines().map_while(Result::ok);
            lines.take_while(|line| !line.is_empty()).for_each(drop);
            let head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close";
            let answer = format!("{head}\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            asked.write_all(answer.as_bytes()).ok();
        }
    });
    format!("http://{address}/latest.json")
}

#[test]
fn a_look_that_gets_no_answer_says_so_and_the_state_agrees() {
    let app = app();
    let looked = app
        .invoke("update_check", json!({}))
        .expect("update_check answers");
    assert_eq!(looked["state"], "failed", "{looked}");
    assert!(looked["reason"].is_string(), "{looked}");
    assert_eq!(app.invoke("update_state", json!({})), Ok(looked));
}

/// A build from the sources looks when asked, and downloads nothing.
#[test]
fn a_look_finds_a_newer_version_and_none_in_an_older_one() {
    let newer = r#"{"version":"999.0.0","url":"http://127.0.0.1:9/a.tar.gz","signature":"none"}"#;
    let app = app_updating_from(&serving(newer));
    let out = json!({ "state": "available", "version": "999.0.0" });
    assert_eq!(app.invoke("update_check", json!({})), Ok(out.clone()));
    assert_eq!(app.invoke("update_state", json!({})), Ok(out));

    let older = r#"{"version":"0.0.1","url":"http://127.0.0.1:9/a.tar.gz","signature":"none"}"#;
    let app = app_updating_from(&serving(older));
    let current = json!({ "state": "up_to_date" });
    assert_eq!(app.invoke("update_check", json!({})), Ok(current));
}

/// A configuration the updater refused would stop the app as it starts, in a release only.
#[test]
fn the_updater_accepts_its_configuration_with_its_key_and_the_signed_version() {
    let file: Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json is JSON");
    let section = file.pointer("/plugins/updater").cloned();
    let config: tauri_plugin_updater::Config =
        serde_json::from_value(section.expect("configured")).expect("accepted");
    assert!(config.require_signed_version);
    // Without it no update could be checked, and the release would only find out when built.
    assert!(!config.pubkey.is_empty(), "plugins.updater.pubkey is empty");
    let endpoints: Vec<&str> = config.endpoints.iter().map(Url::as_str).collect();
    assert_eq!(
        endpoints,
        ["https://github.com/ryckakas/swarmed/releases/latest/download/latest.json"]
    );
}

#[test]
fn a_link_that_is_not_a_web_link_is_not_opened() {
    let app = app();
    let refused = app.invoke("open_link", json!({ "url": "file:///etc/passwd" }));
    let message = refused.expect_err("refused");
    assert!(
        message
            .as_str()
            .is_some_and(|m| m.contains("only web and mail links")),
        "{message}"
    );
}

#[test]
fn the_ui_can_remove_a_repository_with_its_tasks() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    app.ok("project_remove", json!({ "name": "demo", "force": true }));
    assert_eq!(app.ok("overview", json!({})), json!([]));
    assert_eq!(
        app.invoke("project_remove", json!({ "name": "demo", "force": false })),
        Err(json!("project `demo` not found"))
    );
}

#[test]
fn the_ui_lists_the_sessions_a_message_can_mention_and_sends_them_checked() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let how = json!({ "model": null, "supervision": "ask", "effort": null });
    let new = json!({ "project": "demo", "task": "one", "agent": null, "how": how });
    let here = app.ok("session_new", new.clone());
    let found = app.ok("session_new", new);
    let (id, other) = (&here["id"], &found["id"]);

    // The list leaves the session itself out; a new task's has every one of the repository.
    let listed = app.ok("session_reachable", json!({ "id": id }));
    assert_eq!(listed[0]["id"], *other);
    assert_eq!(listed[0]["same_task"], true);
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    let for_new_task = app.ok("project_reachable", json!({ "project": "demo" }));
    assert_eq!(for_new_task.as_array().map(Vec::len), Some(2));

    // A session that is not there, or not to be read from here, is refused before anything goes.
    let refused = app.invoke(
        "session_send",
        json!({ "id": id, "text": "look", "files": [], "sessions": [404] }),
    );
    assert_eq!(
        refused,
        Err(json!(
            "session 404 cannot be mentioned here: it is not another open session this one can read"
        ))
    );
    let mentioned = json!({ "id": id, "text": "look", "files": [], "sessions": [other] });
    app.ok("session_send", mentioned);
    let message = app.wait_for_event(id, "user_message");
    let text = message["text"].as_str().unwrap_or_default();
    assert!(text.starts_with("look\n\nSessions mentioned"), "{text}");
    assert!(text.contains(&format!("- session {other} (")), "{text}");
    // Its turn ends first: ending after "later" is queued, it would send "later" out of the queue.
    app.wait_for_event(id, "turn_done");

    // Queued and edited with the mention, and sent now with one.
    app.ok("session_send", json!({ "id": id, "text": "slow" }));
    let queue = app.ok(
        "session_enqueue",
        json!({ "id": id, "text": "later", "files": [], "sessions": [other] }),
    );
    assert!(
        queue[0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Sessions mentioned"))
    );
    let edited = app.ok(
        "session_queue_edit",
        json!({ "id": id, "message": queue[0]["id"], "text": "then", "files": [], "sessions": [other] }),
    );
    assert!(
        edited[0]["text"]
            .as_str()
            .is_some_and(|text| text.starts_with("then\n\nSessions"))
    );
    app.ok(
        "session_message_now",
        json!({ "id": id, "text": "now", "files": [], "sessions": [other] }),
    );
    wait_for(&app, id, "the message sent now", |e| {
        e["type"] == "user_message" && e["text"].as_str().is_some_and(|t| t.starts_with("now\n\n"))
    });

    // A new task's first message mentions as well.
    let start = json!({
        "branch": null, "environment": null, "agent": null,
        "how": { "model": null, "supervision": "auto", "effort": null, "fast": false },
        "title": null, "prompt": "carry on", "files": [], "sessions": [other], "images": [],
    });
    let started = app.ok(
        "task_start",
        json!({ "project": "demo", "name": "carry", "start": start }),
    );
    let first = app.wait_for_event(&started["session"]["id"], "user_message");
    assert!(
        first["text"]
            .as_str()
            .is_some_and(|text| text.starts_with("carry on\n\nSessions"))
    );
}

#[test]
fn the_ui_lists_pull_requests_and_issues_to_mention_and_sends_only_those_read() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let how = json!({ "model": null, "supervision": "ask", "effort": null });
    let new = json!({ "project": "demo", "task": "one", "agent": null, "how": how });
    let here = app.ok("session_new", new);
    let id = &here["id"];

    // The test repository has no remote: whether `gh` is installed or not, the menu is offered
    // nothing and says why, rather than failing.
    let listed = app.ok(
        "project_github_items",
        json!({ "project": "demo", "query": "" }),
    );
    assert_eq!(listed["items"], json!([]));
    assert!(listed["unavailable"].is_string(), "{listed}");
    let unread = app
        .invoke(
            "project_github_read",
            json!({ "project": "demo", "numbers": [142] }),
        )
        .expect_err("nothing on GitHub to read");
    assert!(
        unread
            .to_string()
            .starts_with("\"#142 cannot be mentioned: "),
        "{unread}"
    );

    // A number nobody read is refused by every way a message goes, before anything goes.
    let refusal = json!("#142 cannot be mentioned: it was not read from GitHub: pick it with #");
    let message = json!({ "id": id, "text": "see", "files": [], "sessions": [], "github": [142] });
    for command in ["session_send", "session_message_now", "session_enqueue"] {
        assert_eq!(
            app.invoke(command, message.clone()),
            Err(refusal.clone()),
            "{command}"
        );
    }
    let start = json!({
        "branch": null, "environment": null, "agent": null,
        "how": { "model": null, "supervision": "auto", "effort": null, "fast": false },
        "title": null, "prompt": "review", "files": [], "sessions": [], "github": [142],
        "images": [],
    });
    let started = app.invoke(
        "task_start",
        json!({ "project": "demo", "name": "review", "start": start }),
    );
    assert_eq!(started, Err(refusal));
    // Nothing at all named goes as before.
    let plain = json!({ "id": id, "text": "hello", "files": [], "sessions": [], "github": [] });
    app.ok("session_send", plain);
    app.wait_for_event(id, "user_message");
}

#[test]
fn the_ui_can_queue_messages_and_change_them_while_they_wait() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "ask", "effort": null } }),
    );
    let id = &session["id"];
    app.ok("session_send", json!({ "id": id, "text": "run: ls" }));
    let request = app.wait_for_event(id, "permission_request");

    let queue = app.ok("session_enqueue", json!({ "id": id, "text": "then" }));
    assert_eq!(queue[0]["text"], "then");
    let edited = app.ok(
        "session_queue_edit",
        json!({ "id": id, "message": queue[0]["id"], "text": "next" }),
    );
    assert_eq!(edited[0]["text"], "next");
    assert_eq!(app.ok("session_queue", json!({ "id": id })), edited);
    // Opened for editing and left as it was: nothing was held back, so nothing goes.
    let held = json!({ "id": id, "message": queue[0]["id"] });
    assert_eq!(app.ok("session_queue_hold", held.clone()), json!(true));
    assert_eq!(app.ok("session_queue_let_go", held), json!(null));

    app.ok(
        "session_allow",
        json!({ "id": id, "requestId": request["request_id"] }),
    );
    // When the turn ends the waiting message goes out by itself.
    let sent = app.wait_for_text(id, "echo: next");
    assert_eq!(sent["type"], "text");
    wait_for_end_of(&app, id, "echo: next");
    assert_eq!(app.ok("session_queue", json!({ "id": id })), json!([]));
    // With nothing waiting, sending the next one is not an error.
    app.ok("session_send_queued", json!({ "id": id }));

    // One sent now stops the turn under way and goes ahead of the others.
    app.ok("session_send", json!({ "id": id, "text": "slow" }));
    app.ok("session_enqueue", json!({ "id": id, "text": "later" }));
    let queue = app.ok("session_enqueue", json!({ "id": id, "text": "now" }));
    app.ok(
        "session_send_now",
        json!({ "id": id, "message": queue[1]["id"] }),
    );
    app.wait_for_text(id, "echo: now");
    app.wait_for_text(id, "echo: later");
    wait_for_end_of(&app, id, "echo: later");

    // What is typed can be sent now too, without waiting in the queue first.
    app.ok("session_send", json!({ "id": id, "text": "slow" }));
    app.ok(
        "session_message_now",
        json!({ "id": id, "text": "typed", "files": [] }),
    );
    app.wait_for_text(id, "echo: typed");

    // Told now, a waiting message is taken into the turn under way, which goes on.
    let earlier = request["request_id"].clone();
    app.ok("session_send", json!({ "id": id, "text": "run: ls" }));
    let request = wait_for(&app, id, "a second request", |e| {
        e["type"] == "permission_request" && e["request_id"] != earlier
    });
    let queue = app.ok("session_enqueue", json!({ "id": id, "text": "a hint" }));
    let told = json!({ "id": id, "message": queue[0]["id"] });
    assert_eq!(app.ok("session_tell_now", told), json!(null));
    app.ok(
        "session_allow",
        json!({ "id": id, "requestId": request["request_id"] }),
    );
    app.wait_for_text(id, "heard: a hint");

    // A command a call waits on can be sent to the background, and the turn goes on.
    app.ok(
        "session_send",
        json!({ "id": id, "text": "foreground bash" }),
    );
    wait_for(&app, id, "the call", |e| {
        e["type"] == "tool_use" && e["id"] == "tool-fg-bash"
    });
    let sent = json!({ "id": id, "toolUseId": "tool-fg-bash" });
    assert_eq!(app.ok("session_send_to_background", sent), json!(true));
    app.wait_for_text(id, "It goes on in the background.");
    // With no call waiting, a call's work does not go; asked for every call, it is taken.
    let gone = json!({ "id": id, "toolUseId": "tool-fg-bash" });
    assert_eq!(app.ok("session_send_to_background", gone), json!(false));
    let every = json!({ "id": id, "toolUseId": null });
    assert_eq!(app.ok("session_send_to_background", every), json!(true));
}

#[test]
fn the_ui_can_send_the_agents_own_commands_and_answer_what_they_ask() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "ask", "effort": null } }),
    );
    let id = &session["id"];
    // A screen is asked of the agent and arrives as an event; nothing is said to it.
    app.ok("session_send", json!({ "id": id, "text": "/usage" }));
    let screen = app.wait_for_event(id, "screen");
    assert_eq!(screen["command"], "usage");
    assert_eq!(screen["sections"][0]["rows"][0]["value"], "usage");

    // A setting is asked about where the conversation stands, and set by the answer.
    app.ok("session_send", json!({ "id": id, "text": "/model" }));
    let asked = app.wait_for_event(id, "command_asked");
    let question = asked["questions"][0]["question"]
        .as_str()
        .expect("a question");
    assert_eq!(asked["questions"][0]["multiSelect"], false);
    app.ok(
        "session_command_answer",
        json!({ "id": id, "requestId": asked["request_id"], "answers": { question: "opus" } }),
    );
    let answered = app.wait_for_event(id, "command_answered");
    assert_eq!(answered["request_id"], asked["request_id"]);
    assert_eq!(answered["said"], "This session's model is now opus.");

    // Put aside, it changes nothing.
    app.ok("session_send", json!({ "id": id, "text": "/effort" }));
    let asked = app.wait_for_event(id, "command_asked");
    app.ok(
        "session_command_answer",
        json!({ "id": id, "requestId": asked["request_id"], "answers": null }),
    );
    let aside = app.wait_for_event(id, "command_answered");
    assert_eq!(aside["said"], "Nothing was changed.");

    let events = app.ok("session_events", json!({ "id": id }));
    let said = |e: &&Value| e["type"] == "user_message";
    assert_eq!(
        events.as_array().into_iter().flatten().filter(said).count(),
        0
    );
}

#[test]
fn the_ui_can_answer_what_an_agent_asks() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "ask", "effort": null } }),
    );
    let id = &session["id"];
    app.ok("session_send", json!({ "id": id, "text": "run: ls" }));
    let request = app.wait_for_event(id, "permission_request");

    let answers = json!({ "Which one?": "The second" });
    app.ok(
        "session_answer",
        json!({ "id": id, "requestId": request["request_id"], "answers": answers }),
    );
    let result = app.wait_for_event(id, "tool_result");
    assert_eq!(
        result["content"],
        r#"answered: {"Which one?": "The second"}"#
    );
    assert_eq!(
        app.wait_for_event(id, "permission_answered")["allowed"],
        true
    );
}

#[test]
fn the_ui_can_ask_for_a_task_name_and_is_told_when_there_is_none() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    // The scripted agent comes without anything that names, so the UI names the task itself.
    let asked = json!({ "project": "demo", "prompt": "fix the login redirect" });
    assert_eq!(app.ok("task_name_for", asked), json!(null));
    assert_eq!(
        app.invoke(
            "task_name_for",
            json!({ "project": "nowhere", "prompt": "x" })
        ),
        Err(json!("project `nowhere` not found"))
    );
}

// `agent_install` and `openrouter_models` are not called here: one would install software on
// whoever runs the tests and the other needs the network. The core tests them with stand-ins.
// `agent_models_refresh` is only asked about an agent there is none of: for a real one it
// would start that agent's program, if whoever runs the tests has it.
#[test]
fn the_ui_can_see_which_agents_can_be_set_up_and_choose_their_models() {
    let app = app();
    let setups = app.ok("agent_setups", json!({}));
    // Claude Code first; whether it is installed depends on who runs the tests.
    assert_eq!(setups[0]["id"], "claude");
    assert_eq!(setups[0]["built_in"], true);
    assert_eq!(setups[2]["id"], "opencode");
    assert_eq!(setups[2]["configured"], false);

    let chosen = json!({ "id": "opencode", "models": ["openrouter/x-ai/grok-4.7"] });
    let setup = app.ok("agent_configure", chosen);
    assert_eq!(setup["configured"], true);
    assert_eq!(setup["models"], json!(["openrouter/x-ai/grok-4.7"]));
    // Whether the agent menu offers it depends on its program being on this machine, which
    // the core's own tests cover: here it may or may not be.
    let agents = app.ok("agents", json!({}));
    assert_eq!(agents[0]["id"], "claude");

    let kept = app.ok(
        "agent_configure",
        json!({ "id": "opencode", "models": null }),
    );
    assert_eq!(kept["models"], json!(["openrouter/x-ai/grok-4.7"]));
    assert_eq!(
        app.invoke("agent_configure", json!({ "id": "nope", "models": null })),
        Err(json!("agent `nope` is not configured"))
    );
    assert_eq!(
        app.invoke("agent_models_refresh", json!({ "id": "nope" })),
        Err(json!("agent `nope` is not configured"))
    );
}

#[test]
fn the_ui_can_see_what_swarmed_takes_up() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    // Nothing has run, so nothing was spent; the figures are there all the same.
    let costs = app.ok("costs", json!({}));
    assert_eq!(costs["total"], json!(0.0));
    assert_eq!(costs["tasks"], json!([]));
    assert_eq!(app.ok("analytics", json!({})), json!([]));
    let usage = app.ok("usage", json!({}));
    let positive = |value: &Value| value.as_u64().is_some_and(|bytes| bytes > 0);
    assert!(positive(&usage["database_bytes"]), "{usage}");
    assert_eq!(usage["worktrees"][0]["task"], "one");
    assert!(positive(&usage["worktrees"][0]["bytes"]), "{usage}");
    // The process running this test is the app, and it is using some memory.
    assert!(positive(&usage["running"]["memory_bytes"]), "{usage}");
    assert_eq!(usage["sessions"], json!([]));
}

#[test]
fn the_ui_can_clean_up_what_removed_tasks_left() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let task = json!({ "project": "demo", "name": "one", "force": true, "deleteBranch": false });
    app.ok("task_remove", task);

    let listed = app.ok("cleanup", json!({}));
    let branches = listed["branches"].as_array().expect("a list");
    assert_eq!(branches.len(), 1, "{listed}");
    assert_eq!(
        (&branches[0]["task"], &branches[0]["at_risk"]),
        (&json!("one"), &json!(false))
    );
    assert_eq!(listed["stale"], json!([]));
    assert_eq!(listed["snapshots"], json!([]));
    let key = branches[0]["key"].clone();
    let cleaned = app.ok(
        "cleanup_clean",
        json!({ "kind": "branches", "keys": [key], "atRisk": [] }),
    );
    assert_eq!(cleaned, json!({ "deleted": [key], "kept": [] }));
    assert_eq!(app.ok("cleanup", json!({}))["branches"], json!([]));
}

#[test]
fn the_ui_can_set_up_an_mcp_server_and_never_reads_its_secrets_back() {
    let app = app();
    assert_eq!(app.ok("mcp_servers", json!({})), json!([]));
    let server = json!({ "server": {
        "name": "tracker", "command": "npx -y tracker", "url": "",
        "secrets": [["TRACKER_TOKEN", "s3cret"]], "enabled": true,
    } });
    let listed = app.ok("mcp_save", server);
    assert_eq!(listed[0]["name"], "tracker");
    assert_eq!(listed[0]["origin"], "swarmed");
    assert_eq!(listed[0]["target"], "npx -y tracker");
    assert_eq!(listed[0]["secrets"], json!(["TRACKER_TOKEN"]));
    assert!(!listed.to_string().contains("s3cret"));

    // An untouched secret field comes back as null and keeps what is stored.
    let off = json!({ "server": {
        "name": "tracker", "command": "npx -y tracker", "url": "",
        "secrets": [["TRACKER_TOKEN", null]], "enabled": false,
    } });
    assert_eq!(app.ok("mcp_save", off)[0]["enabled"], false);
    assert_eq!(
        app.invoke(
            "mcp_save",
            json!({ "server": { "name": "a b", "command": "x", "enabled": true } })
        ),
        Err(json!("a server's name is made of letters, digits, - and _"))
    );
    assert_eq!(
        app.ok("mcp_remove", json!({ "name": "tracker" })),
        json!([])
    );
}

/// A mark is asked for by its colour's name; `none` takes it off.
fn marks_are_put_on_and_taken_off(app: &App, id: &Value) {
    let marked = app.ok(
        "session_mark",
        json!({ "id": id, "colour": "green", "label": " tests pass " }),
    );
    assert_eq!(
        marked["mark"],
        json!({ "colour": "green", "label": "tests pass" })
    );
    assert_eq!(app.wait_for_event(id, "marked")["colour"], "green");
    let no_colour = json!({ "id": id, "colour": "teal", "label": "" });
    app.invoke("session_mark", no_colour)
        .expect_err("not a colour");
    let cleared = app.ok(
        "session_mark",
        json!({ "id": id, "colour": "none", "label": "" }),
    );
    assert_eq!(cleared["mark"], json!(null));
}

#[test]
fn the_ui_can_start_a_chat_and_carry_it_on_in_a_repository() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    // A chat is asked for as a task in `.chats`, which no repository can be called.
    let none = json!({ "project": ".chats", "prompt": "What is a monad?" });
    app.ok("task_name_for", none);
    let chat =
        json!({ "project": ".chats", "name": "monads", "branch": null, "environment": null });
    let made = app.ok("task_new", chat);
    assert_eq!(made["branch"], "");
    let overview = app.ok("overview", json!({}));
    let home = overview
        .as_array()
        .and_then(|projects| projects.iter().find(|project| project["chats"] == true))
        .expect("the chats are listed")
        .clone();
    assert_eq!(home["name"], ".chats");
    assert_eq!(home["tasks"][0]["name"], "monads");
    assert_eq!(overview[0]["chats"], false);
    // What only a repository has is refused, in words the UI can show.
    let refused = app
        .invoke("branch_choices", json!({ "project": ".chats" }))
        .expect_err("no branches");
    assert!(refused.to_string().contains("no repository"), "{refused}");

    let session = app.ok(
        "session_new",
        json!({ "project": ".chats", "task": "monads", "agent": null, "how": { "model": null, "supervision": "ask", "effort": null } }),
    );
    let id = &session["id"];
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let into = json!({ "id": id, "project": "demo", "task": "one" });
    let carried = app.ok("session_fork_into", into);
    assert_ne!(carried["id"], *id);
    let tasks = app.ok("overview", json!({}))[0]["tasks"].clone();
    assert_eq!(tasks[0]["sessions"][0]["id"], carried["id"]);

    let remove =
        json!({ "project": ".chats", "name": "monads", "force": false, "deleteBranch": true });
    app.ok("task_remove", remove);
}

#[test]
fn the_ui_can_list_closed_sessions_and_reopen_one() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "ask", "effort": null } }),
    );
    let id = &session["id"];
    app.ok("session_close", json!({ "id": id, "withWorkers": false }));

    // Closed is not gone: it is counted, listed, and can be put back.
    let closed = |app: &App| app.ok("overview", json!({}))[0]["tasks"][0]["closed"].clone();
    assert_eq!(closed(&app), 1);
    let task = json!({ "project": "demo", "task": "one" });
    assert_eq!(app.ok("sessions_closed", task.clone())[0]["id"], *id);
    // Nothing was said in it, so nothing is found, which is an answer and not an error.
    let nothing = app.ok("sessions_search", json!({ "query": "zeppelin" }));
    assert_eq!(nothing, json!([]));
    assert_eq!(app.ok("session_reopen", json!({ "id": id }))["id"], *id);
    let fork = app.ok("session_fork", json!({ "id": id, "from": null }));
    assert_ne!(fork["id"], *id);
    assert_eq!(fork["title"], json!("Fork"));
    // A fork may name the agent to carry on with: the session's own, or one that is set up.
    let same = json!({ "id": id, "from": null, "agent": "claude" });
    assert_eq!(app.ok("session_fork", same)["agent"], "claude");
    let unknown = json!({ "id": id, "from": null, "agent": "nope" });
    app.invoke("session_fork", unknown)
        .expect_err("not an agent");
    marks_are_put_on_and_taken_off(&app, id);
    assert_eq!(app.ok("sessions_closed", task), json!([]));
    assert_eq!(closed(&app), 0);
}

#[test]
fn the_ui_can_point_the_agent_at_files_with_a_message() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "ask", "effort": null } }),
    );
    let id = &session["id"];
    let readme = app.repo.join("README.md");
    app.ok(
        "session_send",
        json!({ "id": id, "text": "what is this?", "files": [readme] }),
    );
    let said = app.wait_for_event(id, "user_message");
    let expected = format!("what is this?\n\nAttached files:\n- {}", readme.display());
    assert_eq!(said["text"], expected);
    // A file that is not there is refused before anything is sent.
    let missing = json!({ "id": id, "text": "x", "files": ["/nonexistent/file.txt"] });
    assert_eq!(
        app.invoke("session_send", missing),
        Err(json!(
            "/nonexistent/file.txt cannot be attached: there is no such file"
        ))
    );
}

#[test]
fn the_ui_can_keep_a_note_on_a_project_and_on_a_task() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);

    let on_project = json!({ "project": "demo", "task": null });
    let on_task = json!({ "project": "demo", "task": "one" });
    assert_eq!(app.ok("note", on_project.clone()), "");
    app.ok(
        "set_note",
        json!({ "project": "demo", "task": null, "text": "for the repository" }),
    );
    app.ok(
        "set_note",
        json!({ "project": "demo", "task": "one", "text": "for the task" }),
    );
    assert_eq!(app.ok("note", on_project), "for the repository");
    assert_eq!(app.ok("note", on_task.clone()), "for the task");
    let overview = app.ok("overview", json!({}));
    assert_eq!(overview[0]["has_note"], true);
    assert_eq!(overview[0]["tasks"][0]["has_note"], true);

    app.ok(
        "set_note",
        json!({ "project": "demo", "task": "one", "text": "" }),
    );
    assert_eq!(app.ok("note", on_task), "");
    assert_eq!(
        app.ok("overview", json!({}))[0]["tasks"][0]["has_note"],
        false
    );
}

#[test]
fn the_ui_can_store_and_remove_a_key_without_ever_reading_it() {
    let app = app();
    let stored = |app: &App| {
        let keys = app.ok("api_keys", json!({}));
        let openrouter = keys
            .as_array()
            .and_then(|keys| keys.iter().find(|key| key["name"] == "OPENROUTER_API_KEY"));
        openrouter.cloned().expect("OpenRouter is offered")
    };
    assert_eq!(
        stored(&app),
        json!({ "name": "OPENROUTER_API_KEY", "provider": "OpenRouter", "stored": false })
    );
    let key = json!({ "name": "OPENROUTER_API_KEY", "key": " sk-or-test " });
    app.ok("set_api_key", key);
    assert_eq!(stored(&app)["stored"], true);
    assert!(
        !app.ok("api_keys", json!({}))
            .to_string()
            .contains("sk-or-test")
    );

    app.ok("delete_api_key", json!({ "name": "OPENROUTER_API_KEY" }));
    assert_eq!(stored(&app)["stored"], false);
    assert_eq!(
        app.invoke("set_api_key", json!({ "name": "AWS_SECRET", "key": "x" })),
        Err(json!("`AWS_SECRET` is not an API key Swarmed can store"))
    );
    assert_eq!(
        app.invoke(
            "set_api_key",
            json!({ "name": "OPENAI_API_KEY", "key": "  " })
        ),
        Err(json!("an API key cannot be empty"))
    );
}

#[test]
fn an_agents_html_is_served_from_its_own_origin_under_its_own_policy() {
    let dir = TempDir::new().expect("temp dir");
    let paths = Paths {
        data_dir: dir.path().join("data"),
        worktrees_root: dir.path().join("worktrees"),
        claude_config: dir.path().join("claude.json"),
        codex_config: dir.path().join("codex.toml"),
        opencode_config_dir: dir.path().join("opencode"),
    };
    let core = Core::with_backend(paths, Arc::new(FakeBackend::default())).expect("core opens");
    let id = core.view_put("<p>sorted</p>".to_string());

    let found = crate::view_response(&core, "localhost", &format!("/{id}"));
    assert_eq!(found.status(), 200);
    assert_eq!(found.body().as_ref(), b"<p>sorted</p>");
    let policy = found.headers()["content-security-policy"]
        .to_str()
        .expect("text");
    assert_eq!(policy, swarmed_core::VIEW_POLICY);
    assert!(!policy.contains("connect-src"), "{policy}");
    assert_eq!(
        crate::view_response(&core, "localhost", "/missing").status(),
        404
    );
    // A view's id under an app's host is no app.
    assert_eq!(
        crate::view_response(&core, &format!("app-{id}"), "/").status(),
        404
    );
}

/// An app an MCP server attached to a call is served from a host of its own, under the
/// policy the core built from its server's declaration; the UI shows it and calls the tools of
/// its server through the session's agent, as it does.
#[test]
fn the_ui_can_show_the_app_of_a_call_and_call_its_servers_tools() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "auto", "effort": null } }),
    );
    let id = &session["id"];
    app.ok("session_send", json!({ "id": id, "text": "app" }));
    let shown = app.wait_for_event(id, "app");
    // The file it is kept in: the HTML never reaches the window but by its frame.
    assert_eq!(shown.get("html"), None);
    assert_eq!(shown["callable"], json!(["chart", "refresh"]));
    let csp = json!({ "connect": ["https://api.example.com", "http://plain.example.com"],
                      "resources": [], "frames": [], "base_uris": [] });
    let put =
        json!({ "id": id, "toolUseId": "tool-app", "document": shown["document"], "csp": csp });
    let served = app.ok("app_put", put);
    assert_eq!(served["reaches"], json!(["https://api.example.com"]));
    assert_eq!(served["refused"].as_array().map(Vec::len), Some(1));
    let app_id = served["id"].as_str().expect("an id");
    let core = tauri::Manager::state::<Arc<Core>>(&app.window);
    let found = crate::view_response(&core, &format!("app-{app_id}"), "/");
    assert_eq!(found.status(), 200);
    assert_eq!(found.body().as_ref(), b"<!doctype html><h1>Chart</h1>");
    let policy = found.headers()["content-security-policy"]
        .to_str()
        .expect("text");
    assert!(
        policy.contains("connect-src https://api.example.com;"),
        "{policy}"
    );
    assert!(policy.contains("frame-src 'none'"), "{policy}");
    assert_ne!(policy, swarmed_core::VIEW_POLICY);
    // Nothing else is served from its host.
    let elsewhere = crate::view_response(&core, &format!("app-{app_id}"), "/app.js");
    assert_eq!(elsewhere.status(), 404);

    let call =
        json!({ "id": id, "toolUseId": "tool-app", "tool": "refresh", "arguments": { "page": 2 } });
    let called = app.ok("session_app_call", call);
    assert_eq!(called["structuredContent"], json!({ "page": 2 }));
    let refused =
        json!({ "id": id, "toolUseId": "tool-app", "tool": "drop_tables", "arguments": {} });
    assert_eq!(
        app.invoke("session_app_call", refused),
        Err(json!("this app may not call the tool `drop_tables`"))
    );
    // Only a file kept as an app in the session's own folder is served.
    let readme = json!({ "id": id, "toolUseId": "tool-app", "document": app.repo.join("README.md"), "csp": csp });
    assert!(app.invoke("app_put", readme).is_err());
}

// `view_open` is the one command not called here: it would open a browser on whoever runs the
// tests. What it writes is tested in the core.
#[test]
fn the_ui_can_hand_over_html_for_a_frame() {
    let app = app();
    let id = app.ok("view_put", json!({ "document": "<p>hi</p>" }));
    assert_eq!(id.as_str().map(str::len), Some(16));
}

#[test]
fn asking_for_the_changes_of_a_cloud_task_that_is_not_there_says_so() {
    let app = app();
    let diff = app.invoke("cloud_task_diff", json!({ "id": 404 }));
    assert_eq!(
        diff.expect_err("refused"),
        json!("there is no cloud task 404")
    );
    let apply = json!({ "id": 404, "project": "demo", "name": "one" });
    let applied = app.invoke("cloud_task_apply", apply);
    assert_eq!(
        applied.expect_err("refused"),
        json!("there is no cloud task 404")
    );
    // Opening one would start a browser; one that is not there is refused before that.
    let opened = app.invoke("cloud_task_open", json!({ "id": 404 }));
    assert_eq!(
        opened.expect_err("refused"),
        json!("there is no cloud task 404")
    );
    // Forgetting takes a task off the list; one that is not on it is off it already.
    assert_eq!(
        app.ok("cloud_task_forget", json!({ "id": 404 })),
        json!(null)
    );
}

// Delegating for real would hand the prompt to a provider's cloud. The test repository is not
// on GitHub, which the core finds out from git alone and refuses before anything leaves the
// machine; with no tasks delegated, asking for their statuses again asks nobody either.
#[test]
fn the_ui_can_list_cloud_tasks_and_cannot_delegate_from_a_repository_that_is_not_on_github() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let listing = json!({ "project": "demo", "refresh": true });
    assert_eq!(app.ok("cloud_tasks", listing), json!([]));
    for provider in ["codex", "claude"] {
        let delegate = json!({
            "project": "demo", "prompt": "fix the redirect", "task": null, "provider": provider,
        });
        let refused = app.invoke("cloud_delegate", delegate).expect_err("refused");
        let said = |m: &str| m.starts_with("this repository is not on GitHub");
        assert!(refused.as_str().is_some_and(said), "{refused}");
    }
    assert_eq!(
        app.ok(
            "cloud_tasks",
            json!({ "project": "demo", "refresh": false })
        ),
        json!([])
    );
}

// Nothing was said in a new session, which the core finds out before anything leaves the machine.
#[test]
fn the_ui_cannot_hand_on_a_conversation_in_which_nothing_was_said() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let how = json!({ "model": null, "supervision": "ask", "effort": null });
    let new = json!({ "project": "demo", "task": "one", "agent": null, "how": how });
    let id = app.ok("session_new", new)["id"].clone();
    let hand_on = json!({ "id": id, "instruction": "", "provider": "codex" });
    let refused = app
        .invoke("cloud_delegate_conversation", hand_on)
        .expect_err("refused");
    let said = |m: &str| m.starts_with("nothing was said in this session yet");
    assert!(refused.as_str().is_some_and(said), "{refused}");
}

#[test]
fn the_ui_can_keep_a_prompt_and_drop_it() {
    let app = app();
    assert_eq!(app.ok("templates", json!({})), json!([]));
    let kept = app.ok(
        "template_save",
        json!({ "name": "review", "text": "Review {{file}}." }),
    );
    assert_eq!(
        kept,
        json!([{ "name": "review", "text": "Review {{file}}." }])
    );
    assert_eq!(app.ok("templates", json!({})), kept);
    assert_eq!(
        app.ok("template_remove", json!({ "name": "review" })),
        json!([])
    );
}

#[test]
fn the_ui_can_set_the_badge_and_clear_it() {
    let app = app();
    assert_eq!(app.ok("badge_set", json!({ "count": 3 })), json!(null));
    assert_eq!(app.ok("badge_set", json!({ "count": 0 })), json!(null));
}

#[test]
fn a_core_error_reaches_the_ui_as_its_message() {
    let app = app();
    let missing = app.invoke(
        "task_new",
        json!({ "project": "nope", "name": "one", "branch": null }),
    );
    assert_eq!(missing, Err(json!("project `nope` not found")));
    let not_running = app.invoke("session_interrupt", json!({ "id": 7 }));
    assert_eq!(not_running, Err(json!("session 7 is not running")));
}

#[test]
fn the_ui_can_keep_an_action_as_it_holds_it_and_answer_a_proposal() {
    let app = app();
    assert_eq!(app.ok("actions", json!({ "project": null })), json!([]));
    // The UI sends an action back whole, with the flag only the core decides.
    let action = json!({
        "name": "Tests", "icon": "🧪", "prompt": "", "command": "make test", "of_project": true
    });
    let kept = app.ok("action_save", json!({ "project": null, "action": action }));
    assert_eq!(kept[0]["name"], "Tests");
    assert_eq!(kept[0]["of_project"], false);
    let second = json!({ "name": "Lint", "icon": "", "prompt": "", "command": "make lint", "of_project": false });
    app.ok("action_save", json!({ "project": null, "action": second }));
    let moved = app.ok(
        "action_move",
        json!({ "project": null, "name": "Lint", "by": -1 }),
    );
    assert_eq!(
        (&moved[0]["name"], &moved[1]["name"]),
        (&json!("Lint"), &json!("Tests"))
    );
    app.ok("action_remove", json!({ "project": null, "name": "Lint" }));
    assert_eq!(
        app.ok("action_remove", json!({ "project": null, "name": "Tests" })),
        json!([])
    );
    // A proposal that is not open, such as one from before the app was started again.
    let gone = app.invoke(
        "config_answer",
        json!({ "requestId": "config-0-0", "accepted": true }),
    );
    assert!(gone.is_err());
}

#[test]
fn the_ui_can_keep_a_skill_change_it_hold_it_back_and_remove_it() {
    let app = app();
    assert_eq!(app.ok("skills", json!({})), json!([]));
    let skill = json!({
        "name": "house-style", "description": "How we write commits.", "body": "Use the imperative.",
    });
    let kept = app.ok("skill_save", skill);
    assert_eq!(
        (&kept[0]["name"], &kept[0]["origin"], &kept[0]["shared"]),
        (&json!("house-style"), &json!("swarmed"), &json!(true))
    );
    assert_eq!(app.ok("skills", json!({})), kept);
    assert_eq!(
        app.ok("skill_body", json!({ "name": "house-style" })),
        "Use the imperative."
    );
    let held_back = json!({ "origin": "swarmed", "name": "house-style", "shared": false });
    assert_eq!(app.ok("skill_share", held_back)[0]["shared"], false);
    assert_eq!(
        app.ok("skill_remove", json!({ "name": "house-style" })),
        json!([])
    );
    app.invoke("skill_body", json!({ "name": "house-style" }))
        .expect_err("removed");
    let unnamed = json!({ "name": "Bad Name", "description": "d", "body": "b" });
    app.invoke("skill_save", unnamed).expect_err("refused");
}

#[test]
fn the_ui_can_keep_an_environment_start_a_task_in_it_and_snapshot_what_its_setup_left() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let project = json!({ "project": "demo" });
    assert_eq!(app.ok("environments", project.clone()), json!({}));
    // Its setup leaves one file that git ignores, as an install or a build would.
    let environment = json!({
        "setup": "echo .cache > .gitignore && echo built > .cache",
        "start": "", "stop": "", "teardown": "", "open_url": "", "ports": {}, "env": {},
        "max_stacks": 3,
    });
    let save = json!({ "project": "demo", "name": "cached", "environment": environment });
    assert_eq!(app.ok("environment_save", save), json!(null));
    assert_eq!(
        app.ok("environments", project.clone())["cached"],
        environment
    );

    let task = app.ok(
        "task_new",
        json!({
            "project": "demo", "name": "one", "branch": null,
            "environment": { "named": "cached" },
        }),
    );
    assert_eq!(
        (&task["environment"], &task["setup"]),
        (&json!("cached"), &json!("ok"))
    );
    // Set up well with no snapshot to start from, the task is kept as its environment's.
    let first = app.ok("snapshots", project.clone());
    assert_eq!(
        (&first[0]["environment"], &first[0]["task"]),
        (&json!("cached"), &json!("one"))
    );
    // Taken again by hand, it takes the first one's place.
    let taken = app.ok("snapshot_save", json!({ "project": "demo", "task": "one" }));
    assert_eq!(
        (&taken["environment"], &taken["task"], &taken["entries"]),
        (&json!("cached"), &json!("one"), &json!(1))
    );
    assert_eq!(app.ok("snapshots", project.clone()), json!([taken]));
    let forget = json!({ "project": "demo", "environment": "cached" });
    assert_eq!(app.ok("snapshot_remove", forget), json!(null));
    assert_eq!(app.ok("snapshots", project.clone()), json!([]));

    // Not while a task uses it; once the task is gone, it can go too.
    let remove = json!({ "project": "demo", "name": "cached" });
    app.invoke("environment_remove", remove.clone())
        .expect_err("refused while the task uses it");
    let task = json!({ "project": "demo", "name": "one", "force": true, "deleteBranch": false });
    assert_eq!(app.ok("task_remove", task), json!([]));
    assert_eq!(app.ok("environment_remove", remove), json!(null));
    assert_eq!(app.ok("environments", project), json!({}));
    let unnamed = json!({ "project": "demo", "name": "Bad Name", "environment": environment });
    app.invoke("environment_save", unnamed)
        .expect_err("refused");
}

#[test]
fn the_ui_can_make_an_environment_the_default_that_tasks_choosing_none_are_set_up_in() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let project = json!({ "project": "demo" });
    assert_eq!(app.ok("environment_default", project.clone()), json!(null));
    let environment = json!({
        "setup": "", "start": "", "stop": "", "teardown": "", "open_url": "", "ports": {}, "env": {},
    });
    let save = json!({ "project": "demo", "name": "docker", "environment": environment });
    app.ok("environment_save", save);
    app.invoke(
        "environment_set_default",
        json!({ "project": "demo", "name": "nope" }),
    )
    .expect_err("no such environment");
    let set = json!({ "project": "demo", "name": "docker" });
    assert_eq!(app.ok("environment_set_default", set), json!(null));
    assert_eq!(
        app.ok("environment_default", project.clone()),
        json!("docker")
    );

    // The window sends what the menu says: the default, the project's own setup, or a name.
    let new = |name: &str, environment: Value| {
        let start = json!({
            "project": "demo", "name": name, "branch": null, "environment": environment,
        });
        app.ok("task_new", start)["environment"].clone()
    };
    assert_eq!(new("one", json!("default")), json!("docker"));
    assert_eq!(new("two", json!("own")), json!(null));

    let none = json!({ "project": "demo", "name": null });
    assert_eq!(app.ok("environment_set_default", none), json!(null));
    assert_eq!(app.ok("environment_default", project), json!(null));
    assert_eq!(new("three", json!("default")), json!(null));
}

/// `stack_start` is not `async`, so Tauri runs it on a thread that has no Tokio runtime, as
/// this test's thread has none. Were the stack ever spawned with Tokio, the call would panic
/// here, where in the app it would end the app.
#[test]
fn the_ui_can_start_what_a_task_keeps_running_read_what_it_prints_and_stop_it() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let target = json!({ "project": "demo", "task": "one" });
    // A project with no `start` command has no stack, which is an answer and not an error.
    assert_eq!(app.ok("stack_start", target.clone()), json!(false));
    assert_eq!(app.ok("stack_log", target.clone()), json!(""));

    // Short enough to be gone soon if this test fails before stopping it.
    let config = "[hooks]\nstart = \"echo serving {{task}}; sleep 30\"\n";
    let projects = app.data_dir().join("projects");
    std::fs::create_dir_all(&projects).expect("config dir");
    std::fs::write(projects.join("demo.toml"), config).expect("config");
    let stack = |app: &App| app.ok("overview", json!({}))[0]["tasks"][0]["stack"].clone();
    assert_eq!(stack(&app), "stopped");
    assert_eq!(app.ok("stack_start", target.clone()), json!(true));
    assert_eq!(stack(&app), "starting");
    for _ in 0..200 {
        if app.ok("stack_log", target.clone()) != "" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.ok("stack_log", target.clone()), "serving one\n");
    // Starting what runs already starts nothing more.
    assert_eq!(app.ok("stack_start", target.clone()), json!(true));

    assert_eq!(app.ok("stack_stop", target.clone()), json!(null));
    assert_eq!(stack(&app), "stopped");
    let nowhere = json!({ "project": "demo", "task": "nope" });
    app.invoke("stack_start", nowhere)
        .expect_err("no such task");
}

/// A page from the web, were one ever shown in the window, can call nothing: Tauri refuses a
/// call from an address that is not the app's own unless a capability names that address,
/// and no capability of Swarmed's does (see the last test here).
#[test]
fn a_page_from_the_web_cannot_call_a_command() {
    let app = app();
    for address in ["https://example.com/", "http://localhost:3000/"] {
        let url: Url = address.parse().expect("url parses");
        let refused = app.invoke_from(&url, "overview", json!({}));
        let message = refused.expect_err("refused");
        let said = |m: &str| m.contains("overview not allowed");
        assert!(message.as_str().is_some_and(said), "{address}: {message}");
        let opening = json!({ "url": "https://example.com/" });
        app.invoke_from(&url, "open_link", opening)
            .expect_err("refused");
    }
}

/// What keeps an agent's view from calling a command is this key, and not the address the
/// view is served from: Tauri counts every scheme the app registers, `swarmed-view` among
/// them, as the app's own, and it checks the app's own commands against no capability. The
/// key is handed to the window's main frame only, never to a frame inside it, and a view's
/// own policy lets it send no request. A call without the key gets no answer at all.
#[test]
fn a_call_without_the_windows_key_is_not_answered() {
    let app = app();
    assert!(app.answers("overview", INVOKE_KEY));
    assert!(!app.answers("overview", ""));
    assert!(!app.answers("overview", "a-key-a-view-made-up"));
}

/// The capability file is read when the app is built, and the mock runtime these tests run
/// on does not load it: so it is read here as text. Only the main window may be named, no
/// page from elsewhere, and nothing beyond what the description says.
#[test]
fn the_one_capability_is_the_main_windows_and_names_no_page_from_elsewhere() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    let files: Vec<_> = std::fs::read_dir(&folder)
        .expect("capabilities folder")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(files, ["default.json"]);
    let text = std::fs::read_to_string(folder.join("default.json")).expect("capability file");
    let capability: Value = serde_json::from_str(&text).expect("json");
    assert_eq!(capability["windows"], json!(["main"]));
    assert_eq!(
        capability["permissions"],
        json!([
            "core:default",
            "core:window:allow-start-dragging",
            "dialog:allow-open"
        ])
    );
    // No address beyond the app's own, no other webview, no other platform's rules.
    let keys: Vec<&String> = capability.as_object().expect("object").keys().collect();
    let known = [
        "$schema",
        "description",
        "identifier",
        "permissions",
        "windows",
    ];
    assert_eq!(keys, known);
}

/// The stand-in Chrome of the core's tests, installed beside the repository.
fn stand_in_chrome(app: &App) -> std::path::PathBuf {
    let program = app.repo.with_file_name("fake-chrome");
    let source = app.repo.with_file_name("fake-chrome.py");
    std::fs::write(
        &source,
        include_str!("../../core/tests/fixtures/fake_chrome.py"),
    )
    .expect("writes");
    // Installed by a child process: a file this one held open for writing could be inherited
    // by a process another test starts, and running it would fail with "Text file busy".
    let status = Command::new("install")
        .args(["-m", "755"])
        .arg(&source)
        .arg(&program)
        .status()
        .expect("install runs");
    assert!(status.success());
    program
}

/// What the tab shown logged, once it has logged anything.
fn logged(app: &App, which: &Value) -> Value {
    let deadline = Instant::now() + WAIT_LIMIT;
    while Instant::now() < deadline {
        let lines = app.ok("browser_console", which.clone());
        if lines.as_array().is_some_and(|lines| !lines.is_empty()) {
            return lines;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the tab logged nothing");
}

/// Waits until the stand-in Chrome of `task_id` has been put into inspect mode `times` times.
fn inspecting(app: &App, task_id: &Value, times: usize) {
    let profile = app.ok("browser_state", json!({ "taskId": task_id }))["downloads"]
        .as_str()
        .map(|downloads| Path::new(downloads).with_file_name("profile"))
        .expect("a profile");
    let deadline = Instant::now() + WAIT_LIMIT;
    while Instant::now() < deadline {
        let modes = std::fs::read_to_string(profile.join("fake-inspect")).unwrap_or_default();
        if modes.lines().count() >= times {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the page was not put into inspect mode");
}

/// Picks an element of the tab shown for a session of the task, as the pane does, and sends
/// it in a message; then starts a pick and ends it without one.
fn picked_and_sent(app: &App, task_id: &Value) {
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "ask", "effort": null } }),
    );
    let id = &session["id"];
    let asked = json!({ "taskId": task_id, "sessionId": id });
    let picking = app.invoke_waiting("browser_pick", asked.clone());
    inspecting(app, task_id, 1);
    let held = json!({ "alt": false, "ctrl": false, "meta": false, "shift": false });
    let click = json!({ "kind": "down", "x": 60.0, "y": 110.0, "button": "left", "buttons": 1,
        "click_count": 1, "delta_x": 0.0, "delta_y": 0.0, "modifiers": held });
    app.ok(
        "browser_mouse",
        json!({ "taskId": task_id, "input": click }),
    );
    let element = picking
        .recv_timeout(WAIT_LIMIT)
        .expect("answered")
        .expect("picked");
    assert_eq!(element["number"], 1);
    assert_eq!(element["selector"], "#save");
    assert!(
        element["picture"]
            .as_str()
            .is_some_and(|path| path.ends_with("element-1.png"))
    );
    let kept = app.ok("session_elements", json!({ "id": id, "numbers": [1] }));
    assert_eq!(kept[0]["label"], element["label"]);

    let queued = app.ok(
        "session_enqueue",
        json!({ "id": id, "text": "make it blue", "files": [], "sessions": [], "github": [],
            "elements": [1] }),
    );
    let text = queued[0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("\n- element 1: button \"Save\""), "{text}");
    // Every way a message goes takes the elements it names, as `tauri.ts` sends them.
    let listed = |text: &str| text.contains("\n- element 1: button \"Save\"");
    let edited = app.ok(
        "session_queue_edit",
        json!({ "id": id, "message": queued[0]["id"], "text": "make it green", "files": [],
            "sessions": [], "github": [], "elements": [1] }),
    );
    let text = edited[0]["text"].as_str().unwrap_or_default();
    assert!(text.starts_with("make it green") && listed(text), "{text}");
    for (command, words) in [
        ("session_send", "look at it"),
        ("session_message_now", "now"),
    ] {
        let refused = app.invoke(
            command,
            json!({ "id": id, "text": words, "files": [], "sessions": [], "github": [],
                "elements": [404] }),
        );
        let why = refused.expect_err("no such element");
        assert!(why.to_string().contains("404"), "{command}: {why}");
    }

    let ended = app.invoke_waiting("browser_pick", asked);
    inspecting(app, task_id, 3);
    app.ok("browser_pick_end", json!({ "taskId": task_id }));
    let nothing = ended.recv_timeout(WAIT_LIMIT).expect("answered");
    assert_eq!(nothing, Ok(Value::Null));
}

#[test]
fn the_ui_can_open_a_tasks_browser_show_and_drive_a_tab_and_stop_it() {
    // Without Homebrew, nothing could install a browser: no test installs one for real.
    let app = app_built("https://127.0.0.1:9/latest.json", |core| {
        core.set_tool_dirs(vec![]);
    });
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let chrome = stand_in_chrome(&app);
    app.ok(
        "set_settings",
        json!({ "settings": { "browser_program": chrome } }),
    );
    let setup = app.ok("browser_setup", json!({}));
    assert_eq!(
        (&setup["name"], &setup["chosen"]),
        (&json!("fake-chrome"), &json!(true))
    );
    assert_eq!(setup["install_command"], json!(null));
    let refused = app.invoke("browser_install", json!({}));
    let said = refused.expect_err("nothing installs it");
    assert!(
        said.as_str().is_some_and(|said| said.contains("Homebrew")),
        "{said}"
    );

    let task_id = app.ok("overview", json!({}))[0]["tasks"][0]["id"].clone();
    let which = json!({ "taskId": task_id });
    assert_eq!(app.ok("browser_state", which.clone())["running"], false);
    let opened = app.ok("browser_open", which.clone());
    assert_eq!(opened["running"], true);
    assert_eq!(
        app.ok("overview", json!({}))[0]["tasks"][0]["browser"],
        true
    );
    let view = json!({ "width": 640, "height": 480, "scale": 2.0 });
    let shown = app.ok(
        "browser_show",
        json!({ "taskId": task_id, "tab": null, "viewport": view }),
    );
    let tab = shown["shown"].clone();
    assert_eq!(tab, opened["tabs"][0]["id"]);

    let held = json!({ "alt": false, "ctrl": false, "meta": false, "shift": false });
    let click = json!({ "kind": "down", "x": 1.5, "y": 2.0, "button": "left", "buttons": 1,
        "click_count": 1, "delta_x": 0.0, "delta_y": 0.0, "modifiers": held });
    app.ok(
        "browser_mouse",
        json!({ "taskId": task_id, "input": click }),
    );
    let key = json!({ "kind": "down", "key": "a", "code": "KeyA", "key_code": 65,
        "modifiers": held, "repeat": false });
    app.ok("browser_key", json!({ "taskId": task_id, "input": key }));
    app.ok(
        "browser_text",
        json!({ "taskId": task_id, "text": "pasted" }),
    );

    let address = json!({ "address": "localhost:3000/console" });
    app.ok(
        "browser_go",
        json!({ "taskId": task_id, "tab": tab, "go": address }),
    );
    app.ok(
        "browser_go",
        json!({ "taskId": task_id, "tab": tab, "go": "back" }),
    );
    let lines = logged(&app, &json!({ "taskId": task_id, "tab": tab }));
    assert_eq!(lines[0]["text"], "hello from http://localhost:3000/console");
    assert_eq!(lines[0]["level"], "info");

    let opened = app.ok(
        "browser_tab_new",
        json!({ "taskId": task_id, "url": "example.com" }),
    );
    app.ok(
        "browser_tab_close",
        json!({ "taskId": task_id, "tab": opened }),
    );
    let answer = json!({ "taskId": task_id, "tab": tab, "accept": true, "text": null });
    app.ok("browser_dialog", answer);
    picked_and_sent(&app, &task_id);
    // Without a window of its own there is none to raise, and asking does no harm.
    app.ok("browser_raise", json!({ "taskId": task_id, "tab": tab }));
    app.ok("browser_hide", which.clone());
    app.ok("browser_stop", which.clone());
    assert_eq!(app.ok("browser_state", which.clone())["running"], false);
    let gone = app.invoke("browser_text", json!({ "taskId": task_id, "text": "late" }));
    assert!(gone.is_err(), "nothing runs to type into");
}
