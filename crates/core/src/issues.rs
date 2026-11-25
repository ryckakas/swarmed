//! A repository's GitHub issues, as the user's own `gh` reports them, and the issue a task is
//! for.
//!
//! Swarmed holds no GitHub token: it asks `gh` in the repository and shows what that says.
//! Without `gh`, or in a repository that is not on GitHub, a repository has no issues to list,
//! and the list says why instead of failing, as a task without a pull request is no error
//! (`pulls.rs`). What an issue says is anyone's to write: it is handed on as text. The command
//! line prints it so; the window draws only the HTML GitHub allows, narrowed further, with its
//! source a press away.
//!
//! A task records the number of the issue it was started for. Its pull request text then
//! closes that issue ([`with_closes`]), so merging it closes the issue on GitHub.

use serde::{Deserialize, Serialize};

use crate::agent_setup::find_program;
use crate::error::Error;
use crate::merging::gh_words;
use crate::pulls::{GhFailed, ask_gh};
use crate::{Core, Result};

/// What `gh issue list` is asked for, and `gh issue view` for a task's issue.
const LIST_FIELDS: &str = "number,title,state,labels,assignees,author,updatedAt,url";

/// What `gh issue view` is asked for when an issue is read whole.
const VIEW_FIELDS: &str =
    "number,title,state,labels,assignees,author,updatedAt,url,body,comments,createdAt";

/// How many issues a list holds when none is said.
pub const ISSUES_LISTED: u32 = 50;

/// The most issues a list holds: `gh` asks GitHub for a page at a time.
pub const MAX_ISSUES_LISTED: u32 = 200;

/// Whose issues a list holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueWho {
    /// Everyone's.
    #[default]
    Anyone,
    /// Those assigned to the user.
    Assigned,
    /// Those the user opened.
    Authored,
    /// Those that mention the user.
    Mentioned,
}

/// Whether an issue is open, as GitHub says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueState {
    /// Open.
    #[default]
    Open,
    /// Closed, for whatever reason.
    Closed,
}

impl IssueState {
    fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

/// Which of a repository's issues to list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IssueFilter {
    /// Whose.
    pub who: IssueWho,
    /// Open ones or closed ones.
    pub state: IssueState,
    /// Words to look for, in GitHub's search syntax (`label:bug login`); empty for none.
    pub search: String,
    /// How many at most, from 1 to [`MAX_ISSUES_LISTED`].
    pub limit: u32,
}

impl Default for IssueFilter {
    fn default() -> Self {
        Self {
            who: IssueWho::Anyone,
            state: IssueState::Open,
            search: String::new(),
            limit: ISSUES_LISTED,
        }
    }
}

/// One of a repository's issues, as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// Its number in the repository.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Whether it is open.
    pub state: IssueState,
    /// Where it is on GitHub.
    pub url: String,
    /// The names of its labels.
    pub labels: Vec<String>,
    /// The logins of the people it is assigned to.
    pub assignees: Vec<String>,
    /// The login of whoever opened it; empty for an account that is gone.
    pub author: String,
    /// When it last changed, as GitHub writes it (`2026-11-20T10:00:00Z`).
    pub updated_at: String,
}

/// One comment on an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueComment {
    /// The login of whoever wrote it; empty for an account that is gone.
    pub author: String,
    /// What it says, in Markdown; empty for one GitHub hides.
    pub body: String,
    /// When it was written, as GitHub writes it.
    pub created_at: String,
    /// Why GitHub hides it (`spam`, `off-topic`, `outdated` and the like), if it does. What
    /// it says is then left out, as GitHub's own page folds it away.
    pub hidden: Option<String>,
}

/// An issue read whole: what it says, and its comments, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueDetail {
    /// The issue as a list shows it.
    #[serde(flatten)]
    pub issue: Issue,
    /// When it was opened, as GitHub writes it.
    pub created_at: String,
    /// What it says, in Markdown.
    pub body: String,
    /// Its comments, oldest first.
    pub comments: Vec<IssueComment>,
}

/// A repository's issues as listed, or why none could be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueList {
    /// The issues, the latest changed first.
    pub issues: Vec<Issue>,
    /// Why there are none to show, when they could not be asked for: no `gh`, a repository
    /// that is not on GitHub or has no issues, no sign-in, in `gh`'s words.
    pub unavailable: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawLogin {
    #[serde(default)]
    login: String,
}

#[derive(Debug, Deserialize)]
struct RawLabel {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIssue {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    labels: Vec<RawLabel>,
    #[serde(default)]
    assignees: Vec<RawLogin>,
    #[serde(default)]
    author: Option<RawLogin>,
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawComment {
    #[serde(default)]
    author: Option<RawLogin>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    is_minimized: bool,
    #[serde(default)]
    minimized_reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDetail {
    #[serde(flatten)]
    issue: RawIssue,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    comments: Vec<RawComment>,
}

fn login(raw: Option<RawLogin>) -> String {
    raw.map_or_default(|raw| raw.login)
}

fn issue(raw: RawIssue) -> Issue {
    Issue {
        number: raw.number,
        title: raw.title,
        // GitHub has no state but these two.
        state: if raw.state == "CLOSED" {
            IssueState::Closed
        } else {
            IssueState::Open
        },
        url: raw.url,
        labels: raw.labels.into_iter().map(|label| label.name).collect(),
        assignees: raw.assignees.into_iter().map(|raw| raw.login).collect(),
        author: login(raw.author),
        updated_at: raw.updated_at,
    }
}

fn comment(raw: RawComment) -> IssueComment {
    let hidden = raw.is_minimized.then(|| {
        let reason = raw.minimized_reason.to_lowercase().replace('_', " ");
        if reason.is_empty() {
            "hidden".to_string()
        } else {
            reason
        }
    });
    IssueComment {
        author: login(raw.author),
        body: if hidden.is_some() {
            String::new()
        } else {
            raw.body
        },
        created_at: raw.created_at,
        hidden,
    }
}

/// Reads what `gh issue list --json` printed.
fn parse_list(json: &str) -> Result<Vec<Issue>> {
    let raw: Vec<RawIssue> = serde_json::from_str(json)?;
    Ok(raw.into_iter().map(issue).collect())
}

/// Reads what `gh issue view --json` printed, asked for [`LIST_FIELDS`].
fn parse_one(json: &str) -> Result<Issue> {
    Ok(issue(serde_json::from_str(json)?))
}

/// Reads what `gh issue view --json` printed, asked for [`VIEW_FIELDS`].
fn parse_detail(json: &str) -> Result<IssueDetail> {
    let raw: RawDetail = serde_json::from_str(json)?;
    Ok(IssueDetail {
        issue: issue(raw.issue),
        created_at: raw.created_at,
        body: raw.body,
        comments: raw.comments.into_iter().map(comment).collect(),
    })
}

/// What `gh` is asked to list the issues `filter` picks.
fn list_args(filter: &IssueFilter) -> Vec<String> {
    let limit = filter.limit.clamp(1, MAX_ISSUES_LISTED);
    let mut args: Vec<String> = ["issue", "list", "--json", LIST_FIELDS, "--state"]
        .map(String::from)
        .into();
    args.push(filter.state.name().to_string());
    args.extend(["--limit".to_string(), limit.to_string()]);
    let who = match filter.who {
        IssueWho::Anyone => None,
        IssueWho::Assigned => Some("--assignee"),
        IssueWho::Authored => Some("--author"),
        IssueWho::Mentioned => Some("--mention"),
    };
    if let Some(who) = who {
        args.extend([who.to_string(), "@me".to_string()]);
    }
    let words = filter.search.trim();
    if !words.is_empty() {
        // In one piece, so that words starting with a dash (`-label:bug`) are never taken
        // for an option of `gh`'s.
        args.push(format!("--search={words}"));
    }
    args
}

/// Issues in the order a list shows them: the latest changed first, and of two changed at
/// the same moment the newer.
fn ordered(mut issues: Vec<Issue>) -> Vec<Issue> {
    issues
        .sort_by(|a, b| (b.updated_at.as_str(), b.number).cmp(&(a.updated_at.as_str(), a.number)));
    issues
}

/// Whether `body` closes issue `number` already: a line GitHub reads as closing it, such as
/// `Closes #12`, `fixes #12` or `Resolved #12`.
fn closes(body: &str, number: u64) -> bool {
    const KEYWORDS: [&str; 9] = [
        "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
    ];
    let reference = format!("#{number}");
    let words: Vec<&str> = body.split_whitespace().collect();
    words.windows(2).any(|pair| {
        let keyword = pair[0].trim_end_matches(':').to_lowercase();
        let target = pair[1].trim_end_matches(['.', ',', ';', ')']);
        KEYWORDS.contains(&keyword.as_str()) && target == reference
    })
}

/// A pull request's description that closes `issue`, if the task is for one: as it is if it
/// says so already, else with `Closes #<issue>` on a line of its own at its end. Merging it
/// then closes the issue.
pub(crate) fn with_closes(body: &str, issue: Option<u64>) -> String {
    let Some(number) = issue.filter(|number| !closes(body, *number)) else {
        return body.to_string();
    };
    let body = body.trim_end();
    if body.is_empty() {
        format!("Closes #{number}")
    } else {
        format!("{body}\n\nCloses #{number}")
    }
}

/// What a pull request's writer is told of the issue a task is for, among what it is given
/// of the branch; nothing for a task that is for none.
pub(crate) fn issue_for_message(issue: Option<u64>) -> String {
    issue.map_or_else(String::new, |number| {
        format!("\n\nThis branch closes issue #{number}.")
    })
}

fn refused(why: impl Into<String>) -> Error {
    Error::Issue(why.into())
}

/// Why there is no `gh` to ask.
const NO_GH: &str = "the GitHub CLI `gh` is not installed: https://cli.github.com";

impl Core {
    /// The issues of a repository that `filter` picks, through the user's own `gh`, the latest
    /// changed first.
    ///
    /// Not an error when they cannot be asked for: without `gh`, in a repository that is not
    /// on GitHub or has issues turned off, or with `gh` signed out, the list is empty and
    /// says why, in `gh`'s words. Refused for the chats, which have no repository.
    pub async fn project_issues(&self, project: &str, filter: &IssueFilter) -> Result<IssueList> {
        let project = self.project(project)?;
        let repo = project.repo()?;
        let none = |why: String| IssueList {
            issues: Vec::new(),
            unavailable: Some(why),
        };
        let Some(gh) = find_program("gh", &self.tool_dirs) else {
            return Ok(none(NO_GH.to_string()));
        };
        let args = list_args(filter);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match ask_gh(&gh, repo, &args).await {
            Ok(printed) => Ok(IssueList {
                issues: ordered(parse_list(&printed)?),
                unavailable: None,
            }),
            Err(GhFailed(said)) => Ok(none(gh_words(&said))),
        }
    }

    /// Issue `number` of a repository read whole, with its comments, through the user's own
    /// `gh`. Refused with the reason when it cannot be read: no `gh`, no such issue, or
    /// whatever `gh` says.
    pub async fn project_issue(&self, project: &str, number: u64) -> Result<IssueDetail> {
        let project = self.project(project)?;
        let printed = self.ask_issue(project.repo()?, number, VIEW_FIELDS).await?;
        parse_detail(&printed)
    }

    /// The issue a task was started for, read now: its title, and whether it is still open.
    /// Nothing for a task that is for none; refused with the reason when it cannot be read.
    pub async fn task_issue(&self, project: &str, name: &str) -> Result<Option<Issue>> {
        let (project, task) = self.repo_task(project, name)?;
        let Some(number) = task.issue else {
            return Ok(None);
        };
        let printed = self.ask_issue(project.repo()?, number, LIST_FIELDS).await?;
        Ok(Some(parse_one(&printed)?))
    }

    /// What `gh issue view` printed of issue `number` in `repo`, asked for `fields`.
    async fn ask_issue(&self, repo: &std::path::Path, number: u64, fields: &str) -> Result<String> {
        let gh = find_program("gh", &self.tool_dirs).ok_or_else(|| refused(NO_GH))?;
        let number = number.to_string();
        let args = ["issue", "view", &number, "--json", fields];
        ask_gh(&gh, repo, &args)
            .await
            .map_err(|GhFailed(said)| refused(gh_words(&said)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTED: &str = r#"[
        {"number": 12, "title": "Login loops", "state": "OPEN",
         "url": "https://github.com/o/r/issues/12", "updatedAt": "2026-11-20T10:00:00Z",
         "labels": [{"id": "L1", "name": "bug", "color": "d73a4a", "description": "x"}],
         "assignees": [{"id": "U1", "login": "ada", "name": "Ada"}],
         "author": {"id": "U2", "login": "bob", "name": "Bob", "is_bot": false}},
        {"number": 9, "title": "Old", "state": "CLOSED", "url": "u9",
         "updatedAt": "2026-11-22T08:00:00Z", "labels": [], "assignees": [], "author": null},
        {"number": 15, "title": "Newer, same moment", "state": "OPEN", "url": "u15",
         "updatedAt": "2026-11-20T10:00:00Z"}
    ]"#;

    #[test]
    fn what_gh_lists_is_read_and_put_in_order() {
        let issues = ordered(parse_list(LISTED).expect("parses"));
        let numbers: Vec<u64> = issues.iter().map(|issue| issue.number).collect();
        // The latest changed first; of two changed at once, the newer.
        assert_eq!(numbers, [9, 15, 12]);
        let login_loops = &issues[2];
        assert_eq!(login_loops.state, IssueState::Open);
        assert_eq!(login_loops.labels, ["bug"]);
        assert_eq!(login_loops.assignees, ["ada"]);
        assert_eq!(login_loops.author, "bob");
        // An account that is gone has no login, and a field left out is empty.
        assert_eq!(issues[0].state, IssueState::Closed);
        assert_eq!(issues[0].author, "");
        assert_eq!(issues[1].labels, Vec::<String>::new());
        assert!(parse_list("no issues match your search").is_err());
    }

    #[test]
    fn an_issue_is_read_whole_and_a_hidden_comment_is_left_out() {
        let printed = r#"{"number": 12, "title": "Login loops", "state": "OPEN", "url": "u",
            "updatedAt": "2026-11-20T10:00:00Z", "createdAt": "2026-11-01T09:00:00Z",
            "body": "Steps:\n\n1. Log in\n<script>alert(1)</script>",
            "comments": [
                {"author": {"login": "ada"}, "body": "Same here", "createdAt": "2026-11-02T09:00:00Z",
                 "isMinimized": false, "minimizedReason": ""},
                {"author": {"login": "spammer"}, "body": "Buy now", "createdAt": "2026-11-03T09:00:00Z",
                 "isMinimized": true, "minimizedReason": "OFF_TOPIC"}
            ]}"#;
        let detail = parse_detail(printed).expect("parses");
        assert_eq!(detail.issue.number, 12);
        assert_eq!(detail.created_at, "2026-11-01T09:00:00Z");
        // Handed on as it is written: front ends draw it as text.
        assert!(detail.body.contains("<script>"));
        assert_eq!(detail.comments.len(), 2);
        assert_eq!(detail.comments[0].body, "Same here");
        assert_eq!(detail.comments[0].hidden, None);
        assert_eq!(detail.comments[1].author, "spammer");
        assert_eq!(detail.comments[1].body, "");
        assert_eq!(detail.comments[1].hidden.as_deref(), Some("off topic"));
    }

    #[test]
    fn the_filter_becomes_what_gh_is_asked() {
        let plain = list_args(&IssueFilter::default());
        assert_eq!(
            plain,
            [
                "issue",
                "list",
                "--json",
                LIST_FIELDS,
                "--state",
                "open",
                "--limit",
                "50"
            ]
        );
        let mine = IssueFilter {
            who: IssueWho::Assigned,
            state: IssueState::Closed,
            search: "  -label:wontfix login ".to_string(),
            limit: 1000,
        };
        let asked = list_args(&mine);
        assert_eq!(
            asked[4..],
            [
                "--state",
                "closed",
                "--limit",
                "200",
                "--assignee",
                "@me",
                "--search=-label:wontfix login"
            ]
        );
        let who = |who: IssueWho| {
            let filter = IssueFilter {
                who,
                limit: 0,
                ..IssueFilter::default()
            };
            list_args(&filter)[7..].to_vec()
        };
        assert_eq!(who(IssueWho::Authored), ["1", "--author", "@me"]);
        assert_eq!(who(IssueWho::Mentioned), ["1", "--mention", "@me"]);
    }

    #[test]
    fn a_filter_from_a_front_end_may_leave_things_out() {
        let filter: IssueFilter = serde_json::from_str(r#"{"who": "mentioned"}"#).expect("reads");
        assert_eq!(filter.who, IssueWho::Mentioned);
        assert_eq!(filter.state, IssueState::Open);
        assert_eq!(filter.limit, ISSUES_LISTED);
    }

    #[test]
    fn a_description_closes_the_tasks_issue_once() {
        assert_eq!(with_closes("Why.", Some(12)), "Why.\n\nCloses #12");
        assert_eq!(with_closes("", Some(12)), "Closes #12");
        assert_eq!(with_closes("Why.\n\n", Some(12)), "Why.\n\nCloses #12");
        // Said already, in any of GitHub's words: kept as it is.
        for said in [
            "Why.\n\nCloses #12",
            "fixes #12.",
            "Resolved: #12",
            "This closes #12, at last",
        ] {
            assert_eq!(with_closes(said, Some(12)), said, "{said}");
        }
        // Another issue, or a number that only starts the same, is not this one.
        assert_eq!(
            with_closes("Closes #120", Some(12)),
            "Closes #120\n\nCloses #12"
        );
        assert_eq!(with_closes("See #12", Some(12)), "See #12\n\nCloses #12");
        // A task for no issue leaves the description alone.
        assert_eq!(with_closes("Why.", None), "Why.");
    }
}
