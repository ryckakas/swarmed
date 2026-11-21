import { create } from "zustand";
import type { Api } from "./api/client";
import type {
    AgentEvent,
    CustomLayout,
    NoteTarget,
    Project,
    Pull,
    QueuedMessage,
    RemoveTarget,
    SessionEvent,
    SessionSettings,
    Settings,
    Template,
    TerminalConfig,
} from "./api/types";
import type { CloudTarget } from "./cloud";
import type { Comparison } from "./components/CompareDialog";
import { type ReviewComment, withComment } from "./review";
import { atWork, atWorkIn, type SubAgent } from "./subagents";
import {
    applySessionEvent,
    findSession,
    mayHaveEdited,
    taskIdOf,
    toggledId,
    withoutSession,
    withRead,
    withSession,
} from "./tree";
import { turnsBegan } from "./turns";

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
    ["about", "About"],
] as const;

export type SettingsTab = (typeof SETTINGS_TABS)[number][0];

/** What can be shown over the window, from the menu bar or a shortcut, and closed again. */
export type Banner = "about" | "shortcuts";

interface State {
    projects: Project[];
    /** `loading` until the first read finished; an error message if it failed. */
    load: "loading" | "ready" | { error: string };
    selectedSessionId: number | null;
    /** The repository preselected on the new-task screen, shown when no session is selected. */
    draftProject: string | null;
    /** The events of every session whose transcript has been opened. */
    events: Record<number, AgentEvent[]>;
    /** Per task id, a count that goes up whenever its agents may have changed files. */
    edits: Record<number, number>;
    /** The sub-agents each session's agent has at work, as far as this window has heard. */
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
    /** The latest line a hook printed while a task is being set up or removed. */
    hookLine: string | null;
    /** The last thing that went wrong when acting on a session, shown until dismissed. */
    problem: string | null;
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
    /** Whether the status line under the message box is shown, or folded away. */
    statusOpen: boolean;
    toggleStatus(): void;
    /** Whether the terminal is shown in place of the message box. */
    terminalOpen: boolean;
    toggleTerminal(): void;
    /** A command waiting to be typed into a session's terminal, once that is on screen. */
    terminalCommand: { sessionId: number; text: string } | null;
    /** Shows the terminal and has `text` typed into the terminal of `sessionId`. */
    runInTerminal(sessionId: number, text: string): void;
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
    /** Each task's pull request by the task's id, as last heard; `null` for none. */
    pulls: Record<number, Pull | null>;
    /** Asks again for a task's pull request. Not hearing back changes nothing. */
    loadPull(api: Api, project: string, task: string, taskId: number): Promise<void>;
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
    /** The session the user asked to close, until they confirm or change their mind. */
    closing: number | null;
    /** Asks before closing: `null` withdraws the question. */
    askToClose(sessionId: number | null): void;
    /** A closed session the user is asked about deleting for good, if any. */
    deleting: { id: number; title: string } | null;
    askToDelete(session: { id: number; title: string } | null): void;
    /** Deletes a session and its conversation for good, open or closed. */
    deleteSession(api: Api, sessionId: number): Promise<void>;

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

/** What changes when a session is closed: it leaves the tree, and the centre if it was there. */
function closed(state: State, sessionId: number): Partial<State> {
    const selected = state.selectedSessionId;
    return {
        closing: null,
        projects: withoutSession(state.projects, sessionId),
        selectedSessionId: selected === sessionId ? null : selected,
    };
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

/** The text of a waiting command meant for the terminal of `sessionId`, which `forget` then drops. */
function taken(waiting: State["terminalCommand"], sessionId: number, forget: () => void) {
    if (waiting === null || waiting.sessionId !== sessionId) {
        return null;
    }
    forget();
    return waiting.text;
}

export const useStore = create<State>((set, get) => ({
    projects: [],
    load: "loading",
    selectedSessionId: null,
    draftProject: null,
    events: {},
    subAgents: {},
    subView: null,
    openSub: async (api, sessionId, id) => {
        await get().open(api, sessionId);
        set({ subView: { sessionId, id } });
    },
    closeSub: () => set({ subView: null }),
    edits: {},
    turnBegan: {},
    backgroundHeard: 0,
    hookLine: null,
    problem: null,
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
    statusOpen: readFlag(STATUS_OPEN, true),
    toggleStatus: () => {
        const statusOpen = !get().statusOpen;
        writeFlag(STATUS_OPEN, statusOpen);
        set({ statusOpen });
    },
    terminalOpen: false,
    toggleTerminal: () => set((state) => ({ terminalOpen: !state.terminalOpen })),
    terminalCommand: null,
    runInTerminal: (sessionId, text) =>
        set({ terminalOpen: true, terminalCommand: { sessionId, text } }),
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
    closing: null,
    askToClose: (sessionId) => set({ closing: sessionId }),
    deleting: null,
    askToDelete: (session) => set({ deleting: session }),
    refresh: async (api) => {
        try {
            const projects = await api.overview();
            // A session that is gone, with its task or repository, cannot stay in the centre.
            const selected = findSession(projects, get().selectedSessionId)?.id ?? null;
            set({ projects, load: "ready", selectedSessionId: selected });
        } catch (error) {
            set({ load: { error: String(error) } });
        }
    },
    open: async (api, sessionId) => {
        set({ selectedSessionId: sessionId, subView: null });
        get().markRead(api, sessionId);
        if (get().events[sessionId] || loading.has(sessionId)) {
            return;
        }
        try {
            const history = await readHistory(api, sessionId);
            set((state) => ({
                events: { ...state.events, [sessionId]: history },
                subAgents: { ...state.subAgents, [sessionId]: atWorkIn(history) },
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
            set((state) => ({ ...closed(state, sessionId), deleting: null }));
            await get().refresh(api);
        }),
    report: (problem) => set({ problem: String(problem) }),
    dismissProblem: () => set({ problem: null }),
}));

/** The window the app is shown in. There is none where the store runs without one, as in tests. */
const inView = typeof window === "undefined" ? null : window;

/** Reads the settings and the kept prompts, at the start and whenever an agent changed them. */
function readConfiguration(api: Api) {
    void reporting(async () => useStore.setState({ settings: await api.settings() }));
    void reporting(async () => useStore.setState({ customLayout: await api.customLayout() }));
    void reporting(async () => useStore.setState({ terminalConfig: await api.terminalConfig() }));
    void reporting(async () => useStore.setState({ templates: await api.templates() }));
}

/**
 * A session was deleted, from another window or by a session that started it: nothing of it is
 * left, so it is forgotten here and the tree read again. Whether the event was that.
 */
function heardOfDeletion(api: Api, event: SessionEvent): boolean {
    if (event.event.type !== "deleted") {
        return false;
    }
    useStore.setState((state) => closed(state, event.session_id));
    void useStore.getState().refresh(api);
    return true;
}

/**
 * Reads the tree again for an event that changes which sessions it holds: one from a session
 * it does not know, which an agent started, or the closing of one by an agent or another window.
 */
function readTreeIfChanged(api: Api, event: SessionEvent) {
    const unknown = !findSession(useStore.getState().projects, event.session_id);
    if (unknown || event.event.type === "closed") {
        void useStore.getState().refresh(api);
    }
}

/** An agent changed Swarmed's configuration: what was read of it is out of date. */
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

/** Loads the tree and keeps it in step with the sessions. Returns what stops that again. */
export function connect(api: Api): () => void {
    const { refresh, apply } = useStore.getState();
    void refresh(api);
    readConfiguration(api);
    void reporting(async () => useStore.setState({ version: await api.version() }));
    // After dropped events nothing on screen can be trusted: forget the transcripts and reload.
    const reload = () => {
        const { selectedSessionId, open } = useStore.getState();
        useStore.setState({ events: {} });
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
            if (heardOfDeletion(api, event)) {
                return;
            }
            readTreeIfChanged(api, event);
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
        api.onHookOutput((output) => useStore.setState({ hookLine: output.line })),
        api.onMenu(menuChosen),
    ];
    return () => {
        inView?.removeEventListener("focus", readIfSeen);
        for (const subscription of subscriptions) {
            void subscription.then((unsubscribe) => unsubscribe());
        }
    };
}
