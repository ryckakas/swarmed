//! Finding a conversation again by what was said in it: the user's messages and the agents'
//! replies, across every session, closed ones included.

use rusqlite::params;
use serde::Serialize;

use crate::agent::AgentEvent;
use crate::{Core, Result};

/// How many sessions a search returns at most: more than that is not read anyway.
const MAX_SESSIONS: usize = 40;
/// How many matching events are looked at, newest first, before giving up on more sessions.
const MAX_EVENTS: usize = 2000;
/// How much of the text around a match is shown, in characters each way.
const AROUND: usize = 60;

/// One session in which the words searched for were said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchHit {
    /// The session.
    pub session_id: i64,
    /// Its title.
    pub title: String,
    /// The repository it belongs to.
    pub project: String,
    /// The task it belongs to.
    pub task: String,
    /// Whether the session was closed, and has to be reopened to be read.
    pub closed: bool,
    /// The newest place the words were said, with a little of what stands around them.
    pub snippet: String,
    /// Whether that was said by the user, not by the agent.
    pub by_user: bool,
    /// How many of the session's messages hold the words.
    pub matches: u32,
}

/// The text of an event that is something somebody said, and whether the user said it.
fn said(event: &AgentEvent) -> Option<(&str, bool)> {
    match event {
        AgentEvent::UserMessage { text } => Some((text, true)),
        AgentEvent::Text { text } => Some((text, false)),
        _ => None,
    }
}

/// `text` around the first place `query` stands in it, whatever its case, on one line.
fn snippet(text: &str, query: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .filter_map(|c| c.to_lowercase().next())
        .collect();
    let wanted: Vec<char> = query.to_lowercase().chars().collect();
    let at = lower
        .windows(wanted.len().max(1))
        .position(|window| window == wanted)?;
    let from = at.saturating_sub(AROUND);
    let to = (at + wanted.len() + AROUND).min(chars.len());
    let cut: String = chars[from..to].iter().collect();
    let line = cut.split_whitespace().collect::<Vec<_>>().join(" ");
    let before = if from > 0 { "…" } else { "" };
    let after = if to < chars.len() { "…" } else { "" };
    Some(format!("{before}{line}{after}"))
}

/// `query` as a `LIKE` pattern that finds it anywhere in an event's JSON: escaped the way JSON
/// escapes a string, then with the pattern's own special characters made literal.
fn pattern(query: &str) -> String {
    let json = serde_json::to_string(query).unwrap_or_default();
    let inner = json.strip_prefix('"').unwrap_or(&json);
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    let mut pattern = String::from("%");
    for c in inner.chars() {
        if matches!(c, '%' | '_' | '!') {
            pattern.push('!');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

impl Core {
    /// The sessions in which `query` was said, by the user or an agent, newest match first.
    /// Closed sessions are searched too. An empty query finds nothing.
    pub fn sessions_search(&self, query: &str) -> Result<Vec<SearchHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let store = self.store();
        // The pattern narrows the rows down; what was really said is checked on the event
        // itself, since the same letters can stand in a tool's output or a field's name.
        let mut statement = store.conn.prepare(
            "SELECT e.session_id, e.event, s.title, s.closed_at IS NOT NULL, t.name, p.name
             FROM session_events e
             JOIN sessions s ON s.id = e.session_id
             JOIN tasks t ON t.id = s.task_id
             JOIN projects p ON p.id = t.project_id
             WHERE e.event LIKE ?1 ESCAPE '!'
             ORDER BY e.id DESC LIMIT ?2",
        )?;
        let limit = i64::try_from(MAX_EVENTS).unwrap_or(i64::MAX);
        let mut rows = statement.query(params![pattern(query), limit])?;
        let mut hits: Vec<SearchHit> = Vec::new();
        while let Some(row) = rows.next()? {
            add(&mut hits, hit_from(row, query)?);
        }
        Ok(hits)
    }
}

/// One row of the search as a hit, if its event is something said that holds `query`.
fn hit_from(row: &rusqlite::Row<'_>, query: &str) -> rusqlite::Result<Option<SearchHit>> {
    let event: String = row.get(1)?;
    let Some((snippet, by_user)) = found_in(&event, query) else {
        return Ok(None);
    };
    Ok(Some(SearchHit {
        session_id: row.get(0)?,
        title: row.get(2)?,
        closed: row.get(3)?,
        task: row.get(4)?,
        project: row.get(5)?,
        snippet,
        by_user,
        matches: 1,
    }))
}

/// Where `query` stands in what a stored event says, if the event is something said at all.
fn found_in(event: &str, query: &str) -> Option<(String, bool)> {
    let event: AgentEvent = serde_json::from_str(event).ok()?;
    let (text, by_user) = said(&event)?;
    Some((snippet(text, query)?, by_user))
}

/// Counts `hit` towards its session if that is listed already, and lists it otherwise while
/// there is room. Nothing found changes nothing.
fn add(hits: &mut Vec<SearchHit>, hit: Option<SearchHit>) {
    let Some(hit) = hit else {
        return;
    };
    let listed = hits.iter().position(|one| one.session_id == hit.session_id);
    match listed {
        Some(at) => hits[at].matches += 1,
        None if hits.len() < MAX_SESSIONS => hits.push(hit),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snippet_shows_the_words_with_what_stands_around_them() {
        assert_eq!(
            snippet("Fix the Redirect loop", "redirect").as_deref(),
            Some("Fix the Redirect loop")
        );
        let long = format!(
            "{} needle\n\n  in   hay {}",
            "a".repeat(200),
            "b".repeat(200)
        );
        let cut = snippet(&long, "NEEDLE").expect("found");
        assert!(cut.starts_with('…') && cut.ends_with('…'));
        assert!(cut.contains("needle in hay"));
        assert!(cut.chars().count() < 140);
        assert_eq!(snippet("nothing here", "needle"), None);
    }

    #[test]
    fn a_snippet_counts_in_characters_not_bytes() {
        let text = format!("{}ž needle", "é".repeat(100));
        assert!(snippet(&text, "needle").expect("found").ends_with("needle"));
    }

    #[test]
    fn the_pattern_takes_the_query_literally() {
        assert_eq!(pattern("50%_off!"), "%50!%!_off!!%");
        // As the event's JSON holds it: a quote is escaped there too.
        assert_eq!(pattern(r#"say "hi""#), r#"%say \"hi\"%"#);
    }

    #[test]
    fn only_what_somebody_said_counts() {
        let typed = AgentEvent::UserMessage {
            text: "hello".to_string(),
        };
        assert_eq!(said(&typed), Some(("hello", true)));
        let reply = AgentEvent::Text {
            text: "hi".to_string(),
        };
        assert_eq!(said(&reply), Some(("hi", false)));
        assert_eq!(said(&AgentEvent::Closed), None);
    }
}
