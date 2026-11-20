//! The commands the UI can call. Each one forwards to `swarmed-core` and adds nothing.
#![allow(
    clippy::needless_pass_by_value,
    reason = "Tauri hands commands their arguments, `State` included, by value"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use swarmed_core::agent::{AgentEvent, Auth, Effort, Supervision};
use swarmed_core::{
    AgentInfo, AgentSetup, ApiKeyInfo, BackgroundTask, BranchChoices, CloudOrigin, CloudProvider,
    CloudTask, Core, Costs, Environment, FileChange, FileDiff, McpServerInput, McpServerView,
    NewTaskOptions, Project, ProjectOverview, Pull, PullDraft, QueuedMessage, RemoveOptions,
    SearchHit, Session, SessionOptions, SessionSettings, Settings, Skill, Snapshot, Task,
    TaskNaming, Template, TurnStats, Unpublished, Usage, with_files,
};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

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

/// Every MCP server Swarmed knows of, with secrets by name only.
#[tauri::command]
pub(crate) fn mcp_servers(core: CoreState<'_>) -> Command<Vec<McpServerView>> {
    Ok(core.mcp_servers()?)
}

/// Adds a server set up in Swarmed, or replaces the one of that name.
#[tauri::command]
pub(crate) fn mcp_save(core: CoreState<'_>, server: McpServerInput) -> Command<Vec<McpServerView>> {
    Ok(core.mcp_save(&server)?)
}

/// Removes a server set up in Swarmed, with its secrets.
#[tauri::command]
pub(crate) fn mcp_remove(core: CoreState<'_>, name: String) -> Command<Vec<McpServerView>> {
    Ok(core.mcp_remove(&name)?)
}

/// What Swarmed takes up on disk and is using right now. Async: it measures folders.
#[tauri::command]
pub(crate) async fn usage(core: CoreState<'_>) -> Command<Usage> {
    Ok(core.usage().await?)
}

/// What the agents report their work has cost: today, this week, in all, by task and by day.
#[tauri::command]
pub(crate) fn costs(core: CoreState<'_>) -> Command<Costs> {
    Ok(core.costs()?)
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

/// Asks an agent that knows its own models which ones it offers, and offers those.
#[tauri::command]
pub(crate) async fn agent_models_refresh(core: CoreState<'_>, id: String) -> Command<AgentSetup> {
    Ok(core.agent_models_refresh(&id).await?)
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
    environment: Option<String>,
) -> Command<Task> {
    let options = NewTaskOptions {
        existing_branch: branch.as_deref(),
        environment: environment.as_deref(),
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

/// Hands a prompt to a cloud agent working on the project's repository, starting from the
/// branch of `task` if one is named and GitHub has it.
#[tauri::command]
pub(crate) async fn cloud_delegate(
    core: CoreState<'_>,
    project: String,
    prompt: String,
    task: Option<String>,
    provider: CloudProvider,
) -> Command<CloudTask> {
    let origin = CloudOrigin {
        task: task.as_deref(),
        session: None,
        provider,
    };
    Ok(core.cloud_delegate(&project, &prompt, origin).await?)
}

/// The cloud tasks delegated from a project, with their statuses asked for again if wanted.
#[tauri::command]
pub(crate) async fn cloud_tasks(
    core: CoreState<'_>,
    project: String,
    refresh: bool,
) -> Command<Vec<CloudTask>> {
    Ok(core.cloud_tasks(&project, refresh).await?)
}

/// Takes a cloud task off Swarmed's list, without stopping it.
#[tauri::command]
pub(crate) fn cloud_task_forget(core: CoreState<'_>, id: i64) -> Command<()> {
    Ok(core.cloud_task_forget(id)?)
}

/// What a Codex cloud task changed, as a unified diff.
#[tauri::command]
pub(crate) async fn cloud_task_diff(core: CoreState<'_>, id: i64) -> Command<String> {
    Ok(core.cloud_task_diff(id).await?)
}

/// Applies what a Codex cloud task changed to a local task's worktree, uncommitted.
#[tauri::command]
pub(crate) async fn cloud_task_apply(
    core: CoreState<'_>,
    id: i64,
    project: String,
    name: String,
) -> Command<()> {
    Ok(core.cloud_task_apply(id, &project, &name).await?)
}

/// Opens an address in the default browser: a cloud task's page.
#[tauri::command]
pub(crate) fn cloud_task_open(core: CoreState<'_>, id: i64) -> Command<()> {
    Ok(core.cloud_task_open(id)?)
}

/// Starts what a task keeps running, if its project has a `start` command. Says whether it has.
#[tauri::command]
pub(crate) fn stack_start(core: CoreState<'_>, project: String, task: String) -> Command<bool> {
    Ok(core.stack_start(&project, &task)?)
}

/// Stops what a task keeps running.
#[tauri::command]
pub(crate) async fn stack_stop<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
    project: String,
    task: String,
) -> Command<()> {
    let sink = hook_sink(&app, &project, &task);
    Ok(core.stack_stop(&project, &task, &sink).await?)
}

/// The end of what a task's stack has printed since it was last started.
#[tauri::command]
pub(crate) fn stack_log(core: CoreState<'_>, project: String, task: String) -> Command<String> {
    Ok(core.stack_log(&project, &task)?)
}

/// Every skill Swarmed knows of: its own, and each agent's.
#[tauri::command]
pub(crate) fn skills(core: CoreState<'_>) -> Vec<Skill> {
    core.skills()
}

/// What a skill of Swarmed's own says to do, for editing.
#[tauri::command]
pub(crate) fn skill_body(core: CoreState<'_>, name: String) -> Command<String> {
    Ok(core.skill_body(&name)?)
}

/// The prompts the user keeps to send again.
#[tauri::command]
pub(crate) fn templates(core: CoreState<'_>) -> Command<Vec<Template>> {
    Ok(core.templates()?)
}

/// Keeps a prompt, in place of the one of that name if there is one.
#[tauri::command]
pub(crate) fn template_save(
    core: CoreState<'_>,
    name: String,
    text: String,
) -> Command<Vec<Template>> {
    Ok(core.template_save(&name, &text)?)
}

/// Drops a kept prompt.
#[tauri::command]
pub(crate) fn template_remove(core: CoreState<'_>, name: String) -> Command<Vec<Template>> {
    Ok(core.template_remove(&name)?)
}

/// Adds a skill of Swarmed's own, or replaces the one of that name.
#[tauri::command]
pub(crate) fn skill_save(
    core: CoreState<'_>,
    name: String,
    description: String,
    body: String,
) -> Command<Vec<Skill>> {
    core.skill_save(&name, &description, &body)?;
    Ok(core.skills())
}

/// Removes a skill of Swarmed's own.
#[tauri::command]
pub(crate) fn skill_remove(core: CoreState<'_>, name: String) -> Command<Vec<Skill>> {
    core.skill_remove(&name)?;
    Ok(core.skills())
}

/// Says whether a skill is offered to sessions whose agent does not have it by itself.
#[tauri::command]
pub(crate) fn skill_share(
    core: CoreState<'_>,
    origin: String,
    name: String,
    shared: bool,
) -> Command<Vec<Skill>> {
    core.skill_share(&origin, &name, shared)?;
    Ok(core.skills())
}

/// A project's environments, by name.
#[tauri::command]
pub(crate) fn environments(
    core: CoreState<'_>,
    project: String,
) -> Command<BTreeMap<String, Environment>> {
    Ok(core.environments(&project)?)
}

/// Adds an environment to a project, or replaces the one of that name.
#[tauri::command]
pub(crate) fn environment_save(
    core: CoreState<'_>,
    project: String,
    name: String,
    environment: Environment,
) -> Command<()> {
    Ok(core.environment_save(&project, &name, &environment)?)
}

/// Removes an environment from a project.
#[tauri::command]
pub(crate) fn environment_remove(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<()> {
    Ok(core.environment_remove(&project, &name)?)
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

/// Opens a link from a conversation in the user's browser.
#[tauri::command]
pub(crate) fn open_link(url: String) -> Command<()> {
    Ok(swarmed_core::open_link(&url)?)
}

/// What of a task's work is not committed, and what is not pushed.
#[tauri::command]
pub(crate) async fn task_unpublished(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<Unpublished> {
    Ok(core.task_unpublished(&project, &name).await?)
}

/// A commit message for a task's uncommitted work, for the user to read and change.
#[tauri::command]
pub(crate) async fn task_commit_message(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<String> {
    Ok(core.task_commit_message(&project, &name).await?)
}

/// Commits everything in a task's worktree under `message`.
#[tauri::command]
pub(crate) async fn task_commit(
    core: CoreState<'_>,
    project: String,
    name: String,
    message: String,
) -> Command<()> {
    Ok(core.task_commit(&project, &name, &message).await?)
}

/// Pushes a task's branch to its remote.
#[tauri::command]
pub(crate) async fn task_push(core: CoreState<'_>, project: String, name: String) -> Command<()> {
    Ok(core.task_push(&project, &name).await?)
}

/// A title and description to start a task's pull request from.
#[tauri::command]
pub(crate) async fn task_pull_draft(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<PullDraft> {
    Ok(core.task_pull_draft(&project, &name).await?)
}

/// Opens a pull request for a task's branch, as a draft if asked.
#[tauri::command]
pub(crate) async fn task_pull_create(
    core: CoreState<'_>,
    project: String,
    name: String,
    draft: PullDraft,
    as_draft: bool,
) -> Command<Pull> {
    Ok(core
        .task_pull_create(&project, &name, &draft, as_draft)
        .await?)
}

/// The pull request of a task's branch with its checks, if it has one.
#[tauri::command]
pub(crate) async fn task_pull(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<Option<Pull>> {
    Ok(core.task_pull(&project, &name).await?)
}

/// Opens a task's pull request in the browser, or the details of one of its checks.
#[tauri::command]
pub(crate) async fn task_pull_open(
    core: CoreState<'_>,
    project: String,
    name: String,
    check: Option<String>,
) -> Command<()> {
    Ok(core
        .task_pull_open(&project, &name, check.as_deref())
        .await?)
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

/// What a session runs with, as the UI chooses it.
#[derive(Debug, Deserialize)]
pub(crate) struct Running {
    model: Option<String>,
    supervision: Supervision,
    effort: Option<Effort>,
    /// Absent from a front end that knows nothing of it, which is the same as off.
    #[serde(default)]
    fast: bool,
}

#[tauri::command]
pub(crate) fn session_new(
    core: CoreState<'_>,
    project: String,
    task: String,
    agent: Option<String>,
    how: Running,
) -> Command<Session> {
    let options = SessionOptions {
        agent,
        title: None,
        model: how.model,
        supervision: how.supervision,
        effort: how.effort,
        fast: how.fast,
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
pub(crate) async fn session_send(
    core: CoreState<'_>,
    id: i64,
    text: String,
    files: Option<Vec<PathBuf>>,
) -> Command<()> {
    let message = with_files(&text, &files.unwrap_or_default())?;
    core.session_start(id, Auth::Subscription)?;
    Ok(core.session_send(id, &message)?)
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
    files: Option<Vec<PathBuf>>,
) -> Command<Vec<QueuedMessage>> {
    let message = with_files(&text, &files.unwrap_or_default())?;
    Ok(core.session_enqueue(id, &message)?)
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

/// The sessions in which `query` was said, closed ones included.
#[tauri::command]
pub(crate) fn sessions_search(core: CoreState<'_>, query: String) -> Command<Vec<SearchHit>> {
    Ok(core.sessions_search(&query)?)
}

/// A task's closed sessions, the one closed last first.
#[tauri::command]
pub(crate) fn sessions_closed(
    core: CoreState<'_>,
    project: String,
    task: String,
) -> Command<Vec<Session>> {
    Ok(core.sessions_closed(&project, &task)?)
}

/// A new session that starts with what was said in session `id`, up to its event `upto`.
#[tauri::command]
pub(crate) fn session_fork(core: CoreState<'_>, id: i64, upto: Option<usize>) -> Command<Session> {
    Ok(core.session_fork(id, upto)?)
}

/// Puts a closed session back in the lists.
#[tauri::command]
pub(crate) fn session_reopen(core: CoreState<'_>, id: i64) -> Command<Session> {
    Ok(core.session_reopen(id)?)
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

/// The project's snapshots: what tasks of each environment start from.
#[tauri::command]
pub(crate) fn snapshots(core: CoreState<'_>, project: String) -> Command<Vec<Snapshot>> {
    Ok(core.snapshots(&project)?)
}

/// Keeps what a task's setup produced as the snapshot of its environment.
#[tauri::command]
pub(crate) async fn snapshot_save(
    core: CoreState<'_>,
    project: String,
    task: String,
) -> Command<Snapshot> {
    Ok(core.snapshot_save(&project, &task).await?)
}

/// Forgets the snapshot of an environment, or of tasks without one.
#[tauri::command]
pub(crate) fn snapshot_remove(
    core: CoreState<'_>,
    project: String,
    environment: Option<String>,
) -> Command<()> {
    Ok(core.snapshot_remove(&project, environment.as_deref())?)
}

/// The sessions' turns added up by day, task, agent and model, for drawing.
#[tauri::command]
pub(crate) fn analytics(core: CoreState<'_>) -> Command<Vec<TurnStats>> {
    Ok(core.analytics()?)
}

/// What a task's agents left running in the background, and how what ended did.
#[tauri::command]
pub(crate) fn task_background(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<Vec<BackgroundTask>> {
    Ok(core.task_background(&project, &name)?)
}

/// Asks a session's agent to stop something it left running.
#[tauri::command]
pub(crate) fn session_stop_background(
    core: CoreState<'_>,
    id: i64,
    task_id: String,
) -> Command<()> {
    Ok(core.session_stop_background(id, &task_id)?)
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
pub(crate) fn session_configure(core: CoreState<'_>, id: i64, how: Running) -> Command<Session> {
    let settings = SessionSettings {
        model: how.model,
        supervision: how.supervision,
        effort: how.effort,
        fast: how.fast,
    };
    Ok(core.session_configure(id, &settings)?)
}

/// Shows on the app's icon how many sessions wait for the user; nothing for none.
#[tauri::command]
pub(crate) fn badge_set<R: Runtime>(app: AppHandle<R>, count: i64) {
    let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) else {
        return;
    };
    // Not every system has a badge to set, and nothing depends on it.
    window.set_badge_count((count > 0).then_some(count)).ok();
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
