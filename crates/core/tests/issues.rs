//! A repository's GitHub issues, with a stand-in for the user's `gh`: listed with the filters
//! and read whole, an empty list that says why when they cannot be asked for, a task that
//! records the issue it is for, and pull request text that closes it.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::fixture;
use swarmed_core::agent::{FakeBackend, Namer, Naming};
use swarmed_core::{
    CHATS, Core, Error, IssueFilter, IssueState, IssueWho, NewTaskOptions, SessionOptions, hooks,
};

/// A `gh` that answers from files beside it and notes every call in `asked`. A file called
/// `refusal` holds what it says instead, failing.
const GH: &str = r#"#!/bin/sh
here="$(dirname "$0")"
echo "$*" >> "$here/asked"
if [ -f "$here/refusal" ]; then cat "$here/refusal" >&2; exit 1; fi
case "$*" in
  "issue list "*) cat "$here/list.json" ;;
  "issue view "*)
    file="$here/issue-$3.json"
    if [ -f "$file" ]; then cat "$file"; else echo "GraphQL: Could not resolve to an issue or pull request with the number of $3. (repository.issue)" >&2; exit 1; fi ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#;

const LIST: &str = r#"[
    {"number": 12, "title": "Login loops", "state": "OPEN",
     "url": "https://github.com/o/r/issues/12", "updatedAt": "2026-11-20T10:00:00Z",
     "labels": [{"name": "bug"}], "assignees": [{"login": "ada"}], "author": {"login": "bob"}},
    {"number": 14, "title": "Dark mode", "state": "OPEN",
     "url": "https://github.com/o/r/issues/14", "updatedAt": "2026-11-23T10:00:00Z",
     "labels": [], "assignees": [], "author": {"login": "ada"}}
]"#;

const ISSUE_12: &str = r#"{"number": 12, "title": "Login loops", "state": "CLOSED",
    "url": "https://github.com/o/r/issues/12", "updatedAt": "2026-11-24T10:00:00Z",
    "createdAt": "2026-11-01T09:00:00Z", "labels": [{"name": "bug"}],
    "assignees": [{"login": "ada"}], "author": {"login": "bob"},
    "body": "It loops.", "comments": [{"author": {"login": "ada"}, "body": "Same.",
    "createdAt": "2026-11-02T09:00:00Z"}]}"#;

struct Gh {
    tools: PathBuf,
}

impl Gh {
    fn says(&self, file: &str, text: &str) {
        std::fs::write(self.tools.join(file), text).expect("writes");
    }

    fn quiet(&self, file: &str) {
        std::fs::remove_file(self.tools.join(file)).ok();
    }

    /// What it was asked since the last time, one call to a line.
    fn asked(&self) -> Vec<String> {
        let file = self.tools.join("asked");
        let asked = std::fs::read_to_string(&file).unwrap_or_default();
        std::fs::remove_file(&file).ok();
        asked.lines().map(str::to_string).collect()
    }
}

/// Writes every pull request the same way, and shows what it was given, so that a test can
/// see it. It never says which issue the branch closes.
#[derive(Debug)]
struct Writer;

impl Namer for Writer {
    fn name(&self, _prompt: String) -> Naming {
        Box::pin(async { None })
    }

    fn pull_message(&self, branch: String) -> Naming {
        Box::pin(async move {
            Some(format!(
                "feat: add notes\n\nWhy.\n\n<shown>{branch}</shown>"
            ))
        })
    }
}

async fn scene() -> (Gh, Core, common::Fixture) {
    let fx = fixture();
    let tools = fx.paths.data_dir.join("tools");
    std::fs::create_dir_all(&tools).expect("dir");
    common::executable(&tools.join("gh"), GH);
    let gh = Gh { tools };
    gh.says("list.json", LIST);
    gh.says("issue-12.json", ISSUE_12);
    let backend = Arc::new(FakeBackend::default());
    let mut core = Core::with_backend(fx.paths.clone(), backend).expect("opens");
    core.set_tool_dirs(vec![gh.tools.clone()]);
    core.set_namer(Arc::new(Writer));
    core.project_add(&fx.repo, None).await.expect("adds");
    (gh, core, fx)
}

#[tokio::test]
async fn issues_are_listed_with_the_filters_and_read_whole() {
    let (gh, core, _fx) = scene().await;
    let listed = core
        .project_issues("demo", &IssueFilter::default())
        .await
        .expect("lists");
    assert_eq!(listed.unavailable, None);
    let numbers: Vec<u64> = listed.issues.iter().map(|issue| issue.number).collect();
    assert_eq!(numbers, [14, 12], "the latest changed first");
    assert_eq!(listed.issues[1].assignees, ["ada"]);
    let asked = gh.asked();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].starts_with("issue list --json "), "{asked:?}");
    assert!(asked[0].ends_with(" --state open --limit 50"), "{asked:?}");

    let mine = IssueFilter {
        who: IssueWho::Authored,
        state: IssueState::Closed,
        search: "login".to_string(),
        limit: 20,
    };
    core.project_issues("demo", &mine).await.expect("lists");
    let asked = gh.asked();
    assert!(
        asked[0].ends_with(" --state closed --limit 20 --author @me --search=login"),
        "{asked:?}"
    );

    let detail = core.project_issue("demo", 12).await.expect("reads");
    assert_eq!(detail.issue.state, IssueState::Closed);
    assert_eq!(detail.body, "It loops.");
    assert_eq!(detail.comments.len(), 1);
    assert!(gh.asked()[0].starts_with("issue view 12 --json "));
    // One that is not there is refused in `gh`'s words.
    let missing = core.project_issue("demo", 99).await;
    assert!(
        matches!(&missing, Err(Error::Issue(why)) if why.contains("Could not resolve to an issue")),
        "{missing:?}"
    );
}

#[tokio::test]
async fn without_gh_or_github_the_list_is_empty_and_says_why() {
    let (gh, mut core, _fx) = scene().await;
    // Not on GitHub: `gh`'s words, without its hints.
    gh.says(
        "refusal",
        "none of the git remotes configured for this repository point to a known GitHub host\n\
         hint: add a remote first\n",
    );
    let listed = core
        .project_issues("demo", &IssueFilter::default())
        .await
        .expect("no error");
    assert_eq!(listed.issues, Vec::new());
    assert_eq!(
        listed.unavailable.as_deref(),
        Some("none of the git remotes configured for this repository point to a known GitHub host")
    );
    gh.quiet("refusal");

    // No `gh` at all.
    core.set_tool_dirs(vec![]);
    let listed = core
        .project_issues("demo", &IssueFilter::default())
        .await
        .expect("no error");
    assert_eq!(listed.issues, Vec::new());
    let why = listed.unavailable.expect("says why");
    assert!(why.contains("`gh` is not installed"), "{why}");
    let unread = core.project_issue("demo", 12).await;
    assert!(matches!(unread, Err(Error::Issue(_))), "{unread:?}");

    // The chats have no repository, and so no issues.
    core.task_new(CHATS, "hello", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("makes a chat");
    let chats = core.project_issues(CHATS, &IssueFilter::default()).await;
    assert!(matches!(chats, Err(Error::NoRepository)), "{chats:?}");
}

#[tokio::test]
async fn a_task_records_its_issue_and_its_pull_request_text_closes_it() {
    let (gh, core, fx) = scene().await;
    let for_issue = NewTaskOptions {
        issue: Some(12),
        ..NewTaskOptions::default()
    };
    let task = core
        .task_new("demo", "fix-login", for_issue, &hooks::discard)
        .await
        .expect("makes the task");
    assert_eq!(task.issue, Some(12));
    // Kept: read again, and in the tree.
    assert_eq!(
        core.task("demo", "fix-login").expect("there").1.issue,
        Some(12)
    );
    let overview = core.overview().expect("reads");
    assert_eq!(overview[0].tasks[0].task.issue, Some(12));
    // Read when asked: the issue was closed meanwhile.
    gh.asked();
    let issue = core.task_issue("demo", "fix-login").await.expect("reads");
    let issue = issue.expect("has one");
    assert_eq!((issue.number, issue.state), (12, IssueState::Closed));
    assert!(gh.asked()[0].starts_with("issue view 12 --json "));

    // A task for no issue has none, and `gh` is not asked.
    core.task_new("demo", "other", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("makes the task");
    assert_eq!(core.task_issue("demo", "other").await.expect("reads"), None);
    assert_eq!(gh.asked(), Vec::<String>::new());

    // The pull request text, from the commits or written, closes the issue.
    std::fs::write(task.worktree_path.join("notes.txt"), "one\n").expect("writes");
    core.task_commit("demo", "fix-login", "feat: notes\n\nWhy notes.")
        .await
        .expect("commits");
    let draft = core
        .task_pull_draft("demo", "fix-login")
        .await
        .expect("drafts");
    assert_eq!(draft.body, "Why notes.\n\nCloses #12");
    let session = core
        .session_new("demo", "fix-login", &SessionOptions::default())
        .expect("new");
    let written = core
        .task_pull_message("demo", "fix-login", Some(session.id))
        .await
        .expect("written");
    // The writer was told, and the line is there even though it left it out.
    assert!(
        written.body.contains("This branch closes issue #12."),
        "{}",
        written.body
    );
    assert!(written.body.ends_with("\n\nCloses #12"), "{}", written.body);
    drop(fx);
}
