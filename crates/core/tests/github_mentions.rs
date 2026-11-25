//! Pull requests and issues mentioned in a message, with a stand-in for the user's `gh`: what
//! the `#` menu is offered, the list under the message, a number nobody read refused, and a
//! waiting message that goes with each one's state as it is when it is sent.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{core_with_project, fixture, next, turn};
use swarmed_core::agent::{AgentEvent, Auth};
use swarmed_core::{
    Attached, CHATS, Core, Error, FirstSession, GitHubKind, GitHubState, NewTaskOptions, Session,
    SessionOptions, hooks,
};

/// A `gh` that answers from files beside it and notes every call in `asked`: `pr-list.json`,
/// `issue-list.json` and their `-search.json` for words, `pr-<n>.json` and `issue-<n>.json`
/// for one, by number or address. A file `issue-off` holds what it says when asked for
/// issues, failing; `refusal` what it says to everything.
const GH: &str = r#"#!/bin/sh
here="$(dirname "$0")"
echo "$*" >> "$here/asked"
if [ -f "$here/refusal" ]; then cat "$here/refusal" >&2; exit 1; fi
kind="$1"
case "$1 $2" in
  "pr list"|"issue list")
    if [ -f "$here/$kind-off" ]; then cat "$here/$kind-off" >&2; exit 1; fi
    case "$*" in
      *--search=*) cat "$here/$kind-search.json" ;;
      *) cat "$here/$kind-list.json" ;;
    esac ;;
  "pr view"|"issue view")
    number="${3##*/}"
    file="$here/$kind-$number.json"
    if [ -f "$file" ]; then cat "$file"; else
      echo "GraphQL: Could not resolve to a thing with the number of $number." >&2; exit 1; fi ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#;

const PULLS: &str = r#"[
    {"number": 142, "title": "Fix the login form", "state": "OPEN", "isDraft": false,
     "headRefName": "fix-login", "baseRefName": "main",
     "url": "https://github.com/o/r/pull/142", "updatedAt": "2026-11-20T10:00:00Z"},
    {"number": 140, "title": "Dark mode", "state": "MERGED", "isDraft": false,
     "headRefName": "dark", "baseRefName": "main",
     "url": "https://github.com/o/r/pull/140", "updatedAt": "2026-11-18T10:00:00Z"}
]"#;

const ISSUES: &str = r#"[
    {"number": 12, "title": "Login loops", "state": "OPEN",
     "url": "https://github.com/o/r/issues/12", "updatedAt": "2026-11-22T10:00:00Z"}
]"#;

fn pull_142(state: &str) -> String {
    format!(
        r#"{{"number": 142, "title": "Fix the login form", "state": "{state}", "isDraft": false,
            "headRefName": "fix-login", "baseRefName": "main",
            "url": "https://github.com/o/r/pull/142", "updatedAt": "2026-11-25T10:00:00Z"}}"#
    )
}

const LISTED_142: &str = "- pull request #142 (\"Fix the login form\", open, fix-login into main): \
                          https://github.com/o/r/pull/142";
const LISTED_12: &str = "- issue #12 (\"Login loops\", open): https://github.com/o/r/issues/12";

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

/// A core with the stand-in `gh`, Swarmed's tools and a task `one`.
async fn scene(fx: &common::Fixture) -> (Gh, Core) {
    let tools = fx.paths.data_dir.join("tools");
    std::fs::create_dir_all(&tools).expect("dir");
    common::executable(&tools.join("gh"), GH);
    let gh = Gh { tools };
    gh.says("pr-list.json", PULLS);
    gh.says("issue-list.json", ISSUES);
    gh.says("pr-search.json", &format!("[{}]", pull_142("OPEN")));
    gh.says("issue-search.json", "[]");
    gh.says("pr-142.json", &pull_142("OPEN"));
    gh.says("issue-12.json", &ISSUES[1..ISSUES.len() - 1]);
    let mut core = core_with_project(fx).await;
    core.set_tool_dirs(vec![gh.tools.clone()]);
    core.set_tool_server(PathBuf::from("/usr/local/bin/swarmed"));
    core.task_new("demo", "one", NewTaskOptions::default(), &hooks::discard)
        .await
        .expect("test setup");
    (gh, core)
}

fn session(core: &Core, title: &str) -> Session {
    let options = SessionOptions {
        title: Some(title.to_string()),
        ..SessionOptions::default()
    };
    core.session_new("demo", "one", &options).expect("creates")
}

fn naming(numbers: &[u64]) -> Attached {
    Attached {
        github: numbers.to_vec(),
        ..Attached::default()
    }
}

/// Skips ahead to the next event that `wanted` holds for.
async fn until(
    bus: &mut tokio::sync::broadcast::Receiver<swarmed_core::SessionEvent>,
    wanted: fn(&AgentEvent) -> bool,
) {
    while !wanted(&next(bus).await.event) {}
}

fn messages(core: &Core, id: i64) -> Vec<String> {
    let events = core.session_events(id).expect("reads");
    let said = events.into_iter().filter_map(|event| match event {
        AgentEvent::UserMessage { text, .. } => Some(text),
        _ => None,
    });
    said.collect()
}

fn refused(result: &swarmed_core::Result<impl std::fmt::Debug>, number: u64) -> bool {
    matches!(result, Err(Error::CannotMentionOnGitHub(refused, _)) if *refused == number)
}

#[tokio::test]
async fn the_menu_is_offered_pull_requests_and_issues_the_latest_changed_first() {
    let fx = fixture();
    let (gh, core) = scene(&fx).await;
    let listed = core.project_github_items("demo", " ").await.expect("lists");
    assert_eq!(listed.unavailable, None);
    let numbers: Vec<u64> = listed.items.iter().map(|item| item.number).collect();
    assert_eq!(numbers, [12, 142, 140]);
    let (issue, pull) = (&listed.items[0], &listed.items[1]);
    assert_eq!(
        (issue.kind, issue.state),
        (GitHubKind::Issue, GitHubState::Open)
    );
    assert_eq!(issue.head, None);
    assert_eq!(
        (pull.kind, pull.head.as_deref()),
        (GitHubKind::Pull, Some("fix-login"))
    );
    assert_eq!(listed.items[2].state, GitHubState::Merged);
    let mut asked = gh.asked();
    asked.sort();
    assert!(
        asked[0].starts_with("issue list --state all --limit 30 --json "),
        "{asked:?}"
    );
    assert!(
        asked[1].starts_with("pr list --state all --limit 30 --json "),
        "{asked:?}"
    );

    // Words are GitHub's search; a number is that one, a pull request or else an issue.
    let found = core
        .project_github_items("demo", "login")
        .await
        .expect("lists");
    assert_eq!(found.items.len(), 1);
    assert!(
        gh.asked()
            .iter()
            .all(|call| call.ends_with(" --search=login"))
    );
    let one = core
        .project_github_items("demo", "12")
        .await
        .expect("lists");
    assert_eq!(one.items[0].kind, GitHubKind::Issue);
    let asked = gh.asked();
    assert!(asked[0].starts_with("pr view 12 --json "), "{asked:?}");
    assert!(asked[1].starts_with("issue view 12 --json "), "{asked:?}");
    let none = core
        .project_github_items("demo", "99")
        .await
        .expect("lists");
    assert_eq!(none.items, Vec::new());
    assert!(none.unavailable.expect("says why").contains("number of 99"));

    // A repository with its issues turned off offers its pull requests.
    gh.says("issue-off", "the 'o/r' repository has disabled issues");
    let pulls = core.project_github_items("demo", "").await.expect("lists");
    assert_eq!(pulls.items.len(), 2);
    assert_eq!(pulls.unavailable, None);
    // Without a word from `gh`, the menu says why instead of failing.
    gh.says(
        "refusal",
        "X not logged into any GitHub hosts. Run gh auth login",
    );
    let signed_out = core.project_github_items("demo", "").await.expect("lists");
    assert_eq!(signed_out.items, Vec::new());
    assert_eq!(
        signed_out.unavailable.as_deref(),
        Some("not logged into any GitHub hosts. Run gh auth login")
    );
}

#[tokio::test]
async fn without_gh_or_a_repository_there_is_nothing_to_offer() {
    let fx = fixture();
    let (_gh, mut core) = scene(&fx).await;
    core.set_tool_dirs(vec![]);
    let listed = core.project_github_items("demo", "").await.expect("lists");
    assert!(
        listed
            .unavailable
            .expect("says why")
            .contains("`gh` is not installed")
    );

    let free = NewTaskOptions {
        free_name: true,
        ..NewTaskOptions::default()
    };
    core.task_new(CHATS, "question", free, &hooks::discard)
        .await
        .expect("makes a chat");
    let chat = core
        .session_new(CHATS, "question", &SessionOptions::default())
        .expect("creates");
    assert!(matches!(
        core.project_github_items(CHATS, "").await,
        Err(Error::NoRepository)
    ));
    let sent = core.session_message(chat.id, "read it", &naming(&[142]), Auth::Subscription);
    assert!(matches!(sent, Err(Error::NoRepository)), "{sent:?}");
    assert_eq!(messages(&core, chat.id), Vec::<String>::new());
}

#[tokio::test]
async fn a_mention_is_listed_under_the_message_from_what_the_menu_read() {
    let fx = fixture();
    let (gh, core) = scene(&fx).await;
    let here = session(&core, "Here");
    let found = session(&core, "Found it");
    core.project_github_items("demo", "").await.expect("lists");
    gh.asked();

    let readme = fx.repo.join("README.md");
    let attached = Attached {
        files: vec![readme.clone()],
        sessions: vec![found.id],
        github: vec![142, 12, 142],
        elements: vec![],
    };
    let mut bus = core.subscribe();
    let typed = "fix what [#142 Fix the login form] left, as [#12 Login loops] says";
    core.session_message(here.id, typed, &attached, Auth::Subscription)
        .expect("sends");
    turn(&mut bus, here.id).await;
    // Words, the sessions, the pull requests and issues, then the files: each once.
    let expected = format!(
        "{typed}\n\nSessions mentioned, to read with the read_session tool:\n\
         - session {} (\"Found it\")\n\n\
         Pull requests and issues mentioned, to read with gh:\n{LISTED_142}\n{LISTED_12}\n\n\
         Attached files:\n- {}",
        found.id,
        readme.display()
    );
    assert_eq!(messages(&core, here.id), [expected]);
    assert_eq!(
        gh.asked(),
        Vec::<String>::new(),
        "sending asked GitHub nothing"
    );
}

#[tokio::test]
async fn a_number_nobody_read_is_refused_until_it_is_read() {
    let fx = fixture();
    let (gh, core) = scene(&fx).await;
    let here = session(&core, "Here");
    let sent = core.session_message(here.id, "see", &naming(&[142]), Auth::Subscription);
    assert!(refused(&sent, 142), "{sent:?}");
    assert_eq!(messages(&core, here.id), Vec::<String>::new());
    assert_eq!(gh.asked(), Vec::<String>::new());

    // Read first, as a front end does, it goes.
    let read = core
        .session_github_read(here.id, &[142, 142])
        .await
        .expect("reads");
    assert_eq!(read.len(), 1);
    assert_eq!(gh.asked().len(), 1);
    // What was read a moment ago is not asked for again.
    core.project_github_read("demo", &[142])
        .await
        .expect("reads");
    assert_eq!(gh.asked(), Vec::<String>::new());
    let mut bus = core.subscribe();
    core.session_message(here.id, "see", &naming(&[142]), Auth::Subscription)
        .expect("sends");
    turn(&mut bus, here.id).await;
    assert!(messages(&core, here.id)[0].ends_with(LISTED_142));

    // One that is not there is refused in `gh`'s words.
    let missing = core.project_github_read("demo", &[99]).await;
    assert!(refused(&missing, 99), "{missing:?}");
    let said = missing.expect_err("refused").to_string();
    assert!(said.starts_with("#99 cannot be mentioned: "), "{said}");
}

#[tokio::test]
async fn a_waiting_message_goes_with_the_state_as_it_is_when_sent() {
    let fx = fixture();
    let (gh, core) = scene(&fx).await;
    let here = session(&core, "Here");
    core.project_github_items("demo", "").await.expect("lists");
    let mut bus = core.subscribe();
    core.session_message(here.id, "slow", &Attached::default(), Auth::Subscription)
        .expect("sends");
    until(&mut bus, |event| matches!(event, AgentEvent::Text { .. })).await;
    let queue = core
        .session_enqueue_message(here.id, "merge [#142 Fix the login form]", &naming(&[142]))
        .expect("queues");
    assert!(
        queue[0].text.ends_with(LISTED_142),
        "as it was typed: {queue:?}"
    );

    // Merged while it waits: read again, as the follower does while one waits.
    gh.asked();
    gh.says("pr-142.json", &pull_142("MERGED"));
    core.github_waiting_read_again().await;
    let asked = gh.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0].starts_with("pr view https://github.com/o/r/pull/142 --json "));
    gh.quiet("pr-142.json");

    core.session_interrupt(here.id).expect("stops");
    turn(&mut bus, here.id).await;
    turn(&mut bus, here.id).await;
    let sent = messages(&core, here.id);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(
        sent[1].ends_with(
            "- pull request #142 (\"Fix the login form\", merged, fix-login into main): \
             https://github.com/o/r/pull/142"
        ),
        "{}",
        sent[1]
    );
    // Nothing waits: there is nothing to read again.
    core.github_waiting_read_again().await;
    assert_eq!(gh.asked(), Vec::<String>::new());
}

#[tokio::test]
async fn a_new_task_mentions_the_pull_requests_of_its_repository() {
    let fx = fixture();
    let (_gh, core) = scene(&fx).await;
    let core = Arc::new(core);
    let first = |github: Vec<u64>| FirstSession {
        prompt: "review [#142 Fix the login form]".to_string(),
        github,
        ..FirstSession::default()
    };
    // One nobody read is refused before anything is made.
    let start = core.task_start(
        "demo",
        "refused",
        NewTaskOptions::default(),
        first(vec![142]),
        Auth::Subscription,
        &hooks::discard,
    );
    assert!(refused(&start.await, 142));
    assert!(core.task("demo", "refused").is_err());

    core.project_github_read("demo", &[142])
        .await
        .expect("reads");
    let mut bus = core.subscribe();
    let (started, going) = core
        .task_start(
            "demo",
            "review",
            NewTaskOptions::default(),
            first(vec![142]),
            Auth::Subscription,
            &hooks::discard,
        )
        .await
        .expect("starts");
    going.finished().await.expect("set up");
    turn(&mut bus, started.session.id).await;
    let expected = format!(
        "review [#142 Fix the login form]\n\n\
         Pull requests and issues mentioned, to read with gh:\n{LISTED_142}"
    );
    assert_eq!(messages(&core, started.session.id), [expected]);
    // Named by the title's words, not by the token.
    let session = core.session(started.session.id).expect("reads");
    assert_eq!(session.title, "review Fix the login form");
}
