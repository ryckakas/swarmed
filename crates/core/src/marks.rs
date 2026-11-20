//! A mark on a session: a colour and a few words that say how its work stands.
//!
//! The status of a session says what its agent is doing. A mark says what came of it, in the
//! user's own terms: an agent, or a skill it follows, sets one when a check passed or a step
//! is waiting, and the sidebar shows it. It stays until it is set again, taken off, or the
//! user writes to the session, which starts something new.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::AgentEvent;
use crate::error::Error;
use crate::{Core, Result, Session};

/// How long a mark's words may be, in characters.
pub const MAX_MARK_LABEL_CHARS: usize = 40;

/// The colours a mark can have. They are names and not values, so each theme draws its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkColour {
    /// It went well.
    Green,
    /// Something to know.
    Blue,
    /// Something to look at.
    Yellow,
    /// It went wrong.
    Red,
    /// Whatever the user makes of it.
    Purple,
}

impl MarkColour {
    /// Every colour, in the order they are offered.
    pub const ALL: [Self; 5] = [
        Self::Green,
        Self::Blue,
        Self::Yellow,
        Self::Red,
        Self::Purple,
    ];

    /// The colour's name, as stored and as an agent writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Yellow => "yellow",
            Self::Red => "red",
            Self::Purple => "purple",
        }
    }

    /// The colour called `name`, whatever its case.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_lowercase();
        Self::ALL.into_iter().find(|colour| colour.as_str() == name)
    }
}

/// A mark on a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    /// The colour the session is shown in.
    pub colour: MarkColour,
    /// A few words beside it; may be empty.
    pub label: String,
}

/// What takes a mark off, where a colour is asked for.
const NO_MARK: &str = "none";

fn refused(why: impl Into<String>) -> Error {
    Error::Mark(why.into())
}

/// A mark's words as they are kept: one line, without the space around it.
fn label_of(text: &str) -> Result<String> {
    let label = text.trim().lines().next().unwrap_or_default().trim();
    if label.chars().count() > MAX_MARK_LABEL_CHARS {
        return Err(refused(format!(
            "a mark's label can be at most {MAX_MARK_LABEL_CHARS} characters"
        )));
    }
    Ok(label.to_string())
}

impl Mark {
    /// The mark that the words `colour` and `label` ask for, as an agent or a command line
    /// gives them; `None` if the colour is `none`, which takes a mark off.
    pub fn named(colour: &str, label: &str) -> Result<Option<Self>> {
        mark_of(colour, label)
    }
}

fn mark_of(colour: &str, label: &str) -> Result<Option<Mark>> {
    if colour.trim().eq_ignore_ascii_case(NO_MARK) {
        return Ok(None);
    }
    let colour = MarkColour::parse(colour).ok_or_else(|| {
        let known: Vec<&str> = MarkColour::ALL
            .into_iter()
            .map(MarkColour::as_str)
            .collect();
        refused(format!(
            "there is no colour `{}`; use one of {}, or `{NO_MARK}` to take the mark off",
            colour.trim(),
            known.join(", ")
        ))
    })?;
    let label = label_of(label)?;
    Ok(Some(Mark { colour, label }))
}

impl Core {
    /// Puts `mark` on a session, in place of the one it has, or takes its mark off, and tells
    /// every front end.
    pub fn session_mark(&self, id: i64, mark: Option<Mark>) -> Result<Session> {
        self.session(id)?;
        let mark = match mark {
            Some(mark) => Some(Mark {
                colour: mark.colour,
                label: label_of(&mark.label)?,
            }),
            None => None,
        };
        self.store().session_set_mark(id, mark.as_ref())?;
        let (colour, label) = match mark {
            Some(mark) => (Some(mark.colour), mark.label),
            None => (None, String::new()),
        };
        self.live
            .note(&self.store, id, AgentEvent::Marked { colour, label })?;
        self.session(id)
    }

    /// The `set_mark` tool, called by session `from` for itself.
    pub(crate) fn peer_mark(&self, from: i64, arguments: &Value) -> Result<String> {
        let word = |key: &str| arguments.get(key).and_then(Value::as_str);
        let colour = word("colour")
            .or_else(|| word("color"))
            .ok_or_else(|| refused("`colour` is missing"))?;
        let mark = mark_of(colour, word("label").unwrap_or_default())?;
        let said = match &mark {
            Some(mark) if mark.label.is_empty() => {
                format!("This session is now marked {}.", mark.colour.as_str())
            }
            Some(mark) => format!(
                "This session is now marked {}: {}.",
                mark.colour.as_str(),
                mark.label
            ),
            None => "This session's mark is taken off.".to_string(),
        };
        self.session_mark(from, mark)?;
        Ok(said)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_is_found_by_name_whatever_its_case() {
        assert_eq!(MarkColour::parse(" Green "), Some(MarkColour::Green));
        assert_eq!(MarkColour::parse("PURPLE"), Some(MarkColour::Purple));
        assert_eq!(MarkColour::parse("pink"), None);
    }

    #[test]
    fn a_mark_is_a_known_colour_and_one_short_line() {
        let mark = mark_of("green", "  e2e passed \nsecond line").ok();
        let passed = Mark {
            colour: MarkColour::Green,
            label: "e2e passed".to_string(),
        };
        assert_eq!(mark, Some(Some(passed)));
        assert_eq!(mark_of("None", "ignored").ok(), Some(None));
        assert!(mark_of("pink", "").is_err());
        assert!(mark_of("red", &"x".repeat(MAX_MARK_LABEL_CHARS + 1)).is_err());
    }
}
