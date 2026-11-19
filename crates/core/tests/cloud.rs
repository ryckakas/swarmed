//! Handing work to a cloud agent, with a stand-in for the user's `codex` program.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::{core_with_project, fixture, git};
use swarmed_core::{CloudOrigin, Core, Error, MAX_DELEGATED};

/// A `codex` that knows one environment, `acme/tool`, starts tasks in it, and leaves a log
/// behind the way the real one does.
const CODEX: &str = r#"#!/bin/sh
case "$*" in
  "cloud list --env acme/tool --json"*)
    echo '{"tasks":[{"id":"task_1","status":"ready","title":"t"}],"cursor":null}' ;;
  "cloud list"*)
    echo "Error: environment not found; run \`codex cloud\` to list available environments" >&2
    exit 1 ;;
  "cloud exec --env acme/tool --branch "*)
    echo "$4|$6|$7" > "$(dirname "$0")/asked"
    echo "log" > error.log
    echo "https://chatgpt.com/codex/tasks/task_1" ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#;

async fn core_with_codex(fx: &common::Fixture) -> Core {
    let tools = fx.paths.data_dir.join("tools");
    std::fs::create_dir_all(&tools).expect("dir");
    let codex = tools.join("codex");
    std::fs::write(&codex, CODEX).expect("writes");
    let mut permissions = std::fs::metadata(&codex).expect("exists").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&codex, permissions).expect("sets");
    let mut core = core_with_project(fx).await;
    core.set_tool_dirs(vec![tools]);
    core
}

fn refusal(result: swarmed_core::Result<swarmed_core::CloudTask>) -> String {
    match result {
        Err(Error::Cloud(reason)) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

fn asked(fx: &common::Fixture) -> String {
    let file = fx.paths.data_dir.join("tools").join("asked");
    std::fs::read_to_string(file)
        .expect("codex was asked")
        .trim()
        .to_string()
}

fn head_branch(repo: &Path) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[tokio::test]
async fn work_is_handed_to_a_cloud_agent_and_its_status_followed() {
    let fx = fixture();
    let core = core_with_codex(&fx).await;
    let user = CloudOrigin::default();

    // A repository that is only on this machine has nothing a cloud agent could work on.
    let local = refusal(core.cloud_delegate("demo", "tidy up", user).await);
    assert!(local.contains("not on GitHub"), "{local}");

    // On GitHub, but with no cloud environment: refused, with where to make one.
    git(
        &fx.repo,
        &["remote", "add", "origin", "git@github.com:acme/other.git"],
    );
    let unset = refusal(core.cloud_delegate("demo", "tidy up", user).await);
    assert!(
        unset.starts_with("acme/other has no Codex cloud environment"),
        "{unset}"
    );
    assert!(
        unset.contains("chatgpt.com/codex, under Environments"),
        "{unset}"
    );
    assert_eq!(
        core.cloud_tasks("demo", false).await.expect("lists"),
        vec![]
    );

    git(
        &fx.repo,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme/tool.git",
        ],
    );
    let task = core
        .cloud_delegate("demo", "  tidy up the docs  ", user)
        .await
        .expect("delegates");
    assert_eq!(task.url, "https://chatgpt.com/codex/tasks/task_1");
    assert_eq!(
        (task.remote_id.as_str(), task.status.as_str()),
        ("task_1", "pending")
    );
    assert_eq!((task.provider.as_str(), task.session_id), ("codex", None));
    // Codex was given the environment, the repository's own branch and the prompt.
    let branch = head_branch(&fx.repo);
    assert_eq!(asked(&fx), format!("acme/tool|{branch}|tidy up the docs"));
    assert_eq!(task.branch, branch);
    // The log Codex leaves where it runs is not kept, and never lands in the repository.
    assert!(!fx.paths.data_dir.join("run/cloud/error.log").exists());
    assert!(!fx.repo.join("error.log").exists());

    // As last heard of, until asked again; and listed with the project.
    assert_eq!(
        core.cloud_tasks("demo", false).await.expect("lists")[0].status,
        "pending"
    );
    assert_eq!(
        core.cloud_tasks("demo", true).await.expect("lists")[0].status,
        "ready"
    );
    assert_eq!(core.overview().expect("reads")[0].cloud.len(), 1);
    core.cloud_task_forget(task.id).expect("forgets");
    assert_eq!(core.overview().expect("reads")[0].cloud, vec![]);

    assert!(matches!(
        core.cloud_delegate("demo", "  ", user).await,
        Err(Error::EmptyMessage)
    ));
}

#[tokio::test]
async fn a_session_delegates_only_so_much() {
    let fx = fixture();
    let core = core_with_codex(&fx).await;
    git(
        &fx.repo,
        &["remote", "add", "origin", "https://github.com/acme/tool"],
    );
    let agent = CloudOrigin {
        session: Some(7),
        ..CloudOrigin::default()
    };
    for _ in 0..MAX_DELEGATED {
        let task = core.cloud_delegate("demo", "a part", agent).await;
        assert_eq!(task.expect("delegates").session_id, Some(7));
    }
    let more = refusal(core.cloud_delegate("demo", "one more", agent).await);
    assert!(more.contains("the most allowed"), "{more}");
    // Another session, and the user, are not held to its count.
    let other = CloudOrigin {
        session: Some(8),
        ..CloudOrigin::default()
    };
    core.cloud_delegate("demo", "mine", other)
        .await
        .expect("delegates");
    core.cloud_delegate("demo", "the user's", CloudOrigin::default())
        .await
        .expect("delegates");
}
