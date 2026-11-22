//! Skills from every agent and from Swarmed itself, offered to the sessions that lack them.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::{AskedNamer, core_with_project, fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    CommandKind, Core, Error, NewTaskOptions, SessionOptions, SlashCommand, hooks,
    serve_session_tools,
};
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
        names(&core.skills_offered("claude", None)),
        ["house-style", "deploy", "triage"]
    );
    // Codex reads the shared folder itself, and lacks Claude Code's.
    assert_eq!(
        names(&core.skills_offered("codex", None)),
        ["house-style", "review"]
    );
    // An agent with none of its own gets each name once.
    assert_eq!(
        names(&core.skills_offered("opencode", None)),
        ["house-style", "both-have", "review", "deploy"]
    );

    // A skill kept back is offered to nobody else, and still listed.
    core.skill_share("codex", "deploy", false).expect("saves");
    assert_eq!(
        names(&core.skills_offered("claude", None)),
        ["house-style", "triage"]
    );
    let deploy = core.skills().into_iter().find(|s| s.name == "deploy");
    assert!(!deploy.expect("listed").shared);
    core.skill_share("codex", "deploy", true).expect("saves");
    assert_eq!(core.skills_offered("claude", None).len(), 3);

    core.skill_remove("house-style").expect("removes");
    assert_eq!(names(&core.skills_offered("codex", None)), ["review"]);
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
async fn plugins_and_the_repository_bring_skills_for_the_agents_that_would_not_read_them() {
    let fx = fixture();
    let root = fx.paths.claude_config.parent().expect("a folder");
    // Two plugins installed in Claude Code: one for the user, one for a single project.
    let design = root.join("cache/design/1.0.0");
    let local = root.join("cache/local/1.0.0");
    skill(&design.join("skills"), "mock-up", "Draw a screen.");
    skill(&local.join("skills"), "secret", "Only for one project.");
    let installed = serde_json::json!({ "version": 2, "plugins": {
        "design@official": [{ "scope": "user", "installPath": design }],
        "local@official": [{ "scope": "project", "installPath": local }],
    }});
    let plugins = root.join(".claude/plugins");
    std::fs::create_dir_all(&plugins).expect("dir");
    std::fs::write(
        plugins.join("installed_plugins.json"),
        installed.to_string(),
    )
    .expect("writes");
    let core = core_with_project(&fx).await;

    let listed: Vec<String> = core
        .skills()
        .iter()
        .map(|s| format!("{}/{}", s.origin, s.name))
        .collect();
    assert_eq!(listed, ["plugins/design:mock-up"]);
    // Claude Code reads its plugins itself; the others are offered what they bring.
    assert_eq!(names(&core.skills_offered("claude", None)), [""; 0]);
    assert_eq!(
        names(&core.skills_offered("codex", None)),
        ["design:mock-up"]
    );

    // A repository's own skills go with the task whose worktree holds them.
    let worktree = root.join("worktree");
    skill(
        &worktree.join(".claude/skills"),
        "release",
        "Cut a release.",
    );
    skill(&worktree.join(".agents/skills"), "lint", "Run the linters.");
    assert_eq!(
        names(&core.skills_offered("codex", Some(&worktree))),
        ["design:mock-up", "release"]
    );
    assert_eq!(
        names(&core.skills_offered("claude", Some(&worktree))),
        ["lint"]
    );
    // They are not part of the list for the settings, which is about no task in particular.
    assert_eq!(core.skills().len(), 1);
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

/// The names of the commands that are sent to the agent as messages, which skills are.
fn sent_on(commands: &[SlashCommand]) -> Vec<&str> {
    let sent = commands
        .iter()
        .filter(|command| command.kind == CommandKind::Prompt);
    sent.map(|command| command.name.as_str()).collect()
}

#[tokio::test]
async fn a_skill_called_by_name_is_listed_and_sent_with_what_it_says() {
    let fx = fixture();
    let core = core_with_skills(&fx).await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    let mut bus = core.subscribe();
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");

    // Before its agent has said what commands it has, the menu holds the skills it is offered.
    let offered = core.slash_commands(Some(session.id)).expect("lists");
    assert_eq!(sent_on(&offered), ["deploy", "triage"]);

    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    core.session_send(session.id, "/deploy to staging")
        .expect("sends");
    let events = turn(&mut bus, session.id).await;
    // What is recorded is what was typed; what the agent got is the skill and the request.
    assert!(events.contains(&AgentEvent::UserMessage {
        text: "/deploy to staging".to_string()
    }));
    let got = events.iter().find_map(|event| match event {
        AgentEvent::Text { text } => Some(text.as_str()),
        _ => None,
    });
    let got = got.expect("the agent answers with what it was sent");
    assert!(got.contains("The user calls the skill `deploy`"), "{got}");
    assert!(got.contains("Steps for deploy."), "{got}");
    assert!(got.contains("to staging"), "{got}");
    assert!(!got.contains("description:"), "{got}");

    // The agent's own commands come first once it has reported them, each name once.
    let listed = core.slash_commands(Some(session.id)).expect("lists");
    assert_eq!(listed[0].name, "context");
    assert_eq!(sent_on(&listed), ["review", "deploy", "triage"]);

    // One of its own skills, an unknown name, or a slash in the middle goes as it was typed.
    for plain in ["/review this", "/nope", "see /deploy"] {
        core.session_send(session.id, plain).expect("sends");
        let events = turn(&mut bus, session.id).await;
        let got = events.iter().find_map(|event| match event {
            AgentEvent::Text { text } => Some(text.as_str()),
            _ => None,
        });
        assert!(
            !got.unwrap_or_default().contains("The user calls the skill"),
            "{plain}"
        );
    }
}

#[tokio::test]
async fn a_skill_call_is_named_with_its_description_only_when_one_skill_answers_to_it() {
    let fx = fixture();
    let mut core = core_with_skills(&fx).await;
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let last_asked = || asked.lock().expect("not poisoned").pop().expect("asked");
    core.set_namer(Arc::new(AskedNamer {
        answer: "A title",
        asked: Arc::clone(&asked),
    }));
    let task_named = |prompt: &'static str| core.task_name_for("demo", prompt);

    // A new task's agent is not known yet: Claude Code's and Codex's `both-have` say
    // different things, so neither is taken for what the user meant.
    task_named("/both-have 1").await.expect("asks");
    assert_eq!(
        last_asked(),
        "The user calls the command `both-have`.\nWhat they give after it: 1"
    );
    // One skill of that name anywhere: it is the one.
    task_named("/triage 2").await.expect("asks");
    assert!(last_asked().contains("skill `triage`, which is for: Sort incoming bugs."));
    // A name Claude Code has a command of its own by may run that instead.
    task_named("/review 3").await.expect("asks");
    assert!(last_asked().starts_with("The user calls the command `review`."));

    // A session's agent is known: Claude Code takes its own `both-have`.
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("creates");
    let mut bus = core.subscribe();
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    core.session_send(session.id, "/both-have 4")
        .expect("sends");
    turn(&mut bus, session.id).await;
    let named = |asked: &std::sync::Mutex<Vec<String>>| {
        asked
            .lock()
            .expect("not poisoned")
            .iter()
            .any(|request| request.contains("skill `both-have`, which is for: Claude's copy."))
    };
    // Named off the turn, so asked by now or very soon after.
    let mut waited = 0;
    while !named(&asked) && waited < 100 {
        waited += 1;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(named(&asked), "{:?}", asked.lock().expect("not poisoned"));
}
