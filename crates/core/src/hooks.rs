//! Running a project's hook commands.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::error::{Error, Result};

/// Receives hook output line by line, so a front end can show it while the hook runs.
pub type OutputSink<'a> = &'a (dyn Fn(&str) + Send + Sync);

/// An [`OutputSink`] that drops everything.
pub fn discard(_line: &str) {}

/// Runs `command` with `sh -c` in `cwd`. Stdout and stderr are forwarded to `sink` as they
/// arrive. An empty command is a no-op. A non-zero exit is an error.
pub(crate) async fn run(
    hook: &'static str,
    command: &str,
    cwd: &Path,
    env: &[(String, String)],
    sink: OutputSink<'_>,
) -> Result<()> {
    if command.trim().is_empty() {
        return Ok(());
    }
    let mut child = shell(command, cwd, env).spawn()?;
    follow(hook, &mut child, sink).await
}

/// Runs a hook as [`run`] does, for no longer than `limit`: `None` when it took longer. It is
/// then killed with everything it started that is still in its process group, which it is
/// given for that: killing the shell alone leaves what the shell was waiting for.
pub(crate) async fn run_within(
    limit: Duration,
    hook: &'static str,
    command: &str,
    cwd: &Path,
    env: &[(String, String)],
    sink: OutputSink<'_>,
) -> Option<Result<()>> {
    if command.trim().is_empty() {
        return Some(Ok(()));
    }
    let mut child = match shell(command, cwd, env).process_group(0).spawn() {
        Ok(child) => child,
        Err(error) => return Some(Err(error.into())),
    };
    let group = child.id();
    let ran = tokio::time::timeout(limit, follow(hook, &mut child, sink))
        .await
        .ok();
    if let (None, Some(group)) = (&ran, group) {
        // The group's id is that of the process that leads it; a minus addresses the group.
        let killing = Command::new("kill")
            .args(["-KILL", "--", &format!("-{group}")])
            .stderr(Stdio::null())
            .status();
        killing.await.ok();
    }
    ran
}

/// `sh -c command`, with the output piped and the `PATH` of the user's shell.
fn shell(command: &str, cwd: &Path, env: &[(String, String)]) -> Command {
    let mut shell = Command::new("sh");
    shell
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        // Before the task's own variables, which may set a `PATH` of their own.
        .env("PATH", crate::shellenv::path())
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    shell
}

/// Passes on what a started hook prints until it has closed its output, then waits for it.
async fn follow(hook: &'static str, child: &mut Child, sink: OutputSink<'_>) -> Result<()> {
    let (stdout, stderr) = child
        .stdout
        .take()
        .zip(child.stderr.take())
        .ok_or_else(no_pipes)?;
    let (mut out, mut err) = (BufReader::new(stdout), BufReader::new(stderr));
    // One line each, kept over the rounds: a read that the other pipe cut short goes on
    // where it was.
    let (mut out_line, mut err_line) = (Vec::new(), Vec::new());
    let (mut out_open, mut err_open) = (true, true);
    while out_open || err_open {
        tokio::select! {
            read = out.read_until(b'\n', &mut out_line), if out_open => {
                out_open = passed_on(read?, &mut out_line, sink);
            },
            read = err.read_until(b'\n', &mut err_line), if err_open => {
                err_open = passed_on(read?, &mut err_line, sink);
            },
        }
    }

    let status = child.wait().await?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::HookFailed {
            hook,
            status: status.to_string(),
        })
    }
}

/// Hands a line a hook printed to `sink` and empties it, and says whether more may follow:
/// nothing read means the pipe is closed. A hook may print bytes that are no text, as a
/// build tool does with a file's name or in colours of its own; they are shown as best they
/// can be and are no reason to end the hook.
fn passed_on(read: usize, line: &mut Vec<u8>, sink: OutputSink<'_>) -> bool {
    if !line.is_empty() {
        let text = String::from_utf8_lossy(line);
        let text = text.strip_suffix('\n').unwrap_or(&text);
        sink(text.strip_suffix('\r').unwrap_or(text));
        line.clear();
    }
    read > 0
}

fn no_pipes() -> Error {
    Error::Io(std::io::Error::other("the hook's output pipes are missing"))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[tokio::test]
    async fn output_that_is_no_text_does_not_end_a_hook() {
        let dir = tempfile::tempdir().expect("temp dir");
        let lines = Mutex::new(Vec::new());
        let sink = |line: &str| lines.lock().expect("lock").push(line.to_string());
        // A byte that is no UTF-8 on each pipe, then the work the hook is there for.
        let command = "printf 'ol\\351\\n'; printf '\\377\\n' >&2; echo done > done.txt; echo last";
        let ran = run("setup", command, dir.path(), &[], &sink).await;
        assert!(ran.is_ok(), "{ran:?}");
        assert!(dir.path().join("done.txt").exists());
        let lines = lines.into_inner().expect("lock");
        assert!(lines.contains(&"ol\u{fffd}".to_string()), "{lines:?}");
        assert!(lines.contains(&"last".to_string()), "{lines:?}");
    }

    #[tokio::test]
    async fn a_hook_that_takes_too_long_is_killed_with_what_it_started() {
        let dir = tempfile::tempdir().expect("temp dir");
        let lines = Mutex::new(Vec::new());
        let sink = |line: &str| lines.lock().expect("lock").push(line.to_string());
        // What the shell started holds the hook's output open and is waited for.
        let command = "sleep 60 & echo $!; wait";
        let limit = Duration::from_millis(500);
        let ran = run_within(limit, "stop", command, dir.path(), &[], &sink).await;
        assert!(ran.is_none(), "{ran:?}");
        let lines = lines.into_inner().expect("lock");
        let started = lines.first().expect("the hook said what it started");
        assert!(gone_soon(started).await, "process {started} still runs");
    }

    /// Whether a process ends within a few seconds.
    async fn gone_soon(pid: &str) -> bool {
        let mut asked = 0;
        while asked < 50 && alive(pid).await {
            tokio::time::sleep(Duration::from_millis(100)).await;
            asked += 1;
        }
        !alive(pid).await
    }

    /// `kill -0` asks whether a process is there without sending it anything.
    async fn alive(pid: &str) -> bool {
        let asked = Command::new("kill")
            .args(["-0", pid])
            .stderr(Stdio::null())
            .status()
            .await;
        asked.is_ok_and(|status| status.success())
    }

    #[tokio::test]
    async fn a_hook_within_its_limit_is_run_as_any_other() {
        let dir = tempfile::tempdir().expect("temp dir");
        let limit = Duration::from_secs(20);
        let ran = run_within(limit, "stop", "exit 3", dir.path(), &[], &discard).await;
        assert!(
            matches!(ran, Some(Err(Error::HookFailed { .. }))),
            "{ran:?}"
        );
        let ran = run_within(limit, "stop", "true", dir.path(), &[], &discard).await;
        assert!(matches!(ran, Some(Ok(()))), "{ran:?}");
    }
}
