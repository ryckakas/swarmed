//! The Swarmed desktop app: a thin Tauri shell over `swarmed-core`.
#![allow(
    clippy::multiple_crate_versions,
    reason = "Tauri's dependency tree carries the duplicates; nothing of ours adds one"
)]
// Without this a release build on Windows opens a console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod navigation;
mod notify;

use std::borrow::Cow;
use std::sync::Arc;

use swarmed_core::agent::FakeBackend;
use swarmed_core::{
    Core, Paths, SessionEvent, TOOL_SERVER_ARG, TerminalOutput, VIEW_POLICY, serve_session_tools,
    serve_tools,
};
use tauri::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use tauri::http::{HeaderValue, Response, StatusCode};
use tauri::{AppHandle, Builder, Emitter, Manager, Runtime};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;

/// The event every session event is emitted as.
const SESSION_EVENT: &str = "session-event";
/// Emitted when the UI fell too far behind to be sent every event and must reload history.
const SESSION_EVENTS_LAGGED: &str = "session-events-lagged";
/// The event every piece of terminal output is emitted as.
const TERMINAL_OUTPUT: &str = "terminal-output";
/// The label of the app's one window.
const MAIN_WINDOW: &str = "main";
/// The scheme frames load an agent's HTML from: `swarmed-view://localhost/<id>`.
const VIEW_SCHEME: &str = "swarmed-view";

/// `SWARMED_FAKE_AGENT` swaps Claude for the scripted agent, to work on the UI without tokens.
fn open_core() -> swarmed_core::Result<Core> {
    let paths = Paths::from_env()?;
    if std::env::var_os("SWARMED_FAKE_AGENT").is_some() {
        return Core::with_backend(paths, Arc::new(FakeBackend::default()));
    }
    let mut core = Core::open(paths)?;
    // An agent starts this same executable to reach Swarmed's own tools.
    if let Ok(executable) = std::env::current_exe() {
        core.set_tool_server(executable);
    }
    Ok(core)
}

/// Tells the user, through the system, about an event worth it, unless they are looking at
/// Swarmed already. A notification that cannot be shown is not an error: the app shows the same.
fn notify<R: Runtime>(app: &AppHandle<R>, core: &Core, event: &SessionEvent) {
    let looking = app
        .get_webview_window(MAIN_WINDOW)
        .is_some_and(|window| window.is_focused().unwrap_or(false));
    if looking {
        return;
    }
    let Ok(Some(notice)) = core.notice_for(event) else {
        return;
    };
    notify::show(&app.config().identifier, &notice);
}

/// Passes every session event on to the UI until the core goes away, and notifies the user of
/// the ones worth it.
async fn forward_events<R: Runtime>(
    mut bus: Receiver<SessionEvent>,
    app: AppHandle<R>,
    core: Arc<Core>,
) {
    loop {
        // An emit only fails when there is no window to receive it, which is not an error here.
        match bus.recv().await {
            Ok(event) => {
                notify(&app, &core, &event);
                app.emit(SESSION_EVENT, event).ok()
            }
            Err(RecvError::Lagged(_)) => app.emit(SESSION_EVENTS_LAGGED, ()).ok(),
            Err(RecvError::Closed) => return,
        };
    }
}

/// Passes on what the sessions' terminals print. Output a slow window missed is not replayed:
/// reopening the terminal redraws it from what the core kept.
async fn forward_terminals<R: Runtime>(mut bus: Receiver<TerminalOutput>, app: AppHandle<R>) {
    loop {
        match bus.recv().await {
            Ok(output) => app.emit(TERMINAL_OUTPUT, output).ok(),
            Err(RecvError::Lagged(_)) => None,
            Err(RecvError::Closed) => return,
        };
    }
}

/// Answers a frame asking for the HTML document kept under the id in `path`.
///
/// The document comes from an agent. It is served from this scheme's own origin, which holds no
/// permission to call Swarmed's commands, under a policy of its own instead of the app's.
fn view_response(core: &Core, path: &str) -> Response<Cow<'static, [u8]>> {
    let (status, body) = match core.view(path.trim_start_matches('/')) {
        Some(document) => (StatusCode::OK, document),
        None => (StatusCode::NOT_FOUND, String::new()),
    };
    let mut response = Response::new(Cow::Owned(body.into_bytes()));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    let html = HeaderValue::from_static("text/html; charset=utf-8");
    headers.insert(CONTENT_TYPE, html);
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(VIEW_POLICY),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

/// Gives a Tauri app the core and the commands that reach it. Generic over the runtime, so the
/// tests can drive the same commands without a window.
fn with_core<R: Runtime>(builder: Builder<R>, core: Arc<Core>) -> Builder<R> {
    builder
        .plugin(navigation::guard())
        .manage(Arc::clone(&core))
        .register_uri_scheme_protocol(VIEW_SCHEME, move |_context, request| {
            view_response(&core, request.uri().path())
        })
        .invoke_handler(tauri::generate_handler![
            commands::agents,
            commands::usage,
            commands::costs,
            commands::mcp_servers,
            commands::mcp_save,
            commands::mcp_remove,
            commands::agent_setups,
            commands::agent_install,
            commands::agent_configure,
            commands::agent_models_refresh,
            commands::openrouter_models,
            commands::overview,
            commands::project_add,
            commands::project_remove,
            commands::branch_choices,
            commands::task_name_for,
            commands::task_new,
            commands::environments,
            commands::templates,
            commands::template_save,
            commands::template_remove,
            commands::skills,
            commands::skill_body,
            commands::skill_save,
            commands::skill_remove,
            commands::skill_share,
            commands::stack_start,
            commands::stack_stop,
            commands::stack_log,
            commands::cloud_delegate,
            commands::cloud_tasks,
            commands::cloud_task_forget,
            commands::cloud_task_open,
            commands::cloud_task_diff,
            commands::cloud_task_apply,
            commands::environment_save,
            commands::environment_remove,
            commands::task_remove,
            commands::task_open_editor,
            commands::task_open_url,
            commands::open_link,
            commands::task_unpublished,
            commands::task_commit,
            commands::task_push,
            commands::task_pull_draft,
            commands::task_pull_create,
            commands::task_pull,
            commands::task_pull_open,
            commands::task_changes,
            commands::task_diff,
            commands::note,
            commands::set_note,
            commands::api_keys,
            commands::set_api_key,
            commands::delete_api_key,
            commands::settings,
            commands::set_settings,
            commands::session_new,
            commands::session_events,
            commands::session_send,
            commands::session_mark_read,
            commands::session_queue,
            commands::session_enqueue,
            commands::session_queue_edit,
            commands::session_send_queued,
            commands::session_allow,
            commands::session_deny,
            commands::session_interrupt,
            commands::session_stop,
            commands::session_configure,
            commands::slash_commands,
            commands::badge_set,
            commands::view_put,
            commands::view_open,
            commands::terminal_open,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_close,
            commands::status_line,
            commands::session_rename,
            commands::session_close,
            commands::sessions_search,
            commands::sessions_closed,
            commands::session_reopen,
            commands::session_fork,
        ])
}

fn main() {
    // Started by an agent as Swarmed's tool server: answer it and never open a window.
    if std::env::args().nth(1).as_deref() == Some(TOOL_SERVER_ARG) {
        let served = serve_tools(std::io::stdin().lock(), std::io::stdout().lock());
        return served.unwrap_or_else(|e| panic!("the tool server stopped: {e}"));
    }
    let core =
        Arc::new(open_core().unwrap_or_else(|e| panic!("could not open Swarmed's data: {e}")));
    let listening = Arc::clone(&core);
    let leaving = Arc::clone(&core);
    let noticing = Arc::clone(&core);
    let bus = core.subscribe();
    let terminals = core.subscribe_terminals();
    // The dialog plugin is the native folder picker, for adding a repository.
    with_core(Builder::default().plugin(tauri_plugin_dialog::init()), core)
        .setup(move |app| {
            drop(tauri::async_runtime::spawn(forward_events(
                bus,
                app.handle().clone(),
                noticing,
            )));
            // Where the sessions' own tool servers reach the core: to see, message and start
            // other sessions. If it cannot listen, those tools say so; nothing else depends on it.
            drop(tauri::async_runtime::spawn(serve_session_tools(listening)));
            drop(tauri::async_runtime::spawn(forward_terminals(
                terminals,
                app.handle().clone(),
            )));
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| panic!("could not start Swarmed: {e}"))
        .run(move |_app, event| {
            // What the tasks' `start` commands left running goes with Swarmed: nothing it
            // started is to carry on unseen.
            if matches!(event, tauri::RunEvent::Exit) {
                tauri::async_runtime::block_on(leaving.stacks_stop_all());
            }
        });
}

#[cfg(test)]
mod tests;
