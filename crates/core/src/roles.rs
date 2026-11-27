//! What a session is for: its role. A role is data (a name, what it is for, the rules its agent
//! keeps to, the supervision it starts in, whether it changes files, which roles it may start
//! and which it starts unless told otherwise), so that roles of the user's own can come later.
//! Four are built in: architect, planner, implementer and reviewer. A session with no role
//! works as the user asks, as every session did before.
//!
//! The user chooses a role when a session starts and can change it while it runs. Each change
//! once something was said is recorded as [`AgentEvent::RoleChanged`], so the transcript shows
//! where the new rules start, and the agent is told with its next message. Every start of the
//! agent has the role in its guidance ([`section`]), with what each of the other roles is for
//! and who works with it.
//!
//! Swarmed's session tools are a role's work, so a session with a role uses them without the
//! user being asked ([`uses_unasked`]), in plan mode too: Claude Code holds every MCP tool that
//! is not read-only at a question there, whatever its allow rules say.

use serde::Serialize;

use crate::Core;
use crate::agent::{AgentEvent, Supervision};
use crate::error::{Error, Result};
use crate::parents::{self, Relations};
use crate::sessions::Session;

/// A role a session can have: what its agent is for, and what it keeps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Role {
    /// Its name, which the session keeps: `architect` and the like.
    pub name: &'static str,
    /// What it is for, in one line.
    pub description: &'static str,
    /// The rules its agent keeps to, in its guidance.
    #[serde(skip)]
    pub instructions: &'static str,
    /// The supervision it starts in; `None` for what was chosen for the session.
    pub supervision: Option<Supervision>,
    /// The supervision it goes on in once the user approves a plan it presented, for a role
    /// that changes no files and hands its plan on to sessions it starts and follows: asked
    /// about each step, that would be the user's work. `None` to stay in plan mode.
    pub approved_supervision: Option<Supervision>,
    /// Whether its agent changes files.
    pub changes_files: bool,
    /// The roles of the sessions it may start; none for a role that starts no sessions.
    pub starts: &'static [&'static str],
    /// The role of a session it starts without naming one.
    pub child: Option<&'static str>,
    /// What its agent goes on with once the user approves a plan it presented, for a role that
    /// changes no files: it is told this instead of the usual go-ahead, which would have it
    /// carry the plan out itself. `None` for a role that changes files, which goes on to carry
    /// its plan out.
    #[serde(skip)]
    pub approved: Option<&'static str>,
}

/// The architect: over a whole feature, across tasks.
const ARCHITECT: Role = Role {
    name: "architect",
    description: "shapes a whole feature across tasks: splits it into parts, each one task, one \
                  branch and one pull request, and starts a planner for each",
    instructions: "You decide the shape of the whole: the boundaries between the parts, the data \
model, the interfaces between them, the trade-offs, the order they land in, and which pull \
request stacks on which. Split the work into parts that each can be one task, one branch and one \
pull request. Starting a task for each part is your job, so once the user asked you for the \
work, start them with `start_task` without asking again: each with a planner in it (its `role` \
is `planner` unless you name another), told its part, what is settled about it, and the branch \
it starts from (`from`) when it stacks on another part. Then follow them: answer what they ask, \
check that the parts fit together and land in order, and report to the user. You change no \
files and run nothing that changes anything: each part is its planner's to plan and its \
implementers' to build.",
    supervision: Some(Supervision::Plan),
    approved_supervision: Some(Supervision::Auto),
    changes_files: false,
    starts: &["planner", "reviewer"],
    child: Some("planner"),
    approved: Some(
        "Start a task for each part now with `start_task`, each with a planner in it, and follow \
         them as your role says.",
    ),
};

/// The planner: over one part, inside one task.
const PLANNER: Role = Role {
    name: "planner",
    description: "plans one part inside one task, hands every change to implementers in its \
                  worktree, and checks what they report against the plan",
    instructions: "The design of your part is settled: turn it into concrete steps, which files \
change, which tests show it works, and in what order. Hand every change to an implementer with \
`spawn_session` (its `role` is `implementer` unless you name another): they share this worktree, \
so give each a part that does not touch the same files as another's. Check what each reports \
back against the plan, have a reviewer read the whole when it is worth one, and report to the \
session you work for, or to the user if you work for none. You change no files, run nothing that \
changes anything, and do not implement through sub-agents of your own either: a change you would \
make, an implementer makes. If the design does not hold, say so to whom you report rather than \
change it alone.",
    supervision: Some(Supervision::Plan),
    approved_supervision: Some(Supervision::Auto),
    changes_files: false,
    starts: &["implementer", "reviewer"],
    child: Some("implementer"),
    approved: Some(
        "Hand each change to an implementer now with `spawn_session`, in parts that do not touch \
         the same files, and check what each reports back against the plan.",
    ),
};

/// The implementer: builds what it was given.
const IMPLEMENTER: Role = Role {
    name: "implementer",
    description: "builds the part it was given and nothing beside it, and reports back when done \
                  or stuck",
    instructions: "Do the part you were given and nothing beside it: a change you find is needed \
elsewhere, you suggest to the session you work for. When you are done, report what you did and \
found to it with `send_message`. When you are stuck, ask it first, and the user only when it \
cannot decide. You start no sessions and no tasks.",
    supervision: None,
    approved_supervision: None,
    changes_files: true,
    starts: &[],
    child: None,
    approved: None,
};

/// The reviewer: reads, and changes nothing.
const REVIEWER: Role = Role {
    name: "reviewer",
    description: "reads and reviews what it is pointed at, changes nothing itself, and reports \
                  its findings or hands their fixes to implementers",
    instructions: "Read and review what you were asked to: whether it is correct, whether it does \
what was asked, and whether it reads clearly. You change no files and run nothing that changes \
anything; running tests and checks is fine. Report your findings to the session you work for \
with `send_message`, or to the user if you work for none: each with where it is and why it \
matters, the most severe first. When you were asked to get them fixed as well, hand the fixes to \
implementers with `spawn_session` (its `role` is `implementer`), in parts that do not touch the \
same files, check what each reports back, and then report what was found and fixed. Start no \
tasks.",
    supervision: Some(Supervision::Plan),
    approved_supervision: None,
    changes_files: false,
    starts: &["implementer"],
    child: Some("implementer"),
    approved: Some(
        "Report your findings now to the session you work for with `send_message`, or to the \
         user if you work for none, or hand their fixes to implementers with `spawn_session` if \
         you were asked to get them fixed.",
    ),
};

/// The roles built in, in the order they are offered.
pub const ROLES: [Role; 4] = [ARCHITECT, PLANNER, IMPLEMENTER, REVIEWER];

/// How an architect and a planner are told apart, as every agent with a role is told and
/// `docs/coding-agents.md` shows it.
pub(crate) const TABLE: &str = "\
| | Architect | Planner |
| --- | --- | --- |
| Works on | the whole feature, across tasks | one part, inside one task |
| Decides | the shape: boundaries, data model, interfaces between parts, trade-offs, which pull request stacks on which | the steps: which files, which tests, in what order |
| Hands work to | planners, each in a task of its own (`start_task`) | implementers in its own worktree (`spawn_session`) |
| Checks | that the parts fit together and land in order | that each implementer's report matches the plan |
| Reports to | the user | its architect, or the user |";

/// The role called `name`, if there is one.
#[must_use]
pub(crate) fn find(name: &str) -> Option<&'static Role> {
    ROLES.iter().find(|role| role.name == name)
}

/// The role called `name`, or an error that names the roles there are.
pub(crate) fn known(name: &str) -> Result<&'static Role> {
    find(name).ok_or_else(|| {
        let names: Vec<&str> = ROLES.iter().map(|role| role.name).collect();
        Error::UnknownRole {
            name: name.to_string(),
            known: names.join(", "),
        }
    })
}

/// What every role is for, and how an architect and a planner are told apart.
fn overview() -> String {
    let roles: Vec<String> = ROLES
        .iter()
        .map(|role| format!("- `{}`: {}.", role.name, role.description))
        .collect();
    format!(
        "## The roles\n\nThe user gives a session one of these roles, or none:\n{}\nA session \
         with no role works as the user asks. Start each piece of work with the role it \
         belongs to, and when work belongs to another role than yours, hand it on rather than \
         do it. An architect and a planner are told apart by level:\n\n{TABLE}",
        roles.join("\n")
    )
}

/// What a session with `role` may start, in a sentence for its agent.
fn starting(role: &Role) -> String {
    match (role.starts, role.child) {
        ([], _) => "You start no sessions.".to_string(),
        (starts, child) => {
            let names: Vec<String> = starts.iter().map(|name| format!("`{name}`")).collect();
            let unless = child.map_or_else(String::new, |child| {
                format!(", and one you start without naming a role is given `{child}`")
            });
            format!(
                "The sessions you start have a role: {}{unless}.",
                names.join(" or ")
            )
        }
    }
}

/// `name` with the article it takes at the start of a sentence: "An architect", "A planner".
fn with_article(name: &str) -> String {
    let vowel = name.starts_with(['a', 'e', 'i', 'o', 'u']);
    format!("{} {name}", if vowel { "An" } else { "A" })
}

/// How a role that changes no files is kept to it, as its agent is told.
const KEPT_FROM_CHANGES: &str = "Swarmed keeps you to that: your role starts you in plan mode, or \
in your agent's read-only mode where it has no plan mode, either of which keeps you from changing \
files. Swarmed's session tools (`list_sessions`, `read_session`, `send_message`, `spawn_session`, \
`start_task`, `close_session`) change none of your files and are your role's work, so they are \
yours to use in that mode, even where it says to run nothing that changes anything, and the user \
is not asked about them. When the user approves a plan you present, you are told how to hand it \
on: an approval is never a go-ahead to change anything yourself.";

/// Swarmed's session tools, as Claude Code names them: by server, then by tool. Each is a
/// role's work: seeing and reading the others, telling them, starting them and closing them.
/// What a role may start is checked when it starts one, not here.
const SESSION_TOOLS: [&str; 6] = [
    "mcp__swarmed__list_sessions",
    "mcp__swarmed__read_session",
    "mcp__swarmed__send_message",
    "mcp__swarmed__spawn_session",
    "mcp__swarmed__start_task",
    "mcp__swarmed__close_session",
];

/// The rules of `role`, as its agent keeps to them: what it is, its own rules, how Swarmed keeps
/// it from changing files if it changes none, and what it may start, then what every role is
/// for.
fn rules(role: &Role) -> String {
    let opening = format!(
        "Your role in this session is {}. {} {}.",
        role.name,
        with_article(role.name),
        role.description
    );
    let kept = (!role.changes_files).then(|| KEPT_FROM_CHANGES.to_string());
    let parts = [
        Some(opening),
        Some(role.instructions.to_string()),
        kept,
        Some(starting(role)),
        Some(overview()),
    ];
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    parts.join("\n\n")
}

/// What the agent of a session with `role` is told when the user approves a plan it presented,
/// in place of the usual go-ahead: for a role that changes no files, that it still changes
/// none, the mode it goes on in, and how it hands the plan on. `None` for a role that changes
/// files, which goes on to carry its plan out.
pub(crate) fn approval(role: &Role) -> Option<String> {
    let next = role.approved?;
    let mode = if role.approved_supervision.is_some() {
        "You are out of plan mode now, so that starting and following the sessions that carry it \
         out needs no approval of each step; you still change nothing yourself."
    } else {
        "You stay in the mode that keeps you from it."
    };
    Some(format!(
        "The user approved your plan. Your role, the {}'s, changes no files: this is no go-ahead \
         to change anything yourself. {mode} {next}",
        role.name
    ))
}

/// Whether the agent of a session with `role` may call `tool_name` without the user being
/// asked: one of Swarmed's session tools, for a session with a role, whose work they are. A
/// session with no role is asked about them as before.
pub(crate) fn uses_unasked(role: Option<&Role>, tool_name: &str) -> bool {
    role.is_some() && SESSION_TOOLS.contains(&tool_name)
}

/// The supervision a session supervised as `now` has once its role is `role`, and what to keep
/// as the supervision its role took the place of, given `before`, what an earlier role took the
/// place of if one did: a role that holds a supervision puts the session in it and remembers
/// what it took the place of, the first it did; one that holds none puts that back, and
/// remembers nothing.
pub(crate) fn held(
    role: Option<&Role>,
    now: Supervision,
    before: Option<Supervision>,
) -> (Supervision, Option<Supervision>) {
    match role.and_then(|role| role.supervision) {
        Some(own) if own == now => (own, before),
        Some(own) => (own, Some(before.unwrap_or(now))),
        None => (before.unwrap_or(now), None),
    }
}

/// The supervision a new session with `role` starts in when it was asked to start as `asked`,
/// and what to put back when its role is taken off: the role's own if it holds one.
pub(crate) fn starts_in(
    role: Option<&Role>,
    asked: Supervision,
) -> (Supervision, Option<Supervision>) {
    held(role, asked, None)
}

/// What the agent of a session with `role` is told at its start of its role and of who works
/// with it, which `relations` says where the session can reach the others.
pub(crate) fn instructions(role: &Role, relations: Option<&Relations>) -> String {
    let heading = format!("# Your role: {}", role.name);
    let team = relations.and_then(parents::sentences);
    let team = team.map(|said| format!("## Who you work with\n\n{said}"));
    let parts = [Some(heading), Some(rules(role)), team];
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    parts.join("\n\n")
}

/// What the agent of a session is told at its start of what it is for and who works with it:
/// its role's section with `role`, else who works with it, if anyone does. `relations` only
/// where the session can reach the others.
pub(crate) fn section(role: Option<&Role>, relations: Option<&Relations>) -> Option<String> {
    match role {
        Some(role) => Some(instructions(role, relations)),
        None => relations.and_then(parents::guidance),
    }
}

/// What an agent is told along with its next message when its role changed from `previous` to
/// `now` since its last: the new role and its rules, and that the old one's no longer apply.
pub(crate) fn note(now: Option<&Role>, previous: Option<&str>) -> String {
    let before = previous.map_or_else(
        || "Until now you had no role, and worked as the user asked.".to_string(),
        |name| format!("The rules of your role until now, the {name}'s, no longer apply."),
    );
    let after = now.map_or_else(
        || "You have no role now: work as the user asks.".to_string(),
        |role| format!("From this message on, keep to these:\n\n{}", rules(role)),
    );
    format!("<role>\nThe user changed your role in this session. {before} {after}\n</role>")
}

/// The latest change of role since the last message its agent was sent, by `events`, its
/// history so far, as the role now and the role it had then: the agent is told it with the
/// next message. A change before the first message is not: the agent starts with that role in
/// its guidance.
pub(crate) fn changed(events: &[AgentEvent]) -> Option<(Option<String>, Option<String>)> {
    let last = events
        .iter()
        .rposition(|event| matches!(event, AgentEvent::UserMessage { .. }))?;
    let changes: Vec<(&Option<String>, &Option<String>)> = events[last + 1..]
        .iter()
        .filter_map(|event| match event {
            AgentEvent::RoleChanged { role, previous } => Some((role, previous)),
            _ => None,
        })
        .collect();
    let (_, then) = changes.first()?;
    let (now, _) = changes.last()?;
    (now != then).then(|| ((*now).clone(), (*then).clone()))
}

/// The role of a session `parent` starts, asking for `asked`: that one, else what `parent`'s
/// role starts by default. A session with no role may start any role, or none; one with a role
/// only those its role names. `None` for no role.
pub(crate) fn of_child(parent: &Session, asked: Option<&str>) -> Result<Option<&'static Role>> {
    let own = parent.role.as_deref().and_then(find);
    let child = match asked {
        Some(name) => Some(known(name)?),
        None => own.and_then(|role| role.child).and_then(find),
    };
    let Some(own) = own else {
        return Ok(child);
    };
    if child.is_some_and(|child| own.starts.contains(&child.name)) {
        return Ok(child);
    }
    let refusal = if own.starts.is_empty() {
        format!("a session whose role is {} starts no sessions", own.name)
    } else {
        format!(
            "a session whose role is {} may start only {}",
            own.name,
            own.starts.join(" or ")
        )
    };
    Err(Error::Peers(refusal))
}

impl Core {
    /// Gives session `id` the role called `role`, or with `None` takes its role off, as the
    /// user chooses. Refused for a closed session and for a role there is not.
    ///
    /// Before anything was said in the session, its agent starts with the role in its
    /// guidance, and nothing is recorded. After, the change is recorded as
    /// [`AgentEvent::RoleChanged`], where the transcript shows the new rules start, and the
    /// agent is told along with its next message. Front ends are told to read the tree again.
    ///
    /// A role that changes no files holds the session in plan mode, which an agent without one
    /// takes as its read-only mode (`acp.rs`), and taking it off puts back the supervision it
    /// took the place of, unless the user chose another meanwhile: their choice wins. Either is
    /// recorded as [`AgentEvent::SupervisionChanged`] and told to a running agent at once.
    pub fn session_set_role(&self, id: i64, role: Option<&str>) -> Result<Session> {
        let session = self.session(id)?;
        if self.store().session_is_closed(id)? {
            return Err(Error::SessionClosed(id));
        }
        let role = role.map(known).transpose()?;
        let name = role.map(|role| role.name);
        if session.role.as_deref() == name {
            return Ok(session);
        }
        self.store().session_set_role(id, name)?;
        let said = self
            .session_events(id)?
            .iter()
            .any(|event| matches!(event, AgentEvent::UserMessage { .. }));
        if said {
            let changed = AgentEvent::RoleChanged {
                role: name.map(String::from),
                previous: session.role.clone(),
            };
            self.live.note(&self.store, id, changed)?;
        }
        self.role_supervision(&session, role)?;
        self.live.publish_apart(AgentEvent::TreeChanged);
        self.session(id)
    }

    /// Puts `session` under the supervision `role`, its role now, holds, or back under the one
    /// its earlier role took the place of ([`held`]).
    fn role_supervision(&self, session: &Session, role: Option<&Role>) -> Result<()> {
        let before = self.store().session_supervision_before_role(session.id)?;
        let (supervision, before) = held(role, session.supervision, before);
        self.store()
            .session_set_supervision_before_role(session.id, before)?;
        if supervision != session.supervision {
            self.supervise(session.id, supervision)?;
        }
        Ok(())
    }

    /// What becomes of the plan of session `id` that the user approved, if its role changes no
    /// files: what its agent is told instead of the go-ahead, and the supervision it goes on in,
    /// if it leaves plan mode to hand the plan on.
    pub(crate) fn approved_kept(&self, id: i64) -> Result<Option<(String, Option<Supervision>)>> {
        let session = self.session(id)?;
        let role = session.role.as_deref().and_then(find);
        let kept = role.and_then(|role| Some((approval(role)?, role.approved_supervision)));
        Ok(kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> AgentEvent {
        AgentEvent::UserMessage {
            text: "go on".into(),
            remote: false,
            at_reset: false,
        }
    }

    fn changed_to(role: Option<&str>, previous: Option<&str>) -> AgentEvent {
        AgentEvent::RoleChanged {
            role: role.map(String::from),
            previous: previous.map(String::from),
        }
    }

    #[test]
    fn every_role_is_found_by_its_name_and_what_it_starts_is_a_role() {
        for role in ROLES {
            assert_eq!(find(role.name), Some(&role));
            let mut named = role.starts.iter().chain(role.child.iter());
            assert!(named.all(|name| find(name).is_some()), "{}'s", role.name);
            let child = role.child.is_none_or(|child| role.starts.contains(&child));
            assert!(child, "{}'s default is one it starts", role.name);
            assert!(!role.description.ends_with('.'), "a sentence is made of it");
        }
        assert!(find("boss").is_none());
        let error = known("boss").expect_err("no such role");
        assert!(
            error
                .to_string()
                .contains("architect, planner, implementer, reviewer")
        );
    }

    #[test]
    fn a_role_s_section_holds_its_rules_every_role_and_who_works_with_it() {
        let planner = find("planner").expect("built in");
        let alone = instructions(planner, None);
        assert!(alone.starts_with(
            "# Your role: planner\n\nYour role in this session is planner. A planner plans one"
        ));
        assert!(alone.contains(planner.instructions));
        assert!(alone.contains("`implementer` or `reviewer`"));
        assert!(alone.contains("without naming a role is given `implementer`"));
        for role in ROLES {
            assert!(alone.contains(&format!("- `{}`: {}.", role.name, role.description)));
        }
        assert!(alone.contains(TABLE));
        assert!(!alone.contains("## Who you work with"));

        let implementer = find("implementer").expect("built in");
        let told = instructions(implementer, None);
        assert!(told.contains("You start no sessions."));
        assert!(told.contains("An implementer builds the part"));

        let nobody = Relations::default();
        assert_eq!(section(None, Some(&nobody)), None);
        assert_eq!(section(None, None), None);
        assert!(
            !section(Some(planner), Some(&nobody))
                .expect("a role")
                .contains("## Who you work with")
        );
    }

    #[test]
    fn a_change_is_told_once_with_the_next_message() {
        let to_planner = changed_to(Some("planner"), None);
        assert_eq!(
            changed(std::slice::from_ref(&to_planner)),
            None,
            "before any message"
        );
        assert_eq!(
            changed(&[message(), to_planner.clone()]),
            Some((Some("planner".into()), None))
        );
        assert_eq!(changed(&[message(), to_planner.clone(), message()]), None);
        // Changed twice since: told the latest against what the agent last heard.
        let to_reviewer = changed_to(Some("reviewer"), Some("planner"));
        assert_eq!(
            changed(&[message(), to_planner.clone(), to_reviewer]),
            Some((Some("reviewer".into()), None))
        );
        // And back: nothing to tell.
        let back = changed_to(None, Some("planner"));
        assert_eq!(changed(&[message(), to_planner, back]), None);
    }

    #[test]
    fn the_note_names_the_new_rules_and_drops_the_old() {
        let reviewer = find("reviewer");
        let said = note(reviewer, Some("implementer"));
        assert!(said.starts_with("<role>\n") && said.ends_with("\n</role>"));
        assert!(said.contains("the implementer's, no longer apply"));
        assert!(said.contains("Your role in this session is reviewer. A reviewer reads"));
        let off = note(None, Some("planner"));
        assert!(off.contains("the planner's, no longer apply"));
        assert!(off.contains("You have no role now"));
        assert!(note(reviewer, None).contains("Until now you had no role"));
    }

    #[test]
    fn a_role_starts_only_what_it_may() {
        let with = |role: Option<&str>| Session {
            role: role.map(String::from),
            ..parents::tests::session(1, "")
        };
        let named =
            |found: Result<Option<&'static Role>>| found.map(|role| role.map(|role| role.name));
        assert_eq!(named(of_child(&with(None), None)).ok(), Some(None));
        assert_eq!(
            named(of_child(&with(None), Some("architect"))).ok(),
            Some(Some("architect"))
        );
        let planner = with(Some("planner"));
        assert_eq!(
            named(of_child(&planner, None)).ok(),
            Some(Some("implementer"))
        );
        assert_eq!(
            named(of_child(&planner, Some("reviewer"))).ok(),
            Some(Some("reviewer"))
        );
        let refused = of_child(&planner, Some("architect")).expect_err("not a planner's");
        assert!(
            refused
                .to_string()
                .contains("may start only implementer or reviewer")
        );
        let implementer = with(Some("implementer"));
        let refused = of_child(&implementer, None).expect_err("starts none");
        assert!(refused.to_string().contains("starts no sessions"));
        assert!(of_child(&planner, Some("boss")).is_err());
    }

    #[test]
    fn a_role_that_changes_no_files_holds_a_supervision_and_is_told_how_it_is_kept() {
        for role in ROLES {
            let holds = role.supervision == Some(Supervision::Plan);
            assert_eq!(holds, !role.changes_files, "{}", role.name);
            assert_eq!(role.approved.is_none(), role.changes_files, "{}", role.name);
            let told = instructions(&role, None);
            assert_eq!(told.contains(KEPT_FROM_CHANGES), !role.changes_files);
            assert_eq!(approval(&role).is_some(), !role.changes_files);
        }
        let planner = find("planner").expect("built in");
        let approved = approval(planner).expect("kept");
        assert!(approved.starts_with("The user approved your plan. Your role, the planner's,"));
        assert!(approved.contains("You are out of plan mode now"));
        assert!(approved.ends_with(planner.approved.expect("kept")));
        assert!(note(Some(planner), None).contains(KEPT_FROM_CHANGES));
        // A reviewer has no sessions to follow once approved: it stays where it reads.
        let reviewer = approval(find("reviewer").expect("built in")).expect("kept");
        assert!(reviewer.contains("You stay in the mode that keeps you from it."));
    }

    #[test]
    fn a_role_that_hands_its_plan_on_to_sessions_it_follows_leaves_plan_mode_once_approved() {
        let after = |name: &str| find(name).expect("built in").approved_supervision;
        assert_eq!(after("architect"), Some(Supervision::Auto));
        assert_eq!(after("planner"), Some(Supervision::Auto));
        assert_eq!(after("reviewer"), None);
        assert_eq!(after("implementer"), None);
        for role in ROLES {
            let leaves = role.approved_supervision.is_some();
            assert!(!leaves || role.approved.is_some(), "{} is told", role.name);
        }
    }

    #[test]
    fn only_a_session_with_a_role_uses_the_session_tools_unasked() {
        let others = [
            "Bash",
            "mcp__swarmed__configure",
            "mcp__swarmed__delegate_to_cloud",
            "mcp__swarmed__show_browser",
            "mcp__other__send_message",
        ];
        for role in ROLES {
            let asked = |tool: &&str| !uses_unasked(Some(&role), tool);
            assert!(!SESSION_TOOLS.iter().any(asked), "{}", role.name);
            assert!(others.iter().all(asked), "{}", role.name);
        }
        assert!(!uses_unasked(None, "mcp__swarmed__spawn_session"));
    }

    /// Every session tool a role uses unasked is one the tool server serves, by that name.
    #[test]
    fn the_session_tools_are_named_as_claude_code_names_them() {
        let prefix = format!("mcp__{}__", crate::toolserver::SERVER_NAME);
        for tool in SESSION_TOOLS {
            let name = tool.strip_prefix(&prefix).expect("Swarmed's server");
            assert!(crate::toolserver::serves(name), "{name}");
        }
    }

    #[test]
    fn a_role_s_supervision_is_held_and_what_it_took_the_place_of_put_back() {
        let (planner, implementer) = (find("planner"), find("implementer"));
        let (ask, auto, plan) = (Supervision::Ask, Supervision::Auto, Supervision::Plan);
        assert_eq!(starts_in(planner, auto), (plan, Some(auto)));
        assert_eq!(starts_in(planner, plan), (plan, None));
        assert_eq!(starts_in(implementer, auto), (auto, None));
        assert_eq!(starts_in(None, ask), (ask, None));
        // Set, it remembers what it took the place of the first time only.
        assert_eq!(held(planner, ask, None), (plan, Some(ask)));
        assert_eq!(held(find("reviewer"), plan, Some(ask)), (plan, Some(ask)));
        // Taken off, or for one that holds none, that is put back.
        assert_eq!(held(None, plan, Some(ask)), (ask, None));
        assert_eq!(held(implementer, plan, Some(auto)), (auto, None));
        // Nothing to put back: as it is, the user's own choice included.
        assert_eq!(held(None, auto, None), (auto, None));
    }

    /// `docs/coding-agents.md` shows users the same roles their agents are told of.
    #[test]
    fn the_documentation_shows_the_roles_agents_are_told_of() {
        let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let written = std::fs::read_to_string(docs.join("coding-agents.md"))
            .expect("the documentation is there");
        assert!(written.contains(TABLE), "the architect and planner table");
        for role in ROLES {
            let line = format!("| `{}` | {} |", role.name, role.description);
            assert!(written.contains(&line), "{line}");
        }
    }
}
