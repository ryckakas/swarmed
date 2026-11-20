//! Task lifecycle against a real temporary git repository.

mod common;

use std::sync::Mutex;

use common::{core_with_project, fixture, git, write_config};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    ChangeKind, Core, Environment, Error, NewTaskOptions, ProjectConfig, RemoveOptions,
    SessionOptions, Settings, SetupStatus, hooks,
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
    assert_eq!(
        *lines.lock().expect("test setup"),
        vec!["hello from setup".to_string()]
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
        .task_setup("demo", "one", &hooks::discard)
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
    core.task_remove("demo", "one", options, &hooks::discard)
        .await
        .expect("test setup");
    assert!(!task.worktree_path.exists());
    assert_eq!(core.tasks("demo", false).expect("test setup"), vec![]);
    let archived = core.tasks("demo", true).expect("test setup");
    assert_eq!(archived.len(), 1);
    assert!(archived[0].archived);

    // The name and the slot are free again.
    let again = core
        .task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    assert_eq!(again.slot, 1);
}

#[tokio::test]
async fn remove_runs_teardown_and_refuses_when_it_fails() {
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
    let blocked = core
        .task_remove("demo", "one", RemoveOptions::default(), &hooks::discard)
        .await;
    assert!(matches!(
        blocked,
        Err(Error::HookFailed {
            hook: "teardown",
            ..
        })
    ));
    assert!(task.worktree_path.exists());
    assert_eq!(core.tasks("demo", false).expect("test setup").len(), 1);

    std::fs::remove_file(fx.repo.join("block-teardown")).expect("test setup");
    core.task_remove("demo", "one", RemoveOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    assert!(marker.exists());
    assert!(!task.worktree_path.exists());
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

    let options = |environment| NewTaskOptions {
        environment,
        ..NewTaskOptions::default()
    };
    let how = |task: &swarmed_core::Task| {
        std::fs::read_to_string(task.worktree_path.join("how.txt")).expect("set up")
    };
    let plain = core
        .task_new("demo", "one", options(None), &hooks::discard)
        .await
        .expect("creates");
    let staged = core
        .task_new("demo", "two", options(Some("staging")), &hooks::discard)
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

    // One that is not there is refused; one that goes leaves its tasks on the project's own.
    let missing = core
        .task_new("demo", "three", options(Some("nope")), &hooks::discard)
        .await;
    assert!(matches!(missing, Err(Error::EnvironmentNotFound { .. })));
    core.environment_remove("demo", "staging").expect("removes");
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
    assert_eq!(stack(&core, 0), Some(true));
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
    assert_eq!(stack(&core, 0), Some(false));
    assert!(!alive(&child), "the stack's child outlived it");

    // It can be started again, and goes with its task.
    assert!(core.stack_start("demo", "one").expect("starts"));
    assert_eq!(stack(&core, 0), Some(true));
    let force = RemoveOptions {
        force: true,
        ..RemoveOptions::default()
    };
    core.task_remove("demo", "one", force, &hooks::discard)
        .await
        .expect("removes");
    core.stacks_stop_all().await;
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
    std::fs::write(&script, body).expect("writes");
    make_executable(&script);
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

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("chmod");
}

/// The file's contents once the launched program has written it.
async fn wait_for_file(path: &std::path::Path) -> String {
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
