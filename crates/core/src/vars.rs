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
        self.rendered(template, |_, _, value, out| out.push_str(value))
    }

    /// Replaces every `{{name}}` in a command that `sh -c` is to run, so that a value is one
    /// word of the command whatever is in it, and never a piece of shell.
    ///
    /// A plain value, of letters, digits and `_ - . / : @ % + = ,`, is written out as it is:
    /// it means the same wherever it stands. Any other is not written into the command at
    /// all. Its place is taken by its `SWARMED_<NAME>` variable, in double quotes where the
    /// template has none of its own, and the shell fills the value in without reading it. A
    /// branch called `x;rm -rf ~`, or a path with a space in it, so stays one word.
    pub(crate) fn render_for_shell(&self, template: &str) -> Result<String> {
        let mut shell = Shell::default();
        self.rendered(template, |before, name, value, out| {
            shell.read(before);
            out.push_str(&shell.filled_in(name, value));
        })
    }

    /// `template` with `place` called for every `{{name}}`: with the text since the one
    /// before, the name, its value, and what has been rendered so far to add to.
    fn rendered(
        &self,
        template: &str,
        mut place: impl FnMut(&str, &str, &str, &mut String),
    ) -> Result<String> {
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
            place(&rest[..start], name, value, &mut out);
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        Ok(out)
    }
}

/// The name a hook finds the variable `name` under in its environment: `port.web` is
/// `SWARMED_PORT_WEB`. Whatever a shell could not have in a name becomes an underscore.
pub(crate) fn env_name(name: &str) -> String {
    let part = |c: char| {
        if c.is_ascii_alphanumeric() {
            c.to_ascii_uppercase()
        } else {
            '_'
        }
    };
    format!("SWARMED_{}", name.chars().map(part).collect::<String>())
}

/// Whether `c` means nothing to a shell, wherever in a command it stands.
fn is_plain(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_-./:@%+=,".contains(c)
}

/// What kind of quotes a place in a shell command is inside.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Quotes {
    /// None. `depth` counts the brackets open, which is how the end of a `$(…)` is found.
    None {
        depth: u32,
    },
    Single,
    Double,
}

/// Where in a shell command a template has got to, as far as quoting goes.
///
/// It follows quotes, backslashes, comments and `$(…)`, and not everything a shell knows:
/// a here-document, say, is read as ordinary words. That costs no safety. A value is only
/// ever referred to by its variable, so a place that is misjudged gets a quote character
/// too many or too few around the value, never a value read as shell.
struct Shell {
    /// The quotes the place is inside, the innermost last. Never empty.
    inside: Vec<Quotes>,
    /// What the character before was.
    last: Last,
    /// Whether the place is in a comment, which lasts to the end of the line.
    comment: bool,
}

/// What the character before a place in a shell command was, as far as the next one cares.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    /// Nothing, or what ends a word: a `#` here starts a comment.
    Gap,
    /// Part of a word.
    Word,
    /// A `$`, which a `(` after it makes a `$(`.
    Dollar,
    /// A backslash, which takes the next character's meaning away.
    Backslash,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            inside: vec![Quotes::None { depth: 0 }],
            last: Last::Gap,
            comment: false,
        }
    }
}

impl Shell {
    fn now(&self) -> Quotes {
        let innermost = self.inside.last().copied();
        innermost.unwrap_or(Quotes::None { depth: 0 })
    }

    /// Reads on through `text`, which is the template's own and holds no value.
    fn read(&mut self, text: &str) {
        for c in text.chars() {
            self.read_char(c);
        }
    }

    fn read_char(&mut self, c: char) {
        if self.comment {
            self.comment = c != '\n';
            return;
        }
        let last = std::mem::replace(&mut self.last, Last::Word);
        if last == Last::Backslash {
            return;
        }
        match self.now() {
            Quotes::Single if c == '\'' => self.leave(),
            Quotes::Single => {}
            Quotes::Double => self.read_in_double(c, last),
            Quotes::None { depth } => self.read_bare(c, last, depth),
        }
    }

    fn read_in_double(&mut self, c: char, last: Last) {
        match c {
            '\\' => self.last = Last::Backslash,
            '$' => self.last = Last::Dollar,
            '"' => self.leave(),
            '(' if last == Last::Dollar => {
                self.inside.push(Quotes::None { depth: 1 });
                self.last = Last::Gap;
            }
            _ => {}
        }
    }

    fn read_bare(&mut self, c: char, last: Last, depth: u32) {
        if is_separator(c) {
            self.last = Last::Gap;
        }
        match c {
            '\\' => self.last = Last::Backslash,
            '$' => self.last = Last::Dollar,
            '\'' => self.inside.push(Quotes::Single),
            '"' => self.inside.push(Quotes::Double),
            '#' if last == Last::Gap => self.comment = true,
            '(' => self.set_depth(depth + 1),
            // The bracket that closes a `$(…)` begun inside double quotes ends this place.
            ')' if depth == 1 && self.inside.len() > 1 => self.leave(),
            ')' => self.set_depth(depth.saturating_sub(1)),
            _ => {}
        }
    }

    fn set_depth(&mut self, depth: u32) {
        if let Some(Quotes::None { depth: now }) = self.inside.last_mut() {
            *now = depth;
        }
    }

    /// Out of the innermost quotes, back to what they stand in.
    fn leave(&mut self) {
        if self.inside.len() > 1 {
            self.inside.pop();
        }
    }

    /// What stands in a command for the variable `name`: its `value` if that is plain, a
    /// reference to it otherwise.
    fn filled_in(&mut self, name: &str, value: &str) -> String {
        self.last = Last::Word;
        if value.chars().all(is_plain) {
            value.to_string()
        } else {
            self.reference(&env_name(name))
        }
    }

    /// The variable `name` written so that the shell puts its value here as it is, in one
    /// piece: inside the template's double quotes if it has them, in double quotes of its
    /// own otherwise, for which single quotes are left and taken up again.
    fn reference(&self, name: &str) -> String {
        match self.now() {
            Quotes::Double => format!("${{{name}}}"),
            Quotes::Single => format!("'\"${{{name}}}\"'"),
            Quotes::None { .. } => format!("\"${{{name}}}\""),
        }
    }
}

/// Whether `c` ends a word of a shell command, so that a `#` after it starts a comment.
fn is_separator(c: char) -> bool {
    c.is_whitespace() || ";&|()".contains(c)
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

    /// Variables with values a shell would make something of.
    fn awkward() -> Vars {
        let mut v = vars();
        v.set("repo", "/Users/me/My Code/app");
        v.set("branch", "x;touch${IFS}pwned");
        v
    }

    /// What `sh -c` prints for `template`, run as a hook is: with every variable in its
    /// environment as well.
    fn run(vars: &Vars, template: &str) -> String {
        let command = vars.render_for_shell(template).expect("renders");
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .envs(vars.iter().map(|(name, value)| (env_name(name), value)))
            .output()
            .expect("sh runs");
        assert!(output.status.success(), "`{command}` failed");
        String::from_utf8_lossy(&output.stdout).into_owned()
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
        assert!(matches!(
            vars().render_for_shell("{{nope}}"),
            Err(Error::UnknownVariable(_))
        ));
    }

    #[test]
    fn rejects_unclosed_variable() {
        assert!(matches!(
            vars().render("{{task"),
            Err(Error::UnclosedVariable(_))
        ));
    }

    #[test]
    fn a_plain_value_is_written_into_a_command_as_it_is() {
        let template = "docker compose -p {{task}} up -d && echo $(( {{port.web}} + 1 ))";
        assert_eq!(
            vars().render_for_shell(template).ok().as_deref(),
            Some("docker compose -p fix-login up -d && echo $(( 3100 + 1 ))")
        );
    }

    #[test]
    fn a_value_with_a_space_in_it_stays_one_word() {
        // Each `printf` prints its arguments one to a line: one line, one word.
        assert_eq!(
            run(&awkward(), "printf '%s\\n' {{repo}}/.env"),
            "/Users/me/My Code/app/.env\n"
        );
    }

    #[test]
    fn a_value_is_never_run_as_shell() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut vars = awkward();
        vars.set("worktree", dir.path().to_string_lossy());
        // Unquoted, the branch's name would end the `echo` and touch a file.
        let said = run(&vars, "cd {{worktree}} && echo {{branch}}");
        assert_eq!(said, "x;touch${IFS}pwned\n");
        assert!(!dir.path().join("pwned").exists());
    }

    #[test]
    fn a_value_keeps_to_the_quotes_its_template_put_it_in() {
        let vars = awkward();
        let whole = "/Users/me/My Code/app/.env on x;touch${IFS}pwned\n";
        for template in [
            "echo \"{{repo}}/.env on {{branch}}\"",
            "echo '{{repo}}/.env on {{branch}}'",
            "echo \"$(echo {{repo}}/.env) on {{branch}}\"",
            "echo {{repo}}/.env on {{branch}} # it's {{branch}}, isn't it",
            "true # don't mind this\necho {{repo}}/.env on {{branch}}",
            "echo \\\"{{repo}}/.env on {{branch}}\\\" | tr -d '\"'",
        ] {
            assert_eq!(run(&vars, template), whole, "{template}");
        }
    }

    #[test]
    fn a_variable_is_exported_under_a_name_a_shell_can_read() {
        assert_eq!(env_name("branch"), "SWARMED_BRANCH");
        assert_eq!(env_name("port.web"), "SWARMED_PORT_WEB");
        assert_eq!(env_name("port.my-api"), "SWARMED_PORT_MY_API");
    }
}
