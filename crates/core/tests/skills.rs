//! Skills from every agent and from Swarmed itself, offered to the sessions that lack them.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::{core_with_project, fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{Core, Error, NewTaskOptions, SessionOptions, hooks, serve_session_tools};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Writes a skill called `name` into `folder`, the way an agent keeps one.
fn skill(folder: &Path, name: &str, description: &str) {
    let own = folder.join(name);
    std::fs::create_dir_all(&own).expect("dir");
    let text = format!("---\nname: {name}\ndescription: {description}\n---\n\nSteps for {name}.\n");
    std::fs::write(own.join("SKILL.md"), text).expect("writes");
}

/// A core whose agents have skills: Claude Code two, Codex two, one of them by the same name,
/// and one more in the folder Codex shares with other agents.
async fn core_with_skills(fx: &common::Fixture) -> Core {
    let root = fx.paths.claude_config.parent().expect("a folder");
    skill(&root.join(".claude/skills"), "review", "Review a change.");
    skill(&root.join(".claude/skills"), "both-have", "Claude's copy.");
    skill(
        &root.join("skills"),
        "deploy",
        "Ship it. Use when the user says deploy.",
    );
    skill(&root.join("skills"), "both-have", "Codex's copy.");
    skill(
        &root.join(".agents/skills"),
        "triage",
        "Sort incoming bugs.",
    );
    let mut core = core_with_project(fx).await;
    core.set_tool_server("/usr/local/bin/swarmed".into());
    core
}

/// The core's answer to a session's tool server asking for the skill `name`.
async fn asked(socket: std::path::PathBuf, session: i64, name: &str) -> serde_json::Value {
    let request = serde_json::json!({
        "session": session, "tool": "use_skill", "arguments": { "name": name },
    });
    let mut stream = UnixStream::connect(&socket).await.expect("connects");
    let line = format!("{request}\n");
    stream.write_all(line.as_bytes()).await.expect("writes");
    let mut answer = String::new();
    let mut reader = BufReader::new(stream);
    reader.read_line(&mut answer).await.expect("reads");
    serde_json::from_str(&answer).expect("json")
}

fn names(skills: &[swarmed_core::Skill]) -> Vec<&str> {
    skills.iter().map(|skill| skill.name.as_str()).collect()
}

#[tokio::test]
async fn each_agent_is_offered_the_skills_it_does_not_have_by_itself() {
    let fx = fixture();
    let core = core_with_skills(&fx).await;
    core.skill_save(
        "house-style",
        "  How we   write commit messages. ",
        "Use the imperative.",
    )
    .expect("saves");

    let all = core.skills();
    let listed: Vec<String> = all
        .iter()
        .map(|s| format!("{}/{}", s.origin, s.name))
        .collect();
    assert_eq!(
        listed,
        [
            "swarmed/house-style",
            "claude/both-have",
            "claude/review",
            "codex/both-have",
            "codex/deploy",
            "agents/triage",
        ]
    );
    assert_eq!(all[0].description, "How we write commit messages.");
    assert_eq!(
        core.skill_body("house-style").expect("reads"),
        "Use the imperative."
    );

    // Claude Code lacks Codex's and the shared folder's; what both have it has already.
    assert_eq!(
        names(&core.skills_offered("claude")),
        ["house-style", "deploy", "triage"]
    );
    // Codex reads the shared folder itself, and lacks Claude Code's.
    assert_eq!(
        names(&core.skills_offered("codex")),
        ["house-style", "review"]
    );
    // An agent with none of its own gets each name once.
    assert_eq!(
        names(&core.skills_offered("opencode")),
        ["house-style", "both-have", "review", "deploy"]
    );

    // A skill kept back is offered to nobody else, and still listed.
    core.skill_share("codex", "deploy", false).expect("saves");
    assert_eq!(
        names(&core.skills_offered("claude")),
        ["house-style", "triage"]
    );
    let deploy = core.skills().into_iter().find(|s| s.name == "deploy");
    assert!(!deploy.expect("listed").shared);
    core.skill_share("codex", "deploy", true).expect("saves");
    assert_eq!(core.skills_offered("claude").len(), 3);

    core.skill_remove("house-style").expect("removes");
    assert_eq!(names(&core.skills_offered("codex")), ["review"]);
    assert!(matches!(
        core.skill_save("Bad Name", "d", "b"),
        Err(Error::InvalidName(_))
    ));
    assert!(matches!(
        core.skill_save("empty", "d", "  "),
        Err(Error::Skill(_))
    ));
}

#[tokio::test]
async fn a_session_is_told_of_its_skills_and_handed_one_when_it_asks() {
    let fx = fixture();
    let core = Arc::new(core_with_skills(&fx).await);
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let mut bus = core.subscribe();
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    core.session_send(session.id, "instructions")
        .expect("sends");
    let told = turn(&mut bus, session.id).await;
    let said = told.iter().find_map(|event| match event {
        AgentEvent::Text { text } => Some(text.as_str()),
        _ => None,
    });
    let said = said.expect("the agent says what it was told");
    assert!(said.contains("# Skills from Swarmed"), "{said}");
    assert!(
        said.contains("- `deploy`: Ship it. Use when the user says deploy."),
        "{said}"
    );
    assert!(said.contains("- `triage`: Sort incoming bugs."), "{said}");
    // Its own are not named again.
    assert!(!said.contains("`review`"), "{said}");

    // Asked for over the tool server's socket, as the agent's tool call arrives.
    drop(tokio::spawn(serve_session_tools(Arc::clone(&core))));
    let socket = core.paths().tools_socket();
    let mut waits = 0;
    while !socket.exists() && waits < 200 {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        waits += 1;
    }
    let ask = |name: &'static str| asked(socket.clone(), session.id, name);
    let handed = ask("deploy").await;
    assert_eq!(handed["ok"], true, "{handed}");
    let text = handed["text"].as_str().expect("text");
    assert!(text.starts_with("Steps for deploy."), "{text}");
    assert!(text.contains("skills/deploy"), "{text}");
    // One the agent has itself, or that does not exist, is not handed over from here.
    assert_eq!(ask("review").await["ok"], false);
    assert_eq!(ask("nope").await["ok"], false);
}
