import { useEffect } from "react";
import { addProject, newSession } from "./actions";
import { api } from "./api";
import type { Project } from "./api/types";
import { afterPlan, CLAUDE } from "./components/options";
import { type HotkeyId, matchHotkey, stepThrough } from "./hotkeys";
import { useStore } from "./store";
import { toItems } from "./transcript";
import { findSession, findSessionHome } from "./tree";

/** Text sizes the shortcuts step through, in percent; the same range the settings allow. */
const TEXT_STEP = 10;
const TEXT_RANGE = { min: 80, max: 150 };

/** The ids of the sessions in the order the sidebar lists them. */
function sessionIds(projects: Project[]): number[] {
    return projects
        .flatMap((project) => project.tasks)
        .filter((task) => !task.archived)
        .flatMap((task) => task.sessions)
        .map((session) => session.id);
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

/** The request the open session is waiting on, if it is waiting on one. */
function pendingRequest() {
    const { selectedSessionId: id, events } = useStore.getState();
    const items = id === null ? [] : toItems(events[id] ?? []);
    const request = items.findLast((item) => item.kind === "permission");
    return id !== null && request?.kind === "permission" ? { id, request } : null;
}

function answer(allow: boolean) {
    const pending = pendingRequest();
    if (!pending) {
        return;
    }
    const { id, request } = pending;
    const { report } = useStore.getState();
    const sent = allow
        ? api.sessionAllow(id, request.requestId)
        : api.sessionDeny(id, request.requestId, "The user declined this action.");
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
    useStore.getState().showSettings(true, "shortcuts");
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
    "focus-message": focusMessage,
    "toggle-terminal": () => current() && useStore.getState().toggleTerminal(),
    "toggle-plan": togglePlan,
    allow: () => answer(true),
    deny: () => answer(false),
    "toggle-panel": () => useStore.getState().togglePanel(),
    "open-editor": () => openTask("editor"),
    "open-browser": () => openTask("browser"),
    "open-note": () => {
        const open = current();
        if (open) {
            useStore.getState().askForNote({ project: open.project.name, task: open.task.name });
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

/** Listens for the keyboard shortcuts for as long as the app is on screen. */
export function useHotkeys() {
    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            const id = matchHotkey(event);
            if (id === null || event.isComposing) {
                return;
            }
            if (document.querySelector("dialog[open]") && !OVER_A_DIALOG.has(id)) {
                return;
            }
            // Without a request to answer, these keys keep what they mean in a text field.
            if (ANSWERS.has(id) && !pendingRequest()) {
                return;
            }
            event.preventDefault();
            run(id);
        };
        window.addEventListener("keydown", onKeyDown);
        return () => window.removeEventListener("keydown", onKeyDown);
    }, []);
}
