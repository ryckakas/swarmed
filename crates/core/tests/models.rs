//! What a session runs on: the model and effort an agent chooses for a session it starts, and
//! a role's defaults from the user's settings, whoever starts the session. Against the
//! scripted agent.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{core_with_project, fixture, turn};
use serde_json::{Value, json};
use swarmed_core::agent::{AgentEvent, Auth, Effort};
use swarmed_core::{
    Attached, Core, Error, NewTaskOptions, PeerSpawn, RoleDefault, Session, SessionOptions, hooks,
    serve_session_tools,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// A core with one task, in a process that stays after a turn as the window does, so that its
/// sessions can start tasks.
async fn core_with_task(fx: &common::Fixture) -> Arc<Core> {
    let mut core = core_with_project(fx).await;
    core.set_stays();
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    Arc::new(core)
}

fn new_session(core: &Core, options: &SessionOptions) -> Session {
    core.session_new("demo", "one", options).expect("creates")
}

/// A session the user started on `model` at `effort`.
fn starter_on(core: &Core, model: &str, effort: Effort) -> Session {
    let options = SessionOptions {
        model: Some(model.to_string()),
        effort: Some(effort),
        ..SessionOptions::default()
    };
    new_session(core, &options)
}

/// A session the user started with `role`, choosing `model` or none.
fn user_session_with(core: &Core, role: &str, model: Option<&str>) -> Session {
    let options = SessionOptions {
        role: Some(role.to_string()),
        model: model.map(String::from),
        ..SessionOptions::default()
    };
    new_session(core, &options)
}

/// The settings' role defaults with only the reviewer's.
fn reviewer_on(model: Option<&str>, effort: Option<Effort>) -> BTreeMap<String, RoleDefault> {
    let chosen = RoleDefault {
        model: model.map(String::from),
        effort,
    };
    BTreeMap::from([("reviewer".to_string(), chosen)])
}

/// Keeps `roles` as the role defaults in the user's settings.
fn defaults_kept(core: &Core, roles: BTreeMap<String, RoleDefault>) {
    let mut settings = core.settings().expect("reads");
    settings.role_defaults = roles;
    core.set_settings(&settings).expect("saves");
}

/// Serves the session tools of `core`, and returns where they are reached.
async fn serving(core: &Arc<Core>) -> PathBuf {
    drop(tokio::spawn(serve_session_tools(Arc::clone(core))));
    let socket = core.paths().tools_socket();
    for _ in 0..1000 {
        if socket.exists() {
            return socket;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("nothing listens on {}", socket.display());
}

/// Session `from`'s call of `tool` with `arguments`: whether it worked, and what it said.
async fn call(socket: &Path, from: i64, tool: &str, arguments: Value) -> (bool, String) {
    let request = json!({ "session": from, "tool": tool, "arguments": arguments });
    let mut stream = UnixStream::connect(socket).await.expect("connects");
    stream
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("writes");
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .await
        .expect("reads");
    let reply: Value = serde_json::from_str(&line).expect("json");
    let text = reply["text"].as_str().unwrap_or_default().to_string();
    (reply["ok"] == true, text)
}

/// What session `id`'s agent says it runs on, asked between turns.
async fn runs_on(core: &Core, id: i64) -> String {
    let mut bus = core.subscribe();
    let nothing = Attached::default();
    core.session_message(id, "settings", &nothing, Auth::Subscription)
        .expect("sends");
    let events = turn(&mut bus, id).await;
    let said = events.iter().find_map(|event| match event {
        AgentEvent::Text { text } => Some(text.clone()),
        _ => None,
    });
    said.expect("it answers")
}

/// The one session of task `one` that is not `but`.
fn the_other(core: &Core, but: i64) -> Session {
    let sessions = core.sessions("demo", "one").expect("lists");
    let mut others = sessions.into_iter().filter(|session| session.id != but);
    let other = others.next().expect("there is one");
    assert!(others.next().is_none(), "only one other");
    other
}

#[tokio::test]
async fn an_agent_starts_a_session_on_a_model_and_effort_its_agent_offers() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let parent = starter_on(&core, "opus", Effort::Xhigh);
    let socket = serving(&core).await;
    let mut bus = core.subscribe();

    let asked = json!({ "prompt": "Review it.", "model": "fable", "effort": "medium" });
    let (ok, said) = call(&socket, parent.id, "spawn_session", asked).await;
    assert!(ok, "{said}");
    // The starter is told what it runs on, so a mistake shows where it was made.
    assert!(said.contains("on `fable` at medium effort"), "{said}");
    let child = the_other(&core, parent.id);
    assert!(
        said.contains(&format!("Started session {}", child.id)),
        "{said}"
    );
    // Written with the session, before its first message went.
    assert_eq!(
        (child.model.as_deref(), child.effort),
        (Some("fable"), Some(Effort::Medium))
    );
    turn(&mut bus, child.id).await;
    let answer = runs_on(&core, child.id).await;
    assert!(answer.starts_with("model fable, effort medium"), "{answer}");

    // The others see it, beside what the starter runs on.
    let listed = core.peer_sessions(parent.id).expect("lists");
    assert!(
        listed.starts_with("id\tstate\ttask\tworks for\trole\tmodel\teffort\ttitle"),
        "{listed}"
    );
    assert!(listed.contains("\tyou\t-\tfable\tmedium\t"), "{listed}");
    let own = format!("{} (you)\tidle\ttask one\t-\t-\topus\txhigh\t", parent.id);
    assert!(listed.contains(&own), "{listed}");
}

#[tokio::test]
async fn what_the_agent_does_not_offer_is_refused_with_what_it_does() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let parent = starter_on(&core, "opus", Effort::High);
    let socket = serving(&core).await;

    let wrong_model = json!({ "prompt": "Review it.", "model": "claude-fable-5-2" });
    let (ok, said) = call(&socket, parent.id, "spawn_session", wrong_model).await;
    assert!(!ok, "{said}");
    assert!(
        said.contains("`claude-fable-5-2` is not a model your agent offers"),
        "{said}"
    );
    assert!(said.contains("opus, sonnet, haiku, fable"), "{said}");

    let wrong_effort = json!({ "prompt": "Review it.", "effort": "ultra" });
    let (ok, said) = call(&socket, parent.id, "spawn_session", wrong_effort).await;
    assert!(!ok, "{said}");
    assert!(said.contains("`ultra` is not an effort"), "{said}");
    assert!(said.contains("low, medium, high, xhigh, max"), "{said}");

    // Nor a task: nothing is made for a call that is refused.
    let task = json!({ "name": "elsewhere", "prompt": "Do it.", "model": "gpt-5.5" });
    let (ok, said) = call(&socket, parent.id, "start_task", task).await;
    assert!(!ok, "{said}");
    assert!(said.contains("`gpt-5.5` is not a model"), "{said}");
    assert!(core.task("demo", "elsewhere").is_err());
    assert_eq!(core.sessions("demo", "one").expect("lists").len(), 1);
}

#[tokio::test]
async fn left_out_a_session_runs_on_its_starter_s_model_and_effort_as_before() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let parent = starter_on(&core, "opus", Effort::Xhigh);
    let child = core
        .peer_spawn(parent.id, "Help me.", None, None)
        .expect("spawns");
    assert_eq!(
        (child.model.as_deref(), child.effort),
        (Some("opus"), Some(Effort::Xhigh))
    );
    // Only one of the two asked for: the other is still the starter's.
    let asked = PeerSpawn {
        prompt: "Help me too.",
        effort: Some("low"),
        ..PeerSpawn::default()
    };
    let child = core.peer_spawn_asked(parent.id, asked).expect("spawns");
    assert_eq!(
        (child.model.as_deref(), child.effort),
        (Some("opus"), Some(Effort::Low))
    );
}

#[tokio::test]
async fn a_role_s_defaults_apply_whoever_starts_the_session_and_a_choice_wins() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    defaults_kept(&core, reviewer_on(Some("fable"), Some(Effort::Medium)));

    // The user, in the window or with `swarmed session run`, choosing nothing.
    let user = user_session_with(&core, "reviewer", None);
    assert_eq!(
        (user.model.as_deref(), user.effort),
        (Some("fable"), Some(Effort::Medium))
    );
    // Choosing a model: theirs wins, and the effort left open is still the role's.
    let chosen = user_session_with(&core, "reviewer", Some("opus"));
    assert_eq!(
        (chosen.model.as_deref(), chosen.effort),
        (Some("opus"), Some(Effort::Medium))
    );

    // An agent, asking for nothing: the role's, not its own.
    let parent = starter_on(&core, "opus", Effort::Xhigh);
    let reviewer = core
        .peer_spawn(parent.id, "Review it.", None, Some("reviewer"))
        .expect("spawns");
    assert_eq!(
        (reviewer.model.as_deref(), reviewer.effort),
        (Some("fable"), Some(Effort::Medium))
    );
    // Asking for a model: its choice wins.
    let asked = PeerSpawn {
        prompt: "Review it again.",
        role: Some("reviewer"),
        model: Some("sonnet"),
        ..PeerSpawn::default()
    };
    let reviewer = core.peer_spawn_asked(parent.id, asked).expect("spawns");
    assert_eq!(
        (reviewer.model.as_deref(), reviewer.effort),
        (Some("sonnet"), Some(Effort::Medium))
    );
    // A role with no defaults starts as before, on the starter's.
    let implementer = core
        .peer_spawn(parent.id, "Build it.", None, Some("implementer"))
        .expect("spawns");
    assert_eq!(
        (implementer.model.as_deref(), implementer.effort),
        (Some("opus"), Some(Effort::Xhigh))
    );

    // A role given later changes nothing it runs on.
    let later = new_session(&core, &SessionOptions::default());
    let later = core
        .session_set_role(later.id, Some("reviewer"))
        .expect("sets");
    assert_eq!((later.model, later.effort), (None, None));
}

#[tokio::test]
async fn a_role_s_model_the_agent_does_not_offer_is_passed_over_and_said() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    // Written by hand, or with an agent of agents.toml that is gone since.
    let file = "[role_defaults.reviewer]\nmodel = \"gpt-5.5\"\neffort = \"low\"\n";
    std::fs::write(core.paths().settings(), file).expect("writes");

    let user = user_session_with(&core, "reviewer", None);
    assert_eq!((user.model, user.effort), (None, Some(Effort::Low)));

    let parent = starter_on(&core, "opus", Effort::Xhigh);
    let socket = serving(&core).await;
    let asked = json!({ "prompt": "Review it.", "role": "reviewer" });
    let (ok, said) = call(&socket, parent.id, "spawn_session", asked).await;
    assert!(ok, "{said}");
    // As if the role had none: on the starter's model, and the role's effort.
    assert!(said.contains("on `opus` at low effort"), "{said}");
    assert!(
        said.contains("the reviewer's default model in the user's settings, `gpt-5.5`"),
        "{said}"
    );
    assert!(said.contains("passed over"), "{said}");

    // Kept as it was, it stops no other setting from being saved.
    let mut settings = core.settings().expect("reads");
    settings.editor = "zed".to_string();
    core.set_settings(&settings).expect("saves");
    let kept = core.settings().expect("reads");
    assert_eq!(
        kept.role_defaults,
        reviewer_on(Some("gpt-5.5"), Some(Effort::Low))
    );
}

#[tokio::test]
async fn a_role_default_is_checked_when_it_is_saved() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let mut settings = core.settings().expect("reads");

    let critic = RoleDefault {
        model: Some("opus".to_string()),
        effort: None,
    };
    settings.role_defaults = BTreeMap::from([("critic".to_string(), critic)]);
    let refused = core.set_settings(&settings);
    assert!(
        matches!(&refused, Err(Error::UnknownRole { name, .. }) if name == "critic"),
        "{refused:?}"
    );

    settings.role_defaults = reviewer_on(Some("gpt-x"), None);
    let refused = core.set_settings(&settings);
    let Err(Error::RoleModelNotOffered {
        role,
        model,
        offered,
    }) = &refused
    else {
        panic!("{refused:?}");
    };
    assert_eq!((role.as_str(), model.as_str()), ("reviewer", "gpt-x"));
    assert!(offered.contains("fable"), "{offered}");
    assert!(core.settings().expect("reads").role_defaults.is_empty());

    // An effort alone, and a model an agent offers, are kept.
    settings.role_defaults = reviewer_on(None, Some(Effort::High));
    core.set_settings(&settings).expect("saves");
    settings.role_defaults = reviewer_on(Some("haiku"), Some(Effort::High));
    core.set_settings(&settings).expect("saves");
    let kept = core.settings().expect("reads").role_defaults;
    assert_eq!(kept, reviewer_on(Some("haiku"), Some(Effort::High)));
}

#[tokio::test]
async fn a_task_an_agent_starts_runs_on_what_it_chose_from_its_first_turn() {
    let fx = fixture();
    let core = core_with_task(&fx).await;
    let parent = starter_on(&core, "opus", Effort::Xhigh);
    core.session_start(parent.id, Auth::Subscription)
        .expect("starts");
    let socket = serving(&core).await;
    let mut bus = core.subscribe();

    let asked = json!({
        "name": "quick", "prompt": "Do it.", "model": "haiku", "effort": "low",
    });
    let (ok, said) = call(&socket, parent.id, "start_task", asked).await;
    assert!(ok, "{said}");
    assert!(said.contains("on `haiku` at low effort"), "{said}");
    let sessions = core.sessions("demo", "quick").expect("lists");
    let first = &sessions[0];
    // Written with the session, before its setup began: a quick setup cannot outrun them.
    assert_eq!(
        (first.model.as_deref(), first.effort),
        (Some("haiku"), Some(Effort::Low))
    );
    turn(&mut bus, first.id).await;
    let answer = runs_on(&core, first.id).await;
    assert!(answer.starts_with("model haiku, effort low"), "{answer}");
}
