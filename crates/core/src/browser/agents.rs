//! What the agents have of the task's browser (#384): an address of their session's own, their
//! own browser servers pointed at it, a word in Swarmed's guidance, and `show_browser`.
//!
//! No browser tool of Swarmed's own: agents already know Playwright MCP and Chrome DevTools
//! MCP, and only need them to act on the browser the user sees. Nothing is added that the user
//! did not set up. Claude Code is handed a pointed copy of each of its own browser servers in
//! the file it takes servers from, under the same name, which takes the place of its own
//! (tried with Claude Code 2.1.332, plugins' `plugin:<plugin>:<server>` included). Any other
//! agent has the servers Swarmed hands it pointed; its own configuration is not touched.

use std::sync::{Arc, Weak};
use std::time::Duration;

use serde_json::{Value, json};

use super::address::{self, endpoint};
use super::servers::pointed;
use super::{Browser, BrowserEvent, BrowserState, BrowserTab, input};
use crate::agents::CLAUDE;
use crate::error::Result;
use crate::mcp::McpServer;
use crate::sessions::Session;
use crate::store::Task;
use crate::{Core, lock};

/// The variable an agent and its processes find the session's address in.
pub(crate) const BROWSER_ENV: &str = "SWARMED_BROWSER";

/// How long `show_browser` waits for a tab it opened to be told of.
const TAB_WAIT: Duration = Duration::from_secs(3);

/// What a session's agent is given of the task's browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentBrowser {
    /// The session's address, as `http://127.0.0.1:<port>`.
    pub(crate) endpoint: String,
    /// The servers pointed at it, by the names the agent knows them by.
    pub(crate) pointed: Vec<String>,
}

/// What every agent is told of the task's browser, in Swarmed's guidance.
pub(crate) fn instructions(browser: &AgentBrowser) -> String {
    let tools = if browser.pointed.is_empty() {
        "A browser tool or script of yours connects to it by that address.".to_string()
    } else {
        let names: Vec<String> = browser
            .pointed
            .iter()
            .map(|name| format!("`{name}`"))
            .collect();
        format!(
            "Your browser tools on the {} server{} already act on it.",
            names.join(" and "),
            if names.len() == 1 { "" } else { "s" }
        )
    };
    format!(
        "# The task's browser

This task has a browser of its own: a Chrome the user sees live beside this conversation, on a \
profile of the task's own, so what the user signed in to there stays signed in. To look at or \
test a web page, use it; never launch a browser or open a window of your own. Its address is in \
`{BROWSER_ENV}`. {tools} From a script, connect with \
`chromium.connectOverCDP(process.env.{BROWSER_ENV})` (Playwright) or `puppeteer.connect({{ \
browserURL: process.env.{BROWSER_ENV} }})`. Open a tab of your own for your work rather than \
acting in one that is already open, which the user or another session may be using. \
`show_browser` opens the browser for the user, on an address if you give one."
    )
}

impl Core {
    /// Lets sessions' tools start their task's browser: from now on each session that starts
    /// gets an address of its own. Front ends call this when they serve agents' tools.
    pub(crate) fn browsers_listen(self: &Arc<Self>) {
        self.browsers.home.set(Arc::downgrade(self)).ok();
    }

    /// Session `session`'s address of task `task`'s Chrome, listened on from now on; `None`
    /// where nothing serves agents' tools, or no port could be had.
    pub(crate) fn browser_address(&self, session: i64, task: i64) -> Option<String> {
        let home = Weak::clone(self.browsers.home.get()?);
        let mut addresses = lock(&self.browsers.addresses);
        if let Some(known) = addresses.get(&session).filter(|known| known.task == task) {
            return Some(endpoint(known.port));
        }
        let listening = address::listen(home, task, session).ok()?;
        let port = listening.port;
        addresses.insert(session, listening);
        Some(endpoint(port))
    }

    /// Stops listening at session `session`'s address, for a session that goes.
    pub(crate) fn browser_address_forget(&self, session: i64) {
        lock(&self.browsers.addresses).remove(&session);
    }

    /// What the agent of `session`, in `task`, is given of the task's browser: its address,
    /// and the browser servers among `servers` pointed at it. For Claude Code, its own browser
    /// servers are added to `servers`, pointed, to take the place of its own.
    pub(crate) fn browser_for_agent(
        &self,
        session: &Session,
        task: &Task,
        servers: &mut Vec<McpServer>,
    ) -> Option<AgentBrowser> {
        let endpoint = self.browser_address(session.id, task.id)?;
        if session.agent == CLAUDE {
            let own = self.claude_code_own_servers(task);
            let taken = |server: &McpServer| servers.iter().any(|known| known.name == server.name);
            let own: Vec<McpServer> = own
                .into_iter()
                .filter(|server| pointed(server, &endpoint).is_some() && !taken(server))
                .collect();
            servers.extend(own);
        }
        let names = point_all(servers, &endpoint);
        Some(AgentBrowser {
            endpoint,
            pointed: names,
        })
    }

    /// Claude Code's own servers for a session in `task`, by the names it gives them: those it
    /// starts without asking (the user's, approved ones of the repository's) and its plugins'.
    fn claude_code_own_servers(&self, task: &Task) -> Vec<McpServer> {
        let repo = self
            .store()
            .project_by_id(task.project_id)
            .ok()
            .flatten()
            .and_then(|project| project.repo().ok().map(std::path::Path::to_path_buf));
        let repo = repo.unwrap_or_else(|| task.worktree_path.clone());
        self.paths
            .claude_code_own_servers(&repo, &task.worktree_path)
    }

    /// `show_browser`, called by session `from`'s agent: opens the task's browser for the user,
    /// on `url` or the task's stack, in a tab of the session's own, and says how to act on it.
    pub(crate) async fn browser_show_for(&self, from: i64, arguments: &Value) -> Result<String> {
        let session = self.session(from)?;
        let task = self.task_of(&session)?;
        let browser = self.browser_of(task.id).await?;
        browser.touch();
        let asked = arguments.get("url").and_then(Value::as_str);
        let asked = asked.filter(|url| !url.trim().is_empty());
        let wanted = asked
            .map(input::address_of)
            .or_else(|| self.start_page(&task));
        let tab = self.tab_for(&browser, from, wanted.as_deref()).await;
        let tab = tab.map_err(|reason| self.browser_error(task.id, reason))?;
        if let Some(tab) = &tab {
            browser.acted(&self.browsers, tab, from);
        }
        self.browsers.publish(BrowserEvent::Show {
            task_id: task.id,
            session_id: from,
            tab: tab.clone(),
        });
        let address = self.browser_address(from, task.id);
        let address = address.unwrap_or_else(|| endpoint(browser.port));
        Ok(told(&address, &browser.state(), from, tab.as_deref()))
    }

    /// The tab of session `from`'s own to show, on `wanted` if given: the one it last acted
    /// in, else one nobody used that is empty or there already, else a new one. Never a tab
    /// another session acts in, nor one of the user's that is somewhere: the session is told
    /// the tab is its own and to act in it.
    async fn tab_for(
        &self,
        browser: &Arc<Browser>,
        from: i64,
        wanted: Option<&str>,
    ) -> std::result::Result<Option<String>, String> {
        let reused = {
            let pane = lock(&browser.pane);
            let fits = |url: &str| url == "about:blank" || Some(url) == wanted;
            let free = |tab: &BrowserTab| tab.acted_by.is_none() && fits(&tab.url);
            let own = pane.tabs_of(from).pop();
            own.or_else(|| pane.tabs().into_iter().find(free))
        };
        let Some(tab) = reused else {
            return open(browser, wanted.unwrap_or("about:blank"))
                .await
                .map(Some);
        };
        if let Some(wanted) = wanted.filter(|wanted| tab.url != *wanted) {
            let session = attached(browser, &tab.id)
                .await
                .ok_or("its tab is not ready yet")?;
            super::navigate(&browser.cdp, &session, wanted).await?;
        }
        Ok(Some(tab.id))
    }
}

/// Points every browser server of `servers` at `endpoint`, and names them.
fn point_all(servers: &mut [McpServer], endpoint: &str) -> Vec<String> {
    let mut names = Vec::new();
    for server in servers {
        let Some(pointed) = pointed(server, endpoint) else {
            continue;
        };
        names.push(pointed.name.clone());
        *server = pointed;
    }
    names
}

/// The session Swarmed speaks to `tab` on, waiting a moment for a tab just told of.
async fn attached(browser: &Browser, tab: &str) -> Option<String> {
    let until = tokio::time::Instant::now() + TAB_WAIT;
    loop {
        let session = browser.session_of(tab);
        if session.is_some() || tokio::time::Instant::now() >= until {
            return session;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Opens a tab on `url`, and waits until it is told of, so that it can be shown at once.
pub(super) async fn open(browser: &Browser, url: &str) -> std::result::Result<String, String> {
    let created = browser
        .cdp
        .call(None, "Target.createTarget", json!({ "url": url }))
        .await?;
    let tab = created["targetId"].as_str().unwrap_or_default().to_string();
    let until = tokio::time::Instant::now() + TAB_WAIT;
    while !lock(&browser.pane).has(&tab) && tokio::time::Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(tab)
}

/// What `show_browser` answers: where the browser is, its tabs, which is the session's, and
/// how to act on it. No page's title: what a page says is for the agent's own tools to read.
fn told(address: &str, state: &BrowserState, from: i64, tab: Option<&str>) -> String {
    let shown = tab.and_then(|tab| state.tabs.iter().find(|known| known.id == tab));
    // A Chrome with a window of its own draws its pages there, not in the pane, and is never
    // brought forward for a session.
    let (opening, seen) = if state.windowed {
        (
            "The task's browser runs in a window of its own",
            " The user sees it when they bring that window forward; it is not brought forward \
             for you.",
        )
    } else {
        (
            "The task's browser is open beside the conversation, where the user sees it",
            "",
        )
    };
    let opening = match shown {
        Some(tab) => format!("{opening}, on {} (tab {}).{seen}", tab.url, tab.id),
        None => format!("{opening}.{seen}"),
    };
    let tabs: Vec<String> = state
        .tabs
        .iter()
        .map(|known| {
            let whose = match known.acted_by {
                Some(session) if session == from => " (yours)".to_string(),
                Some(session) => format!(" (session {session} acts in it)"),
                None => String::new(),
            };
            format!("- {}: {}{whose}", known.id, known.url)
        })
        .collect();
    format!(
        "{opening}\n\nIts address: {address}, also in ${BROWSER_ENV}.\n\nTabs:\n{}\n\nAct in your \
         own tab: browser tools connected to this address list it among their tabs (Playwright \
         MCP's browser_tabs, Chrome DevTools MCP's list_pages), by where it is; select it \
         before you act. A browser tool that lists other tabs is not connected to this browser \
         and acts on one of its own, which the user does not see: use a script instead, \
         connected with chromium.connectOverCDP(process.env.{BROWSER_ENV}). Clicking, typing, \
         reading the page and screenshots are for those tools or that script.",
        if tabs.is_empty() {
            "- none".to_string()
        } else {
            tabs.join("\n")
        }
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn tab(id: &str, url: &str, acted_by: Option<i64>) -> BrowserTab {
        BrowserTab {
            id: id.into(),
            title: "a title a page chose".into(),
            url: url.into(),
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            dialog: None,
            acted_by,
        }
    }

    #[test]
    fn the_answer_says_which_tab_is_whose_and_never_what_a_page_calls_itself() {
        let state = BrowserState {
            task_id: 1,
            running: true,
            name: Some("Google Chrome".into()),
            endpoint: Some("http://127.0.0.1:9".into()),
            websocket: None,
            tabs: vec![
                tab("A", "http://localhost:3000/", Some(7)),
                tab("B", "http://localhost:3000/cart", Some(8)),
                tab("C", "about:blank", None),
            ],
            shown: Some("A".into()),
            downloads: PathBuf::from("/tmp"),
            windowed: false,
        };
        let said = told("http://127.0.0.1:50123", &state, 7, Some("A"));
        assert!(said.contains("on http://localhost:3000/ (tab A)"), "{said}");
        assert!(
            said.contains("- A: http://localhost:3000/ (yours)"),
            "{said}"
        );
        assert!(said.contains("- B: http://localhost:3000/cart (session 8 acts in it)"));
        assert!(said.contains("- C: about:blank\n"), "{said}");
        assert!(said.contains("http://127.0.0.1:50123, also in $SWARMED_BROWSER"));
        assert!(!said.contains("a title a page chose"), "{said}");
        assert!(said.contains("beside the conversation"), "{said}");

        // A Chrome with a window of its own is not where the conversation is.
        let windowed = BrowserState {
            windowed: true,
            ..state
        };
        let said = told("http://127.0.0.1:50123", &windowed, 7, Some("A"));
        assert!(
            said.starts_with(
                "The task's browser runs in a window of its own, on http://localhost:3000/ (tab \
                 A). The user sees it when they bring that window forward"
            ),
            "{said}"
        );
        assert!(!said.contains("beside the conversation"), "{said}");
        let said = told("http://127.0.0.1:50123", &windowed, 7, None);
        assert!(
            said.starts_with("The task's browser runs in a window of its own. The user sees it"),
            "{said}"
        );
    }

    #[test]
    fn the_guidance_names_the_servers_that_act_on_the_browser_already() {
        let mut browser = AgentBrowser {
            endpoint: "http://127.0.0.1:1".into(),
            pointed: vec!["playwright".into(), "plugin:devtools:chrome".into()],
        };
        let said = instructions(&browser);
        assert!(said.contains("on the `playwright` and `plugin:devtools:chrome` servers already"));
        assert!(said.contains("never launch a browser"), "{said}");
        browser.pointed.clear();
        let said = instructions(&browser);
        assert!(said.contains("connects to it by that address"), "{said}");
    }
}
