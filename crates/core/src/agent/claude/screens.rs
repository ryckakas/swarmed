//! Claude Code's own screens, as it describes them to a host: one control request each.
//!
//! In a terminal `/usage`, `/status` and the like draw a screen. Run as Swarmed runs it, Claude
//! Code has no screen, and answers these requests with the screen's contents instead. They
//! have no specification and some are marked experimental, so every part of an answer is
//! optional here: what is missing, or of another shape, is left out.
//!
//! Only what a screen shows is read. An answer's other parts are not kept, passed on or
//! logged: the definitions of the MCP servers, which hold their secrets, least of all.

use serde_json::{Value, json};

use super::{kind_of, str_of};
use crate::agent::{AgentEvent, ScreenKind, ScreenRow, ScreenSection};

/// What the id of a request for a screen starts with; the screen's command follows.
const REQUEST_PREFIX: &str = "swarmed-screen-";

/// How much of a description is shown under a row.
const NOTE_CHARS: usize = 160;

/// The request that asks Claude Code for the contents of a screen.
pub(super) fn request(kind: ScreenKind) -> Value {
    let mut request = json!({ "subtype": subtype(kind) });
    if kind == ScreenKind::Usage {
        // Without the scan of a week of local transcripts that would come with it otherwise.
        request["skip_behaviors"] = json!(true);
    }
    json!({
        "type": "control_request",
        "request_id": format!("{REQUEST_PREFIX}{}", kind.command()),
        "request": request,
    })
}

fn subtype(kind: ScreenKind) -> &'static str {
    match kind {
        ScreenKind::Usage => "get_usage",
        ScreenKind::Cost => "get_session_cost",
        ScreenKind::Status => "get_status",
        ScreenKind::Context => "get_context_usage",
        ScreenKind::Memory => "get_memory_dialog",
        ScreenKind::Skills => "get_skills_dialog",
        ScreenKind::Hooks => "get_hooks_listing",
        ScreenKind::Mcp => "mcp_status",
    }
}

/// The screen in the answer to a [`request`], if `message` is one. A request Claude Code
/// refuses, as an older one does that has no such screen, is a screen that says so.
pub(super) fn parse(message: &Value) -> Option<AgentEvent> {
    let response = message.get("response")?;
    let command = kind_of(response, "request_id")?.strip_prefix(REQUEST_PREFIX)?;
    let kind = ScreenKind::of(command)?;
    let (mut note, sections) = if kind_of(response, "subtype") == Some("success") {
        contents(kind, response.get("response").unwrap_or(&Value::Null))
    } else {
        let why = kind_of(response, "error").unwrap_or("it gave no reason");
        (format!("Claude Code could not show this: {why}"), vec![])
    };
    if note.is_empty() && sections.is_empty() {
        note = "Claude Code has nothing to show here.".to_string();
    }
    Some(AgentEvent::Screen {
        command: command.to_string(),
        title: kind.title().to_string(),
        note,
        sections,
    })
}

/// A screen's remark and its parts, from the body of Claude Code's answer.
fn contents(kind: ScreenKind, body: &Value) -> (String, Vec<ScreenSection>) {
    match kind {
        ScreenKind::Usage => usage(body),
        ScreenKind::Cost => (String::new(), cost(body)),
        ScreenKind::Status => (String::new(), status(body)),
        ScreenKind::Context => (String::new(), context(body)),
        ScreenKind::Memory => (String::new(), memory(body)),
        ScreenKind::Skills => (String::new(), skills(body)),
        ScreenKind::Hooks => hooks(body),
        ScreenKind::Mcp => mcp(body),
    }
}

fn row(label: impl Into<String>, value: impl Into<String>) -> ScreenRow {
    ScreenRow {
        label: label.into(),
        value: value.into(),
        ..ScreenRow::default()
    }
}

/// The section, unless it has no rows: a heading over nothing is not shown.
fn section(title: &str, rows: Vec<ScreenRow>) -> Option<ScreenSection> {
    (!rows.is_empty()).then(|| ScreenSection {
        title: title.to_string(),
        rows,
    })
}

/// The items of the array under `key`; none if there is no such array.
fn list<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    let items = value.get(key).and_then(Value::as_array);
    items.into_iter().flatten()
}

/// A percentage as a whole number from 0 to 100, whatever kind of number it came as.
pub(super) fn percent(value: Option<&Value>) -> Option<u8> {
    let share = value?.as_f64()?.clamp(0.0, 100.0);
    format!("{share:.0}").parse().ok()
}

/// How many percent `part` is of `whole`, at most 100.
fn percent_of(part: u64, whole: u64) -> Option<u8> {
    let share = part.saturating_mul(100).checked_div(whole)?;
    u8::try_from(share.min(100)).ok()
}

/// `text`, cut to what fits under a row.
fn clipped(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= NOTE_CHARS {
        return text.to_string();
    }
    let kept: String = text.chars().take(NOTE_CHARS - 1).collect();
    format!("{}…", kept.trim_end())
}

fn tokens(count: u64) -> String {
    match count {
        1_000_000.. => format!("{}.{}m", count / 1_000_000, count % 1_000_000 / 100_000),
        1000.. => format!("{}.{}k", count / 1000, count % 1000 / 100),
        _ => count.to_string(),
    }
}

fn duration(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    match (seconds / 3600, seconds / 60 % 60, seconds % 60) {
        (0, 0, seconds) => format!("{seconds}s"),
        (0, minutes, seconds) => format!("{minutes}m {seconds}s"),
        (hours, minutes, _) => format!("{hours}h {minutes}m"),
    }
}

/// An amount in minor units, such as cents, written with its decimal point.
fn money(minor: u64, digits: u32, currency: &str) -> String {
    let unit = 10_u64.pow(digits);
    let width = usize::try_from(digits).unwrap_or_default();
    let amount = match digits {
        0 => minor.to_string(),
        _ => format!("{}.{:0width$}", minor / unit, minor % unit),
    };
    format!("{amount} {currency}").trim().to_string()
}

/// The plan's limits, extra usage and what the session has taken.
fn usage(body: &Value) -> (String, Vec<ScreenSection>) {
    let limits = body.get("rate_limits").unwrap_or(&Value::Null);
    let sections = [
        section(
            "Plan limits",
            list(limits, "limits").filter_map(limit_row).collect(),
        ),
        section(
            "Extra usage",
            extra_usage(limits.get("extra_usage")).into_iter().collect(),
        ),
        section("This session", session_rows(body.get("session"))),
    ];
    // The plan's name is in the answer too, and left there: a screen is stored.
    let credits = limits.get("extra_usage").is_some_and(Value::is_object);
    let note = if credits { CREDITS } else { "" };
    (note.to_string(), sections.into_iter().flatten().collect())
}

/// What the terminal's `/usage-credits` does that Swarmed leaves to it: it handles no billing.
const CREDITS: &str = "To buy usage credits or change their monthly cap, run /usage-credits in Claude Code's own \
     terminal.";

fn limit_row(limit: &Value) -> Option<ScreenRow> {
    let share = percent(limit.get("percent"))?;
    let model = limit
        .pointer("/scope/model/display_name")
        .and_then(Value::as_str);
    let label = match (kind_of(limit, "kind")?, model) {
        ("session", _) => "Current session".to_string(),
        ("weekly_all", _) => "Current week, all models".to_string(),
        ("weekly_scoped", Some(model)) => format!("Current week, {model}"),
        (other, _) => other.replace('_', " "),
    };
    Some(ScreenRow {
        label,
        value: format!("{share}% used"),
        share: Some(share),
        resets_at: kind_of(limit, "resets_at").map(String::from),
        note: String::new(),
    })
}

/// What was spent beyond the plan this month, against the monthly cap.
fn extra_usage(extra: Option<&Value>) -> Option<ScreenRow> {
    let extra = extra.filter(|extra| extra.is_object())?;
    let flag = |name: &str| extra.get(name).and_then(Value::as_bool) == Some(true);
    if !flag("is_enabled") {
        let mut off = row("This month", "Off");
        off.note = "Requests stop at the plan's limits.".to_string();
        return Some(off);
    }
    let digits = extra.get("decimal_places").and_then(Value::as_u64);
    // More digits than any currency has would not fit the arithmetic.
    let digits = u32::try_from(digits.unwrap_or(2).min(6)).unwrap_or(2);
    let amount = |name: &str| {
        let minor = extra.get(name).and_then(Value::as_u64)?;
        Some(money(minor, digits, &str_of(extra, "currency")))
    };
    let used = amount("used_credits")?;
    let mut spent = row(
        "This month",
        amount("monthly_limit").map_or_else(|| used.clone(), |cap| format!("{used} of {cap}")),
    );
    spent.share = percent(extra.get("utilization"));
    if flag("spend_limit_reached") {
        spent.note = "The cap is reached: extra usage pays for nothing more.".to_string();
    }
    Some(spent)
}

fn session_rows(session: Option<&Value>) -> Vec<ScreenRow> {
    let Some(session) = session else {
        return vec![];
    };
    let number = |name: &str| session.get(name).and_then(Value::as_u64);
    let cost = session.get("total_cost_usd").and_then(Value::as_f64);
    let lines = number("total_lines_added").zip(number("total_lines_removed"));
    [
        cost.map(|cost| row("Cost", format!("${cost:.2}"))),
        number("total_api_duration_ms").map(|time| row("Waiting for the model", duration(time))),
        number("total_duration_ms").map(|time| row("Since it started", duration(time))),
        lines.map(|(added, removed)| {
            row("Lines changed", format!("{added} added, {removed} removed"))
        }),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// What the session has cost, which Claude Code gives as lines of text: a name, a colon and
/// a figure on most of them.
fn cost(body: &Value) -> Vec<ScreenSection> {
    let text = str_of(body, "text");
    let lines = text.lines().filter(|line| !line.trim().is_empty());
    let rows = lines.map(|line| match line.split_once(':') {
        Some((label, value)) if !value.trim().is_empty() => row(label.trim(), value.trim()),
        _ => row("", line.trim()),
    });
    section("", rows.collect()).into_iter().collect()
}

/// The status screen, which Claude Code already gives as headed rows.
fn status(body: &Value) -> Vec<ScreenSection> {
    let rows = |part: &Value| {
        list(part, "rows")
            .map(|line| row(str_of(line, "label"), str_of(line, "value")))
            .collect()
    };
    list(body, "sections")
        .filter_map(|part| section(&str_of(part, "title"), rows(part)))
        .collect()
}

/// How full the context window is, and what fills it.
fn context(body: &Value) -> Vec<ScreenSection> {
    let number = |name: &str| body.get(name).and_then(Value::as_u64);
    let max = number("maxTokens").unwrap_or(0);
    let measured = |label: &str, count: u64, value: String| ScreenRow {
        share: percent_of(count, max),
        ..row(label, value)
    };
    let total = number("totalTokens").map(|used| {
        let value = format!("{} of {} tokens", tokens(used), tokens(max));
        measured("In use", used, value)
    });
    let parts = list(body, "categories").filter_map(|part| {
        let count = part.get("tokens").and_then(Value::as_u64)?;
        let shown = count > 0 && kind_of(part, "kind") != Some("free");
        shown.then(|| {
            measured(
                &str_of(part, "name"),
                count,
                format!("{} tokens", tokens(count)),
            )
        })
    });
    [
        section("", total.into_iter().collect()),
        section("What fills it", parts.collect()),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The instruction files Claude Code reads, and where it keeps what it remembers.
fn memory(body: &Value) -> Vec<ScreenSection> {
    let file = |file: &Value| {
        let missing = file.get("exists").and_then(Value::as_bool) == Some(false);
        let about = str_of(file, "description");
        let note = match (missing, about.is_empty()) {
            (true, true) => "Not there yet.".to_string(),
            (true, false) => format!("{about}. Not there yet."),
            (false, _) => about,
        };
        ScreenRow {
            note,
            ..row(str_of(file, "label"), str_of(file, "path"))
        }
    };
    let folder = |folder: &Value| row(str_of(folder, "label"), str_of(folder, "path"));
    let auto = body.pointer("/auto_memory/status").and_then(Value::as_str);
    [
        section("Instruction files", list(body, "files").map(file).collect()),
        section("Folders", list(body, "folders").map(folder).collect()),
        section(
            "Its own memory",
            auto.map(|status| row("Auto-memory", status))
                .into_iter()
                .collect(),
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The skills Claude Code has, and what each takes of the context window.
fn skills(body: &Value) -> Vec<ScreenSection> {
    let skill = |skill: &Value| {
        let shown = kind_of(skill, "display_name").filter(|name| !name.is_empty());
        let name = shown.map_or_else(|| str_of(skill, "name"), String::from);
        let size = skill.get("tokens").and_then(Value::as_u64);
        let value = match (kind_of(skill, "state"), size) {
            (Some("on") | None, Some(size)) => format!("{} tokens", tokens(size)),
            (state, _) => state.unwrap_or_default().to_string(),
        };
        let about = [str_of(skill, "source"), str_of(skill, "description")];
        let about = about.iter().filter(|part| !part.is_empty());
        ScreenRow {
            note: clipped(&about.cloned().collect::<Vec<_>>().join(": ")),
            ..row(name, value)
        }
    };
    let rows: Vec<ScreenRow> = list(body, "skills").map(skill).collect();
    let title = format!("{} skills", rows.len());
    section(&title, rows).into_iter().collect()
}

/// The hooks that run around Claude Code's tools, under the event each runs on.
fn hooks(body: &Value) -> (String, Vec<ScreenSection>) {
    let mut sections: Vec<ScreenSection> = vec![];
    for hook in list(body, "hooks") {
        let matcher = kind_of(hook, "matcher").filter(|matcher| !matcher.is_empty());
        let line = ScreenRow {
            note: str_of(hook, "sourceLabel"),
            ..row(matcher.unwrap_or("Every time"), str_of(hook, "displayText"))
        };
        let event = str_of(hook, "event");
        match sections.iter_mut().find(|known| known.title == event) {
            Some(known) => known.rows.push(line),
            None => sections.push(ScreenSection {
                title: event,
                rows: vec![line],
            }),
        }
    }
    let none = if sections.is_empty() {
        "No hooks are set."
    } else {
        ""
    };
    (none.to_string(), sections)
}

/// The MCP servers by name, and whether each connected. Their definitions come with the
/// answer and are not read: they hold the servers' secrets.
fn mcp(body: &Value) -> (String, Vec<ScreenSection>) {
    let server = |server: &Value| {
        let about = [str_of(server, "transport"), str_of(server, "scope")];
        let about = about.iter().filter(|part| !part.is_empty());
        ScreenRow {
            note: about.cloned().collect::<Vec<_>>().join(", "),
            ..row(
                str_of(server, "name"),
                str_of(server, "status").replace('-', " "),
            )
        }
    };
    let rows: Vec<ScreenRow> = list(body, "mcpServers").map(server).collect();
    if rows.is_empty() {
        return ("Claude Code has no MCP servers.".to_string(), vec![]);
    }
    let how = "Reconnect, enable or disable one with /mcp reconnect, /mcp enable or /mcp \
               disable and its name.";
    let title = format!("{} servers", rows.len());
    (how.to_string(), section(&title, rows).into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claude Code's answer to the request for `kind`, with `body` in it.
    fn answer(kind: ScreenKind, body: &Value) -> Value {
        json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": format!("{REQUEST_PREFIX}{}", kind.command()),
                "response": body,
            },
        })
    }

    fn screen(kind: ScreenKind, body: &Value) -> (String, Vec<ScreenSection>) {
        match parse(&answer(kind, body)).expect("a screen") {
            AgentEvent::Screen { note, sections, .. } => (note, sections),
            other => panic!("not a screen: {other:?}"),
        }
    }

    #[test]
    fn every_screen_has_a_request_of_its_own_and_usage_skips_the_transcripts() {
        let subtypes: Vec<String> = ScreenKind::ALL
            .into_iter()
            .map(|kind| str_of(&request(kind)["request"], "subtype"))
            .collect();
        let mut distinct = subtypes.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), ScreenKind::ALL.len());
        assert!(subtypes.contains(&"get_status".to_string()));
        let usage = request(ScreenKind::Usage);
        assert_eq!(usage["request"]["skip_behaviors"], json!(true));
        assert_eq!(usage["request_id"], json!("swarmed-screen-usage"));
        assert!(
            request(ScreenKind::Status)["request"]
                .get("skip_behaviors")
                .is_none()
        );
    }

    #[test]
    fn usage_shows_the_plans_windows_extra_usage_and_the_session() {
        // The shape Claude Code 2.1.330 answers with, cut to what is read.
        let body = json!({
            "session": {
                "total_cost_usd": 1.239, "total_api_duration_ms": 65_000,
                "total_duration_ms": 3_725_000, "total_lines_added": 12, "total_lines_removed": 3,
            },
            "subscription_type": "team",
            "rate_limits": {
                "limits": [
                    { "kind": "session", "percent": 40, "resets_at": "2026-11-21T19:59:59+00:00" },
                    { "kind": "weekly_all", "percent": 31, "resets_at": null },
                    { "kind": "weekly_scoped", "percent": 46.4,
                      "scope": { "model": { "display_name": "Fable" } } },
                    { "kind": "something_new", "percent": 7 },
                    { "kind": "no_figure" },
                ],
                "extra_usage": {
                    "is_enabled": true, "monthly_limit": 40000, "used_credits": 3871,
                    "utilization": 9.6775, "currency": "EUR", "decimal_places": 2,
                    "spend_limit_reached": false,
                },
            },
        });
        let (note, sections) = screen(ScreenKind::Usage, &body);
        // The plan's name is not kept, and a screen is. What only the terminal does is named.
        assert_eq!(note, CREDITS);
        let titles: Vec<&str> = sections.iter().map(|part| part.title.as_str()).collect();
        assert_eq!(titles, ["Plan limits", "Extra usage", "This session"]);
        let limits = &sections[0].rows;
        let said: Vec<(&str, &str)> = limits
            .iter()
            .map(|line| (line.label.as_str(), line.value.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                ("Current session", "40% used"),
                ("Current week, all models", "31% used"),
                ("Current week, Fable", "46% used"),
                ("something new", "7% used"),
            ]
        );
        assert_eq!(limits[0].share, Some(40));
        assert_eq!(
            limits[0].resets_at.as_deref(),
            Some("2026-11-21T19:59:59+00:00")
        );
        assert_eq!(limits[1].resets_at, None);
        let extra = &sections[1].rows[0];
        assert_eq!(extra.value, "38.71 EUR of 400.00 EUR");
        assert_eq!((extra.share, extra.note.as_str()), (Some(10), ""));
        let session: Vec<&str> = sections[2]
            .rows
            .iter()
            .map(|line| line.value.as_str())
            .collect();
        assert_eq!(session, ["$1.24", "1m 5s", "1h 2m", "12 added, 3 removed"]);
    }

    #[test]
    fn extra_usage_says_when_it_is_off_or_its_cap_is_reached_and_an_api_key_has_no_limits() {
        let off = json!({ "rate_limits": { "extra_usage": { "is_enabled": false } } });
        let (_, sections) = screen(ScreenKind::Usage, &off);
        let line = &sections[0].rows[0];
        assert_eq!(
            (line.value.as_str(), line.note.as_str()),
            ("Off", "Requests stop at the plan's limits.")
        );

        let reached = json!({ "rate_limits": { "extra_usage": {
            "is_enabled": true, "used_credits": 500, "monthly_limit": null, "currency": "JPY",
            "decimal_places": 0, "spend_limit_reached": true,
        } } });
        let (_, sections) = screen(ScreenKind::Usage, &reached);
        let line = &sections[0].rows[0];
        assert_eq!(line.value, "500 JPY");
        assert!(line.note.starts_with("The cap is reached"));

        // Billed by an API key: no plan, so no limits. The session's own figures remain.
        let keyed = json!({ "session": { "total_cost_usd": 0.5 }, "rate_limits": null });
        let (note, sections) = screen(ScreenKind::Usage, &keyed);
        assert_eq!(note, "");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "This session");
    }

    #[test]
    fn status_and_cost_keep_claude_codes_own_rows() {
        let body = json!({ "sections": [
            { "title": "Session", "rows": [
                { "label": "Version", "value": "2.1.330" },
                { "value": "A remark with no label." },
            ] },
            { "title": "Empty", "rows": [] },
        ] });
        let (_, sections) = screen(ScreenKind::Status, &body);
        assert_eq!(sections.len(), 1);
        assert_eq!(
            sections[0].rows,
            [
                row("Version", "2.1.330"),
                row("", "A remark with no label.")
            ]
        );

        let text = "Estimated cost:        $0.0000\nTotal duration (API):  12s\n\nUsage by model:\n    haiku:  1.2k input";
        let (_, sections) = screen(ScreenKind::Cost, &json!({ "text": text }));
        assert_eq!(
            sections[0].rows,
            [
                row("Estimated cost", "$0.0000"),
                row("Total duration (API)", "12s"),
                row("", "Usage by model:"),
                row("haiku", "1.2k input"),
            ]
        );
    }

    #[test]
    fn context_measures_each_part_against_the_window() {
        let body = json!({
            "totalTokens": 250_000, "maxTokens": 1_000_000,
            "categories": [
                { "name": "System prompt", "tokens": 3972, "kind": "used" },
                { "name": "Skills", "tokens": 0, "kind": "used" },
                { "name": "Free space", "tokens": 750_000, "kind": "free" },
                { "name": "Tools (deferred)", "tokens": 22_562, "kind": "deferred" },
            ],
        });
        let (_, sections) = screen(ScreenKind::Context, &body);
        let total = &sections[0].rows[0];
        assert_eq!(total.value, "250.0k of 1.0m tokens");
        assert_eq!(total.share, Some(25));
        let parts: Vec<(&str, &str, Option<u8>)> = sections[1]
            .rows
            .iter()
            .map(|line| (line.label.as_str(), line.value.as_str(), line.share))
            .collect();
        assert_eq!(
            parts,
            [
                ("System prompt", "3.9k tokens", Some(0)),
                ("Tools (deferred)", "22.5k tokens", Some(2)),
            ]
        );
        // A window of no size is measured against nothing.
        let (_, sections) = screen(ScreenKind::Context, &json!({ "totalTokens": 5 }));
        assert_eq!(sections[0].rows[0].share, None);
    }

    #[test]
    fn memory_skills_and_hooks_are_listed_as_claude_code_names_them() {
        let body = json!({
            "files": [
                { "label": "User instructions", "path": "/home/a/.claude/CLAUDE.md",
                  "description": "Saved in ~/.claude/CLAUDE.md", "exists": true },
                { "label": "Project instructions", "path": "/w/CLAUDE.md",
                  "description": "Saved in ./CLAUDE.md", "exists": false },
            ],
            "folders": [{ "label": "Open auto-memory folder", "path": "/home/a/memory" }],
            "auto_memory": { "enabled": true, "status": "on" },
        });
        let (_, sections) = screen(ScreenKind::Memory, &body);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].rows[0].note, "Saved in ~/.claude/CLAUDE.md");
        assert_eq!(
            sections[0].rows[1].note,
            "Saved in ./CLAUDE.md. Not there yet."
        );
        assert_eq!(sections[2].rows, [row("Auto-memory", "on")]);

        let long = "x".repeat(400);
        let body = json!({ "skills": [
            { "name": "a:review", "display_name": "review", "description": long,
              "source": "plugin", "tokens": 1830, "state": "on" },
            { "name": "quiet", "display_name": "", "description": "", "source": "",
              "tokens": 10, "state": "off" },
        ] });
        let (_, sections) = screen(ScreenKind::Skills, &body);
        assert_eq!(sections[0].title, "2 skills");
        let first = &sections[0].rows[0];
        assert_eq!(
            (first.label.as_str(), first.value.as_str()),
            ("review", "1.8k tokens")
        );
        assert_eq!(first.note.chars().count(), NOTE_CHARS);
        assert!(first.note.starts_with("plugin: xxx") && first.note.ends_with('…'));
        assert_eq!(sections[0].rows[1], row("quiet", "off"));

        let body = json!({ "hooks": [
            { "event": "PreToolUse", "matcher": "Bash", "displayText": "~/hooks/gate",
              "sourceLabel": "User settings" },
            { "event": "SessionStart", "matcher": "", "displayText": "~/hooks/hello" },
            { "event": "PreToolUse", "matcher": "Edit", "displayText": "~/hooks/lint" },
        ] });
        let (note, sections) = screen(ScreenKind::Hooks, &body);
        assert_eq!(note, "");
        let events: Vec<(&str, usize)> = sections
            .iter()
            .map(|part| (part.title.as_str(), part.rows.len()))
            .collect();
        assert_eq!(events, [("PreToolUse", 2), ("SessionStart", 1)]);
        assert_eq!(sections[1].rows[0].label, "Every time");
        let (note, sections) = screen(ScreenKind::Hooks, &json!({ "hooks": [] }));
        assert_eq!((note.as_str(), sections.len()), ("No hooks are set.", 0));
    }

    #[test]
    fn mcp_servers_are_shown_by_name_and_state_and_never_with_their_definitions() {
        let body = json!({ "mcpServers": [
            { "name": "docs", "status": "needs-auth", "transport": "http", "scope": "user",
              "config": { "type": "http", "url": "https://example.test/mcp",
                          "headers": { "KEY": "made-up-secret" } } }, // gitleaks:allow
            { "name": "local", "status": "connected", "transport": "stdio", "scope": "project",
              "config": { "command": "/bin/server", "args": ["--token", "made-up-token"] } },
        ] });
        let event = parse(&answer(ScreenKind::Mcp, &body)).expect("a screen");
        let kept = serde_json::to_string(&event).expect("serialises");
        for part in [
            "made-up-secret",
            "made-up-token",
            "example.test",
            "/bin/server",
        ] {
            assert!(!kept.contains(part), "{part} of a definition was kept");
        }
        let (note, sections) = screen(ScreenKind::Mcp, &body);
        assert!(note.contains("/mcp reconnect"));
        assert_eq!(sections[0].title, "2 servers");
        let first = &sections[0].rows[0];
        assert_eq!(
            (
                first.label.as_str(),
                first.value.as_str(),
                first.note.as_str()
            ),
            ("docs", "needs auth", "http, user")
        );
        let (note, sections) = screen(ScreenKind::Mcp, &json!({ "mcpServers": [] }));
        assert_eq!(
            (note.as_str(), sections.len()),
            ("Claude Code has no MCP servers.", 0)
        );
    }

    #[test]
    fn a_refused_request_is_a_screen_that_says_so_and_other_answers_are_none() {
        let refused = json!({ "type": "control_response", "response": {
            "subtype": "error", "request_id": "swarmed-screen-status",
            "error": "Unsupported control request subtype: get_status",
        } });
        let event = parse(&refused).expect("a screen");
        assert_eq!(
            event,
            AgentEvent::Screen {
                command: "status".to_string(),
                title: "Status".to_string(),
                note: "Claude Code could not show this: Unsupported control request subtype: \
                       get_status"
                    .to_string(),
                sections: vec![],
            }
        );
        // An answer with nothing in it that can be read still says something.
        let (note, sections) = screen(ScreenKind::Status, &json!({ "unknown": true }));
        assert_eq!(
            (note.as_str(), sections.len()),
            ("Claude Code has nothing to show here.", 0)
        );
        // Answers to other requests, and to screens there are none of, are not screens.
        let other = |id: &str| {
            json!({ "type": "control_response", "response": {
                "subtype": "success", "request_id": id, "response": {},
            } })
        };
        assert_eq!(parse(&other("swarmed-context")), None);
        assert_eq!(parse(&other("swarmed-screen-nonsense")), None);
        assert_eq!(parse(&json!({ "type": "control_response" })), None);
    }
}
