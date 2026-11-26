//! The commands the UI can call. Each one forwards to `swarmed-core` and adds nothing, apart
//! from the three that update the app itself, which forward to `update.rs`.
#![allow(
    clippy::needless_pass_by_value,
    reason = "Tauri hands commands their arguments, `State` included, by value"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use swarmed_core::agent::{AgentEvent, Auth, Effort, Supervision};
use swarmed_core::{
    AccountUsage, Action, AgentInfo, AgentSetup, ApiKeyInfo, AppCsp, AppShown, Attached,
    BackgroundOutput, BackgroundTask, BranchChoices, BrowserGo, BrowserSetup, BrowserState,
    Cleaned, Cleanup, CleanupKind, CloudOrigin, CloudProvider, CloudTask, ConsoleLine, Core, Costs,
    CustomLayout, Environment, EnvironmentChoice, FileChange, FileDiff, FileListing, FirstSession,
    GitHubItem, GitHubItems, Issue, IssueDetail, IssueFilter, IssueList, KeyInput, McpServerInput,
    McpServerView, MergeMethod, MergeReport, MergeRules, MergeWhen, MouseInput, NewTaskOptions,
    PickedElement, Project, ProjectOverview, Pull, PullDraft, QueuedMessage, Reachable,
    RemoveOptions, RewindCheck, RewindDone, SearchHit, Session, SessionOptions, SessionSettings,
    Settings, SetupLog, Skill, SlashCommand, Snapshot, Task, TaskNaming, TaskStarted, Template,
    TerminalConfig, TerminalOpened, Turn, TurnChanges, TurnStats, Unpublished, Usage, UsageAsk,
    Viewport, hooks,
};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::update::{self, UpdateState, Updates};

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

#[tauri::command(async)]
pub(crate) fn agents(core: CoreState<'_>) -> Vec<AgentInfo> {
    core.agents()
}

#[tauri::command(async)]
pub(crate) fn overview(core: CoreState<'_>) -> Command<Vec<ProjectOverview>> {
    Ok(core.overview()?)
}

/// Every MCP server Swarmed knows of, with secrets by name only.
#[tauri::command(async)]
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

/// What can be cleaned up: removed worktrees not deleted yet, branches of removed tasks, stale
/// worktrees and snapshots. Async: it measures folders and asks git.
#[tauri::command]
pub(crate) async fn cleanup(core: CoreState<'_>) -> Command<Cleanup> {
    Ok(core.cleanup().await?)
}

/// Deletes what the user ticked of one kind, named by the keys it was listed with. `at_risk`
/// names those the window showed as losing work, so that one which became so since is kept.
#[tauri::command]
pub(crate) async fn cleanup_clean(
    core: CoreState<'_>,
    kind: CleanupKind,
    keys: Vec<String>,
    at_risk: Vec<String>,
) -> Command<Cleaned> {
    Ok(core
        .cleanup_clean(kind, Some(&keys), Some(&at_risk))
        .await?)
}

/// What the agents report their work has cost: today, this week, in all, by task and by day.
#[tauri::command(async)]
pub(crate) fn costs(core: CoreState<'_>) -> Command<Costs> {
    Ok(core.costs()?)
}

/// The agents that can be set up from the settings, and how far along each is.
#[tauri::command(async)]
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
) -> Command<Vec<String>> {
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
    environment: Option<EnvironmentChoice>,
) -> Command<Task> {
    // The window makes the name up, from the prompt or a branch: one that is taken, by a task
    // or a branch here or on a remote, gives way to the first free one, which the task returned
    // has. No environment named is the project's default.
    let options = NewTaskOptions {
        existing_branch: branch.as_deref(),
        environment: environment.unwrap_or_default(),
        free_name: true,
        issue: None,
    };
    let sink = hook_sink(&app, &project, &name);
    Ok(core.task_new(&project, &name, options, &sink).await?)
}

/// How a task is started from a prompt, as the new-task view asks for it.
#[derive(Debug, Deserialize)]
pub(crate) struct Start {
    /// An existing branch to check out instead of making one.
    branch: Option<String>,
    environment: Option<EnvironmentChoice>,
    /// The first session's agent; `None` is Claude Code.
    agent: Option<String>,
    how: Running,
    /// The first session's title, if a model named the task.
    title: Option<String>,
    prompt: String,
    #[serde(default)]
    files: Vec<PathBuf>,
    /// Images pasted into the prompt, each in base64.
    #[serde(default)]
    images: Vec<String>,
    /// The sessions the prompt mentions, by id.
    #[serde(default)]
    sessions: Vec<i64>,
    /// The pull requests and issues the prompt mentions, by number.
    #[serde(default)]
    github: Vec<u64>,
    /// The number of the repository's GitHub issue the task is started for, if any.
    #[serde(default)]
    issue: Option<u64>,
}

/// Starts a task from a prompt: the task and its first session are made and returned while
/// the task is set up, with the prompt waiting for the setup, which sends it once it finishes
/// well. The setup goes on by itself, told as `setup` events.
#[tauri::command]
pub(crate) async fn task_start<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
    project: String,
    name: String,
    start: Start,
) -> Command<TaskStarted> {
    // A name that is taken gives way to the first free one, as with `task_new`.
    let options = NewTaskOptions {
        existing_branch: start.branch.as_deref(),
        environment: start.environment.unwrap_or_default(),
        free_name: true,
        issue: start.issue,
    };
    let first = FirstSession {
        options: SessionOptions {
            agent: start.agent,
            title: start.title,
            model: start.how.model,
            supervision: start.how.supervision,
            effort: start.how.effort,
            fast: start.how.fast,
        },
        prompt: start.prompt,
        files: start.files,
        sessions: start.sessions,
        github: start.github,
        images: start.images,
    };
    let sink = hook_sink(&app, &project, &name);
    let starting = core
        .inner()
        .task_start(&project, &name, options, first, Auth::default(), &sink);
    // The setup goes on without anybody waiting for it.
    let (started, _going) = starting.await?;
    Ok(started)
}

/// Runs a task's `setup` again; once it finishes well, what its sessions wait to send goes.
/// How it goes is told as `setup` events.
#[tauri::command]
pub(crate) async fn task_setup(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<Task> {
    let setting = core.task_setup(&project, &name, Some(Auth::default()), &hooks::discard);
    Ok(setting.await?)
}

/// Stops a task's setup, which leaves the task to be set up again.
#[tauri::command]
pub(crate) async fn task_setup_stop(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<()> {
    Ok(core.task_setup_stop(&project, &name).await?)
}

/// Sends what a task's sessions wait to send until it is set up now, in the worktree as it is.
/// Async so that it runs on the runtime the core spawns the agents' tasks on.
#[tauri::command]
pub(crate) async fn task_start_anyway(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<Vec<i64>> {
    Ok(core.task_start_anyway(&project, &name, &Auth::default())?)
}

/// What a task's last setup printed, and how it ended.
#[tauri::command(async)]
pub(crate) fn task_setup_log(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<SetupLog> {
    Ok(core.task_setup_log(&project, &name)?)
}

#[tauri::command]
pub(crate) async fn task_remove<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
    project: String,
    name: String,
    force: bool,
    delete_branch: bool,
) -> Command<Vec<String>> {
    let options = RemoveOptions {
        force,
        delete_branch,
    };
    let sink = hook_sink(&app, &project, &name);
    Ok(core.task_remove(&project, &name, options, &sink).await?)
}

/// Puts a task away: stops its agents and its stack, closes its sessions if the settings say
/// to, and lists it with the done ones. Returns what went wrong on the way, to be said.
#[tauri::command]
pub(crate) async fn task_done<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<Vec<String>> {
    let sink = hook_sink(&app, &project, &name);
    Ok(core.task_done(&project, &name, &sink).await?)
}

/// Brings a put away task back to the others.
#[tauri::command]
pub(crate) fn task_undone(core: CoreState<'_>, project: String, name: String) -> Command<()> {
    Ok(core.task_undone(&project, &name)?)
}

/// What one folder of a task's worktree holds; the worktree itself for an empty `dir`.
#[tauri::command(async)]
pub(crate) fn task_files(
    core: CoreState<'_>,
    project: String,
    name: String,
    dir: String,
) -> Command<FileListing> {
    Ok(core.task_files(&project, &name, &dir)?)
}

/// Opens a file of a task's worktree in the user's editor.
#[tauri::command]
pub(crate) fn task_open_file(
    core: CoreState<'_>,
    project: String,
    name: String,
    path: String,
) -> Command<()> {
    Ok(core.task_open_file(&project, &name, &path)?)
}

#[tauri::command]
pub(crate) fn task_open_terminal(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<()> {
    Ok(core.task_open_terminal(&project, &name)?)
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

/// Hands a session's conversation to a cloud agent, under what it is to do with it.
#[tauri::command]
pub(crate) async fn cloud_delegate_conversation(
    core: CoreState<'_>,
    id: i64,
    instruction: String,
    provider: CloudProvider,
) -> Command<CloudTask> {
    let handed = core.cloud_delegate_conversation(id, &instruction, provider);
    Ok(handed.await?)
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

/// Moves a project `by` places among the others in the sidebar, earlier for a negative number.
/// In order with the window's other commands, as each move is from the list the last one left.
#[tauri::command]
pub(crate) fn project_move(core: CoreState<'_>, project: String, by: i32) -> Command<()> {
    Ok(core.project_move(&project, by)?)
}

/// Moves a task `by` places among its project's other open tasks.
#[tauri::command]
pub(crate) fn task_move(
    core: CoreState<'_>,
    project: String,
    task: String,
    by: i32,
) -> Command<()> {
    Ok(core.task_move(&project, &task, by)?)
}

/// Moves a session `by` places among its task's other open sessions, or among those its
/// starter started.
#[tauri::command]
pub(crate) fn session_move(core: CoreState<'_>, id: i64, by: i32) -> Command<()> {
    Ok(core.session_move(id, by)?)
}

/// Pins a project above the others that are not pinned, or with `pinned` false unpins it.
/// In order with the window's moves, as each is from the list the last one left.
#[tauri::command]
pub(crate) fn project_pin(core: CoreState<'_>, project: String, pinned: bool) -> Command<()> {
    Ok(core.project_pin(&project, pinned)?)
}

/// Pins a task above its project's other open tasks that are not pinned, or unpins it.
#[tauri::command]
pub(crate) fn task_pin(
    core: CoreState<'_>,
    project: String,
    task: String,
    pinned: bool,
) -> Command<()> {
    Ok(core.task_pin(&project, &task, pinned)?)
}

/// Pins a session above its siblings that are not pinned, or unpins it.
#[tauri::command]
pub(crate) fn session_pin(core: CoreState<'_>, id: i64, pinned: bool) -> Command<()> {
    Ok(core.session_pin(id, pinned)?)
}

/// Highlights a task in the colour named, or with `null` or `none` takes its highlight off.
#[tauri::command]
pub(crate) fn task_highlight(
    core: CoreState<'_>,
    project: String,
    task: String,
    colour: Option<String>,
) -> Command<()> {
    core.task_highlight_named(&project, &task, colour.as_deref())?;
    Ok(())
}

/// Highlights a session in the colour named, or with `null` or `none` takes its highlight off.
#[tauri::command]
pub(crate) fn session_highlight(
    core: CoreState<'_>,
    id: i64,
    colour: Option<String>,
) -> Command<()> {
    core.session_highlight_named(id, colour.as_deref())?;
    Ok(())
}

/// Moves a cloud task `by` places among the others of its project.
#[tauri::command]
pub(crate) fn cloud_task_move(core: CoreState<'_>, id: i64, by: i32) -> Command<()> {
    Ok(core.cloud_task_move(id, by)?)
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
#[tauri::command(async)]
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
#[tauri::command(async)]
pub(crate) fn stack_log(core: CoreState<'_>, project: String, task: String) -> Command<String> {
    Ok(core.stack_log(&project, &task)?)
}

/// Every skill Swarmed knows of: its own, and each agent's.
#[tauri::command(async)]
pub(crate) fn skills(core: CoreState<'_>) -> Vec<Skill> {
    core.skills()
}

/// What a skill of Swarmed's own says to do, for editing.
#[tauri::command(async)]
pub(crate) fn skill_body(core: CoreState<'_>, name: String) -> Command<String> {
    Ok(core.skill_body(&name)?)
}

/// The prompts the user keeps to send again.
#[tauri::command(async)]
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

/// The buttons of the user's own that `project` offers, or those for every repository.
#[tauri::command(async)]
pub(crate) fn actions(core: CoreState<'_>, project: Option<String>) -> Command<Vec<Action>> {
    Ok(core.actions(project.as_deref())?)
}

/// Keeps an action, in place of the one of that name: `project`'s own, or one for every
/// repository.
#[tauri::command]
pub(crate) fn action_save(
    core: CoreState<'_>,
    project: Option<String>,
    action: Action,
) -> Command<Vec<Action>> {
    Ok(core.action_save(project.as_deref(), &action)?)
}

/// Drops an action.
#[tauri::command]
pub(crate) fn action_remove(
    core: CoreState<'_>,
    project: Option<String>,
    name: String,
) -> Command<Vec<Action>> {
    Ok(core.action_remove(project.as_deref(), &name)?)
}

/// Moves an action `by` places among those kept in the same place.
#[tauri::command]
pub(crate) fn action_move(
    core: CoreState<'_>,
    project: Option<String>,
    name: String,
    by: i32,
) -> Command<Vec<Action>> {
    Ok(core.action_move(project.as_deref(), &name, by)?)
}

/// Puts a mark on a session, or takes it off when `colour` is `none`.
#[tauri::command]
pub(crate) fn session_mark(
    core: CoreState<'_>,
    id: i64,
    colour: String,
    label: String,
) -> Command<Session> {
    Ok(core.session_mark_named(id, &colour, &label)?)
}

/// Adds a skill of Swarmed's own, or replaces the one of that name.
#[tauri::command]
pub(crate) fn skill_save(
    core: CoreState<'_>,
    name: String,
    description: String,
    body: String,
) -> Command<Vec<Skill>> {
    Ok(core.skills_after_save(&name, &description, &body)?)
}

/// Removes a skill of Swarmed's own.
#[tauri::command]
pub(crate) fn skill_remove(core: CoreState<'_>, name: String) -> Command<Vec<Skill>> {
    Ok(core.skills_after_remove(&name)?)
}

/// Says whether a skill is offered to sessions whose agent does not have it by itself.
#[tauri::command]
pub(crate) fn skill_share(
    core: CoreState<'_>,
    origin: String,
    name: String,
    shared: bool,
) -> Command<Vec<Skill>> {
    Ok(core.skills_after_share(&origin, &name, shared)?)
}

/// A project's environments, by name.
#[tauri::command(async)]
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

/// The environment a new task of a project is set up in when none is chosen, if any.
#[tauri::command(async)]
pub(crate) fn environment_default(core: CoreState<'_>, project: String) -> Command<Option<String>> {
    Ok(core.environment_default(&project)?)
}

/// Makes an environment the one new tasks of a project are set up in when none is chosen;
/// `null` leaves them to the project's own setup.
#[tauri::command]
pub(crate) fn environment_set_default(
    core: CoreState<'_>,
    project: String,
    name: Option<String>,
) -> Command<()> {
    Ok(core.environment_set_default(&project, name.as_deref())?)
}

#[tauri::command(async)]
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

/// The effort an agent last ran a model with when the effort was left to it.
#[tauri::command(async)]
pub(crate) fn effort_default(
    core: CoreState<'_>,
    agent: String,
    model: Option<String>,
) -> Command<Option<String>> {
    Ok(core.effort_default(&agent, model.as_deref())?)
}

/// A commit message for a task's uncommitted work, for the user to read and change.
#[tauri::command]
pub(crate) async fn task_commit_message(
    core: CoreState<'_>,
    project: String,
    name: String,
    session: Option<i64>,
) -> Command<String> {
    Ok(core.task_commit_message(&project, &name, session).await?)
}

/// A title and description for a task's pull request, written from its branch, for the user
/// to read and change.
#[tauri::command]
pub(crate) async fn task_pull_message(
    core: CoreState<'_>,
    project: String,
    name: String,
    session: Option<i64>,
) -> Command<PullDraft> {
    Ok(core.task_pull_message(&project, &name, session).await?)
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

/// Puts a task's worktree on another of the branches it has been on.
#[tauri::command]
pub(crate) async fn task_switch(
    core: CoreState<'_>,
    project: String,
    name: String,
    branch: String,
) -> Command<()> {
    Ok(core.task_switch(&project, &name, &branch).await?)
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

/// A message about a task's failed checks, with the tail of their logs, for the user to read
/// and send to an agent.
#[tauri::command]
pub(crate) async fn task_pull_failure(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<String> {
    Ok(core.task_pull_failure(&project, &name).await?)
}

/// The issues of a repository that `filter` picks, or why none could be listed.
#[tauri::command]
pub(crate) async fn project_issues(
    core: CoreState<'_>,
    project: String,
    filter: IssueFilter,
) -> Command<IssueList> {
    Ok(core.project_issues(&project, &filter).await?)
}

/// One issue of a repository read whole, with its comments.
#[tauri::command]
pub(crate) async fn project_issue(
    core: CoreState<'_>,
    project: String,
    number: u64,
) -> Command<IssueDetail> {
    Ok(core.project_issue(&project, number).await?)
}

/// The issue a task was started for, read now; nothing for a task that is for none.
#[tauri::command]
pub(crate) async fn task_issue(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<Option<Issue>> {
    Ok(core.task_issue(&project, &name).await?)
}

/// What a repository allows for merging its pull requests, and the method a merge starts on.
#[tauri::command]
pub(crate) async fn project_merge_rules(
    core: CoreState<'_>,
    project: String,
) -> Command<MergeRules> {
    Ok(core.project_merge_rules(&project).await?)
}

/// Merges a task's pull request at the head the user saw, now or once its requirements are
/// met.
#[tauri::command]
pub(crate) async fn task_pull_merge(
    core: CoreState<'_>,
    project: String,
    name: String,
    method: MergeMethod,
    when: MergeWhen,
    head: String,
) -> Command<MergeReport> {
    Ok(core
        .task_pull_merge(&project, &name, method, when, &head)
        .await?)
}

/// Turns off the auto-merge of a task's pull request.
#[tauri::command]
pub(crate) async fn task_pull_auto_merge_off(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<MergeReport> {
    Ok(core.task_pull_auto_merge_off(&project, &name).await?)
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

/// The turns of a session whose checkpoints are kept, oldest first. Off the main thread: it
/// reads the task's checkpoints.
#[tauri::command(async)]
pub(crate) fn session_turns(core: CoreState<'_>, id: i64) -> Command<Vec<Turn>> {
    Ok(core.session_turns(id)?)
}

/// What one turn of a session changed in its worktree.
#[tauri::command]
pub(crate) async fn session_turn_changes(
    core: CoreState<'_>,
    id: i64,
    message: usize,
) -> Command<TurnChanges> {
    Ok(core.session_turn_changes(id, message).await?)
}

/// What taking a session back to before its message `message` would do now.
#[tauri::command]
pub(crate) async fn session_rewind_check(
    core: CoreState<'_>,
    id: i64,
    message: usize,
) -> Command<RewindCheck> {
    Ok(core.session_rewind_check(id, message).await?)
}

/// Takes a session back to before its message `message`, its files too with `files`.
#[tauri::command]
pub(crate) async fn session_rewind(
    core: CoreState<'_>,
    id: i64,
    message: usize,
    files: bool,
) -> Command<RewindDone> {
    Ok(core.session_rewind(id, message, files).await?)
}

/// Puts back the files a session's latest rewind took back.
#[tauri::command]
pub(crate) async fn session_files_put_back(core: CoreState<'_>, id: i64) -> Command<()> {
    Ok(core.session_files_put_back(id).await?)
}

/// How one file changed in one turn of a session.
#[tauri::command]
pub(crate) async fn session_turn_diff(
    core: CoreState<'_>,
    id: i64,
    message: usize,
    path: String,
) -> Command<FileDiff> {
    Ok(core.session_turn_diff(id, message, &path).await?)
}

#[tauri::command(async)]
pub(crate) fn settings(core: CoreState<'_>) -> Command<Settings> {
    Ok(core.settings()?)
}

/// What of Swarmed's configuration could not be understood, for the window to show.
#[tauri::command(async)]
pub(crate) fn config_problems(core: CoreState<'_>) -> Command<Vec<String>> {
    Ok(core.config_problems()?)
}

#[tauri::command]
pub(crate) fn set_settings(core: CoreState<'_>, settings: Settings) -> Command<()> {
    Ok(core.set_settings(&settings)?)
}

/// The branch a task called fix-login would get with `prefix` as the branch prefix, or git's
/// reason against it. It asks git, so off the main thread.
#[tauri::command(async)]
pub(crate) fn branch_example(prefix: String) -> Command<String> {
    Ok(swarmed_core::branch_example(&prefix)?)
}

/// Where updating Swarmed stands.
#[tauri::command]
pub(crate) fn update_state(updates: State<'_, Updates>) -> UpdateState {
    updates.state()
}

/// Looks for a new version of Swarmed now.
#[tauri::command]
pub(crate) async fn update_check<R: Runtime>(app: AppHandle<R>) -> UpdateState {
    update::look_now(&app).await
}

/// Downloads the new version if need be, and restarts Swarmed to install it.
#[tauri::command]
pub(crate) async fn update_restart<R: Runtime>(app: AppHandle<R>) -> Command<()> {
    update::restart(&app).await.map_err(CommandError)
}

/// The user's custom layout, as checked by the core.
#[tauri::command(async)]
pub(crate) fn custom_layout(core: CoreState<'_>) -> Command<CustomLayout> {
    Ok(core.custom_layout()?)
}

/// Keeps `text`, JSON as the user wrote it, as the custom layout if it can be drawn.
#[tauri::command]
pub(crate) fn custom_layout_save(core: CoreState<'_>, text: String) -> Command<CustomLayout> {
    Ok(core.set_custom_layout(&text)?)
}

/// The user's configuration of the terminal, as checked by the core.
#[tauri::command(async)]
pub(crate) fn terminal_config(core: CoreState<'_>) -> Command<TerminalConfig> {
    Ok(core.terminal_config()?)
}

/// Keeps `text`, JSON as the user wrote it, as the terminal's configuration if it can be used.
#[tauri::command]
pub(crate) fn terminal_config_save(core: CoreState<'_>, text: String) -> Command<TerminalConfig> {
    Ok(core.set_terminal_config(&text)?)
}

#[tauri::command(async)]
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

#[tauri::command(async)]
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
    sessions: Option<Vec<i64>>,
    github: Option<Vec<u64>>,
    elements: Option<Vec<u64>>,
) -> Command<()> {
    let attached = attached(files, sessions, github, elements);
    Ok(core.session_message(id, &text, &attached, Auth::default())?)
}

/// What a message points the agent at besides its words, as the window sends it: the files
/// attached, the sessions mentioned, the pull requests and issues, and the elements picked in
/// the task's browser, any left out for none.
fn attached(
    files: Option<Vec<PathBuf>>,
    sessions: Option<Vec<i64>>,
    github: Option<Vec<u64>>,
    elements: Option<Vec<u64>>,
) -> Attached {
    Attached {
        files: files.unwrap_or_default(),
        sessions: sessions.unwrap_or_default(),
        github: github.unwrap_or_default(),
        elements: elements.unwrap_or_default(),
    }
}

/// What the `#` menu offers for `query` in a repository: its latest pull requests and issues,
/// one by number, or those words find, through the user's `gh`; or why there are none.
#[tauri::command]
pub(crate) async fn project_github_items(
    core: CoreState<'_>,
    project: String,
    query: String,
) -> Command<GitHubItems> {
    Ok(core.project_github_items(&project, &query).await?)
}

/// The pull requests and issues `numbers` of a repository, read now unless they were a moment
/// ago: what a message mentioning them is sent with.
#[tauri::command]
pub(crate) async fn project_github_read(
    core: CoreState<'_>,
    project: String,
    numbers: Vec<u64>,
) -> Command<Vec<GitHubItem>> {
    Ok(core.project_github_read(&project, &numbers).await?)
}

/// The sessions a session can mention with `@`: those its agent can read, its own task's
/// first. Off the main thread: it looks at every open session.
#[tauri::command(async)]
pub(crate) fn session_reachable(core: CoreState<'_>, id: i64) -> Command<Vec<Reachable>> {
    Ok(core.session_reachable(id)?)
}

/// The sessions a new task's first message in `project` can mention, before it has a session.
#[tauri::command(async)]
pub(crate) fn project_reachable(core: CoreState<'_>, project: String) -> Command<Vec<Reachable>> {
    Ok(core.project_reachable(&project)?)
}

/// Keeps images pasted into a message to a session, each in base64, as files of the session's
/// own, and returns their paths to attach to the message. Off the main thread: an image can be
/// megabytes.
#[tauri::command(async)]
pub(crate) fn session_images_keep(
    core: CoreState<'_>,
    id: i64,
    images: Vec<String>,
) -> Command<Vec<PathBuf>> {
    Ok(core.session_images_keep(id, &images)?)
}

/// An image pasted into a session, as a `data:` address to show it from.
#[tauri::command(async)]
pub(crate) fn session_image(core: CoreState<'_>, id: i64, path: PathBuf) -> Command<String> {
    Ok(core.session_image(id, &path)?)
}

/// Opens an image pasted into a session in the system's viewer.
#[tauri::command]
pub(crate) fn session_image_open(core: CoreState<'_>, id: i64, path: PathBuf) -> Command<()> {
    Ok(core.session_image_open(id, &path)?)
}

/// The folder that holds the pasted images of every session, one folder each.
#[tauri::command]
pub(crate) fn attachments_root(core: CoreState<'_>) -> PathBuf {
    core.attachments_root()
}

/// Tells a waiting message to the session's agent in the middle of its turn, without stopping it.
#[tauri::command]
pub(crate) fn session_tell_now(core: CoreState<'_>, id: i64, message: u64) -> Command<()> {
    Ok(core.session_tell_now(id, message)?)
}

/// Sends the message now, stopping the turn under way first: it goes when that turn has ended,
/// ahead of anything waiting. Async for the same reason as `session_send`.
#[tauri::command]
pub(crate) async fn session_message_now(
    core: CoreState<'_>,
    id: i64,
    text: String,
    files: Option<Vec<PathBuf>>,
    sessions: Option<Vec<i64>>,
    github: Option<Vec<u64>>,
    elements: Option<Vec<u64>>,
) -> Command<()> {
    let attached = attached(files, sessions, github, elements);
    Ok(core.session_message_now(id, &text, &attached, Auth::default())?)
}

#[tauri::command(async)]
pub(crate) fn session_queue(core: CoreState<'_>, id: i64) -> Command<Vec<QueuedMessage>> {
    Ok(core.session_queue(id)?)
}

#[tauri::command]
pub(crate) fn session_enqueue(
    core: CoreState<'_>,
    id: i64,
    text: String,
    files: Option<Vec<PathBuf>>,
    sessions: Option<Vec<i64>>,
    github: Option<Vec<u64>>,
    elements: Option<Vec<u64>>,
) -> Command<Vec<QueuedMessage>> {
    let attached = attached(files, sessions, github, elements);
    Ok(core.session_enqueue_message(id, &text, &attached)?)
}

#[tauri::command]
#[allow(
    clippy::too_many_arguments,
    reason = "the window names each list a message points at, as every command that sends one"
)]
pub(crate) fn session_queue_edit(
    core: CoreState<'_>,
    id: i64,
    message: u64,
    text: String,
    files: Option<Vec<PathBuf>>,
    sessions: Option<Vec<i64>>,
    github: Option<Vec<u64>>,
    elements: Option<Vec<u64>>,
) -> Command<Vec<QueuedMessage>> {
    let attached = attached(files, sessions, github, elements);
    Ok(core.session_queue_edit_message(id, message, &text, &attached)?)
}

/// Holds a waiting message while the user edits it, or renews the hold.
#[tauri::command]
pub(crate) fn session_queue_hold(core: CoreState<'_>, id: i64, message: u64) -> Command<bool> {
    Ok(core.session_queue_hold(id, message)?)
}

/// Lets go of a waiting message the user left as it was.
#[tauri::command]
pub(crate) fn session_queue_let_go(core: CoreState<'_>, id: i64, message: u64) -> Command<()> {
    Ok(core.session_queue_let_go(id, message)?)
}

/// Async for the same reason as `session_send`, which it is a form of.
#[tauri::command]
pub(crate) async fn session_send_queued(core: CoreState<'_>, id: i64) -> Command<()> {
    Ok(core.session_resume_queue(id, Auth::default())?)
}

/// Async for the same reason as `session_send`, which it is a form of.
#[tauri::command]
pub(crate) async fn session_send_now(core: CoreState<'_>, id: i64, message: u64) -> Command<()> {
    Ok(core.session_send_now(id, message, Auth::default())?)
}

/// The sessions in which `query` was said, closed ones included.
#[tauri::command(async)]
pub(crate) fn sessions_search(core: CoreState<'_>, query: String) -> Command<Vec<SearchHit>> {
    Ok(core.sessions_search(&query)?)
}

/// A task's closed sessions, the one closed last first.
#[tauri::command(async)]
pub(crate) fn sessions_closed(
    core: CoreState<'_>,
    project: String,
    task: String,
) -> Command<Vec<Session>> {
    Ok(core.sessions_closed(&project, &task)?)
}

/// A new session that starts with what was said in session `id` from its message number
/// `from` on, or with all of it; on `agent`, if that is another than the session's own.
#[tauri::command(async)]
pub(crate) fn session_fork(
    core: CoreState<'_>,
    id: i64,
    from: Option<usize>,
    agent: Option<String>,
) -> Command<Session> {
    Ok(core.session_fork(id, from, agent.as_deref())?)
}

/// A new session in task `task` of `project` that starts with all that was said in session
/// `id`: for a chat whose work belongs in a repository.
#[tauri::command(async)]
pub(crate) fn session_fork_into(
    core: CoreState<'_>,
    id: i64,
    project: String,
    task: String,
) -> Command<Session> {
    Ok(core.session_fork_into(id, &project, &task)?)
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

/// The user's answer to a change of Swarmed's configuration that an agent proposed.
#[tauri::command]
pub(crate) fn config_answer(
    core: CoreState<'_>,
    request_id: String,
    accepted: bool,
) -> Command<()> {
    Ok(core.config_answer(&request_id, accepted)?)
}

#[tauri::command]
pub(crate) fn session_allow(core: CoreState<'_>, id: i64, request_id: String) -> Command<()> {
    Ok(core.session_allow(id, &request_id)?)
}

#[tauri::command]
pub(crate) fn session_answer(
    core: CoreState<'_>,
    id: i64,
    request_id: String,
    answers: BTreeMap<String, String>,
) -> Command<()> {
    Ok(core.session_answer(id, &request_id, answers)?)
}

/// Async because the answer may start the session's agent, as `session_send` does, and reads
/// the conversation to find the question.
#[tauri::command]
pub(crate) async fn session_command_answer(
    core: CoreState<'_>,
    id: i64,
    request_id: String,
    answers: Option<BTreeMap<String, String>>,
) -> Command<()> {
    Ok(core.session_command_answer(id, &request_id, answers, Auth::default())?)
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
#[tauri::command(async)]
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
#[tauri::command(async)]
pub(crate) fn analytics(core: CoreState<'_>) -> Command<Vec<TurnStats>> {
    Ok(core.analytics()?)
}

/// What a task's agents left running in the background, and how what ended did.
#[tauri::command(async)]
pub(crate) fn task_background(
    core: CoreState<'_>,
    project: String,
    name: String,
) -> Command<Vec<BackgroundTask>> {
    Ok(core.task_background(&project, &name)?)
}

/// What a command a session's agent left running printed, from `from` on or its latest part.
#[tauri::command(async)]
pub(crate) fn session_background_output(
    core: CoreState<'_>,
    id: i64,
    task_id: String,
    from: Option<u64>,
) -> Command<Option<BackgroundOutput>> {
    Ok(core.session_background_output(id, &task_id, from)?)
}

/// Asks a session's agent to send the work a call waits on, or every call's, to the
/// background, and says whether it went.
#[tauri::command]
pub(crate) async fn session_send_to_background(
    core: CoreState<'_>,
    id: i64,
    tool_use_id: Option<String>,
) -> Command<bool> {
    Ok(core
        .session_send_to_background(id, tool_use_id.as_deref())
        .await?)
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

#[tauri::command(async)]
pub(crate) fn slash_commands(core: CoreState<'_>, id: Option<i64>) -> Command<Vec<SlashCommand>> {
    Ok(core.slash_commands(id)?)
}

/// Puts a session that has not begun on another agent.
#[tauri::command]
pub(crate) fn session_set_agent(core: CoreState<'_>, id: i64, agent: String) -> Command<Session> {
    Ok(core.session_set_agent(id, &agent)?)
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

/// Makes the app an MCP server attached to call `tool_use_id` of session `id`, kept in
/// `document`, ready for a frame, under the policy its server's declaration comes to.
#[tauri::command]
pub(crate) fn app_put(
    core: CoreState<'_>,
    id: i64,
    tool_use_id: String,
    document: PathBuf,
    csp: AppCsp,
) -> Command<AppShown> {
    Ok(core.app_put(id, &tool_use_id, &document, &csp)?)
}

/// Calls a tool of its own server for the app shown under call `tool_use_id` of session `id`,
/// through the session's agent, and returns the tool's result. Waits on the agent, which may
/// ask the user first, so it runs off the main thread.
#[tauri::command]
pub(crate) async fn session_app_call(
    core: CoreState<'_>,
    id: i64,
    tool_use_id: String,
    tool: String,
    arguments: Value,
) -> Command<Value> {
    Ok(core
        .session_app_call(id, &tool_use_id, &tool, arguments)
        .await?)
}

/// Async so that it runs on the runtime the core spawns the terminal's tasks on.
#[tauri::command]
pub(crate) async fn terminal_open(
    core: CoreState<'_>,
    id: i64,
    cols: u16,
    rows: u16,
) -> Command<TerminalOpened> {
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

/// Opens Claude Code on `command`'s screen in a terminal of the session's own, or shows the one
/// that runs on it. Async for the same reason as [`terminal_open`].
#[tauri::command]
pub(crate) async fn claude_terminal_open(
    core: CoreState<'_>,
    id: i64,
    command: String,
    cols: u16,
    rows: u16,
) -> Command<TerminalOpened> {
    Ok(core.claude_terminal_open(id, &command, (cols, rows))?)
}

#[tauri::command]
pub(crate) fn claude_terminal_write(core: CoreState<'_>, id: i64, data: String) -> Command<()> {
    Ok(core.claude_terminal_write(id, &data)?)
}

#[tauri::command]
pub(crate) fn claude_terminal_resize(
    core: CoreState<'_>,
    id: i64,
    cols: u16,
    rows: u16,
) -> Command<()> {
    Ok(core.claude_terminal_resize(id, cols, rows)?)
}

#[tauri::command]
pub(crate) fn claude_terminal_close(core: CoreState<'_>, id: i64) {
    core.claude_terminal_close(id);
}

#[tauri::command]
pub(crate) async fn status_line(core: CoreState<'_>, id: i64) -> Command<Option<String>> {
    Ok(core.status_line(id).await?)
}

/// The account's usage as any Claude Code session last heard it. The sessions' records are
/// read for it the first time.
#[tauri::command(async)]
pub(crate) fn account_usage(core: CoreState<'_>) -> Command<AccountUsage> {
    Ok(core.account_usage()?)
}

/// Asks the session's Claude Code for the account's usage now, and says when the question
/// goes: it waits while Claude Code would answer from its own copy. The answer arrives as
/// events.
#[tauri::command]
pub(crate) async fn session_usage_refresh(core: CoreState<'_>, id: i64) -> Command<UsageAsk> {
    Ok(core.session_usage_refresh(id, Auth::default())?)
}

/// Turns Remote Control of a Claude Code session on or off; how it goes arrives as events.
#[tauri::command(async)]
pub(crate) fn session_remote_control(core: CoreState<'_>, id: i64, on: bool) -> Command<()> {
    Ok(core.session_remote_control(id, on, Auth::default())?)
}

/// Lets a session a usage limit stopped go on by itself when the limit resets, and says when,
/// in seconds since 1970.
#[tauri::command]
pub(crate) fn session_continue_at_reset(core: CoreState<'_>, id: i64) -> Command<i64> {
    Ok(core.session_continue_at_reset(id)?)
}

/// Cancels a session's going on by itself when a usage limit resets.
#[tauri::command]
pub(crate) fn session_continue_cancel(core: CoreState<'_>, id: i64) -> Command<()> {
    Ok(core.session_continue_cancel(id)?)
}

#[tauri::command]
pub(crate) fn session_rename(core: CoreState<'_>, id: i64, title: String) -> Command<Session> {
    Ok(core.session_rename(id, &title)?)
}

#[tauri::command]
pub(crate) fn session_close(core: CoreState<'_>, id: i64) -> Command<()> {
    Ok(core.session_close(id)?)
}

/// Deletes a session and its conversation for good.
#[tauri::command]
pub(crate) fn session_delete(core: CoreState<'_>, id: i64) -> Command<()> {
    Ok(core.session_delete(id)?)
}

/// The browser a task's Chrome runs on, or what would install one.
#[tauri::command(async)]
pub(crate) fn browser_setup(core: CoreState<'_>) -> Command<BrowserSetup> {
    Ok(core.browser_setup()?)
}

/// Installs a browser for the tasks. What the installer prints reaches the UI the way hook
/// output does.
#[tauri::command]
pub(crate) async fn browser_install<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
) -> Command<BrowserSetup> {
    let sink = hook_sink(&app, "", "browser");
    Ok(core.browser_install(&sink).await?)
}

/// A task's browser as the pane shows it, without starting it.
#[tauri::command(async)]
pub(crate) fn browser_state(core: CoreState<'_>, task_id: i64) -> Command<BrowserState> {
    Ok(core.browser_state(task_id)?)
}

/// Starts a task's browser, or connects to the one that runs.
#[tauri::command]
pub(crate) async fn browser_open(core: CoreState<'_>, task_id: i64) -> Command<BrowserState> {
    Ok(core.browser_open(task_id).await?)
}

/// Stops a task's browser. Its profile stays.
#[tauri::command]
pub(crate) async fn browser_stop(core: CoreState<'_>, task_id: i64) -> Command<()> {
    Ok(core.browser_stop(task_id).await?)
}

/// Shows a tab in the pane, at the room the pane gives it: its pictures come from now on.
#[tauri::command]
pub(crate) async fn browser_show(
    core: CoreState<'_>,
    task_id: i64,
    tab: Option<String>,
    viewport: Viewport,
) -> Command<BrowserState> {
    Ok(core.browser_show(task_id, tab.as_deref(), viewport).await?)
}

/// The pane is off screen: no more pictures.
#[tauri::command]
pub(crate) async fn browser_hide(core: CoreState<'_>, task_id: i64) -> Command<()> {
    Ok(core.browser_hide(task_id).await?)
}

/// Brings the window of a task's Chrome forward, on a tab, for one with a window of its own.
#[tauri::command]
pub(crate) async fn browser_raise(
    core: CoreState<'_>,
    task_id: i64,
    tab: Option<String>,
) -> Command<()> {
    Ok(core.browser_raise(task_id, tab.as_deref()).await?)
}

/// What the mouse did on the picture. Not `async`: clicks and keys go in the order they came.
#[tauri::command]
pub(crate) fn browser_mouse(core: CoreState<'_>, task_id: i64, input: MouseInput) -> Command<()> {
    Ok(core.browser_mouse(task_id, &input)?)
}

/// A key on the picture, in order with the clicks.
#[tauri::command]
pub(crate) fn browser_key(core: CoreState<'_>, task_id: i64, input: KeyInput) -> Command<()> {
    Ok(core.browser_key(task_id, &input)?)
}

/// Text pasted or composed on the picture, in order with the keys.
#[tauri::command]
pub(crate) fn browser_text(core: CoreState<'_>, task_id: i64, text: String) -> Command<()> {
    Ok(core.browser_text(task_id, &text)?)
}

/// Sends a tab to an address, back, forward, again, or stops it.
#[tauri::command]
pub(crate) async fn browser_go(
    core: CoreState<'_>,
    task_id: i64,
    tab: String,
    go: BrowserGo,
) -> Command<()> {
    Ok(core.browser_go(task_id, &tab, &go).await?)
}

/// Opens a tab, on an address or empty. Returns its id.
#[tauri::command]
pub(crate) async fn browser_tab_new(
    core: CoreState<'_>,
    task_id: i64,
    url: Option<String>,
) -> Command<String> {
    Ok(core.browser_tab_new(task_id, url.as_deref()).await?)
}

/// Closes a tab.
#[tauri::command]
pub(crate) async fn browser_tab_close(
    core: CoreState<'_>,
    task_id: i64,
    tab: String,
) -> Command<()> {
    Ok(core.browser_tab_close(task_id, &tab).await?)
}

/// Answers the dialog a tab opened.
#[tauri::command]
pub(crate) async fn browser_dialog(
    core: CoreState<'_>,
    task_id: i64,
    tab: String,
    accept: bool,
    text: Option<String>,
) -> Command<()> {
    Ok(core
        .browser_dialog(task_id, &tab, accept, text.as_deref())
        .await?)
}

/// What a tab logged that is kept, oldest first.
#[tauri::command(async)]
pub(crate) fn browser_console(
    core: CoreState<'_>,
    task_id: i64,
    tab: String,
) -> Command<Vec<ConsoleLine>> {
    Ok(core.browser_console(task_id, &tab)?)
}

/// Puts the tab the pane shows into Chrome's inspect mode for a pick that goes to session
/// `session_id`, and waits for the click: the element picked, or nothing if picking ended
/// without one.
#[tauri::command]
pub(crate) async fn browser_pick(
    core: CoreState<'_>,
    task_id: i64,
    session_id: i64,
) -> Command<Option<PickedElement>> {
    Ok(core.browser_pick(task_id, session_id).await?)
}

/// Ends picking in the task's browser without a pick.
#[tauri::command]
pub(crate) fn browser_pick_end(core: CoreState<'_>, task_id: i64) -> Command<()> {
    Ok(core.browser_pick_end(task_id)?)
}

/// The elements picked for a session, by number, as they were kept: for showing one large.
#[tauri::command(async)]
pub(crate) fn session_elements(
    core: CoreState<'_>,
    id: i64,
    numbers: Vec<u64>,
) -> Command<Vec<PickedElement>> {
    Ok(core.session_elements(id, &numbers)?)
}
