//! The keys held as Swarmed starts, for the ways out they open.

/// Whether ⌥ is held now. Asked as the app starts, before its window shows: held, it leaves the
/// user's stylesheet out until Swarmed quits (`stylesheet.rs` in the core).
#[cfg(target_os = "macos")]
pub(crate) fn option_held() -> bool {
    use objc2_app_kit::{NSEvent, NSEventModifierFlags};
    NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Option)
}

/// Whether ⌥ is held now: there is no asking elsewhere, and `swarmed layout stylesheet off`
/// is the way out there.
#[cfg(not(target_os = "macos"))]
pub(crate) const fn option_held() -> bool {
    false
}
