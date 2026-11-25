//! A task's Chrome as a process: started, written down, found again, and stopped.
//!
//! Which process it is, is written down under `run/browsers/` as a stack's is: its id and when
//! it started. A Swarmed that ended without stopping it finds it again that way, counts it as
//! running, and can connect to it again, since Chrome keeps the address of its endpoint in its
//! profile (`DevToolsActivePort`).

use std::ffi::OsString;
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::stacks;

/// How long Chrome is given to say where its endpoint is.
const START_LIMIT: Duration = Duration::from_secs(30);

/// How long Chrome is given to close by itself once asked, before its processes are ended.
const CLOSE_GRACE: Duration = Duration::from_secs(5);

/// How often Chrome is looked at while it starts or closes.
const POLL: Duration = Duration::from_millis(50);

/// The most of what Chrome printed that is said when it would not start.
const LAST_WORDS: usize = 2_000;

/// What Chrome writes into its profile once its endpoint listens.
pub(crate) const ACTIVE_PORT: &str = "DevToolsActivePort";

/// Where Chrome's own output goes, in the task's browser folder.
pub(crate) const OUTPUT: &str = "chrome.log";

/// A Chrome that has started and listens.
#[derive(Debug)]
pub(crate) struct Started {
    pub(crate) child: Child,
    pub(crate) port: u16,
    pub(crate) path: String,
}

/// What Chrome is started with: without a window, on a profile of the task's own, with its
/// endpoint on a port it picks on `127.0.0.1`, which is where Chrome listens unless told
/// otherwise. No `--remote-allow-origins`: a web page must never reach the endpoint.
pub(crate) fn args(profile: &Path, url: Option<&str>) -> Vec<OsString> {
    let mut profile_arg = OsString::from("--user-data-dir=");
    profile_arg.push(profile);
    let mut args: Vec<OsString> = vec![
        "--headless".into(),
        "--remote-debugging-port=0".into(),
        profile_arg,
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--window-size=1280,800".into(),
    ];
    // Only an address of the web: anything else on Chrome's command line could be read as a
    // switch.
    let page = url.filter(|url| url.starts_with("http://") || url.starts_with("https://"));
    args.push(page.unwrap_or("about:blank").into());
    args
}

/// Starts `program` on `profile`, its output in `output`, and waits until its endpoint
/// listens. Why not, in words, when it does not.
pub(crate) async fn start(
    program: &Path,
    profile: &Path,
    output: &Path,
    url: Option<&str>,
) -> Result<Started, String> {
    let active = profile.join(ACTIVE_PORT);
    // One an earlier Chrome left would point at a port nobody listens on now.
    std::fs::remove_file(&active).ok();
    let out = File::create(output).map_err(|error| error.to_string())?;
    let err = out.try_clone().map_err(|error| error.to_string())?;
    // With the standard library, not Tokio: the process is to outlive whatever runtime asked
    // for it, as a command line that prints the endpoint and ends.
    let mut child = Command::new(program)
        .args(args(profile, url))
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        // A group of its own, so that Chrome's helpers can be ended with it.
        .process_group(0)
        .spawn()
        .map_err(|error| format!("{} could not be started: {error}", program.display()))?;
    let until = Instant::now() + START_LIMIT;
    loop {
        if let Some((port, path)) = endpoint_in(profile) {
            return Ok(Started { child, port, path });
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "it ended before it was ready ({status}){}",
                said(output)
            ));
        }
        if Instant::now() >= until {
            child.kill().ok();
            child.wait().ok();
            return Err(format!("it did not get ready in time{}", said(output)));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The end of what Chrome printed, to follow why it would not start.
fn said(output: &Path) -> String {
    let printed = std::fs::read_to_string(output).unwrap_or_default();
    let printed = printed.trim();
    if printed.is_empty() {
        return String::new();
    }
    let mut from = printed.len().saturating_sub(LAST_WORDS);
    while !printed.is_char_boundary(from) {
        from += 1;
    }
    format!(": {}", &printed[from..])
}

/// Where the Chrome running on `profile` listens, as it wrote it down.
pub(crate) fn endpoint_in(profile: &Path) -> Option<(u16, String)> {
    endpoint_of(&std::fs::read_to_string(profile.join(ACTIVE_PORT)).ok()?)
}

/// The port and the path of the browser's endpoint, from what Chrome writes into
/// `DevToolsActivePort`: the port on its first line, the path on its second.
pub(crate) fn endpoint_of(written: &str) -> Option<(u16, String)> {
    let mut lines = written.lines();
    let port: u16 = lines
        .next()?
        .trim()
        .parse()
        .ok()
        .filter(|port| *port != 0)?;
    let path = lines.next()?.trim();
    path.starts_with("/devtools/browser/")
        .then(|| (port, path.to_string()))
}

/// Writes down which process the task's Chrome is, and when it started: a process id is
/// handed out again sooner or later, and the next holder of it is no Chrome of the task's.
pub(crate) fn keep(record: &Path, pid: u32) {
    let Some(started) = stacks::started_at(pid) else {
        return;
    };
    if let Some(folder) = record.parent() {
        std::fs::create_dir_all(folder).ok();
    }
    std::fs::write(record, format!("{pid}\n{started}\n")).ok();
}

/// The process written down in `record`, if it is still that process. What was written is
/// forgotten otherwise.
pub(crate) fn kept(record: &Path) -> Option<u32> {
    let written = std::fs::read_to_string(record).ok()?;
    let (pid, started) = written.trim().split_once('\n')?;
    let pid: u32 = pid.trim().parse().ok()?;
    if stacks::started_at(pid).as_deref() == Some(started.trim()) {
        return Some(pid);
    }
    std::fs::remove_file(record).ok();
    None
}

/// Ends a Chrome: waits for one that was `asked` to close, then ends its whole group as a
/// stack's is ended. Says whether it is gone.
pub(crate) async fn end(pid: u32, mut child: Option<Child>, asked: bool) -> bool {
    if asked && gone_within(pid, child.as_mut(), CLOSE_GRACE).await {
        return true;
    }
    let gone = stacks::end_group(pid).await;
    if let Some(child) = &mut child {
        child.kill().ok();
        child.try_wait().ok();
    }
    gone
}

/// Waits up to `limit` for the process to end: `None` if it did, else what was started of it
/// back, still running.
pub(crate) async fn ended_within(
    pid: u32,
    mut child: Option<Child>,
    limit: Duration,
) -> Option<Option<Child>> {
    if gone_within(pid, child.as_mut(), limit).await {
        None
    } else {
        Some(child)
    }
}

/// Whether the process ended within `limit`. Looking is also what reaps one of our own.
async fn gone_within(pid: u32, mut child: Option<&mut Child>, limit: Duration) -> bool {
    let until = Instant::now() + limit;
    loop {
        let gone = match child.as_deref_mut() {
            Some(child) => !matches!(child.try_wait(), Ok(None)),
            None => !stacks::alive(pid).await,
        };
        if gone {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_is_read_as_chrome_writes_it_and_nothing_else_is_taken() {
        let written = "53412\n/devtools/browser/5f1c-9e\n";
        assert_eq!(
            endpoint_of(written),
            Some((53412, "/devtools/browser/5f1c-9e".to_string()))
        );
        assert_eq!(endpoint_of("0\n/devtools/browser/x\n"), None);
        assert_eq!(endpoint_of("53412\n"), None);
        assert_eq!(endpoint_of("53412\n/somewhere/else\n"), None);
        assert_eq!(endpoint_of("port\n/devtools/browser/x\n"), None);
    }

    #[test]
    fn chrome_runs_without_a_window_on_the_tasks_profile_and_never_allows_an_origin() {
        let args = args(
            Path::new("/data/browsers/7/profile"),
            Some("http://localhost:3000/"),
        );
        let args: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into())
            .collect();
        assert!(args.contains(&"--headless".to_string()));
        assert!(args.contains(&"--remote-debugging-port=0".to_string()));
        assert!(args.contains(&"--user-data-dir=/data/browsers/7/profile".to_string()));
        assert!(
            !args
                .iter()
                .any(|arg| arg.starts_with("--remote-allow-origins"))
        );
        assert!(
            !args
                .iter()
                .any(|arg| arg.starts_with("--remote-debugging-address"))
        );
        assert_eq!(
            args.last().map(String::as_str),
            Some("http://localhost:3000/")
        );
        let odd = super::args(Path::new("/p"), Some("--load-extension=/x"));
        assert_eq!(
            odd.last().map(|arg| arg.to_string_lossy()),
            Some("about:blank".into())
        );
    }
}
