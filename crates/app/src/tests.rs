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

    /// Whether a call of `command` carrying `key` is answered at all. A command that is not
    /// `async` has answered by the time it was handed over.
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
        answer.try_recv().unwrap_or(false)
    }

    /// The folder Swarmed keeps its own files in, beside the repository.
    fn data_dir(&self) -> std::path::PathBuf {
        self.repo.with_file_name("data")
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
        json!({ "project": "demo", "task": "one", "agent": null, "how": { "model": null, "supervision": "ask", "effort": null } }),
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

    // The finished turn is unread until the UI says the user has seen it.
    let unread =
        |app: &App| app.ok("overview", json!({}))[0]["tasks"][1]["sessions"][0]["unread"].clone();
    assert_eq!(unread(&app), true);
    app.ok("session_mark_read", json!({ "id": id }));
    assert_eq!(unread(&app), false);
}

/// Closed, a session is still kept; deleted, it is gone for good.
fn deleted_for_good(app: &App, id: &Value) {
    assert_eq!(app.ok("overview", json!({}))[0]["tasks"][0]["closed"], 1);
    app.ok("session_delete", json!({ "id": id }));
    assert_eq!(app.ok("overview", json!({}))[0]["tasks"][0]["closed"], 0);
    assert!(app.invoke("session_events", json!({ "id": id })).is_err());
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
    let size = json!({ "id": id, "cols": 80, "rows": 24 });
    assert_eq!(
        app.ok("terminal_open", size.clone()),
        json!({ "replay": "", "end": 0 })
    );
    app.ok("terminal_write", json!({ "id": id, "data": "true\n" }));
    app.ok("terminal_resize", size);
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
fn the_ui_can_commit_a_tasks_work_and_is_told_what_cannot_be_published() {
    let app = app();
    app.ok("project_add", json!({ "path": app.repo, "name": "demo" }));
    let task = json!({ "project": "demo", "name": "one", "branch": null });
    app.ok("task_new", task);
    let worktree = app.ok("overview", json!({}))[0]["tasks"][0]["worktree_path"].clone();
    let worktree = Path::new(worktree.as_str().expect("a path"));
    std::fs::write(worktree.join("notes.txt"), "one\n").expect("writes");
    let target = json!({ "project": "demo", "name": "one" });
    // A branch that was never pushed has no pull request, and opening it says so.
    assert_eq!(app.ok("task_pull", target.clone()), json!(null));
    let opening = json!({ "project": "demo", "name": "one", "check": null });
    let refused = app
        .invoke("task_pull_open", opening)
        .expect_err("nothing to open");
    assert_eq!(refused, json!("`one` has no pull request"));
    // The file written above is not committed yet; committing it leaves one commit to push.
    assert_eq!(
        app.ok("task_unpublished", target.clone()),
        json!({ "changed": 1, "unpushed": 0, "pushed_before": false })
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
        json!({ "changed": 0, "unpushed": 1, "pushed_before": false })
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
            "html_views": true, "notifications": true, "default_supervision": "auto",
            "status_items": ["branch", "changes", "model", "context", "cost"],
            "status_command": "", "rules": "",
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
    app.ok("session_close", json!({ "id": id }));

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
    });
    let save = json!({ "project": "demo", "name": "cached", "environment": environment });
    assert_eq!(app.ok("environment_save", save), json!(null));
    assert_eq!(
        app.ok("environments", project.clone())["cached"],
        environment
    );

    let task = app.ok(
        "task_new",
        json!({ "project": "demo", "name": "one", "branch": null, "environment": "cached" }),
    );
    assert_eq!(
        (&task["environment"], &task["setup"]),
        (&json!("cached"), &json!("ok"))
    );
    assert_eq!(app.ok("snapshots", project.clone()), json!([]));
    let taken = app.ok("snapshot_save", json!({ "project": "demo", "task": "one" }));
    assert_eq!(
        (&taken["environment"], &taken["task"], &taken["entries"]),
        (&json!("cached"), &json!("one"), &json!(1))
    );
    assert_eq!(app.ok("snapshots", project.clone()), json!([taken]));
    let forget = json!({ "project": "demo", "environment": "cached" });
    assert_eq!(app.ok("snapshot_remove", forget), json!(null));
    assert_eq!(app.ok("snapshots", project.clone()), json!([]));

    let remove = json!({ "project": "demo", "name": "cached" });
    assert_eq!(app.ok("environment_remove", remove), json!(null));
    assert_eq!(app.ok("environments", project), json!({}));
    let unnamed = json!({ "project": "demo", "name": "Bad Name", "environment": environment });
    app.invoke("environment_save", unnamed)
        .expect_err("refused");
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
