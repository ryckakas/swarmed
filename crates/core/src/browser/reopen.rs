//! The tabs a task's Chrome had when Swarmed stopped it, opened again when it starts, as a
//! browser's own come back: its profile already keeps what was signed in to. Only web
//! addresses, checked again as they are read, and opened over the DevTools connection, never
//! on Chrome's command line, where `ps` would show them and one could be read as a switch.
//! Kept beside the profile in the task's browser folder, readable by the user alone, deleted
//! with it, and never logged.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use serde_json::json;

use super::{Browser, BrowserTab};
use crate::lock;

/// How long the blank tab Chrome started on is waited for to go, once closed.
const CLOSING: Duration = Duration::from_secs(2);

/// The file the addresses are kept in, in the task's browser folder.
pub(super) const KEPT: &str = "tabs.json";

/// The most tabs opened again.
const MOST: usize = 20;

/// The addresses worth opening again of `urls`: web pages, each once, in order, at most
/// [`MOST`].
fn worth_opening<'a>(urls: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for url in urls {
        let web = url.starts_with("http://") || url.starts_with("https://");
        if web && !kept.iter().any(|known| known == url) {
            kept.push(url.to_string());
        }
        if kept.len() == MOST {
            break;
        }
    }
    kept
}

/// Keeps the addresses of `tabs` in `path`, readable by the user alone. Nothing worth opening
/// again keeps that too, so that a Chrome stopped with no page open starts with none.
pub(super) fn keep(path: &Path, tabs: &[BrowserTab]) {
    let urls = worth_opening(tabs.iter().map(|tab| tab.url.as_str()));
    let Ok(text) = serde_json::to_string(&urls) else {
        return;
    };
    if std::fs::write(path, text).is_ok() {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).ok();
    }
}

/// The addresses kept in `path`, checked again: none when there is no such file, or it holds
/// what Swarmed did not write.
pub(super) fn kept(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let urls: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    worth_opening(urls.iter().map(String::as_str))
}

/// Opens the tabs kept in `path` in a Chrome just started, then closes the blank one it started
/// on, once any of them opened: the pane then shows the first of them. A page Chrome started
/// on, such as the task's stack, is not opened twice.
pub(super) async fn reopen(browser: &Browser, path: &Path) {
    let urls = kept(path);
    if urls.is_empty() {
        return;
    }
    let started = lock(&browser.pane).tabs();
    let mut opened = false;
    for url in urls
        .iter()
        .filter(|url| !started.iter().any(|tab| tab.url == **url))
    {
        opened |= super::agents::open(browser, url).await.is_ok();
    }
    if !opened {
        return;
    }
    for blank in started.iter().filter(|tab| tab.url == "about:blank") {
        let close = json!({ "targetId": blank.id });
        browser
            .cdp
            .call(None, "Target.closeTarget", close)
            .await
            .ok();
        let until = tokio::time::Instant::now() + CLOSING;
        while lock(&browser.pane).has(&blank.id) && tokio::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_pages_are_opened_again_each_once_and_not_too_many() {
        let urls = [
            "http://localhost:3000/cart",
            "about:blank",
            "chrome://settings",
            "data:text/html,<p>hi</p>",
            "--load-extension=/x",
            "https://example.com/",
            "http://localhost:3000/cart",
        ];
        assert_eq!(
            worth_opening(urls),
            ["http://localhost:3000/cart", "https://example.com/"]
        );
        let many: Vec<String> = (0..30)
            .map(|n| format!("https://example.com/{n}"))
            .collect();
        assert_eq!(worth_opening(many.iter().map(String::as_str)).len(), MOST);
    }

    #[test]
    fn what_was_kept_is_read_back_and_anything_else_is_nothing() {
        let folder = tempfile::tempdir().expect("a folder");
        let path = folder.path().join(KEPT);
        assert_eq!(kept(&path), Vec::<String>::new(), "nothing kept yet");
        std::fs::write(&path, r#"["https://example.com/", "file:///etc/passwd"]"#).expect("writes");
        assert_eq!(kept(&path), ["https://example.com/"]);
        std::fs::write(&path, "not JSON").expect("writes");
        assert_eq!(kept(&path), Vec::<String>::new());
    }
}
