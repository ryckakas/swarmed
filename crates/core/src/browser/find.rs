//! Which Chrome a task's browser runs on: one already on the machine, or one installed when
//! the user asks.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::agent_setup::find_program;

/// A browser that speaks Chrome's DevTools protocol, and where it is found.
struct Known {
    name: &'static str,
    /// Where its program is inside its application on a Mac, from `/Applications`.
    mac: &'static str,
    /// What its program is called elsewhere, on the `PATH`.
    programs: &'static [&'static str],
}

/// In the order they are looked for.
const KNOWN: &[Known] = &[
    Known {
        name: "Google Chrome",
        mac: "Google Chrome.app/Contents/MacOS/Google Chrome",
        programs: &["google-chrome", "google-chrome-stable"],
    },
    Known {
        name: "Chromium",
        mac: "Chromium.app/Contents/MacOS/Chromium",
        programs: &["chromium", "chromium-browser"],
    },
    Known {
        name: "Microsoft Edge",
        mac: "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        programs: &["microsoft-edge", "microsoft-edge-stable"],
    },
    Known {
        name: "Brave",
        mac: "Brave Browser.app/Contents/MacOS/Brave Browser",
        programs: &["brave-browser", "brave"],
    },
];

/// What installs a browser when none is found: Google Chrome, with Homebrew. Homebrew's
/// Chromium was taken out of it on 2026-09-01, since it does not pass the Mac's Gatekeeper.
const INSTALL: &str = "brew install --cask google-chrome";

/// The browser a task's Chrome would run on, or how to get one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowserSetup {
    /// Its name, such as "Google Chrome", or `None` if none was found.
    pub name: Option<String>,
    /// Its program.
    pub path: Option<PathBuf>,
    /// What would install one, as the user is shown it before it runs; `None` where Swarmed
    /// knows no way, as away from a Mac or without Homebrew.
    pub install_command: Option<String>,
    /// Whether the program was set in the settings rather than found.
    pub chosen: bool,
}

/// The browser to run: the program the settings name, else the first of the known ones in
/// `apps` (the folders a Mac keeps applications in) or in `dirs`.
pub(crate) fn find(chosen: &str, apps: &[PathBuf], dirs: &[PathBuf]) -> Option<(String, PathBuf)> {
    let chosen = chosen.trim();
    if !chosen.is_empty() {
        let path = PathBuf::from(chosen);
        return path.is_file().then(|| (name_of(&path), path));
    }
    KNOWN.iter().find_map(|known| {
        let app = apps
            .iter()
            .map(|folder| folder.join(known.mac))
            .find(|program| program.is_file());
        let found = app.or_else(|| {
            known
                .programs
                .iter()
                .find_map(|program| find_program(program, dirs))
        })?;
        Some((known.name.to_string(), found))
    })
}

/// What a program the user chose is called: the application it is in, or its own name.
fn name_of(program: &Path) -> String {
    let app = program
        .ancestors()
        .filter_map(Path::file_name)
        .find_map(|name| name.to_str()?.strip_suffix(".app"));
    let own = || {
        program
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    };
    app.map(String::from).or_else(own).unwrap_or_default()
}

/// The folders applications are kept in on this machine.
pub(crate) fn app_folders() -> Vec<PathBuf> {
    if !cfg!(target_os = "macos") {
        return vec![];
    }
    let mut folders = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::home_dir() {
        folders.push(home.join("Applications"));
    }
    folders
}

/// What installs a browser, as the shell is to run it, where there is a way.
pub(crate) fn install_command(dirs: &[PathBuf]) -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let brew = find_program("brew", dirs)?;
    Some(INSTALL.replacen("brew", &format!("'{}'", brew.display()), 1))
}

/// The install command as the user is shown it.
pub(crate) fn install_shown(dirs: &[PathBuf]) -> Option<String> {
    install_command(dirs).map(|_| INSTALL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(at: &Path) {
        std::fs::create_dir_all(at.parent().expect("has a folder")).expect("makes the folder");
        std::fs::write(at, "").expect("writes it");
    }

    #[test]
    fn chrome_is_looked_for_first_then_the_others_in_their_order() {
        let apps = tempfile::tempdir().expect("a folder");
        let bin = tempfile::tempdir().expect("a folder");
        let (apps_at, dirs) = (
            vec![apps.path().to_path_buf()],
            vec![bin.path().to_path_buf()],
        );
        assert_eq!(find("", &apps_at, &dirs), None);
        program(&bin.path().join("brave-browser"));
        assert_eq!(find("", &apps_at, &dirs).expect("found").0, "Brave");
        program(&apps.path().join(KNOWN[2].mac));
        assert_eq!(
            find("", &apps_at, &dirs).expect("found").0,
            "Microsoft Edge"
        );
        program(&bin.path().join("chromium"));
        assert_eq!(find("", &apps_at, &dirs).expect("found").0, "Chromium");
        program(&apps.path().join(KNOWN[0].mac));
        let (name, path) = find("", &apps_at, &dirs).expect("found");
        assert_eq!(name, "Google Chrome");
        assert!(path.ends_with("Contents/MacOS/Google Chrome"));
    }

    #[test]
    fn a_program_set_in_the_settings_wins_and_is_named_by_its_application() {
        let apps = tempfile::tempdir().expect("a folder");
        program(&apps.path().join(KNOWN[0].mac));
        let chosen = apps
            .path()
            .join("Chrome Canary.app/Contents/MacOS/Chrome Canary");
        program(&chosen);
        let apps_at = vec![apps.path().to_path_buf()];
        let found = find(&chosen.to_string_lossy(), &apps_at, &[]).expect("found");
        assert_eq!(found, ("Chrome Canary".to_string(), chosen));
        let plain = apps.path().join("bin/my-chromium");
        program(&plain);
        assert_eq!(
            find(&plain.to_string_lossy(), &apps_at, &[])
                .expect("found")
                .0,
            "my-chromium"
        );
        // One set and not there is not passed over for another: the user chose it.
        assert_eq!(find("/nowhere/chrome", &apps_at, &[]), None);
    }
}
