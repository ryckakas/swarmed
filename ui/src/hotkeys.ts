// The keyboard shortcuts: what they are, how a key press is matched to one, and how they are
// written for people. Pure, so it can be tested; what each one does lives in `useHotkeys`.

export type HotkeyId =
    | "new-task"
    | "new-session"
    | "add-repository"
    | "close-session"
    | "rename-session"
    | "previous-session"
    | "next-session"
    | "next-waiting"
    | "session-1"
    | "session-2"
    | "session-3"
    | "session-4"
    | "session-5"
    | "session-6"
    | "session-7"
    | "session-8"
    | "session-9"
    | "focus-message"
    | "toggle-terminal"
    | "allow"
    | "deny"
    | "toggle-panel"
    | "open-editor"
    | "open-browser"
    | "toggle-status"
    | "settings"
    | "shortcuts"
    | "text-larger"
    | "text-smaller"
    | "text-reset";

type Modifier = "cmd" | "ctrl" | "shift" | "alt";

export interface Hotkey {
    id: HotkeyId;
    /** Modifiers, then the key as `KeyboardEvent.key` gives it in lower case: "cmd+shift+w". */
    keys: string;
    /** What it does, as the shortcuts list says it. */
    label: string;
    group: "Sessions" | "In a session" | "Task" | "Window";
}

const session = (n: number): Hotkey => ({
    id: `session-${n}` as HotkeyId,
    keys: `cmd+${n}`,
    label: `Go to session ${n} in the sidebar`,
    group: "Sessions",
});

/**
 * Every shortcut. Combinations macOS or the window already use (⌘W, ⌘Q, ⌘M, ⌘H, copy and
 * paste) are left alone.
 */
export const HOTKEYS: Hotkey[] = [
    { id: "new-task", keys: "cmd+n", label: "New task", group: "Sessions" },
    { id: "new-session", keys: "cmd+t", label: "New session in this task", group: "Sessions" },
    { id: "add-repository", keys: "cmd+o", label: "Add a repository", group: "Sessions" },
    { id: "previous-session", keys: "cmd+[", label: "Previous session", group: "Sessions" },
    { id: "next-session", keys: "cmd+]", label: "Next session", group: "Sessions" },
    {
        id: "next-waiting",
        keys: "cmd+shift+a",
        label: "Go to the next session that needs you",
        group: "Sessions",
    },
    ...[1, 2, 3, 4, 5, 6, 7, 8, 9].map(session),
    { id: "focus-message", keys: "cmd+l", label: "Go to the message box", group: "In a session" },
    {
        id: "toggle-terminal",
        keys: "ctrl+`",
        label: "Switch between message box and terminal",
        group: "In a session",
    },
    { id: "allow", keys: "cmd+enter", label: "Allow the pending request", group: "In a session" },
    { id: "deny", keys: "cmd+backspace", label: "Deny the pending request", group: "In a session" },
    {
        id: "rename-session",
        keys: "cmd+shift+r",
        label: "Rename the session",
        group: "In a session",
    },
    { id: "close-session", keys: "cmd+shift+w", label: "Close the session", group: "In a session" },
    { id: "toggle-panel", keys: "cmd+j", label: "Show or hide the task panel", group: "Task" },
    { id: "open-editor", keys: "cmd+e", label: "Open the task in the editor", group: "Task" },
    {
        id: "open-browser",
        keys: "cmd+shift+b",
        label: "Open the task in the browser",
        group: "Task",
    },
    {
        id: "toggle-status",
        keys: "cmd+shift+s",
        label: "Show or hide the status line",
        group: "Window",
    },
    { id: "text-larger", keys: "cmd+=", label: "Larger text", group: "Window" },
    { id: "text-smaller", keys: "cmd+-", label: "Smaller text", group: "Window" },
    { id: "text-reset", keys: "cmd+0", label: "Normal text size", group: "Window" },
    { id: "settings", keys: "cmd+,", label: "Settings", group: "Window" },
    { id: "shortcuts", keys: "cmd+/", label: "Show these shortcuts", group: "Window" },
];

/** The shortcuts as the list shows them: the nine "go to session" ones as a single line. */
export function listedHotkeys(): { keys: string; label: string; group: Hotkey["group"] }[] {
    const numbered = /^session-\d$/;
    const others = HOTKEYS.filter((hotkey) => !numbered.test(hotkey.id));
    const first = HOTKEYS.findIndex((hotkey) => numbered.test(hotkey.id));
    const nine = { keys: "cmd+1…9", label: "Go to session 1 to 9 in the sidebar" };
    return others.toSpliced(first, 0, { ...nine, group: "Sessions" });
}

/** Shortcuts that are handled where they happen, listed so the map is complete. */
export const LOCAL_HOTKEYS: { keys: string; label: string }[] = [
    { keys: "enter", label: "Send the message" },
    { keys: "shift+enter", label: "New line in the message" },
    { keys: "escape", label: "Stop the agent's turn" },
    { keys: "/", label: "Suggest commands and skills, at the start of a message" },
    { keys: "arrowup", label: "Previous session, with a sidebar row focused" },
    { keys: "arrowdown", label: "Next session, with a sidebar row focused" },
];

/** What of a key press decides which shortcut it is. */
export interface KeyPress {
    key: string;
    metaKey: boolean;
    ctrlKey: boolean;
    shiftKey: boolean;
    altKey: boolean;
}

/** A key press in the form shortcuts are written in. */
function written(press: KeyPress): string {
    const modifiers: [boolean, Modifier][] = [
        [press.metaKey, "cmd"],
        [press.ctrlKey, "ctrl"],
        [press.shiftKey, "shift"],
        [press.altKey, "alt"],
    ];
    const held = modifiers.filter(([down]) => down).map(([, name]) => name);
    // With Shift held some keyboards report the shifted character; the unshifted one is meant.
    const key = { "+": "=", _: "-", "{": "[", "}": "]", "?": "/", "<": ",", "~": "`" }[press.key];
    return [...held, (key ?? press.key).toLowerCase()].join("+");
}

/** The shortcut a key press is, if it is one. */
export function matchHotkey(press: KeyPress): HotkeyId | null {
    const pressed = written(press);
    return HOTKEYS.find((hotkey) => hotkey.keys === pressed)?.id ?? null;
}

const SYMBOLS: Record<string, string> = {
    cmd: "⌘",
    ctrl: "⌃",
    shift: "⇧",
    alt: "⌥",
    enter: "↩",
    backspace: "⌫",
    escape: "Esc",
    arrowup: "↑",
    arrowdown: "↓",
};

/** A shortcut as macOS writes it, key by key: ["⌘", "⇧", "W"]. */
export function keyCaps(keys: string): string[] {
    return keys.split("+").map((key) => SYMBOLS[key] ?? key.toUpperCase());
}

/** The id one step from `current` in `ids`, wrapping round; the first if `current` is not there. */
export function stepThrough(ids: number[], current: number | null, step: number): number | null {
    if (ids.length === 0) {
        return null;
    }
    const at = current === null ? -1 : ids.indexOf(current);
    const next = at === -1 ? 0 : (at + step + ids.length) % ids.length;
    return ids[next] ?? null;
}
