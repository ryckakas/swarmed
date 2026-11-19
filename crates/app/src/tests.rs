//! Calls the app's commands the way the UI does, by name with JSON arguments, on Tauri's mock
//! runtime. The argument names here are the ones `ui/src/api/tauri.ts` sends: if a command's
//! parameters are renamed, this is where it shows.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use swarmed_core::agent::FakeBackend;
use swarmed_core::{Core, Paths};
use tauri::http::HeaderMap;
use tauri::ipc::{CallbackFn, InvokeBody};
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
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("temp dir resolves");
    let repo = root.join("demo");
    std::fs::create_dir(&repo).expect("repo dir");
    git(&repo, &["init", "--quiet", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "demo\n").expect("readme");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--quiet", "-m", "init"]);

    let paths = Paths {
        data_dir: root.join("data"),
        worktrees_root: root.join("worktrees"),
        claude_config: root.join("claude.json"),
    };
    let core = Core::with_backend(paths, Arc::new(FakeBackend::default())).expect("core opens");
    let app = with_core(mock_builder(), Arc::new(core))
        .build(mock_context(noop_assets()))
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
    /// Invokes a command as the UI would; `Err` holds the error the UI would be shown.
    fn invoke(&self, command: &str, arguments: Value) -> Result<Value, Value> {
        let request = InvokeRequest {
            cmd: command.to_string(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: self.origin.clone(),
            body: InvokeBody::Json(arguments),
            headers: HeaderMap::default(),
            invoke_key: INVOKE_KEY.to_string(),
        };
        let body = get_ipc_response(&self.window, request)?;
        body.deserialize::<Value>()
            .map_err(|e| json!(e.to_string()))
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

fn wait_for(app: &App, id: &Value, what: &str, wanted: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..200 {
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
        json!({ "project": "demo", "task": "one", "agent": null, "model": null, "supervision": "ask", "effort": null }),
    );
    let id = &session["id"];

    app.ok("session_send", json!({ "id": id, "text": "run: ls" }));
    let request = app.wait_for_event(id, "permission_request");
    let overview = app.ok("overview", json!({}));
    assert_eq!(overview[0]["tasks"][1]["running"], json!([id]));
    assert_eq!(overview[0]["tasks"][1]["sessions"][0]["title"], "run: ls");

    assert_eq!(
        app.ok("slash_commands", json!({ "id": id })),
        json!(["context", "review"])
    );
    assert_eq!(
        app.ok("slash_commands", json!({ "id": null })),
        json!(["context", "review"])
    );
    app.ok(
        "session_allow",
        json!({ "id": id, "requestId": request["request_id"] }),
    );
    let result = app.wait_for_event(id, "tool_result");
    assert_eq!(result["is_error"], false);
    app.wait_for_event(id, "turn_done");
}

#[test]
fn the_ui_can_deny_interrupt_stop_and_remove() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null }),
    );
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "model": null, "supervision": "auto", "effort": "high" }),
    );
    let id = &session["id"];
    assert_eq!(session["supervision"], "auto");

    app.ok("session_send", json!({ "id": id, "text": "run: rm -rf /" }));
    let request = app.wait_for_event(id, "permission_request");
    let deny = json!({ "id": id, "requestId": request["request_id"], "reason": "no" });
    app.ok("session_deny", deny);
    assert_eq!(app.wait_for_event(id, "tool_result")["is_error"], true);
    app.wait_for_event(id, "turn_done");

    app.ok("session_send", json!({ "id": id, "text": "slow" }));
    app.wait_for_event(id, "text");
    assert_eq!(session["effort"], "high");
    let settings = json!({ "id": id, "model": "opus", "supervision": "ask", "effort": "high" });
    let changed = app.ok("session_configure", settings);
    assert_eq!(
        (&changed["model"], &changed["supervision"]),
        (&json!("opus"), &json!("ask"))
    );
    app.ok("session_interrupt", json!({ "id": id }));
    app.ok("session_stop", json!({ "id": id }));
    app.wait_for_event(id, "exited");
    assert_eq!(app.ok("status_line", json!({ "id": id })), json!(null));
    let size = json!({ "id": id, "cols": 80, "rows": 24 });
    assert_eq!(app.ok("terminal_open", size.clone()), json!(""));
    app.ok("terminal_write", json!({ "id": id, "data": "true\n" }));
    app.ok("terminal_resize", size);
    app.ok("terminal_close", json!({ "id": id }));
    assert_eq!(
        app.invoke("terminal_write", json!({ "id": id, "data": "ls\n" })),
        Err(json!(format!("session {id} has no terminal open")))
    );
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
    app.ok("session_close", json!({ "id": id }));
    assert_eq!(
        app.ok("overview", json!({}))[0]["tasks"][0]["sessions"],
        json!([])
    );

    let remove = json!({ "project": "demo", "name": "one", "force": false, "deleteBranch": true });
    app.ok("task_remove", remove);
    assert_eq!(
        app.ok("overview", json!({}))[0]["tasks"][0]["archived"],
        true
    );
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
            "editor": "code", "auto_title": true, "theme": "system", "text_scale": 100,
            "html_views": true, "default_supervision": "auto",
            "status_items": ["branch", "changes", "model", "context", "cost"],
            "status_command": "",
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
        "editor": "swarmed-no-such-editor", "auto_title": false, "theme": "light", "text_scale": 110,
        "html_views": false, "default_supervision": "ask", "status_items": ["cost"], "status_command": "",
    } });
    app.ok("set_settings", chosen);
    let target = json!({ "project": "demo", "name": "one" });
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
fn the_ui_can_remove_a_repository_with_its_tasks() {
fn the_ui_can_queue_messages_and_change_them_while_they_wait() {
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
    let session = app.ok(
        "session_new",
        json!({ "project": "demo", "task": "one", "agent": null, "model": null, "supervision": "ask", "effort": null }),
    );
    let id = &session["id"];
    app.ok("session_send", json!({ "id": id, "text": "run: ls" }));
    let request = app.wait_for_event(id, "permission_request");

    let queue = app.ok("session_enqueue", json!({ "id": id, "text": "nxt" }));
    assert_eq!(queue[0]["text"], "nxt");
    let edited = app.ok(
        "session_queue_edit",
        json!({ "id": id, "message": queue[0]["id"], "text": "next" }),
    );
    assert_eq!(edited[0]["text"], "next");
    assert_eq!(app.ok("session_queue", json!({ "id": id })), edited);

    app.ok(
        "session_allow",
        json!({ "id": id, "requestId": request["request_id"] }),
    );
    // When the turn ends the waiting message goes out by itself.
    let sent = app.wait_for_text(id, "echo: next");
    assert_eq!(sent["type"], "text");
    assert_eq!(app.ok("session_queue", json!({ "id": id })), json!([]));
    // With nothing waiting, sending the next one is not an error.
    app.ok("session_send_queued", json!({ "id": id }));
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
    };
    let core = Core::with_backend(paths, Arc::new(FakeBackend::default())).expect("core opens");
    let id = core.view_put("<p>sorted</p>".to_string());

    let found = crate::view_response(&core, &format!("/{id}"));
    assert_eq!(found.status(), 200);
    assert_eq!(found.body().as_ref(), b"<p>sorted</p>");
    let policy = found.headers()["content-security-policy"]
        .to_str()
        .expect("text");
    assert!(policy.starts_with("default-src 'none'"), "{policy}");
    assert!(!policy.contains("connect-src"), "{policy}");
    assert_eq!(crate::view_response(&core, "/missing").status(), 404);
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
