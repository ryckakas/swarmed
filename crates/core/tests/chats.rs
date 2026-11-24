//! Chats: sessions with no repository, against the scripted agent.

mod common;

use common::{core_with_project, fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    CHATS, Core, Error, NewTaskOptions, RemoveOptions, SessionOptions, SetupStatus, hooks,
};
use tokio::sync::broadcast::Receiver;

/// Makes the chat `name`, as the app does: named by the core, past any name taken.
async fn chat(core: &Core, name: &str) -> swarmed_core::Task {
    let options = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    core.task_new(CHATS, name, options, &hooks::discard)
        .await
        .expect("makes a chat")
}

/// What the agent of `session` says to `message`, its last words in the turn.
async fn answer(
    core: &Core,
    bus: &mut Receiver<swarmed_core::SessionEvent>,
    session: i64,
    message: &str,
) -> String {
    core.session_send(session, message).expect("sends");
    let events = turn(bus, session).await;
    let said = events.iter().rev().find_map(|event| match event {
        AgentEvent::Text { text } => Some(text.clone()),
        _ => None,
    });
    said.expect("the agent answers")
}

fn worktrees_of(fx: &common::Fixture) -> usize {
    let listed = std::process::Command::new("git")
        .arg("-C")
        .arg(&fx.repo)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter(|line| line.starts_with("worktree "))
        .count()
}

#[tokio::test]
async fn a_chat_has_a_folder_of_its_own_and_touches_no_repository() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    // No chat, no home for them: nothing is added to the repositories until one is made.
    assert_eq!(core.tasks(CHATS, true).expect("lists"), vec![]);
    assert_eq!(core.projects().expect("lists").len(), 1);

    let made = chat(&core, "what-is-a-monad").await;
    assert_eq!(made.name, "what-is-a-monad");
    assert_eq!(made.branch, "");
    assert_eq!(made.setup, SetupStatus::Ok);
    let folder = fx.paths.worktrees_root.join(CHATS).join("what-is-a-monad");
    assert_eq!(made.worktree_path, folder);
    assert!(folder.is_dir());
    assert!(!folder.join(".git").exists());
    // The repository got no worktree and no branch.
    assert_eq!(worktrees_of(&fx), 1);

    let projects = core.projects().expect("lists");
    let home = projects.iter().find(|project| project.chats);
    let home = home.expect("the chats have a home now");
    assert_eq!(home.name, CHATS);
    assert!(matches!(home.repo(), Err(Error::NoRepository)));
    assert!(projects.iter().any(|project| !project.chats));

    // Named like a task: a name taken goes on to the next free one.
    assert_eq!(
        chat(&core, "what-is-a-monad").await.name,
        "what-is-a-monad-2"
    );
}

#[tokio::test]
async fn no_repository_can_take_the_name_of_the_chats_and_a_general_one_is_its_own() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let refused = core.project_add(&fx.repo, Some(CHATS)).await;
    assert!(matches!(refused, Err(Error::InvalidName(_))), "{refused:?}");

    // A repository called what a window calls the chats is a repository like any other.
    let general = fx.repo.with_file_name("general");
    std::fs::create_dir(&general).expect("test setup");
    common::git(&general, &["init", "--quiet", "-b", "main"]);
    std::fs::write(general.join("README.md"), "general\n").expect("test setup");
    common::git(&general, &["add", "."]);
    common::git(&general, &["commit", "--quiet", "-m", "init"]);
    core.project_add(&general, None).await.expect("adds");
    core.task_new("general", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("a task in it");
    chat(&core, "one").await;

    let names = |project: &str| -> Vec<String> {
        let tasks = core.tasks(project, false).expect("lists");
        tasks.into_iter().map(|task| task.name).collect()
    };
    assert_eq!(names("general"), ["one"]);
    assert_eq!(names(CHATS), ["one"]);
    let (_, in_repository) = core.task("general", "one").expect("found");
    assert_ne!(in_repository.branch, "");
    let (home, in_chats) = core.task(CHATS, "one").expect("found");
    assert!(home.chats);
    assert_ne!(in_chats.worktree_path, in_repository.worktree_path);

    // Moving repositories leaves the chats out: they are listed apart.
    core.project_move("general", -5).expect("moves");
    let order: Vec<String> = core
        .projects()
        .expect("lists")
        .into_iter()
        .filter(|project| !project.chats)
        .map(|project| project.name)
        .collect();
    assert_eq!(order, ["general", "demo"]);
    // Nor are they pinned among them; a chat is pinned among the chats.
    core.project_pin(CHATS, true).expect("left as it is");
    assert!(!core.project(CHATS).expect("found").pinned);
    chat(&core, "two").await;
    core.task_pin(CHATS, "two", true).expect("pins");
    let chats = core.tasks(CHATS, false).expect("lists");
    let chats: Vec<(String, bool)> = chats.into_iter().map(|t| (t.name, t.pinned)).collect();
    assert_eq!(chats, [("two".into(), true), ("one".into(), false)]);
}

#[tokio::test]
async fn what_only_a_repository_has_is_refused_for_a_chat() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    chat(&core, "question").await;
    let no_repository = |result: Result<(), Error>| matches!(result, Err(Error::NoRepository));
    assert!(no_repository(core.branch_choices(CHATS).await.map(drop)));
    assert!(no_repository(
        core.task_changes(CHATS, "question").await.map(drop)
    ));
    assert!(no_repository(
        core.task_unpublished(CHATS, "question").await.map(drop)
    ));
    assert!(no_repository(core.task_push(CHATS, "question").await));
    assert!(no_repository(
        core.task_diff(CHATS, "question", "a.txt").await.map(drop)
    ));
    assert!(no_repository(
        core.task_switch(CHATS, "question", "main").await
    ));
    assert!(no_repository(
        core.task_commit(CHATS, "question", "a message").await
    ));
    assert!(no_repository(
        core.task_pull(CHATS, "question").await.map(drop)
    ));
    assert!(no_repository(
        core.task_pull_draft(CHATS, "question").await.map(drop)
    ));
    assert!(no_repository(
        core.snapshot_save(CHATS, "question").await.map(drop)
    ));
    assert!(no_repository(core.stack_start(CHATS, "question").map(drop)));
    assert!(no_repository(
        core.stack_stop(CHATS, "question", &hooks::discard).await
    ));
    assert!(no_repository(
        core.task_setup(CHATS, "question", None, &hooks::discard)
            .await
            .map(drop)
    ));
    let on_branch = NewTaskOptions {
        existing_branch: Some("main"),
        ..NewTaskOptions::default()
    };
    let refused = core
        .task_new(CHATS, "other", on_branch, &hooks::discard)
        .await;
    assert!(no_repository(refused.map(drop)));
}

#[tokio::test]
async fn cleanup_keeps_a_chat_and_holds_back_a_stray_folder_that_holds_files() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let made = chat(&core, "question").await;
    let home = made.worktree_path.parent().expect("the chats' folder");
    std::fs::create_dir(home.join("empty")).expect("test setup");
    std::fs::create_dir(home.join("left")).expect("test setup");
    std::fs::write(home.join("left/draft.md"), "kept?\n").expect("test setup");

    let stale = core.cleanup().await.expect("lists").stale;
    let at_risk = |name: &str| {
        let folder = home.join(name).to_string_lossy().into_owned();
        stale
            .iter()
            .find(|item| item.key == folder)
            .map(|item| item.at_risk)
    };
    // The chat's own folder is no item; a stray one goes unasked only while it is empty.
    assert_eq!(at_risk("question"), None);
    assert_eq!(at_risk("empty"), Some(false));
    assert_eq!(at_risk("left"), Some(true));
}

#[tokio::test]
async fn a_chat_goes_with_its_folder_but_not_one_that_holds_files_unless_forced() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let made = chat(&core, "notes").await;
    std::fs::write(made.worktree_path.join("draft.md"), "kept?\n").expect("test setup");

    let options = RemoveOptions::default();
    let kept = core
        .task_remove(CHATS, "notes", options, &hooks::discard)
        .await;
    assert!(
        matches!(&kept, Err(Error::TaskNotClean { reason, .. }) if reason.contains("files")),
        "{kept:?}"
    );
    assert!(made.worktree_path.join("draft.md").exists());

    let forced = RemoveOptions {
        force: true,
        ..RemoveOptions::default()
    };
    core.task_remove(CHATS, "notes", forced, &hooks::discard)
        .await
        .expect("removes");
    assert!(!made.worktree_path.exists());
    assert_eq!(core.tasks(CHATS, false).expect("lists"), vec![]);

    // An empty one goes when asked.
    let empty = chat(&core, "empty").await;
    core.task_remove(CHATS, "empty", options, &hooks::discard)
        .await
        .expect("removes");
    assert!(!empty.worktree_path.exists());
}

#[tokio::test]
async fn a_chats_agent_works_in_its_folder_and_is_told_it_has_no_repository() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let made = chat(&core, "question").await;
    let session = core
        .session_new(CHATS, "question", &SessionOptions::default())
        .expect("creates");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    let mut bus = core.subscribe();

    let guidance = answer(&core, &mut bus, session.id, "instructions").await;
    assert!(guidance.starts_with("# No repository"), "{guidance}");
    assert!(!guidance.contains("Your task's branch"), "{guidance}");
    // Nothing tells a skill that this is a task's worktree.
    let task = answer(&core, &mut bus, session.id, "env:SWARMED_TASK").await;
    assert_eq!(task, "unset");
    // Its terminal opens there too, as a task's opens in its worktree.
    assert!(made.worktree_path.is_dir());
}

#[tokio::test]
async fn a_chat_carries_on_in_a_task_of_a_repository_and_stays_as_it_was() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    chat(&core, "idea").await;
    let session = core
        .session_new(CHATS, "idea", &SessionOptions::default())
        .expect("creates");
    core.session_rename(session.id, "An idea").expect("renames");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    let mut bus = core.subscribe();
    answer(&core, &mut bus, session.id, "make it so").await;

    core.task_new("demo", "idea", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("a task for it");
    let carried = core
        .session_fork_into(session.id, "demo", "idea")
        .expect("forks");
    assert_eq!(carried.title, "An idea");
    let (_, task) = core.task("demo", "idea").expect("found");
    assert_eq!(carried.task_id, task.id);
    let events = core.session_events(carried.id).expect("reads");
    let said = |text: &str| {
        events.iter().any(
            |event| matches!(event, AgentEvent::UserMessage { text: said, .. } if said == text),
        )
    };
    assert!(said("make it so"), "{events:?}");
    assert!(matches!(events.last(), Some(AgentEvent::Forked { .. })));
    // The chat keeps its conversation.
    let first = core.session_events(session.id).expect("reads");
    assert!(first.len() >= events.len());
}
