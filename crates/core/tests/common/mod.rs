//! Shared fixtures for the integration tests.
#![allow(dead_code, reason = "each test binary uses a different subset")]
#![allow(unreachable_pub, reason = "pub marks the helpers the test files call")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use swarmed_core::agent::{AgentEvent, FakeBackend};
use swarmed_core::{Core, Paths, SessionEvent};
use tempfile::TempDir;
use tokio::sync::broadcast::Receiver;

/// A temporary directory holding a git repository called `demo` and Swarmed's own directories.
pub struct Fixture {
    _dir: TempDir,
    pub repo: PathBuf,
    pub paths: Paths,
}

pub fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("test setup");
    assert!(status.success(), "git {args:?} failed");
}

pub fn fixture() -> Fixture {
    let dir = TempDir::new().expect("test setup");
    // macOS temp dirs are symlinks; git reports the resolved path.
    let root = dir.path().canonicalize().expect("test setup");
    let repo = root.join("demo");
    std::fs::create_dir(&repo).expect("test setup");
    git(&repo, &["init", "--quiet", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "demo\n").expect("test setup");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--quiet", "-m", "init"]);
    let paths = Paths {
        data_dir: root.join("data"),
        worktrees_root: root.join("worktrees"),
        claude_config: root.join("claude.json"),
    };
    Fixture {
        _dir: dir,
        repo,
        paths,
    }
}

pub fn write_config(fx: &Fixture, toml: &str) {
    let path = fx.paths.project_config("demo");
    std::fs::create_dir_all(path.parent().expect("test setup")).expect("test setup");
    std::fs::write(path, toml).expect("test setup");
}

pub async fn core_with_project(fx: &Fixture) -> Core {
    let core =
        Core::with_backend(fx.paths.clone(), Arc::new(FakeBackend::default())).expect("test setup");
    core.project_add(&fx.repo, None).await.expect("test setup");
    core
}

/// The next published event, failing the test instead of hanging if the agent goes quiet.
pub async fn next(bus: &mut Receiver<SessionEvent>) -> SessionEvent {
    tokio::time::timeout(Duration::from_secs(5), bus.recv())
        .await
        .expect("the agent went quiet")
        .expect("the event stream closed")
}

/// One session's events up to and including the end of the turn, as they are stored: without
/// the pieces of replies that are only published.
pub async fn turn(bus: &mut Receiver<SessionEvent>, session_id: i64) -> Vec<AgentEvent> {
    let mut events = vec![];
    loop {
        let published = next(bus).await;
        let piece = matches!(published.event, AgentEvent::TextDelta { .. });
        if published.session_id != session_id || piece {
            continue;
        }
        let done = matches!(published.event, AgentEvent::TurnDone { .. });
        events.push(published.event);
        if done {
            return events;
        }
    }
}
