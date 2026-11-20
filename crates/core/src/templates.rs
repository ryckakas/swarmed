//! Prompts the user keeps to send again: a name and a text, the same for every agent.
//!
//! They live in one file in the data directory and are put into the message box by a front
//! end. The core never sends one by itself, and agents are not told about them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::{Core, Result, paths};

/// How long a kept prompt may be, in bytes.
const MAX_TEMPLATE_BYTES: usize = 50_000;

/// A prompt kept to be sent again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    /// What it is called, and found by after a `/` in the message box.
    pub name: String,
    /// What goes into the message box. `{{something}}` marks a place to fill in.
    pub text: String,
}

/// The file the prompts are kept in.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Kept {
    template: Vec<Template>,
}

fn refused(why: impl Into<String>) -> Error {
    Error::Template(why.into())
}

impl Core {
    fn templates_file(&self) -> PathBuf {
        self.paths.data_dir.join("templates.toml")
    }

    fn unreadable(&self, error: &dyn std::fmt::Display) -> Error {
        Error::Config {
            path: self.templates_file(),
            message: error.to_string(),
        }
    }

    /// The kept prompts, by name. None if there are none or the file cannot be read.
    pub fn templates(&self) -> Result<Vec<Template>> {
        let text = match std::fs::read_to_string(self.templates_file()) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut kept: Kept = toml::from_str(&text).map_err(|error| self.unreadable(&error))?;
        kept.template.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(kept.template)
    }

    fn templates_write(&self, template: Vec<Template>) -> Result<Vec<Template>> {
        let kept = Kept { template };
        let text = toml::to_string_pretty(&kept).map_err(|error| self.unreadable(&error))?;
        std::fs::create_dir_all(&self.paths.data_dir)?;
        std::fs::write(self.templates_file(), text)?;
        self.templates()
    }

    /// Keeps a prompt, in place of the one called `name` if there is one, and returns them all.
    pub fn template_save(&self, name: &str, text: &str) -> Result<Vec<Template>> {
        paths::validate_name(name)?;
        if text.trim().is_empty() {
            return Err(refused("a prompt needs some text"));
        }
        if text.len() > MAX_TEMPLATE_BYTES {
            return Err(refused(format!(
                "a prompt can hold at most {MAX_TEMPLATE_BYTES} bytes"
            )));
        }
        let mut kept = self.templates()?;
        kept.retain(|template| template.name != name);
        kept.push(Template {
            name: name.to_string(),
            text: text.trim().to_string(),
        });
        self.templates_write(kept)
    }

    /// Drops the prompt called `name` and returns the rest. Dropping one that is not there is
    /// not an error.
    pub fn template_remove(&self, name: &str) -> Result<Vec<Template>> {
        let mut kept = self.templates()?;
        kept.retain(|template| template.name != name);
        self.templates_write(kept)
    }
}
