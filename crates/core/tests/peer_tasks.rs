//! A session starting a new task for work that needs a branch of its own, against the scripted
//! agent.

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use common::{core_with_project, fixture, git, turn};
use swarmed_core::agent::{AgentEvent, Auth, Supervision};
use swarmed_core::{
    CHATS, Core, Error, MAX_SPAWNED, NewTaskOptions, PeerTask, Session, SessionOptions, hooks,
    serve_session_tools,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

async fn core_with_task(fx: &common::Fixture) -> Arc<Core> {
    core_that(fx, true).await
}

/// A core with one task, in a process that stays after a turn as the window does, or not.
async fn core_that(fx: &common::Fixture, stays: bool) -> Arc<Core> {
    let mut core = core_with_project(fx).await;
    if stays {
        core.set_stays();
    }
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    Arc::new(core)
}

fn new_session(core: &Core) -> Session {
    core.session_new("demo", "one", &SessionOptions::default())
        .expect("creates")
}

/// Waits until something listens on `socket`, for as long as a busy test runner may take.
async fn listening(socket: &Path) {
    for _ in 0..1000 {
        if socket.exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("nothing listens on {}", socket.display());
}

/// The commit `name` points at in the repository at `dir`.
fn commit_of(dir: &Path, name: &str) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", name])
        .output()
        .expect("runs git");
    String::from_utf8(out.stdout)
        .expect("text")
        .trim()
        .to_string()
}

#[tokio::test]
async fn a_session_starts_a_task_of_its_own_whose_session_hears_from_it() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let mut told = core.subscribe();
    let asking = new_session(&core);
    let own = core.task("demo", "one").expect("exists").1;

    let asked = PeerTask {
        name: "fix-review",
        prompt: "Fix the review findings.",
        title: Some("Review fixes"),
        ..PeerTask::default()
    };
    let started = core
        .peer_start_task(asking.id, asked)
        .await
        .expect("starts");
    assert_eq!(started.task.name, "fix-review");
    assert_eq!(started.task.project_id, own.project_id);
    assert_ne!(started.task.worktree_path, own.worktree_path);
    assert_eq!(started.task.base_ref, own.base_ref);
    assert_eq!(started.session.task_id, started.task.id);
    assert_eq!(started.session.title, "Review fixes");
    // The first of a task of its own, not a helper of the starter's: it can start more. It
    // works for the starter all the same, and the database says so too.
    assert!(!started.session.spawned);
    assert_eq!(started.session.parent, Some(asking.id));
    let kept = core.session(started.session.id).expect("reads");
    assert_eq!((kept.parent, kept.spawned), (Some(asking.id), false));
    // The window hears of a task this process made.
    let tree = std::iter::from_fn(|| told.try_recv().ok())
        .any(|heard| heard.session_id == 0 && matches!(heard.event, AgentEvent::TreeChanged));
    assert!(tree, "the tree was not said to have changed");

    // Set up, its prompt goes, marked as the starter's words and asking for a report.
    let events = turn(&mut bus, started.session.id).await;
    let prompt = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::UserMessage { text, .. } => Some(text.clone()),
            _ => None,
        })
        .expect("the prompt went");
    let mark = format!("Message from session {} (\"", asking.id);
    assert!(prompt.starts_with(&mark), "{prompt}");
    assert!(prompt.contains("Fix the review findings."), "{prompt}");
    assert!(
        prompt.contains(&format!("to session {}", asking.id)),
        "{prompt}"
    );

    // A name that is taken gives way, and a title left out is the prompt's, not the mark's.
    let again = PeerTask {
        name: "fix-review",
        prompt: "Fix the other findings.",
        ..PeerTask::default()
    };
    let second = core
        .peer_start_task(asking.id, again)
        .await
        .expect("starts");
    assert_eq!(second.task.name, "fix-review-2");
    assert_eq!(second.session.title, "Fix the other findings.");
}

#[tokio::test]
async fn a_task_checks_out_a_pull_requests_branch_or_stacks_on_one() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let asking = new_session(&core);
    git(&fx.repo, &["switch", "--quiet", "-c", "pr/5028"]);
    git(
        &fx.repo,
        &["commit", "--quiet", "--allow-empty", "-m", "the pr"],
    );
    git(&fx.repo, &["switch", "--quiet", "main"]);

    // A fix to another pull request: on its branch as it is.
    let on_branch = PeerTask {
        name: "fix-5028",
        prompt: "Fix it.",
        branch: Some("pr/5028"),
        ..PeerTask::default()
    };
    let fixing = core
        .peer_start_task(asking.id, on_branch)
        .await
        .expect("starts");
    assert_eq!(fixing.task.branch, "pr/5028");

    // The next of a stack: a new branch from that one, which is its base.
    let stacked = PeerTask {
        name: "next",
        prompt: "Build on it.",
        from: Some("pr/5028"),
        ..PeerTask::default()
    };
    let next = core
        .peer_start_task(asking.id, stacked)
        .await
        .expect("starts");
    assert_ne!(next.task.branch, "pr/5028");
    assert_eq!(next.task.base_ref, "pr/5028");
    assert_eq!(
        commit_of(&next.task.worktree_path, "HEAD"),
        commit_of(&fx.repo, "pr/5028")
    );

    // A branch that is nowhere, and an existing branch asked to start elsewhere, are refused.
    let nowhere = PeerTask {
        name: "lost",
        prompt: "Build on it.",
        from: Some("no/such"),
        ..PeerTask::default()
    };
    let refused = core.peer_start_task(asking.id, nowhere).await;
    assert!(
        matches!(refused, Err(Error::NoBranchToStartFrom(ref name)) if name == "no/such"),
        "{refused:?}"
    );
    // A commit named by way of a branch is no branch to keep as a base.
    let earlier = PeerTask {
        name: "earlier",
        prompt: "Build on it.",
        from: Some("pr/5028~1"),
        ..PeerTask::default()
    };
    let refused = core.peer_start_task(asking.id, earlier).await;
    assert!(
        matches!(refused, Err(Error::NoBranchToStartFrom(ref name)) if name == "pr/5028~1"),
        "{refused:?}"
    );
    let both = PeerTask {
        name: "both",
        prompt: "Build on it.",
        branch: Some("pr/5028"),
        from: Some("main"),
        ..PeerTask::default()
    };
    let refused = core.peer_start_task(asking.id, both).await;
    assert!(
        matches!(refused, Err(Error::ExistingBranchStartsFromNothing(_))),
        "{refused:?}"
    );
    let names: Vec<String> = core
        .tasks("demo", false)
        .expect("lists")
        .into_iter()
        .map(|task| task.name)
        .collect();
    assert_eq!(names, ["one", "fix-5028", "next"]);
}

#[tokio::test]
async fn a_task_stacks_on_a_branch_that_is_only_on_the_remote() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let asking = new_session(&core);
    let remote = fx.repo.with_file_name("origin.git");
    let remote = remote.to_string_lossy();
    git(&fx.repo, &["init", "--quiet", "--bare", &remote]);
    git(&fx.repo, &["remote", "add", "origin", &remote]);
    git(&fx.repo, &["switch", "--quiet", "-c", "feature-x"]);
    git(
        &fx.repo,
        &["commit", "--quiet", "--allow-empty", "-m", "the pr"],
    );
    git(&fx.repo, &["push", "--quiet", "origin", "feature-x"]);
    let pushed = commit_of(&fx.repo, "feature-x");
    git(&fx.repo, &["switch", "--quiet", "main"]);
    git(&fx.repo, &["branch", "--quiet", "-D", "feature-x"]);

    // Named as a pull request names its head, and with its remote: the same branch.
    for (name, from) in [("short", "feature-x"), ("whole", "origin/feature-x")] {
        let stacked = PeerTask {
            name,
            prompt: "Build on it.",
            from: Some(from),
            ..PeerTask::default()
        };
        let next = core
            .peer_start_task(asking.id, stacked)
            .await
            .expect("starts");
        assert_eq!(next.task.base_ref, "origin/feature-x");
        assert_eq!(commit_of(&next.task.worktree_path, "HEAD"), pushed);
    }
}

#[tokio::test]
async fn a_task_is_refused_to_a_helper_a_chat_and_an_empty_prompt() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut bus = core.subscribe();
    let asking = new_session(&core);
    let asked = |prompt| PeerTask {
        name: "work",
        prompt,
        ..PeerTask::default()
    };

    assert!(matches!(
        core.peer_start_task(asking.id, asked(" \n")).await,
        Err(Error::EmptyMessage)
    ));
    let helper = core
        .peer_spawn(asking.id, "a part", None, None)
        .expect("spawns");
    turn(&mut bus, helper.id).await;
    let refused = core.peer_start_task(helper.id, asked("more")).await;
    assert!(
        matches!(refused, Err(Error::Peers(ref reason)) if reason.contains("cannot start tasks")),
        "{refused:?}"
    );

    let free = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    core.task_new(CHATS, "question", free, &hooks::discard)
        .await
        .expect("makes a chat");
    let chat = core
        .session_new(CHATS, "question", &SessionOptions::default())
        .expect("creates");
    let refused = core.peer_start_task(chat.id, asked("work")).await;
    assert!(
        matches!(refused, Err(Error::Peers(ref reason)) if reason.contains("no repository")),
        "{refused:?}"
    );
    assert_eq!(core.tasks("demo", false).expect("lists").len(), 1);
}

#[tokio::test]
async fn a_task_is_refused_while_as_many_work_for_the_session_as_may() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let asking = new_session(&core);
    // However they came to work for it: these the user chose.
    for _ in 0..MAX_SPAWNED {
        let another = new_session(&core);
        core.session_set_parent(another.id, Some(asking.id))
            .expect("works for it");
    }
    let asked = PeerTask {
        name: "work",
        prompt: "one more part",
        ..PeerTask::default()
    };
    let refused = core.peer_start_task(asking.id, asked).await;
    assert!(
        matches!(refused, Err(Error::Peers(ref reason)) if reason.contains("the most allowed")),
        "{refused:?}"
    );
    assert_eq!(core.tasks("demo", false).expect("lists").len(), 1);
}

/// What the tool server of `core` answers session `from`'s `start_task` call for `name`.
async fn asked_over_the_socket(core: &Arc<Core>, from: i64, name: &str) -> serde_json::Value {
    drop(tokio::spawn(serve_session_tools(Arc::clone(core))));
    let socket = core.paths().tools_socket();
    listening(&socket).await;
    let request = serde_json::json!({
        "session": from, "tool": "start_task",
        "arguments": { "name": name, "prompt": "Do the work.", "title": "Work" },
    });
    let mut stream = UnixStream::connect(&socket).await.expect("connects");
    stream
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("writes");
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .await
        .expect("reads");
    serde_json::from_str(&line).expect("json")
}

#[tokio::test]
async fn a_command_that_ends_with_its_turn_starts_no_task() {
    let fx = fixture();
    let core = core_that(&fx, false).await;
    let asking = new_session(&core);
    let answer = asked_over_the_socket(&core, asking.id, "cut-short").await;
    assert_eq!(answer["ok"], false, "{answer}");
    let text = answer["text"].as_str().expect("text");
    assert!(text.contains("from the command line"), "{text}");
    assert!(core.task("demo", "cut-short").is_err());
}

#[tokio::test]
async fn the_tool_server_starts_a_task_over_its_socket() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let asking = new_session(&core);
    core.session_start(asking.id, Auth::Subscription)
        .expect("starts");
    let answer = asked_over_the_socket(&core, asking.id, "over-the-wire").await;
    assert_eq!(answer["ok"], true, "{answer}");
    let text = answer["text"].as_str().expect("text");
    assert!(text.starts_with("Started task `over-the-wire`"), "{text}");
    let (_, task) = core.task("demo", "over-the-wire").expect("made");
    let sessions = core.sessions("demo", "over-the-wire").expect("lists");
    assert_eq!(sessions.len(), 1);
    assert!(
        text.contains(&format!("with session {}", sessions[0].id)),
        "{text}"
    );
    assert!(text.contains(&task.branch), "{text}");
}

#[tokio::test]
async fn a_task_s_session_has_the_role_its_starter_may_start() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let with_role = |role: &str| SessionOptions {
        role: Some(role.into()),
        ..SessionOptions::default()
    };
    let architect = core
        .session_new("demo", "one", &with_role("architect"))
        .expect("creates");
    let asked = |name, role| PeerTask {
        name,
        prompt: "Plan the export.",
        role,
        ..PeerTask::default()
    };
    // Without a role asked for, the one an architect starts: a planner.
    let started = core
        .peer_start_task(architect.id, asked("export", None))
        .await
        .expect("starts");
    assert_eq!(started.session.role.as_deref(), Some("planner"));
    assert_eq!(started.session.parent, Some(architect.id));
    let refused = core
        .peer_start_task(architect.id, asked("build", Some("implementer")))
        .await;
    assert!(
        matches!(&refused, Err(Error::Peers(why)) if why.contains("may start only planner or reviewer")),
        "{refused:?}"
    );
    // In plan mode, as a planner starts, with the supervision its starter had before its own
    // role took its place to put back.
    assert_eq!(architect.supervision, Supervision::Plan);
    assert_eq!(started.session.supervision, Supervision::Plan);
    core.session_set_role(started.session.id, None)
        .expect("takes it off");
    let without = core.session(started.session.id).expect("reads");
    assert_eq!(without.supervision, SessionOptions::default().supervision);

    // An architect another session started starts tasks all the same: its role decides.
    let user_s = new_session(&core);
    let spawned = core
        .peer_spawn(user_s.id, "Shape the export.", None, Some("architect"))
        .expect("spawns");
    assert!(spawned.spawned);
    let started = core
        .peer_start_task(spawned.id, asked("export-part", None))
        .await
        .expect("a role that starts sessions may");
    assert_eq!(started.session.parent, Some(spawned.id));

    let implementer = core
        .session_new("demo", "one", &with_role("implementer"))
        .expect("creates");
    let refused = core
        .peer_start_task(implementer.id, asked("more", None))
        .await;
    assert!(
        matches!(&refused, Err(Error::Peers(why)) if why.contains("starts no sessions")),
        "{refused:?}"
    );
}
