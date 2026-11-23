//! Cloning a file or a whole folder in one call, where the file system can.
//!
//! On APFS, `clonefile` copies a folder with everything in it as one call: the copy shares its
//! blocks with the original until either changes. `cp -c` clones too, but one file at a time,
//! which for the 275,000 files of a JavaScript monorepo's `node_modules` takes eight times as
//! long. Where a whole clone cannot be made, the caller copies instead.

use std::path::Path;

/// Clones `from` to `to`, which must not exist yet: a folder whole, with what is in it. Says
/// whether it did. It does not where the file system cannot, across volumes, for a symbolic
/// link or anything else that is neither a file nor a folder, and off macOS; then nothing of
/// `to` is left behind, for the caller to copy instead.
#[cfg(target_os = "macos")]
pub(crate) fn clone_whole(from: &Path, to: &Path) -> bool {
    use rustix::fs::{CloneFlags, Mode, OFlags, fclonefileat, open};

    let Ok(kind) = std::fs::symlink_metadata(from).map(|meta| meta.file_type()) else {
        return false;
    };
    let (Some(parent), Some(name)) = (to.parent(), to.file_name()) else {
        return false;
    };
    // Asked first: nothing is cloned over what is there.
    let taken = std::fs::symlink_metadata(to).is_ok();
    if taken || !(kind.is_file() || kind.is_dir()) {
        return false;
    }
    let opened = |path: &Path, flags: OFlags| open(path, flags | OFlags::CLOEXEC, Mode::empty());
    let (Ok(source), Ok(folder)) = (
        opened(from, OFlags::RDONLY | OFlags::NOFOLLOW),
        opened(parent, OFlags::RDONLY | OFlags::DIRECTORY),
    ) else {
        return false;
    };
    match fclonefileat(&source, &folder, name, CloneFlags::empty()) {
        Ok(()) => return true,
        // Something took the name since it was asked for: that is not this clone's to clear.
        Err(rustix::io::Errno::EXIST) => return false,
        Err(_) => {}
    }
    // A clone that failed part of the way must not leave half of `to` for the copy to land in.
    let _gone: std::io::Result<()> = if kind.is_dir() {
        std::fs::remove_dir_all(to)
    } else {
        std::fs::remove_file(to)
    };
    false
}

/// Off macOS there is no `clonefile`: the caller always copies, cloning per file where `cp`
/// can.
#[cfg(not(target_os = "macos"))]
pub(crate) fn clone_whole(_from: &Path, _to: &Path) -> bool {
    false
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_cloned_whole_with_its_links_and_never_over_what_is_there() {
        let dir = tempfile::tempdir().expect("temp dir");
        let from = dir.path().join("deps");
        std::fs::create_dir_all(from.join("acme/lib")).expect("test setup");
        std::fs::write(from.join("acme/lib/code.js"), "code").expect("test setup");
        std::os::unix::fs::symlink("acme/lib/code.js", from.join("main.js")).expect("test setup");

        let to = dir.path().join("worktree-deps");
        assert!(clone_whole(&from, &to));
        let read = |path: &str| std::fs::read_to_string(to.join(path)).expect("cloned");
        assert_eq!(read("acme/lib/code.js"), "code");
        // A link stays a link, to the same place.
        let link = std::fs::read_link(to.join("main.js")).expect("a link");
        assert_eq!(link, Path::new("acme/lib/code.js"));

        // What is there already is not cloned over, and is left as it was.
        std::fs::write(to.join("acme/lib/code.js"), "changed").expect("test setup");
        assert!(!clone_whole(&from, &to));
        assert_eq!(read("acme/lib/code.js"), "changed");
        // Nor is a link itself: the caller copies it as a link.
        assert!(!clone_whole(
            &from.join("main.js"),
            &dir.path().join("main.js")
        ));
        assert!(!dir.path().join("main.js").exists());
    }
}
