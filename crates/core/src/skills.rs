//! Skills: instructions for a kind of task, kept as a folder with a `SKILL.md` in it.
//!
//! Every agent keeps its own, in its own folder, and knows nothing of the others'. Swarmed
//! reads them all, keeps some of its own, and offers each session the ones its agent does not
//! have by itself: they are named in what the agent is told at the start, and handed over
//! through Swarmed's tool server when the agent asks for one. Nothing is copied into an
//! agent's own folders, and those are never written to.
//!
//! Two more places hold skills. The plugins installed in Claude Code bring their own, which
//! Claude Code reads by itself and other agents are offered. And a repository can keep skills
//! beside its code, in `.claude/skills` or `.agents/skills`: those go with the task, so a
//! session is offered the ones of its own worktree that its agent would not read by itself.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::agents::CLAUDE;
use crate::error::{Error, Result};
use crate::paths;

/// The file a skill is written in, inside its folder.
const SKILL_FILE: &str = "SKILL.md";
/// Whose a skill is, for one kept in Swarmed itself.
const OWN: &str = "swarmed";
/// Whose a skill is, for one a plugin installed in Claude Code brought.
const PLUGINS: &str = "plugins";
/// Whose a skill is, for one kept in the task's repository: in the folder Claude Code reads,
/// and in the one the other agents read.
const REPO_CLAUDE: &str = "repo-claude";
const REPO_AGENTS: &str = "repo-agents";
/// How much of a skill's description goes into what an agent is told at the start.
const TOLD_CHARS: usize = 220;
/// How long a skill of Swarmed's own may be, in bytes.
const MAX_SKILL_BYTES: usize = 100_000;

/// A skill as a front end shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skill {
    /// What it is called, and asked for by.
    pub name: String,
    /// When to use it, in its own words.
    pub description: String,
    /// Whose it is: `swarmed` for one kept here, the agent it was found with, `agents` for
    /// the folder several agents share, `plugins` for one a Claude Code plugin brought, or
    /// `repo-claude` and `repo-agents` for one kept in the task's repository.
    pub origin: String,
    /// The folder it lives in, with whatever else it needs beside its `SKILL.md`.
    pub folder: PathBuf,
    /// Whether it is offered to sessions whose agent does not have it by itself.
    pub shared: bool,
}

/// Which skills are kept back from other agents, by `origin/name`.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Sharing {
    hidden: BTreeSet<String>,
}

fn key(origin: &str, name: &str) -> String {
    format!("{origin}/{name}")
}

/// The name and the description in a skill's front matter: the `key: value` lines between
/// the first two `---` lines. A value may be quoted, or folded over indented lines after `>`
/// or `|`.
fn front_matter(text: &str) -> (Option<String>, String) {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, String::new());
    }
    let head: Vec<&str> = lines.take_while(|line| line.trim() != "---").collect();
    (
        value_of(&head, "name"),
        value_of(&head, "description").unwrap_or_default(),
    )
}

/// The value of `key` among front-matter `lines`, with the indented lines that continue it.
fn value_of(lines: &[&str], key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    let at = lines.iter().position(|line| line.starts_with(&prefix))?;
    let first = lines[at][prefix.len()..].trim();
    let continued = lines[at + 1..]
        .iter()
        .take_while(|line| line.starts_with([' ', '\t']) || line.is_empty());
    let folded = matches!(first, ">" | "|" | ">-" | "|-" | "");
    let start = (!folded).then_some(first);
    let words: Vec<&str> = start
        .into_iter()
        .chain(continued.map(|line| line.trim()))
        .filter(|piece| !piece.is_empty())
        .collect();
    let value = words.join(" ");
    let value = value.trim_matches(['"', '\'']).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// The skills in `folder`: every folder in it, or link to one, that holds a `SKILL.md`.
fn skills_in(folder: &Path, origin: &str) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return vec![];
    };
    let skill = |entry: std::fs::DirEntry| {
        let folder = entry.path();
        let text = std::fs::read_to_string(folder.join(SKILL_FILE)).ok()?;
        let (name, description) = front_matter(&text);
        Some(Skill {
            name: name.unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned()),
            description,
            origin: origin.to_string(),
            folder,
            shared: true,
        })
    };
    let mut skills: Vec<Skill> = entries.flatten().filter_map(skill).collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// The skills the plugins installed in Claude Code bring, each named `plugin:skill` as Claude
/// Code names it. Only plugins installed for the user count: one installed for a single
/// project belongs to that project's sessions, which Claude Code sees to itself.
fn plugin_skills(claude_home: &Path) -> Vec<Skill> {
    let record = claude_home.join("plugins").join("installed_plugins.json");
    let Ok(text) = std::fs::read_to_string(record) else {
        return vec![];
    };
    let record: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let Some(plugins) = record.get("plugins").and_then(serde_json::Value::as_object) else {
        return vec![];
    };
    let mut skills: Vec<Skill> = plugins
        .iter()
        .flat_map(|(id, installs)| installed_skills(id, installs))
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills.dedup_by(|a, b| a.name == b.name);
    skills
}

/// The skills of one plugin, `plugin@marketplace`, from wherever it is installed for the user.
fn installed_skills(id: &str, installs: &serde_json::Value) -> Vec<Skill> {
    let plugin = id.split('@').next().unwrap_or(id);
    let for_user = |install: &&serde_json::Value| {
        install.get("scope").and_then(serde_json::Value::as_str) == Some("user")
    };
    let folder = |install: &serde_json::Value| {
        let path = install.get("installPath")?.as_str()?;
        Some(PathBuf::from(path).join("skills"))
    };
    let installs = installs.as_array().map_or_default(Vec::as_slice);
    let found = installs.iter().filter(for_user).filter_map(folder);
    found
        .flat_map(|folder| skills_in(&folder, PLUGINS))
        .map(|skill| Skill {
            name: format!("{plugin}:{}", skill.name),
            ..skill
        })
        .collect()
}

/// The skills a repository keeps beside its code, as checked out in `worktree`.
fn repo_skills(worktree: &Path) -> Vec<Skill> {
    let claude = worktree.join(".claude").join("skills");
    let agents = worktree.join(".agents").join("skills");
    let mut skills = skills_in(&claude, REPO_CLAUDE);
    skills.extend(skills_in(&agents, REPO_AGENTS));
    skills
}

/// Whether `agent` reads the skills of `origin` by itself, so that it need not be offered them.
fn reads_itself(agent: &str, origin: &str) -> bool {
    match origin {
        PLUGINS | REPO_CLAUDE => agent == CLAUDE,
        REPO_AGENTS => agent != CLAUDE,
        _ => origin == agent || origin == native_also(agent),
    }
}

impl Core {
    /// Where each agent keeps its skills, by whose they are. The folder several agents read
    /// goes by `agents`.
    fn skill_folders(&self) -> Vec<(&'static str, PathBuf)> {
        let home = self
            .paths
            .claude_config
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let codex = self
            .paths
            .codex_config
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let opencode = &self.paths.opencode_config_dir;
        vec![
            (CLAUDE, home.join(".claude").join("skills")),
            ("codex", codex.join("skills")),
            ("opencode", opencode.join("skill")),
            ("opencode", opencode.join("skills")),
            ("agents", home.join(".agents").join("skills")),
        ]
    }

    fn own_skills_folder(&self) -> PathBuf {
        self.paths.data_dir.join("skills")
    }

    fn sharing(&self) -> Sharing {
        let text = std::fs::read_to_string(self.paths.data_dir.join("skills.toml"));
        text.ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Every skill Swarmed knows of wherever a session runs: its own first, then each
    /// agent's by name, then the ones Claude Code's plugins bring.
    pub fn skills(&self) -> Vec<Skill> {
        self.skills_with(None)
    }

    /// The same, with the ones kept in the repository checked out in `worktree` at the end.
    fn skills_with(&self, worktree: Option<&Path>) -> Vec<Skill> {
        let sharing = self.sharing();
        let mut skills = skills_in(&self.own_skills_folder(), OWN);
        for (origin, folder) in self.skill_folders() {
            skills.extend(skills_in(&folder, origin));
        }
        let home = self.paths.claude_config.parent();
        skills.extend(home.map_or_else(Vec::new, |home| plugin_skills(&home.join(".claude"))));
        skills.extend(worktree.map_or_else(Vec::new, repo_skills));
        for skill in &mut skills {
            skill.shared = !sharing.hidden.contains(&key(&skill.origin, &skill.name));
        }
        skills
    }

    /// Says whether a skill is offered to sessions whose agent does not have it by itself.
    pub fn skill_share(&self, origin: &str, name: &str, shared: bool) -> Result<()> {
        let mut sharing = self.sharing();
        if shared {
            sharing.hidden.remove(&key(origin, name));
        } else {
            sharing.hidden.insert(key(origin, name));
        }
        let path = self.paths.data_dir.join("skills.toml");
        let text = toml::to_string(&sharing).map_err(|error| Error::Config {
            path: path.clone(),
            message: error.to_string(),
        })?;
        std::fs::create_dir_all(&self.paths.data_dir)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Adds a skill of Swarmed's own, or replaces the one called `name`: when to use it, and
    /// what it says to do.
    pub fn skill_save(&self, name: &str, description: &str, body: &str) -> Result<()> {
        paths::validate_name(name)?;
        let description = description.split_whitespace().collect::<Vec<_>>().join(" ");
        if description.is_empty() || body.trim().is_empty() {
            return Err(Error::Skill(
                "a skill needs a line on when to use it, and something to say".to_string(),
            ));
        }
        if body.len() > MAX_SKILL_BYTES {
            return Err(Error::Skill(format!(
                "a skill can hold at most {MAX_SKILL_BYTES} bytes"
            )));
        }
        let folder = self.own_skills_folder().join(name);
        std::fs::create_dir_all(&folder)?;
        let quoted = description.replace('"', "'");
        let text = format!(
            "---\nname: {name}\ndescription: \"{quoted}\"\n---\n\n{}\n",
            body.trim()
        );
        std::fs::write(folder.join(SKILL_FILE), text)?;
        Ok(())
    }

    /// What a skill of Swarmed's own says to do, without its front matter, for editing.
    pub fn skill_body(&self, name: &str) -> Result<String> {
        paths::validate_name(name)?;
        let file = self.own_skills_folder().join(name).join(SKILL_FILE);
        let text = std::fs::read_to_string(file).map_err(|_| no_skill(name))?;
        Ok(without_front_matter(&text).trim().to_string())
    }

    /// Removes a skill of Swarmed's own, with everything in its folder.
    pub fn skill_remove(&self, name: &str) -> Result<()> {
        paths::validate_name(name)?;
        let folder = self.own_skills_folder().join(name);
        match std::fs::remove_dir_all(folder) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }

    /// The skills offered to a session on `agent` that works in `worktree`, if it is known
    /// where it works: the shared ones its agent does not have by itself, each name once,
    /// Swarmed's own before the others'.
    pub fn skills_offered(&self, agent: &str, worktree: Option<&Path>) -> Vec<Skill> {
        let all = self.skills_with(worktree);
        let native = |skill: &Skill| reads_itself(agent, &skill.origin);
        let has: BTreeSet<&str> = all
            .iter()
            .filter(|skill| native(skill))
            .map(|skill| skill.name.as_str())
            .collect();
        let mut taken: BTreeSet<&str> = has.clone();
        let offered = all
            .iter()
            .filter(|skill| skill.shared && !native(skill))
            // `insert` says whether the name is new: each is offered once.
            .filter(|skill| taken.insert(skill.name.as_str()));
        offered.cloned().collect()
    }

    /// What a session on `agent` is told about the skills it is offered, or `None` if there
    /// are none.
    pub(crate) fn skills_guidance(&self, agent: &str, worktree: &Path) -> Option<String> {
        let offered = self.skills_offered(agent, Some(worktree));
        if offered.is_empty() {
            return None;
        }
        let line = |skill: &Skill| format!("- `{}`: {}", skill.name, told(&skill.description));
        let list: Vec<String> = offered.iter().map(line).collect();
        Some(format!("{SKILLS_HEADING}\n{}", list.join("\n")))
    }

    /// A skill as it is handed to the agent of session `from` that asked for it: what it says,
    /// and where its other files are.
    pub(crate) fn skill_for_session(
        &self,
        agent: &str,
        worktree: &Path,
        name: &str,
    ) -> Result<String> {
        let offered = self.skills_offered(agent, Some(worktree));
        let skill = offered.iter().find(|skill| skill.name == name);
        let skill = skill.ok_or_else(|| no_skill(name))?;
        let text = std::fs::read_to_string(skill.folder.join(SKILL_FILE))?;
        Ok(format!(
            "{}\n\n(This skill's folder is {}. Paths it mentions are relative to that folder.)",
            without_front_matter(&text).trim(),
            skill.folder.display()
        ))
    }
}

/// What stands above the list of skills where an agent reads it.
const SKILLS_HEADING: &str = "\
# Skills from Swarmed

Besides any skills of your own, the user has these. Each is a set of instructions for one kind \
of task. Before doing work that one of them covers, call `use_skill` on the `swarmed` tool \
server (in Claude Code: `mcp__swarmed__use_skill`) with its name, and follow what it says:";

/// The folder a second set of skills comes from that `agent` reads by itself: Codex and
/// OpenCode both read the one several agents share.
fn native_also(agent: &str) -> &'static str {
    if agent == CLAUDE { "" } else { "agents" }
}

fn no_skill(name: &str) -> Error {
    Error::Skill(format!("there is no skill called `{name}`"))
}

/// A description cut to what fits a line of the list an agent is given.
fn told(description: &str) -> String {
    if description.chars().count() <= TOLD_CHARS {
        return description.to_string();
    }
    let mut cut: String = description.chars().take(TOLD_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// A skill's text after its front matter; all of it, if it has none.
fn without_front_matter(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---") else {
        return text;
    };
    rest.split_once("\n---")
        .map_or(text, |(_, body)| body.trim_start_matches('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_description_are_read_from_the_front_matter() {
        let plain =
            "---\nname: bug-hunter\ndescription: Find the bug. Use for defects.\n---\n\n# Body\n";
        let read = front_matter(plain);
        assert_eq!(read.0.as_deref(), Some("bug-hunter"));
        assert_eq!(read.1, "Find the bug. Use for defects.");
        assert_eq!(without_front_matter(plain).trim(), "# Body");

        let folded = "---\nname: \"review\"\ndescription: >\n  Review a change\n  for quality.\nlicense: MIT\n---\nbody";
        let read = front_matter(folded);
        assert_eq!(read.0.as_deref(), Some("review"));
        assert_eq!(read.1, "Review a change for quality.");

        // No front matter: the folder's name will do, and the whole text is the skill.
        assert_eq!(front_matter("# Just words"), (None, String::new()));
        assert_eq!(without_front_matter("# Just words"), "# Just words");
    }

    #[test]
    fn a_long_description_is_cut_for_the_list() {
        assert_eq!(told("short"), "short");
        let cut = told(&"x".repeat(500));
        assert_eq!(cut.chars().count(), TOLD_CHARS);
        assert!(cut.ends_with('…'));
    }
}
