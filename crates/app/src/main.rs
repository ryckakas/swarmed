//! The Swarmed desktop app: a thin Tauri shell over `swarmed-core`.
#![allow(
    clippy::multiple_crate_versions,
    reason = "Tauri's dependency tree carries the duplicates; nothing of ours adds one"
)]
// Without this a release build on Windows opens a console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod menu;
mod navigation;
mod notify;
mod update;

use std::borrow::Cow;
use std::io::Write;
use std::sync::Arc;

use swarmed_core::agent::{Auth, FakeBackend};
use swarmed_core::{
    BrowserEvent, Core, Paths, SessionEvent, TOOL_SERVER_ARG, TerminalOutput, VIEW_POLICY,
    follow_browsers, follow_others, follow_pulls, follow_put_away, follow_resets,
    serve_session_tools, serve_tools,
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
/// The event every piece of a Claude Code that a session opened prints is emitted as.
const CLAUDE_TERMINAL_OUTPUT: &str = "claude-terminal-output";
/// What the tasks' browsers show: pictures of the tab in the pane, its tabs, what it logged.
const BROWSER_EVENT: &str = "browser-event";
/// The label of the app's one window.
const MAIN_WINDOW: &str = "main";
/// The event that tells the UI which session a clicked notification was about.
const NOTICE_OPENED: &str = "notice-opened";
/// The argument that has the app check its notifications and say what it found, with no window.
const NOTIFY_CHECK_ARG: &str = "notify-check";
/// The scheme frames load an agent's HTML from, `swarmed-view://localhost/<id>`, and an MCP
/// server's app, `swarmed-view://app-<id>/`.
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

/// Whether the user has `window` in front of them. A window put away in the Dock or hidden
/// can still count as the focused one, as long as no other window of Swarmed took its place.
fn is_looked_at<R: Runtime>(window: &tauri::WebviewWindow<R>) -> bool {
    let focused = window.is_focused().unwrap_or(false);
    let put_away = window.is_minimized().unwrap_or(false);
    let shown = window.is_visible().unwrap_or(true);
    focused && shown && !put_away
}

/// Tells the user, through the system, about an event worth it, unless they are looking at
/// Swarmed already. A notification that cannot be shown is not an error: the app shows the same.
fn notify<R: Runtime>(app: &AppHandle<R>, core: &Core, event: &SessionEvent) {
    let Ok(Some(notice)) = core.notice_for(event) else {
        return;
    };
    let window = app.get_webview_window(MAIN_WINDOW);
    let looking = window.as_ref().is_some_and(is_looked_at);
    notify::trace(&format!("{}: looking={looking}", notice.title));
    if looking {
        return;
    }
    let app = app.clone();
    notify::show(
        &notice,
        Box::new(move |session| open_session(&app, session)),
    );
}

/// Brings Swarmed to the front on the session a notification was about.
fn open_session<R: Runtime>(app: &AppHandle<R>, session: i64) {
    bring_forward(app);
    app.emit(NOTICE_OPENED, session).ok();
}

/// Shows Swarmed's window again, wherever it was put: hidden, or in the Dock.
fn bring_forward<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        window.unminimize().ok();
        window.show().ok();
        window.set_focus().ok();
    }
}

/// Hides Swarmed's window without ending anything.
fn put_away<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        window.hide().ok();
    }
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

/// Passes on what the sessions' terminals print, as `event`. Output a slow window missed is
/// not replayed: reopening the terminal redraws it from what the core kept.
async fn forward_terminals<R: Runtime>(
    mut bus: Receiver<TerminalOutput>,
    app: AppHandle<R>,
    event: &'static str,
) {
    loop {
        match bus.recv().await {
            Ok(output) => app.emit(event, output).ok(),
            Err(RecvError::Lagged(_)) => None,
            Err(RecvError::Closed) => return,
        };
    }
}

/// Passes on what the tasks' browsers show. Pictures a slow window missed are not sent again:
/// the next one draws the whole page.
async fn forward_browsers<R: Runtime>(mut bus: Receiver<BrowserEvent>, app: AppHandle<R>) {
    loop {
        match bus.recv().await {
            Ok(event) => app.emit(BROWSER_EVENT, event).ok(),
            Err(RecvError::Lagged(_)) => None,
            Err(RecvError::Closed) => return,
        };
    }
}

/// What the host of an app's address starts with: `swarmed-view://app-<id>/`. Each app is
/// served from a host of its own, so that no app shares an origin with another or a view.
const APP_HOST: &str = "app-";

/// Answers a frame asking for a document: an app an MCP server attached to a tool call, by
/// the id in `host` (`app-<id>`), else an agent's view, by the id in `path`.
///
/// The document comes from an agent or a server. It is served from this scheme's own origin,
/// which holds no permission to call Swarmed's commands, under a policy of its own instead of
/// the app's: a view under [`VIEW_POLICY`], an app under the one the core built from what its
/// server declared.
fn view_response(core: &Core, host: &str, path: &str) -> Response<Cow<'static, [u8]>> {
    let found = match host.strip_prefix(APP_HOST) {
        // An app is one document at its root: nothing else is served from its host.
        Some(app) => (path.trim_start_matches('/').is_empty())
            .then(|| core.app_document(app))
            .flatten()
            .map(|(document, policy)| (document, HeaderValue::from_str(&policy).ok())),
        None => core
            .view(path.trim_start_matches('/'))
            .map(|document| (document, Some(HeaderValue::from_static(VIEW_POLICY)))),
    };
    // A policy that cannot be sent as a header serves nothing.
    let (status, body, policy) = match found {
        Some((document, Some(policy))) => (StatusCode::OK, document, policy),
        _ => (
            StatusCode::NOT_FOUND,
            String::new(),
            HeaderValue::from_static(VIEW_POLICY),
        ),
    };
    let mut response = Response::new(Cow::Owned(body.into_bytes()));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    let html = HeaderValue::from_static("text/html; charset=utf-8");
    headers.insert(CONTENT_TYPE, html);
    headers.insert(CONTENT_SECURITY_POLICY, policy);
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

/// Gives a Tauri app the core, the guard on where its window may go, and the scheme agents'
/// views are served from.
fn with_state<R: Runtime>(builder: Builder<R>, core: Arc<Core>) -> Builder<R> {
    builder
        .plugin(navigation::guard())
        .manage(Arc::clone(&core))
        .manage(update::Updates::default())
        .register_uri_scheme_protocol(VIEW_SCHEME, move |_context, request| {
            let uri = request.uri();
            view_response(&core, uri.host().unwrap_or_default(), uri.path())
        })
}

/// Gives a Tauri app the core and the commands that reach it. Generic over the runtime, so the
/// tests can drive the same commands without a window.
///
/// The commands are listed several to a line, and left that way: one to a line, the list alone
/// is longer than a function may be.
#[rustfmt::skip]
fn with_core<R: Runtime>(builder: Builder<R>, core: Arc<Core>) -> Builder<R> {
    with_state(builder, core).invoke_handler(tauri::generate_handler![
        commands::agents, commands::usage, commands::cleanup, commands::cleanup_clean,
        commands::costs, commands::analytics,
        commands::effort_default, commands::mcp_servers, commands::mcp_save,
        commands::mcp_remove, commands::agent_setups, commands::agent_install,
        commands::agent_configure, commands::agent_models_refresh, commands::openrouter_models,
        commands::overview, commands::project_add, commands::project_remove,
        commands::project_move, commands::task_move, commands::session_move,
        commands::cloud_task_move, commands::project_pin, commands::task_pin, commands::session_pin,
        commands::task_highlight, commands::session_highlight,
        commands::branch_choices, commands::task_name_for, commands::task_new,
        commands::task_start, commands::task_setup, commands::task_setup_stop,
        commands::task_start_anyway, commands::task_setup_log,
        commands::environments, commands::templates, commands::template_save,
        commands::template_remove, commands::actions, commands::action_save,
        commands::action_remove, commands::action_move, commands::session_mark, commands::config_answer,
        commands::skills, commands::skill_body, commands::skill_save, commands::skill_remove,
        commands::skill_share, commands::stack_start, commands::stack_stop, commands::stack_log,
        commands::cloud_delegate,
            commands::cloud_delegate_conversation, commands::cloud_tasks, commands::cloud_task_forget,
        commands::cloud_task_open, commands::cloud_task_diff, commands::cloud_task_apply,
        commands::environment_save, commands::environment_remove, commands::environment_default,
        commands::environment_set_default, commands::snapshots,
        commands::snapshot_save, commands::snapshot_remove, commands::task_remove,
        commands::task_done, commands::task_undone,
        commands::task_open_editor, commands::task_open_terminal, commands::task_files, commands::task_open_file, commands::task_open_url, commands::open_link,
        commands::task_unpublished, commands::task_commit, commands::task_commit_message, commands::task_pull_message,
        commands::task_push, commands::task_switch, commands::task_pull_draft, commands::task_pull_create,
        commands::task_pull, commands::task_pull_failure, commands::task_pull_open,
        commands::project_merge_rules, commands::task_pull_merge, commands::task_pull_auto_merge_off,
        commands::project_issues, commands::project_issue, commands::task_issue,
        commands::task_changes,
        commands::task_diff, commands::session_turns, commands::session_turn_changes,
        commands::session_turn_diff, commands::session_rewind_check, commands::session_rewind,
        commands::session_files_put_back, commands::note, commands::set_note, commands::api_keys,
        commands::set_api_key, commands::delete_api_key, commands::settings,
        commands::config_problems,
        commands::set_settings, commands::branch_example, commands::custom_layout, commands::custom_layout_save, commands::terminal_config, commands::terminal_config_save, commands::session_new, commands::session_events, commands::session_reachable, commands::project_reachable, commands::project_github_items, commands::project_github_read,
        commands::session_send, commands::session_images_keep, commands::session_image,
        commands::session_image_open, commands::attachments_root,
        commands::session_mark_read, commands::session_queue,
        commands::session_enqueue, commands::session_queue_edit, commands::session_queue_hold,
        commands::session_queue_let_go, commands::session_send_queued,
        commands::session_send_now, commands::session_message_now, commands::session_tell_now,
        commands::session_allow, commands::session_answer, commands::session_command_answer, commands::session_deny, commands::session_interrupt,
        commands::session_stop_background, commands::session_send_to_background,
        commands::task_background,
        commands::session_background_output,
        commands::session_configure, commands::slash_commands, commands::badge_set,
        commands::view_put, commands::view_open, commands::app_put, commands::session_app_call,
        commands::terminal_open,
        commands::terminal_write, commands::terminal_resize, commands::claude_terminal_open,
        commands::claude_terminal_write, commands::claude_terminal_resize,
        commands::claude_terminal_close,
        commands::status_line, commands::account_usage, commands::session_usage_refresh, commands::session_rename, commands::session_close, commands::session_delete,
        commands::session_remote_control, commands::session_continue_at_reset,
        commands::session_continue_cancel,
        commands::sessions_search, commands::sessions_closed, commands::session_reopen,
        commands::session_fork, commands::session_fork_into, commands::session_set_agent,
        commands::update_state, commands::update_check, commands::update_restart,
        commands::browser_setup, commands::browser_install, commands::browser_state,
        commands::browser_open, commands::browser_stop, commands::browser_show,
        commands::browser_hide, commands::browser_raise, commands::browser_mouse,
        commands::browser_key,
        commands::browser_text, commands::browser_go, commands::browser_tab_new,
        commands::browser_tab_close, commands::browser_dialog, commands::browser_console,
        commands::browser_pick, commands::browser_pick_end, commands::session_elements,
    ])
}

fn main() {
    // Started by an agent as Swarmed's tool server: answer it and never open a window.
    if std::env::args().nth(1).as_deref() == Some(TOOL_SERVER_ARG) {
        let served = serve_tools(std::io::stdin().lock(), std::io::stdout().lock());
        return served.unwrap_or_else(|e| panic!("the tool server stopped: {e}"));
    }
    if std::env::args().nth(1).as_deref() == Some(NOTIFY_CHECK_ARG) {
        let found = tauri::async_runtime::block_on(notify::check());
        return writeln!(std::io::stdout(), "{found}").unwrap_or_default();
    }
    // Started from the Finder or the Dock, the app has the system's bare `PATH`: ask the
    // user's shell for theirs before anything is looked up or started.
    if !tauri::is_dev() {
        swarmed_core::learn_shell_path();
    }
    let core = open_core().unwrap_or_else(|e| panic!("could not open Swarmed's data: {e}"));
    // No agent runs yet: a session that still counts as busy was left so by a Swarmed that
    // ended in the middle of its turn, and would never be anything else. Unless another
    // Swarmed runs beside this one, which the core knows.
    core.arrive().ok();
    let core = Arc::new(core);
    let listening = Arc::clone(&core);
    let leaving = Arc::clone(&core);
    let noticing = Arc::clone(&core);
    let resuming = Arc::clone(&core);
    let following = Arc::clone(&core);
    let resetting = Arc::downgrade(&core);
    let pulling = Arc::downgrade(&core);
    let tidying = Arc::downgrade(&core);
    let browsing = Arc::downgrade(&core);
    let bus = core.subscribe();
    let terminals = core.subscribe_terminals();
    let claude_terminals = core.subscribe_claude_terminals();
    let browsers = core.subscribe_browsers();
    // The dialog plugin is the native folder picker, for adding a repository; the updater
    // fetches and checks new versions (`update.rs`).
    let plugins = Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build());
    with_core(plugins, core)
        .setup(move |app| {
            drop(tauri::async_runtime::spawn(forward_events(
                bus,
                app.handle().clone(),
                noticing,
            )));
            // Where the sessions' own tool servers reach the core: to see, message and start
            // other sessions. If it cannot listen, those tools say so; nothing else depends on it.
            drop(tauri::async_runtime::spawn(serve_session_tools(listening)));
            // What the command line or another Swarmed on the same data writes reaches the
            // window as if this core had written it, and is noticed within a second.
            drop(tauri::async_runtime::spawn(follow_others(following)));
            // Sessions a usage limit stopped go on by themselves when it resets, where the user
            // asked for that. Only the first Swarmed on the data sends them.
            drop(tauri::async_runtime::spawn(follow_resets(resetting)));
            // The pull requests of tasks are followed, and what changed on them is said once.
            // Only the first Swarmed on the data asks GitHub.
            drop(tauri::async_runtime::spawn(follow_pulls(pulling)));
            // Finished tasks are put away by themselves, where the user's settings say so. Only
            // the first Swarmed on the data does this.
            drop(tauri::async_runtime::spawn(follow_put_away(tidying)));
            // The tasks' browsers that sat unused are stopped. Only the first Swarmed on the data
            // does this.
            drop(tauri::async_runtime::spawn(follow_browsers(browsing)));
            drop(tauri::async_runtime::spawn(forward_browsers(
                browsers,
                app.handle().clone(),
            )));
            // Sessions that had Remote Control on can be reached from claude.ai again, without
            // anyone writing to them here first. Only the first Swarmed on the data does this.
            drop(tauri::async_runtime::spawn(async move {
                resuming.remote_controls_resume(&Auth::default()).ok();
            }));
            // New versions, looked for by themselves only if the user wants that.
            update::watch(app.handle().clone());
            // Asked once, by the system; from then on its settings hold the answer.
            notify::ask_permission();
            // In place of the default menu bar, whose About is the system's bare panel.
            if cfg!(target_os = "macos") {
                app.set_menu(menu::build(app.handle())?)?;
                app.on_menu_event(|app, event| menu::chosen(app, &event));
            }
            drop(tauri::async_runtime::spawn(forward_terminals(
                terminals,
                app.handle().clone(),
                TERMINAL_OUTPUT,
            )));
            drop(tauri::async_runtime::spawn(forward_terminals(
                claude_terminals,
                app.handle().clone(),
                CLAUDE_TERMINAL_OUTPUT,
            )));
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| panic!("could not start Swarmed: {e}"))
        .run(move |app, event| match event {
            // What the tasks' `start` commands left running goes with Swarmed: nothing it
            // started is to carry on unseen.
            // A new version that is ready goes in now, as Swarmed goes. Agents it started may
            // still be on their way out: they run on from the files they were started from.
            tauri::RunEvent::Exit => {
                tauri::async_runtime::block_on(leaving.leave());
                update::install_ready(app);
            }
            // Closing the window puts Swarmed away, as it does for most apps on a Mac: the
            // sessions carry on and tell the user when they need them. Quitting is ⌘Q.
            tauri::RunEvent::WindowEvent {
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } if cfg!(target_os = "macos") => {
                api.prevent_close();
                put_away(app);
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => bring_forward(app),
            _ => {}
        });
}

#[cfg(test)]
mod tests;
