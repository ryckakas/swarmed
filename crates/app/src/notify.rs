//! The system's notifications: a banner for a session that needs the user while they look
//! elsewhere. Only macOS is asked; anywhere else nothing is shown.
//!
//! An installed Swarmed, which is a bundle with an identity of its own, shows them as itself
//! through the system's notification centre: the user is asked once whether it may, and
//! clicking a banner opens the session it is about. A development build has no identity, and
//! falls back on the older way of showing them as the Terminal's, which newer systems may
//! not show at all.
//!
//! To see banners, try them in the built app: `pnpm app:build` makes the bundle, signed ad
//! hoc, which is enough; `pnpm app` does not. `open Swarmed.app --args notify-check` says
//! what the system allows and sends one banner.

use swarmed_core::Notice;

/// What a click on a notification does: handed the session it was about.
pub(crate) type OnOpen = Box<dyn FnOnce(i64) + Send>;

#[cfg(target_os = "macos")]
mod mac {
    use std::path::Path;
    use std::time::Duration;

    use block2::RcBlock;
    use mac_usernotifications::{Notification, check_bundle};
    use objc2::runtime::Bool;
    use objc2_foundation::NSError;
    use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};
    use swarmed_core::Notice;

    use super::OnOpen;

    /// The application a development build's notifications are shown as. An app that was not
    /// installed has no identity of its own for the system to show.
    const DEVELOPMENT_SENDER: &str = "com.apple.Terminal";

    /// How long a banner nobody touched stays in the notification centre. It also bounds how
    /// long Swarmed waits to hear of a click, which the system reports for no longer anyway
    /// once its list has been cleared.
    const KEPT: Duration = Duration::from_hours(12);

    /// Whether `executable` is the program of an application bundle: `Name.app/Contents/MacOS/`
    /// is where it lies.
    pub(super) fn lies_in_bundle(executable: &Path) -> bool {
        let named = |folder: Option<&Path>, name: &str| {
            folder
                .and_then(Path::file_name)
                .is_some_and(|is| is == name)
        };
        let programs = executable.parent();
        let contents = programs.and_then(Path::parent);
        let bundle = contents.and_then(Path::parent);
        let is_app = bundle
            .and_then(Path::extension)
            .is_some_and(|extension| extension == "app");
        named(programs, "MacOS") && named(contents, "Contents") && is_app
    }

    /// Whether this is an installed Swarmed, which the notification centre knows by name.
    ///
    /// Having a bundle identifier is not enough, because a development build gets one the
    /// moment it shows its first notice: showing it as the Terminal's works by making the
    /// program answer to the Terminal's identifier from then on. Asked again, it looked like
    /// a bundle, the next notice went to the notification centre, and that ends the whole
    /// process with an exception that cannot be caught here. So the program also has to lie
    /// where a bundle keeps its own, which nothing changes while it runs.
    pub(crate) fn has_identity() -> bool {
        let bundled = std::env::current_exe().is_ok_and(|executable| lies_in_bundle(&executable));
        bundled && check_bundle().is_ok()
    }

    /// Asks the user, the first time, whether Swarmed may show notifications. Whatever they
    /// say is theirs to change in the system's settings; a refusal only means no banners.
    ///
    /// The count on the icon is asked for along with banners and sounds: an app the
    /// notification centre knows is only allowed a badge if it asked for one.
    pub(crate) fn ask_permission() {
        if !has_identity() {
            return;
        }
        let wanted = UNAuthorizationOptions::Alert
            | UNAuthorizationOptions::Sound
            | UNAuthorizationOptions::Badge;
        // The answer is the system's to keep: every notice finds out for itself.
        let heard = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(wanted, &heard);
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
        let sent = notification.send().await;
        super::trace(&format!("{}: sent={}", notice.title, sent.is_ok()));
        let Ok(shown) = sent else {
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

/// The variable that names a file to note in what became of each notice. For finding out why
/// one did not appear: nobody can look into the notification centre from outside.
const TRACE_VARIABLE: &str = "SWARMED_NOTIFY_TRACE";

/// Notes `line` in the file `SWARMED_NOTIFY_TRACE` names, if it names one. A notice's title
/// is the session's title and what happened, never what a tool was given.
pub(crate) fn trace(line: &str) {
    use std::io::Write;

    let Some(path) = std::env::var_os(TRACE_VARIABLE) else {
        return;
    };
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);
    if let Ok(mut file) = file {
        writeln!(file, "{line}").ok();
    }
}

/// Asks the user, the first time, whether Swarmed may show notifications.
#[cfg(target_os = "macos")]
pub(crate) fn ask_permission() {
    mac::ask_permission();
}

/// Asks nothing: this system's notifications are not known here.
#[cfg(not(target_os = "macos"))]
pub(crate) fn ask_permission() {}

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
    mac::ask_permission();
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

/// Says that there is nothing to check here. A future, as on macOS, though nothing is waited for.
#[cfg(not(target_os = "macos"))]
pub(crate) fn check() -> std::future::Ready<String> {
    std::future::ready("notifications are only shown on macOS".to_string())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::path::Path;

    use super::mac::{has_identity, lies_in_bundle};

    #[test]
    fn only_a_program_inside_an_application_bundle_is_one_the_system_knows() {
        let installed = "/Applications/Swarmed.app/Contents/MacOS/swarmed-app";
        assert!(lies_in_bundle(Path::new(installed)));
        // A development build, which carries a bundle identifier all the same.
        assert!(!lies_in_bundle(Path::new("/repo/target/debug/swarmed-app")));
        assert!(!lies_in_bundle(Path::new("/repo/Swarmed.app/swarmed-app")));
        assert!(!lies_in_bundle(Path::new("/MacOS/swarmed-app")));
        assert!(!lies_in_bundle(Path::new("swarmed-app")));
    }

    #[test]
    fn a_build_shown_as_the_terminal_does_not_come_to_look_like_a_bundle() {
        assert!(!has_identity());
        // What showing a notice as the Terminal's does first.
        mac_notification_sys::set_application("com.apple.Terminal").ok();
        // The program now answers to an identifier, which used to pass for an identity.
        assert!(mac_usernotifications::check_bundle().is_ok());
        assert!(!has_identity());
    }
}
