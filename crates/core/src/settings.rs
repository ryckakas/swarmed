//! Settings that apply to every project on this machine.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The user's own preferences, one TOML file in the data directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// The command that opens a folder in the user's code editor, such as `code` or `zed`.
    pub editor: String,
}

impl Default for Settings {
    /// Visual Studio Code.
    fn default() -> Self {
        Self {
            editor: "code".to_string(),
        }
    }
}

impl Settings {
    /// Loads the settings, or returns the defaults if the file does not exist.
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        toml::from_str(&text).map_err(|e| invalid(path, &e))
    }

    pub(crate) fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string(self).map_err(|e| invalid(path, &e))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, text)?;
        Ok(())
    }
}

fn invalid(path: &Path, error: &dyn std::fmt::Display) -> Error {
    Error::Config {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_save_and_default_to_vs_code() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("settings.toml");
        assert_eq!(Settings::load(&path).expect("defaults").editor, "code");

        let chosen = Settings {
            editor: "zed --new".to_string(),
        };
        chosen.save(&path).expect("saves");
        assert_eq!(Settings::load(&path).expect("loads"), chosen);
    }

    #[test]
    fn a_misspelt_key_is_an_error_not_a_silent_default() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "editr = \"zed\"").expect("writes");
        assert!(matches!(Settings::load(&path), Err(Error::Config { .. })));
    }
}
