//! The built-in repolore, with stand-ins for repolore, uv and Claude Code's own command line,
//! which answer from files a test writes and keep what they were asked.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{fixture, turn};
use swarmed_core::agent::{AgentEvent, Auth, FakeBackend};
use swarmed_core::{
    BuiltinMcpView, BuiltinState, CheckKind, Core, NewTaskOptions, REPOLORE, SessionOptions, hooks,
};

/// Answers each of repolore's commands from a file in `$state`, and notes every call with the
/// home it was about. `uv` stands in for the installer, putting repolore where it is looked
/// for; `claude` for Claude Code, whose remove command takes a registration out.
const REPOLORE_SCRIPT: &str = r#"#!/bin/sh
state="__STATE__"
echo "$* | home=${REPOLORE_HOME-unset} managed=${REPOLORE_MANAGED_BY-unset}" >> "$state/calls"
which=old
if [ -n "$REPOLORE_HOME" ]; then which=builtin; fi
case "$1" in
  version) cat "$state/version" ;;
  doctor) cat "$state/doctor-$which.json" 2>/dev/null || echo '{}' ;;
  server) cat "$state/server-$which.json" 2>/dev/null || echo '{"running": false}' ;;
  copy)
    if [ -f "$state/copy-fails" ]; then
      echo "Traceback (most recent call last):" >&2
      echo "Error: a refresh is running; try again once it is done" >&2
      exit 1
    fi
    mkdir -p "$2" && echo "repos = []" > "$2/repolore.toml" ;;
  session-start) ;;
  *) exit 2 ;;
esac
"#;

const UV_SCRIPT: &str = r#"#!/bin/sh
echo "$*" >> "__STATE__/uv-calls"
echo "Installed 2 executables: repolore, repolore-mcp"
cp "__STATE__/repolore.source" "__TOOLS__/repolore"
cp "__STATE__/repolore-mcp.source" "__TOOLS__/repolore-mcp"
chmod +x "__TOOLS__/repolore" "__TOOLS__/repolore-mcp"
"#;

const CLAUDE_SCRIPT: &str = r#"#!/bin/sh
echo "$*" >> "__STATE__/claude-calls"
echo "Removed MCP server repolore"
mv "__STATE__/doctor-builtin-after.json" "__STATE__/doctor-builtin.json"
"#;

struct Stand {
    fx: common::Fixture,
    tools: PathBuf,
    state: PathBuf,
}

impl Stand {
    fn new() -> Self {
        let fx = fixture();
        let root = fx
            .paths
            .data_dir
            .parent()
            .expect("has a parent")
            .to_path_buf();
        let (tools, state) = (root.join("tools"), root.join("state"));
        std::fs::create_dir_all(&tools).expect("test setup");
        std::fs::create_dir_all(&state).expect("test setup");
        Self { fx, tools, state }
    }

    fn script(&self, text: &str) -> String {
        let state = self.state.to_string_lossy();
        let tools = self.tools.to_string_lossy();
        text.replace("__STATE__", &state)
            .replace("__TOOLS__", &tools)
    }

    /// Puts the stand-in `program` where the core looks for programs.
    fn install(&self, program: &str, text: &str) {
        common::executable(&self.tools.join(program), &self.script(text));
    }

    /// Keeps the stand-ins for repolore where the installer stand-in finds them.
    fn sources(&self) {
        std::fs::write(
            self.state.join("repolore.source"),
            self.script(REPOLORE_SCRIPT),
        )
        .expect("test setup");
        std::fs::write(
            self.state.join("repolore-mcp.source"),
            "#!/bin/sh\nexit 0\n",
        )
        .expect("test setup");
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.state.join(name), text).expect("test setup");
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.state.join(name)).unwrap_or_default()
    }

    async fn core(&self) -> Core {
        let backend = Arc::new(FakeBackend::default());
        let mut core = Core::with_backend(self.fx.paths.clone(), backend).expect("opens");
        core.set_tool_dirs(vec![self.tools.clone()]);
        core.project_add(&self.fx.repo, None)
            .await
            .expect("test setup");
        core
    }

    fn home(&self) -> PathBuf {
        self.fx.paths.repolore_home()
    }

    /// The user's own setup from before, in a folder of its own.
    fn old_folder(&self) -> PathBuf {
        let root = self.fx.paths.data_dir.parent().expect("has a parent");
        let folder = root.join("old").join("repolore");
        std::fs::create_dir_all(&folder).expect("test setup");
        folder
    }
}

fn registration(client: &str, scope: &str, remove: &str, text: &str) -> String {
    format!(
        r#"{{"client": "{client}", "client_name": "", "name": "repolore", "command": ["repolore-mcp"],
            "scope": "{scope}", "for_every_project": true, "remove": {remove}, "remove_text": "{text}"}}"#
    )
}

fn doctor(configured: bool, registrations: &[String], folder: &Path) -> String {
    let folder = folder.to_string_lossy();
    format!(
        r#"{{"version": "0.5.1", "managed_by": null,
            "paths": {{"home": null, "config_file": "{folder}/repolore.toml", "data_dir": "{folder}"}},
            "configured": {configured},
            "index": {{"built_at": "2026-11-20T10:00:00Z", "refreshed_at": "2026-11-27T08:00:00Z"}},
            "checks": [{{"name": "config", "ok": true, "warning": false, "detail": "fine", "fix": null}},
                       {{"name": "model", "ok": true, "warning": true, "detail": "not loaded", "fix": "wait"}}],
            "server": {{"running": false, "pid": null, "version": null, "relays": 0}},
            "registrations": [{}]}}"#,
        registrations.join(", ")
    )
}

async fn status(core: &Core) -> BuiltinMcpView {
    let views = core.builtin_mcp_servers().await.expect("reads");
    assert_eq!(views.len(), 1);
    views.into_iter().next().expect("one")
}

#[tokio::test]
async fn the_built_in_goes_from_missing_to_on_and_back_off() {
    let stand = Stand::new();
    let core = stand.core().await;
    installed_from_nothing(&stand, &core).await;
    let opencode = off_with_a_setup_from_before(&stand, &core).await;
    turned_on_from_a_copy(&stand, &core).await;
    registrations_taken_out(&stand, &core, opencode).await;
    on_then_off(&stand, &core).await;
}

/// Not installed, then installed with uv in a version too old.
async fn installed_from_nothing(stand: &Stand, core: &Core) {
    // Nothing installed, not even uv: Homebrew installs it first.
    let missing = status(core).await;
    assert_eq!(missing.state, BuiltinState::Missing);
    assert_eq!(
        missing.install_command,
        "brew install uv && uv tool install --managed-python --python 3.12 --upgrade \
         'repolore-mcp>=0.5.1'"
    );
    assert_eq!(missing.minimum, "0.5.1");
    assert_eq!(missing.home, stand.home().to_string_lossy());
    let failed = core.builtin_mcp_install(REPOLORE, &hooks::discard).await;
    assert!(failed.is_err_and(|e| e.to_string().contains("nor Homebrew")));

    installed_too_old(stand, core).await;
}

/// uv installs it, in a version too old.
async fn installed_too_old(stand: &Stand, core: &Core) {
    // Printing as it goes.
    stand.sources();
    stand.install("uv", UV_SCRIPT);
    stand.write("version", "repolore 0.5.0\n");
    let printed = Mutex::new(Vec::new());
    let sink = |line: &str| printed.lock().expect("not poisoned").push(line.to_string());
    let outdated = core
        .builtin_mcp_install(REPOLORE, &sink)
        .await
        .expect("installs");
    assert_eq!(
        stand.read("uv-calls").trim(),
        "tool install --managed-python --python 3.12 --upgrade repolore-mcp>=0.5.1"
    );
    assert!(
        printed
            .lock()
            .expect("not poisoned")
            .iter()
            .any(|line| line.contains("Installed"))
    );
    // An older one is offered the upgrade, and is asked nothing more than its version.
    assert_eq!(outdated.state, BuiltinState::Outdated);
    assert_eq!(outdated.version.as_deref(), Some("0.5.0"));
    assert!(
        !stand.read("calls").contains("doctor"),
        "{}",
        stand.read("calls")
    );
    let refused = core.builtin_mcp_turn_on(REPOLORE, false).await;
    assert!(refused.is_err_and(|e| e.to_string().contains("older than 0.5.1")));
}

/// The current one, with a setup from before, registered with Claude Code and OpenCode, whose
/// server still runs. Says OpenCode's registration, which stays.
async fn off_with_a_setup_from_before(stand: &Stand, core: &Core) -> String {
    stand.write("version", "repolore 0.5.1\n");
    stand.install("claude", CLAUDE_SCRIPT);
    let old = stand.old_folder();
    let claude = registration(
        "claude",
        "user",
        r#"["claude", "mcp", "remove", "repolore", "-s", "user"]"#,
        "",
    );
    let opencode = registration(
        "opencode",
        "user",
        "null",
        "Delete repolore from opencode.json.",
    );
    let both = [claude, opencode.clone()];
    stand.write("doctor-builtin.json", &doctor(false, &both, &stand.home()));
    stand.write("doctor-old.json", &doctor(true, &both, &old));
    stand.write(
        "server-old.json",
        r#"{"running": true, "pid": 4242, "relays": 1}"#,
    );
    let off = status(core).await;
    assert_eq!(off.state, BuiltinState::Off, "{off:?}");
    assert_eq!(off.install_command.split(" && ").count(), 1);
    let existing = off.existing.clone().expect("found");
    assert_eq!(existing.folders, [old.to_string_lossy()]);
    assert!(existing.indexed);
    let keys: Vec<&str> = off.registrations.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, ["claude:user:repolore", "opencode:user:repolore"]);
    assert!(off.registrations[0].runs_here);
    assert_eq!(
        off.registrations[0].command.as_deref(),
        Some("claude mcp remove repolore -s user")
    );
    assert!(!off.registrations[1].runs_here);
    assert_eq!(off.old_server.map(|server| server.relays), Some(1));
    assert_eq!(off.checked, Some(CheckKind::Offline));
    assert!(!off.home_ready);
    asked_each_home_apart(stand);
    opencode
}

/// Each home is asked with its own environment: the user's own with none of repolore's.
fn asked_each_home_apart(stand: &Stand) {
    let calls = stand.read("calls");
    let home = stand.home();
    let builtin = format!(
        "doctor --json --offline | home={} managed=Swarmed",
        home.display()
    );
    assert!(calls.contains(&builtin), "{calls}");
    assert!(
        calls.contains("doctor --json --offline | home=unset managed=unset"),
        "{calls}"
    );
    assert!(
        calls.contains("server status --json | home=unset"),
        "{calls}"
    );
}

/// Turned on from a copy of the setup from before, which fails once.
async fn turned_on_from_a_copy(stand: &Stand, core: &Core) {
    let home = stand.home();
    // A copy that fails changes nothing, and says repolore's reason.
    stand.write("copy-fails", "");
    let failed = core.builtin_mcp_turn_on(REPOLORE, true).await;
    assert!(
        failed.is_err_and(|e| e.to_string().contains("a refresh is running")),
        "the reason is passed on"
    );
    assert_eq!(status(core).await.state, BuiltinState::Off);
    std::fs::remove_file(stand.state.join("copy-fails")).expect("test setup");
    let waiting = core
        .builtin_mcp_turn_on(REPOLORE, true)
        .await
        .expect("copies");
    assert_eq!(waiting.state, BuiltinState::Waiting);
    assert!(home.join("repolore.toml").is_file());
    assert!(
        stand
            .read("calls")
            .contains(&format!("copy {} | home=unset", home.display()))
    );
}

/// Claude Code's registration is taken out with its own command; OpenCode's is the user's.
async fn registrations_taken_out(stand: &Stand, core: &Core, opencode: String) {
    let home = stand.home();
    let after = doctor(true, &[opencode], &home);
    stand.write("doctor-builtin-after.json", &after);
    let one_left = core
        .builtin_mcp_remove_registration(REPOLORE, "claude:user:repolore", &hooks::discard)
        .await
        .expect("removes");
    assert_eq!(
        stand.read("claude-calls").trim(),
        "mcp remove repolore -s user"
    );
    assert_eq!(one_left.registrations.len(), 1);
    assert!(one_left.home_ready);
    let yours = core
        .builtin_mcp_remove_registration(REPOLORE, "opencode:user:repolore", &hooks::discard)
        .await;
    assert!(yours.is_err_and(|e| e.to_string().contains("Delete repolore from opencode.json")));
    // A key nobody lists is gone already.
    let gone = core
        .builtin_mcp_remove_registration(REPOLORE, "cursor:user:repolore", &hooks::discard)
        .await
        .expect("nothing to do");
    assert_eq!(gone.state, BuiltinState::Waiting);
}

/// Nothing left elsewhere and the old server stopped: on, with its checks. Then off again.
async fn on_then_off(stand: &Stand, core: &Core) {
    let home = stand.home();
    stand.write("doctor-builtin.json", &doctor(true, &[], &home));
    stand.write("server-old.json", r#"{"running": false, "relays": 0}"#);
    let on = core
        .builtin_mcp_check(REPOLORE, true)
        .await
        .expect("checks");
    assert_eq!(on.state, BuiltinState::On, "{on:?}");
    assert_eq!(on.checked, Some(CheckKind::Full));
    assert!(stand.read("calls").contains("doctor --json | home="));
    assert_eq!(on.built_at.as_deref(), Some("2026-11-20T10:00:00Z"));
    assert_eq!(on.checks.len(), 2);
    assert!(on.old_server.is_none());

    // Off again: its home stays, and its page cannot be opened.
    let off = core
        .builtin_mcp_turn_off(REPOLORE)
        .await
        .expect("turns off");
    assert_eq!(off.state, BuiltinState::Off);
    assert!(home.join("repolore.toml").is_file());
    assert!(
        core.builtin_mcp_open(REPOLORE)
            .is_err_and(|e| e.to_string().contains("on first"))
    );
    assert!(core.builtin_mcp_check("lore", false).await.is_err());
}

/// What the agent of `session` answers `text` with.
async fn said(core: &Core, session: i64, text: &str) -> String {
    let mut bus = core.subscribe();
    core.session_send(session, text).expect("sends");
    let events = turn(&mut bus, session).await;
    let words = events.into_iter().filter_map(|event| match event {
        AgentEvent::Text { text } => Some(text),
        _ => None,
    });
    words.collect()
}

fn started(core: &Core) -> i64 {
    let session = core
        .session_new("demo", "one", &SessionOptions::default())
        .expect("creates");
    core.session_start(session.id, Auth::Subscription)
        .expect("starts");
    session.id
}

/// Waits until `file` in the stand's state says `text`.
async fn noted(stand: &Stand, file: &str, text: &str) -> bool {
    for _ in 0..200 {
        if stand.read(file).contains(text) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

#[tokio::test]
async fn sessions_are_given_the_built_in_only_while_no_other_repolore_is_about() {
    let stand = Stand::new();
    stand.sources();
    stand.install("repolore", REPOLORE_SCRIPT);
    stand.install("repolore-mcp", "#!/bin/sh\nexit 0\n");
    stand.write("version", "repolore 0.5.1\n");
    let home = stand.home();
    stand.write("doctor-builtin.json", &doctor(false, &[], &home));
    let core = stand.core().await;
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");

    // Off: no repolore at all.
    let first = started(&core);
    assert_eq!(said(&core, first, "mcp: repolore").await, "none");

    // On and clear: the built-in, run from its program with its home, and a refresh is started.
    let on = core
        .builtin_mcp_turn_on(REPOLORE, false)
        .await
        .expect("turns on");
    assert_eq!(on.state, BuiltinState::On, "{on:?}");
    assert!(home.is_dir());
    let second = started(&core);
    let program = stand.tools.join("repolore-mcp");
    assert_eq!(
        said(&core, second, "mcp: repolore").await,
        program.to_string_lossy()
    );
    let refresh = format!(
        "session-start --hours 12 | home={} managed=Swarmed",
        home.display()
    );
    assert!(
        noted(&stand, "calls", &refresh).await,
        "{}",
        stand.read("calls")
    );
    assert_eq!(status(&core).await.sessions, []);

    // repolore registered with Claude Code again: new sessions go without the built-in at once,
    // the row waits, and the session that has the old one is held.
    let claude_config = serde_json::json!({
        "mcpServers": { "lore": { "command": "/Users/me/.local/bin/repolore-mcp" } }
    });
    std::fs::write(&stand.fx.paths.claude_config, claude_config.to_string()).expect("test setup");
    let third = started(&core);
    assert_eq!(said(&core, third, "mcp: repolore").await, "none");
    let waiting = status(&core).await;
    assert_eq!(waiting.state, BuiltinState::Waiting);
    let found: Vec<&str> = waiting
        .registrations
        .iter()
        .map(|r| r.key.as_str())
        .collect();
    assert_eq!(found, ["claude:user:lore"]);
    assert!(!waiting.registrations[0].runs_here);
    let held: Vec<(i64, bool)> = waiting.sessions.iter().map(|s| (s.id, s.idle)).collect();
    assert_eq!(held, [(third, true)]);
    // The Trash waits until the built-in is on.
    let refused = core.builtin_mcp_trash_old(REPOLORE).await;
    assert!(refused.is_err_and(|e| e.to_string().contains("only once the built-in")));

    // Its idle agent is stopped, to start again with its next message.
    let restarted = core
        .builtin_mcp_restart_sessions(REPOLORE)
        .await
        .expect("restarts");
    assert_eq!(restarted.sessions, []);
    assert!(!core.running_sessions().contains(&third));
    assert!(core.running_sessions().contains(&second));
}
