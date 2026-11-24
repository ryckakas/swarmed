//! The branch prefix: what a new task's branch starts with, as `{{prefix}}` in a repository's
//! `branch_pattern`, whose default is `{{prefix}}/{{task}}`.

use crate::error::{Error, Result};
use crate::git;

/// The name of the task a branch prefix is shown and checked with.
pub const EXAMPLE_TASK: &str = "fix-login";

/// `prefix` as it is kept: without blank space or slashes at either end, since the pattern
/// joins it to the task with one `/` of its own. Those inside stay: `jd/feat` gives
/// `jd/feat/fix-login`.
pub(crate) fn trimmed(prefix: &str) -> &str {
    prefix.trim_matches(|c: char| c == '/' || c.is_whitespace())
}

/// What `{{prefix}}` stands for: the prefix, or the user's login when there is none, which is
/// what branches started with before there was a prefix.
pub(crate) fn resolved(prefix: &str) -> String {
    match trimmed(prefix) {
        "" => login(),
        prefix => prefix.to_string(),
    }
}

/// The branch a task called [`EXAMPLE_TASK`] gets with `prefix` as the branch prefix, by the
/// default pattern: `jd` gives `jd/fix-login`, and an empty one the user's login.
///
/// # Errors
///
/// [`Error::BranchPrefix`], with git's reason, when git would not take that as a branch's
/// name; an I/O error when git cannot be run.
pub fn branch_example(prefix: &str) -> Result<String> {
    let prefix = resolved(prefix);
    let branch = format!("{prefix}/{EXAMPLE_TASK}");
    match git::branch_name_problem(&branch)? {
        None => Ok(branch),
        Some(reason) => Err(Error::BranchPrefix { prefix, reason }),
    }
}

/// The user's login, which `{{user}}` stands for.
pub(crate) fn login() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slashes_and_space_at_the_ends_are_dropped_and_those_inside_kept() {
        assert_eq!(trimmed("jd"), "jd");
        assert_eq!(trimmed(" /jd/ "), "jd");
        assert_eq!(trimmed("//jd/feat//"), "jd/feat");
        assert_eq!(trimmed(" / "), "");
        assert_eq!(resolved("/team-a/"), "team-a");
        assert_eq!(resolved(""), login());
    }

    #[test]
    fn a_prefix_is_shown_as_the_branch_it_gives() {
        assert_eq!(branch_example("jd").expect("valid"), "jd/fix-login");
        assert_eq!(
            branch_example("/jd/feat/").expect("valid"),
            "jd/feat/fix-login"
        );
        // Empty, it is what it always was: the login.
        assert_eq!(
            branch_example("  ").expect("valid"),
            format!("{}/{EXAMPLE_TASK}", login())
        );
    }

    #[test]
    fn a_prefix_git_would_not_take_is_refused_with_its_reason() {
        for prefix in [
            "a b", "a..b", "x~1", "feat:", ".hidden", "-x", "jd.lock", "a//b",
            // Inside a repository git reads this as the branch it was on before, and prints
            // that one; outside one it refuses it.
            "@{-1}",
        ] {
            let refused = branch_example(prefix);
            assert!(
                matches!(
                    &refused,
                    Err(Error::BranchPrefix { prefix: kept, reason })
                        if kept == prefix && reason.contains("not a valid branch name")
                ),
                "{prefix:?}: {refused:?}"
            );
        }
    }
}
