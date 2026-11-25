//! Taking a session back to before one of its messages: its conversation, its files if asked,
//! putting the files back, and every reason it is refused.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use common::{core_with_project, fixture, git, next, turn};
use swarmed_core::agent::{AcpBackend, AgentEvent, Auth, FakeBackend};
use swarmed_core::{
    AgentInfo, Attached, CHATS, ChangeKind, Core, Error, MAX_UNTRACKED_BYTES, NewTaskOptions,
    RewindDone, SessionEvent, SessionOptions, Task, hooks,
};
use tokio::sync::broadcast::Receiver;

async fn task(core: &Core) -> Task {
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup")
}

fn started(core: &Core) -> i64 {
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("test setup");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    session.id
}

async fn say(
    core: &Core,
    bus: &mut Receiver<SessionEvent>,
    id: i64,
    text: &str,
) -> Vec<AgentEvent> {
    core.session_start(id, Auth::Subscription).expect("starts");
    core.session_send(id, text).expect("sends");
    turn(bus, id).await
}

/// What the agent said in a turn, in one piece.
fn said(events: &[AgentEvent]) -> String {
    let texts = events.iter().filter_map(|event| match event {
        AgentEvent::Text { text } => Some(text.as_str()),
        _ => None,
    });
    texts.collect::<Vec<_>>().join("\n")
}

fn read(worktree: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(worktree.join(path)).ok()
}

fn head(worktree: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("runs");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The worktree's own folder in the repository.
fn git_dir(worktree: &Path) -> PathBuf {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["rev-parse", "--path-format=absolute", "--git-dir"])
        .output()
        .expect("runs");
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
}

fn refused_for(result: Result<RewindDone, Error>, words: &str) {
    match result {
        Err(Error::Rewind(reason)) => assert!(reason.contains(words), "{reason}"),
        other => panic!("not refused for {words}: {other:?}"),
    }
}

#[tokio::test]
async fn claude_code_goes_back_to_before_a_message_and_resumes_where_it_stood() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    task(&core).await;
    let mut bus = core.subscribe();
    let id = started(&core);
    for text in ["one", "two", "three"] {
        say(&core, &mut bus, id, text).await;
    }

    let done = core.session_rewind(id, 1, false).await.expect("rewinds");
    assert_eq!(
        done,
        RewindDone {
            to: 1,
            files: false,
            resumes: true,
            commits_since: None
        }
    );
    let events = core.session_events(id).expect("reads");
    let at = |event: &AgentEvent| matches!(event, AgentEvent::Rewound { .. });
    assert!(matches!(
        events.iter().rfind(|event| at(event)),
        Some(AgentEvent::Rewound { to: 1, files: false, resume_at: Some(point), .. })
            if point == "fake-1-1"
    ));
    // Nothing of it is deleted: the messages taken back are still stored.
    let typed = events
        .iter()
        .filter(|event| matches!(event, AgentEvent::UserMessage { .. }));
    assert_eq!(typed.count(), 3);

    // Its agent was stopped, and starts again at the end of the first turn.
    let resumed = say(&core, &mut bus, id, "resumed").await;
    assert_eq!(said(&resumed), "resumed fake-1 at fake-1-1");
    // A turn has ended there since: from then on it carries on as it is.
    core.session_stop(id);
    let again = say(&core, &mut bus, id, "resumed").await;
    assert_eq!(said(&again), "resumed fake-1 at none");

    // What a rewind dropped cannot be gone back to.
    refused_for(
        core.session_rewind(id, 2, false).await,
        "not in the conversation",
    );
    refused_for(
        core.session_rewind(id, 9, false).await,
        "not in the conversation",
    );

    // Before the first message there is nothing to resume: a new conversation.
    let fresh = core.session_rewind(id, 0, false).await.expect("rewinds");
    assert!(!fresh.resumes);
    assert_eq!(core.session(id).expect("exists").agent_session_id, None);
    let anew = say(&core, &mut bus, id, "resumed").await;
    assert_eq!(said(&anew), "resumed none at none");
}

#[tokio::test]
async fn the_files_go_back_to_before_the_message_and_can_be_put_back() {
    let fx = fixture();
    // Ignored in every worktree of the repository.
    std::fs::write(fx.repo.join(".git/info/exclude"), "*.log\n").expect("test setup");
    let core = core_with_project(&fx).await;
    let task = task(&core).await;
    let worktree = &task.worktree_path;
    let mut bus = core.subscribe();
    let id = started(&core);
    say(&core, &mut bus, id, "write: notes.txt = first").await;
    say(&core, &mut bus, id, "write: notes.txt = second").await;
    say(&core, &mut bus, id, "write: other.txt = new").await;
    // Since then: a file git ignores, and one too large to keep. Neither is touched.
    std::fs::write(worktree.join("build.log"), "log\n").expect("writes");
    let big = std::fs::File::create(worktree.join("dump.bin")).expect("creates");
    big.set_len(MAX_UNTRACKED_BYTES + 1).expect("grows");
    let repository = common_files(&fx.repo.join(".git"));

    let check = core.session_rewind_check(id, 1).await.expect("checks");
    assert_eq!((check.refused, check.files_refused), (None, None));
    assert!(check.resumes);
    assert_eq!(check.commits_since, Some(0));
    let going: Vec<(&str, ChangeKind)> = check
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.kind))
        .collect();
    assert_eq!(
        going,
        [
            ("notes.txt", ChangeKind::Modified),
            ("other.txt", ChangeKind::Deleted)
        ]
    );

    let done = core.session_rewind(id, 1, true).await.expect("rewinds");
    assert!(done.files && done.resumes);
    assert_eq!(done.commits_since, Some(0));
    assert_eq!(read(worktree, "notes.txt").as_deref(), Some("first\n"));
    assert_eq!(read(worktree, "other.txt"), None);
    assert_eq!(read(worktree, "build.log").as_deref(), Some("log\n"));
    let kept = std::fs::metadata(worktree.join("dump.bin")).expect("still there");
    assert_eq!(kept.len(), MAX_UNTRACKED_BYTES + 1);
    // Nothing of the repository's changed: no object, no ref, no index of its own.
    assert_eq!(common_files(&fx.repo.join(".git")), repository);

    core.session_files_put_back(id).await.expect("puts back");
    assert_eq!(read(worktree, "notes.txt").as_deref(), Some("second\n"));
    assert_eq!(read(worktree, "other.txt").as_deref(), Some("new\n"));
    let events = core.session_events(id).expect("reads");
    assert!(matches!(
        events.last(),
        Some(AgentEvent::FilesPutBack { to: 1 })
    ));
    // Only once.
    assert!(matches!(
        core.session_files_put_back(id).await,
        Err(Error::Rewind(_))
    ));
}

#[tokio::test]
async fn an_ignored_file_in_the_way_is_never_written_over() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = task(&core).await;
    let worktree = &task.worktree_path;
    let mut bus = core.subscribe();
    let id = started(&core);
    // Both are in the checkpoint before the second message, then deleted, ignored and made
    // again by the user: a file, and a folder where a file was.
    say(&core, &mut bus, id, "write: .env = made up").await;
    say(&core, &mut bus, id, "write: build = a file").await;
    say(&core, &mut bus, id, "write: notes.txt = later").await;
    std::fs::write(fx.repo.join(".git/info/exclude"), ".env\nbuild\n").expect("test setup");
    std::fs::write(worktree.join(".env"), "the user's own\n").expect("writes");

    refused_for(core.session_rewind(id, 2, true).await, ".env is in the way");
    assert_eq!(read(worktree, ".env").as_deref(), Some("the user's own\n"));
    assert_eq!(read(worktree, "notes.txt").as_deref(), Some("later\n"));

    std::fs::remove_file(worktree.join(".env")).expect("removes");
    std::fs::remove_file(worktree.join("build")).expect("removes");
    std::fs::create_dir(worktree.join("build")).expect("creates");
    std::fs::write(worktree.join("build/out.o"), "built\n").expect("writes");
    refused_for(
        core.session_rewind(id, 2, true).await,
        "build is in the way",
    );
    assert_eq!(read(worktree, "build/out.o").as_deref(), Some("built\n"));
    // Nothing was taken back, the conversation neither.
    let events = core.session_events(id).expect("reads");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AgentEvent::Rewound { .. }))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn files_git_could_not_all_take_back_are_put_as_they_were() {
    use std::os::unix::fs::PermissionsExt;

    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = task(&core).await;
    let worktree = &task.worktree_path;
    let mut bus = core.subscribe();
    let id = started(&core);
    say(&core, &mut bus, id, "write: a.txt = first").await;
    std::fs::create_dir(worktree.join("z")).expect("creates");
    std::fs::write(worktree.join("z/b.txt"), "kept\n").expect("writes");
    say(&core, &mut bus, id, "write: a.txt = second").await;
    // The file that would come back has nowhere to go: its folder takes no new file.
    std::fs::remove_file(worktree.join("z/b.txt")).expect("removes");
    let locked = std::fs::Permissions::from_mode(0o555);
    std::fs::set_permissions(worktree.join("z"), locked).expect("locks");

    let refused = core.session_rewind(id, 1, true).await;
    let open = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(worktree.join("z"), open).expect("unlocks");
    refused_for(refused, "git would not take the files back");
    // What git had written by then is as it was, and nothing is recorded as taken back.
    assert_eq!(read(worktree, "a.txt").as_deref(), Some("second\n"));
    let events = core.session_events(id).expect("reads");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AgentEvent::Rewound { .. }))
    );
    assert!(matches!(
        core.session_files_put_back(id).await,
        Err(Error::Rewind(_))
    ));
}

/// Every file under `dir` with its contents, by path.
fn common_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![];
    let mut waiting = vec![dir.to_path_buf()];
    while let Some(folder) = waiting.pop() {
        let entries = std::fs::read_dir(&folder).expect("reads").flatten();
        let (folders, found): (Vec<_>, Vec<_>) = entries
            .map(|entry| entry.path())
            .partition(|path| path.is_dir());
        waiting.extend(folders);
        files.extend(found.into_iter().map(|path| {
            let bytes = std::fs::read(&path).expect("reads");
            (path.to_string_lossy().into_owned(), bytes)
        }));
    }
    files.sort();
    files
}

#[tokio::test]
async fn files_that_changed_since_they_went_back_are_not_put_back_over() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = task(&core).await;
    let mut bus = core.subscribe();
    let id = started(&core);
    say(&core, &mut bus, id, "write: notes.txt = first").await;
    say(&core, &mut bus, id, "write: notes.txt = second").await;
    core.session_rewind(id, 1, true).await.expect("rewinds");
    std::fs::write(task.worktree_path.join("notes.txt"), "by hand\n").expect("writes");

    let refused = core.session_files_put_back(id).await;
    assert!(
        matches!(&refused, Err(Error::Rewind(reason)) if reason.contains("changed since")),
        "{refused:?}"
    );
    assert_eq!(
        read(&task.worktree_path, "notes.txt").as_deref(),
        Some("by hand\n")
    );
}

#[tokio::test]
async fn commits_made_since_stay_on_the_branch() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = task(&core).await;
    let worktree = &task.worktree_path;
    let mut bus = core.subscribe();
    let id = started(&core);
    say(&core, &mut bus, id, "write: notes.txt = first").await;
    git(worktree, &["add", "notes.txt"]);
    git(worktree, &["commit", "--quiet", "-m", "notes"]);
    let committed = head(worktree);
    say(&core, &mut bus, id, "write: notes.txt = second").await;

    let check = core.session_rewind_check(id, 0).await.expect("checks");
    assert_eq!(check.commits_since, Some(1));
    let done = core.session_rewind(id, 0, true).await.expect("rewinds");
    assert_eq!(done.commits_since, Some(1));
    // The branch is where it was; only the files went back.
    assert_eq!(head(worktree), committed);
    assert_eq!(read(worktree, "notes.txt"), None);
}

#[tokio::test]
async fn nothing_goes_back_while_an_agent_could_write_into_the_worktree() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    task(&core).await;
    let mut bus = core.subscribe();
    let quiet = started(&core);
    let busy = started(&core);
    say(&core, &mut bus, quiet, "write: notes.txt = first").await;

    // The session itself at work.
    core.session_send(busy, "slow").expect("sends");
    while !matches!(next(&mut bus).await.event, AgentEvent::Text { .. }) {}
    refused_for(core.session_rewind(busy, 0, false).await, "at work");
    // Another of the task's: its files cannot go back, its conversation can.
    let check = core.session_rewind_check(quiet, 0).await.expect("checks");
    assert!(check.refused.is_none());
    let files = check.files_refused.expect("refused");
    assert!(files.contains("is at work in this worktree"), "{files}");
    refused_for(core.session_rewind(quiet, 0, true).await, "at work");
    core.session_interrupt(busy).expect("stops");
    turn(&mut bus, busy).await;

    // Something an agent left running.
    say(&core, &mut bus, busy, "background bash").await;
    refused_for(
        core.session_rewind(busy, 0, false).await,
        "left something running",
    );
    refused_for(core.session_rewind(quiet, 0, true).await, "left running");
    core.session_rewind(quiet, 0, false)
        .await
        .expect("the conversation alone");
}

#[tokio::test]
async fn files_are_not_changed_in_the_middle_of_a_rebase() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = task(&core).await;
    let mut bus = core.subscribe();
    let id = started(&core);
    say(&core, &mut bus, id, "write: notes.txt = first").await;
    let state = git_dir(&task.worktree_path).join("rebase-merge");
    std::fs::create_dir_all(&state).expect("test setup");

    let check = core.session_rewind_check(id, 0).await.expect("checks");
    let refused = check.files_refused.expect("refused");
    assert!(refused.contains("a rebase is under way"), "{refused}");
    refused_for(
        core.session_rewind(id, 0, true).await,
        "a rebase is under way",
    );
    assert_eq!(
        read(&task.worktree_path, "notes.txt").as_deref(),
        Some("first\n")
    );
    std::fs::remove_dir_all(&state).expect("test setup");
    core.session_rewind(id, 0, true).await.expect("rewinds");
}

#[tokio::test]
async fn files_without_a_checkpoint_are_refused_and_a_chat_has_none() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    task(&core).await;
    let mut bus = core.subscribe();
    let id = started(&core);
    say(&core, &mut bus, id, "write: notes.txt = first").await;
    // A fork's messages were copied, and none of them went through a checkpoint there.
    let fork = core.session_fork(id, None, None).expect("forks");
    assert!(matches!(
        core.session_rewind(fork.id, 0, true).await,
        Err(Error::TurnNotKept { message: 0, .. })
    ));
    core.session_rewind(fork.id, 0, false)
        .await
        .expect("the conversation alone");

    let chat = core
        .task_new(CHATS, "hello", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("a chat");
    let talking = core
        .session_new(CHATS, &chat.name, &SessionOptions::default())
        .expect("creates");
    say(&core, &mut bus, talking.id, "hi").await;
    assert!(matches!(
        core.session_rewind(talking.id, 0, true).await,
        Err(Error::NoRepository)
    ));
    core.session_rewind(talking.id, 0, false)
        .await
        .expect("the conversation alone");

    core.session_close(id).expect("closes");
    assert!(matches!(
        core.session_rewind(id, 0, false).await,
        Err(Error::SessionClosed(_))
    ));
}

#[tokio::test]
async fn a_session_another_swarmed_runs_is_not_taken_back_here() {
    let fx = fixture();
    let here = core_with_project(&fx).await;
    task(&here).await;
    let there =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let session = here
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    let mut theirs = there.subscribe();
    there
        .session_message(session.id, "hi", &Attached::default(), Auth::Subscription)
        .expect("sends");
    turn(&mut theirs, session.id).await;

    assert!(matches!(
        here.session_rewind(session.id, 0, false).await,
        Err(Error::RunsElsewhere(id)) if id == session.id
    ));
}

#[tokio::test]
async fn a_fork_of_a_session_taken_back_leaves_out_what_was_dropped() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    task(&core).await;
    let mut bus = core.subscribe();
    let id = started(&core);
    for text in ["one", "two"] {
        say(&core, &mut bus, id, text).await;
    }
    core.session_rewind(id, 1, false).await.expect("rewinds");
    say(&core, &mut bus, id, "three").await;

    let fork = core.session_fork(id, None, None).expect("forks");
    let copied: Vec<String> = core
        .session_events(fork.id)
        .expect("reads")
        .into_iter()
        .filter_map(|event| match event {
            AgentEvent::UserMessage { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(copied, ["one", "three"]);
}

const AGENT: &str = "scripted";

/// A core with the scripted ACP agent registered next to (fake) Claude Code.
#[tokio::test]
async fn another_session_reads_only_what_still_counts() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    task(&core).await;
    let mut bus = core.subscribe();
    let id = started(&core);
    let reader = started(&core);
    say(&core, &mut bus, id, "keep the first").await;
    say(&core, &mut bus, id, "then the second").await;
    core.session_rewind(id, 1, false).await.expect("rewinds");

    let read = core.peer_digest(reader, id, None, None).expect("reads");
    assert!(read.contains("keep the first"), "{read}");
    assert!(!read.contains("then the second"), "{read}");
}

async fn core_with_acp(fx: &common::Fixture) -> Core {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_agent.py");
    let backend = AcpBackend {
        program: "python3".into(),
        args: vec![script.to_string_lossy().into_owned()],
        env: vec![],
        default_model: None,
    };
    let mut core =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("opens");
    let info = AgentInfo {
        id: AGENT.into(),
        name: "Scripted".into(),
        models: vec![],
        default_model: None,
        hears_mid_turn: false,
        sends_to_background: false,
    };
    core.add_agent(info, Arc::new(backend));
    core.project_add(&fx.repo, None).await.expect("test setup");
    core
}

#[tokio::test]
async fn an_acp_agent_starts_a_new_conversation_told_what_was_said_up_to_the_message() {
    let fx = fixture();
    let core = core_with_acp(&fx).await;
    task(&core).await;
    let mut bus = core.subscribe();
    let options = SessionOptions {
        agent: Some(AGENT.into()),
        ..Default::default()
    };
    let id = core
        .session_new("demo", "one", &options)
        .expect("creates")
        .id;
    say(&core, &mut bus, id, "the password is swordfish").await;
    say(&core, &mut bus, id, "the colour is teal").await;
    assert!(core.session(id).expect("exists").agent_session_id.is_some());

    let done = core.session_rewind(id, 1, false).await.expect("rewinds");
    assert!(!done.resumes);
    assert_eq!(core.session(id).expect("exists").agent_session_id, None);

    // The scripted agent repeats what it is sent: the message, then what was said before it.
    let heard = said(&say(&core, &mut bus, id, "what was it?").await);
    assert!(heard.contains("what was it?"), "{heard}");
    assert!(heard.contains("swordfish"), "{heard}");
    assert!(!heard.contains("teal"), "{heard}");
    // Told once: a turn there ended well.
    let next_turn = said(&say(&core, &mut bus, id, "and now?").await);
    assert!(!next_turn.contains("swordfish"), "{next_turn}");
}
