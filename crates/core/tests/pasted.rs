//! Images pasted into a message: kept as the session's own, handed on to a fork, and gone with
//! the session.

mod common;

use std::path::PathBuf;

use common::{core_with_project, fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    Attached, CHATS, Core, Error, NewTaskOptions, RemoveOptions, Session, SessionOptions, hooks,
};

/// The first bytes of a PNG, which is all the core looks at, in base64.
const PNG: &str = "iVBORw0KGgo=";
/// The first bytes of a GIF, in base64.
const GIF: &str = "R0lGODlhAQABAA==";

async fn session_in(core: &Core, project: &str, task: &str) -> Session {
    let options = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    core.task_new(project, task, options, &hooks::discard)
        .await
        .expect("makes the task");
    core.session_new(project, task, &SessionOptions::default())
        .expect("makes a session")
}

fn messages(core: &Core, id: i64) -> Vec<String> {
    let events = core.session_events(id).expect("reads");
    let said = events.into_iter().filter_map(|event| match event {
        AgentEvent::UserMessage { text, .. } => Some(text),
        _ => None,
    });
    said.collect()
}

fn attached(text: &str, files: &[PathBuf]) -> String {
    let listed: Vec<String> = files
        .iter()
        .map(|file| format!("- {}", file.display()))
        .collect();
    format!("{text}\n\nAttached files:\n{}", listed.join("\n"))
}

#[tokio::test]
async fn pasted_images_are_kept_in_the_sessions_own_folder_and_attached_by_their_paths() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let session = session_in(&core, "demo", "one").await;
    let folder = fx.paths.attachments(session.id);
    assert_eq!(core.attachments_root().join(session.id.to_string()), folder);

    let kept = core
        .session_images_keep(session.id, &[PNG.to_string(), GIF.to_string()])
        .expect("kept");
    assert_eq!(
        kept,
        [folder.join("pasted-1.png"), folder.join("pasted-2.gif")]
    );
    let shown = core.session_image(session.id, &kept[1]).expect("read");
    assert_eq!(shown, format!("data:image/gif;base64,{GIF}"));
    // Nothing is written into the worktree; nothing of another session's is read.
    let other = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("makes a session");
    assert!(matches!(
        core.session_image(other.id, &kept[0]),
        Err(Error::NotPastedImage(_))
    ));
    // One image refused keeps none; a session that is not there keeps none either.
    let refused = core.session_images_keep(session.id, &[PNG.to_string(), "SUkqAA==".into()]);
    assert!(matches!(refused, Err(Error::ImageNotRead(what)) if what == "a TIFF image"));
    assert!(core.session_images_keep(999, &[PNG.to_string()]).is_err());
    assert!(!fx.paths.attachments(999).exists());
    let entries = std::fs::read_dir(&folder).expect("lists").count();
    assert_eq!(entries, 2);

    let mut bus = core.subscribe();
    core.session_message(
        session.id,
        "what is wrong here?",
        &Attached::files(&kept),
        Auth::Subscription,
    )
    .expect("sends");
    turn(&mut bus, session.id).await;
    assert_eq!(
        messages(&core, session.id),
        [attached("what is wrong here?", &kept)]
    );
}

#[tokio::test]
async fn a_fork_has_copies_of_its_own_and_the_first_session_takes_its_images_along() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let source = session_in(&core, "demo", "one").await;
    let mut bus = core.subscribe();
    let first = core
        .session_images_keep(source.id, &[PNG.to_string()])
        .expect("kept");
    core.session_message(
        source.id,
        "before",
        &Attached::files(&first),
        Auth::Subscription,
    )
    .expect("sends");
    turn(&mut bus, source.id).await;
    let second = core
        .session_images_keep(source.id, &[GIF.to_string()])
        .expect("kept");
    core.session_message(
        source.id,
        "after",
        &Attached::files(&second),
        Auth::Subscription,
    )
    .expect("sends");
    turn(&mut bus, source.id).await;

    // From the second message: only what it names is copied, and it names the copy.
    let fork = core.session_fork(source.id, Some(1), None).expect("forks");
    let copy = fx.paths.attachments(fork.id).join("pasted-2.gif");
    assert_eq!(
        messages(&core, fork.id),
        [attached("after", std::slice::from_ref(&copy))]
    );
    assert!(copy.exists());
    assert!(!fx.paths.attachments(fork.id).join("pasted-1.png").exists());
    assert!(core.session_image(fork.id, &copy).is_ok());
    // A fork of a conversation with no images makes no folder.
    let plain = session_in(&core, "demo", "two").await;
    core.session_message(
        plain.id,
        "words only",
        &Attached::default(),
        Auth::Subscription,
    )
    .expect("sends");
    turn(&mut bus, plain.id).await;
    let words = core.session_fork(plain.id, None, None).expect("forks");
    assert!(!fx.paths.attachments(words.id).exists());

    // Closed, a session keeps its images; deleted, they go, and the fork's stay.
    core.session_close(source.id).expect("closes");
    assert!(first[0].exists());
    core.session_delete(source.id).expect("deletes");
    assert!(!fx.paths.attachments(source.id).exists());
    assert!(copy.exists());
}

#[tokio::test]
async fn removing_a_task_or_a_chat_deletes_the_images_of_its_sessions() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let in_task = session_in(&core, "demo", "one").await;
    let in_chat = session_in(&core, CHATS, "about-images").await;
    for session in [&in_task, &in_chat] {
        core.session_images_keep(session.id, &[PNG.to_string()])
            .expect("kept");
    }
    let gone = |session: &Session| !fx.paths.attachments(session.id).exists();
    assert!(!(gone(&in_task) || gone(&in_chat)));

    let remove = RemoveOptions {
        force: true,
        ..RemoveOptions::default()
    };
    core.task_remove("demo", "one", remove, &hooks::discard)
        .await
        .expect("removes the task");
    assert!(gone(&in_task));
    let chat = core.session(in_chat.id).expect("reads").task_id;
    let name = core
        .tasks(CHATS, true)
        .expect("lists")
        .into_iter()
        .find(|task| task.id == chat)
        .expect("the chat")
        .name;
    core.task_remove(CHATS, &name, remove, &hooks::discard)
        .await
        .expect("removes the chat");
    assert!(gone(&in_chat));
}

#[tokio::test]
async fn a_task_started_with_a_pasted_image_attaches_it_to_its_prompt() {
    let fx = fixture();
    let core = std::sync::Arc::new(core_with_project(&fx).await);
    let first = swarmed_core::FirstSession {
        options: SessionOptions::default(),
        prompt: "match this design".to_string(),
        files: vec![],
        sessions: vec![],
        images: vec![PNG.to_string()],
    };
    let mut bus = core.subscribe();
    let (started, going) = core
        .task_start(
            "demo",
            "design",
            NewTaskOptions::default(),
            first.clone(),
            Auth::Subscription,
            &hooks::discard,
        )
        .await
        .expect("starts");
    going.finished().await.expect("set up");
    let id = started.session.id;
    turn(&mut bus, id).await;
    let image = fx.paths.attachments(id).join("pasted-1.png");
    assert_eq!(
        messages(&core, id),
        [attached("match this design", &[image])]
    );

    // An image agents do not read refuses the start before anything is made.
    let refused = swarmed_core::FirstSession {
        images: vec!["Qk0AAA==".to_string()],
        ..first
    };
    let start = core.task_start(
        "demo",
        "refused",
        NewTaskOptions::default(),
        refused,
        Auth::Subscription,
        &hooks::discard,
    );
    assert!(matches!(start.await, Err(Error::ImageNotRead(what)) if what == "a BMP image"));
    assert!(core.task("demo", "refused").is_err());
    assert!(!fx.paths.worktree("demo", "refused").exists());
}
