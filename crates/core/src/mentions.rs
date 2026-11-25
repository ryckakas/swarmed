//! Sessions the user points an agent at with a message: by `@` in the message box, or with
//! `--session` on the command line.
//!
//! Only a reference is sent, never the other conversation: the agent reads as much of it as it
//! needs, when it needs it, with `read_session`. Each reference is checked before anything is
//! sent, under the reach rules of that tool (`peers.rs`), so that one the session cannot read is
//! refused where the user is rather than by the agent. Like attached files, the references are
//! listed under the message in one fixed form, which front ends read back to show them as
//! sessions. An agent that has no Swarmed tool server to read them with is given a short digest
//! of each in its place.

use std::fmt::Write;

use crate::attach::{Attached, with_files};
use crate::error::{Error, Result};
use crate::peers::{Access, Reach};
use crate::{Core, Session};

/// The line that opens the list of sessions a message mentions, for an agent that reads them
/// with its own tool.
pub const MENTIONED_SESSIONS: &str = "Sessions mentioned, to read with the read_session tool:";
/// The line that opens it for an agent with no tool to read them: each comes with what was
/// last said in it.
pub const MENTIONED_SESSIONS_SAID: &str = "Sessions mentioned, with what was last said in each:";

/// How many of a mentioned session's latest messages its digest holds, and how many characters
/// at most: a short look, where the agent cannot read further.
const DIGEST_MESSAGES: usize = 4;
const DIGEST_CHARS: usize = 2000;

/// A session as a message mentions it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    /// The session's id, which the agent reads it by.
    pub id: i64,
    /// Its title when the message went, for the agent to know it by.
    pub title: String,
    /// What was last said in it, for an agent that cannot read it; `None` for one that can.
    pub said: Option<String>,
}

/// A title on one line, as the list gives it.
fn one_line(title: &str) -> String {
    let words: Vec<&str> = title.split_whitespace().collect();
    if words.is_empty() {
        "untitled".to_string()
    } else {
        words.join(" ")
    }
}

/// `text` with `mentions` listed under it, in the form every front end reads back.
///
/// A heading line, then `- session <id> ("<title>")` for each, with what was said in it under
/// that in `<session_<id>>` tags when the agent cannot read it itself. Nothing is added for none.
#[must_use]
pub fn with_mentions(text: &str, mentions: &[Mention]) -> String {
    let text = text.trim();
    if mentions.is_empty() {
        return text.to_string();
    }
    let digests = mentions.iter().any(|mention| mention.said.is_some());
    let mut message = String::from(text);
    if !message.is_empty() {
        message.push_str("\n\n");
    }
    message.push_str(if digests {
        MENTIONED_SESSIONS_SAID
    } else {
        MENTIONED_SESSIONS
    });
    for mention in mentions {
        let Mention { id, title, said } = mention;
        // Writing to a `String` cannot fail.
        write!(message, "\n- session {id} (\"{}\")", one_line(title)).ok();
        if let Some(said) = said {
            write!(message, ":\n<session_{id}>\n{said}\n</session_{id}>").ok();
        }
    }
    message
}

/// `ids` in the order given, each once.
fn each_once(ids: &[i64]) -> Vec<i64> {
    let mut seen = Vec::with_capacity(ids.len());
    for id in ids {
        if !seen.contains(id) {
            seen.push(*id);
        }
    }
    seen
}

impl Core {
    /// The message session `id` is sent for `text` with what is `attached`: the words, the
    /// sessions it mentions listed under them, then the files. Refused, before anything is
    /// sent, if it names a file that is not there or a session this one cannot read.
    pub(crate) fn message_for(&self, id: i64, text: &str, attached: &Attached) -> Result<String> {
        let mentioned = if attached.sessions.is_empty() {
            text.trim().to_string()
        } else {
            let own = self.session(id)?;
            let reach = self.reach_of(&own)?;
            self.mentioned(reach, Some(id), text, &attached.sessions)?
        };
        with_files(&mentioned, &attached.files)
    }

    /// `text` with the sessions `ids` listed under it, each one a session of a new task of
    /// `project` could read: the first message of a task, checked before the task is made.
    pub(crate) fn mentioned_from(&self, project: &str, text: &str, ids: &[i64]) -> Result<String> {
        if ids.is_empty() {
            return Ok(text.trim().to_string());
        }
        self.mentioned(self.reach_of_project(project)?, None, text, ids)
    }

    /// `text` with the sessions `ids` listed under it: each one `reach` can read, and none of
    /// them `own`, the session the message goes to. Refused at the first that is not.
    pub(crate) fn mentioned(
        &self,
        reach: Reach,
        own: Option<i64>,
        text: &str,
        ids: &[i64],
    ) -> Result<String> {
        let sessions: Vec<Session> = each_once(ids)
            .into_iter()
            .map(|id| {
                let other = (own != Some(id)).then(|| self.in_reach(reach, id, Access::Read));
                other.flatten().ok_or(Error::CannotMention(id))
            })
            .collect::<Result<_>>()?;
        let mentions = sessions
            .into_iter()
            .map(|session| self.mention_of(session))
            .collect::<Result<Vec<_>>>()?;
        Ok(with_mentions(text, &mentions))
    }

    /// `session` as a message mentions it. Without Swarmed's tools the agent has no way to
    /// read a session, and is not told of one: it is given a look at it instead of a reference
    /// it could not follow.
    fn mention_of(&self, session: Session) -> Result<Mention> {
        let said = if self.tool_server.is_some() {
            None
        } else {
            let said = self.said_in(session.id, None, DIGEST_MESSAGES, DIGEST_CHARS)?;
            let said = said.map(|said| said.text);
            Some(said.unwrap_or_else(|| "Nothing has been said there yet.".to_string()))
        };
        Ok(Mention {
            id: session.id,
            title: session.title,
            said,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mention(id: i64, title: &str) -> Mention {
        Mention {
            id,
            title: title.to_string(),
            said: None,
        }
    }

    #[test]
    fn sessions_are_listed_under_the_message_by_id_and_title() {
        let mentions = [mention(42, "Fix the login form"), mention(7, "  ")];
        assert_eq!(
            with_mentions(" do what [@42 Fix the login form] did ", &mentions),
            "do what [@42 Fix the login form] did\n\n\
             Sessions mentioned, to read with the read_session tool:\n\
             - session 42 (\"Fix the login form\")\n\
             - session 7 (\"untitled\")"
        );
        // Mentions alone are a message; none add nothing.
        assert_eq!(
            with_mentions("", &mentions[..1]),
            "Sessions mentioned, to read with the read_session tool:\n\
             - session 42 (\"Fix the login form\")"
        );
        assert_eq!(with_mentions(" words ", &[]), "words");
    }

    #[test]
    fn a_title_is_given_on_one_line() {
        let mentions = [mention(3, "two\nlines\tand  gaps")];
        assert!(with_mentions("x", &mentions).ends_with("- session 3 (\"two lines and gaps\")"));
    }

    #[test]
    fn an_agent_that_cannot_read_a_session_is_given_what_was_said_in_it() {
        let mentions = [Mention {
            said: Some("User: hello\n\nAgent: hi".to_string()),
            ..mention(42, "Greeting")
        }];
        assert_eq!(
            with_mentions("see this", &mentions),
            "see this\n\n\
             Sessions mentioned, with what was last said in each:\n\
             - session 42 (\"Greeting\"):\n\
             <session_42>\nUser: hello\n\nAgent: hi\n</session_42>"
        );
    }

    #[test]
    fn a_session_is_listed_once() {
        assert_eq!(each_once(&[4, 2, 4, 9, 2]), [4, 2, 9]);
    }
}
