//! Pull requests and issues the user points an agent at with a message: by `#` in the message
//! box, or with `--pull` and `--issue` on the command line.
//!
//! They are read through the user's own `gh` in the repository, as `pulls.rs` and `issues.rs`
//! read theirs: Swarmed holds no GitHub token, and a repository that is not on GitHub, or a
//! machine with no `gh`, has none to offer. A chat has no repository and none either. Only a
//! reference is sent: each one's number, title, state, branches and address, listed under the
//! message in one fixed form ([`with_github`]), so that the agent knows what it is and in which
//! repository, and reads more of it with `gh` when it needs to. Front ends read the list back
//! to show each as a chip. A number typed by hand is not looked up: only one picked is listed.
//!
//! What was read is kept in memory ([`Known`]), so that sending a message asks nothing of the
//! network: it is composed from what the menu listed, or from what a front end had read just
//! before it sent ([`Core::project_github_read`]). A message that waits in a queue goes with
//! each one's state at the time it is sent, not when it was typed (the user's choice): while
//! one waits, what it names is read again every [`FRESH_EVERY`]
//! ([`Core::github_waiting_read_again`]), and its list is written again from that as it goes
//! ([`fresh_list`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::agent_setup::find_program;
use crate::error::{Error, Result};
use crate::mentions::one_line;
use crate::merging::gh_words;
use crate::pulls::{GhFailed, ask_gh};
use crate::sessions::Live;
use crate::{Core, lock};

/// The line that opens the list of pull requests and issues a message mentions.
pub const MENTIONED_ON_GITHUB: &str = "Pull requests and issues mentioned, to read with gh:";

/// How many pull requests, and how many issues, the `#` menu is offered at most.
pub const GITHUB_LISTED: u32 = 30;

/// How often what a waiting message names is read again, so that it goes with each one's
/// state as it is when it is sent.
pub const FRESH_EVERY: Duration = Duration::from_secs(30);

/// How old a reading may be and still go into a message sent now: one the `#` menu listed a
/// moment ago is not asked for again.
const STALE_AFTER: Duration = Duration::from_secs(60);

/// How long a reading is kept when nothing reads it again: a waiting message's are read
/// again all the time.
const KEPT_FOR: Duration = Duration::from_hours(1);

/// What `gh pr view` and `gh pr list` are asked for.
const PULL_FIELDS: &str = "number,title,state,isDraft,headRefName,baseRefName,url,updatedAt";

/// What `gh issue view` and `gh issue list` are asked for.
const ISSUE_FIELDS: &str = "number,title,state,url,updatedAt";

/// Why there is no `gh` to ask.
const NO_GH: &str = "the GitHub CLI `gh` is not installed: https://cli.github.com";

/// Whether a numbered thing on GitHub is a pull request or an issue: they share one count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubKind {
    /// A pull request.
    Pull,
    /// An issue.
    Issue,
}

/// Where a pull request or an issue stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubState {
    /// Open, and for a pull request no draft.
    Open,
    /// A pull request that is open as a draft.
    Draft,
    /// A pull request merged into its base.
    Merged,
    /// Closed: an issue for whatever reason, a pull request without merging.
    Closed,
}

impl GitHubState {
    /// The word the list under a message says it with.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Draft => "draft",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }
}

/// A pull request or an issue of a repository, as the `#` menu offers it and a message names
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubItem {
    /// A pull request or an issue.
    pub kind: GitHubKind,
    /// Its number in the repository.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Where it stands.
    pub state: GitHubState,
    /// Where it is on GitHub, which also says the repository.
    pub url: String,
    /// A pull request's branch; nothing for an issue.
    pub head: Option<String>,
    /// The branch a pull request goes into; nothing for an issue.
    pub base: Option<String>,
    /// When it last changed, as GitHub writes it (`2026-11-20T10:00:00Z`).
    pub updated_at: String,
}

/// What the `#` menu offers, or why it offers nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubItems {
    /// The pull requests and issues, the latest changed first.
    pub items: Vec<GitHubItem>,
    /// Why there are none, when they could not be asked for: no `gh`, a repository that is
    /// not on GitHub, no sign-in, no such number, in `gh`'s words.
    pub unavailable: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItem {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    head_ref_name: String,
    #[serde(default)]
    base_ref_name: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    updated_at: String,
}

fn item(kind: GitHubKind, raw: RawItem) -> GitHubItem {
    // GitHub has no state but these, and only a pull request is merged or a draft.
    let state = match raw.state.as_str() {
        "MERGED" => GitHubState::Merged,
        "CLOSED" => GitHubState::Closed,
        _ if raw.is_draft => GitHubState::Draft,
        _ => GitHubState::Open,
    };
    let branch = |name: String| (kind == GitHubKind::Pull && !name.is_empty()).then_some(name);
    GitHubItem {
        kind,
        number: raw.number,
        title: raw.title,
        state,
        url: raw.url,
        head: branch(raw.head_ref_name),
        base: branch(raw.base_ref_name),
        updated_at: raw.updated_at,
    }
}

/// Reads what `gh pr list --json` or `gh issue list --json` printed.
fn parse_list(kind: GitHubKind, json: &str) -> Result<Vec<GitHubItem>> {
    let raw: Vec<RawItem> = serde_json::from_str(json)?;
    Ok(raw.into_iter().map(|raw| item(kind, raw)).collect())
}

/// Reads what `gh pr view --json` or `gh issue view --json` printed.
fn parse_one(kind: GitHubKind, json: &str) -> Result<GitHubItem> {
    Ok(item(kind, serde_json::from_str(json)?))
}

/// What `gh` is asked to list the `kind` that `words` find, or the latest changed with none.
fn list_args(kind: GitHubKind, words: &str) -> Vec<String> {
    let (command, fields) = match kind {
        GitHubKind::Pull => ("pr", PULL_FIELDS),
        GitHubKind::Issue => ("issue", ISSUE_FIELDS),
    };
    let limit = GITHUB_LISTED.to_string();
    let mut args: Vec<String> = [command, "list", "--state", "all", "--limit", &limit]
        .map(String::from)
        .into();
    args.extend(["--json".to_string(), fields.to_string()]);
    let words = words.trim();
    if !words.is_empty() {
        // In one piece, so that words starting with a dash are never taken for an option.
        args.push(format!("--search={words}"));
    }
    args
}

/// Pull requests and issues in the order the menu shows them: the latest changed first, and
/// of two changed at the same moment the higher number.
fn ordered(mut items: Vec<GitHubItem>) -> Vec<GitHubItem> {
    items.sort_by(|a, b| (b.updated_at.as_str(), b.number).cmp(&(a.updated_at.as_str(), a.number)));
    items
}

/// `numbers` in the order given, each once.
fn each_once(numbers: &[u64]) -> Vec<u64> {
    let mut seen = Vec::with_capacity(numbers.len());
    for number in numbers {
        if !seen.contains(number) {
            seen.push(*number);
        }
    }
    seen
}

/// One line of the list under a message:
/// `- pull request #142 ("Fix the login form", open, fix-login into main): <address>`.
fn listed(item: &GitHubItem) -> String {
    let kind = match item.kind {
        GitHubKind::Pull => "pull request",
        GitHubKind::Issue => "issue",
    };
    let branches = match (&item.head, &item.base) {
        (Some(head), Some(base)) => format!(", {head} into {base}"),
        _ => String::new(),
    };
    let (number, title, state) = (item.number, one_line(&item.title), item.state.word());
    format!(
        "- {kind} #{number} (\"{title}\", {state}{branches}): {}",
        item.url
    )
}

/// `text` with `items` listed under it, in the form every front end reads back: a heading
/// line, then one line for each. Nothing is added for none.
#[must_use]
pub fn with_github(text: &str, items: &[GitHubItem]) -> String {
    let text = text.trim();
    if items.is_empty() {
        return text.to_string();
    }
    let mut message = String::from(text);
    if !message.is_empty() {
        message.push_str("\n\n");
    }
    message.push_str(MENTIONED_ON_GITHUB);
    for item in items {
        message.push('\n');
        message.push_str(&listed(item));
    }
    message
}

/// The address a line of the list names, if it is one. Only a web address is one: it is handed
/// to `gh` to read again, and whoever wrote the message wrote the line.
fn address_of(line: &str) -> Option<&str> {
    let one = line.starts_with("- pull request #") || line.starts_with("- issue #");
    let (_, address) = line.rsplit_once("): ").filter(|_| one)?;
    let web = address.starts_with("https://") && !address.contains(char::is_whitespace);
    web.then_some(address)
}

/// Where the list under `text` starts, after its heading, and how many lines it has: the last
/// heading in it, which only lines of the list follow, up to the next empty line.
fn list_in<'a>(lines: &[&'a str]) -> Option<(usize, Vec<&'a str>)> {
    let heading = lines
        .iter()
        .rposition(|line| *line == MENTIONED_ON_GITHUB)?;
    let after = &lines[heading + 1..];
    let addresses: Vec<&str> = after.iter().map_while(|line| address_of(line)).collect();
    (!addresses.is_empty()).then_some((heading + 1, addresses))
}

/// The addresses the list under `text` names; none if it has no list.
pub(crate) fn addresses_in(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let listed = list_in(&lines).map(|(_, addresses)| addresses);
    listed
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// `text` with each line of the list under it written again from the latest reading
/// `known` has of what it names: for a message that waited, which goes with each one's state
/// as it is now. A line whose reading is gone stays as it was.
pub(crate) fn fresh_list(text: &str, known: &Known) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let Some((first, addresses)) = list_in(&lines) else {
        return text.to_string();
    };
    let mut fresh: Vec<String> = lines.iter().map(ToString::to_string).collect();
    for (at, address) in addresses.into_iter().enumerate() {
        if let Some(item) = known.at(address) {
            fresh[first + at] = listed(&item);
        }
    }
    fresh.join("\n")
}

/// The latest reading of a pull request or an issue: when, and which repositories asked for
/// it, by id (two repositories can be clones of one on GitHub).
#[derive(Debug, Clone)]
struct Reading {
    item: GitHubItem,
    at: Instant,
    projects: Vec<i64>,
}

/// What this process has read of the repositories' pull requests and issues, by address, and
/// whether the waiting messages are being followed.
#[derive(Debug, Default)]
pub(crate) struct Known {
    read: Mutex<HashMap<String, Reading>>,
    following: AtomicBool,
}

impl Known {
    /// Keeps what was just read of `project`'s, and lets go of readings nobody renewed.
    fn keep(&self, project: i64, items: &[GitHubItem]) {
        let mut read = lock(&self.read);
        read.retain(|_, reading| reading.at.elapsed() < KEPT_FOR);
        for item in items {
            take_in(&mut read, item, Some(project));
        }
    }

    /// The latest reading of `project`'s number `number`, if it is no older than `within`.
    fn of(&self, project: i64, number: u64, within: Duration) -> Option<GitHubItem> {
        let read = lock(&self.read);
        let mut found = read
            .values()
            .filter(|reading| reading.item.number == number && reading.projects.contains(&project));
        let reading = found.find(|reading| reading.at.elapsed() < within)?;
        Some(reading.item.clone())
    }

    /// The latest reading of what is at `address`.
    fn at(&self, address: &str) -> Option<GitHubItem> {
        lock(&self.read)
            .get(address)
            .map(|reading| reading.item.clone())
    }

    /// Takes in a reading of what is at its address, which the waiting messages name.
    fn renew(&self, item: &GitHubItem) {
        take_in(&mut lock(&self.read), item, None);
    }
}

/// `item` as the latest reading of its address in `read`, which keeps the repositories that
/// asked for it before, and `project` too if one asks now.
fn take_in(read: &mut HashMap<String, Reading>, item: &GitHubItem, project: Option<i64>) {
    let reading = read.entry(item.url.clone()).or_insert_with(|| Reading {
        item: item.clone(),
        at: Instant::now(),
        projects: Vec::new(),
    });
    reading.item = item.clone();
    reading.at = Instant::now();
    if let Some(project) = project.filter(|project| !reading.projects.contains(project)) {
        reading.projects.push(project);
    }
}

/// Which `gh` command reads what is at `address`, by the kind its path says.
fn kind_at(address: &str) -> GitHubKind {
    if address.contains("/pull/") {
        GitHubKind::Pull
    } else {
        GitHubKind::Issue
    }
}

/// What `gh` says of `what` (a number, or an address) as a `kind`, asked in `dir`.
async fn read_as(gh: &Path, dir: &Path, kind: GitHubKind, what: &str) -> Result<GitHubItem> {
    let (command, fields) = match kind {
        GitHubKind::Pull => ("pr", PULL_FIELDS),
        GitHubKind::Issue => ("issue", ISSUE_FIELDS),
    };
    let args = [command, "view", what, "--json", fields];
    let printed = ask_gh(gh, dir, &args).await;
    let printed = printed.map_err(|GhFailed(said)| Error::Issue(gh_words(&said)))?;
    parse_one(kind, &printed)
}

/// Number `number` of the repository at `repo`: a pull request if it is one, else an issue.
/// `gh issue view` answers for a pull request too, but without its branches.
async fn read_number(gh: &Path, repo: &Path, number: u64) -> Result<GitHubItem> {
    let number = number.to_string();
    if let Ok(pull) = read_as(gh, repo, GitHubKind::Pull, &number).await {
        return Ok(pull);
    }
    read_as(gh, repo, GitHubKind::Issue, &number).await
}

/// Reads again what the waiting messages name, each once, every [`FRESH_EVERY`], until none
/// waits. One follower at a time, started when such a message is queued.
async fn follow_waiting(live: Arc<Live>, gh: PathBuf, dir: PathBuf) {
    loop {
        tokio::time::sleep(FRESH_EVERY).await;
        if !(read_again(&live, &gh, &dir).await || goes_on(&live)) {
            return;
        }
    }
}

/// Whether a follower that found nothing waiting goes on after all: it stops following, then
/// looks once more, since a message queued between its look and now would wait unread. It
/// goes on only if no other follower started meanwhile.
fn goes_on(live: &Live) -> bool {
    live.github.following.store(false, Ordering::SeqCst);
    let more = !live.queues.addresses_waiting().is_empty();
    more && !live.github.following.swap(true, Ordering::SeqCst)
}

/// Reads again what the waiting messages name; whether any names something.
async fn read_again(live: &Live, gh: &Path, dir: &Path) -> bool {
    let addresses = live.queues.addresses_waiting();
    for address in &addresses {
        // One that cannot be read now goes as it was last read.
        if let Ok(item) = read_as(gh, dir, kind_at(address), address).await {
            live.github.renew(&item);
        }
    }
    !addresses.is_empty()
}

impl Core {
    /// What the `#` menu offers for `query` in a repository, through the user's own `gh`: with
    /// nothing typed, its latest changed pull requests and issues, up to [`GITHUB_LISTED`] of
    /// each; for a number, that one; for words, those GitHub's search finds.
    ///
    /// Not an error when they cannot be asked for: without `gh`, in a repository that is not
    /// on GitHub, or with `gh` signed out, the list is empty and says why, in `gh`'s words.
    /// A repository with issues turned off offers its pull requests. Refused for the chats,
    /// which have no repository. What is listed is kept, so that a message mentioning it is
    /// sent without asking again.
    pub async fn project_github_items(&self, project: &str, query: &str) -> Result<GitHubItems> {
        let project = self.project(project)?;
        let repo = project.repo()?;
        let none = |why: String| GitHubItems {
            items: Vec::new(),
            unavailable: Some(why),
        };
        let Some(gh) = find_program("gh", &self.tool_dirs) else {
            return Ok(none(NO_GH.to_string()));
        };
        let query = query.trim();
        let listed = match query.parse::<u64>() {
            Ok(number) => read_number(&gh, repo, number).await.map(|item| vec![item]),
            Err(_) => list(&gh, repo, query).await,
        };
        match listed {
            Ok(items) => {
                self.live.github.keep(project.id, &items);
                Ok(GitHubItems {
                    items: ordered(items),
                    unavailable: None,
                })
            }
            Err(Error::Issue(why)) => Ok(none(why)),
            Err(error) => Err(error),
        }
    }

    /// The pull requests and issues `numbers` of a repository, as a message mentioning them
    /// goes now: what was read in the last minute as it was, the rest read through `gh`.
    /// Refused with `gh`'s words for one that cannot be read. A front end asks this before it
    /// sends such a message, so that the message itself waits on nothing.
    pub async fn project_github_read(
        &self,
        project: &str,
        numbers: &[u64],
    ) -> Result<Vec<GitHubItem>> {
        let project = self.project(project)?;
        let repo = project.repo()?;
        let mut items = Vec::new();
        for number in each_once(numbers) {
            let known = self.live.github.of(project.id, number, STALE_AFTER);
            let item = match known {
                Some(item) => item,
                None => self.read_now(project.id, repo, number).await?,
            };
            items.push(item);
        }
        Ok(items)
    }

    /// Number `number` of the repository at `repo`, project `project`, read through `gh` now
    /// and kept; refused with `gh`'s words when it cannot be read.
    async fn read_now(&self, project: i64, repo: &Path, number: u64) -> Result<GitHubItem> {
        let gh = find_program("gh", &self.tool_dirs).ok_or_else(|| cannot(number, NO_GH))?;
        let item = read_number(&gh, repo, number).await;
        let item = item.map_err(|error| cannot(number, &error.to_string()))?;
        self.live.github.keep(project, std::slice::from_ref(&item));
        Ok(item)
    }

    /// The pull requests and issues `numbers` of the repository session `id` works in, as
    /// [`Core::project_github_read`] reads them.
    pub async fn session_github_read(&self, id: i64, numbers: &[u64]) -> Result<Vec<GitHubItem>> {
        let project = self.project_of_session(id)?;
        self.project_github_read(&project.name, numbers).await
    }

    /// Reads again, now, what the messages waiting in every queue mention, as the follower
    /// does every [`FRESH_EVERY`] while one waits: each goes with what was read last.
    pub async fn github_waiting_read_again(&self) {
        if let Some(gh) = find_program("gh", &self.tool_dirs) {
            read_again(&self.live, &gh, &self.paths.data_dir).await;
        }
    }

    /// `text` with the pull requests and issues `numbers` of `project` listed under it, as
    /// they were last read. Refused for a chat, which has no repository, and for one that was
    /// not read: a front end reads it first ([`Core::project_github_read`]).
    pub(crate) fn github_listed(
        &self,
        project: i64,
        text: &str,
        numbers: &[u64],
    ) -> Result<String> {
        if numbers.is_empty() {
            return Ok(text.trim().to_string());
        }
        let home = self.store().project_by_id(project)?;
        home.as_ref().ok_or(Error::NoRepository)?.repo()?;
        let items = each_once(numbers)
            .into_iter()
            .map(|number| {
                let item = self.live.github.of(project, number, KEPT_FOR);
                item.ok_or_else(|| cannot(number, "it was not read from GitHub: pick it with #"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(with_github(text, &items))
    }

    /// Follows what the waiting messages mention, so that each goes with the state of what it
    /// names when it is sent: started when a message is queued, if one mentions any and
    /// nothing follows them yet. Without `gh`, or outside a runtime, nothing is followed, and
    /// each goes as it was read.
    pub(crate) fn github_follow_waiting(&self) {
        if self.live.queues.addresses_waiting().is_empty() {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let Some(gh) = find_program("gh", &self.tool_dirs) else {
            return;
        };
        if self.live.github.following.swap(true, Ordering::SeqCst) {
            return;
        }
        let live = Arc::clone(&self.live);
        runtime.spawn(follow_waiting(live, gh, self.paths.data_dir.clone()));
    }
}

/// The pull requests and issues of the repository at `repo` that `words` find, or the latest
/// changed: both lists asked at once. One that `gh` cannot give, such as the issues of a
/// repository that has them turned off, is left out; both failing says why.
async fn list(gh: &Path, repo: &Path, words: &str) -> Result<Vec<GitHubItem>> {
    let ask = |kind: GitHubKind| async move {
        let args = list_args(kind, words);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let printed = ask_gh(gh, repo, &args).await;
        let printed = printed.map_err(|GhFailed(said)| Error::Issue(gh_words(&said)))?;
        parse_list(kind, &printed)
    };
    let (pulls, issues) = tokio::join!(ask(GitHubKind::Pull), ask(GitHubKind::Issue));
    match (pulls, issues) {
        (Err(error), Err(_)) => Err(error),
        (pulls, issues) => Ok([pulls, issues].into_iter().flatten().flatten().collect()),
    }
}

fn cannot(number: u64, why: &str) -> Error {
    Error::CannotMentionOnGitHub(number, why.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull(number: u64, title: &str, state: GitHubState) -> GitHubItem {
        GitHubItem {
            kind: GitHubKind::Pull,
            number,
            title: title.to_string(),
            state,
            url: format!("https://github.com/o/r/pull/{number}"),
            head: Some("fix-login".to_string()),
            base: Some("main".to_string()),
            updated_at: "2026-11-20T10:00:00Z".to_string(),
        }
    }

    fn issue(number: u64, title: &str) -> GitHubItem {
        GitHubItem {
            kind: GitHubKind::Issue,
            number,
            title: title.to_string(),
            state: GitHubState::Closed,
            url: format!("https://github.com/o/r/issues/{number}"),
            head: None,
            base: None,
            updated_at: "2026-11-21T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn what_gh_lists_is_read_as_pull_requests_and_issues() {
        let printed = r#"[
            {"number": 142, "title": "Fix the login form", "state": "OPEN", "isDraft": true,
             "headRefName": "fix-login", "baseRefName": "main",
             "url": "https://github.com/o/r/pull/142", "updatedAt": "2026-11-20T10:00:00Z"},
            {"number": 140, "title": "Old", "state": "MERGED", "isDraft": true,
             "headRefName": "old", "baseRefName": "main", "url": "u", "updatedAt": ""},
            {"number": 139, "title": "Gone", "state": "CLOSED", "url": "u", "updatedAt": ""}
        ]"#;
        let pulls = parse_list(GitHubKind::Pull, printed).expect("parses");
        // A draft that is open is a draft; one merged or closed is that.
        let states: Vec<GitHubState> = pulls.iter().map(|pull| pull.state).collect();
        assert_eq!(
            states,
            [GitHubState::Draft, GitHubState::Merged, GitHubState::Closed]
        );
        assert_eq!(pulls[0].head.as_deref(), Some("fix-login"));
        assert_eq!(pulls[2].head, None, "a branch left out is none");
        let issues = parse_list(
            GitHubKind::Issue,
            r#"[{"number": 12, "title": "Login loops", "state": "OPEN", "url": "u12",
                 "updatedAt": "2026-11-22T10:00:00Z", "headRefName": "never"}]"#,
        )
        .expect("parses");
        assert_eq!(issues[0].state, GitHubState::Open);
        assert_eq!(issues[0].head, None, "an issue has no branches");
        let numbers: Vec<u64> = ordered([pulls, issues].concat())
            .iter()
            .map(|item| item.number)
            .collect();
        assert_eq!(numbers, [12, 142, 140, 139], "the latest changed first");
        assert!(parse_list(GitHubKind::Pull, "no pull requests match").is_err());
    }

    #[test]
    fn the_menu_asks_gh_for_both_kinds_with_the_words_in_one_piece() {
        assert_eq!(
            list_args(GitHubKind::Pull, " "),
            [
                "pr",
                "list",
                "--state",
                "all",
                "--limit",
                "30",
                "--json",
                PULL_FIELDS
            ]
        );
        let issues = list_args(GitHubKind::Issue, " -label:bug login ");
        assert_eq!(issues[..2], ["issue", "list"]);
        assert_eq!(issues[7], ISSUE_FIELDS);
        assert_eq!(issues[8], "--search=-label:bug login");
    }

    #[test]
    fn they_are_listed_under_the_message_with_their_state_branches_and_address() {
        let items = [
            pull(142, "Fix the \"login\"\nform", GitHubState::Open),
            issue(12, ""),
        ];
        assert_eq!(
            with_github(" look at [#142 Fix the login form] ", &items),
            "look at [#142 Fix the login form]\n\n\
             Pull requests and issues mentioned, to read with gh:\n\
             - pull request #142 (\"Fix the \"login\" form\", open, fix-login into main): \
             https://github.com/o/r/pull/142\n\
             - issue #12 (\"untitled\", closed): https://github.com/o/r/issues/12"
        );
        // Alone they are a message; none add nothing.
        assert!(with_github("", &items[1..]).starts_with(MENTIONED_ON_GITHUB));
        assert_eq!(with_github(" words ", &[]), "words");
    }

    #[test]
    fn a_waiting_message_goes_with_what_was_read_last() {
        let known = Known::default();
        let open = pull(142, "Fix the login form", GitHubState::Open);
        known.keep(1, &[open.clone(), issue(12, "Login loops")]);
        let message = format!(
            "{}\n\nAttached files:\n- /tmp/a",
            with_github("see", &[open.clone(), issue(12, "Login loops")])
        );
        assert_eq!(
            addresses_in(&message),
            [
                "https://github.com/o/r/pull/142",
                "https://github.com/o/r/issues/12"
            ]
        );
        assert_eq!(fresh_list(&message, &known), message, "nothing new");
        let merged = GitHubItem {
            state: GitHubState::Merged,
            title: "Fixed the login form".to_string(),
            ..open
        };
        known.renew(&merged);
        let fresh = fresh_list(&message, &known);
        assert!(
            fresh.contains("- pull request #142 (\"Fixed the login form\", merged, "),
            "{fresh}"
        );
        assert!(fresh.contains("- issue #12 (\"Login loops\", closed)"));
        assert!(fresh.ends_with("Attached files:\n- /tmp/a"), "{fresh}");
        // A message with no list is sent as it is.
        assert_eq!(fresh_list("just words", &known), "just words");
        // A line naming anything but a web address is no line of the list: `gh` never sees it.
        let odd = format!("{MENTIONED_ON_GITHUB}\n- issue #1 (\"x\", open): --repo=elsewhere");
        assert_eq!(addresses_in(&odd), Vec::<String>::new());
        assert_eq!(addresses_in("just words"), Vec::<String>::new());
    }

    #[test]
    fn a_reading_is_had_by_repository_and_number_while_it_is_recent() {
        let known = Known::default();
        known.keep(1, &[issue(12, "Login loops")]);
        assert!(known.of(1, 12, STALE_AFTER).is_some());
        assert_eq!(known.of(2, 12, STALE_AFTER), None, "another repository's");
        assert_eq!(known.of(1, 12, Duration::ZERO), None, "too old");
        assert!(known.at("https://github.com/o/r/issues/12").is_some());
    }

    #[test]
    fn a_number_is_named_once() {
        assert_eq!(each_once(&[4, 2, 4, 9, 2]), [4, 2, 9]);
    }
}
