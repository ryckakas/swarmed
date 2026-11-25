//! A task started from a prompt, and how its setup is followed, stopped, limited and told.

mod common;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{core_with_project, fixture, turn, write_config};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    Core, Error, FirstSession, NewTaskOptions, RemoveOptions, SessionEvent, SessionOptions,
    SettingUp, SetupGoing, SetupProgress, SetupStatus, SetupStep, SetupStop, TaskStarted, hooks,
};
use tokio::sync::broadcast::Receiver;

/// A setup that waits for a file called `go` in the worktree, then prints and ends.
const WAITS_FOR_GO: &str = "[hooks]\nsetup = \"echo installing; while [ ! -f go ]; do sleep 0.05; done; echo installed\"\n";

fn first(prompt: &str) -> FirstSession {
    FirstSession {
        options: SessionOptions::default(),
        prompt: prompt.to_string(),
        files: vec![],
        sessions: vec![],
        github: vec![],
        images: vec![],
    }
}

/// How the task's setup stands, as the sidebar is told; `None` too for a task not written
/// down yet.
fn setting_up(core: &Core, task: &str) -> Option<SettingUp> {
    let overview = core.overview().expect("reads the tree");
    let mut tasks = overview.into_iter().flat_map(|project| project.tasks);
    tasks
        .find(|overview| overview.task.name == task)
        .and_then(|task| task.setting_up)
}

/// Waits until `check` holds, for as long as a setup may take to get there on a slow machine.
async fn soon(mut check: impl FnMut() -> bool) {
    for _ in 0..400 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("it never came to pass");
}

fn step_of(core: &Core, task: &str) -> Option<SetupStep> {
    match setting_up(core, task) {
        Some(SettingUp::Here { step, .. }) => Some(step),
        _ => None,
    }
}

fn said_by_user(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::UserMessage { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_task_started_from_a_prompt_has_its_session_at_once_and_sends_the_prompt_once_set_up() {
    let fx = fixture();
    write_config(&fx, WAITS_FOR_GO);
    let core = Arc::new(core_with_project(&fx).await);
    let mut bus = core.subscribe();
    let (started, going) = started(&core, "one", "fix the login").await;

    // The session is there before the setup is done, named after the prompt, with the prompt
    // waiting in its queue and nothing said yet.
    let session = started.session.id;
    assert_eq!(started.session.title, "fix the login");
    assert!(core.session(session).expect("reads").waits_for_setup);
    let queue = core.session_queue(session).expect("reads the queue");
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].text, "fix the login");
    soon(|| step_of(&core, "one") == Some(SetupStep::Hook)).await;
    let log = core.task_setup_log("demo", "one").expect("reads the log");
    let said = said_by_user(&core.session_events(session).expect("reads"));
    assert_eq!(said, Vec::<String>::new());

    std::fs::write(started.task.worktree_path.join("go"), "").expect("test setup");
    let task = going.finished().await.expect("sets up");
    assert_eq!(task.setup, SetupStatus::Ok);
    let events = turn(&mut bus, session).await;
    assert_eq!(said_by_user(&events), ["fix the login"]);
    assert!(!core.session(session).expect("reads").waits_for_setup);
    assert_eq!(core.session_queue(session).expect("reads"), vec![]);
    assert_eq!(setting_up(&core, "one"), None);

    // What it printed is in its log, which reads on from where the first read ended.
    let whole = core.task_setup_log("demo", "one").expect("reads the log");
    assert!(
        whole.text.contains("installing\ninstalled\n"),
        "{}",
        whole.text
    );
    assert!(whole.text.contains("setup finished in"), "{}", whole.text);
    assert!(whole.end >= log.end);
}

#[tokio::test]
async fn what_a_setup_prints_is_published_as_it_goes_with_its_steps_and_its_end() {
    let fx = fixture();
    write_config(&fx, "[hooks]\nsetup = \"echo one; echo two\"\n");
    let core = Arc::new(core_with_project(&fx).await);
    let mut bus = core.subscribe();
    let (started, going) = started(&core, "one", "hello").await;
    going.finished().await.expect("sets up");

    let mut output = String::new();
    let mut steps = vec![];
    let reports = reports_to_the_end(&mut bus).await;
    for progress in &reports {
        assert_eq!(progress.task_id, started.task.id);
        assert_eq!(progress.from, output.len() as u64);
        output.push_str(&progress.output);
        steps.push(progress.step);
    }
    let ended = reports.last().and_then(|last| last.ended.clone());
    assert_eq!(ended, Some(swarmed_core::SetupEnd::Done));
    assert!(steps.contains(&SetupStep::Hook), "{steps:?}");
    assert!(output.starts_with("one\ntwo\n"), "{output}");
    let log = core.task_setup_log("demo", "one").expect("reads");
    assert_eq!(log.text, output);
}

#[tokio::test]
async fn a_failed_setup_says_why_and_keeps_the_prompt_through_a_restart_until_sent_anyway() {
    let fx = fixture();
    write_config(&fx, "[hooks]\nsetup = \"echo about to fail; exit 3\"\n");
    let core = Arc::new(core_with_project(&fx).await);
    let (started, going) = started(&core, "one", "fix the login").await;
    let failed = going.finished().await;
    assert!(
        matches!(failed, Err(Error::HookFailed { .. })),
        "{failed:?}"
    );

    let (_, task) = core.task("demo", "one").expect("the task stays");
    assert_eq!(task.setup, SetupStatus::Failed);
    let reason = task.setup_reason.expect("says why");
    assert!(reason.contains("exited with status 3"), "{reason}");
    let log = core.task_setup_log("demo", "one").expect("reads");
    assert!(log.text.contains("about to fail\n"), "{}", log.text);
    assert!(log.text.contains(&reason), "{}", log.text);
    let session = started.session.id;
    assert_eq!(core.session_queue(session).expect("reads").len(), 1);

    // Swarmed quits; the prompt is still there when it starts again.
    drop(core);
    let core = Core::with_backend(
        fx.paths.clone(),
        Arc::new(swarmed_core::agent::FakeBackend::default()),
    )
    .expect("reopens");
    let mut bus = core.subscribe();
    assert!(core.session(session).expect("reads").waits_for_setup);
    let queue = core.session_queue(session).expect("reads the queue");
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].text, "fix the login");

    let sent = core
        .task_start_anyway("demo", "one", &Auth::default())
        .expect("sends anyway");
    assert_eq!(sent, [session]);
    let events = turn(&mut bus, session).await;
    assert_eq!(said_by_user(&events), ["fix the login"]);
    assert!(!core.session(session).expect("reads").waits_for_setup);
    let again = core.task_start_anyway("demo", "one", &Auth::default());
    assert!(
        matches!(again, Err(Error::NothingWaitsForSetup(_))),
        "{again:?}"
    );
}

#[tokio::test]
async fn setting_up_again_sends_what_waits_once_it_finishes_well() {
    let fx = fixture();
    write_config(&fx, "[hooks]\nsetup = \"test -f go\"\n");
    let core = Arc::new(core_with_project(&fx).await);
    let mut bus = core.subscribe();
    let (started, going) = started(&core, "one", "carry on").await;
    assert!(going.finished().await.is_err());
    // Edited while it waits: what goes is what it says now.
    let session = started.session.id;
    let waiting = core.session_queue(session).expect("reads")[0].id;
    core.session_queue_edit(session, waiting, "carry on, carefully")
        .expect("edits");

    std::fs::write(started.task.worktree_path.join("go"), "").expect("test setup");
    let task = core
        .task_setup("demo", "one", Some(Auth::default()), &hooks::discard)
        .await
        .expect("sets up");
    assert_eq!((task.setup, task.setup_reason), (SetupStatus::Ok, None));
    let events = turn(&mut bus, session).await;
    assert_eq!(said_by_user(&events), ["carry on, carefully"]);
}

#[tokio::test]
async fn setting_up_again_after_a_restart_sends_what_waits() {
    let fx = fixture();
    write_config(&fx, "[hooks]\nsetup = \"test -f go\"\n");
    let core = Arc::new(core_with_project(&fx).await);
    let (started, going) = started(&core, "one", "carry on").await;
    assert!(going.finished().await.is_err());

    // Swarmed quits, and the setup is run again once it has started: the prompt was only in
    // the database, and goes once.
    drop(core);
    let core = Core::with_backend(
        fx.paths.clone(),
        Arc::new(swarmed_core::agent::FakeBackend::default()),
    )
    .expect("reopens");
    let mut bus = core.subscribe();
    std::fs::write(started.task.worktree_path.join("go"), "").expect("test setup");
    let task = core
        .task_setup("demo", "one", Some(Auth::default()), &hooks::discard)
        .await
        .expect("sets up");
    assert_eq!(task.setup, SetupStatus::Ok);
    let session = started.session.id;
    let events = turn(&mut bus, session).await;
    assert_eq!(said_by_user(&events), ["carry on"]);
    assert_eq!(core.session_queue(session).expect("reads"), vec![]);
}

/// A task with no setup sends its prompt at once, while the window opens its session and
/// reads its queue. That read must not find the prompt as it goes and put it back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_look_at_the_queue_as_the_prompt_goes_does_not_put_it_back() {
    let fx = fixture();
    let core = Arc::new(core_with_project(&fx).await);
    let mut bus = core.subscribe();
    // When the prompt goes is a matter of timing: tried a few times, with the queue read over
    // and over meanwhile.
    for round in 0..10 {
        let (started, going) = started(&core, &format!("task-{round}"), "fix the login").await;
        let session = started.session.id;
        let looking = Arc::new(AtomicBool::new(true));
        let looker = look_at_queue(&core, session, &looking);
        going.finished().await.expect("sets up");
        turn(&mut bus, session).await;
        looking.store(false, Ordering::Relaxed);
        looker.join().expect("stops looking");
        let queue = core.session_queue(session).expect("reads");
        assert_eq!(queue, vec![], "round {round}");
        let said = said_by_user(&core.session_events(session).expect("reads"));
        assert_eq!(said, ["fix the login"], "round {round}");
    }
}

/// Reads session `id`'s queue over and over on a thread of its own while `looking` holds.
fn look_at_queue(
    core: &Arc<Core>,
    id: i64,
    looking: &Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    let (core, looking) = (Arc::clone(core), Arc::clone(looking));
    std::thread::spawn(move || {
        while looking.load(Ordering::Relaxed) {
            core.session_queue(id).ok();
            // Lets the sender have the store between two looks.
            std::thread::yield_now();
        }
    })
}

#[tokio::test]
async fn a_session_made_while_its_task_is_set_up_sends_what_it_waited_to_send_too() {
    let fx = fixture();
    write_config(&fx, WAITS_FOR_GO);
    let core = Arc::new(core_with_project(&fx).await);
    let mut bus = core.subscribe();
    let (started, going) = started(&core, "one", "fix the login").await;
    soon(|| step_of(&core, "one") == Some(SetupStep::Hook)).await;
    let second = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("makes a second session");
    core.session_enqueue(second.id, "and the tests")
        .expect("waits");

    std::fs::write(started.task.worktree_path.join("go"), "").expect("test setup");
    going.finished().await.expect("sets up");
    let events = turn(&mut bus, second.id).await;
    assert_eq!(said_by_user(&events), ["and the tests"]);
}

#[tokio::test]
async fn a_prompt_another_swarmed_sent_is_not_sent_again_from_this_ones_queue() {
    let fx = fixture();
    write_config(&fx, "[hooks]\nsetup = \"exit 3\"\n");
    let core = Arc::new(core_with_project(&fx).await);
    let (started, going) = started(&core, "one", "fix the login").await;
    assert!(going.finished().await.is_err());
    let session = started.session.id;

    // Another Swarmed, the command line say, sends it and ends.
    let other = Core::with_backend(
        fx.paths.clone(),
        Arc::new(swarmed_core::agent::FakeBackend::default()),
    )
    .expect("opens beside it");
    let mut others = other.subscribe();
    other
        .task_start_anyway("demo", "one", &Auth::default())
        .expect("sends anyway");
    turn(&mut others, session).await;
    drop(other);

    // This one still has it waiting; the end of its next turn drops it.
    assert_eq!(core.session_queue(session).expect("reads").len(), 1);
    let mut bus = core.subscribe();
    core.session_start(session, Auth::default())
        .expect("starts");
    core.session_send(session, "and the tests").expect("sends");
    turn(&mut bus, session).await;
    soon(|| core.session_queue(session).expect("reads").is_empty()).await;
    let said = said_by_user(&core.session_events(session).expect("reads"));
    assert_eq!(said, ["fix the login", "and the tests"]);
}

#[tokio::test]
async fn a_setup_stopped_by_the_user_leaves_the_task_failed_and_the_prompt_waiting() {
    let fx = fixture();
    write_config(
        &fx,
        "[hooks]\nsetup = \"sleep 300 & echo $! > sleeping.pid; wait\"\n",
    );
    let core = Arc::new(core_with_project(&fx).await);
    let (started, going) = started(&core, "one", "hello").await;
    let pid = written_soon(&started.task.worktree_path.join("sleeping.pid")).await;
    core.task_setup_stop("demo", "one").await.expect("stops");
    let stopped = going.finished().await;
    assert!(
        matches!(
            stopped,
            Err(Error::SetupStopped {
                why: SetupStop::User,
                ..
            })
        ),
        "{stopped:?}"
    );
    assert!(!alive(&pid), "process {pid} still runs");
    let (_, task) = core.task("demo", "one").expect("the task stays");
    assert_eq!(task.setup, SetupStatus::Failed);
    assert_eq!(task.setup_reason.as_deref(), Some("Stopped by you."));
    assert!(
        core.session(started.session.id)
            .expect("reads")
            .waits_for_setup
    );
    // Stopping what does not run does nothing.
    core.task_setup_stop("demo", "one")
        .await
        .expect("nothing to stop");
}

#[tokio::test]
async fn setups_over_the_projects_limit_wait_for_a_place_and_say_so() {
    let fx = fixture();
    write_config(&fx, &format!("max_setups = 1\n{WAITS_FOR_GO}"));
    let core = Arc::new(core_with_project(&fx).await);
    let (one, one_going) = started(&core, "one", "hello").await;
    soon(|| step_of(&core, "one") == Some(SetupStep::Hook)).await;
    let (two, two_going) = started(&core, "two", "hello").await;
    soon(|| step_of(&core, "two") == Some(SetupStep::Waiting)).await;
    std::fs::write(two.task.worktree_path.join("go"), "").expect("test setup");
    // Its hook has not begun: it waits for the first.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(step_of(&core, "two"), Some(SetupStep::Waiting));

    std::fs::write(one.task.worktree_path.join("go"), "").expect("test setup");
    one_going.finished().await.expect("sets up the first");
    two_going.finished().await.expect("sets up the second");
}

#[tokio::test]
async fn a_setup_nothing_runs_any_more_is_told_from_one_another_swarmed_runs() {
    let fx = fixture();
    write_config(&fx, WAITS_FOR_GO);
    let core = core_with_project(&fx).await;
    let other = Core::with_backend(
        fx.paths.clone(),
        Arc::new(swarmed_core::agent::FakeBackend::default()),
    )
    .expect("opens beside it");
    let making = core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard);
    // Given up halfway, as by a Swarmed that ended: the setup is never finished.
    let looked = async {
        soon(|| step_of(&core, "one") == Some(SetupStep::Hook)).await;
        assert_eq!(setting_up(&other, "one"), Some(SettingUp::Elsewhere));
        let again = other.task_setup("demo", "one", None, &hooks::discard).await;
        assert!(matches!(again, Err(Error::SetupRunning(_))), "{again:?}");
    };
    // Given up only once it was looked at, however long the setup took to get there.
    tokio::select! {
        made = making => panic!("the setup ended: {made:?}"),
        () = looked => {}
    }
    assert_eq!(setting_up(&core, "one"), Some(SettingUp::Interrupted));
    assert_eq!(setting_up(&other, "one"), Some(SettingUp::Interrupted));
}

#[tokio::test]
async fn a_chat_started_from_a_prompt_sends_it_at_once() {
    let fx = fixture();
    let core = Arc::new(core_with_project(&fx).await);
    let mut bus = core.subscribe();
    let (started, going) = core
        .task_start(
            swarmed_core::CHATS,
            "question",
            NewTaskOptions::default(),
            first("what is a worktree?"),
            Auth::default(),
            &hooks::discard,
        )
        .await
        .expect("starts the chat");
    let events = turn(&mut bus, started.session.id).await;
    assert_eq!(said_by_user(&events), ["what is a worktree?"]);
    assert_eq!(going.finished().await.expect("done").setup, SetupStatus::Ok);
}

#[tokio::test]
async fn removing_a_task_deletes_its_setup_log() {
    let fx = fixture();
    write_config(&fx, "[hooks]\nsetup = \"echo secret-ish\"\n");
    let core = core_with_project(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let logs = fx.paths.data_dir.join("run").join("setups");
    let log = std::fs::read_dir(&logs)
        .expect("a folder of logs")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "log"))
        .expect("a log");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&log).expect("reads").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let options = RemoveOptions {
        force: true,
        delete_branch: false,
    };
    core.task_remove("demo", "one", options, &hooks::discard)
        .await
        .expect("removes");
    assert!(!log.exists());
}

/// Starts task `name` of the project `demo` with `prompt`.
async fn started(core: &Arc<Core>, name: &str, prompt: &str) -> (TaskStarted, SetupGoing) {
    let options = NewTaskOptions::default();
    let starting = core.task_start(
        "demo",
        name,
        options,
        first(prompt),
        Auth::default(),
        &hooks::discard,
    );
    starting.await.expect("starts the task")
}

/// The reports of a setup heard on `bus`, up to the one that says how it ended. Each comes
/// with session id 0, which no session has.
async fn reports_to_the_end(bus: &mut Receiver<SessionEvent>) -> Vec<SetupProgress> {
    let mut reports: Vec<SetupProgress> = vec![];
    while reports.last().is_none_or(|last| last.ended.is_none()) {
        reports.extend(report_in(common::next(bus).await));
    }
    reports
}

fn report_in(heard: SessionEvent) -> Option<SetupProgress> {
    match heard.event {
        AgentEvent::Setup { progress } if heard.session_id == 0 => Some(progress),
        _ => None,
    }
}

/// Waits for `file` to be written and returns what it holds.
async fn written_soon(file: &Path) -> String {
    for _ in 0..400 {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        if !text.trim().is_empty() {
            return text.trim().to_string();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{} was never written", file.display());
}

fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
