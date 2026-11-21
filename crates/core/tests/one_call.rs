//! What a front end asks for in one call: operations made of several steps that every front
//! end would otherwise have to put in the same order itself.

mod common;

use common::{core_with_project, fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{Core, Error, MarkColour, NewTaskOptions, SessionOptions, hooks};

/// A core with one task and one session in it whose agent has not been started.
async fn core_with_session(fx: &common::Fixture) -> (Core, i64) {
    let core = core_with_project(fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("test setup");
    (core, session.id)
}

fn user_message(text: String) -> AgentEvent {
    AgentEvent::UserMessage { text }
}

#[tokio::test]
async fn a_message_starts_the_agent_and_is_sent_with_its_files() {
    let fx = fixture();
    let (core, id) = core_with_session(&fx).await;
    let mut bus = core.subscribe();
    let readme = fx.repo.join("README.md");

    // A file that is not there is refused before the agent is started.
    let missing = fx.repo.join("nope.txt");
    assert!(matches!(
        core.session_message(id, "look", &[missing], Auth::Subscription),
        Err(Error::NoSuchAttachment(_))
    ));
    assert!(matches!(
        core.session_message(id, "  ", &[], Auth::Subscription),
        Err(Error::EmptyMessage)
    ));
    assert_eq!(core.running_sessions(), Vec::<i64>::new());

    let files = std::slice::from_ref(&readme);
    core.session_message(id, "what is this?", files, Auth::Subscription)
        .expect("sends");
    assert_eq!(core.running_sessions(), [id]);
    let events = turn(&mut bus, id).await;
    let sent = format!("what is this?\n\nAttached files:\n- {}", readme.display());
    assert_eq!(events.first(), Some(&user_message(sent)));

    // Running already, the agent is not started a second time: the next message is a turn.
    core.session_message(id, "and this?", &[], Auth::Subscription)
        .expect("sends");
    let events = turn(&mut bus, id).await;
    assert_eq!(events.first(), Some(&user_message("and this?".into())));
    assert_eq!(core.running_sessions(), [id]);
}

#[tokio::test]
async fn a_message_waits_in_the_queue_with_its_files_and_is_sent_to_an_agent_started_for_it() {
    let fx = fixture();
    let (core, id) = core_with_session(&fx).await;
    let mut bus = core.subscribe();
    let readme = fx.repo.join("README.md");

    let missing = fx.repo.join("nope.txt");
    assert!(matches!(
        core.session_enqueue_message(id, "later", &[missing]),
        Err(Error::NoSuchAttachment(_))
    ));
    let queue = core
        .session_enqueue_message(id, " later ", std::slice::from_ref(&readme))
        .expect("queues");
    let waiting = format!("later\n\nAttached files:\n- {}", readme.display());
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.first().map(|m| m.text.as_str()), Some(&*waiting));

    // No agent runs: sending what waits starts one.
    assert_eq!(core.running_sessions(), Vec::<i64>::new());
    core.session_resume_queue(id, Auth::Subscription)
        .expect("sends");
    assert_eq!(core.running_sessions(), [id]);
    let events = turn(&mut bus, id).await;
    assert!(events.contains(&user_message(waiting)), "{events:?}");
    assert_eq!(core.session_queue(id).expect("lists"), []);
    // With nothing waiting it is not an error, and a session that is not there is.
    core.session_resume_queue(id, Auth::Subscription)
        .expect("nothing to send");
    assert!(matches!(
        core.session_resume_queue(999, Auth::Subscription),
        Err(Error::SessionNotFound(999))
    ));
}

#[tokio::test]
async fn a_mark_is_put_on_and_taken_off_by_its_words() {
    let fx = fixture();
    let (core, id) = core_with_session(&fx).await;

    let marked = core
        .session_mark_named(id, " Green ", " tests pass ")
        .expect("marks");
    let mark = marked.mark.expect("a mark");
    assert_eq!(
        (mark.colour, mark.label.as_str()),
        (MarkColour::Green, "tests pass")
    );
    assert!(matches!(
        core.session_mark_named(id, "teal", ""),
        Err(Error::Mark(_))
    ));
    // A colour that is refused leaves the mark as it was.
    assert!(core.session(id).expect("exists").mark.is_some());
    let cleared = core.session_mark_named(id, "none", "").expect("clears");
    assert_eq!(cleared.mark, None);
}

#[tokio::test]
async fn a_change_to_the_skills_answers_with_the_list_as_it_then_stands() {
    let fx = fixture();
    let core = core_with_project(&fx).await;
    let names = |skills: &[swarmed_core::Skill]| -> Vec<String> {
        skills.iter().map(|skill| skill.name.clone()).collect()
    };

    let saved = core
        .skills_after_save(
            "house-style",
            "How we write commits.",
            "Use the imperative.",
        )
        .expect("saves");
    assert_eq!(names(&saved), ["house-style"]);
    assert!(saved.iter().all(|skill| skill.shared));
    assert!(matches!(
        core.skills_after_save("Bad Name", "d", "b"),
        Err(Error::InvalidName(_))
    ));

    let kept_back = core
        .skills_after_share("swarmed", "house-style", false)
        .expect("saves");
    assert_eq!(kept_back.first().map(|skill| skill.shared), Some(false));
    let offered = core
        .skills_after_share("swarmed", "house-style", true)
        .expect("saves");
    assert_eq!(offered.first().map(|skill| skill.shared), Some(true));

    assert_eq!(
        core.skills_after_remove("house-style").expect("removes"),
        []
    );
    assert!(matches!(
        core.skills_after_remove("Bad Name"),
        Err(Error::InvalidName(_))
    ));
}
