//! Keeping the window on Swarmed's own pages.
//!
//! A link in a conversation is the agent's text. Followed inside the window it would replace
//! the app with a web page that has no way back, so anything that is not Swarmed's own page
//! is refused here.
//!
//! Refused is all it is: nothing is opened in the browser from here. This is asked about every
//! frame, an agent's view included, and a view's script can send its frame anywhere without a
//! click, with whatever it likes in the address. A link the user presses is opened by the UI,
//! through the `open_link` command.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime, Url};

use crate::VIEW_SCHEME;

/// Schemes the window's own pages and frames use.
const OWN_SCHEMES: &[&str] = &["tauri", "about", "blob", "data", VIEW_SCHEME];

/// Whether `url` is one of Swarmed's own pages: the bundled app, a view an agent wrote, or,
/// while developing, the dev server at `dev`.
fn is_own(url: &Url, dev: Option<&Url>) -> bool {
    OWN_SCHEMES.contains(&url.scheme()) || dev.is_some_and(|dev| dev.origin() == url.origin())
}

/// Refuses to leave Swarmed's pages, and opens nothing in their place.
pub(crate) fn guard<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("navigation")
        .on_navigation(|webview, url| {
            let dev = webview.config().build.dev_url.as_ref();
            is_own(url, dev.filter(|_| tauri::is_dev()))
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).expect("a URL")
    }

    #[test]
    fn the_app_and_its_views_stay_in_the_window() {
        assert!(is_own(&url("tauri://localhost/index.html"), None));
        assert!(is_own(&url("swarmed-view://localhost/3"), None));
        assert!(is_own(&url("about:blank"), None));
    }

    #[test]
    fn the_dev_server_is_ours_only_while_developing() {
        let dev = url("http://localhost:5173/");
        assert!(is_own(&url("http://localhost:5173/x"), Some(&dev)));
        assert!(!is_own(&url("http://localhost:5173/x"), None));
        // Another port on this machine is somebody's dev server, not Swarmed.
        assert!(!is_own(&url("http://localhost:3000/"), Some(&dev)));
    }

    #[test]
    fn the_web_is_never_shown_in_the_window() {
        assert!(!is_own(&url("https://example.com/"), None));
        assert!(!is_own(&url("file:///etc/passwd"), None));
    }
}
