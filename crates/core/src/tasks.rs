//! Task lifecycle: create, set up, remove.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::agent::Auth;
use crate::chats::CHATS;
use crate::config::ProjectConfig;
use crate::environments::EnvironmentChoice;
use crate::error::{Error, Result};
use crate::hooks::OutputSink;
use crate::paths::Paths;
use crate::sessions::is_busy_status;
use crate::setups::{self, Setup, SetupRun, SetupStep, SetupStop};
use crate::stacks::HOOK_LIMIT;
use crate::store::{NewTask, Project, SetupStatus, Task};
use crate::vars::{Vars, env_name};
use crate::{Core, git, hooks, leftovers, lock, paths, prefix};
use crate::{Settings, open};

/// Options for [`Core::task_new`].
#[derive(Debug, Default, Clone)]
pub struct NewTaskOptions<'a> {
    /// Check out this existing branch instead of creating one from the base.
    pub existing_branch: Option<&'a str>,
    /// Start the new branch from this branch, here or on a remote (`origin/<name>`), instead
    /// of the repository's base, which it then is for the task: its changes, its pull
    /// request's base. For work stacked on another branch's. Not with `existing_branch`.
    pub from: Option<&'a str>,
    /// The environment of the project to set the task up in: by default the project's default
    /// one, if it has one.
    pub environment: EnvironmentChoice,
    /// Take the first of the name, `name-2`, `name-3` and so on that is free, as a name made
    /// up for the user is: one no task of the project has had, whose branch exists neither
    /// here nor on a remote, and whose folder is free. Without it, a name that is taken is
    /// refused, as one the user chose is. The task returned has the name taken.
    pub free_name: bool,
    /// The number of the repository's GitHub issue the task is for, which it records. Its
    /// pull request text then closes that issue (`issues.rs`). Left out of a chat, which has
    /// no repository.
    pub issue: Option<u64>,
}

/// A task just written down, with what setting it up needs.
pub(crate) struct Made {
    pub(crate) project: Project,
    pub(crate) config: ProjectConfig,
    pub(crate) environment: Option<String>,
    pub(crate) task: Task,
}

/// What a new task in a project can start from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BranchChoices {
    /// The ref a new branch would be created from.
    pub base: String,
    /// Existing branches a task could check out instead, sorted.
    pub existing: Vec<String>,
}

/// Options for [`Core::task_remove`].
#[derive(Debug, Default, Clone, Copy)]
pub struct RemoveOptions {
    /// Remove even with uncommitted changes, unpushed commits or a failing `teardown` hook.
    pub force: bool,
    /// Also delete the task's local branch, unless commits on it exist nowhere else: on neither
    /// the base nor a remote. Such a branch is kept, and that is said.
    pub delete_branch: bool,
}

impl Core {
    /// What a new task in `project` can start from. Reads local refs only; it does not fetch.
    pub async fn branch_choices(&self, project: &str) -> Result<BranchChoices> {
        let project = self.project(project)?;
        let repo = project.repo()?;
        let config = self.project_config(&project.name)?;
        let base = if config.base_branch.is_empty() {
            git::default_base(repo).await?
        } else {
            config.base_branch
        };
        let existing = git::available_branches(repo).await?;
        Ok(BranchChoices { base, existing })
    }

    /// The project's tasks, oldest first, archived ones last when included.
    pub fn tasks(&self, project: &str, include_archived: bool) -> Result<Vec<Task>> {
        let project = match self.project(project) {
            // The chats have no home until the first one is made, and none until then.
            Err(Error::ProjectNotFound(_)) if project == CHATS => return Ok(vec![]),
            found => found?,
        };
        self.store().tasks(project.id, include_archived)
    }

    /// The active task called `name`, with its project.
    pub fn task(&self, project: &str, name: &str) -> Result<(Project, Task)> {
        let project = self.project(project)?;
        let task = self.store().task_by_name(project.id, name)?;
        let task = task.ok_or_else(|| Error::TaskNotFound {
            project: project.name.clone(),
            task: name.to_string(),
        })?;
        Ok((project, task))
    }

    /// Creates a task: a branch, a worktree outside the repo, and a slot, then runs `setup`.
    ///
    /// If `setup` fails the task is kept with [`SetupStatus::Failed`] and the hook's error is
    /// returned, so the worktree can be inspected and `setup` retried with [`Core::task_setup`].
    ///
    /// In [`CHATS`] it makes a chat instead: a folder of its own, with no repository, branch
    /// or setup.
    pub async fn task_new(
        &self,
        project: &str,
        name: &str,
        options: NewTaskOptions<'_>,
        sink: OutputSink<'_>,
    ) -> Result<Task> {
        paths::validate_name(name)?;
        if project == CHATS {
            return self.chat_new(name, &options).await;
        }
        let made = self.task_made(project, name, &options, sink).await?;
        // From here on a removal stops the setup on purpose, rather than finding its hook among
        // what runs in the worktree and killing it.
        let run = self.setup_begin(&made.task)?;
        self.set_up_made(made, &run, None, sink).await
    }

    /// Makes a task in a repository and writes it down: fetches, makes its branch and its
    /// worktree, and gives it a slot. Setting it up is left to the caller.
    pub(crate) async fn task_made(
        &self,
        project: &str,
        name: &str,
        options: &NewTaskOptions<'_>,
        sink: OutputSink<'_>,
    ) -> Result<Made> {
        let project = self.project(project)?;
        let environment = self.environment_chosen(&project.name, &options.environment)?;
        let config = self.config_in(&project.name, environment.as_deref())?;
        let repo = project.repo()?;

        sink("fetching");
        git::fetch(repo).await?;
        // A brand new repository has nothing to branch from until it has a commit.
        git::ensure_first_commit(repo).await?;
        let base = match options.from {
            Some(from) => start_point(repo, from, options.existing_branch).await?,
            None => base_of(repo, &config).await?,
        };
        let pattern = BranchPattern {
            project: &project,
            pattern: &config.branch_pattern,
            prefix: prefix::resolved(&self.settings()?.branch_prefix),
        };
        // After the fetch, so that a branch just pushed from elsewhere counts too.
        // Claimed until the task is written down, which then takes the name.
        let claim = self
            .name_for_task(&project, name, &pattern, options)
            .await?;
        let name = &claim.name;
        let worktree = self.paths.worktree(&project.name, name);
        if let Some(parent) = worktree.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let branch = match options.existing_branch {
            Some(branch) => branch.to_string(),
            None => pattern.branch(name)?,
        };
        sink(&format!("making the worktree, on {branch}"));
        add_worktree(
            repo,
            &worktree,
            &branch,
            (&base, options.existing_branch.is_none()),
        )
        .await?;

        let new = NewTask {
            project_id: project.id,
            name,
            branch: &branch,
            base_ref: &base,
            worktree_path: &worktree,
            slot: 0,
            environment: environment.as_deref(),
            branch_made: options.existing_branch.is_none(),
            issue: options.issue,
        };
        let task = match self.insert_in_free_slot(new) {
            Ok(task) => task,
            Err(e) => {
                // Do not leave a worktree behind that Swarmed has no record of.
                let created = options.existing_branch.is_none().then_some(branch.as_str());
                discard_worktree(&project, &worktree, created).await;
                return Err(e);
            }
        };
        drop(claim);
        Ok(Made {
            project,
            config,
            environment,
            task,
        })
    }

    /// Sets up a task just written down: puts the snapshot of its environment in place, then
    /// runs `setup` and what follows it, as [`Core::task_setup`] does with `send`.
    pub(crate) async fn set_up_made(
        &self,
        made: Made,
        run: &SetupRun,
        send: Option<Auth>,
        sink: OutputSink<'_>,
    ) -> Result<Task> {
        let setup = &run.setup;
        let said = |line: &str| {
            setup.say(line);
            sink(line);
        };
        let Made {
            project,
            config,
            environment,
            task,
        } = made;
        // What an earlier task's setup produced, so that this one's has less to do. Not
        // having it is no reason to fail: setup then does all the work, as it always could.
        let seeded = self
            .seed_from_snapshot(
                &project.name,
                environment.as_deref(),
                &task.worktree_path,
                &said,
            )
            .await;
        match seeded {
            Ok(Some(took)) => said(&format!(
                "started from the snapshot of this environment (copied in {})",
                crate::snapshots::seconds(took)
            )),
            Ok(None) => {}
            Err(error) => said(&format!("the snapshot could not be used: {error}")),
        }
        self.run_setup(&project, &config, task, (true, run), send, sink)
            .await
    }

    /// Takes the slot and writes the row under one lock, so two concurrent creations cannot
    /// get the same slot.
    pub(crate) fn insert_in_free_slot(&self, mut new: NewTask<'_>) -> Result<Task> {
        let store = self.store();
        new.slot = store.free_slot(new.project_id)?;
        store.task_insert(&new)
    }

    /// Runs the `setup` hook again, for a task whose setup failed or whose hook changed.
    /// Refused while the task's setup runs already, here or in another Swarmed.
    ///
    /// With `send`, the messages the task's sessions wait to send until it is set up go once
    /// it finishes well, their agents started with `send`: the message a task was started with
    /// ([`Core::task_start`]), and what was queued meanwhile. Without, they stay waiting.
    pub async fn task_setup(
        &self,
        project: &str,
        name: &str,
        send: Option<Auth>,
        sink: OutputSink<'_>,
    ) -> Result<Task> {
        let (project, task) = self.repo_task(project, name)?;
        let config = self.config_in(&project.name, task.environment.as_deref())?;
        // A task set up well before has been lived in since: its stack's logs and caches lie
        // beside what setup made, so it is not what new tasks should start from.
        let fresh = task.setup != SetupStatus::Ok;
        if self.setup_runs_elsewhere(&task) {
            return Err(Error::SetupRunning(task.name));
        }
        // What a restart took out of the queues, so that the setup holds it and sends it. Read
        // first: the store is let go before the loop, which reads it again.
        let sessions = self.store().sessions(task.id)?;
        for session in sessions {
            self.queue_kept_again(session.id)?;
        }
        let run = self.setup_begin(&task)?;
        self.run_setup(&project, &config, task, (fresh, &run), send, sink)
            .await
    }

    /// Runs the task's `setup` and what follows it, then says how it ended: in its log, to
    /// front ends and in the database. `fresh` for a task never set up well before, which is
    /// one a first snapshot can be of; `run` is the task's place among the running setups,
    /// through which a removal, the user or quitting stops it. With `send`, the messages its
    /// sessions wait to send go once it finishes well.
    async fn run_setup(
        &self,
        project: &Project,
        config: &ProjectConfig,
        task: Task,
        (fresh, run): (bool, &SetupRun),
        send: Option<Auth>,
        sink: OutputSink<'_>,
    ) -> Result<Task> {
        let setup = &run.setup;
        let said = |line: &str| {
            setup.say(line);
            sink(line);
        };
        let result = self
            .setup_steps(project, config, task, (fresh, setup), &said)
            .await;
        if let (Ok(task), Some(auth)) = (&result, send) {
            self.send_waiting(task, &auth, &said);
        }
        setup.end(setups::setup_end(&result));
        result
    }

    /// The steps of a setup: its hook, once a place is free for it; the first snapshot of its
    /// environment, if it is one; and its stack.
    async fn setup_steps(
        &self,
        project: &Project,
        config: &ProjectConfig,
        mut task: Task,
        (fresh, setup): (bool, &Setup),
        sink: OutputSink<'_>,
    ) -> Result<Task> {
        let result = self.setup_hook(setup, project, config, &task, sink).await;
        // Stopped while it ran, the hook ended because it was told to: no failure of its own.
        if let Some(why) = setup.interrupted() {
            return self.setup_stopped(task, why);
        }
        let reason = result.as_ref().err().map(setups::failure_said);
        task.setup = if result.is_ok() {
            SetupStatus::Ok
        } else {
            SetupStatus::Failed
        };
        task.setup_reason.clone_from(&reason);
        self.store()
            .task_set_setup_said(task.id, task.setup, reason.as_deref())?;
        // The first task set up well in its environment is what the next ones start from: kept
        // before its stack starts writing logs and caches beside what setup made. Not keeping
        // it is said, and costs the task nothing.
        if result.is_ok() && fresh && setup.stopped().is_none() {
            setup.step_to(SetupStep::Keeping);
            self.keep_first_snapshot(project, &task, sink).await;
        }
        // A removal that came meanwhile takes the task: nothing more is started in it.
        if let Some(why @ SetupStop::Removed) = setup.stopped() {
            return Err(Error::SetupStopped {
                task: task.name,
                why,
            });
        }
        // Set up, so what the task keeps running can be started, unless Swarmed is on its way
        // out. That failing does not make the task unusable: it is said, and the stack can be
        // started again later.
        let started = (result.is_ok() && setup.stopped().is_none()).then(|| {
            setup.step_to(SetupStep::Stack);
            self.start_stack(project, config, &task)
        });
        if let Some(Err(error)) = started {
            sink(&format!("the stack could not be started: {error}"));
        }
        result.map(|()| task)
    }

    /// Runs the `setup` hook in a process group of its own, which a stop ends with everything
    /// it started, once a place is free for it among the project's setups. One stopped before
    /// it began is not begun.
    async fn setup_hook(
        &self,
        setup: &Setup,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
        sink: OutputSink<'_>,
    ) -> Result<()> {
        self.setup_place(setup, config.max_setups).await;
        // An empty hook is no command, and is not run.
        let ran = match self.hook_command(&config.hooks.setup, project, config, task) {
            _ if setup.stopped().is_some() => Ok(()),
            Ok((command, env)) => {
                let cwd = &task.worktree_path;
                let started = |group| setup.started(group);
                hooks::run_in_group("setup", &command, cwd, &env, sink, &started).await
            }
            Err(error) => Err(error),
        };
        self.setup_hook_over(setup);
        ran
    }

    /// What a setup stopped while it ran comes to. Stopped by asking, the task is not set up,
    /// and setup can be run again; a removal takes the task, and leaves it as it was.
    fn setup_stopped(&self, task: Task, why: SetupStop) -> Result<Task> {
        if why != SetupStop::Removed {
            self.store()
                .task_set_setup_said(task.id, SetupStatus::Failed, Some(why.reason()))?;
        }
        Err(Error::SetupStopped {
            task: task.name,
            why,
        })
    }

    /// Sends what the task's sessions waited to send until it was set up, starting their
    /// agents with `auth`. A message that cannot be sent is said, and stays waiting. One whose
    /// agent is in a turn is left to that turn's end, as any queue is: never sent into a busy
    /// agent.
    fn send_waiting(&self, task: &Task, auth: &Auth, sink: OutputSink<'_>) {
        // Every session of the task, not only those held when the setup began: one made
        // meanwhile was queued into as well, and no turn of its own would send it.
        let held = self.store().session_ids(task.id);
        let held = held.unwrap_or_else(|_| self.live.queues.held_of(task.id));
        // From here on their queues go as any do, also for a turn that ends before the last
        // of them is sent.
        self.live.queues.release_for_setup(task.id);
        let idle = |id: &i64| {
            self.session(*id)
                .is_ok_and(|session| !is_busy_status(session.status))
        };
        let waiting = held
            .into_iter()
            .filter(|id| self.live.queues.is_waiting(*id))
            .filter(idle);
        let unsent = waiting.filter_map(|id| {
            let sent = self.session_resume_queue(id, auth.clone());
            sent.err().map(|error| (id, error))
        });
        for (id, error) in unsent {
            sink(&format!(
                "what session {id} waited to send could not be sent: {error}"
            ));
        }
    }

    /// Removes a task: stops its agents, its setup if that is under way, its stack and its
    /// browser, runs `teardown`, ends whatever still runs in the worktree, takes the worktree
    /// away and archives the record. The browser's profile goes once the task is gone. A
    /// setup stopped so ends as [`Error::SetupStopped`], not failed.
    ///
    /// Without `force` this refuses when the worktree holds work that would be lost: changes
    /// not committed, commits not pushed, or a worktree git no longer knows, whose work cannot
    /// be checked. Nor, whatever `force` says, while another Swarmed runs the agent of one of its
    /// sessions ([`Error::RunsElsewhere`]): that agent works in the worktree, and only that
    /// Swarmed can stop it. Nothing else stops it. A `stop` or `teardown` that fails or hangs
    /// and a process that will not end are passed over, and returned to be said.
    ///
    /// A chat goes with its folder, which without `force` must be empty.
    pub async fn task_remove(
        &self,
        project: &str,
        name: &str,
        options: RemoveOptions,
        sink: OutputSink<'_>,
    ) -> Result<Vec<String>> {
        let (project, task) = self.task(project, name)?;
        if project.chats {
            return self.chat_remove(&project, &task, options.force, sink).await;
        }
        let repo = project.repo()?;
        self.none_run_elsewhere(&self.store().session_ids(task.id)?)?;
        // Nothing may still be running in a directory that is about to go: the agents are
        // waited for, and killed if they do not go by themselves.
        self.sessions_ended(task.id).await?;
        let exists = task.worktree_path.exists();
        let known = exists && git::worktree_known(&task.worktree_path).await;
        if exists && !options.force {
            ensure_removable(&task, known).await?;
        }
        let (config, mut notes) = self.config_for_removal(&project, &task);
        notes.extend(self.stop_for_removal(&project, &config, &task, sink).await);
        if exists {
            notes.extend(
                self.remove_worktree(&project, &config, &task, known, sink)
                    .await?,
            );
        } else {
            // The directory was deleted by hand; make git forget it too.
            git::worktree_prune(repo).await.ok();
        }
        self.store().task_archive(task.id)?;
        self.forget_note(&project.name, &task.name)?;
        self.forget_setup_files(task.id);
        notes.extend(self.browser_forget(task.id).await);
        // The task is gone by now: what is left of its checkpoints is the clean-up's to list.
        if let Err(error) = self.forget_checkpoints(task.id) {
            notes.push(format!("its checkpoints could not be deleted: {error}"));
        }
        self.sessions_forgotten(task.id)?;
        if options.delete_branch && git::local_branch_exists(repo, &task.branch).await {
            notes.extend(delete_branch_if_safe(repo, &task).await);
        }
        Ok(notes)
    }

    /// Stops what runs for a task that is being removed: its setup first, before anything else
    /// is ended in the worktree, which would find the hook there and kill it, and before the
    /// stack, which it would start once done; then the stack, and the browser. Its profile
    /// stays until the task is gone: a removal that fails after this must not have cost what
    /// was signed in to. Returns what did not go as it should, to be said.
    async fn stop_for_removal(
        &self,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
        sink: OutputSink<'_>,
    ) -> Vec<String> {
        let mut notes: Vec<String> = self
            .setup_stop_for_removal(task, sink)
            .await
            .into_iter()
            .collect();
        notes.extend(self.stop_stack(project, config, task, sink).await);
        notes.extend(self.stop_browser(task.id).await);
        notes
    }

    /// The name a new task gets: `name` itself, or with `free_name` the first of `name`,
    /// `name-2`, `name-3` and so on that is free. Refused if `name` is taken and may not be
    /// changed, or if no name up to `name-99` is free. A name another task is being created
    /// under counts as taken, and the one returned is claimed until the claim is dropped.
    pub(crate) async fn name_for_task(
        &self,
        project: &Project,
        name: &str,
        pattern: &BranchPattern<'_>,
        options: &NewTaskOptions<'_>,
    ) -> Result<NameClaim<'_>> {
        let tasks = self.store().tasks(project.id, options.free_name)?;
        // A chat has no branch to keep free.
        let branches = match project.repo() {
            Ok(repo) => git::branch_names(repo).await?,
            Err(_) => HashSet::new(),
        };
        // From here to the claim under one lock, with nothing awaited: tasks created at once
        // each see the names the others chose.
        let mut claimed = lock(&self.names_claimed);
        let creating = claimed.iter().filter(|(id, _)| *id == project.id);
        let taken = TakenNames {
            project,
            tasks: tasks
                .into_iter()
                .map(|task| task.name)
                .chain(creating.map(|(_, name)| name.clone()))
                .collect(),
            branches,
            pattern,
            existing_branch: options.existing_branch.is_some() || project.chats,
            paths: &self.paths,
        };
        let chosen = if options.free_name {
            first_free(name, &taken)?.ok_or_else(|| Error::NoFreeTaskName {
                project: project.name.clone(),
                task: name.to_string(),
            })?
        } else if let Some(reason) = taken.reason(name)? {
            return Err(reason);
        } else {
            name.to_string()
        };
        claimed.insert((project.id, chosen.clone()));
        Ok(NameClaim {
            claimed: &self.names_claimed,
            project: project.id,
            name: chosen,
        })
    }

    /// The configuration the task's hooks run with. One that cannot be read does not keep the
    /// task: it goes without its `stop` and `teardown`, and that is said.
    pub(crate) fn config_for_removal(
        &self,
        project: &Project,
        task: &Task,
    ) -> (ProjectConfig, Vec<String>) {
        match self.config_in(&project.name, task.environment.as_deref()) {
            Ok(config) => (config, vec![]),
            Err(error) => (
                ProjectConfig::default(),
                vec![format!(
                    "the project's configuration could not be read, so stop and teardown \
                     did not run: {error}"
                )],
            ),
        }
    }

    /// Takes a project out of Swarmed. Its tasks are removed as [`Core::task_remove`] removes
    /// them, keeping their branches, and then everything Swarmed holds about the project is
    /// deleted: its sessions' history, the notes on it, its snapshots, its environments and
    /// its own actions. The repository itself stays as it is, and so does the project's
    /// config file, in case the repository is added again.
    ///
    /// The snapshots and the environments have to go: they hold what git ignores, `.env`
    /// included, and variables' values, and another repository added under the same name
    /// later would otherwise be started from them.
    ///
    /// Without `force` this stops at the first task that holds unsaved work. The tasks before
    /// it are removed by then; the project and everything else about it are still there.
    /// Nothing is removed while another Swarmed runs the agent of a session of one of its
    /// tasks ([`Error::RunsElsewhere`]).
    /// Returns what its tasks' removals passed over, as [`Core::task_remove`] does.
    pub async fn project_remove(
        &self,
        name: &str,
        force: bool,
        sink: OutputSink<'_>,
    ) -> Result<Vec<String>> {
        let project = self.project(name)?;
        let options = RemoveOptions {
            force,
            delete_branch: false,
        };
        let tasks = self.store().tasks(project.id, false)?;
        // Asked of every task first, so that none is removed when one of them cannot be.
        for task in &tasks {
            self.none_run_elsewhere(&self.store().session_ids(task.id)?)?;
        }
        let mut notes = vec![];
        for task in tasks {
            let passed_over = self.task_remove(name, &task.name, options, sink).await?;
            notes.extend(
                passed_over
                    .into_iter()
                    .map(|note| format!("{}: {note}", task.name)),
            );
        }
        self.store().project_delete(project.id)?;
        self.forget_notes(name)?;
        self.forget_snapshots(name)?;
        forget_file(&self.paths.project_environments(name))?;
        forget_file(&self.paths.project_actions(name))?;
        Ok(notes)
    }

    /// Runs `teardown`, ends whatever still runs in the worktree, then takes the worktree away.
    /// A `teardown` that fails or takes longer than [`HOOK_LIMIT`] does not keep the worktree,
    /// and neither does a process that will not end: both are returned, to be said.
    async fn remove_worktree(
        &self,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
        known: bool,
        sink: OutputSink<'_>,
    ) -> Result<Vec<String>> {
        let teardown = &config.hooks.teardown;
        let tearing_down = self.run_hook_limited("teardown", teardown, project, config, task, sink);
        let mut notes = match tearing_down.await {
            Some(Ok(())) => vec![],
            Some(Err(error)) => vec![format!("teardown failed, and was passed over: {error}")],
            None => vec!["teardown took longer than a minute, and was passed over".to_string()],
        };
        notes.extend(leftovers::end_in(&task.worktree_path, sink).await);
        self.take_away_worktree(project, task, known).await?;
        Ok(notes)
    }

    /// Takes a worktree away. It is moved aside, which is instant, and deleted out of the way
    /// after: dependencies and build output can hold hundreds of thousands of files, and
    /// deleting them made a removal wait the better part of a minute. One that cannot be moved
    /// is deleted where it is.
    async fn take_away_worktree(&self, project: &Project, task: &Task, known: bool) -> Result<()> {
        let repo = project.repo()?;
        let path = &task.worktree_path;
        if let Some(aside) = move_aside(path, task.id) {
            // git still has the worktree where it was, which is not there any more.
            git::worktree_prune(repo).await.ok();
            delete_later(&aside);
            return Ok(());
        }
        if known {
            return git::worktree_remove(repo, path).await;
        }
        tokio::fs::remove_dir_all(path).await?;
        git::worktree_prune(repo).await.ok();
        Ok(())
    }

    /// Deletes what removals moved aside and did not see deleted, Swarmed having quit or the
    /// computer having stopped first.
    pub(crate) fn finish_removals(&self) {
        let Ok(projects) = std::fs::read_dir(&self.paths.worktrees_root) else {
            return;
        };
        let folders = projects
            .flatten()
            .map(|project| project.path().join(REMOVED));
        let left = folders
            .filter_map(|folder| std::fs::read_dir(folder).ok())
            .flatten();
        for entry in left.flatten() {
            delete_later(&entry.path());
        }
    }

    /// Where the task's stack is opened in a browser: what its project says, or else the
    /// first address the running stack printed that answers.
    pub fn task_url(&self, project: &str, task: &str) -> Result<Option<String>> {
        let (project, task) = self.task(project, task)?;
        let config = self.config_in(&project.name, task.environment.as_deref())?;
        // Asked for by name first, so that a URL that cannot be rendered is said.
        let named = self.url_of(&project, &config, &task)?;
        let printed = || self.reach(&project, &config, &task).urls.into_iter().next();
        Ok(named.or_else(printed))
    }

    pub(crate) fn url_of(
        &self,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
    ) -> Result<Option<String>> {
        if config.open_url.trim().is_empty() {
            return Ok(None);
        }
        let vars = self.task_vars(project, config, task)?;
        Ok(Some(vars.render(config.open_url.trim())?))
    }

    /// Opens the task's running stack in the default browser.
    pub fn task_open_url(&self, project: &str, task: &str) -> Result<()> {
        let url = self.task_url(project, task)?;
        let url = url.ok_or_else(|| Error::NoOpenUrl(project.to_string()))?;
        open::in_browser(&url)
    }

    /// Opens the task's worktree, with its branch checked out, in the user's editor.
    pub fn task_open_editor(&self, project: &str, task: &str) -> Result<()> {
        let (_, task) = self.task(project, task)?;
        open::in_editor(&self.settings()?.editor, &task.worktree_path)
    }

    /// Opens the task's worktree in the terminal application the user chose.
    pub fn task_open_terminal(&self, project: &str, task: &str) -> Result<()> {
        let (_, task) = self.task(project, task)?;
        open::in_terminal(&self.settings()?.terminal_app, &task.worktree_path)
    }

    /// The user's own preferences. What of the file could not be understood keeps its default
    /// here, and `config_problems` says what that was.
    pub fn settings(&self) -> Result<Settings> {
        Settings::load(&self.paths.settings())
    }

    /// Replaces the user's preferences.
    ///
    /// A branch prefix other than the one kept is checked first, by git's rules for a
    /// branch's name, and refused with git's reason. One kept already is not: one written by
    /// hand that git refuses must not stop every other setting from being saved, and git says
    /// why when a task is made with it.
    pub fn set_settings(&self, settings: &Settings) -> Result<()> {
        let prefix = prefix::trimmed(&settings.branch_prefix);
        if !prefix.is_empty() && prefix != self.settings()?.branch_prefix {
            prefix::branch_example(prefix)?;
        }
        settings.save(&self.paths.settings())
    }

    /// The variables a task's hooks and environment values can use.
    fn task_vars(&self, project: &Project, config: &ProjectConfig, task: &Task) -> Result<Vars> {
        let mut vars = branch_vars(project, &task.name);
        vars.set("branch", task.branch.as_str());
        vars.set("slot", task.slot.to_string());
        vars.set("worktree", task.worktree_path.to_string_lossy());
        vars.set("repo", project.repo_path.to_string_lossy());
        for (name, base) in &config.ports {
            let port = config
                .port_for(*base, task.slot)
                .ok_or_else(|| Error::Config {
                    path: self.paths.project_config(&project.name),
                    message: format!("port `{name}` overflows for slot {}", task.slot),
                })?;
            vars.set(format!("port.{name}"), port.to_string());
        }
        Ok(vars)
    }

    /// The variables a task's agents are started with: what the task's environment sets, the
    /// project's own and, on top, those of the environment the task was created in, with their
    /// values worked out; then every task variable as `SWARMED_<NAME>`, as hooks have them. A
    /// skill can tell from `SWARMED_TASK` that it already runs in a task's own worktree.
    pub(crate) fn task_env(&self, task: &Task) -> Result<Vec<(String, String)>> {
        let Some(project) = self.store().project_by_id(task.project_id)? else {
            return Ok(vec![]);
        };
        // A chat has no repository, branch or environment to tell of, and is no worktree a
        // skill could take it for.
        if project.chats {
            return Ok(vec![]);
        }
        let config = self.config_in(&project.name, task.environment.as_deref())?;
        let vars = self.task_vars(&project, &config, task)?;
        let rendered = |(name, value): (&String, &String)| Ok((name.clone(), vars.render(value)?));
        let mut env: Vec<(String, String)> =
            config.env.iter().map(rendered).collect::<Result<_>>()?;
        env.extend(exported(&vars));
        Ok(env)
    }

    /// Renders and runs one hook in the task's worktree, for no longer than [`HOOK_LIMIT`]:
    /// `None` when the hook took longer, and was killed with what it had started. Every
    /// variable is also exported as `SWARMED_<NAME>`, so a hook can be a script that never
    /// sees the template syntax.
    pub(crate) async fn run_hook_limited(
        &self,
        hook: &'static str,
        template: &str,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
        sink: OutputSink<'_>,
    ) -> Option<Result<()>> {
        if template.trim().is_empty() {
            return Some(Ok(()));
        }
        let (command, env) = match self.hook_command(template, project, config, task) {
            Ok(rendered) => rendered,
            Err(error) => return Some(Err(error)),
        };
        let cwd = &task.worktree_path;
        hooks::run_within(HOOK_LIMIT, hook, &command, cwd, &env, sink).await
    }

    /// A hook as it is run: its command with the variables filled in, and its environment.
    ///
    /// The command is read by a shell, and a value such as the name of somebody else's
    /// branch is not ours to trust, so it is filled in as [`Vars::render_for_shell`] does:
    /// always one word, never shell. The environment holds every value as it is.
    pub(crate) fn hook_command(
        &self,
        template: &str,
        project: &Project,
        config: &ProjectConfig,
        task: &Task,
    ) -> Result<(String, Vec<(String, String)>)> {
        let vars = self.task_vars(project, config, task)?;
        let command = vars.render_for_shell(template)?;
        let mut env: Vec<(String, String)> = Vec::new();
        for (key, value) in &config.env {
            env.push((key.clone(), vars.render(value)?));
        }
        env.extend(exported(&vars));
        Ok((command, env))
    }
}

/// Every task variable as an environment variable, `SWARMED_<NAME>`. Put last, so that they are
/// what a hook or an agent finds whatever the configuration sets.
fn exported(vars: &Vars) -> impl Iterator<Item = (String, String)> + '_ {
    vars.iter()
        .map(|(name, value)| (env_name(name), value.to_string()))
}

/// Removes a file of Swarmed's own. One that is not there is as good as removed.
fn forget_file(file: &Path) -> Result<()> {
    match std::fs::remove_file(file) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// The variables known before the task exists, which a branch pattern may use, and which
/// hooks have too.
fn branch_vars(project: &Project, task: &str) -> Vars {
    let mut vars = Vars::default();
    vars.set("user", prefix::login());
    vars.set("task", task);
    vars.set("project", project.name.as_str());
    vars
}

/// How a new task's name becomes its branch's: the repository's pattern, with the variables
/// known before the task exists and `{{prefix}}`. The prefix is for the pattern alone: hooks
/// have `{{branch}}`, which holds the prefix the task was made with.
pub(crate) struct BranchPattern<'a> {
    pub(crate) project: &'a Project,
    pub(crate) pattern: &'a str,
    /// What `{{prefix}}` stands for.
    pub(crate) prefix: String,
}

impl BranchPattern<'_> {
    /// The branch a new task called `task` gets.
    fn branch(&self, task: &str) -> Result<String> {
        let mut vars = branch_vars(self.project, task);
        vars.set("prefix", self.prefix.as_str());
        vars.render(self.pattern)
    }
}

/// A new task's name, kept from other tasks being created until this is dropped.
pub(crate) struct NameClaim<'a> {
    claimed: &'a Mutex<HashSet<(i64, String)>>,
    project: i64,
    pub(crate) name: String,
}

impl Drop for NameClaim<'_> {
    fn drop(&mut self) {
        let key = (self.project, std::mem::take(&mut self.name));
        lock(self.claimed).remove(&key);
    }
}

/// What a new task's name must not be: a task's, the name of a folder that is there, or
/// that of a branch, here or on a remote.
struct TakenNames<'a> {
    project: &'a Project,
    /// The project's tasks' names; with `free_name` those of removed tasks too, since a
    /// removed task's branch can still be there.
    tasks: Vec<String>,
    /// Every branch here and on a remote, by its short name.
    branches: HashSet<String>,
    /// What a task's name becomes as a branch.
    pattern: &'a BranchPattern<'a>,
    /// Whether the task checks out a branch that exists, rather than making one, or is a chat
    /// and has none: either way no branch is made, and there is none to keep free.
    existing_branch: bool,
    paths: &'a Paths,
}

impl TakenNames<'_> {
    /// Why `name` cannot be a new task's, if it cannot.
    fn reason(&self, name: &str) -> Result<Option<Error>> {
        if self.tasks.iter().any(|task| task == name) {
            return Ok(Some(Error::TaskExists {
                project: self.project.name.clone(),
                task: name.to_string(),
            }));
        }
        let worktree = self.paths.worktree(&self.project.name, name);
        if worktree.exists() {
            return Ok(Some(Error::TaskFolderExists(worktree)));
        }
        // A branch the task is to check out is expected to exist.
        if self.existing_branch {
            return Ok(None);
        }
        let branch = self.pattern.branch(name)?;
        Ok(self
            .branches
            .contains(&branch)
            .then_some(Error::BranchExists(branch)))
    }
}

/// The first of `name`, `name-2`, `name-3` and so on up to `name-99` that is not taken.
fn first_free(name: &str, taken: &TakenNames<'_>) -> Result<Option<String>> {
    let candidates =
        std::iter::once(name.to_string()).chain((2..100).map(|number| format!("{name}-{number}")));
    for candidate in candidates {
        paths::validate_name(&candidate)?;
        if taken.reason(&candidate)?.is_none() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// The branch new tasks start from: the configured one, or the repository's default.
async fn base_of(repo: &Path, config: &ProjectConfig) -> Result<String> {
    if config.base_branch.is_empty() {
        git::default_base(repo).await
    } else {
        Ok(config.base_branch.clone())
    }
}

/// The branch a new task's branch starts from when it is asked for one: `from` itself, once it
/// is known to be a branch, here or on a remote. Only a branch, since the task keeps it as its
/// base and its pull request goes into it; and only for a new branch. One that is only on
/// `origin` can be named as a pull request names its head, without the remote, and is then
/// `origin/<from>`.
async fn start_point(repo: &Path, from: &str, existing: Option<&str>) -> Result<String> {
    if let Some(existing) = existing {
        return Err(Error::ExistingBranchStartsFromNothing(existing.to_string()));
    }
    for kind in ["refs/heads", "refs/remotes"] {
        // By its exact name: `main~1` is a commit and no branch.
        if git::is_ref(repo, &format!("{kind}/{from}")).await {
            return Ok(from.to_string());
        }
    }
    let on_origin = format!("origin/{from}");
    if git::is_ref(repo, &format!("refs/remotes/{on_origin}")).await {
        return Ok(on_origin);
    }
    Err(Error::NoBranchToStartFrom(from.to_string()))
}

/// Adds the worktree at `worktree` on `branch`: a new branch from `base` if `new`, the
/// existing one otherwise.
async fn add_worktree(
    repo: &Path,
    worktree: &Path,
    branch: &str,
    (base, new): (&str, bool),
) -> Result<()> {
    if new {
        git::worktree_add_new(repo, worktree, branch, base).await
    } else {
        git::worktree_add_existing(repo, worktree, branch).await
    }
}

/// Deletes a removed task's local branch if nothing is lost by it; the note returned says why
/// it was kept. The task is gone by now, so a branch git would not check or delete is kept
/// and said too, instead of failing a removal that has been done.
async fn delete_branch_if_safe(repo: &Path, task: &Task) -> Option<String> {
    match delete_safe_branch(repo, task).await {
        Ok(note) => note,
        Err(error) => Some(format!("the branch {} was kept: {error}", task.branch)),
    }
}

/// Deletes the branch unless it is the one the task started from, whose commits cannot be
/// told from the base's, or commits on it exist nowhere else, which a forced removal can leave.
async fn delete_safe_branch(repo: &Path, task: &Task) -> Result<Option<String>> {
    if git::names_branch(repo, &task.base_ref, &task.branch).await? {
        return Ok(Some(format!(
            "the branch {} was kept: the task started from it",
            task.branch
        )));
    }
    match git::commits_only_on(repo, &task.branch, &task.base_ref).await? {
        0 => {
            git::branch_delete(repo, &task.branch).await?;
            Ok(None)
        }
        count => Ok(Some(format!(
            "the branch {} was kept: {count} commit(s) on it are on neither {} nor a remote",
            task.branch, task.base_ref
        ))),
    }
}

/// Best-effort removal of a worktree, and the branch created for it, after a failed creation.
async fn discard_worktree(project: &Project, worktree: &Path, branch: Option<&str>) {
    let Ok(repo) = project.repo() else {
        return;
    };
    git::worktree_remove(repo, worktree).await.ok();
    if let Some(branch) = branch {
        git::branch_delete(repo, branch).await.ok();
    }
}

/// Fails, as [`ensure_clean`] does, when removing the worktree would lose work, and also
/// when git no longer knows it, so that what is in it cannot be checked.
async fn ensure_removable(task: &Task, known: bool) -> Result<()> {
    if known {
        return ensure_clean(task).await;
    }
    Err(Error::TaskNotClean {
        task: task.name.clone(),
        reason: "a worktree git no longer knows, so what is in it cannot be checked".to_string(),
    })
}

/// The folder beside a project's worktrees that removed ones are moved into.
pub(crate) const REMOVED: &str = ".removed";

/// Moves a worktree into [`REMOVED`] beside it: on the same disk, so the move is instant.
/// `None` when it cannot be moved.
pub(crate) fn move_aside(worktree: &Path, id: i64) -> Option<PathBuf> {
    let folder = worktree.parent()?.join(REMOVED);
    std::fs::create_dir_all(&folder).ok()?;
    let name = worktree.file_name()?.to_string_lossy();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let aside = folder.join(format!("{name}-{id}-{now}"));
    std::fs::rename(worktree, &aside).ok()?;
    Some(aside)
}

/// Deletes a folder in a process of its own, which goes on if Swarmed quits meanwhile; what
/// it leaves is deleted when Swarmed next starts ([`Core::finish_removals`]). Nobody hears of
/// a file it could not delete, so what stays in [`REMOVED`] is counted with what Swarmed takes
/// up ([`Usage::removed_bytes`](crate::Usage::removed_bytes)).
pub(crate) fn delete_later(folder: &Path) {
    let deleting = std::process::Command::new("rm")
        .arg("-rf")
        .arg("--")
        .arg(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = deleting {
        // Waited for, so that it does not linger as an ended process nobody took note of.
        std::thread::spawn(move || child.wait());
    }
}

/// Refuses when removing the worktree would destroy work that exists nowhere else.
async fn ensure_clean(task: &Task) -> Result<()> {
    let reason = if git::is_dirty(&task.worktree_path).await? {
        "uncommitted changes".to_string()
    } else {
        match git::unpushed_commits(&task.worktree_path, &task.base_ref).await? {
            0 => return Ok(()),
            count => format!("{count} unpushed commit(s)"),
        }
    };
    Err(Error::TaskNotClean {
        task: task.name.clone(),
        reason,
    })
}
