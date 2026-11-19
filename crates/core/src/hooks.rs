//! Running a project's hook commands.

use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

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
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let (stdout, stderr) = child
        .stdout
        .take()
        .zip(child.stderr.take())
        .ok_or_else(no_pipes)?;
    let mut out = BufReader::new(stdout).lines();
    let mut err = BufReader::new(stderr).lines();
    let (mut out_open, mut err_open) = (true, true);
    while out_open || err_open {
        tokio::select! {
            line = out.next_line(), if out_open => match line? {
                Some(line) => sink(&line),
                None => out_open = false,
            },
            line = err.next_line(), if err_open => match line? {
                Some(line) => sink(&line),
                None => err_open = false,
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

fn no_pipes() -> Error {
    Error::Io(std::io::Error::other("the hook's output pipes are missing"))
}
