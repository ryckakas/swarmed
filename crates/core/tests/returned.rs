//! Images a tool returns, such as a browser's screenshot: kept as the session's own, listed on
//! the result, shown only to the user, and gone with the session.

mod common;

use std::path::PathBuf;

use common::{core_with_project, fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{Attached, Core, Error, NewTaskOptions, Session, SessionOptions, hooks};

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

/// The images listed on the first tool result among `events`, and what it said.
fn returned(events: &[AgentEvent]) -> (String, Vec<PathBuf>) {
    events
        .iter()
        .find_map(|event| match event {
            AgentEvent::ToolResult {
                content,
                images,
                returned,
                ..
            } => {
                assert!(returned.is_empty(), "the core keeps what was read");
                Some((content.clone(), images.clone()))
            }
            _ => None,
        })
        .expect("a tool result")
}

#[tokio::test]
async fn a_screenshot_is_kept_in_the_sessions_folder_and_listed_on_its_result() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let session = session_in(&core, "demo", "one").await;
    let folder = fx.paths.attachments(session.id);
    let mut bus = core.subscribe();
    let screenshot = |core: &Core| {
        core.session_message(
            session.id,
            "screenshot",
            &Attached::default(),
            Auth::Subscription,
        )
        .expect("sends");
    };

    screenshot(&core);
    let (said, images) = returned(&turn(&mut bus, session.id).await);
    assert_eq!(said, "Took the viewport screenshot\n[image]");
    assert_eq!(images, [folder.join("tool-tool-shot-1.png")]);
    // Stored as it was published: by its path, never its bytes.
    let stored = core.session_events(session.id).expect("reads");
    assert_eq!(returned(&stored).1, images);
    let shown = core.session_image(session.id, &images[0]).expect("read");
    assert!(shown.starts_with("data:image/png;base64,iVBORw0KGgo"));

    // The same call again keeps what it returned before.
    screenshot(&core);
    let (_, again) = returned(&turn(&mut bus, session.id).await);
    assert_eq!(again, [folder.join("tool-tool-shot-2.png")]);
    assert!(images[0].exists());

    // Counted with what Swarmed keeps on disk.
    assert!(core.usage().await.expect("measures").images_bytes > 0);

    // A fork carries what was said, not what tools returned.
    let fork = core.session_fork(session.id, None, None).expect("forks");
    assert!(!fx.paths.attachments(fork.id).exists());
    assert!(matches!(
        core.session_image(fork.id, &images[0]),
        Err(Error::NotPastedImage(_))
    ));

    core.session_close(session.id).expect("closes");
    assert!(images[0].exists());
    core.session_delete(session.id).expect("deletes");
    assert!(!folder.exists());
}
