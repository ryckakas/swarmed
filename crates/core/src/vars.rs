use std::collections::BTreeMap;

use crate::error::{Error, Result};

/// The values a project config can refer to as `{{name}}` in its branch pattern, hooks and
/// environment values.
#[derive(Debug, Clone, Default)]
pub(crate) struct Vars(BTreeMap<String, String>);

impl Vars {
    pub(crate) fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.insert(name.into(), value.into());
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Replaces every `{{name}}` in `template`. An unknown name is an error, so a typo in a
    /// config cannot silently produce a command with a hole in it.
    pub(crate) fn render(&self, template: &str) -> Result<String> {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let end = after
                .find("}}")
                .ok_or_else(|| Error::UnclosedVariable(template.to_string()))?;
            let name = after[..end].trim();
            let value = self
                .0
                .get(name)
                .ok_or_else(|| Error::UnknownVariable(name.to_string()))?;
            out.push_str(value);
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        let mut v = Vars::default();
        v.set("task", "fix-login");
        v.set("port.web", "3100");
        v
    }

    #[test]
    fn renders_known_variables() {
        let out = vars().render("docker compose -p {{task}} up # {{ port.web }}");
        assert_eq!(
            out.ok().as_deref(),
            Some("docker compose -p fix-login up # 3100")
        );
    }

    #[test]
    fn leaves_plain_text_alone() {
        let out = vars().render("echo ${HOME} {single}");
        assert_eq!(out.ok().as_deref(), Some("echo ${HOME} {single}"));
    }

    #[test]
    fn rejects_unknown_variable() {
        assert!(matches!(vars().render("{{nope}}"), Err(Error::UnknownVariable(n)) if n == "nope"));
    }

    #[test]
    fn rejects_unclosed_variable() {
        assert!(matches!(
            vars().render("{{task"),
            Err(Error::UnclosedVariable(_))
        ));
    }
}
