//! The commands the UI can call. Each one forwards to `swarmed-core` and adds nothing.
#![allow(
    clippy::needless_pass_by_value,
    reason = "Tauri hands commands their arguments, `State` included, by value"
)]

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use swarmed_core::agent::{AgentEvent, Auth, Supervision};
use swarmed_core::{
    AgentInfo, ApiKeyInfo, BranchChoices, Core, FileChange, FileDiff, NewTaskOptions, Project,
    ProjectOverview, RemoveOptions, Session, SessionOptions, SessionSettings, Settings, Task,
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

#[tauri::command]
pub(crate) async fn project_add(
    core: CoreState<'_>,
    path: PathBuf,
    name: Option<String>,
) -> Command<Project> {
    Ok(core.project_add(&path, name.as_deref()).await?)
}

#[tauri::command]
pub(crate) async fn branch_choices(core: CoreState<'_>, project: String) -> Command<BranchChoices> {
    Ok(core.branch_choices(&project).await?)
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
) -> Command<Session> {
    let options = SessionOptions {
        agent,
        title: None,
        model,
        supervision,
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
) -> Command<Session> {
    let settings = SessionSettings { model, supervision };
    Ok(core.session_configure(id, &settings)?)
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
