import { keyCaps, LOCAL_HOTKEYS, listedHotkeys } from "../hotkeys";

const SHORTCUT_GROUPS = ["Sessions", "In a session", "Task", "Window"] as const;

/** Every keyboard shortcut, by where it applies. */
export function ShortcutLists() {
    return (
        <>
            {SHORTCUT_GROUPS.map((group) => (
                <ShortcutList
                    key={group}
                    title={group}
                    shortcuts={listedHotkeys().filter((hotkey) => hotkey.group === group)}
                />
            ))}
            <ShortcutList title="While typing or in the sidebar" shortcuts={LOCAL_HOTKEYS} />
        </>
    );
}

interface ShortcutListProps {
    title: string;
    shortcuts: { keys: string; label: string }[];
}

function ShortcutList({ title, shortcuts }: ShortcutListProps) {
    return (
        <>
            <p className="shortcut-group">{title}</p>
            <dl className="shortcuts">
                {shortcuts.map((shortcut) => (
                    // ↑ and ↓ do one thing in the message box and another in the sidebar.
                    <div key={`${shortcut.keys} ${shortcut.label}`} className="shortcut">
                        <dt>{shortcut.label}</dt>
                        <dd>
                            {keyCaps(shortcut.keys).map((cap) => (
                                <kbd key={cap}>{cap}</kbd>
                            ))}
                        </dd>
                    </div>
                ))}
            </dl>
        </>
    );
}
