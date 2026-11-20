//! The system's notifications: a banner for a session that needs the user while they look
//! elsewhere. Only macOS is asked; anywhere else nothing is shown.
//!
//! An installed Swarmed, which is a bundle with an identity of its own, shows them as itself
//! through the system's notification centre: the user is asked once whether it may, and
//! clicking a banner opens the session it is about. A development build has no identity, and
//! falls back on the older way of showing them as the Terminal's, which newer systems may
//! not show at all.

use swarmed_core::Notice;

/// What a click on a notification does: handed the session it was about.
pub(crate) type OnOpen = Box<dyn FnOnce(i64) + Send>;

#[cfg(target_os = "macos")]
mod mac {
    use std::time::Duration;

    use mac_usernotifications::{Notification, check_bundle, request_auth};
    use swarmed_core::Notice;

    use super::OnOpen;

    /// The application a development build's notifications are shown as. An app that was not
    /// installed has no identity of its own for the system to show.
    const DEVELOPMENT_SENDER: &str = "com.apple.Terminal";

    /// How long a banner nobody touched stays in the notification centre. It also bounds how
    /// long Swarmed waits to hear of a click, which the system reports for no longer anyway
    /// once its list has been cleared.
    const KEPT: Duration = Duration::from_hours(12);

    /// Whether this is an installed Swarmed, which the notification centre knows by name.
    pub(crate) fn has_identity() -> bool {
        check_bundle().is_ok()
    }

    /// Asks the user, the first time, whether Swarmed may show notifications. Whatever they
    /// say is theirs to change in the system's settings; a refusal only means no banners.
    pub(crate) async fn ask_permission() {
        if has_identity() {
            request_auth().await.ok();
        }
    }

    /// Shows `notice` as Swarmed's own and waits to hear whether it was clicked.
    async fn show_as_swarmed(notice: Notice, on_open: OnOpen) {
        let notification = Notification::new()
            .title(&notice.title)
            .message(&notice.body)
            .default_sound()
            // The notices of one session are kept together in the notification centre.
            .thread_id(format!("session-{}", notice.session_id))
            .timeout(KEPT);
        let Ok(shown) = notification.send().await else {
            return;
        };
        if shown
            .response()
            .await
            .is_ok_and(|said| said.is_default_action())
        {
            on_open(notice.session_id);
        }
    }

    /// Shows `notice` as the Terminal's, since this build is nobody the system knows.
    fn show_as_terminal(notice: &Notice) {
        use mac_notification_sys::{Notification, set_application};

        // Only the first call sets it; every later one reports that it is set already.
        set_application(DEVELOPMENT_SENDER).ok();
        let mut notification = Notification::new();
        let notification = notification.title(&notice.title).message(&notice.body);
        notification.asynchronous(true).send().ok();
    }

    pub(crate) fn show(notice: &Notice, on_open: OnOpen) {
        if has_identity() {
            let showing = show_as_swarmed(notice.clone(), on_open);
            drop(tauri::async_runtime::spawn(showing));
        } else {
            show_as_terminal(notice);
        }
    }
}

/// Asks the user, the first time, whether Swarmed may show notifications.
#[cfg(target_os = "macos")]
pub(crate) async fn ask_permission() {
    mac::ask_permission().await;
}

/// Asks nothing: this system's notifications are not known here.
#[cfg(not(target_os = "macos"))]
pub(crate) async fn ask_permission() {}

/// Shows `notice` and returns at once. `on_open` is called if the user clicks it, which only
/// an installed Swarmed gets to hear of. A notification that cannot be shown is not an error:
/// the app itself shows the same.
#[cfg(target_os = "macos")]
pub(crate) fn show(notice: &Notice, on_open: OnOpen) {
    mac::show(notice, on_open);
}

/// Shows nothing: this system's notifications are not known here.
#[cfg(not(target_os = "macos"))]
pub(crate) fn show(_notice: &Notice, _on_open: OnOpen) {}

/// What a check of the notifications found, in words for a terminal.
#[cfg(target_os = "macos")]
pub(crate) async fn check() -> String {
    use mac_usernotifications::{Notification, get_notification_settings, request_auth};

    if !mac::has_identity() {
        return "not a bundle: run the built Swarmed.app, not a development build".to_string();
    }
    let allowed = request_auth().await;
    let settings = get_notification_settings().await;
    let sent = Notification::new()
        .title("Swarmed can notify you")
        .message("This is what a session that needs you looks like.")
        .default_sound()
        .send()
        .await;
    format!(
        "allowed: {allowed:?}\nsettings: {settings:?}\nsent: {:?}",
        sent.map(|shown| shown.notification_id().to_string())
    )
}

/// Says that there is nothing to check here.
#[cfg(not(target_os = "macos"))]
pub(crate) async fn check() -> String {
    "notifications are only shown on macOS".to_string()
}
