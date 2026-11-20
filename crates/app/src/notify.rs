//! The system's notifications: a banner for a session that needs the user while they look
//! elsewhere. Only macOS is asked; anywhere else nothing is shown.

use swarmed_core::Notice;

/// The application a development build's notifications are shown as. An app that was not
/// installed has no identity of its own for the system to show.
#[cfg(target_os = "macos")]
const DEVELOPMENT_SENDER: &str = "com.apple.Terminal";

/// Shows `notice` as coming from the application `identifier` names, and returns at once.
/// A notification that cannot be shown is not an error: the app itself shows the same.
#[cfg(target_os = "macos")]
pub(crate) fn show(identifier: &str, notice: &Notice) {
    use mac_notification_sys::{Notification, set_application};

    let sender = if tauri::is_dev() {
        DEVELOPMENT_SENDER
    } else {
        identifier
    };
    // Only the first call sets it; every later one reports that it is set already.
    set_application(sender).ok();
    let mut notification = Notification::new();
    let notification = notification.title(&notice.title).message(&notice.body);
    notification.asynchronous(true).send().ok();
}

/// Shows nothing: this system's notifications are not known here.
#[cfg(not(target_os = "macos"))]
pub(crate) fn show(_identifier: &str, _notice: &Notice) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run by hand (`cargo test -p swarmed-app -- --ignored`): a banner must appear.
    #[test]
    #[ignore = "shows a real notification on this machine"]
    fn a_notice_reaches_the_system() {
        let notice = Notice {
            session_id: 1,
            title: "Needs you: a test from Swarmed".to_string(),
            body: "demo / one\nWants to use Bash".to_string(),
        };
        show("dev.swarmed.test", &notice);
        // The banner is handed over on another thread; give it a moment before the test ends.
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
