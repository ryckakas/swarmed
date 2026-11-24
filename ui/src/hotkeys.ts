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
    | "move-up"
    | "move-down"
    | "session-1"
    | "session-2"
    | "session-3"
    | "session-4"
    | "session-5"
    | "session-6"
    | "session-7"
    | "session-8"
    | "session-9"
    | "inbox"
    | "search"
    | "focus-message"
    | "toggle-terminal"
    | "toggle-plan"
    | "allow"
    | "deny"
    | "send-now"
    | "toggle-panel"
    | "open-editor"
    | "open-browser"
    | "open-note"
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
        id: "move-up",
        // Key combinations, which gitleaks' rule for keys takes for secrets.
        keys: "cmd+alt+arrowup", // gitleaks:allow
        label: "Move up in the sidebar: the focused row, or the open session",
        group: "Sessions",
    },
    {
        id: "move-down",
        keys: "cmd+alt+arrowdown", // gitleaks:allow
        label: "Move down in the sidebar: the focused row, or the open session",
        group: "Sessions",
    },
    {
        id: "next-waiting",
        keys: "cmd+shift+a",
        label: "Go to the next session that needs you or has an unread reply",
        group: "Sessions",
    },
    { id: "inbox", keys: "cmd+i", label: "List everything that waits for you", group: "Sessions" },
    {
        id: "search",
        keys: "cmd+shift+f",
        label: "Search what was said, in any session",
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
    {
        id: "toggle-plan",
        keys: "cmd+shift+p",
        label: "Switch plan mode on or off",
        group: "In a session",
    },
    { id: "allow", keys: "cmd+enter", label: "Allow the pending request", group: "In a session" },
    { id: "deny", keys: "cmd+backspace", label: "Deny the pending request", group: "In a session" },
    {
        id: "send-now",
        // A key combination, which gitleaks' rule for keys takes for a secret.
        keys: "cmd+shift+enter", // gitleaks:allow
        label: "Stop the agent and send what is typed now",
        group: "In a session",
    },
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
    { id: "open-note", keys: "cmd+shift+n", label: "Open the task's note", group: "Task" },
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

/** A shortcut as the list shows it. */
export type ListedHotkey = Pick<Hotkey, "keys" | "label" | "group">;

/** The shortcuts as the list shows them: the nine "go to session" ones as a single line. */
export function listedHotkeys(): ListedHotkey[] {
    const numbered = /^session-\d$/;
    const others: ListedHotkey[] = HOTKEYS.filter((hotkey) => !numbered.test(hotkey.id));
    const first = HOTKEYS.findIndex((hotkey) => numbered.test(hotkey.id));
    const nine: ListedHotkey = {
        keys: "cmd+1…9",
        label: "Go to session 1 to 9 in the sidebar",
        group: "Sessions",
    };
    return others.toSpliced(first, 0, nine);
}

/** Shortcuts that are handled where they happen, listed so the map is complete. */
export const LOCAL_HOTKEYS: { keys: string; label: string }[] = [
    { keys: "enter", label: "Send the message" },
    { keys: "shift+enter", label: "New line in the message" },
    { keys: "escape", label: "Stop the agent's turn" },
    { keys: "home", label: "Start of the line in the message" },
    { keys: "end", label: "End of the line in the message" },
    { keys: "cmd+backspace", label: "Empty the message, with the caret at its start" },
    { keys: "/", label: "Suggest commands and skills, at the start of a message" },
    {
        keys: "arrowup",
        label: "Bring back an earlier message to the session, with the caret at the very start",
    },
    {
        keys: "arrowdown",
        label: "Go on to a later message, or back to what was typed, with the caret at the very end",
    },
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

/** A shortcut of the table written in one piece, as the shortcuts list shows it: "⌘⌥↑". */
export function writtenHotkey(id: HotkeyId): string {
    const hotkey = HOTKEYS.find((each) => each.id === id);
    return hotkey ? keyCaps(hotkey.keys).join("") : "";
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
