//! Task lifecycle against a real temporary git repository.

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};

use common::{core_with_project, fixture, git, write_config};
use swarmed_core::agent::{AgentEvent, Auth, FakeBackend};
use swarmed_core::{
    Action, ChangeKind, Core, Environment, EnvironmentChoice, Error, NewTaskOptions, ProjectConfig,
    RemoveOptions, SessionOptions, Settings, SetupStatus, SetupStop, StackState, hooks,
};

#[tokio::test]
async fn project_add_writes_a_starter_config() {
    let fx = fixture();
    let core = core_with_project(&fx).await;

    let projects = core.projects().expect("test setup");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "demo");
    assert_eq!(projects[0].repo_path, fx.repo);
    assert!(fx.paths.project_config("demo").exists());
    assert_eq!(
        core.project_config("demo").expect("test setup"),
        ProjectConfig::default()
    );

    let again = core.project_add(&fx.repo, None).await;
    assert!(matches!(again, Err(Error::ProjectExists(_))));
}

#[tokio::test]
async fn project_add_rejects_a_directory_that_is_not_a_repo() {
    let fx = fixture();
    let core = Core::open(fx.paths.clone()).expect("test setup");
    let plain = fx.repo.parent().expect("test setup").join("plain");
    std::fs::create_dir(&plain).expect("test setup");
    assert!(matches!(
        core.project_add(&plain, None).await,
        Err(Error::NotARepo(_))
    ));
}

#[tokio::test]
async fn a_task_in_a_repository_without_commits_gets_an_empty_first_commit() {
    let fx = fixture();
    let core = Core::open(fx.paths.clone()).expect("test setup");
    let fresh = fx.repo.parent().expect("test setup").join("fresh");
    std::fs::create_dir(&fresh).expect("test setup");
    git(&fresh, &["init", "--quiet", "-b", "main"]);
    git(&fresh, &["config", "user.name", "Test"]);
    git(&fresh, &["config", "user.email", "test@example.com"]);
    // Work that is there already, staged and not, must come through untouched.
    std::fs::write(fresh.join("staged.txt"), "staged\n").expect("test setup");
    std::fs::write(fresh.join("loose.txt"), "loose\n").expect("test setup");
    git(&fresh, &["add", "staged.txt"]);
    core.project_add(&fresh, None).await.expect("adds");
    let choices = core.branch_choices("fresh").await.expect("reads");
    assert_eq!(choices.base, "main");

    let task = core
        .task_new("fresh", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates the task");
    assert_eq!(task.base_ref, "main");
    assert!(task.worktree_path.join(".git").exists());

    let output = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&fresh)
            .args(args)
            .output()
            .expect("git runs");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    // One commit, with nothing in it; the staged file is still only staged.
    assert_eq!(output(&["rev-list", "--count", "main"]), "1");
    assert_eq!(output(&["ls-tree", "--name-only", "main"]), "");
    assert_eq!(
        output(&["status", "--porcelain"]),
        "A  staged.txt\n?? loose.txt".trim()
    );

    // A second task starts from the same commit instead of making another.
    core.task_new("fresh", "two", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates the task");
    assert_eq!(output(&["rev-list", "--count", "--all"]), "1");
}

#[tokio::test]
async fn task_new_creates_branch_worktree_and_runs_setup_with_variables() {
    let fx = fixture();
    write_config(
        &fx,
        r#"
branch_pattern = "swarm/{{task}}"
[ports]
web = 3000
[hooks]
setup = "echo {{port.web}} {{slot}} {{task}} > setup.out && echo $SWARMED_PORT_WEB $STACK > env.out && echo hello from setup"
[env]
STACK = "{{project}}-{{task}}"
"#,
    );
    let core = core_with_project(&fx).await;

    let lines = Mutex::new(Vec::new());
    let sink = |line: &str| lines.lock().expect("test setup").push(line.to_string());
    let task = core
        .task_new("demo", "fix-login", NewTaskOptions::default(), &sink)
        .await
        .expect("test setup");

    assert_eq!(task.branch, "swarm/fix-login");
    assert_eq!(task.slot, 1);
    assert_eq!(task.setup, SetupStatus::Ok);
    assert_eq!(task.worktree_path, fx.paths.worktree("demo", "fix-login"));
    assert!(task.worktree_path.join("README.md").exists());
    assert_eq!(
        std::fs::read_to_string(task.worktree_path.join("setup.out")).expect("test setup"),
        "3100 1 fix-login\n"
    );
    assert_eq!(
        std::fs::read_to_string(task.worktree_path.join("env.out")).expect("test setup"),
        "3100 demo-fix-login\n"
    );
    // What making the task does, then what its setup printed.
    assert_eq!(
        *lines.lock().expect("test setup"),
        [
            "fetching",
            "making the worktree, on swarm/fix-login",
            "hello from setup"
        ]
    );

    // A second task gets the next slot and therefore different ports.
    let second = core
        .task_new(
            "demo",
            "invoices",
            NewTaskOptions::default(),
            &hooks::discard,
        )
        .await
        .expect("test setup");
    assert_eq!(second.slot, 2);
    assert_eq!(
        std::fs::read_to_string(second.worktree_path.join("setup.out")).expect("test setup"),
        "3200 2 invoices\n"
    );

    let duplicate = core
        .task_new(
            "demo",
            "invoices",
            NewTaskOptions::default(),
            &hooks::discard,
        )
        .await;
    assert!(matches!(duplicate, Err(Error::TaskExists { .. })));
}

#[tokio::test]
async fn tasks_survive_reopening_the_core() {
    let fx = fixture();
    {
        let core = core_with_project(&fx).await;
        core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
            .await
            .expect("test setup");
    }
    let core = Core::open(fx.paths.clone()).expect("test setup");
    let tasks = core.tasks("demo", false).expect("test setup");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "one");
    assert_eq!(tasks[0].setup, SetupStatus::Ok);
}

#[tokio::test]
async fn failed_setup_keeps_the_task_and_can_be_retried() {
    let fx = fixture();
    write_config(&fx, "[hooks]\nsetup = \"test -f allow-setup\"\n");
    let core = core_with_project(&fx).await;

    let result = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await;
    assert!(matches!(
        result,
        Err(Error::HookFailed { hook: "setup", .. })
    ));
    let (_, task) = core.task("demo", "one").expect("test setup");
    assert_eq!(task.setup, SetupStatus::Failed);

    std::fs::write(task.worktree_path.join("allow-setup"), "").expect("test setup");
    let task = core
        .task_setup("demo", "one", None, &hooks::discard)
        .await
        .expect("test setup");
    assert_eq!(task.setup, SetupStatus::Ok);
}

#[tokio::test]
async fn remove_refuses_dirty_and_unpushed_work_unless_forced() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");

    std::fs::write(task.worktree_path.join("new.txt"), "x").expect("test setup");
    let dirty = core
        .task_remove("demo", "one", RemoveOptions::default(), &hooks::discard)
        .await;
    assert!(
        matches!(dirty, Err(Error::TaskNotClean { reason, .. }) if reason.contains("uncommitted"))
    );

    git(&task.worktree_path, &["add", "."]);
    git(&task.worktree_path, &["commit", "--quiet", "-m", "work"]);
    let unpushed = core
        .task_remove("demo", "one", RemoveOptions::default(), &hooks::discard)
        .await;
    assert!(
        matches!(unpushed, Err(Error::TaskNotClean { reason, .. }) if reason.contains("unpushed"))
    );
    assert!(task.worktree_path.exists());

    let options = RemoveOptions {
        force: true,
        delete_branch: true,
    };
    let notes = core
        .task_remove("demo", "one", options, &hooks::discard)
        .await
        .expect("test setup");
    assert!(!task.worktree_path.exists());
    assert_eq!(core.tasks("demo", false).expect("test setup"), vec![]);
    let archived = core.tasks("demo", true).expect("test setup");
    assert_eq!(archived.len(), 1);
    assert!(archived[0].archived);
    // The commit is on no other branch, so its branch is kept, and that is said.
    assert!(
        notes
            .iter()
            .any(|note| note.contains(&task.branch) && note.contains("kept")),
        "{notes:?}"
    );

    // A name chosen by the user whose branch is still there is refused, saying why.
    let refused = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await;
    assert!(
        matches!(&refused, Err(Error::BranchExists(branch)) if *branch == task.branch),
        "{refused:?}"
    );
    // A name made up for the user steps past it. The slot is free again.
    let free = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    let again = core
        .task_new("demo", "one", free, &hooks::discard)
        .await
        .expect("test setup");
    assert_eq!((again.name.as_str(), again.slot), ("one-2", 1));
}

#[tokio::test]
async fn a_made_up_name_steps_past_branches_here_and_on_a_remote_and_folders_in_the_way() {
    let fx = fixture();
    write_config(&fx, "branch_pattern = \"swarm/{{task}}\"\n");
    let core = core_with_project(&fx).await;
    // A branch here, one only on a remote, and a folder where the worktree would go.
    git(&fx.repo, &["branch", "swarm/fix"]);
    git(
        &fx.repo,
        &["update-ref", "refs/remotes/origin/swarm/fix-2", "HEAD"],
    );
    let folder = fx.paths.worktree("demo", "fix-3");
    std::fs::create_dir_all(&folder).expect("test setup");

    let chosen = |name: &'static str| {
        core.task_new("demo", name, NewTaskOptions::default(), &hooks::discard)
    };
    assert!(
        matches!(chosen("fix").await, Err(Error::BranchExists(branch)) if branch == "swarm/fix")
    );
    let remote = chosen("fix-2").await;
    assert!(
        matches!(&remote, Err(Error::BranchExists(branch)) if branch == "swarm/fix-2"),
        "{remote:?}"
    );
    assert!(matches!(chosen("fix-3").await, Err(Error::TaskFolderExists(path)) if path == folder));

    let free = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    let task = core
        .task_new("demo", "fix", free.clone(), &hooks::discard)
        .await
        .expect("creates");
    assert_eq!(
        (task.name.as_str(), task.branch.as_str()),
        ("fix-4", "swarm/fix-4")
    );
    // Taken by a task now, the name is refused as the user's and stepped past as made up.
    assert!(matches!(
        chosen("fix-4").await,
        Err(Error::TaskExists { .. })
    ));
    let next = core
        .task_new("demo", "fix", free, &hooks::discard)
        .await
        .expect("creates");
    assert_eq!(next.name, "fix-5");
}

#[tokio::test]
async fn removing_a_task_deletes_its_branch_when_nothing_on_it_is_lost() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "done", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let options = RemoveOptions {
        force: false,
        delete_branch: true,
    };
    let notes = core
        .task_remove("demo", "done", options, &hooks::discard)
        .await
        .expect("removes");
    assert!(notes.iter().all(|note| !note.contains("kept")), "{notes:?}");
    let branches = std::process::Command::new("git")
        .args([
            "-C",
            &fx.repo.to_string_lossy(),
            "branch",
            "--list",
            &task.branch,
        ])
        .output()
        .expect("runs git");
    assert_eq!(String::from_utf8_lossy(&branches.stdout).trim(), "");
}

/// Whether the repository still has the local branch.
fn has_branch(repo: &Path, branch: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .status()
        .expect("runs git")
        .success()
}

#[tokio::test]
async fn the_branch_a_task_started_from_is_kept_even_as_its_own_base() {
    let fx = fixture();
    git(&fx.repo, &["branch", "release"]);
    write_config(&fx, "base_branch = \"release\"\n");
    let core = core_with_project(&fx).await;
    let options = NewTaskOptions {
        existing_branch: Some("release"),
        ..NewTaskOptions::default()
    };
    let task = core
        .task_new("demo", "on-release", options, &hooks::discard)
        .await
        .expect("test setup");
    // A commit only this branch has, which no comparison with the base, itself, can see.
    std::fs::write(task.worktree_path.join("new.txt"), "x").expect("test setup");
    git(&task.worktree_path, &["add", "."]);
    git(&task.worktree_path, &["commit", "--quiet", "-m", "work"]);

    let options = RemoveOptions {
        force: false,
        delete_branch: true,
    };
    let notes = core
        .task_remove("demo", "on-release", options, &hooks::discard)
        .await
        .expect("removes");
    assert!(
        notes
            .iter()
            .any(|note| note.contains("release") && note.contains("kept")),
        "{notes:?}"
    );
    assert!(has_branch(&fx.repo, "release"));
}

#[tokio::test]
async fn a_branch_that_cannot_be_checked_is_kept_and_the_removal_still_done() {
    let fx = fixture();
    git(&fx.repo, &["branch", "release"]);
    write_config(&fx, "base_branch = \"release\"\n");
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    // The base goes, as a remote's branch does once merged and pruned.
    git(&fx.repo, &["branch", "-D", "release"]);

    let options = RemoveOptions {
        force: true,
        delete_branch: true,
    };
    let notes = core
        .task_remove("demo", "one", options, &hooks::discard)
        .await
        .expect("the removal is done, whatever became of the branch");
    assert!(
        notes
            .iter()
            .any(|note| note.contains(&task.branch) && note.contains("kept")),
        "{notes:?}"
    );
    assert!(has_branch(&fx.repo, &task.branch));
    assert_eq!(core.tasks("demo", false).expect("lists"), vec![]);
}

#[tokio::test]
async fn what_a_tasks_sessions_left_goes_with_the_task_and_not_with_a_refused_removal() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    core.session_enqueue(session.id, "later").expect("queues");
    let spool = fx.paths.view_spool(session.id);
    let private = fx
        .paths
        .data_dir
        .join("run/agents")
        .join(session.id.to_string());
    for folder in [&spool, &private] {
        std::fs::create_dir_all(folder).expect("test setup");
        std::fs::write(folder.join("left"), "x").expect("test setup");
    }

    std::fs::write(task.worktree_path.join("new.txt"), "x").expect("test setup");
    let refused = core
        .task_remove("demo", "one", RemoveOptions::default(), &hooks::discard)
        .await;
    assert!(
        matches!(refused, Err(Error::TaskNotClean { .. })),
        "{refused:?}"
    );
    assert_eq!(core.session_queue(session.id).expect("reads").len(), 1);
    assert!(spool.join("left").exists() && private.join("left").exists());

    let options = RemoveOptions {
        force: true,
        delete_branch: false,
    };
    core.task_remove("demo", "one", options, &hooks::discard)
        .await
        .expect("removes");
    assert_eq!(core.session_queue(session.id).expect("reads"), vec![]);
    assert!(!spool.exists() && !private.exists());
}

#[tokio::test]
async fn remove_runs_teardown_and_passes_over_one_that_fails() {
    let fx = fixture();
    let marker = fx.repo.parent().expect("test setup").join("torn-down");
    write_config(
        &fx,
        &format!(
            "[hooks]\nteardown = \"test ! -f {{{{repo}}}}/block-teardown && touch {}\"\n",
            marker.display()
        ),
    );
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");

    std::fs::write(fx.repo.join("block-teardown"), "").expect("test setup");
    let notes = core
        .task_remove("demo", "one", RemoveOptions::default(), &hooks::discard)
        .await
        .expect("a failing teardown does not keep the task");
    assert!(
        notes.iter().any(|note| note.starts_with("teardown failed")),
        "{notes:?}"
    );
    assert!(!marker.exists());
    assert!(!task.worktree_path.exists());
    assert_eq!(core.tasks("demo", false).expect("test setup"), vec![]);

    std::fs::remove_file(fx.repo.join("block-teardown")).expect("test setup");
    let task = core
        .task_new("demo", "two", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let notes = core
        .task_remove("demo", "two", RemoveOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    assert_eq!(notes, Vec::<String>::new());
    assert!(marker.exists());
    assert!(!task.worktree_path.exists());
}

#[tokio::test]
async fn remove_ends_what_still_runs_in_the_worktree() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    // A stack Swarmed does not know of: started by hand, and still working in the worktree.
    let mut stray = std::process::Command::new("sleep")
        .arg("300")
        .current_dir(&task.worktree_path)
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("test setup");

    core.task_remove("demo", "one", RemoveOptions::default(), &hooks::discard)
        .await
        .expect("removes");
    let ended = stray.try_wait().expect("asks");
    stray.kill().ok();
    assert!(ended.is_some(), "the process in the worktree still runs");
    assert!(!task.worktree_path.exists());
}

#[tokio::test]
async fn removing_a_task_while_it_is_set_up_stops_setup_on_purpose_with_all_it_started() {
    let fx = fixture();
    // What setup starts: one that ignores being asked to stop and ends four seconds on, as a
    // tool that winds down its own does, and one working outside the worktree, where looking
    // for what runs in the worktree does not find it.
    write_config(
        &fx,
        "[hooks]\nsetup = \"(trap '' TERM; sleep 4) & echo $! > slow.pid; (cd / && exec sleep 300) & echo $! > outside.pid; wait\"\n",
    );
    let core = core_with_project(&fx).await;
    let worktree = fx.paths.worktree("demo", "one");
    let making = core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard);
    let removing = async {
        let started = [
            written_soon(&worktree.join("slow.pid")).await,
            written_soon(&worktree.join("outside.pid")).await,
        ];
        let options = RemoveOptions {
            force: true,
            delete_branch: false,
        };
        let removed = core
            .task_remove("demo", "one", options, &hooks::discard)
            .await;
        (removed, started)
    };
    let (made, (removed, started)) = tokio::join!(making, removing);

    // Said for what it was, not as a hook that failed.
    assert!(
        matches!(
            made,
            Err(Error::SetupStopped {
                why: SetupStop::Removed,
                ..
            })
        ),
        "{made:?}"
    );
    assert_eq!(removed.expect("removes"), Vec::<String>::new());
    for pid in started {
        assert!(!process_alive(&pid), "process {pid} still runs");
    }
    assert_eq!(core.tasks("demo", false).expect("lists"), vec![]);
    assert!(!worktree.exists());
}

#[tokio::test]
async fn a_setup_stopped_when_swarmed_goes_leaves_the_task_to_be_set_up_again() {
    let fx = fixture();
    write_config(
        &fx,
        "[hooks]\nsetup = \"test -f done || { sleep 300 & echo $! > sleeping.pid; wait; }\"\n",
    );
    let core = core_with_project(&fx).await;
    let worktree = fx.paths.worktree("demo", "one");
    let making = core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard);
    let stopping = async {
        let sleeping = written_soon(&worktree.join("sleeping.pid")).await;
        core.setups_stop_all().await;
        sleeping
    };
    let (made, sleeping) = tokio::join!(making, stopping);

    assert!(
        matches!(
            made,
            Err(Error::SetupStopped {
                why: SetupStop::Asked,
                ..
            })
        ),
        "{made:?}"
    );
    assert!(!process_alive(&sleeping), "process {sleeping} still runs");
    let (_, task) = core.task("demo", "one").expect("the task stays");
    assert_eq!(task.setup, SetupStatus::Failed);
    // A setup begun after that is stopped before it starts: Swarmed is on its way out.
    let again = core.task_setup("demo", "one", None, &hooks::discard).await;
    assert!(
        matches!(again, Err(Error::SetupStopped { .. })),
        "{again:?}"
    );
    std::fs::write(worktree.join("done"), "").expect("test setup");
    let core = Core::open(fx.paths.clone()).expect("reopens");
    let task = core
        .task_setup("demo", "one", None, &hooks::discard)
        .await
        .expect("sets up");
    assert_eq!(task.setup, SetupStatus::Ok);
}

/// Waits for `file` to be written, for as long as making a task may take on a slow machine,
/// and returns what it holds.
async fn written_soon(file: &Path) -> String {
    for _ in 0..400 {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        if !text.trim().is_empty() {
            return text.trim().to_string();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("{} was never written", file.display());
}

#[tokio::test]
async fn remove_finishes_a_worktree_git_has_forgotten() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    // A removal that failed halfway: git deleted its record of the worktree, and the folder
    // is still there.
    std::fs::remove_dir_all(fx.repo.join(".git/worktrees/one")).expect("test setup");
    let listed = |fx: &common::Fixture| {
        let out = std::process::Command::new("git")
            .args(["-C", &fx.repo.to_string_lossy(), "worktree", "list"])
            .output()
            .expect("lists");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let unchecked = core
        .task_remove("demo", "one", RemoveOptions::default(), &hooks::discard)
        .await;
    assert!(
        matches!(&unchecked, Err(Error::TaskNotClean { reason, .. }) if reason.contains("no longer knows")),
        "{unchecked:?}"
    );
    assert!(task.worktree_path.exists());

    let force = RemoveOptions {
        force: true,
        ..RemoveOptions::default()
    };
    core.task_remove("demo", "one", force, &hooks::discard)
        .await
        .expect("removes");
    assert!(!task.worktree_path.exists());
    assert_eq!(core.tasks("demo", false).expect("test setup"), vec![]);
    assert!(!listed(&fx).contains("/one "), "{}", listed(&fx));
}

#[tokio::test]
async fn a_removed_worktree_is_moved_aside_and_deleted_after() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    std::fs::create_dir_all(task.worktree_path.join("node_modules/a")).expect("test setup");
    std::fs::write(task.worktree_path.join("node_modules/a/index.js"), "").expect("test setup");

    // Forced: the fixture's repository does not ignore what it holds.
    let force = RemoveOptions {
        force: true,
        ..RemoveOptions::default()
    };
    core.task_remove("demo", "one", force, &hooks::discard)
        .await
        .expect("removes");
    assert!(!task.worktree_path.exists());
    let worktrees = std::process::Command::new("git")
        .args([
            "-C",
            &fx.repo.to_string_lossy(),
            "worktree",
            "list",
            "--porcelain",
        ])
        .output()
        .expect("lists");
    let worktrees = String::from_utf8_lossy(&worktrees.stdout).into_owned();
    assert_eq!(worktrees.matches("worktree ").count(), 1, "{worktrees}");

    // Deleted in a process of its own, which is soon done.
    let removed = task
        .worktree_path
        .parent()
        .expect("has one")
        .join(".removed");
    assert!(
        emptied(&removed).await,
        "{:?}",
        std::fs::read_dir(&removed).map(Iterator::count)
    );
}

#[tokio::test]
async fn what_a_removal_left_unfinished_is_deleted_at_the_next_start() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let left = fx
        .paths
        .worktrees_root
        .join("demo/.removed/one-1-1/node_modules");
    std::fs::create_dir_all(&left).expect("test setup");
    std::fs::write(left.join("index.js"), "").expect("test setup");

    core.arrive().expect("arrives");
    let removed = fx.paths.worktrees_root.join("demo/.removed");
    assert!(emptied(&removed).await);
}

/// Whether `folder` is empty, or gone, within a few seconds.
async fn emptied(folder: &Path) -> bool {
    for _ in 0..100 {
        let empty = std::fs::read_dir(folder).map_or(true, |mut entries| entries.next().is_none());
        if empty {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn a_task_is_set_up_in_the_environment_it_was_created_in() {
    let fx = fixture();
    write_config(
        &fx,
        "open_url = \"http://plain\"\n[hooks]\nsetup = \"echo plain > how.txt\"\n[env]\nMODE = \"plain\"\nSHARED = \"{{task}}\"\n",
    );
    let core = core_with_project(&fx).await;
    let staging = Environment {
        setup: "echo \"$MODE $SHARED\" > how.txt".to_string(),
        open_url: "http://staging/{{task}}".to_string(),
        env: [("MODE".to_string(), "staging".to_string())].into(),
        ..Environment::default()
    };
    core.environment_save("demo", "staging", &staging)
        .expect("saves");
    assert_eq!(
        core.environments("demo").expect("lists")["staging"],
        staging
    );
    // Its variables may be credentials: the file is the user's alone.
    let file = fx.paths.project_environments("demo");
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(&file).expect("exists").permissions(),
    );
    assert_eq!(mode & 0o777, 0o600);

    let options = |environment: &str| NewTaskOptions {
        environment: EnvironmentChoice::Named(environment.to_string()),
        ..NewTaskOptions::default()
    };
    let how = |task: &swarmed_core::Task| {
        std::fs::read_to_string(task.worktree_path.join("how.txt")).expect("set up")
    };
    // With no default environment, a task that chooses none is set up the project's own way.
    let plain = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let staged = core
        .task_new("demo", "two", options("staging"), &hooks::discard)
        .await
        .expect("creates");
    assert_eq!(
        (how(&plain).trim(), plain.environment.as_deref()),
        ("plain", None)
    );
    // The environment's setup ran, with its variable over the project's and the rest kept.
    assert_eq!(how(&staged).trim(), "staging two");
    assert_eq!(staged.environment.as_deref(), Some("staging"));
    assert_eq!(
        core.task_url("demo", "one").expect("reads").as_deref(),
        Some("http://plain")
    );
    assert_eq!(
        core.task_url("demo", "two").expect("reads").as_deref(),
        Some("http://staging/two")
    );

    // The agent working in the task gets the same variables.
    let mut bus = core.subscribe();
    let session = core
        .session_new("demo", "two", &SessionOptions::default())
        .expect("creates");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    core.session_send(session.id, "env: MODE").expect("sends");
    let said = common::turn(&mut bus, session.id).await;
    let staging_mode = AgentEvent::Text {
        text: "staging".into(),
    };
    assert!(said.contains(&staging_mode), "{said:?}");

    // One that is not there is refused. One a task uses cannot be removed, and one that goes
    // all the same, deleted by hand, leaves its tasks on the project's own.
    let missing = core
        .task_new("demo", "three", options("nope"), &hooks::discard)
        .await;
    assert!(matches!(missing, Err(Error::EnvironmentNotFound { .. })));
    let in_use = core.environment_remove("demo", "staging");
    assert!(
        matches!(&in_use, Err(Error::EnvironmentInUse { tasks, .. }) if tasks == "two"),
        "{in_use:?}"
    );
    std::fs::remove_file(&file).expect("deletes");
    assert_eq!(
        core.task_url("demo", "two").expect("reads").as_deref(),
        Some("http://plain")
    );
    let bad = Environment {
        env: [("NOT-A-NAME".to_string(), "x".to_string())].into(),
        ..Environment::default()
    };
    assert!(matches!(
        core.environment_save("demo", "bad", &bad),
        Err(Error::InvalidVariable(_))
    ));
}

#[tokio::test]
async fn a_task_that_chooses_no_environment_is_set_up_in_the_projects_default() {
    let fx = fixture();
    write_config(&fx, "[hooks]\nsetup = \"echo plain > how.txt\"\n");
    let core = core_with_project(&fx).await;
    for (name, setup) in [("docker", "echo docker > how.txt"), ("spare", "")] {
        let environment = Environment {
            setup: setup.to_string(),
            ..Environment::default()
        };
        core.environment_save("demo", name, &environment)
            .expect("saves");
    }
    assert_eq!(core.environment_default("demo").expect("reads"), None);
    let missing = core.environment_set_default("demo", Some("nope"));
    assert!(matches!(missing, Err(Error::EnvironmentNotFound { .. })));
    core.environment_set_default("demo", Some("docker"))
        .expect("sets");
    assert_eq!(
        core.environment_default("demo").expect("reads").as_deref(),
        Some("docker")
    );

    let how = |task: &swarmed_core::Task| {
        let set_up = std::fs::read_to_string(task.worktree_path.join("how.txt")).expect("set up");
        (set_up.trim().to_string(), task.environment.clone())
    };
    let new = |name: &'static str, environment: EnvironmentChoice| {
        let options = NewTaskOptions {
            environment,
            ..NewTaskOptions::default()
        };
        core.task_new("demo", name, options, &hooks::discard)
    };
    // Chosen by nobody, the default; the project's own setup, or another, only when chosen.
    let default = new("a", EnvironmentChoice::Default).await.expect("creates");
    let own = new("b", EnvironmentChoice::Own).await.expect("creates");
    let named = EnvironmentChoice::Named("spare".to_string());
    let spare = new("c", named).await.expect("creates");
    let docker = "docker".to_string();
    assert_eq!(how(&default), (docker.clone(), Some(docker)));
    assert_eq!(how(&own), ("plain".to_string(), None));
    assert_eq!(spare.environment.as_deref(), Some("spare"));

    // Removing the default leaves none, and so does setting none.
    let force = RemoveOptions {
        force: true,
        ..RemoveOptions::default()
    };
    core.task_remove("demo", "c", force, &hooks::discard)
        .await
        .expect("removes");
    core.environment_set_default("demo", Some("spare"))
        .expect("sets");
    core.environment_remove("demo", "spare").expect("removes");
    assert_eq!(core.environment_default("demo").expect("reads"), None);
    core.environment_set_default("demo", Some("docker"))
        .expect("sets");
    core.environment_set_default("demo", None).expect("clears");
    assert_eq!(core.environment_default("demo").expect("reads"), None);

    // A default taken out of the file by hand, as an older Swarmed would, is none: a new task
    // is then set up the project's own way, not refused.
    core.environment_set_default("demo", Some("docker"))
        .expect("sets");
    std::fs::remove_file(fx.paths.project_environments("demo")).expect("deletes");
    assert_eq!(core.environment_default("demo").expect("reads"), None);
    let after = new("d", EnvironmentChoice::Default).await.expect("creates");
    assert_eq!(how(&after), ("plain".to_string(), None));
}

#[tokio::test]
async fn what_a_task_keeps_running_is_started_after_setup_and_does_not_hold_the_task_up() {
    let fx = fixture();
    // `start` never ends by itself, as a dev server does not. It leaves a child behind, to
    // show that stopping takes the whole group.
    write_config(
        &fx,
        "[hooks]\nsetup = \"echo ready > setup.txt\"\nstart = \"echo serving {{task}}; sleep 300 & echo $! > child.pid; wait\"\nstop = \"echo stopping > stopped.txt\"\n",
    );
    let core = core_with_project(&fx).await;
    let stack = |core: &Core, index: usize| core.overview().expect("reads")[0].tasks[index].stack;

    // The task is there as soon as setup is done, with its stack running behind it.
    let created = core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard);
    let task = tokio::time::timeout(std::time::Duration::from_secs(20), created)
        .await
        .expect("setup ended though `start` did not")
        .expect("creates");
    assert!(task.worktree_path.join("setup.txt").exists());
    assert_eq!(stack(&core, 0), Some(StackState::Starting));
    let pid_file = task.worktree_path.join("child.pid");
    let started = |core: &Core| {
        pid_file.exists() && !core.stack_log("demo", "one").expect("reads").is_empty()
    };
    let mut waits = 0;
    while !started(&core) && waits < 200 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        waits += 1;
    }
    assert_eq!(
        core.stack_log("demo", "one").expect("reads"),
        "serving one\n"
    );
    // Starting what is running starts nothing more.
    assert!(core.stack_start("demo", "one").expect("starts"));

    // Stopping runs `stop`, and ends what `start` left running, children and all.
    let child = std::fs::read_to_string(&pid_file).expect("written");
    let alive = |pid: &str| {
        let probe = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .output();
        probe.expect("kill runs").status.success()
    };
    assert!(alive(&child));
    core.stack_stop("demo", "one", &hooks::discard)
        .await
        .expect("stops");
    assert!(task.worktree_path.join("stopped.txt").exists());
    assert_eq!(stack(&core, 0), Some(StackState::Stopped));
    assert!(!alive(&child), "the stack's child outlived it");

    // It can be started again, and goes with its task.
    assert!(core.stack_start("demo", "one").expect("starts"));
    assert_eq!(stack(&core, 0), Some(StackState::Starting));
    let mut again = std::fs::read_to_string(&pid_file).expect("written");
    let mut waits = 0;
    while (again == child || !alive(&again)) && waits < 300 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        again = std::fs::read_to_string(&pid_file).unwrap_or_default();
        waits += 1;
    }
    assert!(
        again != child && alive(&again),
        "the stack did not start again"
    );
    let kept = fx
        .paths
        .data_dir
        .join(format!("run/stacks/{}.pid", task.id));
    assert!(kept.exists());
    let force = RemoveOptions {
        force: true,
        ..RemoveOptions::default()
    };
    core.task_remove("demo", "one", force, &hooks::discard)
        .await
        .expect("removes");
    assert!(!alive(&again), "the stack outlived its task");
    assert!(!kept.exists());
    assert!(!task.worktree_path.exists());
    core.stacks_stop_all().await;
}

/// When the process `pid` started, in the words Swarmed writes it down in, or in those of
/// wherever this test runs.
fn start_time(pid: &str, fixed: bool) -> String {
    let mut ps = std::process::Command::new("ps");
    if fixed {
        ps.env("LC_ALL", "C").env("TZ", "UTC");
    }
    let asked = ps.args(["-o", "lstart=", "-p", pid.trim()]).output();
    String::from_utf8_lossy(&asked.expect("ps runs").stdout)
        .trim()
        .to_string()
}

#[tokio::test]
async fn a_command_beside_a_running_swarmed_leaves_its_stacks_alone() {
    let fx = fixture();
    write_config(
        &fx,
        "[hooks]\nstart = \"sleep 300 & echo $! > child.pid; wait\"\n",
    );
    let app = core_with_project(&fx).await;
    let task = app
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let child = written(&task.worktree_path.join("child.pid")).await;
    let kept = fx
        .paths
        .data_dir
        .join(format!("run/stacks/{}.pid", task.id));
    assert!(kept.exists());

    // A second Swarmed on the same data, as a command run beside the app is. It started a
    // stack of its own, and ends that one and no other when it is done.
    let command =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    write_config(
        &fx,
        "max_stacks = 5\n[hooks]\nstart = \"sleep 300 & echo $! > child.pid; wait\"\n",
    );
    let own = command
        .task_new("demo", "two", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let own_child = written(&own.worktree_path.join("child.pid")).await;
    let own_kept = fx.paths.data_dir.join(format!("run/stacks/{}.pid", own.id));
    assert!(own_kept.exists());
    command.stacks_stop_started().await;
    assert!(!process_alive(&own_child));
    assert!(!own_kept.exists());
    assert!(process_alive(&child), "the app's stack was ended");
    assert!(kept.exists(), "the app's stack was forgotten");
    let stack = |core: &Core| core.overview().expect("reads")[0].tasks[0].stack;
    assert_eq!(stack(&app), Some(StackState::Starting));

    // Nor does a second Swarmed that quits, when the first has arrived before it.
    app.arrive().expect("arrives");
    command.arrive().expect("arrives");
    command.leave().await;
    assert!(process_alive(&child), "the app's stack was ended");

    // The first one quitting is another matter: then everything goes.
    app.leave().await;
    assert!(!process_alive(&child));
    assert!(!kept.exists());
}

#[tokio::test]
async fn a_stack_is_known_by_a_start_time_that_reads_the_same_everywhere() {
    let fx = fixture();
    write_config(&fx, "[hooks]\nstart = \"sleep 300\"\n");
    let first = core_with_project(&fx).await;
    let task = first
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let kept = fx
        .paths
        .data_dir
        .join(format!("run/stacks/{}.pid", task.id));
    let written = std::fs::read_to_string(&kept).expect("written");
    let (pid, started) = written.trim().split_once('\n').expect("two lines");
    // Not in the words of this machine's language and time zone, which another Swarmed,
    // started from somewhere else, would not get out of `ps` again.
    assert_eq!(started, start_time(pid, true));
    drop(first);

    // A file from before, in the words of wherever that Swarmed ran: if they are this
    // one's too, the stack is still known, and written down anew.
    std::fs::write(&kept, format!("{pid}\n{}\n", start_time(pid, false))).expect("writes");
    let second =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let stack = |core: &Core| core.overview().expect("reads")[0].tasks[0].stack;
    assert_eq!(stack(&second), Some(StackState::Starting));
    assert_eq!(std::fs::read_to_string(&kept).expect("kept"), written);

    // A process id that somebody else has by now is forgotten, as ever.
    std::fs::write(&kept, format!("{pid}\nlong ago\n")).expect("writes");
    assert_eq!(stack(&second), Some(StackState::Stopped));
    assert!(!kept.exists());
    std::process::Command::new("kill")
        .arg(pid)
        .status()
        .expect("kill runs");
}

#[tokio::test]
async fn no_more_stacks_of_a_project_run_than_its_config_allows() {
    let fx = fixture();
    write_config(&fx, "max_stacks = 1\n[hooks]\nstart = \"sleep 300\"\n");
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    // A second task is made all the same, and told why its stack is not running.
    let said = Mutex::new(Vec::new());
    let sink = |line: &str| said.lock().expect("lock").push(line.to_string());
    core.task_new("demo", "two", NewTaskOptions::default(), &sink)
        .await
        .expect("creates");
    let said = said.into_inner().expect("lock").join("\n");
    assert!(said.contains("max_stacks"), "{said}");
    let refused = core.stack_start("demo", "two");
    assert!(
        matches!(&refused, Err(Error::TooManyStacks { limit: 1, .. })),
        "{refused:?}"
    );
    // The one that runs can be asked for again, and makes room when it stops.
    assert!(core.stack_start("demo", "one").expect("runs already"));
    core.stack_stop("demo", "one", &hooks::discard)
        .await
        .expect("stops");
    assert!(core.stack_start("demo", "two").expect("starts"));
    assert!(matches!(
        core.stack_start("demo", "one"),
        Err(Error::TooManyStacks { .. })
    ));
    core.stacks_stop_all().await;
}

#[tokio::test]
async fn an_environments_stack_limit_counts_all_of_the_projects_stacks_for_its_own_tasks() {
    let fx = fixture();
    write_config(&fx, "max_stacks = 1\n[hooks]\nstart = \"sleep 300\"\n");
    let core = core_with_project(&fx).await;
    let roomy = Environment {
        max_stacks: Some(2),
        ..Environment::default()
    };
    core.environment_save("demo", "roomy", &roomy)
        .expect("saves");
    let in_roomy = || NewTaskOptions {
        environment: EnvironmentChoice::Named("roomy".to_string()),
        ..NewTaskOptions::default()
    };
    for name in ["one", "two", "three"] {
        core.task_new("demo", name, in_roomy(), &hooks::discard)
            .await
            .expect("creates");
    }
    assert!(core.stack_start("demo", "two").expect("runs already"));
    let third = core.stack_start("demo", "three");
    assert!(
        matches!(&third, Err(Error::TooManyStacks { limit: 2, .. })),
        "{third:?}"
    );
    // A task set up the project's own way keeps the project's limit, and counts the others.
    core.task_new("demo", "plain", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let plain = core.stack_start("demo", "plain");
    assert!(
        matches!(&plain, Err(Error::TooManyStacks { limit: 1, .. })),
        "{plain:?}"
    );
    let none = Environment {
        max_stacks: Some(0),
        ..Environment::default()
    };
    assert!(matches!(
        core.environment_save("demo", "none", &none),
        Err(Error::NoRoomForAStack)
    ));
    core.stacks_stop_all().await;
}

#[tokio::test]
async fn a_stacks_log_is_read_on_from_where_it_was_left_and_its_end_is_seen() {
    let fx = fixture();
    write_config(
        &fx,
        "[hooks]\nstart = \"echo one; sleep 0.3; echo tw\u{f6}\"\n",
    );
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    // As a front end follows it: asked whether it runs, then read on, until it has ended.
    let (mut text, mut at, mut running, mut waits) = (String::new(), 0, true, 0);
    while running && waits < 300 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        running = core.stack_is_running("demo", "one").expect("asks");
        let (new, end) = core.stack_log_from("demo", "one", at).expect("reads");
        text.push_str(&new);
        at = end;
        waits += 1;
    }
    assert_eq!(text, "one\ntw\u{f6}\n");
    assert_eq!(at, 9);
    assert!(!core.stack_is_running("demo", "one").expect("asks"));
    // From a place in it, the rest; from its end, nothing; from beyond it, a log begun anew.
    let from = |at: u64| core.stack_log_from("demo", "one", at).expect("reads");
    assert_eq!(from(4), ("tw\u{f6}\n".to_string(), 9));
    assert_eq!(from(9), (String::new(), 9));
    assert_eq!(from(500), ("one\ntw\u{f6}\n".to_string(), 9));
}

#[tokio::test]
async fn a_hook_takes_a_value_as_one_word_whatever_is_in_it() {
    let fx = fixture();
    let core = Core::open(fx.paths.clone()).expect("test setup");
    // A repository in a folder with a space in its name, and in it a branch, as somebody
    // else might have pushed it, whose name is a command to a shell.
    let repo = fx.repo.parent().expect("test setup").join("my repo");
    std::fs::create_dir(&repo).expect("test setup");
    git(&repo, &["init", "--quiet", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "spaced\n").expect("test setup");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--quiet", "-m", "init"]);
    let branch = "feature/x;touch${IFS}pwned";
    git(&repo, &["branch", branch]);
    core.project_add(&repo, Some("spaced")).await.expect("adds");
    let config = fx.paths.project_config("spaced");
    std::fs::write(
        config,
        "[hooks]\nsetup = \"cp {{repo}}/README.md copied.md && echo {{branch}} > branch.out && echo \\\"$SWARMED_BRANCH\\\" > env.out\"\n",
    )
    .expect("test setup");

    let options = NewTaskOptions {
        existing_branch: Some(branch),
        ..NewTaskOptions::default()
    };
    let task = core
        .task_new("spaced", "one", options, &hooks::discard)
        .await
        .expect("sets up");
    let read = |name: &str| std::fs::read_to_string(task.worktree_path.join(name)).expect("reads");
    assert_eq!(read("copied.md"), "spaced\n");
    assert_eq!(read("branch.out"), format!("{branch}\n"));
    assert_eq!(read("env.out"), format!("{branch}\n"));
    assert!(!task.worktree_path.join("pwned").exists());
}

#[tokio::test]
async fn a_folder_that_is_a_link_out_of_the_worktree_is_not_walked_into() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let outside = fx.repo.parent().expect("test setup").join("outside");
    std::fs::create_dir(&outside).expect("test setup");
    std::fs::write(outside.join("secret.txt"), "x").expect("test setup");
    std::os::unix::fs::symlink(&outside, task.worktree_path.join("elsewhere")).expect("links");

    let listing = core.task_files("demo", "one", "").expect("lists");
    assert!(
        listing
            .entries
            .iter()
            .any(|entry| entry.name == "elsewhere")
    );
    assert!(matches!(
        core.task_files("demo", "one", "elsewhere"),
        Err(Error::PathOutsideWorktree(_))
    ));
    assert!(matches!(
        core.task_open_file("demo", "one", "elsewhere/secret.txt"),
        Err(Error::PathOutsideWorktree(_))
    ));
}

#[tokio::test]
async fn a_snapshot_takes_any_file_name_and_environments_keep_theirs_apart() {
    let fx = fixture();
    std::fs::write(fx.repo.join(".gitignore"), "*.cache\n").expect("writes");
    git(&fx.repo, &["add", "."]);
    git(&fx.repo, &["commit", "--quiet", "-m", "ignore caches"]);
    // A name git would spell out in quotes if asked for a list by lines.
    write_config(
        &fx,
        "[hooks]\nsetup = \"test -f na\u{ef}ve.cache || echo {{task}} > na\u{ef}ve.cache\"\n",
    );
    let core = core_with_project(&fx).await;
    // Two environments, one named as the other with something after a dot.
    for name in ["staging", "staging.new"] {
        core.environment_save("demo", name, &Environment::default())
            .expect("saves");
    }
    let new = |name: &'static str, environment: &'static str| {
        let options = NewTaskOptions {
            environment: EnvironmentChoice::Named(environment.to_string()),
            ..NewTaskOptions::default()
        };
        core.task_new("demo", name, options, &hooks::discard)
    };
    new("a", "staging.new").await.expect("creates");
    let kept = core.snapshot_save("demo", "a").await.expect("saves");
    assert_eq!(kept.entries, 1);
    new("b", "staging").await.expect("creates");
    core.snapshot_save("demo", "b").await.expect("saves");

    // Saving the one did not take the other's away.
    let snapshots = core.snapshots("demo").expect("reads");
    let of: Vec<(Option<&str>, &str)> = snapshots
        .iter()
        .map(|snapshot| (snapshot.environment.as_deref(), snapshot.task.as_str()))
        .collect();
    assert_eq!(of, [(Some("staging"), "b"), (Some("staging.new"), "a")]);
    // And the file came along, under its own name.
    let c = new("c", "staging.new").await.expect("creates");
    let cache = std::fs::read_to_string(c.worktree_path.join("na\u{ef}ve.cache"));
    assert_eq!(cache.expect("seeded"), "a\n");
}

/// Whether there is a process with the id written in `pid`.
fn process_alive(pid: &str) -> bool {
    let probe = std::process::Command::new("kill")
        .args(["-0", pid.trim()])
        .output();
    probe.expect("kill runs").status.success()
}

/// Waits for `file` to be written and returns what it holds.
async fn written(file: &Path) -> String {
    for _ in 0..300 {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        if !text.trim().is_empty() {
            return text;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("{} was never written", file.display());
}

#[tokio::test]
async fn a_stack_left_behind_is_found_again_and_stopped_with_what_set_itself_apart() {
    let fx = fixture();
    // As the services of some stacks do, the child puts itself in a process group of its
    // own: `set -m` gives every background job one.
    write_config(
        &fx,
        "[hooks]\nstart = \"set -m; sleep 300 & echo $! > child.pid; wait\"\n",
    );
    let first = core_with_project(&fx).await;
    let task = first
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let child = written(&task.worktree_path.join("child.pid")).await;
    assert!(process_alive(&child));
    // That Swarmed goes away without stopping anything, as one that crashed does.
    drop(first);

    let second =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let stack = |core: &Core| core.overview().expect("reads")[0].tasks[0].stack;
    assert_eq!(stack(&second), Some(StackState::Starting));
    // It is not started a second time over itself.
    assert!(second.stack_start("demo", "one").expect("starts"));
    assert_eq!(written(&task.worktree_path.join("child.pid")).await, child);

    second
        .stack_stop("demo", "one", &hooks::discard)
        .await
        .expect("stops");
    assert_eq!(stack(&second), Some(StackState::Stopped));
    assert!(
        !process_alive(&child),
        "what set itself apart outlived the stack"
    );
}

/// Which task a snapshot was taken from, how much it kept, and for which environment.
fn taken_from(snapshot: &swarmed_core::Snapshot) -> (String, usize, Option<String>) {
    let environment = snapshot.environment.clone();
    (snapshot.task.clone(), snapshot.entries, environment)
}

#[tokio::test]
async fn the_first_task_set_up_well_is_what_the_next_ones_start_from() {
    let fx = fixture();
    std::fs::write(fx.repo.join(".gitignore"), "deps/\n").expect("writes");
    git(&fx.repo, &["add", "."]);
    git(&fx.repo, &["commit", "--quiet", "-m", "ignore deps"]);
    // Setup builds only what is not there yet, as installing dependencies does.
    write_config(
        &fx,
        "[hooks]\nsetup = \"mkdir -p deps && test -f deps/lib.txt || echo built-{{task}} > deps/lib.txt\"\n",
    );
    let core = core_with_project(&fx).await;
    let said = Mutex::new(Vec::<String>::new());
    let note = |line: &str| said.lock().expect("not poisoned").push(line.to_string());
    let heard = |what: &str| {
        let lines = std::mem::take(&mut *said.lock().expect("not poisoned"));
        lines.iter().any(|line| line.contains(what))
    };
    let new = |name: &'static str| core.task_new("demo", name, NewTaskOptions::default(), &note);
    let built = |task: &swarmed_core::Task| {
        std::fs::read_to_string(task.worktree_path.join("deps/lib.txt")).expect("built")
    };
    let kept = || {
        let snapshots = core.snapshots("demo").expect("reads");
        snapshots.iter().map(taken_from).collect::<Vec<_>>()
    };

    // Set up well with nothing to start from, the task is kept as the snapshot, and it is said.
    let one = new("one").await.expect("creates");
    assert_eq!(built(&one), "built-one\n");
    assert_eq!(kept(), [("one".to_string(), 1, None)]);
    assert!(heard("kept: new tasks of this environment start from it"));

    // The next task finds the first one's work in place, and what git checked out untouched.
    let two = new("two").await.expect("creates");
    assert!(heard("started from the snapshot of this environment"));
    // It does not take the snapshot's place: the first one set up well stays what is kept.
    assert_eq!(kept(), [("one".to_string(), 1, None)]);
    assert_eq!(built(&two), "built-one\n");
    let readme = std::fs::read_to_string(two.worktree_path.join("README.md")).expect("reads");
    assert_eq!(readme, "demo\n");
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(&two.worktree_path)
        .args(["status", "--porcelain"])
        .output()
        .expect("git runs");
    // Nothing git tracks was changed, and what was put there is all ignored.
    assert_eq!(String::from_utf8_lossy(&status.stdout), "");

    // Without the snapshot a task is set up from nothing again, and is kept in its place.
    core.snapshot_remove("demo", None).expect("removes");
    let three = new("three").await.expect("creates");
    assert_eq!(built(&three), "built-three\n");
    assert_eq!(kept(), [("three".to_string(), 1, None)]);

    // And one can be taken by hand from any task set up well, in place of the one there is.
    let replaced = core.snapshot_save("demo", "two").await.expect("saves");
    assert_eq!(replaced.task, "two");
    assert_eq!(kept(), [("two".to_string(), 1, None)]);
}

/// A repository whose setup leaves `deps/`, which git ignores, once `allow-setup` is there.
fn fixture_with_deps(setup: &str) -> common::Fixture {
    let fx = fixture();
    std::fs::write(fx.repo.join(".gitignore"), "deps/\nallow-setup\n").expect("writes");
    git(&fx.repo, &["add", "."]);
    git(&fx.repo, &["commit", "--quiet", "-m", "ignore deps"]);
    write_config(&fx, &format!("[hooks]\nsetup = \"{setup}\"\n"));
    fx
}

#[tokio::test]
async fn a_snapshot_being_taken_is_neither_taken_again_nor_forgotten() {
    let fx = fixture_with_deps("mkdir -p deps && echo built > deps/lib.txt");
    let core = core_with_project(&fx).await;
    // Held as another Swarmed building this environment's snapshot holds it.
    let folder = fx.paths.data_dir.join("snapshots/demo");
    std::fs::create_dir_all(&folder).expect("creates");
    let building = std::fs::File::create(folder.join("_lock._default")).expect("creates");
    building.try_lock().expect("free");

    // The task is set up as ever, and its snapshot is left to whoever is taking one.
    let one = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    assert_eq!(one.setup, SetupStatus::Ok);
    assert_eq!(core.snapshots("demo").expect("reads"), []);
    // By hand it is refused, and so is forgetting what is about to be there.
    let saved = core.snapshot_save("demo", "one").await;
    assert!(matches!(saved, Err(Error::Snapshot(_))));
    let forgotten = core.snapshot_remove("demo", None);
    assert!(matches!(forgotten, Err(Error::Snapshot(_))));

    // Let go as a build lets it go, it can be taken and forgotten.
    building.unlock().expect("unlocks");
    core.snapshot_save("demo", "one").await.expect("saves");
    core.snapshot_remove("demo", None).expect("forgets");
    assert_eq!(core.snapshots("demo").expect("reads"), []);
}

#[tokio::test]
async fn a_snapshot_that_cannot_be_kept_costs_the_task_nothing() {
    // A file nobody may read cannot be copied.
    let fx = fixture_with_deps("mkdir -p deps && echo x > deps/lib.txt && chmod 000 deps/lib.txt");
    let core = core_with_project(&fx).await;
    let said = Mutex::new(Vec::<String>::new());
    let note = |line: &str| said.lock().expect("not poisoned").push(line.to_string());

    let one = core
        .task_new("demo", "one", NewTaskOptions::default(), &note)
        .await
        .expect("creates");
    assert_eq!(one.setup, SetupStatus::Ok);
    let said = said.lock().expect("not poisoned");
    assert!(
        said.iter()
            .any(|line| line.contains("the snapshot could not be kept"))
    );
    assert_eq!(core.snapshots("demo").expect("reads"), []);
}

#[tokio::test]
async fn setting_up_again_keeps_a_task_only_if_it_was_never_set_up_well() {
    let fx = fixture_with_deps("test -f allow-setup && mkdir -p deps && echo built > deps/lib.txt");
    let core = core_with_project(&fx).await;
    let kept = || core.snapshots("demo").expect("reads").len();
    let again = || core.task_setup("demo", "one", None, &hooks::discard);

    let failed = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await;
    assert!(failed.is_err());
    assert_eq!(kept(), 0);
    // Set up well for the first time, it is as good as a new task that was.
    let (_, task) = core.task("demo", "one").expect("there");
    std::fs::write(task.worktree_path.join("allow-setup"), "").expect("writes");
    again().await.expect("sets up");
    assert_eq!(kept(), 1);

    // Set up again later, it has been lived in: what its stack and agents left is not kept.
    core.snapshot_remove("demo", None).expect("forgets");
    again().await.expect("sets up");
    assert_eq!(kept(), 0);
}

#[tokio::test]
async fn a_snapshot_is_put_in_place_part_by_part_saying_how_far_it_is() {
    let fx = fixture();
    // A folder git tracks, with one it ignores inside it, as in a monorepo's apps.
    std::fs::create_dir_all(fx.repo.join("apps/web")).expect("test setup");
    std::fs::write(fx.repo.join("apps/web/index.js"), "app\n").expect("writes");
    std::fs::write(fx.repo.join(".gitignore"), "node_modules/\n.env\nlinked\n").expect("writes");
    git(&fx.repo, &["add", "."]);
    git(&fx.repo, &["commit", "--quiet", "-m", "an app"]);
    write_config(
        &fx,
        "[hooks]\nsetup = \"test -f .env || { mkdir -p node_modules/acme apps/web/node_modules/ui && echo lib > node_modules/acme/lib.js && echo ui > apps/web/node_modules/ui/ui.js && ln -s node_modules/acme linked && echo KEY={{task}} > .env; }\"\n",
    );
    let core = core_with_project(&fx).await;
    let said = Mutex::new(Vec::<String>::new());
    let note = |line: &str| said.lock().expect("not poisoned").push(line.to_string());
    let new = |name: &'static str| core.task_new("demo", name, NewTaskOptions::default(), &note);
    new("one").await.expect("creates");

    // A file git has come to track since is what the next task gets, not the snapshot's.
    std::fs::write(fx.repo.join(".env"), "KEY=tracked\n").expect("writes");
    git(&fx.repo, &["add", "--force", ".env"]);
    git(&fx.repo, &["commit", "--quiet", "-m", "track .env"]);
    said.lock().expect("not poisoned").clear();
    let two = new("two").await.expect("creates");

    // Each missing folder is put there whole, and each step is said as it begins.
    let lines = said.lock().expect("not poisoned").clone();
    let step = |n: usize, path: &str| {
        let line = format!("starting from the snapshot of this environment: {n} of 3, {path}");
        lines.contains(&line)
    };
    assert!(step(1, "apps/web/node_modules"), "{lines:?}");
    assert!(step(2, "linked"), "{lines:?}");
    assert!(step(3, "node_modules"), "{lines:?}");
    let done = "started from the snapshot of this environment (copied in ";
    assert!(lines.iter().any(|line| line.starts_with(done)), "{lines:?}");
    let read = |path: &str| std::fs::read_to_string(two.worktree_path.join(path)).expect("there");
    assert_eq!(read("node_modules/acme/lib.js"), "lib\n");
    assert_eq!(read("apps/web/node_modules/ui/ui.js"), "ui\n");
    assert_eq!(read("apps/web/index.js"), "app\n");
    assert_eq!(read(".env"), "KEY=tracked\n");
    // A link cannot be cloned whole: it is copied, into the snapshot and out of it, as a link.
    let link = std::fs::read_link(two.worktree_path.join("linked")).expect("a link");
    assert_eq!(link, Path::new("node_modules/acme"));
}

#[tokio::test]
async fn a_snapshot_copies_once_what_a_rule_for_a_folders_contents_ignores() {
    let fx = fixture();
    // As Composer's folder is often ignored: what is in it, not the folder. Git then names the
    // folder and each thing in it.
    std::fs::write(fx.repo.join(".gitignore"), "/vendor/*\n").expect("writes");
    git(&fx.repo, &["add", "."]);
    git(&fx.repo, &["commit", "--quiet", "-m", "ignore vendor"]);
    // Copied a second time, into the first copy, a package lands inside itself; one holding a
    // file of its own name, as phpstan/phpstan holds `phpstan`, meets that file and fails.
    write_config(
        &fx,
        "[hooks]\nsetup = \"test -f vendor/autoload.php || { mkdir -p vendor/able vendor/acme/acme && echo lib > vendor/able/lib && echo tool > vendor/acme/acme/acme && echo {{task}} > vendor/autoload.php; }\"\n",
    );
    let core = core_with_project(&fx).await;
    let new = |name: &'static str| {
        core.task_new("demo", name, NewTaskOptions::default(), &hooks::discard)
    };
    new("one").await.expect("creates");
    let snapshot = core.snapshot_save("demo", "one").await.expect("saves");
    assert_eq!(snapshot.entries, 1);

    // The next task starts from it, with each file once and where it was.
    let two = new("two").await.expect("creates");
    let read = |path: &str| std::fs::read_to_string(two.worktree_path.join(path)).expect("seeded");
    assert_eq!(read("vendor/autoload.php"), "one\n");
    assert_eq!(read("vendor/acme/acme/acme"), "tool\n");
    assert_eq!(read("vendor/able/lib"), "lib\n");
    assert!(!two.worktree_path.join("vendor/able/able").exists());
}

#[tokio::test]
async fn a_task_whose_setup_failed_or_left_nothing_cannot_be_kept_as_a_snapshot() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    core.task_new("demo", "plain", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let nothing = core.snapshot_save("demo", "plain").await;
    assert!(matches!(nothing, Err(Error::Snapshot(_))), "{nothing:?}");

    write_config(&fx, "[hooks]\nsetup = \"exit 3\"\n");
    let broken = core
        .task_new("demo", "broken", NewTaskOptions::default(), &hooks::discard)
        .await;
    assert!(broken.is_err());
    let refused = core.snapshot_save("demo", "broken").await;
    assert!(matches!(refused, Err(Error::Snapshot(_))), "{refused:?}");
}

/// The window's own commands run on a thread with no Tokio runtime. Starting a stack from one
/// used to bring the whole app down.
#[test]
fn a_stack_can_be_started_from_a_thread_that_has_no_runtime() {
    let fx = fixture();
    write_config(&fx, "[hooks]\nstart = \"sleep 300\"\n");
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let core = runtime.block_on(async {
        let core = core_with_project(&fx).await;
        core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
            .await
            .expect("creates");
        core.stack_stop("demo", "one", &hooks::discard)
            .await
            .expect("stops");
        core
    });
    assert!(core.stack_start("demo", "one").expect("starts"));
    assert_eq!(
        core.overview().expect("reads")[0].tasks[0].stack,
        Some(StackState::Starting)
    );
    runtime.block_on(core.stacks_stop_all());
}

#[tokio::test]
async fn a_stack_is_starting_until_the_address_it_printed_answers() {
    let fx = fixture();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let port = listener.local_addr().expect("an address").port();
    write_config(
        &fx,
        &format!("[hooks]\nstart = \"echo up at http://localhost:{port}/; sleep 300\"\n"),
    );
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let task = |core: &Core| core.overview().expect("reads")[0].tasks[0].clone();
    let mut waits = 0;
    while task(&core).stack != Some(StackState::Running) && waits < 200 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        waits += 1;
    }
    let url = format!("http://localhost:{port}/");
    assert_eq!(task(&core).stack, Some(StackState::Running));
    assert_eq!(task(&core).urls, std::slice::from_ref(&url));
    assert_eq!(core.task_url("demo", "one").expect("reads"), Some(url));

    core.stacks_stop_all().await;
    drop(listener);
    assert_eq!(task(&core).stack, Some(StackState::Stopped));
}

#[tokio::test]
async fn a_task_without_a_start_command_has_no_stack() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    assert_eq!(core.overview().expect("reads")[0].tasks[0].stack, None);
    assert!(!core.stack_start("demo", "one").expect("nothing to start"));
    assert_eq!(core.stack_log("demo", "one").expect("reads"), "");
}

#[tokio::test]
async fn task_new_can_check_out_an_existing_branch() {
    let fx = fixture();
    git(&fx.repo, &["branch", "feature/existing"]);
    let core = core_with_project(&fx).await;
    let options = NewTaskOptions {
        existing_branch: Some("feature/existing"),
        ..NewTaskOptions::default()
    };
    let task = core
        .task_new("demo", "existing", options, &hooks::discard)
        .await
        .expect("test setup");
    assert_eq!(task.branch, "feature/existing");
    assert!(task.worktree_path.join("README.md").exists());
}

#[tokio::test]
async fn invalid_names_are_rejected() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    for name in ["", "Has Space", "../up", "UPPER", "-lead", "a/b"] {
        let result = core
            .task_new("demo", name, NewTaskOptions::default(), &hooks::discard)
            .await;
        assert!(
            matches!(result, Err(Error::InvalidName(_))),
            "{name:?} should be rejected"
        );
    }
}

#[tokio::test]
async fn with_a_remote_tasks_branch_from_it_and_pushed_work_can_be_removed() {
    let fx = fixture();
    let root = fx.repo.parent().expect("test setup");
    let remote = root.join("remote.git");
    git(
        root,
        &["init", "--quiet", "--bare", "-b", "main", "remote.git"],
    );
    git(
        &fx.repo,
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    git(&fx.repo, &["push", "--quiet", "-u", "origin", "main"]);
    // A commit that exists only on the remote: the task must start from it, not from local main.
    let other = root.join("other");
    git(
        root,
        &["clone", "--quiet", &remote.to_string_lossy(), "other"],
    );
    std::fs::write(other.join("remote-only.txt"), "x").expect("test setup");
    git(&other, &["add", "."]);
    git(&other, &["commit", "--quiet", "-m", "remote work"]);
    git(&other, &["push", "--quiet"]);

    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    assert_eq!(task.base_ref, "origin/main");
    assert!(task.worktree_path.join("remote-only.txt").exists());

    std::fs::write(task.worktree_path.join("work.txt"), "x").expect("test setup");
    git(&task.worktree_path, &["add", "."]);
    git(&task.worktree_path, &["commit", "--quiet", "-m", "work"]);
    let unpushed = core
        .task_remove("demo", "one", RemoveOptions::default(), &hooks::discard)
        .await;
    assert!(matches!(unpushed, Err(Error::TaskNotClean { .. })));

    git(
        &task.worktree_path,
        &["push", "--quiet", "-u", "origin", "HEAD"],
    );
    core.task_remove("demo", "one", RemoveOptions::default(), &hooks::discard)
        .await
        .expect("pushed work can be removed");
    assert!(!task.worktree_path.exists());
}

#[tokio::test]
async fn branch_choices_offer_the_base_and_branches_no_worktree_holds() {
    let fx = fixture();
    git(&fx.repo, &["branch", "feature/a"]);
    git(&fx.repo, &["branch", "feature/b"]);
    let core = core_with_project(&fx).await;

    let choices = core.branch_choices("demo").await.expect("reads");
    assert_eq!(choices.base, "main");
    // `main` is checked out in the repository itself, so a task cannot take it.
    assert_eq!(choices.existing, ["feature/a", "feature/b"]);

    let options = NewTaskOptions {
        existing_branch: Some("feature/a"),
        ..NewTaskOptions::default()
    };
    core.task_new("demo", "a", options, &hooks::discard)
        .await
        .expect("creates");
    core.task_new("demo", "fresh", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let choices = core.branch_choices("demo").await.expect("reads");
    assert_eq!(choices.existing, ["feature/b"]);
}

#[tokio::test]
async fn a_task_knows_where_its_stack_is_reached() {
    let fx = fixture();
    write_config(
        &fx,
        "open_url = \"http://localhost:{{port.web}}/{{task}}\"\n[ports]\nweb = 3000\n",
    );
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");

    let url = Some("http://localhost:3100/one".to_string());
    assert_eq!(core.task_url("demo", "one").expect("renders"), url);
    assert_eq!(core.overview().expect("reads")[0].tasks[0].url, url);

    write_config(&fx, "");
    assert_eq!(core.task_url("demo", "one").expect("reads"), None);
    assert!(matches!(
        core.task_open_url("demo", "one"),
        Err(Error::NoOpenUrl(project)) if project == "demo"
    ));
}

#[tokio::test]
async fn the_editor_is_started_with_the_worktree() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");

    // An "editor" that writes down what it was asked to open.
    let opened = fx.paths.data_dir.join("opened");
    let script = fx.paths.data_dir.join("editor.sh");
    let body = format!("#!/bin/sh\necho \"$1\" > '{}'\n", opened.display());
    common::executable(&script, &body);
    let editor = script.to_string_lossy().into_owned();
    let settings = Settings {
        editor,
        ..Settings::default()
    };
    core.set_settings(&settings).expect("saves");

    core.task_open_editor("demo", "one").expect("starts");
    let written = wait_for_file(&opened).await;
    assert_eq!(written.trim(), task.worktree_path.to_string_lossy());
}

/// The file's contents once the launched program has written it.
async fn wait_for_file(path: &Path) -> String {
    for _ in 0..200 {
        match std::fs::read_to_string(path) {
            Ok(text) if !text.is_empty() => return text,
            _ => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
        }
    }
    panic!("{} was never written", path.display());
}

#[tokio::test]
async fn a_tasks_changes_cover_commits_edits_and_new_files() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let worktree = &task.worktree_path;
    assert_eq!(core.task_changes("demo", "one").await.expect("reads"), []);

    // One committed change, one uncommitted edit of the same file, and one new file.
    std::fs::write(worktree.join("README.md"), "demo\ncommitted\n").expect("writes");
    git(worktree, &["commit", "--quiet", "-am", "more"]);
    std::fs::write(worktree.join("README.md"), "demo\ncommitted\nedited\n").expect("writes");
    std::fs::create_dir(worktree.join("src")).expect("mkdir");
    std::fs::write(worktree.join("src/new.txt"), "one\ntwo\n").expect("writes");

    let changes = core.task_changes("demo", "one").await.expect("reads");
    let seen: Vec<_> = changes
        .iter()
        .map(|c| (c.path.as_str(), c.kind, c.added, c.removed))
        .collect();
    assert_eq!(
        seen,
        [
            ("README.md", ChangeKind::Modified, Some(2), Some(0)),
            ("src/new.txt", ChangeKind::Added, Some(2), Some(0)),
        ]
    );

    let readme = core
        .task_diff("demo", "one", "README.md")
        .await
        .expect("diffs");
    assert!(
        readme.text.contains("+committed\n+edited"),
        "{}",
        readme.text
    );
    let new = core
        .task_diff("demo", "one", "src/new.txt")
        .await
        .expect("diffs");
    assert!(
        new.text.ends_with("@@ -0,0 +1,2 @@\n+one\n+two\n"),
        "{}",
        new.text
    );
    assert!(matches!(
        core.task_diff("demo", "one", "../demo/README.md").await,
        Err(Error::PathOutsideWorktree(_))
    ));
}

#[tokio::test]
async fn notes_are_kept_per_project_and_per_task_for_the_user_alone() {
    use std::os::unix::fs::PermissionsExt;

    let fx = fixture();
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");

    assert_eq!(core.note("demo", None).expect("reads"), "");
    core.set_note("demo", None, "staging: https://example.test\n")
        .expect("writes");
    core.set_note("demo", Some("one"), "password: hunter2")
        .expect("writes");
    assert_eq!(
        core.note("demo", None).expect("reads"),
        "staging: https://example.test\n"
    );
    assert_eq!(
        core.note("demo", Some("one")).expect("reads"),
        "password: hunter2"
    );

    // A plain text file nobody else on the machine can read.
    let file = fx.paths.note("demo", Some("one"));
    assert_eq!(file.extension().and_then(|e| e.to_str()), Some("txt"));
    let mode = std::fs::metadata(&file)
        .expect("exists")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);

    let overview = core.overview().expect("reads");
    assert!(overview[0].has_note);
    assert!(overview[0].tasks[0].has_note);

    // Blank space is no note.
    core.set_note("demo", None, "  \n").expect("clears");
    assert!(!fx.paths.note("demo", None).exists());
    assert!(!core.overview().expect("reads")[0].has_note);

    assert!(matches!(
        core.set_note("demo", Some("one"), &"x".repeat(200_001)),
        Err(Error::NoteTooLong(_))
    ));
    assert!(matches!(
        core.note("demo", Some("two")),
        Err(Error::TaskNotFound { .. })
    ));
    assert!(matches!(
        core.set_note("nowhere", None, "x"),
        Err(Error::ProjectNotFound(_))
    ));
}

#[tokio::test]
async fn a_removed_task_takes_its_note_along() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core.set_note("demo", Some("one"), "token: abc")
        .expect("writes");
    let options = RemoveOptions {
        force: true,
        delete_branch: true,
    };
    core.task_remove("demo", "one", options, &hooks::discard)
        .await
        .expect("removes");
    assert!(!fx.paths.note("demo", Some("one")).exists());

    // A new task of the same name starts with an empty note.
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    assert_eq!(core.note("demo", Some("one")).expect("reads"), "");
}

#[tokio::test]
async fn a_removed_project_leaves_swarmed_and_its_repository_stays() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    core.set_note("demo", None, "for the repository")
        .expect("writes");
    core.set_note("demo", Some("one"), "for the task")
        .expect("writes");
    // What else is kept under the project's name: a snapshot, environments, its own actions.
    let snapshot = fx.paths.data_dir.join("snapshots/demo");
    std::fs::create_dir_all(snapshot.join("_default/files")).expect("test setup");
    std::fs::write(snapshot.join("_default/files/.env"), "KEY=secret").expect("test setup");
    let staging = Environment {
        env: [("KEY".to_string(), "secret".to_string())].into(),
        ..Environment::default()
    };
    core.environment_save("demo", "staging", &staging)
        .expect("saves");
    core.action_save(Some("demo"), &action("Tests", "", "make test"))
        .expect("saves");
    assert!(fx.paths.project_environments("demo").exists());
    assert!(fx.paths.project_actions("demo").exists());

    // Unsaved work in a task stops it, and nothing about the project is gone.
    std::fs::write(task.worktree_path.join("new.txt"), "x").expect("test setup");
    let refused = core.project_remove("demo", false, &hooks::discard).await;
    assert!(matches!(refused, Err(Error::TaskNotClean { .. })));
    assert_eq!(core.projects().expect("reads").len(), 1);
    assert_eq!(
        core.note("demo", None).expect("reads"),
        "for the repository"
    );

    core.project_remove("demo", true, &hooks::discard)
        .await
        .expect("removes");
    assert_eq!(core.projects().expect("reads"), vec![]);
    assert_eq!(core.overview().expect("reads"), vec![]);
    assert!(!task.worktree_path.exists());
    assert!(!fx.paths.notes("demo").exists());
    // Nothing is left that another repository of this name would be started from.
    assert!(!snapshot.exists());
    assert!(!fx.paths.project_environments("demo").exists());
    assert!(!fx.paths.project_actions("demo").exists());
    assert!(fx.paths.project_config("demo").exists());
    // The repository is as it was, the task's branch included.
    assert!(fx.repo.join("README.md").exists());
    git(
        &fx.repo,
        &["rev-parse", "--verify", "--quiet", &task.branch],
    );
    assert!(matches!(
        core.project_remove("demo", false, &hooks::discard).await,
        Err(Error::ProjectNotFound(_))
    ));

    // It can be added again, and starts empty.
    core.project_add(&fx.repo, None).await.expect("adds again");
    assert_eq!(core.tasks("demo", true).expect("reads"), vec![]);
    assert_eq!(core.snapshots("demo").expect("reads"), vec![]);
    assert!(core.environments("demo").expect("reads").is_empty());
    assert_eq!(core.actions(Some("demo")).expect("reads"), vec![]);

    // Not under a name that is another project's file, though.
    for name in ["demo.actions", "demo.environments"] {
        let refused = core.project_add(&fx.repo, Some(name)).await;
        assert!(
            matches!(refused, Err(Error::ReservedName(_))),
            "{refused:?}"
        );
    }
    assert!(!fx.paths.project_config("demo.actions").exists());
}

#[tokio::test]
async fn a_tasks_work_is_committed_and_pushed_when_asked() {
    let fx = fixture();
    let root = fx.repo.parent().expect("test setup");
    let remote = root.join("remote.git");
    git(
        root,
        &["init", "--quiet", "--bare", "-b", "main", "remote.git"],
    );
    git(
        &fx.repo,
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    git(&fx.repo, &["push", "--quiet", "-u", "origin", "main"]);
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");

    let clean = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!(
        (clean.changed, clean.unpushed, clean.pushed_before),
        (0, 0, false)
    );
    let nothing = core.task_commit("demo", "one", "feat: nothing").await;
    assert_eq!(
        nothing.expect_err("refused").to_string(),
        "there is nothing to commit"
    );

    std::fs::write(task.worktree_path.join("a.txt"), "a").expect("test setup");
    std::fs::write(task.worktree_path.join("b.txt"), "b").expect("test setup");
    let dirty = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!((dirty.changed, dirty.unpushed), (2, 0));
    let wordless = core.task_commit("demo", "one", "  ").await;
    assert_eq!(
        wordless.expect_err("refused").to_string(),
        "a commit needs a message"
    );

    core.task_commit("demo", "one", "feat: two files\n\nBoth are new.")
        .await
        .expect("commits");
    let committed = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!((committed.changed, committed.unpushed), (0, 1));
    let draft = core.task_pull_draft("demo", "one").await.expect("drafts");
    assert_eq!(draft.title, "feat: two files");
    assert_eq!(draft.body, "Both are new.");

    core.task_push("demo", "one").await.expect("pushes");
    let pushed = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!((pushed.unpushed, pushed.pushed_before), (0, true));
}

/// A worktree something switched to a second branch: the task remembers both, what is counted
/// and pushed is the branch at hand, and the worktree can be put on either.
#[tokio::test]
async fn a_worktree_on_another_branch_is_followed_and_can_be_switched() {
    let fx = fixture();
    let root = fx.repo.parent().expect("test setup");
    let remote = root.join("remote.git");
    git(
        root,
        &["init", "--quiet", "--bare", "-b", "main", "remote.git"],
    );
    git(
        &fx.repo,
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    git(&fx.repo, &["push", "--quiet", "-u", "origin", "main"]);
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let worktree = &task.worktree_path;
    let own = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!(own.branch.as_deref(), Some(task.branch.as_str()));
    assert_eq!(own.branches, std::slice::from_ref(&task.branch));
    // A branch the worktree was never on is not one to switch to.
    let stranger = core.task_switch("demo", "one", "main").await;
    assert_eq!(
        stranger.expect_err("refused").to_string(),
        "`main` is not a branch this task's worktree has been on"
    );

    git(worktree, &["switch", "--quiet", "-c", "second"]);
    std::fs::write(worktree.join("a.txt"), "a").expect("test setup");
    core.task_commit("demo", "one", "feat: on the second branch")
        .await
        .expect("commits");
    let switched = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!(switched.branch.as_deref(), Some("second"));
    assert_eq!(switched.unpushed, 1);
    let both = [task.branch.clone(), "second".to_string()];
    assert_eq!(switched.branches, both);
    // The second branch's commit is nowhere else yet, so the task is not removed over it.
    let options = RemoveOptions::default();
    let removed = core
        .task_remove("demo", "one", options, &hooks::discard)
        .await;
    assert!(matches!(removed, Err(Error::TaskNotClean { .. })));

    // Pushing sends the branch at hand, and never the task's own in its place.
    core.task_push("demo", "one").await.expect("pushes");
    let there = std::process::Command::new("git")
        .arg("-C")
        .arg(&remote)
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .output()
        .expect("test setup");
    assert_eq!(
        String::from_utf8_lossy(&there.stdout).trim(),
        "main\nsecond"
    );
    let pushed = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!((pushed.unpushed, pushed.pushed_before), (0, true));

    // On no branch at all there is nothing to push, and the branches are still known.
    git(worktree, &["switch", "--quiet", "--detach"]);
    let detached = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!((detached.branch, detached.rebasing), (None, false));
    assert_eq!(detached.branches, both);
    let refused = core.task_push("demo", "one").await.expect_err("refused");
    assert_eq!(
        refused.to_string(),
        "this worktree is on no branch: switch it to one first"
    );

    // Back on its own branch, with a change that is not committed carried across.
    std::fs::write(worktree.join("b.txt"), "b").expect("test setup");
    core.task_switch("demo", "one", &task.branch)
        .await
        .expect("switches");
    let back = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!(back.branch.as_deref(), Some(task.branch.as_str()));
    assert_eq!((back.changed, back.unpushed), (1, 0));
    assert!(!worktree.join("a.txt").exists());

    // A change that would be overwritten stops the switch, with git's reason.
    core.task_switch("demo", "one", "second")
        .await
        .expect("switches");
    std::fs::write(worktree.join("a.txt"), "changed").expect("test setup");
    let stuck = core.task_switch("demo", "one", &task.branch).await;
    let said = stuck.expect_err("refused").to_string();
    assert!(said.contains("a.txt"), "{said}");
    let still = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!(still.branch.as_deref(), Some("second"));

    // A branch deleted since is no longer one of the task's.
    git(worktree, &["checkout", "--quiet", "--", "a.txt"]);
    git(worktree, &["clean", "--quiet", "-f"]);
    git(worktree, &["switch", "--quiet", &task.branch]);
    git(worktree, &["branch", "--quiet", "-D", "second"]);
    let gone = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!(gone.branches, std::slice::from_ref(&task.branch));
}

/// Starts rebasing `worktree` with `args`, and expects it to stop on a conflict.
fn rebase_stops(worktree: &Path, args: &[&str]) {
    let ran = std::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .arg("rebase")
        .args(args)
        .output()
        .expect("test setup");
    assert!(!ran.status.success(), "the rebase stops on the conflict");
}

/// A rebase that stopped on a conflict leaves the worktree on no branch: it is said to be a
/// rebase, under either of git's ways of rebasing, and nothing is pushed or switched until it
/// is over.
#[tokio::test]
async fn a_rebase_under_way_is_named_and_waited_for() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let worktree = &task.worktree_path;
    let before = core.task_unpublished("demo", "one").await.expect("reads");
    assert!(!before.rebasing);
    // The task and its base change the same line, so rebasing the task stops there.
    std::fs::write(worktree.join("README.md"), "the task's\n").expect("test setup");
    git(worktree, &["commit", "--quiet", "-am", "the task's"]);
    std::fs::write(fx.repo.join("README.md"), "the base's\n").expect("test setup");
    git(&fx.repo, &["commit", "--quiet", "-am", "the base's"]);

    let why = "a rebase is under way in this worktree: finish it or abort it first";
    for way in ["--merge", "--apply"] {
        rebase_stops(worktree, &["--quiet", way, &task.base_ref]);
        let rebasing = core.task_unpublished("demo", "one").await.expect("reads");
        assert_eq!((rebasing.branch, rebasing.rebasing), (None, true), "{way}");
        let pushed = core.task_push("demo", "one").await;
        assert_eq!(pushed.expect_err("refused").to_string(), why, "{way}");
        let back = core.task_switch("demo", "one", &task.branch).await;
        assert_eq!(back.expect_err("refused").to_string(), why, "{way}");
        git(worktree, &["rebase", "--abort"]);
    }
    let after = core.task_unpublished("demo", "one").await.expect("reads");
    assert_eq!(after.branch.as_deref(), Some(task.branch.as_str()));
    assert!(!after.rebasing);
}

fn action(name: &str, prompt: &str, command: &str) -> Action {
    Action {
        name: name.to_string(),
        prompt: prompt.to_string(),
        command: command.to_string(),
        ..Action::default()
    }
}

#[tokio::test]
async fn actions_are_kept_for_every_repository_and_for_one() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    assert_eq!(core.actions(Some("demo")).expect("reads"), []);

    core.action_save(None, &action(" Review ", "Review the changes.", ""))
        .expect("keeps");
    core.action_save(None, &action("Tests", "", "cargo test"))
        .expect("keeps");
    let own = Action {
        icon: "❄️".to_string(),
        ..action("Tests", "", "nix develop -c make test")
    };
    let offered = core.action_save(Some("demo"), &own).expect("keeps");
    // The general ones first, and the repository's own in place of the one it shares a name with.
    let seen: Vec<_> = offered
        .iter()
        .map(|action| {
            (
                action.name.as_str(),
                action.command.as_str(),
                action.of_project,
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            ("Review", "", false),
            ("Tests", "nix develop -c make test", true)
        ]
    );
    assert_eq!(offered[1].icon, "❄️");
    assert_eq!(core.actions(None).expect("reads").len(), 2);

    // Saved again under the same name it is replaced, not added.
    let replaced = core
        .action_save(None, &action("Review", "Review it again.", ""))
        .expect("replaces");
    assert_eq!(replaced.len(), 2);
    assert_eq!(replaced[0].prompt, "Review it again.");

    for unusable in [
        action("", "x", ""),
        action("Both", "a prompt", "and a command"),
        action("Neither", "", "  "),
        action(&"n".repeat(41), "x", ""),
    ] {
        assert!(matches!(
            core.action_save(None, &unusable),
            Err(Error::Action(_))
        ));
    }
    assert!(
        core.action_save(Some("nowhere"), &action("A", "x", ""))
            .is_err()
    );

    let left = core.action_remove(Some("demo"), "Tests").expect("drops");
    assert_eq!(left[1].command, "cargo test");
    assert_eq!(core.action_remove(None, "Tests").expect("drops").len(), 1);
    assert_eq!(
        core.action_remove(None, "Tests").expect("no error").len(),
        1
    );
}

#[tokio::test]
async fn an_actions_file_written_by_hand_is_read_and_a_slip_in_it_costs_one_button() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let file = fx.paths.data_dir.join("actions.toml");
    std::fs::write(
        &file,
        "[[action]]\nname = \"Call home\"\nicon = \"📞\"\ncommand = \"./call.sh\"\n\n\
         [[action]]\nname = \"Nothing to do\"\n",
    )
    .expect("test setup");
    let offered = core.actions(Some("demo")).expect("reads");
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].name, "Call home");

    // A misspelt key costs the button it is in, and no other.
    std::fs::write(
        &file,
        "[[action]]\nnmae = \"typo\"\nprompt = \"review this\"\n\n\
         [[action]]\nname = \"Call home\"\ncommand = \"./call.sh\"\n",
    )
    .expect("test setup");
    let offered = core.actions(None).expect("reads");
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].name, "Call home");
    // What could not be read is still in the file after Swarmed wrote it anew.
    core.action_save(None, &action("Tests", "", "make test"))
        .expect("saves");
    core.action_move(None, "Tests", -1).expect("moves");
    core.action_remove(None, "Call home").expect("removes");
    let text = std::fs::read_to_string(&file).expect("reads");
    assert!(text.contains("= \"typo\""), "{text}");
    assert!(text.contains("review this"), "{text}");
    assert_eq!(core.actions(None).expect("reads").len(), 1);

    // A file that is not TOML at all is said, not passed over.
    std::fs::write(&file, "[[action]\nname = ").expect("test setup");
    assert!(matches!(core.actions(None), Err(Error::Config { .. })));
}

#[tokio::test]
async fn prompts_are_kept_by_name_replaced_and_dropped() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    assert_eq!(core.templates().expect("reads"), []);

    core.template_save("review", "Review {{file}} for bugs.")
        .expect("keeps");
    core.template_save("explain", "  Explain this.  ")
        .expect("keeps");
    let kept = core.templates().expect("reads");
    let names: Vec<_> = kept.iter().map(|template| template.name.as_str()).collect();
    assert_eq!(names, ["explain", "review"]);
    assert_eq!(kept[0].text, "Explain this.");

    let replaced = core
        .template_save("review", "Review it.\nSay \"done\".")
        .expect("replaces");
    assert_eq!(replaced.len(), 2);
    assert_eq!(replaced[1].text, "Review it.\nSay \"done\".");

    assert!(core.template_save("Not A Name", "x").is_err());
    let empty = core.template_save("empty", "   ").expect_err("refused");
    assert_eq!(empty.to_string(), "a prompt needs some text");

    assert_eq!(core.template_remove("review").expect("drops").len(), 1);
    assert_eq!(core.template_remove("review").expect("no error").len(), 1);
}

#[tokio::test]
async fn tasks_named_alike_at_the_same_time_each_get_a_name_of_their_own() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let free = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    // Five sessions started at once with `/code-review 123` are all named alike.
    let new = || core.task_new("demo", "code-review-pr-123", free.clone(), &hooks::discard);
    let made = tokio::join!(new(), new(), new(), new(), new());
    let made = [made.0, made.1, made.2, made.3, made.4];
    let mut names: Vec<String> = made
        .into_iter()
        .map(|task| task.expect("each is created").name)
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "code-review-pr-123",
            "code-review-pr-123-2",
            "code-review-pr-123-3",
            "code-review-pr-123-4",
            "code-review-pr-123-5",
        ]
    );
}

/// Sets the branch prefix as the settings window does, with the other settings as they are.
fn set_prefix(core: &Core, prefix: &str) -> swarmed_core::Result<()> {
    let settings = Settings {
        branch_prefix: prefix.to_string(),
        ..core.settings()?
    };
    core.set_settings(&settings)
}

/// The user's login, which branches start with when there is no prefix.
fn login() -> String {
    let example = swarmed_core::branch_example("").expect("the login makes a branch");
    let task = format!("/{}", swarmed_core::EXAMPLE_TASK);
    example
        .strip_suffix(&task)
        .expect("ends in the task")
        .to_string()
}

#[tokio::test]
async fn a_new_tasks_branch_starts_with_the_branch_prefix_or_else_the_login() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let new = |name: &'static str| {
        core.task_new("demo", name, NewTaskOptions::default(), &hooks::discard)
    };
    // Empty, as before there was a prefix: the login.
    let task = new("one").await.expect("creates");
    assert_eq!(task.branch, format!("{}/one", login()));

    // Typed with slashes at its ends, it is kept without them, and those inside stay.
    set_prefix(&core, " /jd/feat/ ").expect("saves");
    assert_eq!(core.settings().expect("reads").branch_prefix, "jd/feat");
    assert_eq!(new("two").await.expect("creates").branch, "jd/feat/two");

    // A made-up name steps past a branch that is taken, after the prefix; a name the user
    // chose is refused, with the branch named.
    set_prefix(&core, "jd").expect("saves");
    git(&fx.repo, &["branch", "jd/fix"]);
    let free = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    let stepped = core
        .task_new("demo", "fix", free, &hooks::discard)
        .await
        .expect("creates");
    assert_eq!(
        (stepped.name.as_str(), stepped.branch.as_str()),
        ("fix-2", "jd/fix-2")
    );
    assert!(matches!(new("fix").await, Err(Error::BranchExists(branch)) if branch == "jd/fix"));

    // A branch the user picks is used as it is.
    let picked = NewTaskOptions {
        existing_branch: Some("jd/fix"),
        ..NewTaskOptions::default()
    };
    let task = core
        .task_new("demo", "picked", picked, &hooks::discard)
        .await
        .expect("creates");
    assert_eq!(task.branch, "jd/fix");
}

#[tokio::test]
async fn a_repository_keeps_its_own_pattern_and_the_earlier_default_takes_the_prefix() {
    let fx = fixture();
    // What the template wrote into every config before there was a prefix.
    write_config(&fx, "branch_pattern = \"{{user}}/{{task}}\"\n");
    let core = core_with_project(&fx).await;
    set_prefix(&core, "team-a").expect("saves");
    let new = |name: &'static str| {
        core.task_new("demo", name, NewTaskOptions::default(), &hooks::discard)
    };
    assert_eq!(new("one").await.expect("creates").branch, "team-a/one");

    write_config(&fx, "branch_pattern = \"swarm/{{task}}\"\n");
    assert_eq!(new("two").await.expect("creates").branch, "swarm/two");

    // One of the user's own can use the prefix too.
    write_config(&fx, "branch_pattern = \"{{prefix}}-{{user}}/{{task}}\"\n");
    let task = new("three").await.expect("creates");
    assert_eq!(task.branch, format!("team-a-{}/three", login()));
}

#[tokio::test]
async fn a_branch_prefix_git_would_not_take_is_refused_with_its_reason() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    set_prefix(&core, "jd").expect("saves");
    for refused in ["a b", "a..b", "x~", ".hidden", "jd.lock", "-x"] {
        let result = set_prefix(&core, refused);
        assert!(
            matches!(
                &result,
                Err(Error::BranchPrefix { prefix, reason })
                    if prefix == refused && reason.contains("not a valid branch name")
            ),
            "{refused:?}: {result:?}"
        );
    }
    assert_eq!(core.settings().expect("reads").branch_prefix, "jd");

    // Written by hand, one git refuses does not stop the other settings from being saved, and
    // git says why when a task is made with it.
    std::fs::write(fx.paths.settings(), "branch_prefix = \"a b\"\n").expect("writes");
    let settings = Settings {
        auto_title: false,
        ..core.settings().expect("reads")
    };
    core.set_settings(&settings).expect("saves the rest");
    let made = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await;
    assert!(
        matches!(&made, Err(Error::Git { stderr, .. }) if stderr.contains("not a valid branch name")),
        "{made:?}"
    );

    // Emptied, branches start with the login again.
    set_prefix(&core, "").expect("saves");
    let task = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    assert_eq!(task.branch, format!("{}/one", login()));
}

#[tokio::test]
async fn pinned_rows_stay_above_their_siblings_and_move_among_themselves() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    two_repositories_and_three_tasks(&fx, &core).await;
    let mut bus = core.subscribe();
    let told = |bus: &mut tokio::sync::broadcast::Receiver<_>, what: &str| {
        let heard: swarmed_core::SessionEvent = bus.try_recv().expect("told");
        assert_eq!(heard.session_id, 0);
        assert!(matches!(&heard.event, AgentEvent::Reordered { kind } if kind == what));
    };

    // A pinned repository goes above the others, and stays there as they move.
    core.project_pin("aardvark", true).expect("pins");
    told(&mut bus, "project");
    assert_eq!(listed(&core).0, ["aardvark", "demo"]);
    assert!(core.project("aardvark").expect("found").pinned);
    core.project_pin("aardvark", true).expect("pinned already");
    core.project_move("demo", -1).expect("moves");
    assert_eq!(listed(&core).0, ["aardvark", "demo"]);
    assert!(
        bus.try_recv().is_err(),
        "nothing changed, so nobody is told"
    );
    // Unpinned, it is first among the others, where it was.
    core.project_pin("aardvark", false).expect("unpins");
    told(&mut bus, "project");
    assert_eq!(listed(&core).0, ["aardvark", "demo"]);
    assert!(!core.project("aardvark").expect("found").pinned);
    core.project_move("aardvark", 1).expect("moves");
    told(&mut bus, "project");

    // Each task pinned goes last among the pinned ones, which move among themselves.
    core.task_pin("demo", "three", true).expect("pins");
    told(&mut bus, "task");
    core.task_pin("demo", "two", true).expect("pins");
    assert_eq!(listed(&core).1, ["three", "two", "one"]);
    core.task_move("demo", "one", -5).expect("moves");
    assert_eq!(listed(&core).1, ["three", "two", "one"]);
    core.task_move("demo", "two", -1).expect("moves");
    assert_eq!(listed(&core).1, ["two", "three", "one"]);
    // A new task goes at the end, below the pinned ones.
    core.task_new("demo", "four", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    core.task_pin("demo", "two", false).expect("unpins");
    assert_eq!(listed(&core).1, ["three", "two", "one", "four"]);
    assert!(matches!(
        core.task_pin("demo", "nowhere", true),
        Err(Error::TaskNotFound { .. })
    ));

    // A closed session keeps its pin, and comes back where it was.
    let new = || {
        core.session_new("demo", "one", &SessionOptions::default())
            .expect("creates")
            .id
    };
    let (first, second, third) = (new(), new(), new());
    let ids = |core: &Core| -> Vec<i64> {
        let sessions = core.sessions("demo", "one").expect("lists");
        sessions.iter().map(|session| session.id).collect()
    };
    while bus.try_recv().is_ok() {}
    core.session_pin(third, true).expect("pins");
    told(&mut bus, "session");
    core.session_pin(second, true).expect("pins");
    assert_eq!(ids(&core), [third, second, first]);
    core.session_close(third).expect("closes");
    assert!(matches!(
        core.session_pin(third, false),
        Err(Error::SessionClosed(id)) if id == third
    ));
    core.session_move(first, -5).expect("moves");
    assert_eq!(ids(&core), [second, first]);
    core.session_reopen(third).expect("reopens");
    assert_eq!(ids(&core), [third, second, first]);
    assert!(core.session(third).expect("found").pinned);
}

#[tokio::test]
async fn a_row_pinned_again_after_a_move_goes_above_the_rows_not_pinned() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    two_repositories_and_three_tasks(&fx, &core).await;

    // Pinned, unpinned, moved to the end, and pinned again (#407).
    core.project_pin("demo", true).expect("pins");
    core.project_pin("demo", false).expect("unpins");
    core.project_move("demo", 5).expect("moves");
    assert_eq!(listed(&core).0, ["aardvark", "demo"]);
    core.project_pin("demo", true).expect("pins");
    assert_eq!(listed(&core).0, ["demo", "aardvark"]);

    core.task_pin("demo", "one", true).expect("pins");
    core.task_pin("demo", "one", false).expect("unpins");
    core.task_move("demo", "one", 5).expect("moves");
    assert_eq!(listed(&core).1, ["two", "three", "one"]);
    core.task_pin("demo", "one", true).expect("pins");
    assert_eq!(listed(&core).1, ["one", "two", "three"]);

    let new = || {
        core.session_new("demo", "two", &SessionOptions::default())
            .expect("creates")
            .id
    };
    let (first, second, third) = (new(), new(), new());
    let ids = |core: &Core| -> Vec<i64> {
        let sessions = core.sessions("demo", "two").expect("lists");
        sessions.iter().map(|session| session.id).collect()
    };
    core.session_pin(first, true).expect("pins");
    core.session_pin(first, false).expect("unpins");
    core.session_move(first, 5).expect("moves");
    assert_eq!(ids(&core), [second, third, first]);
    core.session_pin(first, true).expect("pins");
    assert_eq!(ids(&core), [first, second, third]);
}

#[tokio::test]
async fn a_task_put_away_is_not_among_the_tasks_a_move_passes() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    two_repositories_and_three_tasks(&fx, &core).await;
    core.task_done("demo", "two", &hooks::discard)
        .await
        .expect("puts away");
    let open = |core: &Core| -> Vec<String> {
        let overview = core.overview().expect("reads");
        let tasks = overview[0].tasks.iter().map(|t| &t.task);
        let open = tasks.filter(|task| !task.archived && task.done_at.is_none());
        open.map(|task| task.name.clone()).collect()
    };

    // The sidebar lists "one" and "three" together, and "two" apart: one place down passes
    // "three", not "two", which the sidebar does not show among them.
    assert_eq!(open(&core), ["one", "three"]);
    core.task_move("demo", "one", 1).expect("moves");
    assert_eq!(open(&core), ["three", "one"]);

    // Put away, it keeps its pin as it is, and is brought back where it was: before "three".
    // Neither pinned nor moved meanwhile, and told so.
    let pinned = core.task_pin("demo", "two", true);
    assert!(
        matches!(pinned, Err(Error::TaskPutAway { .. })),
        "{pinned:?}"
    );
    let moved = core.task_move("demo", "two", 1);
    assert!(matches!(moved, Err(Error::TaskPutAway { .. })), "{moved:?}");
    assert!(!core.task("demo", "two").expect("found").1.pinned);
    core.task_undone("demo", "two").expect("brings back");
    assert_eq!(listed(&core).1, ["two", "three", "one"]);
}

/// The names of the repositories, and of the first one's open tasks, as the sidebar lists them.
fn listed(core: &Core) -> (Vec<String>, Vec<String>) {
    let overview = core.overview().expect("reads");
    let projects = overview.iter().map(|p| p.project.name.clone()).collect();
    let tasks = overview[0]
        .tasks
        .iter()
        .filter(|t| !t.task.archived)
        .map(|t| t.task.name.clone())
        .collect();
    (projects, tasks)
}

/// Adds a second repository, `aardvark`, beside the fixture's `demo`, and three tasks to `demo`.
async fn two_repositories_and_three_tasks(fx: &common::Fixture, core: &Core) {
    let other = fx.repo.with_file_name("aardvark");
    std::fs::create_dir(&other).expect("test setup");
    git(&other, &["init", "--quiet", "-b", "main"]);
    std::fs::write(other.join("README.md"), "other\n").expect("test setup");
    git(&other, &["add", "."]);
    git(&other, &["commit", "--quiet", "-m", "init"]);
    core.project_add(&other, None).await.expect("adds");
    for name in ["one", "two", "three"] {
        core.task_new("demo", name, NewTaskOptions::default(), &hooks::discard)
            .await
            .expect("creates");
    }
}

#[tokio::test]
async fn repositories_tasks_and_sessions_keep_the_order_they_are_put_in() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    // A repository added later goes at the end, whatever its name.
    two_repositories_and_three_tasks(&fx, &core).await;
    assert_eq!(listed(&core).0, ["demo", "aardvark"]);
    assert_eq!(listed(&core).1, ["one", "two", "three"]);
    // After the tasks are made, whose setups are told on the same stream.
    let mut bus = core.subscribe();

    core.project_move("aardvark", -1).expect("moves");
    let heard = bus.try_recv().expect("told");
    assert_eq!(heard.session_id, 0);
    assert!(matches!(&heard.event, AgentEvent::Reordered { kind } if kind == "project"));
    assert_eq!(listed(&core).0, ["aardvark", "demo"]);
    // Moved no further than either end.
    core.project_move("demo", 5).expect("moves");
    assert_eq!(listed(&core).0, ["aardvark", "demo"]);
    assert!(bus.try_recv().is_err(), "nothing moved, so nobody is told");
    core.project_move("aardvark", 1).expect("moves");

    core.task_move("demo", "three", -2).expect("moves");
    assert_eq!(listed(&core).1, ["three", "one", "two"]);
    // A new task goes at the end.
    core.task_new("demo", "four", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    assert_eq!(listed(&core).1, ["three", "one", "two", "four"]);
    assert!(matches!(
        core.task_move("demo", "nowhere", 1),
        Err(Error::TaskNotFound { .. })
    ));

    // Sessions move among the open ones; a closed one keeps its place and is not moved.
    let new = || {
        core.session_new("demo", "one", &SessionOptions::default())
            .expect("creates")
            .id
    };
    let (first, second, third) = (new(), new(), new());
    core.session_close(second).expect("closes");
    let ids = |core: &Core| -> Vec<i64> {
        let sessions = core.sessions("demo", "one").expect("lists");
        sessions.iter().map(|session| session.id).collect()
    };
    core.session_move(third, -1).expect("moves");
    assert_eq!(ids(&core), [third, first]);
    assert!(matches!(
        core.session_move(second, -1),
        Err(Error::SessionClosed(id)) if id == second
    ));
    // Reopened, it is where it was: after the one it came after.
    core.session_reopen(second).expect("reopens");
    assert_eq!(ids(&core), [third, first, second]);

    // What an agent lists keeps each task's sessions together, in their order.
    let elsewhere = core
        .session_new("demo", "two", &SessionOptions::default())
        .expect("creates")
        .id;
    let told = core.peer_sessions(first).expect("lists");
    let told: Vec<&str> = told.lines().skip(1).collect();
    let told: Vec<i64> = told
        .iter()
        .filter_map(|line| line.split(['\t', ' ']).next()?.parse().ok())
        .collect();
    assert_eq!(told, [third, first, second, elsewhere]);
}
