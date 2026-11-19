//! The commands the UI can call. Each one forwards to `swarmed-core` and adds nothing.
#![allow(
    clippy::needless_pass_by_value,
    reason = "Tauri hands commands their arguments, `State` included, by value"
)]

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use swarmed_core::agent::{AgentEvent, Auth, Effort, Supervision};
use swarmed_core::{
    AgentInfo, AgentSetup, ApiKeyInfo, BranchChoices, Core, FileChange, FileDiff, NewTaskOptions,
    Project, ProjectOverview, QueuedMessage, RemoveOptions, Session, SessionOptions,
    SessionSettings, Settings, Task, TaskNaming, Usage,
};
use tauri::{AppHandle, Emitter, Runtime, State};

/// The event a hook's output lines are emitted as while a task is created or removed.
pub(crate) const HOOK_OUTPUT: &str = "hook-output";

/// A core error as the UI sees it: its message.
#[derive(Debug, Serialize)]
pub(crate) struct CommandError(String);

impl From<swarmed_core::Error> for CommandError {
    fn from(error: swarmed_core::Error) -> Self {
        Self(error.to_string())
    }
}

type Command<T> = Result<T, CommandError>;
type CoreState<'a> = State<'a, Arc<Core>>;

/// One line a hook printed, with the task it belongs to.
#[derive(Debug, Clone, Serialize)]
struct HookOutput<'a> {
    project: &'a str,
    task: &'a str,
    line: &'a str,
}

/// A sink for [`Core::task_new`] and [`Core::task_remove`] that shows hook output in the UI.
fn hook_sink<'a, R: Runtime>(
    app: &'a AppHandle<R>,
    project: &'a str,
    task: &'a str,
) -> impl Fn(&str) + 'a {
    move |line| {
        // A window that has gone away is no reason to fail the hook.
        app.emit(
            HOOK_OUTPUT,
            HookOutput {
                project,
                task,
                line,
            },
        )
        .ok();
    }
}

#[tauri::command]
pub(crate) fn agents(core: CoreState<'_>) -> Vec<AgentInfo> {
    core.agents()
}

#[tauri::command]
pub(crate) fn overview(core: CoreState<'_>) -> Command<Vec<ProjectOverview>> {
    Ok(core.overview()?)
}

/// What Swarmed takes up on disk and is using right now. Async: it measures folders.
#[tauri::command]
pub(crate) async fn usage(core: CoreState<'_>) -> Command<Usage> {
    Ok(core.usage().await?)
}

/// The agents that can be set up from the settings, and how far along each is.
#[tauri::command]
pub(crate) fn agent_setups(core: CoreState<'_>) -> Command<Vec<AgentSetup>> {
    Ok(core.agent_setups()?)
}

/// Installs a known agent. What the installer prints reaches the UI the way hook output does.
#[tauri::command]
pub(crate) async fn agent_install<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
    id: String,
) -> Command<AgentSetup> {
    let sink = hook_sink(&app, "", &id);
    Ok(core.agent_install(&id, &sink).await?)
}

/// Writes a known agent's entry, with these models to offer if any are given.
#[tauri::command]
pub(crate) fn agent_configure(
    core: CoreState<'_>,
    id: String,
    models: Option<Vec<String>>,
) -> Command<AgentSetup> {
    Ok(core.agent_configure(&id, models)?)
}

/// Every model OpenRouter offers right now, for choosing from.
#[tauri::command]
pub(crate) async fn openrouter_models() -> Command<Vec<String>> {
    Ok(swarmed_core::openrouter_models().await?)
}

#[tauri::command]
pub(crate) async fn project_add(
    core: CoreState<'_>,
    path: PathBuf,
    name: Option<String>,
) -> Command<Project> {
    Ok(core.project_add(&path, name.as_deref()).await?)
}

#[tauri::command]
pub(crate) async fn project_remove<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
    name: String,
    force: bool,
) -> Command<()> {
    let sink = hook_sink(&app, &name, "");
    Ok(core.project_remove(&name, force, &sink).await?)
}

#[tauri::command]
pub(crate) async fn branch_choices(core: CoreState<'_>, project: String) -> Command<BranchChoices> {
    Ok(core.branch_choices(&project).await?)
}

/// A name for a new task and a title for its first session, or nothing if the UI is to name
/// the task itself.
#[tauri::command]
pub(crate) async fn task_name_for(
    core: CoreState<'_>,
    project: String,
    prompt: String,
) -> Command<Option<TaskNaming>> {
    Ok(core.task_name_for(&project, &prompt).await?)
}

#[tauri::command]
pub(crate) async fn task_new<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
    project: String,
    name: String,
    branch: Option<String>,
) -> Command<Task> {
    let options = NewTaskOptions {
        existing_branch: branch.as_deref(),
    };
    let sink = hook_sink(&app, &project, &name);
    Ok(core.task_new(&project, &name, options, &sink).await?)
}

#[tauri::command]
pub(crate) async fn task_remove<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
    project: String,
    name: String,
    force: bool,
    delete_branch: bool,
) -> Command<()> {
    let options = RemoveOptions {
        force,
        delete_branch,
    };
    let sink = hook_sink(&app, &project, &name);
    Ok(core.task_remove(&project, &name, options, &sink).await?)
}

#[tauri::command]
pub(crate) fn task_open_editor(core: CoreState<'_>, project: String, name: String) -> Command<()> {
    Ok(core.task_open_editor(&project, &name)?)
}

#[tauri::command]
pub(crate) fn note(core: CoreState<'_>, project: String, task: Option<String>) -> Command<String> {
    Ok(core.note(&project, task.as_deref())?)
}

#[tauri::command]
pub(crate) fn set_note(
    core: CoreState<'_>,
    project: String,
    task: Option<String>,
    text: String,
) -> Command<()> {
    Ok(core.set_note(&project, task.as_deref(), &text)?)
}

#[tauri::command]
pub(crate) fn task_open_url(core: CoreState<'_>, project: String, name: String) -> Command<()> {
    Ok(core.task_open_url(&project, &name)?)
}

#[tauri::command]
pub(crate) async fn task_changes(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<Vec<FileChange>> {
    Ok(core.task_changes(&project, &name).await?)
}

#[tauri::command]
pub(crate) async fn task_diff(
    core: CoreState<'_>,
    project: String,
    name: String,
    path: String,
) -> Command<FileDiff> {
    Ok(core.task_diff(&project, &name, &path).await?)
}

#[tauri::command]
pub(crate) fn settings(core: CoreState<'_>) -> Command<Settings> {
    Ok(core.settings()?)
}

#[tauri::command]
pub(crate) fn set_settings(core: CoreState<'_>, settings: Settings) -> Command<()> {
    Ok(core.set_settings(&settings)?)
}

#[tauri::command]
pub(crate) fn api_keys(core: CoreState<'_>) -> Command<Vec<ApiKeyInfo>> {
    Ok(core.api_keys()?)
}

#[tauri::command]
pub(crate) fn set_api_key(core: CoreState<'_>, name: String, key: String) -> Command<()> {
    Ok(core.set_api_key(&name, &key)?)
}

#[tauri::command]
pub(crate) fn delete_api_key(core: CoreState<'_>, name: String) -> Command<()> {
    Ok(core.delete_api_key(&name)?)
}

#[tauri::command]
pub(crate) fn session_new(
    core: CoreState<'_>,
    project: String,
    task: String,
    agent: Option<String>,
    model: Option<String>,
    supervision: Supervision,
    effort: Option<Effort>,
) -> Command<Session> {
    let options = SessionOptions {
        agent,
        title: None,
        model,
        supervision,
        effort,
    };
    Ok(core.session_new(&project, &task, &options)?)
}

#[tauri::command]
pub(crate) fn session_events(core: CoreState<'_>, id: i64) -> Command<Vec<AgentEvent>> {
    Ok(core.session_events(id)?)
}

/// Starts the session's agent if it is not running, then sends the message.
///
/// Async so that it runs on the runtime the core spawns the agent's tasks on.
#[tauri::command]
pub(crate) async fn session_send(core: CoreState<'_>, id: i64, text: String) -> Command<()> {
    core.session_start(id, Auth::Subscription)?;
    Ok(core.session_send(id, &text)?)
}

#[tauri::command]
pub(crate) fn session_queue(core: CoreState<'_>, id: i64) -> Command<Vec<QueuedMessage>> {
    Ok(core.session_queue(id)?)
}

#[tauri::command]
pub(crate) fn session_enqueue(
    core: CoreState<'_>,
    id: i64,
    text: String,
) -> Command<Vec<QueuedMessage>> {
    Ok(core.session_enqueue(id, &text)?)
}

#[tauri::command]
pub(crate) fn session_queue_edit(
    core: CoreState<'_>,
    id: i64,
    message: u64,
    text: String,
) -> Command<Vec<QueuedMessage>> {
    Ok(core.session_queue_edit(id, message, &text)?)
}

/// Async for the same reason as `session_send`, which it is a form of.
#[tauri::command]
pub(crate) async fn session_send_queued(core: CoreState<'_>, id: i64) -> Command<()> {
    core.session_start(id, Auth::Subscription)?;
    Ok(core.session_send_queued(id)?)
}

#[tauri::command]
pub(crate) fn session_mark_read(core: CoreState<'_>, id: i64) -> Command<()> {
    Ok(core.session_mark_read(id)?)
}

#[tauri::command]
pub(crate) fn session_allow(core: CoreState<'_>, id: i64, request_id: String) -> Command<()> {
    Ok(core.session_allow(id, &request_id)?)
}

#[tauri::command]
pub(crate) fn session_deny(
    core: CoreState<'_>,
    id: i64,
    request_id: String,
    reason: String,
) -> Command<()> {
    Ok(core.session_deny(id, &request_id, &reason)?)
}

#[tauri::command]
pub(crate) fn session_interrupt(core: CoreState<'_>, id: i64) -> Command<()> {
    Ok(core.session_interrupt(id)?)
}

#[tauri::command]
pub(crate) fn session_stop(core: CoreState<'_>, id: i64) {
    core.session_stop(id);
}

#[tauri::command]
pub(crate) fn slash_commands(core: CoreState<'_>, id: Option<i64>) -> Command<Vec<String>> {
    Ok(core.slash_commands(id)?)
}

#[tauri::command]
pub(crate) fn session_configure(
    core: CoreState<'_>,
    id: i64,
    model: Option<String>,
    supervision: Supervision,
    effort: Option<Effort>,
) -> Command<Session> {
    let settings = SessionSettings {
        model,
        supervision,
        effort,
    };
    Ok(core.session_configure(id, &settings)?)
}

/// Hands over an HTML document for a sandboxed frame and returns the id the frame loads it by.
#[tauri::command]
pub(crate) fn view_put(core: CoreState<'_>, document: String) -> String {
    core.view_put(document)
}

/// Opens an HTML document an agent wrote in the user's browser.
#[tauri::command]
pub(crate) fn view_open(core: CoreState<'_>, document: String) -> Command<()> {
    Ok(core.view_open(&document)?)
}

/// Async so that it runs on the runtime the core spawns the terminal's tasks on.
#[tauri::command]
pub(crate) async fn terminal_open(
    core: CoreState<'_>,
    id: i64,
    cols: u16,
    rows: u16,
) -> Command<String> {
    Ok(core.terminal_open(id, cols, rows)?)
}

#[tauri::command]
pub(crate) fn terminal_write(core: CoreState<'_>, id: i64, data: String) -> Command<()> {
    Ok(core.terminal_write(id, &data)?)
}

#[tauri::command]
pub(crate) fn terminal_resize(core: CoreState<'_>, id: i64, cols: u16, rows: u16) -> Command<()> {
    Ok(core.terminal_resize(id, cols, rows)?)
}

#[tauri::command]
pub(crate) fn terminal_close(core: CoreState<'_>, id: i64) {
    core.terminal_close(id);
}

#[tauri::command]
pub(crate) async fn status_line(core: CoreState<'_>, id: i64) -> Command<Option<String>> {
    Ok(core.status_line(id).await?)
}

#[tauri::command]
pub(crate) fn session_rename(core: CoreState<'_>, id: i64, title: String) -> Command<Session> {
    Ok(core.session_rename(id, &title)?)
}

#[tauri::command]
pub(crate) fn session_close(core: CoreState<'_>, id: i64) -> Command<()> {
    Ok(core.session_close(id)?)
}
