//! Task lifecycle against a real temporary git repository.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use swarmed_core::{
    Core, Error, NewTaskOptions, Paths, ProjectConfig, RemoveOptions, SetupStatus, hooks,
};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    repo: PathBuf,
    paths: Paths,
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
        .expect("test setup");
    assert!(status.success(), "git {args:?} failed");
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("test setup");
    // macOS temp dirs are symlinks; git reports the resolved path.
    let root = dir.path().canonicalize().expect("test setup");
    let repo = root.join("demo");
    std::fs::create_dir(&repo).expect("test setup");
    git(&repo, &["init", "--quiet", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "demo\n").expect("test setup");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--quiet", "-m", "init"]);
    let paths = Paths {
        data_dir: root.join("data"),
        worktrees_root: root.join("worktrees"),
    };
    Fixture {
        _dir: dir,
        repo,
        paths,
    }
}

fn write_config(fx: &Fixture, toml: &str) {
    let path = fx.paths.project_config("demo");
    std::fs::create_dir_all(path.parent().expect("test setup")).expect("test setup");
    std::fs::write(path, toml).expect("test setup");
}

async fn core_with_project(fx: &Fixture) -> Core {
    let core = Core::open(fx.paths.clone()).expect("test setup");
    core.project_add(&fx.repo, None).await.expect("test setup");
    core
}

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
async fn task_new_can_check_out_an_existing_branch() {
    let fx = fixture();
    git(&fx.repo, &["branch", "feature/existing"]);
    let core = core_with_project(&fx).await;
    let options = NewTaskOptions {
        existing_branch: Some("feature/existing"),
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
