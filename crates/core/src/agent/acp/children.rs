//! The sub-agents of an ACP agent: sessions of their own that it starts for parts of its work.
//!
//! Two shapes are read. The protocol's own, still unstable, which Codex sends to a client that
//! declares `subagents`: a `subagent_update` on the agent's session announces a child session
//! and says how its work stands, and the child's own updates and permission requests come
//! under the child's session id. And OpenCode's, to a client that declares
//! `opencode/child-session-updates` in its `_meta`: `opencode/session/child_update`
//! notifications carry a child's status and its updates. Without that, what a child does
//! during the turn that started it comes on the agent's own stream, marked as the child's.
//!
//! A sub-agent is shown as Claude Code's are: by the call that started it, with its events
//! filed under that call, and reported at work in the background from when its agent says it
//! runs to when it says it is done. Codex does not show the call that starts one, so one is
//! made up for it. A sub-agent that is set to work again is shown again, by a call of its own.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::{Voice, str_of};
use crate::agent::{AgentEvent, BackgroundState};

/// The name a call that starts a sub-agent is shown by, which front ends know it by.
pub(super) const STARTING_TOOL: &str = "Agent";
/// The `_meta` key OpenCode marks a child session's updates and requests with.
const CHILD_META: &str = "opencode/child-session";
/// OpenCode's names for its tool that starts a sub-agent: the newer, then the older.
const OPENCODE_TOOLS: [&str; 2] = ["subagent", "task"];
/// What a call made up for a sub-agent answers: the call that started it returned at once.
const STARTED: &str = "Started; it works in the background.";

/// How a sub-agent's work stands, as its agent says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Work {
    /// It is at work, or waits on the user.
    Running,
    /// It is done, and how: with the error it ended on, if it failed.
    Ended(BackgroundState, Option<String>),
}

/// How the agent tells of a sub-agent, which says what can be reported of it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Told {
    /// Only by what it does, on the agent's own stream: nothing says when it starts or ends.
    #[default]
    ByWhatItDoes,
    /// In OpenCode's notifications, which say when it starts and ends. Nothing stops one.
    InNotices,
    /// As the protocol has it, which says when it starts and ends, and stops what it does
    /// when its session is cancelled.
    ByTheProtocol,
}

/// Where a sub-agent's work stands.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Run {
    /// No run of it is under way.
    #[default]
    Idle,
    /// Its agent said a run of it started, which is not shown yet: Codex says what a
    /// sub-agent was asked just after, and the call that shows the run shows that too.
    Due,
    /// A run of it is under way, and shown.
    Running,
}

/// One sub-agent.
#[derive(Debug, Default)]
struct Child {
    /// What it is called, in its agent's words.
    title: String,
    /// What it is there for, in its agent's words: Codex names its role.
    role: String,
    /// What it has said and done and not yet passed on.
    voice: Voice,
    /// What it was last asked and has not been shown with yet.
    prompt: Option<String>,
    /// The call its events are filed under: the one its latest run is shown by.
    call: Option<String>,
    /// A call of the agent's that starts its next run, known before that run starts.
    next: Option<String>,
    /// Where its work stands.
    run: Run,
    /// How many runs it has had.
    runs: u32,
    /// How its agent tells of it.
    told: Told,
}

impl Child {
    fn running(&self) -> bool {
        self.run == Run::Running
    }

    /// Whether its agent says when its runs start and end, so that they are reported.
    fn reported(&self) -> bool {
        self.told != Told::ByWhatItDoes
    }

    /// Whether it can be stopped on its own, by cancelling its session.
    fn cancellable(&self) -> bool {
        self.told == Told::ByTheProtocol
    }

    /// What it is shown by: its title, or failing that its role.
    fn name(&self) -> &str {
        if self.title.is_empty() {
            &self.role
        } else {
            &self.title
        }
    }
}

/// A call of the agent's that starts a sub-agent and is not yet tied to one.
#[derive(Debug)]
struct Starter {
    call: String,
    /// What the sub-agent is to do, which OpenCode also titles the sub-agent with.
    description: String,
    /// The sub-agent it sets to work again, if it names one.
    continues: Option<String>,
}

/// Every sub-agent of a conversation, by its session id.
#[derive(Debug, Default)]
pub(super) struct Children {
    by_session: HashMap<String, Child>,
    starters: Vec<Starter>,
}

impl Children {
    /// What sub-agent `child` has said and done and not yet passed on.
    pub(super) fn voice(&mut self, child: &str) -> Option<&mut Voice> {
        self.by_session.get_mut(child).map(|one| &mut one.voice)
    }

    /// The call sub-agent `child`'s events are filed under, once it has one.
    pub(super) fn call_of(&self, child: &str) -> Option<&str> {
        self.by_session.get(child)?.call.as_deref()
    }

    /// What sub-agent `child` is titled, in its agent's words.
    pub(super) fn title(&self, child: &str) -> Option<String> {
        Some(self.by_session.get(child)?.title.clone())
    }

    /// Passes on what every sub-agent has said since it last moved on: nothing it says after
    /// this belongs with what came before.
    pub(super) fn flush_all(&mut self) -> Vec<AgentEvent> {
        let children: Vec<String> = self.by_session.keys().cloned().collect();
        children
            .iter()
            .filter_map(|child| self.flush(child))
            .collect()
    }

    /// Whether `child` is a sub-agent heard of.
    pub(super) fn knows(&self, child: &str) -> bool {
        self.by_session.contains_key(child)
    }

    /// What a request sub-agent `child` makes of the user says about who asks.
    pub(super) fn asked_by(&self, child: &str) -> Option<String> {
        let name = self.by_session.get(child)?.name();
        Some(if name.is_empty() {
            "Asked by a sub-agent".to_string()
        } else {
            format!("Asked by the sub-agent {name}")
        })
    }

    /// The session of the sub-agent a report of work in the background is about, if it runs
    /// and can be stopped: `task_id` is the call its run is shown by.
    pub(super) fn to_stop(&self, task_id: &str) -> Option<&str> {
        let stoppable = |one: &Child| {
            one.running() && one.cancellable() && one.call.as_deref() == Some(task_id)
        };
        self.by_session
            .iter()
            .find(|(_, one)| stoppable(one))
            .map(|(session, _)| session.as_str())
    }

    /// Takes in a `subagent_update`, the protocol's word on a sub-agent: that it exists, what
    /// it is called, and how its work stands.
    pub(super) fn update(&mut self, update: &Value) -> Vec<AgentEvent> {
        let Some(child) = str_of(update, "sessionId") else {
            return vec![];
        };
        let one = self.by_session.entry(child.to_string()).or_default();
        // A run its agent cannot stop at the moment it is asked to is left as it is.
        one.told = Told::ByTheProtocol;
        if let Some(title) = str_of(update, "title") {
            one.title = title.to_string();
        }
        if let Some(role) = str_of(update, "description") {
            one.role = role.to_string();
        }
        let Some(state) = update.get("state").filter(|state| !state.is_null()) else {
            return vec![];
        };
        let work = match str_of(state, "state").unwrap_or_default() {
            "running" | "requires_action" if !one.running() && one.prompt.is_none() => {
                // Shown once it is known what it was asked, or once it does something.
                one.run = Run::Due;
                return vec![];
            }
            "running" | "requires_action" => Work::Running,
            "idle" => ended_as(state),
            // It can no longer be followed: closed, put away, or out of reach.
            "unknown" => Work::Ended(BackgroundState::Stopped, None),
            _ => return vec![],
        };
        // A run that ends before it was shown is shown all the same.
        let mut events = if one.run == Run::Due {
            self.start_run(child)
        } else {
            vec![]
        };
        events.extend(self.work(child, &work));
        events
    }

    /// Takes in what OpenCode says of a sub-agent's work: `created`, `running`, `completed`,
    /// `failed` or `interrupted`, with the error it failed on.
    pub(super) fn status(&mut self, child: &str, title: &str, params: &Value) -> Vec<AgentEvent> {
        let mut events = self.follow_noticed(child, title);
        let work = match str_of(params, "status").unwrap_or_default() {
            "created" | "running" => Work::Running,
            "completed" => Work::Ended(BackgroundState::Completed, None),
            "interrupted" => Work::Ended(BackgroundState::Stopped, None),
            "failed" => {
                let error = params.pointer("/error/message").and_then(Value::as_str);
                Work::Ended(BackgroundState::Failed, error.map(String::from))
            }
            _ => return events,
        };
        events.extend(self.work(child, &work));
        events
    }

    /// Makes sure sub-agent `child` is known and has a call to file its events under, which
    /// starts a run of it if it has none: what it does says it is at work.
    pub(super) fn follow(&mut self, child: &str, title: Option<&str>) -> Vec<AgentEvent> {
        self.follow_as(child, title, Told::ByWhatItDoes)
    }

    /// Follows sub-agent `child`, as [`Self::follow`] does, from OpenCode's notifications.
    pub(super) fn follow_noticed(&mut self, child: &str, title: &str) -> Vec<AgentEvent> {
        self.follow_as(child, Some(title), Told::InNotices)
    }

    fn follow_as(&mut self, child: &str, title: Option<&str>, told: Told) -> Vec<AgentEvent> {
        let one = self.by_session.entry(child.to_string()).or_default();
        one.told = one.told.max(told);
        if let Some(title) = title.filter(|title| one.title.is_empty() && !title.is_empty()) {
            one.title = title.to_string();
        }
        if one.call.is_some() && one.run != Run::Due {
            return vec![];
        }
        self.start_run(child)
    }

    /// Notes that the agent itself called `call`, a tool that starts a sub-agent, with `input`.
    pub(super) fn starting(&mut self, call: &str, input: &Value) {
        self.starters.push(Starter {
            call: call.to_string(),
            description: str_of(input, "description").unwrap_or_default().to_string(),
            continues: str_of(input, "sessionID")
                .or_else(|| str_of(input, "task_id"))
                .map(String::from),
        });
    }

    /// Takes in the end of the agent's own call `call`: one that started a sub-agent names it,
    /// as OpenCode's does, and that ties the two if nothing has yet.
    pub(super) fn answered(&mut self, call: &str, update: &Value) {
        let Some(at) = self.starters.iter().position(|one| one.call == call) else {
            return;
        };
        self.starters.remove(at);
        let named = update.pointer("/rawOutput/metadata/sessionID");
        let Some(child) = named.and_then(Value::as_str) else {
            return;
        };
        let one = self.by_session.entry(child.to_string()).or_default();
        if !one.running() {
            one.next = Some(call.to_string());
        }
    }

    /// Takes in a message the agent sent one of its sub-agents, as Codex tells it: shown with
    /// the call of the run it starts. What one is told while at work is not shown: a front end
    /// shows a sub-agent's conversation as its own words alone.
    pub(super) fn message(&mut self, update: &Value) -> Vec<AgentEvent> {
        let to = str_of(update, "recipientSessionId").unwrap_or_default();
        let Some(one) = self.by_session.get_mut(to) else {
            return vec![];
        };
        let blocks = update.get("content").and_then(Value::as_array);
        let texts: Vec<&str> = blocks
            .into_iter()
            .flatten()
            .filter_map(|block| str_of(block, "text"))
            .collect();
        if texts.is_empty() {
            return vec![];
        }
        if one.running() {
            return vec![];
        }
        one.prompt = Some(texts.join("\n"));
        if one.run == Run::Due {
            return self.start_run(to);
        }
        vec![]
    }

    /// Passes on what sub-agent `child` has said since it last moved on, filed under its call.
    pub(super) fn flush(&mut self, child: &str) -> Option<AgentEvent> {
        let one = self.by_session.get_mut(child)?;
        let call = one.call.clone()?;
        let text = one.voice.take_text()?;
        Some(sub(&call, AgentEvent::Text { text }))
    }

    /// What follows from sub-agent `child`'s work standing as `work` now: a run that starts or
    /// one that ends, with the call it is shown by and the reports of it.
    fn work(&mut self, child: &str, work: &Work) -> Vec<AgentEvent> {
        let running = self.by_session.get(child).is_some_and(Child::running);
        match work {
            Work::Running if !running => self.start_run(child),
            Work::Ended(state, error) if running => self.end_run(child, *state, error.clone()),
            _ => vec![],
        }
    }

    /// Starts a run of sub-agent `child`: tied to the agent's own call that started it, if
    /// one did, else to one made up for it, which is shown as having returned at once.
    fn start_run(&mut self, child: &str) -> Vec<AgentEvent> {
        let tied = self.tie(child);
        let Some(one) = self.by_session.get_mut(child) else {
            return vec![];
        };
        if one.running() {
            return vec![];
        }
        one.run = Run::Running;
        one.runs += 1;
        // What the run before it said last is not what this one hands back.
        one.voice.said.clear();
        let (call, mut events) = match tied {
            Some(call) => (call, vec![]),
            None => made_up(child, one),
        };
        one.call = Some(call.clone());
        if one.reported() {
            events.push(report(one, &call, BackgroundState::Running, None));
        }
        events
    }

    /// The agent's own call that starts sub-agent `child`'s next run, if there is one: the one
    /// its end named it in, else one that says it continues it, else one that set out to do
    /// what it is titled with.
    fn tie(&mut self, child: &str) -> Option<String> {
        let one = self.by_session.get_mut(child)?;
        if let Some(call) = one.next.take() {
            return Some(call);
        }
        let title = one.title.clone();
        let continues = |starter: &Starter| starter.continues.as_deref() == Some(child);
        let titled =
            |starter: &Starter| starter.continues.is_none() && starter.description == title;
        let at = self
            .starters
            .iter()
            .position(continues)
            .or_else(|| self.starters.iter().position(titled))?;
        Some(self.starters.remove(at).call)
    }

    /// Ends the run of sub-agent `child` under way: what it last said is what it hands back.
    fn end_run(
        &mut self,
        child: &str,
        state: BackgroundState,
        error: Option<String>,
    ) -> Vec<AgentEvent> {
        let mut events: Vec<AgentEvent> = self.flush(child).into_iter().collect();
        let Some(one) = self.by_session.get_mut(child) else {
            return events;
        };
        one.run = Run::Idle;
        let Some(call) = one.call.clone().filter(|_| one.reported()) else {
            return events;
        };
        let summary =
            error.or_else(|| Some(one.voice.said.clone()).filter(|said| !said.is_empty()));
        events.push(report(one, &call, state, summary));
        events
    }
}

/// A call made up to show a run of sub-agent `child`, which no call of its agent's shows, and
/// how it is shown: started, and returned at once, as its starter's own call did.
fn made_up(child: &str, one: &mut Child) -> (String, Vec<AgentEvent>) {
    let call = if one.runs == 1 {
        child.to_string()
    } else {
        format!("{child}~{}", one.runs)
    };
    let prompt = one.prompt.take().unwrap_or_default();
    let input = json!({ "description": one.name(), "subagent_type": one.role, "prompt": prompt });
    let started = AgentEvent::ToolUse {
        id: call.clone(),
        name: STARTING_TOOL.to_string(),
        input,
    };
    let returned = AgentEvent::ToolResult {
        tool_use_id: call.clone(),
        content: STARTED.to_string(),
        is_error: false,
        images: Vec::new(),
        returned: Vec::new(),
    };
    (call, vec![started, returned])
}

/// `event` filed under the sub-agent shown by `call`.
fn sub(call: &str, event: AgentEvent) -> AgentEvent {
    AgentEvent::Sub {
        parent: call.to_string(),
        event: Box::new(event),
    }
}

/// A report of sub-agent `one`'s run shown by `call`, as work in the background.
fn report(one: &Child, call: &str, state: BackgroundState, summary: Option<String>) -> AgentEvent {
    AgentEvent::Background {
        task_id: call.to_string(),
        description: one.name().to_string(),
        kind: "local_agent".to_string(),
        state,
        summary,
        tool_use_id: Some(call.to_string()),
        output_file: None,
        moved: false,
        unstoppable: !one.cancellable(),
    }
}

/// How a run ended, by why the protocol says its sub-agent is idle.
fn ended_as(state: &Value) -> Work {
    match str_of(state, "stopReason").unwrap_or_default() {
        "cancelled" => Work::Ended(BackgroundState::Stopped, None),
        "error" => {
            let said = state.pointer("/error/message").and_then(Value::as_str);
            Work::Ended(BackgroundState::Failed, said.map(String::from))
        }
        _ => Work::Ended(BackgroundState::Completed, None),
    }
}

/// The sub-agent an update on the agent's own stream belongs to, by its session id and
/// title, if OpenCode marked it as one's.
pub(super) fn marked(value: &Value) -> Option<(String, String)> {
    let child = value.get("_meta")?.get(CHILD_META)?;
    let id = str_of(child, "id")?;
    let title = str_of(child, "title").unwrap_or_default();
    Some((id.to_string(), title.to_string()))
}

/// `update` without what OpenCode puts before a sub-agent's tool call's title: the
/// sub-agent's own title, which the sub-agent's own conversation needs no reminding of.
pub(super) fn untitled(update: &Value, title: &str) -> Value {
    let mut update = update.clone();
    let prefix = format!("{title}: ");
    let shorter = str_of(&update, "title")
        .and_then(|said| said.strip_prefix(&prefix))
        .map(String::from);
    if let (Some(shorter), Some(fields)) = (shorter, update.as_object_mut()) {
        fields.insert("title".to_string(), Value::String(shorter));
    }
    update
}

/// Whether `tool`, a call of the agent's own, starts a sub-agent the way OpenCode's does.
pub(super) fn starts_one(tool: &Value) -> bool {
    let named = |key: &str| str_of(tool, key).is_some_and(|name| OPENCODE_TOOLS.contains(&name));
    let asked = tool
        .get("rawInput")
        .and_then(|input| str_of(input, "prompt"))
        .is_some();
    str_of(tool, "kind") == Some("think") && (named("name") || named("title")) && asked
}

/// The arguments of a call that starts a sub-agent, with its kind under the name front ends
/// read it by: OpenCode calls it `agent`, and before that called it what Claude Code does.
pub(super) fn starting_input(input: &Value) -> Value {
    let mut input = input.clone();
    let agent = str_of(&input, "agent").map(String::from);
    if let (Some(agent), Some(fields)) = (agent, input.as_object_mut()) {
        fields
            .entry("subagent_type")
            .or_insert(Value::String(agent));
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(events: &[AgentEvent]) -> Vec<String> {
        let kind = |event: &AgentEvent| match event {
            AgentEvent::ToolUse { name, .. } => format!("use {name}"),
            AgentEvent::ToolResult { .. } => "result".to_string(),
            AgentEvent::Background { state, .. } => format!("background {state:?}"),
            AgentEvent::Sub { event, .. } => format!("sub {:?}", std::mem::discriminant(&**event)),
            other => format!("{other:?}"),
        };
        events.iter().map(kind).collect()
    }

    fn update(body: Value) -> Value {
        let mut body = body;
        body["sessionUpdate"] = json!("subagent_update");
        body
    }

    const CODEX_CHILD: &str = "P:subagent:T1";

    /// A sub-agent Codex announced, titled, asked something and set to work.
    fn codex_child_at_work(children: &mut Children) -> Vec<AgentEvent> {
        let announced = children.update(&update(
            json!({ "sessionId": CODEX_CHILD, "capabilities": {} }),
        ));
        assert_eq!(announced, [], "nothing to show before it works");
        children.update(&update(
            json!({ "sessionId": CODEX_CHILD, "title": "Explorer 1", "description": "explorer" }),
        ));
        let asked = json!({ "sessionUpdate": "session_message", "senderSessionId": "P",
            "recipientSessionId": CODEX_CHILD, "content": [{ "type": "text", "text": "Find it." }] });
        assert_eq!(
            children.message(&asked),
            [],
            "kept for the call that starts its run"
        );
        children.update(&update(
            json!({ "sessionId": CODEX_CHILD, "state": { "state": "running" } }),
        ))
    }

    #[test]
    fn a_codex_sub_agent_is_shown_by_a_call_made_up_for_it_and_reported_while_it_works() {
        let mut children = Children::default();
        let started = codex_child_at_work(&mut children);
        assert_eq!(
            kinds(&started),
            ["use Agent", "result", "background Running"]
        );
        let AgentEvent::ToolUse { id, input, .. } = &started[0] else {
            panic!("not a call: {started:?}");
        };
        assert_eq!(id, CODEX_CHILD);
        assert_eq!(
            input,
            &json!({ "description": "Explorer 1", "subagent_type": "explorer", "prompt": "Find it." })
        );
        assert_eq!(children.call_of(CODEX_CHILD), Some(CODEX_CHILD));
        assert_eq!(children.to_stop(CODEX_CHILD), Some(CODEX_CHILD));
    }

    #[test]
    fn a_codex_sub_agent_hands_back_what_it_said_last_and_is_shown_again_when_set_to_work() {
        let mut children = Children::default();
        codex_child_at_work(&mut children);
        let voice = children.voice(CODEX_CHILD).expect("known");
        voice.text.push_str("Found it in auth.rs.");
        let idle = json!({ "sessionId": CODEX_CHILD, "state": { "state": "idle", "stopReason": "end_turn" } });
        let ended = children.update(&update(idle));
        assert!(
            matches!(&ended[1], AgentEvent::Background { state: BackgroundState::Completed, summary: Some(said), .. } if said == "Found it in auth.rs.")
        );
        assert_eq!(children.to_stop(CODEX_CHILD), None, "nothing runs");
        let more = json!({ "recipientSessionId": CODEX_CHILD, "content": [{ "type": "text", "text": "More." }] });
        assert_eq!(children.message(&more), []);
        let again = children.update(&update(
            json!({ "sessionId": CODEX_CHILD, "state": { "state": "running" } }),
        ));
        assert!(
            matches!(&again[0], AgentEvent::ToolUse { id, input, .. } if id == "P:subagent:T1~2" && input["prompt"] == "More.")
        );
        // Stopped before it said a word, it hands back nothing, not the first run's words.
        let idle = json!({ "sessionId": CODEX_CHILD, "state": { "state": "idle", "stopReason": "cancelled" } });
        let ended = children.update(&update(idle));
        let handed_back = ended.iter().find_map(|event| match event {
            AgentEvent::Background { summary, .. } => Some(summary.clone()),
            _ => None,
        });
        assert_eq!(handed_back, Some(None), "{ended:?}");
    }

    #[test]
    fn a_run_said_to_start_before_what_it_was_asked_is_shown_with_that() {
        let mut children = Children::default();
        children.update(&update(
            json!({ "sessionId": "c", "capabilities": {}, "title": "Worker" }),
        ));
        let early = children.update(&update(
            json!({ "sessionId": "c", "state": { "state": "running" } }),
        ));
        assert!(early.is_empty(), "held until it is known what it was asked");
        let asked =
            json!({ "recipientSessionId": "c", "content": [{ "type": "text", "text": "Count." }] });
        let started = children.message(&asked);
        assert!(
            matches!(&started[0], AgentEvent::ToolUse { input, .. } if input["prompt"] == "Count.")
        );
        // One that does something first is shown then, without it.
        children.update(&update(
            json!({ "sessionId": "c", "state": { "state": "idle" } }),
        ));
        children.update(&update(
            json!({ "sessionId": "c", "state": { "state": "running" } }),
        ));
        let doing = children.follow("c", None);
        assert!(
            matches!(&doing[0], AgentEvent::ToolUse { id, input, .. } if id == "c~2" && input["prompt"] == "")
        );
        // And one that ends before either is shown as it ends.
        children.update(&update(
            json!({ "sessionId": "c", "state": { "state": "idle" } }),
        ));
        children.update(&update(
            json!({ "sessionId": "c", "state": { "state": "running" } }),
        ));
        let ended = children.update(&update(
            json!({ "sessionId": "c", "state": { "state": "idle" } }),
        ));
        assert_eq!(
            kinds(&ended),
            [
                "use Agent",
                "result",
                "background Running",
                "background Completed"
            ]
        );
    }

    #[test]
    fn a_failed_or_cancelled_run_says_so() {
        let failed = json!({ "state": "idle", "stopReason": "error", "error": { "code": -32603, "message": "boom" } });
        assert_eq!(
            ended_as(&failed),
            Work::Ended(BackgroundState::Failed, Some("boom".into()))
        );
        let stopped = json!({ "state": "idle", "stopReason": "cancelled" });
        assert_eq!(
            ended_as(&stopped),
            Work::Ended(BackgroundState::Stopped, None)
        );
    }

    #[test]
    fn an_opencode_sub_agent_hangs_from_the_call_that_started_it_and_cannot_be_stopped() {
        let mut children = Children::default();
        let call = json!({ "toolCallId": "call_1", "name": "subagent", "title": "subagent", "kind": "think",
            "rawInput": { "agent": "explore", "description": "Find auth code", "prompt": "Look." } });
        assert!(starts_one(&call));
        let input = starting_input(&call["rawInput"]);
        assert_eq!(input["subagent_type"], "explore");
        children.starting("call_1", &input);
        let created = children.status("ses_C", "Find auth code", &json!({ "status": "created" }));
        // Its own call shows it: nothing is made up.
        assert_eq!(kinds(&created), ["background Running"]);
        assert_eq!(children.call_of("ses_C"), Some("call_1"));
        assert_eq!(children.to_stop("call_1"), None, "OpenCode cannot stop one");
        assert!(matches!(
            &created[0],
            AgentEvent::Background {
                unstoppable: true,
                ..
            }
        ));
        children.answered(
            "call_1",
            &json!({ "rawOutput": { "metadata": { "sessionID": "ses_C", "status": "running" } } }),
        );
        let failed = children.status(
            "ses_C",
            "Find auth code",
            &json!({ "status": "failed", "error": { "message": "no model" } }),
        );
        assert!(
            matches!(&failed[..], [AgentEvent::Background { state: BackgroundState::Failed, summary: Some(why), .. }] if why == "no model")
        );
        // Continued by a call that names it, that call shows its next run.
        let again = json!({ "agent": "explore", "description": "Look again", "prompt": "More.", "sessionID": "ses_C" });
        children.starting("call_2", &again);
        let running = children.status("ses_C", "Find auth code", &json!({ "status": "running" }));
        assert!(
            matches!(&running[..], [AgentEvent::Background { task_id, .. }] if task_id == "call_2")
        );
    }

    #[test]
    fn a_sub_agent_whose_call_ended_first_is_tied_by_what_the_call_named() {
        let mut children = Children::default();
        children.starting(
            "call_9",
            &json!({ "description": "Other words", "prompt": "Go." }),
        );
        children.answered(
            "call_9",
            &json!({ "rawOutput": { "metadata": { "sessionID": "ses_D" } } }),
        );
        let created = children.status("ses_D", "Its own title", &json!({ "status": "created" }));
        assert!(
            matches!(&created[..], [AgentEvent::Background { task_id, .. }] if task_id == "call_9")
        );
    }

    #[test]
    fn opencode_marks_and_titles_are_read_and_taken_off() {
        let update = json!({ "sessionUpdate": "tool_call", "toolCallId": "ses_C:call_1", "title": "Find auth code: read",
            "_meta": { "opencode/child-session": { "id": "ses_C", "parentID": "ses_R", "depth": 1, "title": "Find auth code" } } });
        assert_eq!(
            marked(&update),
            Some(("ses_C".to_string(), "Find auth code".to_string()))
        );
        assert_eq!(untitled(&update, "Find auth code")["title"], "read");
        assert_eq!(marked(&json!({ "title": "x" })), None);
        // A call of another kind, or of another name, starts none.
        let read = json!({ "kind": "read", "title": "subagent", "rawInput": { "prompt": "x" } });
        assert!(!starts_one(&read));
    }

    #[test]
    fn a_sub_agent_heard_of_only_by_what_it_does_gets_a_call_and_no_reports() {
        let mut children = Children::default();
        let followed = children.follow("ses_E", Some("Count lines"));
        assert_eq!(kinds(&followed), ["use Agent", "result"]);
        assert_eq!(children.follow("ses_E", None), [], "once");
        assert_eq!(
            children.asked_by("ses_E").as_deref(),
            Some("Asked by the sub-agent Count lines")
        );
    }
}
