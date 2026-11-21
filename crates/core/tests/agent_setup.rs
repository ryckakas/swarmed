//! Setting up agents, with stand-ins for Homebrew and for the agents' own programs.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

use common::fixture;
use swarmed_core::{AgentSetup, Core, Error};

/// An executable file at `path` that runs `script`.
fn executable(path: &Path, script: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{script}\n")).expect("writes");
    let mut permissions = std::fs::metadata(path).expect("exists").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("sets");
}

/// A core that looks for programs in `tools` only.
fn core_with_tools(fx: &common::Fixture, tools: &Path) -> Core {
    std::fs::create_dir_all(tools).expect("dir");
    let mut core = Core::open(fx.paths.clone()).expect("opens");
    core.set_tool_dirs(vec![tools.to_path_buf()]);
    core
}

/// What the core says about setting up the agent `id`.
fn setup_of(core: &Core, id: &str) -> AgentSetup {
    let setups = core.agent_setups().expect("reads");
    let found = setups.into_iter().find(|setup| setup.id == id);
    found.expect("a known agent")
}

#[test]
fn claude_code_and_codex_can_be_set_up_like_any_other_agent() {
    let fx = fixture();
    let tools = fx.paths.data_dir.join("tools");
    let core = core_with_tools(&fx, &tools);
    let ids: Vec<String> = core
        .agent_setups()
        .expect("reads")
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, ["claude", "codex", "opencode", "antigravity"]);

    // Claude Code is installed by its own installer, and needs nothing written down.
    let claude = setup_of(&core, "claude");
    assert_eq!((claude.installed.clone(), claude.configured), (None, false));
    assert!(claude.built_in);
    assert!(claude.install_command.contains("claude.ai/install.sh"));
    assert!(claude.sign_in.contains("/login"));
    executable(&tools.join("claude"), "exit 0");
    let claude = core.agent_configure("claude", None).expect("nothing to do");
    assert_eq!(claude.installed, Some(tools.join("claude")));
    assert!(claude.configured);
    assert!(!fx.paths.agents_config().exists());

    // Codex runs through its ACP adapter, started without arguments.
    let codex = setup_of(&core, "codex");
    assert_eq!(codex.install_command, "brew install codex-acp");
    assert!(!codex.built_in);
    executable(&tools.join("codex-acp"), "exit 0");
    let codex = core.agent_configure("codex", None).expect("configures");
    assert!(codex.configured);
    let written = std::fs::read_to_string(fx.paths.agents_config()).expect("written");
    let command = format!("command = \"{}\"", tools.join("codex-acp").display());
    assert!(written.contains(&command), "{written}");
    assert!(written.contains("args = []"), "{written}");
    assert!(core.agents().iter().any(|agent| agent.id == "codex"));
}

#[tokio::test]
async fn an_agent_that_knows_its_models_is_asked_for_them() {
    let fx = fixture();
    let tools = fx.paths.data_dir.join("tools");
    let core = core_with_tools(&fx, &tools);
    // A stand-in that answers the greeting, chatters, then says what it offers.
    let answers = r#"read greeting
echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}'
read session
echo '{"jsonrpc":"2.0","method":"_auth/status_update","params":{}}'
echo '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s","configOptions":[{"id":"model","options":[{"value":"gpt-a"},{"value":"gpt-b"}]}]}}'
sleep 5"#;
    executable(&tools.join("codex-acp"), answers);

    let setup = core.agent_models_refresh("codex").await.expect("asks");
    assert_eq!(setup.models, ["gpt-a", "gpt-b"]);
    assert_eq!(setup.model_source, "agent");
    let codex = core.agents().into_iter().find(|agent| agent.id == "codex");
    assert_eq!(codex.expect("offered").models, ["gpt-a", "gpt-b"]);
    // What kept Claude Code's MCP servers from it is no longer written.
    let written = std::fs::read_to_string(fx.paths.agents_config()).expect("written");
    assert!(!written.contains("mcp_servers"), "{written}");

    // An agent that will not say leaves the models as they were, and says why.
    let unwilling = r#"read greeting
echo '{"jsonrpc":"2.0","id":1,"result":{}}'
read session
echo '{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"sign in first"}}'"#;
    executable(&tools.join("codex-acp"), unwilling);
    let refused = core.agent_models_refresh("codex").await;
    assert!(
        matches!(&refused, Err(Error::Install { reason, .. }) if reason.ends_with("sign in first")),
        "{refused:?}"
    );
    assert_eq!(setup_of(&core, "codex").models, ["gpt-a", "gpt-b"]);
    // Nothing to ask of an agent whose models the user picks.
    executable(&tools.join("opencode"), "exit 0");
    let picked = core.agent_models_refresh("opencode").await.expect("keeps");
    assert_eq!(picked.model_source, "openrouter");
    assert_eq!(picked.models.len(), 3);
}

#[test]
fn an_installed_agent_is_configured_without_editing_the_file() {
    let fx = fixture();
    let tools = fx.paths.data_dir.join("tools");
    let core = core_with_tools(&fx, &tools);

    let before = setup_of(&core, "opencode");
    assert_eq!((before.installed.clone(), before.configured), (None, false));
    assert!(before.install_command.starts_with("brew install "));

    // Something the user wrote by hand must survive.
    let config = fx.paths.agents_config();
    std::fs::write(&config, "[goose]\ncommand = \"goose\"\nargs = [\"acp\"]\n").expect("writes");
    // Configured, but with no program to start: not something to offer.
    let ids = |core: &Core| -> Vec<String> { core.agents().into_iter().map(|a| a.id).collect() };
    assert_eq!(ids(&core), ["claude"]);
    executable(&tools.join("opencode"), "exit 0");
    executable(&tools.join("goose"), "exit 0");

    let setup = core.agent_configure("opencode", None).expect("configures");
    assert_eq!(setup.installed, Some(tools.join("opencode")));
    assert!(setup.configured);
    assert_ne!(setup.models, Vec::<String>::new());
    // It is usable at once, started by its whole path, and the other entry is still there.
    assert_eq!(ids(&core), ["claude", "goose", "opencode"]);
    // Uninstalled again, it leaves the menu, though its entry stays.
    std::fs::remove_file(tools.join("goose")).expect("removes");
    assert_eq!(ids(&core), ["claude", "opencode"]);
    let written = std::fs::read_to_string(&config).expect("reads");
    assert!(written.contains(&format!(
        "command = \"{}\"",
        tools.join("opencode").display()
    )));
    assert!(written.contains("[goose]"));
}

#[test]
fn the_models_offered_are_the_users_to_choose() {
    let fx = fixture();
    let tools = fx.paths.data_dir.join("tools");
    let core = core_with_tools(&fx, &tools);
    executable(&tools.join("opencode"), "exit 0");

    let chosen = vec!["openrouter/x-ai/grok-4.7".to_string()];
    let setup = core
        .agent_configure("opencode", Some(chosen.clone()))
        .expect("configures");
    assert_eq!(setup.models, chosen);
    let opencode = core
        .agents()
        .into_iter()
        .find(|agent| agent.id == "opencode");
    assert_eq!(opencode.expect("listed").models, chosen);
    // Configuring again without naming models leaves the choice alone.
    let again = core.agent_configure("opencode", None).expect("configures");
    assert_eq!(again.models, chosen);

    assert!(matches!(
        core.agent_configure("nope", None),
        Err(Error::AgentNotFound(id)) if id == "nope"
    ));
}

#[tokio::test]
async fn an_agent_is_installed_with_homebrew_and_set_up_in_one_go() {
    let fx = fixture();
    let tools = fx.paths.data_dir.join("tools");
    let core = core_with_tools(&fx, &tools);

    // Without Homebrew there is nothing to install with, and the error says so.
    let none = core
        .agent_install("opencode", &swarmed_core::hooks::discard)
        .await;
    assert!(matches!(none, Err(Error::Install { reason, .. }) if reason.contains("Homebrew")));

    // A stand-in that "installs" by putting the program next to itself.
    let installs = format!(
        "echo \"Installing $2\"; printf '#!/bin/sh\\n' > '{0}'; chmod +x '{0}'",
        tools.join("opencode").display()
    );
    executable(&tools.join("brew"), &installs);
    let printed = Mutex::new(Vec::new());
    let sink = |line: &str| printed.lock().expect("lock").push(line.to_string());
    let setup = core
        .agent_install("opencode", &sink)
        .await
        .expect("installs");
    assert_eq!(setup.installed, Some(tools.join("opencode")));
    assert!(setup.configured);
    let printed = printed.into_inner().expect("lock");
    assert_eq!(printed, ["Installing anomalyco/tap/opencode-v2"]);
    assert!(core.agents().iter().any(|agent| agent.id == "opencode"));

    // An installer that fails, or leaves nothing behind, is an error and not a silent success.
    std::fs::remove_file(tools.join("opencode")).expect("removes");
    executable(&tools.join("brew"), "echo nope >&2; exit 1");
    let failed = core
        .agent_install("opencode", &swarmed_core::hooks::discard)
        .await;
    assert!(matches!(failed, Err(Error::Install { .. })));
    executable(&tools.join("brew"), "exit 0");
    let empty = core
        .agent_install("opencode", &swarmed_core::hooks::discard)
        .await;
    assert!(matches!(empty, Err(Error::Install { reason, .. }) if reason.contains("nowhere")));
}
