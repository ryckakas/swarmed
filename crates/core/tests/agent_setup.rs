//! Setting up an agent other than Claude Code, with a stand-in for Homebrew.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

use common::fixture;
use swarmed_core::{Core, Error};

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

#[test]
fn an_installed_agent_is_configured_without_editing_the_file() {
    let fx = fixture();
    let tools = fx.paths.data_dir.join("tools");
    let core = core_with_tools(&fx, &tools);

    let before = core.agent_setups().expect("reads");
    assert_eq!(before[0].id, "opencode");
    assert_eq!(
        (before[0].installed.clone(), before[0].configured),
        (None, false)
    );
    assert!(before[0].install_command.starts_with("brew install "));

    // Something the user wrote by hand must survive.
    let config = fx.paths.agents_config();
    std::fs::write(&config, "[goose]\ncommand = \"goose\"\nargs = [\"acp\"]\n").expect("writes");
    executable(&tools.join("opencode"), "exit 0");

    let setup = core.agent_configure("opencode", None).expect("configures");
    assert_eq!(setup.installed, Some(tools.join("opencode")));
    assert!(setup.configured);
    assert_ne!(setup.models, Vec::<String>::new());
    // It is usable at once, started by its whole path, and the other entry is still there.
    let ids: Vec<String> = core.agents().into_iter().map(|agent| agent.id).collect();
    assert_eq!(ids, ["claude", "goose", "opencode"]);
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
