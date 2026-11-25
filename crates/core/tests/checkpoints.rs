//! Checkpoints: the worktree before each message and after each turn, kept outside the
//! repository, and what one turn changed read back from them.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    CHATS, ChangeKind, Core, Error, FileChange, MAX_UNTRACKED_BYTES, NewTaskOptions, RemoveOptions,
    Session, SessionOptions, Task, hooks,
};
use tokio::sync::broadcast::Receiver;

async fn task(core: &Core) -> Task {
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup")
}

fn started(core: &Core, project: &str, task: &str) -> Session {
    let session = core
        .session_new(project, task, &SessionOptions::default())
        .expect("test setup");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    session
}

async fn say(core: &Core, bus: &mut Receiver<swarmed_core::SessionEvent>, id: i64, text: &str) {
    core.session_send(id, text).expect("sends");
    turn(bus, id).await;
}

/// Every file under `dir` with its contents, by path.
fn files_under(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut waiting = vec![dir.to_path_buf()];
    while let Some(folder) = waiting.pop() {
        let entries = std::fs::read_dir(&folder).expect("reads").flatten();
        let (folders, found): (Vec<_>, Vec<_>) = entries
            .map(|entry| entry.path())
            .partition(|path| path.is_dir());
        waiting.extend(folders);
        files.extend(found.into_iter().map(|path| {
            let relative = path.strip_prefix(dir).expect("inside");
            let bytes = std::fs::read(&path).expect("reads");
            (relative.to_string_lossy().into_owned(), bytes)
        }));
    }
    files
}

/// Waits for the end of session `id`'s turn.
async fn turn_end(bus: &mut Receiver<swarmed_core::SessionEvent>, id: i64) {
    loop {
        let published = next(bus).await;
        let ended = matches!(published.event, AgentEvent::TurnDone { .. });
        if published.session_id == id && ended {
            return;
        }
    }
}

fn change(path: &str, kind: ChangeKind, added: u32, removed: u32) -> FileChange {
    FileChange {
        path: path.to_string(),
        kind,
        added: Some(added),
        removed: Some(removed),
    }
}

#[tokio::test]
async fn each_turn_is_kept_outside_the_repository_and_read_on_its_own() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = task(&core).await;
    let mut bus = core.subscribe();
    let session = started(&core, "demo", "one");
    let repository = files_under(&fx.repo.join(".git"));

    say(&core, &mut bus, session.id, "write: notes.txt = first").await;
    // Written by hand between the turns: in neither of them.
    std::fs::write(task.worktree_path.join("hand.txt"), "by hand\n").expect("writes");
    say(&core, &mut bus, session.id, "write: notes.txt = second").await;

    // Nothing of the repository's changed: no object, no ref, no index, no log.
    assert_eq!(files_under(&fx.repo.join(".git")), repository);
    assert!(fx.paths.checkpoints(task.id).join("objects").is_dir());

    let turns = core.session_turns(session.id).expect("lists");
    let listed: Vec<(usize, &str, bool)> = turns
        .iter()
        .map(|turn| (turn.message, turn.words.as_str(), turn.ended))
        .collect();
    assert_eq!(
        listed,
        [
            (0, "write: notes.txt = first", true),
            (1, "write: notes.txt = second", true)
        ]
    );
    assert!(turns.iter().all(|turn| turn.beside.is_empty()));

    let first = core
        .session_turn_changes(session.id, 0)
        .await
        .expect("reads");
    assert_eq!(first.files, [change("notes.txt", ChangeKind::Added, 1, 0)]);
    assert!(first.ended && first.skipped.is_empty());
    let second = core
        .session_turn_changes(session.id, 1)
        .await
        .expect("reads");
    assert_eq!(
        second.files,
        [change("notes.txt", ChangeKind::Modified, 1, 1)]
    );

    let diff = core
        .session_turn_diff(session.id, 1, "notes.txt")
        .await
        .expect("reads");
    assert!(diff.text.contains("-first\n+second\n"), "{}", diff.text);
    assert!(matches!(
        core.session_turn_diff(session.id, 1, "../outside").await,
        Err(Error::PathOutsideWorktree(_))
    ));
    assert!(matches!(
        core.session_turn_changes(session.id, 7).await,
        Err(Error::TurnNotKept { message: 7, .. })
    ));
}

#[tokio::test]
async fn an_untracked_file_too_large_is_left_out_and_said() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = task(&core).await;
    let mut bus = core.subscribe();
    let session = started(&core, "demo", "one");
    let size = MAX_UNTRACKED_BYTES + 1;
    let big = std::fs::File::create(task.worktree_path.join("dump.bin")).expect("creates");
    big.set_len(size).expect("grows");

    say(&core, &mut bus, session.id, "write: small.txt = kept").await;

    let changes = core
        .session_turn_changes(session.id, 0)
        .await
        .expect("reads");
    assert_eq!(
        changes.files,
        [change("small.txt", ChangeKind::Added, 1, 0)]
    );
    let skipped: Vec<(&str, u64)> = changes
        .skipped
        .iter()
        .map(|file| (file.path.as_str(), file.bytes))
        .collect();
    assert_eq!(skipped, [("dump.bin", size)]);
    // Never hashed into the store: it holds far less than the file.
    let store = files_under(&fx.paths.checkpoints(task.id));
    let kept: usize = store.values().map(Vec::len).sum();
    assert!(
        (kept as u64) < MAX_UNTRACKED_BYTES / 10,
        "{kept} bytes kept"
    );
}

#[tokio::test]
async fn a_turn_under_way_is_read_up_to_now_and_names_who_else_was_at_work() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = task(&core).await;
    let mut bus = core.subscribe();
    let slow = started(&core, "demo", "one");
    let quick = started(&core, "demo", "one");

    core.session_send(slow.id, "slow").expect("sends");
    std::fs::write(task.worktree_path.join("meanwhile.txt"), "x\n").expect("writes");
    let turns = core.session_turns(slow.id).expect("lists");
    assert_eq!(turns.len(), 1);
    assert!(!turns[0].ended);
    let so_far = core.session_turn_changes(slow.id, 0).await.expect("reads");
    assert!(!so_far.ended);
    assert_eq!(
        so_far.files,
        [change("meanwhile.txt", ChangeKind::Added, 1, 0)]
    );

    say(&core, &mut bus, quick.id, "write: quick.txt = done").await;
    let quick_turns = core.session_turns(quick.id).expect("lists");
    assert_eq!(quick_turns[0].beside, ["slow"]);

    core.session_interrupt(slow.id).expect("stops");
    turn_end(&mut bus, slow.id).await;
    let ended = core.session_turns(slow.id).expect("lists");
    assert!(ended[0].ended);
    // The quick session's turn is in the slow one's: the trees cannot tell who wrote it.
    assert_eq!(ended[0].beside, [quick_turns_title(&core, quick.id)]);
    let whole = core.session_turn_changes(slow.id, 0).await.expect("reads");
    let paths: Vec<&str> = whole.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["meanwhile.txt", "quick.txt"]);
}

fn quick_turns_title(core: &Core, id: i64) -> String {
    core.session(id).expect("exists").title
}

#[tokio::test]
async fn a_chat_keeps_no_checkpoints() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let options = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    let chat = core
        .task_new(CHATS, "question", options, &hooks::discard)
        .await
        .expect("makes a chat");
    let mut bus = core.subscribe();
    let session = started(&core, CHATS, &chat.name);
    say(&core, &mut bus, session.id, "write: answer.txt = 42").await;

    assert_eq!(core.session_turns(session.id).expect("lists"), vec![]);
    assert!(!fx.paths.checkpoints(chat.id).exists());
}

#[tokio::test]
async fn removing_a_task_deletes_its_checkpoints() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let task = task(&core).await;
    let mut bus = core.subscribe();
    let session = started(&core, "demo", "one");
    say(&core, &mut bus, session.id, "write: notes.txt = first").await;
    assert!(fx.paths.checkpoints(task.id).exists());

    let options = RemoveOptions {
        force: true,
        delete_branch: false,
    };
    core.task_remove("demo", "one", options, &hooks::discard)
        .await
        .expect("removes");
    assert!(!fx.paths.checkpoints(task.id).exists());
    assert_eq!(core.session_turns(session.id).expect("lists"), vec![]);
}
