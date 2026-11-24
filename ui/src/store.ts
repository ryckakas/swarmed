import { create } from "zustand";
import type { Api } from "./api/client";
import type {
    AccountUsage,
    AgentEvent,
    CustomLayout,
    Issue,
    NoteTarget,
    Project,
    Pull,
    QueuedMessage,
    RemoveTarget,
    SessionEvent,
    SessionSettings,
    Settings,
    SetupProgress,
    Template,
    TerminalConfig,
    UpdateState,
} from "./api/types";
import { CHATS } from "./chats";
import type { CloudTarget } from "./cloud";
import type { Comparison } from "./components/CompareDialog";
import { type IssueLink, type IssuesTarget, issueLink, withIssuePrompt } from "./issues";
import { forgetImages } from "./keptImages";
import { pullsOf, withMessage } from "./pulls";
import { doneShownFor } from "./putAway";
import { type Dismissed, readDismissed } from "./rateLimit";
import type { Browsing } from "./recall";
import { type ReviewComment, withComment } from "./review";
import { type ScrollSpot, withoutScrolls } from "./scrollSpot";
import { type Draft, NO_DRAFT, putBack, withDraft } from "./sending";
import { heard, readFrom, type SetupSeen } from "./setup";
import { atWork, type SubAgent, subAgentsIn } from "./subagents";
import {
    applySessionEvent,
    findSession,
    findSessionHome,
    mayHaveEdited,
    stillFolded,
    taskIdOf,
    toggledId,
    unfoldedFor,
    withoutIds,
    withoutSession,
    withRead,
    withSession,
} from "./tree";
import { turnsBegan } from "./turns";
import { unlessSame } from "./unchanged";

/** The parts the settings are divided into, in the order their tabs are listed. */
export const SETTINGS_TABS = [
    ["appearance", "Appearance"],
    ["sessions", "Sessions"],
    ["rules", "Rules"],
    ["status", "Status line"],
    ["agents", "Agents"],
    ["skills", "Skills"],
    ["prompts", "Prompts"],
    ["actions", "Actions"],
    ["terminal", "Terminal"],
    ["mcp", "MCP servers"],
    ["keys", "API keys"],
    ["usage", "Storage and activity"],
    ["shortcuts", "Keyboard shortcuts"],
    // Only in the swarm's styles: the plain words need no dictionary.
    ["dictionary", "Swarm dictionary"],
    ["about", "About"],
] as const;

export type SettingsTab = (typeof SETTINGS_TABS)[number][0];

/** What can be shown over the window, from the menu bar or a shortcut, and closed again. */
export type Banner = "about" | "shortcuts";

/** What is being done to a task's stack until it is done. */
export type StackChange = "starting" | "stopping";

interface State {
    projects: Project[];
    /** `loading` until the first read finished; an error message if it failed. */
    load: "loading" | "ready" | { error: string };
    selectedSessionId: number | null;
    /** The repository preselected on the new-task screen, shown when no session is selected. */
    draftProject: string | null;
    /**
     * What each session's message box holds and has not sent, by the session's id. Only in
     * memory: it lasts as long as the window, and nothing typed and unsent is written anywhere.
     */
    drafts: Record<number, Draft>;
    /** Changes what the message box of session `sessionId` holds. */
    editDraft(sessionId: number, change: Partial<Draft>): void;
    /**
     * Puts a message that could not be sent back in its session's box, wherever the user is by
     * then, unless something else was written there meanwhile, which is theirs to keep.
     */
    putDraftBack(sessionId: number, unsent: Draft): void;
    /**
     * Where going through the messages sent with ↑ and ↓ stands in each session's message box.
     * It holds what was typed before the first ↑, which is not in `drafts` while a sent message
     * is in the box, so it is kept here, where opening another session does not lose it.
     */
    recalls: Record<number, Browsing>;
    /** Notes where going through the messages sent stands in session `sessionId`, or that it ended. */
    noteRecall(sessionId: number, browsing: Browsing | null): void;
    /**
     * Where each transcript was left, by `scrollKey`: a session's, and a sub-agent's under its
     * session. In memory only, and forgotten with the session, as its draft is.
     */
    scrolls: Record<string, ScrollSpot>;
    /** Keeps where a transcript was left, if its session is still there to come back to. */
    keepScroll(sessionId: number, key: string, spot: ScrollSpot): void;
    /** What the new-task box holds and has not sent, per repository by its name; in memory too. */
    newTaskDrafts: Record<string, Draft>;
    /** Changes what the new-task box holds for repository `project`. */
    editNewTaskDraft(project: string, change: Partial<Draft>): void;
    /** The issue the new task of each repository is for, by the repository's name; in memory. */
    newTaskIssues: Record<string, IssueLink>;
    /** Says which issue the new task of repository `project` is for; `null` for none. */
    linkIssue(project: string, link: IssueLink | null): void;
    /**
     * Starts a task on an issue: the new-task view of its repository opens with the issue's
     * prompt in the box, for the user to read and send, and the task is linked to the issue.
     */
    startFromIssue(project: string, issue: Pick<Issue, "number" | "title" | "url">): void;
    /** The repository whose GitHub issues are listed, and the issue picked there first. */
    issuesOf: IssuesTarget | null;
    /** Opens a repository's issues; `null` puts them away. */
    showIssues(target: IssuesTarget | null): void;
    /** The events of every session whose transcript has been opened. */
    events: Record<number, AgentEvent[]>;
    /** Per task id, a count that goes up whenever its agents may have changed files. */
    edits: Record<number, number>;
    /**
     * The stacks being started or stopped, by task id, until that is done. Kept per task, so
     * that it shows on that task alone, and still does when it is opened again meanwhile.
     */
    stackChanges: Record<number, StackChange>;
    /** Notes that the stack of task `taskId` is being started or stopped, or that it is done. */
    noteStackChange(taskId: number, change: StackChange | null): void;
    /**
     * What each task's commit message box holds and has not committed, by the task's id. Only
     * in memory, as the message box's drafts are.
     */
    commitDrafts: Record<number, string>;
    editCommitDraft(taskId: number, message: string): void;
    /**
     * The sub-agents each session's agent has started and not yet seen done, as far as this
     * window has heard; which are at work is `working` in `subagents.ts`.
     */
    subAgents: Record<number, SubAgent[]>;
    /** The sub-agent whose own conversation is on screen in place of its session's, if any. */
    subView: { sessionId: number; id: string } | null;
    /** Shows what a sub-agent of a session says and does, in place of the session itself. */
    openSub(api: Api, sessionId: number, id: string): Promise<void>;
    /** Goes back from a sub-agent to its session. */
    closeSub(): void;
    /** When each session's running turn began, as seen from this window. */
    turnBegan: Record<number, number>;
    /** Goes up each time an agent reports something it left running starting or ending. */
    backgroundHeard: number;
    /**
     * The account's usage as any Claude Code session last heard it, which every Claude Code
     * session shows; `null` until it is read.
     */
    accountUsage: AccountUsage | null;
    /**
     * The latest line a hook or an installer printed, by what it was printed for
     * (`hookKey`): a task being made or removed, or an agent being installed. Kept apart, so
     * that two at once do not share one line.
     */
    hookLines: Record<string, string>;
    /** Forgets the line kept under `key`, for something that starts printing anew. */
    forgetHookLine(key: string): void;
    /**
     * What each task's setup printed and where it stands, by the task's id, as this window
     * heard it and read it from the setup's log. Kept here, so that opening another session and
     * coming back loses nothing.
     */
    setups: Record<number, SetupSeen>;
    /** Reads the end of a task's setup log into `setups`; `began` names the run under way, if any. */
    readSetupLog(
        api: Api,
        target: { project: string; task: string; taskId: number },
        began: number | null,
    ): Promise<void>;
    /**
     * The task each repository's new-task view is making, by the repository's name, until it is
     * written down and its first session opened: `NAMING` while a model names it.
     */
    creating: Record<string, string>;
    /** Notes the task a repository's new-task view is making, or with `null` that it is made. */
    noteCreating(project: string, name: string | null): void;
    /** The last thing that went wrong when acting on a session, shown until dismissed. */
    problem: string | null;
    /**
     * The usage notice as it was last dismissed, for the account and kept on this machine:
     * it stays away in every session until a higher level, or until its window resets.
     */
    usageDismissed: Dismissed | null;
    dismissUsage(dismissed: Dismissed): void;
    refresh(api: Api): Promise<void>;
    /** Selects a session and loads its transcript if it has not been opened yet. */
    open(api: Api, sessionId: number): Promise<void>;
    /** Shows the new-task screen, for `project` if one is given. */
    startNew(project?: string): void;
    apply(event: SessionEvent): void;
    /** Changes a session's model and supervision. */
    configure(api: Api, sessionId: number, settings: SessionSettings): Promise<void>;
    /** Puts a session that has not begun on another agent. */
    setAgent(api: Api, sessionId: number, agent: string): Promise<void>;
    /** The user's preferences; `null` until they are read. */
    settings: Settings | null;
    /** Whether the task panel on the right is shown. */
    panelOpen: boolean;
    togglePanel(): void;
    /** Changes some of the user's preferences and keeps the rest. */
    changeSettings(api: Api, change: Partial<Settings>): Promise<void>;
    /** The user's configuration of the terminal, once read. */
    terminalConfig: TerminalConfig | null;
    /** Has the core check `text` as the terminal's configuration and keep it; resolves to what is wrong with it, or to nothing. */
    saveTerminalConfig(api: Api, text: string): Promise<string | null>;
    /**
     * Has the core check `prefix` as the branch prefix and keep it. Resolves to what is wrong
     * with it, or to nothing once it is kept, as the core wrote it down.
     */
    saveBranchPrefix(api: Api, prefix: string): Promise<string | null>;
    /** The user's custom layout, once read; drawn when the settings choose it. */
    customLayout: CustomLayout | null;
    /**
     * Has the core check `text` as the custom layout and keep it. Resolves to what is wrong
     * with it, or to nothing once it is kept and drawn.
     */
    saveCustomLayout(api: Api, text: string): Promise<string | null>;
    /** The ids of the repositories whose tasks are folded away in the sidebar. */
    folded: number[];
    toggleFolded(projectId: number): void;
    /** The ids of the tasks whose sessions are folded away in the sidebar. */
    foldedTasks: number[];
    toggleTaskFolded(taskId: number): void;
    /**
     * The ids of the repositories whose Done group, of the tasks put away, is unfolded in the
     * sidebar. Folded until the user unfolds it, or opens a session of a task in it.
     */
    doneShown: number[];
    toggleDoneShown(projectId: number): void;
    /**
     * Puts a task away and reads the tree again. What went wrong on the way, a `stop` that
     * failed say, is said; a refusal is thrown, for whoever asked to say.
     */
    putAway(api: Api, project: string, task: string): Promise<void>;
    /** Brings a task that was put away back to the others, and reads the tree again. */
    bringBack(api: Api, project: string, task: string): Promise<void>;
    /** Whether the status line under the message box is shown, or folded away. */
    statusOpen: boolean;
    toggleStatus(): void;
    /** Whether the terminal is shown in place of the message box. */
    terminalOpen: boolean;
    toggleTerminal(): void;
    /**
     * Words of one of the agent's replies waiting to be quoted into a session's message box,
     * once that is on screen: the box puts the quote in at its caret.
     */
    quoteWaiting: { sessionId: number; text: string } | null;
    /** Quotes `words` into the message box of `sessionId`, which takes the terminal's place. */
    quoteReply(sessionId: number, words: string): void;
    /** The words waiting to be quoted into session `sessionId`'s box, once: `null` if none. */
    takeQuote(sessionId: number): string | null;
    /** A command waiting to be typed into a session's terminal, once that is on screen. */
    terminalCommand: { sessionId: number; text: string } | null;
    /**
     * Shows the terminal and has `text` typed into the shell of `sessionId`. A Claude Code
     * shown there in its place makes way and keeps running: the command is run at once, where
     * the user sees it, never held back for whenever the shell is next shown.
     */
    runInTerminal(sessionId: number, text: string): void;
    /**
     * The command, without its slash, that each session opened Claude Code on in its terminal,
     * shown there in place of the shell until it ends or the user goes back to the shell.
     */
    claudeTerminals: Record<number, string>;
    /** Shows the terminal with Claude Code opened on `command`'s screen in place of the shell. */
    openClaudeTerminal(sessionId: number, command: string): void;
    /** Shows the session's shell again, once its Claude Code has ended or is being ended. */
    leaveClaudeTerminal(sessionId: number): void;
    /**
     * The command waiting for the terminal of `sessionId`, which is then no longer waiting: so
     * however often it is asked for, it is typed once.
     */
    takeTerminalCommand(sessionId: number): string | null;
    /** Goes up whenever the user's actions were changed, so what shows them reads them again. */
    actionsChanged: number;
    noteActionsChanged(): void;
    /** Goes up each time the user asks to rename the open session, from wherever. */
    renameAsked: number;
    askToRename(): void;
    /** Whether the settings are on screen. */
    settingsOpen: boolean;
    /** The part of the settings that is on screen, or was last. */
    settingsTab: SettingsTab;
    /** Shows or hides the settings; opening can name the part to show. */
    showSettings(open: boolean, tab?: SettingsTab): void;
    /** Gives a session a new title. The tree follows when the app confirms it. */
    rename(api: Api, sessionId: number, title: string): Promise<void>;
    /** The repository or task the user asked to remove, until they confirm or think again. */
    removing: RemoveTarget | null;
    askToRemove(target: RemoveTarget | null): void;
    /** The removals under way, each by what names it: closing the dialog stops none. */
    removals: string[];
    removalBegun(key: string): void;
    removalEnded(key: string): void;
    /**
     * Leaves the session on screen if it belongs to what is about to be removed, before the
     * removal stops its agents and takes its worktree away: nothing shown may read that worktree
     * meanwhile. The centre goes to the new-task view of the task's repository, or of another
     * repository when the repository itself is going.
     */
    leaveForRemoval(target: RemoveTarget): void;
    /** Notes that the user has looked at a session, here and in the core. */
    markRead(api: Api, sessionId: number): void;
    /** Goes up whenever the agents or their models change, so menus read them again. */
    agentsVersion: number;
    agentsChanged(): void;
    /** Per session, the messages waiting for its agent to finish. */
    queues: Record<number, QueuedMessage[]>;
    setQueue(sessionId: number, messages: QueuedMessage[]): void;
    /** The repository, task or session work is being handed to the cloud from. */
    delegating: CloudTarget | null;
    /** Opens the form that delegates to the cloud; `null` puts it away. */
    askToDelegate(target: CloudTarget | null): void;
    /** The repository whose environments are being edited. */
    environmentsOf: string | null;
    /** Opens a repository's environments; `null` puts them away. */
    askForEnvironments(project: string | null): void;
    /** The repository or task whose note is being edited. */
    noting: NoteTarget | null;
    /** Opens a note; `null` puts it away. */
    askForNote(target: NoteTarget | null): void;
    /** The user's comments on each task's diff, by the task's id, until they are sent. */
    reviews: Record<number, ReviewComment[]>;
    /** Puts a comment on a line, changes it, or with empty text takes it away. */
    comment(taskId: number, comment: ReviewComment): void;
    /** Drops a task's comments: they were sent, or are not wanted. */
    clearReview(taskId: number): void;
    /**
     * Each task's pull request by the task's id, as the core last read it: from the tree, and
     * from each reading it publishes since. `null` for none.
     */
    pulls: Record<number, Pull | null>;
    /** Asks again for a task's pull request. Not hearing back changes nothing. */
    loadPull(api: Api, project: string, task: string, taskId: number): Promise<void>;
    /**
     * Puts a message about a task's failed checks, with the last lines of their logs, into
     * session `sessionId`'s message box after what is typed there, for the user to read and
     * send. Nothing is sent.
     */
    draftFailure(api: Api, project: string, task: string, sessionId: number): Promise<void>;
    /** A task's worktree was put on another branch: whatever reads its files reads again. */
    worktreeMoved(taskId: number): void;
    /** The cloud task whose changes are being looked at, by Swarmed's id for it. */
    cloudDiffOf: number | null;
    showCloudDiff(id: number | null): void;
    /** The two tasks set side by side, while they are. */
    comparing: Comparison | null;
    /** Sets two tasks side by side; `null` puts the comparison away. */
    compare(comparison: Comparison | null): void;
    /** The prompts the user keeps to send again. */
    templates: Template[];
    setTemplates(templates: Template[]): void;
    /** Whether the search across conversations is open. */
    searchOpen: boolean;
    showSearch(open: boolean): void;
    /** Whether the list of everything that waits for the user is open. */
    inboxOpen: boolean;
    showInbox(open: boolean): void;
    /** The About view or the keyboard shortcuts, over the window; `null` when neither is. */
    banner: Banner | null;
    showBanner(banner: Banner | null): void;
    /** The installed app's version; `null` until it is read. */
    version: string | null;
    /** Where updating Swarmed stands, as the app last said. */
    update: UpdateState;
    /** The session the user asked to close, until they confirm or change their mind. */
    closing: number | null;
    /** Asks before closing: `null` withdraws the question. */
    askToClose(sessionId: number | null): void;
    /** A closed session the user is asked about deleting for good, if any. */
    deleting: { id: number; title: string } | null;
    askToDelete(session: { id: number; title: string } | null): void;
    /** Deletes a session and its conversation for good, open or closed. */
    deleteSession(api: Api, sessionId: number): Promise<void>;
    /**
     * Forgets what this window kept of tasks that were removed, and of their sessions: their
     * conversations, queues and the like. Nothing reads them again.
     */
    forgetTasks(taskIds: number[]): void;

    /** Stops a session and takes it out of the tree. */
    close(api: Api, sessionId: number): Promise<void>;
    report(problem: unknown): void;
    dismissProblem(): void;
}

/** What was heard of a session while its history was being read. */
export interface Reading {
    /** Whether an event arrived that the history may not hold yet, so it is read again. */
    missedEvent: boolean;
    /** The pieces of a reply still being written, which no history holds. */
    streamed: string;
}

/** Sessions whose history is being read, and what arrived for them meanwhile. */
const loading = new Map<number, Reading>();

/**
 * The events the core publishes and never stores. Reading the history again finds none of
 * them, so they are no reason to.
 */
const NEVER_STORED = new Set<AgentEvent["type"]>([
    "text_delta",
    "queue",
    "config_changed",
    "deleted",
    "reordered",
    "tree_changed",
    "account_usage",
    "setup",
    "pull_read",
]);

/**
 * The events that end the reply being written, or take its place: the whole reply, a tool call
 * the agent goes on to, and the end of the turn however it came. The core sends the whole
 * reply before any of the others, so they only find pieces still standing after a turn that
 * was cut short.
 */
const ENDS_REPLY = new Set<AgentEvent["type"]>([
    "text",
    "tool_use",
    "turn_done",
    "exited",
    "user_message",
]);

/**
 * Takes note of an event that arrived while its session's history was being read. Only an
 * event the core stores is a reason to read again: a reply arrives in many pieces that are
 * never stored, and reading again for each would go on for as long as the agent writes. The
 * pieces are kept instead, to stand at the end of the history once it is read.
 */
export function heardWhileReading(reading: Reading, event: AgentEvent): void {
    if (event.type === "text_delta") {
        reading.streamed += event.text;
    }
    if (ENDS_REPLY.has(event.type)) {
        reading.streamed = "";
    }
    if (!NEVER_STORED.has(event.type)) {
        reading.missedEvent = true;
    }
}

/** A history that was just read, with the reply that is still being written at its end. */
export function withStreamed(history: AgentEvent[], streamed: string): AgentEvent[] {
    return streamed === "" ? history : [...history, { type: "text_delta", text: streamed }];
}

/** Reads a session's history, again if an event arrived while reading, so none is lost. */
async function readHistory(api: Api, sessionId: number): Promise<AgentEvent[]> {
    const state: Reading = { missedEvent: false, streamed: "" };
    loading.set(sessionId, state);
    try {
        let events = await api.sessionEvents(sessionId);
        while (state.missedEvent) {
            state.missedEvent = false;
            events = await api.sessionEvents(sessionId);
        }
        return withStreamed(events, state.streamed);
    } finally {
        loading.delete(sessionId);
    }
}

/** A session's events with one more, if its transcript is open. */
export function appendEvent(
    events: Record<number, AgentEvent[]>,
    published: SessionEvent,
): Record<number, AgentEvent[]> {
    const open = events[published.session_id];
    return open ? { ...events, [published.session_id]: withEvent(open, published.event) } : events;
}

/**
 * `events` with one more. The pieces of a reply that is still being written are kept as one
 * growing event at the end. The whole reply takes its place, as does anything else that ends
 * it. Whatever else arrives meanwhile, a new title or a sub-agent's word, has nothing to do
 * with the reply and goes in before it, so the text written so far stays on screen.
 */
export function withEvent(events: AgentEvent[], event: AgentEvent): AgentEvent[] {
    const last = events.at(-1);
    if (last?.type !== "text_delta") {
        return [...events, event];
    }
    const earlier = events.slice(0, -1);
    if (event.type === "text_delta") {
        return [...earlier, { ...event, text: last.text + event.text }];
    }
    return ENDS_REPLY.has(event.type) ? [...earlier, event] : [...earlier, event, last];
}

/** The sub-agents at work after an event, a new record only if the event changed them. */
function withSubAgents(
    all: Record<number, SubAgent[]>,
    published: SessionEvent,
): Record<number, SubAgent[]> {
    const before = all[published.session_id] ?? NO_SUB_AGENTS;
    const after = atWork(before, published.event);
    return after === before ? all : { ...all, [published.session_id]: after };
}

const NO_SUB_AGENTS: SubAgent[] = [];

/** The edit counts after an event: one more for the task of a session that may have edited. */
function countEdit(state: State, published: SessionEvent): Record<number, number> {
    const taskId = taskIdOf(state.projects, published.session_id);
    if (taskId === undefined || !mayHaveEdited(published.event)) {
        return state.edits;
    }
    return { ...state.edits, [taskId]: (state.edits[taskId] ?? 0) + 1 };
}

const PANEL_OPEN = "swarmed.panel-open";
const STATUS_OPEN = "swarmed.status-open";
const FOLDED = "swarmed.folded-projects";
const FOLDED_TASKS = "swarmed.folded-tasks";
const USAGE_DISMISSED = "swarmed.usage-dismissed";

/** The usage notice's dismissal as this machine kept it; `null` for none, or none readable. */
function readUsageDismissed(): Dismissed | null {
    try {
        return readDismissed(JSON.parse(localStorage.getItem(USAGE_DISMISSED) ?? "null"));
    } catch {
        return null;
    }
}

/** A list of ids remembered on this machine; empty if nothing usable is stored. */
function readIds(key: string): number[] {
    try {
        const stored: unknown = JSON.parse(localStorage.getItem(key) ?? "[]");
        return Array.isArray(stored) ? stored.filter((id) => typeof id === "number") : [];
    } catch {
        return [];
    }
}

function writeStored(key: string, value: string) {
    try {
        localStorage.setItem(key, value);
    } catch {
        // Without storage the choice just lasts until the window closes.
    }
}

/** A yes or no remembered on this machine; `fallback` if nothing, or nothing readable, is stored. */
function readFlag(key: string, fallback: boolean): boolean {
    try {
        const stored = localStorage.getItem(key);
        return stored === null ? fallback : stored === "true";
    } catch {
        return fallback;
    }
}

function writeFlag(key: string, value: boolean) {
    try {
        localStorage.setItem(key, String(value));
    } catch {
        // Without storage the choice just lasts until the window closes.
    }
}

/** Runs an action on a session; if it fails, the reason is shown instead of thrown. */
async function reporting(action: () => Promise<void>) {
    try {
        await action();
    } catch (error) {
        useStore.getState().report(error);
    }
}

/** `record` without the entries of `ids`; the same object if it had none of them. */
function without<K extends string | number, T>(record: Record<K, T>, ids: K[]): Record<K, T> {
    if (!ids.some((id) => id in record)) {
        return record;
    }
    const kept = { ...record };
    for (const id of ids) {
        delete kept[id];
    }
    return kept;
}

/** `record` with `id` set to `value`, or without `id` for `null`. */
function withEntry<K extends string | number, T>(
    record: Record<K, T>,
    id: K,
    value: T | null,
): Record<K, T> {
    return value === null ? without(record, [id]) : { ...record, [id]: value };
}

/** The commit drafts with task `taskId`'s set to `message`; an empty one is not kept. */
function withCommitDraft(
    drafts: Record<number, string>,
    taskId: number,
    message: string,
): Record<number, string> {
    return withEntry(drafts, taskId, message === "" ? null : message);
}

/**
 * Forgets everything kept of sessions `sessionIds` and tasks `taskIds`, which are gone: a
 * deleted session's conversation must not stay behind, to be read by mistake or to sit in
 * memory for nothing.
 */
function forgotten(state: State, sessionIds: number[], taskIds: number[]): Partial<State> {
    for (const id of sessionIds) {
        loading.delete(id);
    }
    forgetImages(sessionIds);
    const of = <T extends { sessionId: number }>(held: T | null): T | null =>
        held !== null && sessionIds.includes(held.sessionId) ? null : held;
    return {
        // Left on screen with its conversation forgotten, it would show as loading for good.
        ...leaving(state, sessionIds),
        events: without(state.events, sessionIds),
        subAgents: without(state.subAgents, sessionIds),
        turnBegan: without(state.turnBegan, sessionIds),
        claudeTerminals: without(state.claudeTerminals, sessionIds),
        queues: without(state.queues, sessionIds),
        drafts: without(state.drafts, sessionIds),
        recalls: without(state.recalls, sessionIds),
        scrolls: withoutScrolls(state.scrolls, sessionIds),
        subView: of(state.subView),
        terminalCommand: of(state.terminalCommand),
        edits: without(state.edits, taskIds),
        stackChanges: without(state.stackChanges, taskIds),
        setups: without(state.setups, taskIds),
        commitDrafts: without(state.commitDrafts, taskIds),
        reviews: without(state.reviews, taskIds),
        pulls: without(state.pulls, taskIds),
        foldedTasks: withoutIds(state.foldedTasks, taskIds),
    };
}

/**
 * What changes when sessions `sessionIds` go, if the one on screen is among them: the centre
 * moves to the new-task view of `project`, by default the repository the session was in. Every
 * way a session goes comes through here, before anything of it is forgotten, so the centre
 * never holds on to a session that is not there.
 */
function leaving(state: State, sessionIds: number[], project?: string | null): Partial<State> {
    const selected = state.selectedSessionId;
    if (selected === null || !sessionIds.includes(selected)) {
        return {};
    }
    const session = findSession(state.projects, selected);
    const home = session && findSessionHome(state.projects, session);
    return {
        selectedSessionId: null,
        subView: null,
        // A repository that is gone by then is passed over for the first one left.
        draftProject: project === undefined ? (home?.project.name ?? null) : project,
    };
}

/** What changes when a session is closed: it leaves the tree, and the centre if it was there. */
function closed(state: State, sessionId: number): Partial<State> {
    // Put back, it reads its images again; put away, they are not held for it.
    forgetImages([sessionId]);
    return {
        ...leaving(state, [sessionId]),
        // Asked about it, the question goes with it; asked about another, that stays.
        closing: state.closing === sessionId ? null : state.closing,
        projects: withoutSession(state.projects, sessionId),
        // Put away, it cannot be written to; and its task can go while it is put away.
        drafts: without(state.drafts, [sessionId]),
        recalls: without(state.recalls, [sessionId]),
        scrolls: withoutScrolls(state.scrolls, [sessionId]),
    };
}

/** What changes when the tree is read again: a session on screen that is not in it has gone. */
function readAgain(state: State, projects: Project[]): Partial<State> {
    const selected = state.selectedSessionId;
    const gone = selected !== null && !findSession(projects, selected);
    return {
        projects,
        load: "ready",
        pulls: pullsOf(projects),
        newTaskDrafts: listedOnly(state.newTaskDrafts, projects),
        newTaskIssues: listedOnly(state.newTaskIssues, projects),
        foldedTasks: stillFolded(state.foldedTasks, projects),
        ...(gone ? leaving(state, [selected]) : {}),
    };
}

/**
 * What keeping a transcript's spot changes: its spot kept, unless its session is gone. A
 * transcript goes after its session when that is closed or forgotten, and its spot would
 * outlive it.
 */
function keptScroll(
    state: State,
    sessionId: number,
    key: string,
    spot: ScrollSpot,
): Partial<State> {
    const there = state.events[sessionId] !== undefined && findSession(state.projects, sessionId);
    return there ? { scrolls: { ...state.scrolls, [key]: spot } } : {};
}

/** `recalls` with session `sessionId`'s set to `browsing`, or dropped when that is `null`. */
function withRecall(
    recalls: Record<number, Browsing>,
    sessionId: number,
    browsing: Browsing | null,
): Record<number, Browsing> {
    return browsing === null
        ? without(recalls, [sessionId])
        : { ...recalls, [sessionId]: browsing };
}

/** The new-task drafts of the repositories still listed: a removed one's goes with it. */
/** Each repository's issue for its new task, with `project`'s now `link`, or none for `null`. */
function withIssue(
    links: Record<string, IssueLink>,
    project: string,
    link: IssueLink | null,
): Record<string, IssueLink> {
    if (link === null) {
        const { [project]: _gone, ...rest } = links;
        return rest;
    }
    return { ...links, [project]: link };
}

function listedOnly<T>(drafts: Record<string, T>, projects: Project[]): Record<string, T> {
    // The chats' draft stays: there is no project to list until the first chat is made.
    const listed = new Set([CHATS, ...projects.map((project) => project.name)]);
    const names = Object.keys(drafts);
    if (names.every((name) => listed.has(name))) {
        return drafts;
    }
    return Object.fromEntries(Object.entries(drafts).filter(([name]) => listed.has(name)));
}

/** What to show when `target` is about to be removed, if the session on screen goes with it. */
function leftForRemoval(state: State, target: RemoveTarget): Partial<State> {
    const project = state.projects.find((candidate) => candidate.name === target.project);
    const tasks = (project?.tasks ?? []).filter(
        (task) => target.task === null || task.name === target.task,
    );
    const sessionIds = tasks.flatMap((task) => task.sessions.map((session) => session.id));
    // A repository on its way out is no place for a new task: the first other one is.
    const other = state.projects.find((candidate) => candidate.name !== target.project);
    return leaving(
        state,
        sessionIds,
        target.task === null ? (other?.name ?? null) : target.project,
    );
}

/** What the window says of what in Swarmed's configuration could not be understood. */
export function configProblemsSaid(problems: string[]): string {
    return `Swarmed works with what it understood of its configuration. It could not understand this: ${problems.join(" ")}`;
}

/** Has the core keep something it checks first; what is wrong with it, or nothing once `taken`. */
async function kept<T>(save: () => Promise<T>, taken: (kept: T) => void) {
    try {
        taken(await save());
        return null;
    } catch (error) {
        return String(error);
    }
}

/** The text of what waits for `sessionId` (a command, a quote), which `forget` then drops. */
function taken(
    waiting: { sessionId: number; text: string } | null,
    sessionId: number,
    forget: () => void,
) {
    if (waiting === null || waiting.sessionId !== sessionId) {
        return null;
    }
    forget();
    return waiting.text;
}

/**
 * The folded tasks once session `sessionId` is opened: its task unfolds to show its row. Not
 * for the session already on screen, which is only read again, after events were missed say:
 * its task is folded because the user folded it.
 */
function foldedOnOpening(state: State, sessionId: number): number[] {
    return state.selectedSessionId === sessionId
        ? state.foldedTasks
        : unfoldedFor(state.foldedTasks, state.projects, sessionId);
}

/** The Done groups unfolded so that the session on screen has its row in view. */
function doneInView(state: State): Partial<State> {
    const id = state.selectedSessionId;
    return id === null ? {} : { doneShown: doneShownFor(state.doneShown, state.projects, id) };
}

/**
 * Puts a task away and reads the tree again; what went wrong on the way is said. The session
 * on screen keeps its row in view: the Done group it went into unfolds.
 */
async function putTaskAway(
    api: Api,
    project: string,
    task: string,
    store: Pick<State, "refresh" | "report"> & { keepInView(): void },
) {
    const passedOver = await api.taskDone(project, task);
    await store.refresh(api);
    store.keepInView();
    if (passedOver.length > 0) {
        store.report(`Put away, though not everything went as set up: ${passedOver.join("; ")}`);
    }
}

export const useStore = create<State>((set, get) => ({
    projects: [],
    load: "loading",
    selectedSessionId: null,
    draftProject: null,
    drafts: {},
    editDraft: (sessionId, change) =>
        set((state) => ({ drafts: withDraft(state.drafts, sessionId, change) })),
    putDraftBack: (sessionId, unsent) =>
        set((state) => {
            const now = state.drafts[sessionId] ?? NO_DRAFT;
            return { drafts: withDraft(state.drafts, sessionId, putBack(now, unsent)) };
        }),
    recalls: {},
    noteRecall: (sessionId, browsing) =>
        set((state) => ({ recalls: withRecall(state.recalls, sessionId, browsing) })),
    scrolls: {},
    keepScroll: (sessionId, key, spot) => set((state) => keptScroll(state, sessionId, key, spot)),
    newTaskDrafts: {},
    editNewTaskDraft: (project, change) =>
        set((state) => ({ newTaskDrafts: withDraft(state.newTaskDrafts, project, change) })),
    newTaskIssues: {},
    linkIssue: (project, link) =>
        set((state) => ({ newTaskIssues: withIssue(state.newTaskIssues, project, link) })),
    startFromIssue: (project, issue) =>
        set((state) => {
            const link = issueLink(issue);
            const draft = state.newTaskDrafts[project] ?? NO_DRAFT;
            const before = state.newTaskIssues[project] ?? null;
            const text = withIssuePrompt(draft.text, before, link.prompt);
            return {
                newTaskDrafts: withDraft(state.newTaskDrafts, project, { text }),
                newTaskIssues: withIssue(state.newTaskIssues, project, link),
                issuesOf: null,
                selectedSessionId: null,
                draftProject: project,
            };
        }),
    issuesOf: null,
    showIssues: (target) => set({ issuesOf: target }),
    events: {},
    subAgents: {},
    subView: null,
    openSub: async (api, sessionId, id) => {
        await get().open(api, sessionId);
        set({ subView: { sessionId, id } });
    },
    closeSub: () => set({ subView: null }),
    edits: {},
    stackChanges: {},
    noteStackChange: (taskId, change) =>
        set((state) => ({ stackChanges: withEntry(state.stackChanges, taskId, change) })),
    commitDrafts: {},
    editCommitDraft: (taskId, message) =>
        set((state) => ({ commitDrafts: withCommitDraft(state.commitDrafts, taskId, message) })),
    turnBegan: {},
    backgroundHeard: 0,
    accountUsage: null,
    hookLines: {},
    forgetHookLine: (key) => set((state) => ({ hookLines: without(state.hookLines, [key]) })),
    setups: {},
    readSetupLog: (api, target, began) => readSetupLogOf(api, target, began),
    creating: {},
    noteCreating: (project, name) =>
        set((state) => ({ creating: withEntry(state.creating, project, name) })),
    problem: null,
    usageDismissed: readUsageDismissed(),
    dismissUsage: (dismissed) => {
        writeStored(USAGE_DISMISSED, JSON.stringify(dismissed));
        set({ usageDismissed: dismissed });
    },
    settings: null,
    panelOpen: readFlag(PANEL_OPEN, true),
    togglePanel: () => {
        const panelOpen = !get().panelOpen;
        writeFlag(PANEL_OPEN, panelOpen);
        set({ panelOpen });
    },
    changeSettings: (api, change) =>
        reporting(async () => {
            const current = get().settings ?? (await api.settings());
            const settings = { ...current, ...change };
            // Shown at once; the app only has to remember it.
            set({ settings });
            await api.setSettings(settings);
        }),
    terminalConfig: null,
    saveTerminalConfig: (api, text) =>
        kept(
            () => api.terminalConfigSave(text),
            (terminalConfig) => set({ terminalConfig }),
        ),
    saveBranchPrefix: (api, prefix) =>
        kept(
            async () => {
                const current = get().settings ?? (await api.settings());
                await api.setSettings({ ...current, branch_prefix: prefix });
                // Read back: the core keeps it without the slashes at its ends.
                return api.settings();
            },
            (settings) => set({ settings }),
        ),
    customLayout: null,
    saveCustomLayout: (api, text) =>
        kept(
            () => api.customLayoutSave(text),
            (customLayout) => set({ customLayout }),
        ),
    folded: readIds(FOLDED),
    toggleFolded: (projectId) => {
        const folded = toggledId(get().folded, projectId);
        writeStored(FOLDED, JSON.stringify(folded));
        set({ folded });
    },
    foldedTasks: readIds(FOLDED_TASKS),
    toggleTaskFolded: (taskId) =>
        set((state) => ({ foldedTasks: toggledId(state.foldedTasks, taskId) })),
    doneShown: [],
    toggleDoneShown: (projectId) =>
        set((state) => ({ doneShown: toggledId(state.doneShown, projectId) })),
    putAway: (api, project, task) =>
        putTaskAway(api, project, task, { ...get(), keepInView: () => set(doneInView) }),
    bringBack: async (api, project, task) => {
        await api.taskUndone(project, task);
        await get().refresh(api);
    },
    statusOpen: readFlag(STATUS_OPEN, true),
    toggleStatus: () => {
        const statusOpen = !get().statusOpen;
        writeFlag(STATUS_OPEN, statusOpen);
        set({ statusOpen });
    },
    terminalOpen: false,
    toggleTerminal: () => set((state) => ({ terminalOpen: !state.terminalOpen })),
    quoteWaiting: null,
    quoteReply: (sessionId, words) =>
        set({ terminalOpen: false, quoteWaiting: { sessionId, text: words } }),
    takeQuote: (sessionId) =>
        taken(get().quoteWaiting, sessionId, () => set({ quoteWaiting: null })),
    terminalCommand: null,
    runInTerminal: (sessionId, text) => {
        get().leaveClaudeTerminal(sessionId);
        set({ terminalOpen: true, terminalCommand: { sessionId, text } });
    },
    claudeTerminals: {},
    openClaudeTerminal: (sessionId, command) =>
        set((state) => ({
            terminalOpen: true,
            claudeTerminals: { ...state.claudeTerminals, [sessionId]: command },
        })),
    leaveClaudeTerminal: (sessionId) =>
        set((state) => {
            const { [sessionId]: _left, ...others } = state.claudeTerminals;
            return { claudeTerminals: others };
        }),
    takeTerminalCommand: (sessionId) =>
        taken(get().terminalCommand, sessionId, () => set({ terminalCommand: null })),
    actionsChanged: 0,
    noteActionsChanged: () => set((state) => ({ actionsChanged: state.actionsChanged + 1 })),
    renameAsked: 0,
    askToRename: () => set((state) => ({ renameAsked: state.renameAsked + 1 })),
    settingsOpen: false,
    settingsTab: "appearance",
    showSettings: (open, tab) =>
        set((state) => ({ settingsOpen: open, settingsTab: tab ?? state.settingsTab })),
    rename: (api, sessionId, title) =>
        reporting(async () => {
            await api.sessionRename(sessionId, title);
        }),
    removing: null,
    askToRemove: (target) => set({ removing: target }),
    removals: [],
    removalBegun: (key) => set((state) => ({ removals: [...state.removals, key] })),
    removalEnded: (key) =>
        set((state) => ({ removals: state.removals.filter((removal) => removal !== key) })),
    leaveForRemoval: (target) => set((state) => leftForRemoval(state, target)),
    markRead: (api, sessionId) => {
        if (!findSession(get().projects, sessionId)?.unread) {
            return;
        }
        set((state) => ({ projects: withRead(state.projects, sessionId) }));
        api.sessionMarkRead(sessionId).catch(get().report);
    },
    agentsVersion: 0,
    agentsChanged: () => set((state) => ({ agentsVersion: state.agentsVersion + 1 })),
    queues: {},
    setQueue: (sessionId, messages) =>
        set((state) => ({ queues: { ...state.queues, [sessionId]: messages } })),
    delegating: null,
    askToDelegate: (target) => set({ delegating: target }),
    environmentsOf: null,
    askForEnvironments: (project) => set({ environmentsOf: project }),
    noting: null,
    askForNote: (target) => set({ noting: target }),
    reviews: {},
    comment: (taskId, comment) =>
        set((state) => ({
            reviews: {
                ...state.reviews,
                [taskId]: withComment(state.reviews[taskId] ?? [], comment),
            },
        })),
    clearReview: (taskId) => set((state) => ({ reviews: { ...state.reviews, [taskId]: [] } })),
    pulls: {},
    loadPull: async (api, project, task, taskId) => {
        const pull = await api.taskPull(project, task).catch(() => undefined);
        if (pull !== undefined) {
            set((state) => ({ pulls: { ...state.pulls, [taskId]: pull } }));
        }
    },
    draftFailure: async (api, project, task, sessionId) => {
        const message = await api.taskPullFailure(project, task);
        set((state) => {
            const typed = state.drafts[sessionId]?.text ?? "";
            const text = withMessage(typed, message);
            return { drafts: withDraft(state.drafts, sessionId, { text }) };
        });
    },
    worktreeMoved: (taskId) =>
        set((state) => ({ edits: { ...state.edits, [taskId]: (state.edits[taskId] ?? 0) + 1 } })),
    cloudDiffOf: null,
    showCloudDiff: (id) => set({ cloudDiffOf: id }),
    comparing: null,
    compare: (comparison) => set({ comparing: comparison }),
    templates: [],
    setTemplates: (templates) => set({ templates }),
    searchOpen: false,
    showSearch: (open) => set({ searchOpen: open }),
    inboxOpen: false,
    showInbox: (open) => set({ inboxOpen: open }),
    banner: null,
    showBanner: (banner) => set({ banner }),
    version: null,
    update: { state: "unknown" },
    closing: null,
    askToClose: (sessionId) => set({ closing: sessionId }),
    deleting: null,
    askToDelete: (session) => set({ deleting: session }),
    refresh: async (api) => {
        try {
            const projects = await api.overview();
            // A session that is gone, with its task or repository, cannot stay in the centre.
            set((state) => readAgain(state, projects));
        } catch (error) {
            set({ load: { error: String(error) } });
        }
    },
    open: async (api, sessionId) => {
        // Opened from anywhere, its row is in view: a folded task unfolds to show it.
        set((state) => ({
            selectedSessionId: sessionId,
            subView: null,
            foldedTasks: foldedOnOpening(state, sessionId),
            // And the Done group it is in, if its task was put away.
            doneShown: doneShownFor(state.doneShown, state.projects, sessionId),
        }));
        get().markRead(api, sessionId);
        if (get().events[sessionId] || loading.has(sessionId)) {
            return;
        }
        try {
            const history = await readHistory(api, sessionId);
            set((state) => ({
                events: { ...state.events, [sessionId]: history },
                subAgents: { ...state.subAgents, [sessionId]: subAgentsIn(history) },
            }));
        } catch (error) {
            get().report(error);
        }
    },
    startNew: (project) => set({ selectedSessionId: null, draftProject: project ?? null }),
    apply: (published) => {
        // A queue is not something that was said: it is kept beside the transcript, not in it.
        if (published.event.type === "queue") {
            return get().setQueue(published.session_id, published.event.messages);
        }
        const reading = loading.get(published.session_id);
        if (reading) {
            heardWhileReading(reading, published.event);
        }
        set((state) => ({
            projects: applySessionEvent(state.projects, published),
            events: appendEvent(state.events, published),
            subAgents: withSubAgents(state.subAgents, published),
            edits: countEdit(state, published),
        }));
    },
    setAgent: (api, sessionId, agent) =>
        reporting(async () => {
            const session = await api.sessionSetAgent(sessionId, agent);
            set((state) => ({ projects: withSession(state.projects, session) }));
        }),
    configure: (api, sessionId, settings) =>
        reporting(async () => {
            const session = await api.sessionConfigure(sessionId, settings);
            set((state) => ({ projects: withSession(state.projects, session) }));
        }),
    close: (api, sessionId) =>
        reporting(async () => {
            await api.sessionClose(sessionId);
            set((state) => closed(state, sessionId));
            // The task now counts one more closed session.
            await get().refresh(api);
        }),
    deleteSession: (api, sessionId) =>
        reporting(async () => {
            await api.sessionDelete(sessionId);
            set((state) => ({
                ...closed(state, sessionId),
                ...forgotten(state, [sessionId], []),
                deleting: null,
            }));
            await get().refresh(api);
        }),
    forgetTasks: (taskIds) =>
        set((state) => {
            const tasks = state.projects.flatMap((project) => project.tasks);
            const gone = tasks.filter((task) => taskIds.includes(task.id));
            const sessionIds = gone.flatMap((task) => task.sessions.map((session) => session.id));
            return forgotten(state, sessionIds, taskIds);
        }),
    report: (problem) => set({ problem: String(problem) }),
    dismissProblem: () => set({ problem: null }),
}));

// Folding, opening a session and a task going all change which tasks are folded. Whichever it
// was, what is left is remembered on this machine from here.
useStore.subscribe((state, before) => {
    if (state.foldedTasks !== before.foldedTasks) {
        writeStored(FOLDED_TASKS, JSON.stringify(state.foldedTasks));
    }
});

/** The window the app is shown in. There is none where the store runs without one, as in tests. */
const inView = typeof window === "undefined" ? null : window;

/**
 * Reads the settings and the kept prompts, at the start and whenever an agent changed them.
 * What of the configuration could not be understood is said, never in the way: the rest is in
 * use.
 */
function readConfiguration(api: Api) {
    // What is the same as before is kept as it was: a file written anywhere has all of it read
    // again, and a terminal is built anew for a configuration that only looks new.
    void reporting(async () => {
        const settings = await api.settings();
        useStore.setState((state) => ({ settings: unlessSame(state.settings, settings) }));
    });
    void reporting(async () => {
        const said = configProblemsSaid(await api.configProblems());
        const problems = said === configProblemsSaid([]) ? "" : said;
        // The same problems are said once: dismissed, they stay so until they change.
        if (problems !== "" && problems !== problemsSaid) {
            useStore.getState().report(problems);
        }
        problemsSaid = problems;
    });
    void reporting(async () => {
        const layout = await api.customLayout();
        useStore.setState((state) => ({ customLayout: unlessSame(state.customLayout, layout) }));
    });
    void reporting(async () => {
        const config = await api.terminalConfig();
        useStore.setState((state) => ({
            terminalConfig: unlessSame(state.terminalConfig, config),
        }));
    });
    void reporting(async () => {
        const templates = await api.templates();
        useStore.setState((state) => ({ templates: unlessSame(state.templates, templates) }));
    });
}

/** The problems with the configuration last said, so that reading it again does not repeat them. */
let problemsSaid = "";

/**
 * A session was deleted, from another window or by a session that started it: nothing of it is
 * left, so it is forgotten here and the tree read again. Whether the event was that.
 */
function heardOfDeletion(api: Api, event: SessionEvent): boolean {
    if (event.event.type !== "deleted") {
        return false;
    }
    useStore.setState((state) => ({
        ...closed(state, event.session_id),
        ...forgotten(state, [event.session_id], []),
    }));
    void useStore.getState().refresh(api);
    return true;
}

/**
 * A session was closed, from another window or by the session that started it: it leaves the
 * tree at once, and the centre if it was there, without waiting for the tree to be read again.
 * The event is still applied, and the tree read again for the task's count of closed sessions.
 */
function heardOfClosing(event: SessionEvent) {
    if (event.event.type === "closed") {
        useStore.setState((state) => closed(state, event.session_id));
    }
}

/**
 * Something that belongs to no session, which comes with session id 0: repositories, tasks,
 * sessions or cloud tasks put in another order, here or in another window, or changed by
 * another Swarmed process such as the command line, and the tree is read again; the account's
 * usage, which every session shows; or a configuration file changed on disk, and the
 * configuration is read again. Nothing else is done with it. Whether the event was that.
 */
function heardApart(api: Api, event: SessionEvent): boolean {
    if (event.session_id !== 0) {
        return false;
    }
    const heard = event.event;
    if (heard.type === "reordered" || heard.type === "tree_changed") {
        void useStore.getState().refresh(api);
    } else if (heard.type === "account_usage") {
        useStore.setState({ accountUsage: heard.usage });
    } else if (heard.type === "pull_read") {
        const { task_id, pull } = heard;
        useStore.setState((state) => ({ pulls: { ...state.pulls, [task_id]: pull } }));
    } else if (heard.type === "setup") {
        heardOfSetup(api, heard.progress);
    } else if (heard.type === "config_changed") {
        readConfiguration(api);
        useStore.getState().noteActionsChanged();
    }
    return true;
}

/** Reads the end of a task's setup log into the store. Not reading it costs nothing. */
async function readSetupLogOf(
    api: Api,
    { project, task, taskId }: { project: string; task: string; taskId: number },
    began: number | null,
) {
    try {
        const log = await api.taskSetupLog(project, task);
        useStore.setState((state) => ({
            setups: { ...state.setups, [taskId]: readFrom(state.setups[taskId], log, began) },
        }));
    } catch {
        // What it printed shows once it is heard of again.
    }
}

/**
 * Keeps what a task's setup said of itself. When a run begins, and once it has ended, the tree
 * is read again: the task is being set up, or is set up now or failed, and its first message
 * may have gone.
 */
function heardOfSetup(api: Api, progress: SetupProgress) {
    const before = useStore.getState().setups[progress.task_id];
    useStore.setState((state) => ({
        setups: { ...state.setups, [progress.task_id]: heard(before, progress) },
    }));
    // A run the tree does not know of yet, such as one asked for from the panel.
    const begun = before?.began !== progress.began;
    if (begun || progress.ended !== null) {
        void useStore.getState().refresh(api);
    }
}

/**
 * Reads the account's usage, unless an event has said what it is meanwhile: what was read can
 * be older than what that said.
 */
function readAccountUsage(api: Api) {
    void reporting(async () => {
        const usage = await api.accountUsage();
        if (useStore.getState().accountUsage === null) {
            useStore.setState({ accountUsage: usage });
        }
    });
}

/**
 * Reads the tree again for an event that changes what it holds: one from a session it does
 * not know, which an agent started, the closing of one by an agent or another window, or a
 * command such as `/model` having set something of a session's.
 */
function readTreeIfChanged(api: Api, event: SessionEvent) {
    const unknown = !findSession(useStore.getState().projects, event.session_id);
    const changes = event.event.type === "closed" || event.event.type === "command_answered";
    if (unknown || changes) {
        void useStore.getState().refresh(api);
    }
}

/** An agent changed Swarmed's configuration from a session: what was read of it is out of date. */
function heardOfConfiguration(api: Api, event: SessionEvent) {
    if (event.event.type === "config_changed") {
        readConfiguration(api);
        useStore.getState().noteActionsChanged();
    }
}

/** Shows what one of Swarmed's own items in the menu bar stands for (`menu.rs`). */
export function menuChosen(item: string) {
    const { showBanner, showSettings } = useStore.getState();
    if (item === "about" || item === "shortcuts") {
        showBanner(item);
    } else if (item === "settings") {
        showSettings(true);
    }
}

/**
 * What a hook's or an installer's output is kept under: the repository and the task it was
 * printed for, or for an agent being installed, no repository and the agent's id.
 */
export function hookKey(project: string, task: string): string {
    return `${project}/${task}`;
}

/** Loads the tree and keeps it in step with the sessions. Returns what stops that again. */
export function connect(api: Api): () => void {
    const { refresh, apply } = useStore.getState();
    void refresh(api);
    problemsSaid = "";
    readConfiguration(api);
    void reporting(async () => useStore.setState({ version: await api.version() }));
    void reporting(async () => useStore.setState({ update: await api.updateState() }));
    readAccountUsage(api);
    // After dropped events nothing on screen can be trusted: forget the transcripts and reload.
    const reload = () => {
        const { selectedSessionId, open } = useStore.getState();
        useStore.setState({ events: {}, accountUsage: null });
        readAccountUsage(api);
        void refresh(api);
        if (selectedSessionId !== null) {
            void open(api, selectedSessionId);
        }
    };
    // Fresh closures on every call: React mounts twice in development, and an `Api` that tells
    // listeners apart by identity would let the first mount's cleanup remove the second's.
    // An answer in the session on screen is read as it arrives, if the window is being looked
    // at; otherwise when the window is.
    const readIfSeen = () => {
        const { selectedSessionId, markRead } = useStore.getState();
        // A window put away in the Dock can still hold the focus; nobody reads it there.
        const seen = inView?.document.hasFocus() && inView.document.visibilityState === "visible";
        if (selectedSessionId !== null && seen) {
            markRead(api, selectedSessionId);
        }
    };
    inView?.addEventListener("focus", readIfSeen);
    const subscriptions = [
        api.onSessionEvent((event) => {
            if (heardApart(api, event) || heardOfDeletion(api, event)) {
                return;
            }
            readTreeIfChanged(api, event);
            heardOfClosing(event);
            apply(event);
            const turnBegan = turnsBegan(useStore.getState().turnBegan, event, Date.now());
            if (turnBegan !== useStore.getState().turnBegan) {
                useStore.setState({ turnBegan });
            }
            heardOfConfiguration(api, event);
            if (event.event.type === "background") {
                useStore.setState((state) => ({ backgroundHeard: state.backgroundHeard + 1 }));
            }
            readIfSeen();
        }),
        api.onNoticeOpened((sessionId) => void useStore.getState().open(api, sessionId)),
        api.onEventsLagged(() => reload()),
        api.onHookOutput((output) =>
            useStore.setState((state) => ({
                hookLines: {
                    ...state.hookLines,
                    [hookKey(output.project, output.task)]: output.line,
                },
            })),
        ),
        api.onUpdateState((update) => useStore.setState({ update })),
        api.onMenu(menuChosen),
    ];
    return () => {
        inView?.removeEventListener("focus", readIfSeen);
        for (const subscription of subscriptions) {
            void subscription.then((unsubscribe) => unsubscribe());
        }
    };
}
