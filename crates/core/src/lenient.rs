//! Reading configuration that a newer Swarmed, an older one or a person may have written. What
//! this version understands is used. What it does not costs only itself and is said, never a
//! reason to refuse the file: two Swarmeds of different versions can share one data
//! directory, and one key the older does not know must not stop it.

use std::collections::BTreeMap;
use std::path::Path;

use serde::de::DeserializeOwned;

use crate::config::ProjectConfig;
use crate::error::{Error, Result};
use crate::{Core, Settings, agents, environments, layouts, mcp_own, stylesheet};

/// What could not be understood, each said for the user.
pub(crate) type Problems = Vec<String>;

/// The text of the file at `path`, or `None` if there is no such file.
pub(crate) fn read_if_there(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// What to tell the user of a file that is not TOML: where it went wrong, and `meanwhile`,
/// what is done until it is fixed.
pub(crate) fn not_toml(text: &str, error: &toml::de::Error, meanwhile: &str) -> String {
    let before = error.span().and_then(|span| text.get(..span.start));
    let line = before.map_or(1, |before| before.matches('\n').count() + 1);
    format!(
        "the file is not TOML (line {line}: {}). {meanwhile}",
        error.message().trim()
    )
}

/// The name a file that is not TOML is kept under, beside it, when it is written over.
pub(crate) fn unreadable_name(path: &Path) -> String {
    let name = path.file_name().map(|name| name.to_string_lossy());
    format!("{}.unreadable", name.unwrap_or_default())
}

/// Moves the file at `path` aside, to `unreadable_name`, if it is there and is not TOML: it is
/// about to be written over, and what the user wrote in it must not be lost to the defaults
/// that stood in for it.
pub(crate) fn keep_if_unreadable(path: &Path) -> Result<()> {
    let Some(text) = read_if_there(path)? else {
        return Ok(());
    };
    if text.parse::<toml::Table>().is_err() {
        std::fs::rename(path, path.with_file_name(unreadable_name(path)))?;
    }
    Ok(())
}

/// Where `key` stands under `at`, as the user would look for it in the file.
fn path_of(at: &str, key: &str) -> String {
    if at.is_empty() {
        key.to_string()
    } else {
        format!("{at}.{key}")
    }
}

/// What to tell the user of a key left out, by where it stands in its file.
pub(crate) fn unknown_said(key: &str) -> String {
    format!(
        "`{key}` is not something this version of Swarmed knows, perhaps something a newer \
         version wrote: it is left out."
    )
}

/// `value` read as `T`, leaving out what `T` has no place for, at any depth: each key left out
/// is said, by where it stands under `at`. A value `T` cannot take is the error, in words.
fn read_as<T: DeserializeOwned>(
    value: toml::Value,
    at: &str,
) -> std::result::Result<(T, Problems), String> {
    let mut unknown = Vec::new();
    let read = serde_ignored::deserialize(value, |key| unknown.push(key.to_string()))
        .map_err(|error: toml::de::Error| error.message().trim().to_string())?;
    let said = unknown
        .into_iter()
        .map(|key| unknown_said(&path_of(at, &key)));
    Ok((read, said.collect()))
}

/// A table of settings read key by key: a key `T` does not know is left out, and one whose value
/// it cannot take leaves that setting at its default. Both are said.
pub(crate) fn fields<T: DeserializeOwned + Default>(table: toml::Table) -> (T, Problems) {
    let mut problems = Vec::new();
    let mut taken = toml::Table::new();
    for (key, value) in table {
        let alone = toml::Table::from_iter([(key.clone(), value.clone())]);
        match read_as::<T>(toml::Value::Table(alone), "") {
            Ok((_, unknown)) => {
                problems.extend(unknown);
                taken.insert(key, value);
            }
            Err(why) => problems.push(format!(
                "`{key}` could not be read ({why}), so it is at its default."
            )),
        }
    }
    // Each key was read on its own already, so together they read too.
    let read = read_as::<T>(toml::Value::Table(taken), "").map(|(read, _)| read);
    (read.unwrap_or_default(), problems)
}

/// A table of named entries, each read on its own: one that cannot be read is left out, and a
/// key an entry does not know is left out of it. Both are said.
pub(crate) fn entries<T: DeserializeOwned>(table: toml::Table) -> (BTreeMap<String, T>, Problems) {
    entries_at(table, "")
}

/// [`entries`] of a table that stands under `at` in its file, such as `role_defaults` in the
/// settings: what is said names each entry by where it stands.
pub(crate) fn entries_at<T: DeserializeOwned>(
    table: toml::Table,
    at: &str,
) -> (BTreeMap<String, T>, Problems) {
    let mut problems = Vec::new();
    let mut read = BTreeMap::new();
    for (name, value) in table {
        let name_at = path_of(at, &name);
        match read_as::<T>(value, &name_at) {
            Ok((entry, unknown)) => {
                problems.extend(unknown);
                read.insert(name, entry);
            }
            Err(why) => problems.push(format!(
                "`{name_at}` could not be read ({why}), so it is left out."
            )),
        }
    }
    (read, problems)
}

impl Core {
    /// What of Swarmed's configuration could not be understood, each said for the user and led
    /// by the file it is in: the settings, the agents, the MCP servers set up here, the skills
    /// kept back, the custom layout, and each repository's configuration and environments.
    /// What was understood is in use; none of this stops the app. Empty when all of it was
    /// understood.
    pub fn config_problems(&self) -> Result<Vec<String>> {
        let paths = &self.paths;
        let mut said = Vec::new();
        let mut add = |path: &Path, problems: Problems| {
            let file = path.strip_prefix(&paths.data_dir).unwrap_or(path);
            let file = file.display();
            said.extend(
                problems
                    .into_iter()
                    .map(|problem| format!("{file}: {problem}")),
            );
        };
        add(&paths.settings(), Settings::read(&paths.settings())?.1);
        add(
            &paths.agents_config(),
            agents::problems_in(&paths.agents_config())?,
        );
        add(
            &paths.mcp_config(),
            mcp_own::problems_in(&paths.mcp_config())?,
        );
        add(
            &paths.data_dir.join("skills.toml"),
            paths.sharing_problems(),
        );
        add(
            &paths.custom_layout(),
            layouts::problems_in(&paths.custom_layout()),
        );
        add(
            &paths.stylesheet(),
            stylesheet::problems_in(&paths.stylesheet()),
        );
        for project in self.projects()? {
            let config = paths.project_config(&project.name);
            add(
                &config,
                said_of(ProjectConfig::read(&config).map(|(_, problems)| problems)),
            );
            let environments = paths.project_environments(&project.name);
            add(
                &environments,
                said_of(environments::problems_in(&environments)),
            );
        }
        Ok(said)
    }
}

/// What to tell the user of a file read with `read`: its problems, or why it could not be read.
fn said_of(read: Result<Problems>) -> Problems {
    match read {
        Ok(problems) => problems,
        Err(Error::Config { message, .. }) => vec![message],
        Err(error) => vec![error.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, PartialEq, Deserialize)]
    #[serde(default)]
    struct Looks {
        theme: String,
        size: u16,
        inner: Inner,
    }

    #[derive(Debug, Default, PartialEq, Deserialize)]
    #[serde(default)]
    struct Inner {
        colour: String,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Program {
        command: String,
    }

    fn table(text: &str) -> toml::Table {
        text.parse().expect("TOML")
    }

    #[test]
    fn a_settings_table_keeps_what_it_understands_and_names_the_rest() {
        let text =
            "theme = \"dark\"\nsize = \"big\"\nlater = 1\n[inner]\ncolour = \"red\"\nshade = 2\n";
        let (looks, problems) = fields::<Looks>(table(text));
        assert_eq!(
            looks,
            Looks {
                theme: "dark".to_string(),
                size: 0,
                inner: Inner {
                    colour: "red".to_string()
                }
            }
        );
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(
            problems[0].starts_with("`inner.shade` is not something"),
            "{problems:?}"
        );
        assert!(
            problems[1].starts_with("`later` is not something"),
            "{problems:?}"
        );
        assert!(
            problems[2].starts_with("`size` could not be read"),
            "{problems:?}"
        );
    }

    #[test]
    fn an_entry_that_cannot_be_read_is_left_out_and_the_others_stay() {
        let text = "[good]\ncommand = \"a\"\nlater = true\n[bad]\ncomand = \"typo\"\n";
        let (read, problems) = entries::<Program>(table(text));
        assert_eq!(read.keys().collect::<Vec<_>>(), ["good"]);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems[0].contains("`bad` could not be read"),
            "{problems:?}"
        );
        assert!(
            problems[1].starts_with("`good.later` is not something"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_file_that_is_not_toml_is_said_with_its_line_and_kept_before_it_is_written_over() {
        let text = "a = 1\nb = \"open\n";
        let error = text.parse::<toml::Table>().expect_err("not TOML");
        assert!(not_toml(text, &error, "Meanwhile.").contains("(line 2:"));

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("mcp.toml");
        std::fs::write(&path, text).expect("writes");
        keep_if_unreadable(&path).expect("kept");
        let kept = std::fs::read_to_string(dir.path().join("mcp.toml.unreadable")).expect("kept");
        assert_eq!(kept, text);
        assert!(!path.exists());
        // A file that is TOML stays where it is.
        std::fs::write(&path, "a = 1\n").expect("writes");
        keep_if_unreadable(&path).expect("nothing to keep");
        assert!(path.exists());
    }
}
