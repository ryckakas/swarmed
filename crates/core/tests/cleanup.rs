//! Cleaning up what removed tasks left, and the snapshots, against real git repositories.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::{core_with_project, fixture, git, write_config};
use swarmed_core::{CleanupItem, CleanupKind, Core, NewTaskOptions, RemoveOptions, Task, hooks};

async fn new_task(core: &Core, name: &str) -> Task {
    core.task_new("demo", name, NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup")
}

/// Removes a task and keeps its branch, as the command line does unless told otherwise.
async fn remove(core: &Core, name: &str) {
    let options = RemoveOptions {
        force: true,
        delete_branch: false,
    };
    core.task_remove("demo", name, options, &hooks::discard)
        .await
        .expect("test setup");
}

fn commit(worktree: &Path, file: &str) {
    std::fs::write(worktree.join(file), file).expect("test setup");
    git(worktree, &["add", "."]);
    git(worktree, &["commit", "--quiet", "-m", file]);
}

/// Whether the repository has the local branch.
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

fn names(items: &[CleanupItem]) -> Vec<&str> {
    items.iter().map(|item| item.name.as_str()).collect()
}

fn key_of(items: &[CleanupItem], name: &str) -> String {
    let item = items.iter().find(|item| item.name == name);
    item.expect("listed").key.clone()
}

#[tokio::test]
async fn removed_worktrees_not_deleted_yet_are_listed_with_their_size_and_deleted() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let left = fx
        .paths
        .worktrees_root
        .join("demo/.removed/one-7-1700000000000000000");
    // A folder its own tool left without write permission, as Go's module cache is.
    let locked = left.join("pkg/mod");
    std::fs::create_dir_all(&locked).expect("test setup");
    std::fs::write(locked.join("module.go"), vec![b'x'; 50_000]).expect("test setup");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).expect("test setup");

    let listed = core
        .cleanup_items(CleanupKind::Removed)
        .await
        .expect("lists");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].task.as_deref(), Some("one"));
    assert_eq!(listed[0].since, Some(1_700_000_000));
    assert!(
        listed[0].bytes.is_some_and(|bytes| bytes >= 50_000),
        "{listed:?}"
    );
    assert!(core.usage().await.expect("measures").removed_bytes > 0);

    let cleaned = core
        .cleanup_clean(CleanupKind::Removed, None, None)
        .await
        .expect("cleans");
    assert_eq!(cleaned.deleted, [listed[0].key.clone()]);
    assert_eq!(cleaned.kept, []);
    assert!(!left.exists());
    assert_eq!(core.usage().await.expect("measures").removed_bytes, 0);
}

#[tokio::test]
async fn only_branches_swarmed_made_for_removed_tasks_are_listed_and_work_goes_only_when_named() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let done = new_task(&core, "done").await;
    remove(&core, "done").await;
    // Work on no other branch and no remote.
    let work = new_task(&core, "work").await;
    commit(&work.worktree_path, "work.txt");
    remove(&core, "work").await;
    // A branch that was there, which a task checked out as it was.
    git(&fx.repo, &["branch", "theirs"]);
    let existing = NewTaskOptions {
        existing_branch: Some("theirs"),
        ..NewTaskOptions::default()
    };
    core.task_new("demo", "on-theirs", existing, &hooks::discard)
        .await
        .expect("test setup");
    remove(&core, "on-theirs").await;
    // A removed task's branch the user has checked out since, in the repository's checkout.
    let mine = new_task(&core, "mine").await;
    remove(&core, "mine").await;
    git(&fx.repo, &["checkout", "--quiet", &mine.branch]);
    let current = new_task(&core, "current").await;

    let listed = core
        .cleanup_items(CleanupKind::Branches)
        .await
        .expect("lists");
    let mut listed_names = names(&listed);
    listed_names.sort_unstable();
    let mut expected = [done.branch.as_str(), work.branch.as_str()];
    expected.sort_unstable();
    assert_eq!(listed_names, expected);
    let of = |branch: &str| {
        listed
            .iter()
            .find(|item| item.name == branch)
            .expect("listed")
    };
    assert!(!of(&done.branch).at_risk);
    assert_eq!(of(&done.branch).task.as_deref(), Some("done"));
    assert!(of(&work.branch).at_risk);
    assert!(of(&work.branch).reason.contains("1 commit"), "{listed:?}");

    // Asked for everything, what would lose work stays.
    let cleaned = core
        .cleanup_clean(CleanupKind::Branches, None, None)
        .await
        .expect("cleans");
    assert_eq!(cleaned.deleted, [key_of(&listed, &done.branch)]);
    assert!(!has_branch(&fx.repo, &done.branch));
    assert!(has_branch(&fx.repo, &work.branch));
    // Named on its own, it goes.
    let named = [key_of(&listed, &work.branch)];
    let cleaned = core
        .cleanup_clean(CleanupKind::Branches, Some(&named), None)
        .await
        .expect("cleans");
    assert_eq!(cleaned.deleted, named);
    assert!(!has_branch(&fx.repo, &work.branch));
    // What Swarmed did not make, what the checkout is on, and what a current task uses stay.
    for branch in ["theirs", mine.branch.as_str(), current.branch.as_str()] {
        assert!(has_branch(&fx.repo, branch), "{branch}");
    }
}

#[tokio::test]
async fn a_branch_git_refuses_to_delete_is_said_and_the_others_still_go() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let stuck = new_task(&core, "stuck").await;
    remove(&core, "stuck").await;
    let free = new_task(&core, "free").await;
    remove(&core, "free").await;
    // As a git that crashed mid-update leaves it: the branch cannot be locked to delete.
    let lock = fx
        .repo
        .join(".git/refs/heads")
        .join(format!("{}.lock", stuck.branch));
    std::fs::write(&lock, "").expect("test setup");

    let cleaned = core
        .cleanup_clean(CleanupKind::Branches, None, None)
        .await
        .expect("cleans what it can");
    assert_eq!(cleaned.deleted.len(), 1, "{cleaned:?}");
    assert!(cleaned.deleted[0].ends_with(&free.branch));
    assert_eq!(cleaned.kept.len(), 1, "{cleaned:?}");
    assert_eq!(cleaned.kept[0].name, stuck.branch);
    assert!(
        cleaned.kept[0]
            .reason
            .starts_with("git would not delete it"),
        "{cleaned:?}"
    );
    assert!(has_branch(&fx.repo, &stuck.branch));
    assert!(!has_branch(&fx.repo, &free.branch));
}

#[tokio::test]
async fn a_branch_on_a_remote_keeps_its_remote_copy() {
    let fx = fixture();
    let remote = fx.repo.with_file_name("remote.git");
    git(
        fx.repo.parent().expect("has one"),
        &["init", "--quiet", "--bare", "remote.git"],
    );
    git(
        &fx.repo,
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    git(&fx.repo, &["push", "--quiet", "origin", "main"]);
    let core = core_with_project(&fx).await;
    let shared = new_task(&core, "shared").await;
    commit(&shared.worktree_path, "shared.txt");
    git(
        &shared.worktree_path,
        &["push", "--quiet", "-u", "origin", &shared.branch],
    );
    remove(&core, "shared").await;

    let listed = core
        .cleanup_items(CleanupKind::Branches)
        .await
        .expect("lists");
    assert_eq!(names(&listed), [shared.branch.as_str()]);
    // Its commits are on the remote, so it loses nothing.
    assert!(!listed[0].at_risk, "{listed:?}");
    core.cleanup_clean(CleanupKind::Branches, None, None)
        .await
        .expect("cleans");
    assert!(!has_branch(&fx.repo, &shared.branch));
    assert!(has_branch(&remote, &shared.branch));
}

#[tokio::test]
async fn stale_worktrees_are_what_no_task_owns_and_work_in_one_goes_only_when_named() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let current = new_task(&core, "current").await;
    let area = fx.paths.worktrees_root.join("demo");
    // A folder a removal that went wrong left, with nothing of git's in it.
    let leftover = area.join("leftover");
    std::fs::create_dir_all(&leftover).expect("test setup");
    std::fs::write(leftover.join("build.log"), "x").expect("test setup");
    // A worktree made by hand, with a change never committed.
    let by_hand = area.join("by-hand");
    git(
        &fx.repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "by-hand",
            &by_hand.to_string_lossy(),
        ],
    );
    std::fs::write(by_hand.join("draft.txt"), "x").expect("test setup");
    // A repository of its own: anybody's clone, never listed.
    let clone = area.join("clone");
    std::fs::create_dir_all(&clone).expect("test setup");
    git(&clone, &["init", "--quiet"]);
    // A worktree elsewhere whose folder is gone, which git still keeps a record of.
    let gone = fx.repo.with_file_name("gone");
    git(
        &fx.repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "gone",
            &gone.to_string_lossy(),
        ],
    );
    std::fs::remove_dir_all(&gone).expect("test setup");

    let listed = core.cleanup_items(CleanupKind::Stale).await.expect("lists");
    let mut listed_names = names(&listed);
    listed_names.sort_unstable();
    let as_listed = |path: &Path| path.to_string_lossy().into_owned();
    let mut expected = [as_listed(&by_hand), as_listed(&gone), as_listed(&leftover)];
    expected.sort_unstable();
    assert_eq!(listed_names, expected);
    let of = |path: &Path| {
        let name = as_listed(path);
        listed
            .iter()
            .find(|item| item.name == name)
            .expect("listed")
            .clone()
    };
    assert!(of(&by_hand).at_risk, "{listed:?}");
    assert!(of(&by_hand).reason.contains("never committed"));
    assert!(!of(&leftover).at_risk);
    assert!(of(&leftover).bytes.is_some());
    assert!(!of(&gone).at_risk);

    let cleaned = core
        .cleanup_clean(CleanupKind::Stale, None, None)
        .await
        .expect("cleans");
    assert_eq!(cleaned.deleted.len(), 2, "{cleaned:?}");
    assert!(!leftover.exists());
    assert!(by_hand.exists());
    assert!(clone.exists() && current.worktree_path.exists());
    let records = worktrees(&fx.repo);
    assert!(!records.contains("/gone"), "{records}");
    // Forgetting the record kept the branch.
    assert!(has_branch(&fx.repo, "gone"));

    let named = [of(&by_hand).key];
    let cleaned = core
        .cleanup_clean(CleanupKind::Stale, Some(&named), None)
        .await
        .expect("cleans");
    assert_eq!(cleaned.deleted, named);
    assert!(!by_hand.exists());
    let records = worktrees(&fx.repo);
    assert!(!records.contains("by-hand"), "{records}");
    assert!(records.contains(&current.worktree_path.to_string_lossy().into_owned()));
}

#[tokio::test]
async fn a_record_whose_folder_is_there_again_is_not_listed_and_the_folder_stays() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    // A worktree made by hand, deleted, and its place then used for something else. Git still
    // calls the record prunable: it only looks for the `.git` file in it.
    let reused = fx.repo.with_file_name("reused");
    let place = reused.to_string_lossy().into_owned();
    git(
        &fx.repo,
        &["worktree", "add", "--quiet", "-b", "reused", &place],
    );
    std::fs::remove_dir_all(&reused).expect("test setup");
    std::fs::create_dir_all(&reused).expect("test setup");
    std::fs::write(reused.join("thesis.txt"), "years of work").expect("test setup");
    assert!(worktrees(&fx.repo).contains("prunable"), "git's own view");

    let listed = core.cleanup_items(CleanupKind::Stale).await.expect("lists");
    assert!(listed.is_empty(), "{listed:?}");
    // Named by the key it would have, as a list read before the folder came back names it.
    let named = [place];
    let cleaned = core
        .cleanup_clean(CleanupKind::Stale, Some(&named), None)
        .await
        .expect("cleans");
    assert!(cleaned.deleted.is_empty(), "{cleaned:?}");
    assert_eq!(cleaned.kept.len(), 1, "{cleaned:?}");
    assert!(reused.join("thesis.txt").exists());
}

#[tokio::test]
async fn a_repository_that_is_away_lists_no_branches_and_stops_no_other_kind() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let done = new_task(&core, "done").await;
    remove(&core, "done").await;
    let left = fx
        .paths
        .worktrees_root
        .join("demo/.removed/one-7-1700000000000000000");
    std::fs::create_dir_all(&left).expect("test setup");
    let listed = core.cleanup().await.expect("lists");
    assert_eq!(names(&listed.branches), [done.branch.as_str()]);

    // The repository is moved, or on a disk that is not there now.
    std::fs::rename(&fx.repo, fx.repo.with_file_name("moved")).expect("test setup");
    let listed = core.cleanup().await.expect("still lists");
    assert!(listed.branches.is_empty(), "{listed:?}");
    assert_eq!(listed.removed.len(), 1);
}

/// What `git worktree list` says of the repository.
fn worktrees(repo: &Path) -> String {
    let listed = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .expect("runs git");
    String::from_utf8_lossy(&listed.stdout).into_owned()
}

#[tokio::test]
async fn a_snapshot_being_taken_is_kept_and_one_cut_short_goes() {
    let fx = fixture();
    std::fs::write(fx.repo.join(".gitignore"), "*.cache\n").expect("test setup");
    git(&fx.repo, &["add", "."]);
    git(&fx.repo, &["commit", "--quiet", "-m", "ignore caches"]);
    write_config(&fx, "[hooks]\nsetup = \"echo built > deps.cache\"\n");
    let core = core_with_project(&fx).await;
    // Set up well, the first task is kept as the snapshot of tasks without an environment.
    new_task(&core, "one").await;
    let snapshots = fx.paths.data_dir.join("snapshots/demo");
    // A taking of another environment's that was cut short.
    let cut_short = snapshots.join("_new.staging/files");
    std::fs::create_dir_all(&cut_short).expect("test setup");
    std::fs::write(cut_short.join("deps.cache"), "half").expect("test setup");

    let listed = core
        .cleanup_items(CleanupKind::Snapshots)
        .await
        .expect("lists");
    assert_eq!(names(&listed), ["tasks without an environment", "staging"]);
    assert_eq!(listed[0].task.as_deref(), Some("one"));
    assert!(
        listed[0].since.is_some() && listed[0].bytes.is_some(),
        "{listed:?}"
    );
    assert!(listed[1].reason.contains("cut short"));

    // Another snapshot of tasks without an environment is being taken meanwhile.
    let lock = std::fs::File::open(snapshots.join("_lock._default")).expect("there");
    lock.try_lock().expect("free");
    let cleaned = core
        .cleanup_clean(CleanupKind::Snapshots, None, None)
        .await
        .expect("cleans");
    assert_eq!(cleaned.deleted, [listed[1].key.clone()]);
    assert_eq!(cleaned.kept.len(), 1, "{cleaned:?}");
    assert!(
        cleaned.kept[0].reason.contains("being taken"),
        "{cleaned:?}"
    );
    assert!(!snapshots.join("_new.staging").exists());
    assert_eq!(core.snapshots("demo").expect("reads").len(), 1);

    lock.unlock().expect("unlocks");
    let cleaned = core
        .cleanup_clean(CleanupKind::Snapshots, None, None)
        .await
        .expect("cleans");
    assert_eq!(cleaned.deleted, [listed[0].key.clone()]);
    assert_eq!(core.snapshots("demo").expect("reads"), []);
}

#[tokio::test]
async fn checkpoints_of_tasks_that_are_gone_are_listed_and_a_current_tasks_never() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let current = new_task(&core, "current").await;
    let gone = new_task(&core, "gone").await;
    remove(&core, "gone").await;
    // As a removal cut short, or one by a Swarmed that knew nothing of checkpoints, leaves them.
    for task in [&current, &gone] {
        let store = fx.paths.checkpoints(task.id).join("objects");
        std::fs::create_dir_all(&store).expect("test setup");
        std::fs::write(store.join("object"), "tree").expect("test setup");
    }
    let unknown = fx.paths.checkpoints(9_999);
    std::fs::create_dir_all(&unknown).expect("test setup");

    let listed = core
        .cleanup_items(CleanupKind::Checkpoints)
        .await
        .expect("lists");
    let mut tasks: Vec<Option<&str>> = listed.iter().map(|item| item.task.as_deref()).collect();
    tasks.sort_unstable();
    assert_eq!(tasks, [None, Some("gone")]);
    assert!(listed.iter().all(|item| !item.at_risk));

    let cleaned = core
        .cleanup_clean(CleanupKind::Checkpoints, None, None)
        .await
        .expect("cleans");
    assert_eq!(cleaned.deleted.len(), 2);
    assert!(!fx.paths.checkpoints(gone.id).exists());
    assert!(!unknown.exists());
    assert!(fx.paths.checkpoints(current.id).exists());
}
