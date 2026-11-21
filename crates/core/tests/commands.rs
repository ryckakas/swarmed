//! What becomes of a message that is one of the agent's own commands, against the scripted
//! fake agent, which stands in for Claude Code.

mod common;

use std::collections::BTreeMap;

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Asked, Auth, Effort};
use swarmed_core::{
    CommandKind, Core, Error, NewTaskOptions, Session, SessionOptions, SessionStatus, hooks,
};
use tokio::sync::broadcast::Receiver;

async fn core_with_session(fx: &common::Fixture) -> (Core, Session) {
    let core = core_with_project(fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("test setup");
    (core, session)
}

fn said(core: &Core, id: i64) -> Vec<String> {
    let events = core.session_events(id).expect("reads");
    let messages = events.into_iter().filter_map(|event| match event {
        AgentEvent::UserMessage { text } => Some(text),
        _ => None,
    });
    messages.collect()
}

/// The next question a command put to the user: its id and what it asks.
async fn asked(bus: &mut Receiver<swarmed_core::SessionEvent>) -> (String, Vec<Asked>) {
    loop {
        if let AgentEvent::CommandAsked {
            request_id,
            questions,
            ..
        } = next(bus).await.event
        {
            return (request_id, questions);
        }
    }
}

/// What the next answer to a command's question came to.
async fn answered(bus: &mut Receiver<swarmed_core::SessionEvent>) -> (String, String) {
    loop {
        if let AgentEvent::CommandAnswered { request_id, said } = next(bus).await.event {
            return (request_id, said);
        }
    }
}

fn answers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let pairs = pairs.iter();
    pairs
        .map(|(question, answer)| ((*question).to_string(), (*answer).to_string()))
        .collect()
}

/// Skips ahead to the next event of the session that `wanted` holds for.
async fn until(bus: &mut Receiver<swarmed_core::SessionEvent>, wanted: fn(&AgentEvent) -> bool) {
    while !wanted(&next(bus).await.event) {}
}

/// The answer that changes the first setting asked about and leaves the rest.
fn first_off(at: usize) -> &'static str {
    if at == 0 { "Off" } else { "Leave as it is" }
}

#[tokio::test]
async fn a_screen_command_starts_the_agent_asks_it_and_sends_no_message() {
    let fx = fixture();
    let (core, session) = core_with_session(&fx).await;
    let mut bus = core.subscribe();
    assert_eq!(core.running_sessions(), [0_i64; 0]);

    core.session_message(session.id, " /usage ", &[], Auth::Subscription)
        .expect("is shown");
    let published = next(&mut bus).await;
    let AgentEvent::Screen {
        command,
        title,
        sections,
        ..
    } = &published.event
    else {
        panic!("not a screen: {published:?}");
    };
    assert_eq!((command.as_str(), title.as_str()), ("usage", "Usage"));
    assert_eq!(sections[0].rows[0].value, "usage");
    // It is no turn: nothing was said, the session is as idle as before and has no title.
    assert_eq!(published.status, None);
    let stored = core.session(session.id).expect("exists");
    assert_eq!(stored.status, SessionStatus::Idle);
    assert_eq!(stored.title, "");
    assert!(!stored.unread);
    assert_eq!(said(&core, session.id), [""; 0]);
    // The screen stays in the conversation's history, for the user alone.
    let kept = core.session_events(session.id).expect("reads");
    assert!(kept.contains(&published.event));

    // In a running session it is the same through the plain send.
    core.session_send(session.id, "/status").expect("is shown");
    let shown = next(&mut bus).await.event;
    assert!(matches!(shown, AgentEvent::Screen { command, .. } if command == "status"));
}

#[tokio::test]
async fn a_screen_command_with_words_after_it_and_a_skill_are_sent_as_typed() {
    let fx = fixture();
    let (core, session) = core_with_session(&fx).await;
    let mut bus = core.subscribe();
    for typed in ["/mcp reconnect docs", "/review this", "/no-such-command"] {
        core.session_message(session.id, typed, &[], Auth::Subscription)
            .expect("sends");
        let events = turn(&mut bus, session.id).await;
        let echoed = AgentEvent::Text {
            text: format!("echo: {typed}"),
        };
        assert!(events.contains(&echoed), "{typed}: {events:?}");
    }
    assert_eq!(said(&core, session.id).len(), 3);
}

#[tokio::test]
async fn a_terminal_command_is_refused_before_anything_is_started_or_said() {
    let fx = fixture();
    let (core, session) = core_with_session(&fx).await;
    let refused = core.session_message(session.id, "/login", &[], Auth::Subscription);
    let Err(Error::TerminalOnly { command, instead }) = &refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(command, "login");
    assert!(instead.contains("sign-in"), "{instead}");
    let told = refused.expect_err("refused").to_string();
    assert!(
        told.starts_with("/login only works in a terminal. "),
        "{told}"
    );
    let plain = core
        .session_message(session.id, "/vim", &[], Auth::Subscription)
        .expect_err("refused")
        .to_string();
    assert!(
        plain.starts_with("/vim only works in a terminal: open the task in one"),
        "{plain}"
    );
    assert_eq!(core.running_sessions(), [0_i64; 0]);
    assert_eq!(core.session_events(session.id).expect("reads"), []);
}

#[tokio::test]
async fn the_model_command_asks_and_the_answer_is_set_without_a_message() {
    let fx = fixture();
    let (core, session) = core_with_session(&fx).await;
    let mut bus = core.subscribe();
    core.session_message(session.id, "/model", &[], Auth::Subscription)
        .expect("asks");
    let (request_id, questions) = asked(&mut bus).await;
    let [question] = &questions[..] else {
        panic!("one question: {questions:?}");
    };
    let labels: Vec<&str> = question.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels[0], "Default");
    assert!(labels.contains(&"opus"), "{labels:?}");
    // Asking starts no agent and is no turn.
    assert_eq!(core.running_sessions(), [0_i64; 0]);
    assert_eq!(
        core.session(session.id).expect("exists").status,
        SessionStatus::Idle
    );

    let answer = Some(answers(&[(&question.question, "opus")]));
    core.session_command_answer(session.id, &request_id, answer.clone(), Auth::Subscription)
        .expect("is set");
    let (of, what) = answered(&mut bus).await;
    assert_eq!(of, request_id);
    assert_eq!(what, "This session's model is now opus.");
    let stored = core.session(session.id).expect("exists");
    assert_eq!(stored.model.as_deref(), Some("opus"));
    assert_eq!(said(&core, session.id), [""; 0]);

    // A second answer to the same question finds it closed and changes nothing.
    let other = Some(answers(&[(&question.question, "haiku")]));
    core.session_command_answer(session.id, &request_id, other, Auth::Subscription)
        .expect("is ignored");
    core.session_command_answer(session.id, "model-0", answer, Auth::Subscription)
        .expect("is ignored");
    let stored = core.session(session.id).expect("exists");
    assert_eq!(stored.model.as_deref(), Some("opus"));
}

#[tokio::test]
async fn a_setting_named_with_its_command_is_set_at_once_and_one_put_aside_changes_nothing() {
    let fx = fixture();
    let (core, session) = core_with_session(&fx).await;
    let mut bus = core.subscribe();
    core.session_message(session.id, "/effort High", &[], Auth::Subscription)
        .expect("is set");
    let (_, what) = answered(&mut bus).await;
    assert_eq!(what, "This session's effort is now High.");
    let stored = core.session(session.id).expect("exists");
    assert_eq!(stored.effort, Some(Effort::High));

    core.session_message(session.id, "/model default", &[], Auth::Subscription)
        .expect("is set");
    let (_, what) = answered(&mut bus).await;
    assert_eq!(what, "This session's model is now the default.");
    assert_eq!(core.session(session.id).expect("exists").model, None);

    let wrong = core.session_message(session.id, "/effort enormous", &[], Auth::Subscription);
    assert!(
        matches!(&wrong, Err(Error::NotAChoice { command, answer }) if command == "effort" && answer == "enormous"),
        "{wrong:?}"
    );
    assert_eq!(
        core.session(session.id).expect("exists").effort,
        Some(Effort::High)
    );

    core.session_message(session.id, "/effort", &[], Auth::Subscription)
        .expect("asks");
    let (request_id, _) = asked(&mut bus).await;
    core.session_command_answer(session.id, &request_id, None, Auth::Subscription)
        .expect("is put aside");
    let (of, what) = answered(&mut bus).await;
    assert_eq!((of, what.as_str()), (request_id, "Nothing was changed."));
    assert_eq!(
        core.session(session.id).expect("exists").effort,
        Some(Effort::High)
    );
}

#[tokio::test]
async fn config_asks_about_claude_codes_settings_and_tells_it_only_what_changes() {
    let fx = fixture();
    let (core, session) = core_with_session(&fx).await;
    let mut bus = core.subscribe();
    core.session_message(session.id, "/config", &[], Auth::Subscription)
        .expect("asks");
    let (request_id, questions) = asked(&mut bus).await;
    assert!(questions.len() > 1);
    let chosen: Vec<(&str, &str)> = questions
        .iter()
        .enumerate()
        .map(|(at, asked)| (asked.question.as_str(), first_off(at)))
        .collect();
    core.session_command_answer(
        session.id,
        &request_id,
        Some(answers(&chosen)),
        Auth::Subscription,
    )
    .expect("is passed on");
    let (_, what) = answered(&mut bus).await;
    assert_eq!(what, "Claude Code was told: /config autoCompact=false");
    // Claude Code takes its settings in words, so that is what it is sent, as a message.
    let events = turn(&mut bus, session.id).await;
    let echoed = AgentEvent::Text {
        text: "echo: /config autoCompact=false".to_string(),
    };
    assert!(events.contains(&echoed), "{events:?}");
    assert_eq!(said(&core, session.id), ["/config autoCompact=false"]);

    // Everything left as it is sends nothing.
    core.session_message(session.id, "/config", &[], Auth::Subscription)
        .expect("asks");
    let (request_id, questions) = asked(&mut bus).await;
    let unchanged: Vec<(&str, &str)> = questions
        .iter()
        .map(|asked| (asked.question.as_str(), "Leave as it is"))
        .collect();
    core.session_command_answer(
        session.id,
        &request_id,
        Some(answers(&unchanged)),
        Auth::Subscription,
    )
    .expect("changes nothing");
    let (_, what) = answered(&mut bus).await;
    assert_eq!(what, "Nothing was changed.");
    assert_eq!(said(&core, session.id).len(), 1);

    // Typed with its values, it goes to Claude Code as it stands.
    core.session_message(
        session.id,
        "/config thinking=false",
        &[],
        Auth::Subscription,
    )
    .expect("sends");
    turn(&mut bus, session.id).await;
    assert_eq!(said(&core, session.id).len(), 2);
}

#[tokio::test]
async fn a_command_passed_on_while_the_agent_works_waits_its_turn() {
    let fx = fixture();
    let (core, session) = core_with_session(&fx).await;
    let mut bus = core.subscribe();
    core.session_message(session.id, "slow", &[], Auth::Subscription)
        .expect("sends");
    until(&mut bus, |event| matches!(event, AgentEvent::Text { .. })).await;
    core.session_message(session.id, "/mcp reconnect docs", &[], Auth::Subscription)
        .expect("waits");
    let waiting = core.session_queue(session.id).expect("reads");
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].text, "/mcp reconnect docs");
    assert_eq!(said(&core, session.id), ["slow"]);
    assert_eq!(
        core.session(session.id).expect("exists").status,
        SessionStatus::Working
    );
    core.session_interrupt(session.id).expect("stops");
}

#[tokio::test]
async fn the_menu_marks_what_swarmed_does_with_each_command() {
    let fx = fixture();
    let (core, session) = core_with_session(&fx).await;
    let menu = core.slash_commands(Some(session.id)).expect("lists");
    let kind = |name: &str| {
        let found = menu.iter().find(|command| command.name == name);
        found.map(|command| command.kind)
    };
    assert_eq!(kind("usage"), Some(CommandKind::Screen));
    assert_eq!(kind("status"), Some(CommandKind::Screen));
    assert_eq!(kind("model"), Some(CommandKind::Setting));
    assert_eq!(kind("config"), Some(CommandKind::Setting));
    assert_eq!(kind("login"), None);
}
