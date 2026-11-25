import { useEffect } from "react";
import { addProject, moveFocusedRow, newSession, pinFocusedRow } from "./actions";
import { api } from "./api";
import type { Project } from "./api/types";
import { inSidebarOrder } from "./chats";
import { afterPlan, CLAUDE } from "./components/options";
import { quoteSelected } from "./components/QuoteButton";
import { type HotkeyId, matchHotkey, stepThrough } from "./hotkeys";
import { issuesRepository } from "./issues";
import { type ShortcutRequest, shortcutRequest } from "./pending";
import { useStore } from "./store";
import { toItems } from "./transcript";
import { findSession, findSessionHome, nestedSessions } from "./tree";

/** Text sizes the shortcuts step through, in percent; the same range the settings allow. */
const TEXT_STEP = 10;
const TEXT_RANGE = { min: 80, max: 150 };

/** The ids of the sessions in the order the sidebar lists them. */
function sessionIds(projects: Project[]): number[] {
    return inSidebarOrder(projects)
        .flatMap((project) => project.tasks)
        .filter((task) => !task.archived)
        .flatMap((task) => nestedSessions(task.sessions))
        .map(({ session }) => session.id);
}

function openSession(id: number | null) {
    if (id !== null) {
        void useStore.getState().open(api, id);
    }
}

/** The open session with its task and project, if a session is open. */
function current() {
    const { projects, selectedSessionId } = useStore.getState();
    const session = findSession(projects, selectedSessionId);
    const home = session && findSessionHome(projects, session);
    return session && home ? { session, ...home } : null;
}

function step(by: number) {
    const { projects, selectedSessionId } = useStore.getState();
    openSession(stepThrough(sessionIds(projects), selectedSessionId, by));
}

function nextWaiting() {
    const { projects, selectedSessionId } = useStore.getState();
    const waiting = projects
        .flatMap((project) => project.tasks)
        .flatMap((task) => task.sessions)
        .filter((session) => session.status === "waiting" || session.unread)
        .map((session) => session.id);
    openSession(stepThrough(waiting, selectedSessionId, 1));
}

/** The request of the open session that a shortcut would allow, or with `allow` false deny. */
function pendingRequest(allow: boolean): (ShortcutRequest & { id: number }) | null {
    const { selectedSessionId: id, events } = useStore.getState();
    const request = id === null ? null : shortcutRequest(toItems(events[id] ?? []), allow);
    return id !== null && request ? { id, ...request } : null;
}

function answer(allow: boolean) {
    const pending = pendingRequest(allow);
    if (!pending) {
        return;
    }
    const { id, requestId, refusal } = pending;
    const { report } = useStore.getState();
    const sent = allow ? api.sessionAllow(id, requestId) : api.sessionDeny(id, requestId, refusal);
    sent.catch(report);
}

/** Puts the open session into plan mode, or back to how new sessions start. */
function togglePlan() {
    const open = current();
    if (!open || open.session.agent !== CLAUDE) {
        return;
    }
    const { settings, configure } = useStore.getState();
    const { id, model, effort, supervision: now } = open.session;
    const supervision = now === "plan" ? afterPlan(settings?.default_supervision) : "plan";
    void configure(api, id, { model, effort, supervision });
}

function resize(by: number | null) {
    const { settings, changeSettings } = useStore.getState();
    const now = settings?.text_scale ?? 100;
    const wanted = by === null ? 100 : now + by;
    const text_scale = Math.min(TEXT_RANGE.max, Math.max(TEXT_RANGE.min, wanted));
    void changeSettings(api, { text_scale });
}

function openTask(where: "editor" | "browser") {
    const open = current();
    if (!open) {
        return;
    }
    const { project, task } = open;
    const opened =
        where === "editor"
            ? api.taskOpenEditor(project.name, task.name)
            : api.taskOpenUrl(project.name, task.name);
    opened.catch(useStore.getState().report);
}

function focusMessage() {
    const store = useStore.getState();
    if (store.terminalOpen) {
        store.toggleTerminal();
    }
    // After the message box is back on screen, if the terminal had its place.
    requestAnimationFrame(() => {
        document.querySelector<HTMLTextAreaElement>(".composer-box textarea")?.focus();
    });
}

function showShortcuts() {
    useStore.getState().showBanner("shortcuts");
}

/**
 * Starts picking an element on the page of the task on screen, opening the Browser pane for it,
 * or stops a pick under way.
 */
function pickElement() {
    const { picking, report, toggleBrowser, askToPick } = useStore.getState();
    if (picking) {
        void api.browserPickEnd(picking.task).catch(report);
        return;
    }
    const shown = current();
    if (shown) {
        toggleBrowser(shown.task.id, true);
        askToPick(true);
    }
}

/** What each shortcut does. Those that act on a session do nothing when none is open. */
const ACTIONS: Partial<Record<HotkeyId, () => void>> = {
    "new-task": () => useStore.getState().startNew(),
    "new-session": () => {
        const open = current();
        if (open) {
            void newSession(open.project.name, open.task.name);
        }
    },
    "add-repository": () => void addProject(),
    "close-session": () => {
        const open = current();
        if (open) {
            useStore.getState().askToClose(open.session.id);
        }
    },
    "rename-session": () => current() && useStore.getState().askToRename(),
    "previous-session": () => step(-1),
    "next-session": () => step(1),
    "next-waiting": nextWaiting,
    "move-up": () => void moveFocusedRow(-1),
    "move-down": () => void moveFocusedRow(1),
    pin: () => void pinFocusedRow(),
    search: () => useStore.getState().showSearch(!useStore.getState().searchOpen),
    inbox: () => useStore.getState().showInbox(!useStore.getState().inboxOpen),
    "focus-message": focusMessage,
    "toggle-terminal": () => current() && useStore.getState().toggleTerminal(),
    "toggle-plan": togglePlan,
    allow: () => answer(true),
    deny: () => answer(false),
    quote: () => void quoteSelected(),
    "toggle-panel": () => useStore.getState().togglePanel(),
    "open-editor": () => openTask("editor"),
    "open-browser": () => openTask("browser"),
    "pick-element": pickElement,
    "open-note": () => {
        const open = current();
        if (open) {
            useStore.getState().askForNote({ project: open.project.name, task: open.task.name });
        }
    },
    issues: () => {
        const { projects, selectedSessionId, draftProject, showIssues } = useStore.getState();
        const project = issuesRepository(projects, selectedSessionId, draftProject);
        if (project !== null) {
            showIssues({ project, issue: null });
        }
    },
    "toggle-status": () => useStore.getState().toggleStatus(),
    settings: () => useStore.getState().showSettings(!useStore.getState().settingsOpen),
    shortcuts: showShortcuts,
    "text-larger": () => resize(TEXT_STEP),
    "text-smaller": () => resize(-TEXT_STEP),
    "text-reset": () => resize(null),
};

function run(id: HotkeyId) {
    const nth = /^session-(\d)$/.exec(id);
    if (nth) {
        const ids = sessionIds(useStore.getState().projects);
        return openSession(ids[Number(nth[1]) - 1] ?? null);
    }
    ACTIONS[id]?.();
}

/** Shortcuts that still work while a dialog is open; the rest wait until it is closed. */
const OVER_A_DIALOG = new Set<HotkeyId>(["settings", "text-larger", "text-smaller", "text-reset"]);

const ANSWERS = new Set<HotkeyId>(["allow", "deny"]);

/** Shortcuts the message box acts on itself, where what is typed is: left to it here. */
const IN_THE_MESSAGE_BOX = new Set<HotkeyId>(["send-now"]);

/**
 * Whether a key press is a shortcut for the window to act on: not one something else acted on
 * already, nor one the message box acts on itself, and over a dialog only those that work there.
 */
function forTheWindow(id: HotkeyId | null, event: KeyboardEvent): id is HotkeyId {
    if (id === null || event.isComposing || event.defaultPrevented || IN_THE_MESSAGE_BOX.has(id)) {
        return false;
    }
    return !document.querySelector("dialog[open]") || OVER_A_DIALOG.has(id);
}

/** Listens for the keyboard shortcuts for as long as the app is on screen. */
export function useHotkeys() {
    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            const id = matchHotkey(event);
            if (!forTheWindow(id, event)) {
                return;
            }
            if (ANSWERS.has(id)) {
                // Without a request to answer, these keys keep what they mean in a text field.
                if (!pendingRequest(id === "allow")) {
                    return;
                }
                // With one, the key is the answer and nothing else: the message box, which has
                // the keyboard while a request waits, would send its message on the same key.
                event.stopPropagation();
            }
            event.preventDefault();
            run(id);
        };
        // Heard on the way down, before whatever is typed into: only so can an answer keep its
        // key from the message box, and the terminal cannot take a shortcut for its shell.
        window.addEventListener("keydown", onKeyDown, true);
        return () => window.removeEventListener("keydown", onKeyDown, true);
    }, []);
}
