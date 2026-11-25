import { type KeyboardEvent, useEffect, useState } from "react";
import { api } from "../api";
import type { Reachable } from "../api/types";
import {
    fromManyRepositories,
    mentionMatches,
    mentionQuery,
    moreToFind,
    placeOf,
    withMention,
} from "../mentions";
import { stepActive } from "../slash";
import { useStore } from "../store";

/**
 * Where `@` finds the sessions to mention: those a session's agent can read, or, in the
 * new-task view where there is no session yet, those a new task's in a repository could.
 */
export type MentionFrom = { session: number } | { project: string };

/** One key for where the list comes from, so that it is read again when that changes. */
function keyOf(from: MentionFrom | undefined): string | null {
    if (!from) {
        return null;
    }
    return "session" in from ? `session:${from.session}` : `project:${from.project}`;
}

/**
 * The sessions `from` can mention, read from the core each time the list opens (`open`), so
 * that one started or closed since is right. `null` until they are read.
 */
function useReachable(from: MentionFrom | undefined, open: boolean): Reachable[] | null {
    const [read, setRead] = useState<{ key: string; sessions: Reachable[] } | null>(null);
    const key = keyOf(from);
    // biome-ignore lint/correctness/useExhaustiveDependencies: read again when the list opens or its source changes
    useEffect(() => {
        if (!(open && from && key)) {
            return;
        }
        const reading =
            "session" in from
                ? api.sessionReachable(from.session)
                : api.projectReachable(from.project);
        let current = true;
        reading
            .then((sessions) => {
                if (current) {
                    setRead({ key, sessions });
                }
            })
            .catch(useStore.getState().report);
        return () => {
            current = false;
        };
    }, [open, key]);
    return read && read.key === key ? read.sessions : null;
}

/**
 * The list of sessions to mention for an `@` being typed in a box holding `value` with the
 * caret at `caret`, or `null` where it is not known: which to show, which one the arrows are on, and the keys the list takes
 * for itself while it is open. `onChange` is given the box's new text and where its caret goes.
 */
export function useMentionMenu(
    value: string,
    caret: number | null,
    from: MentionFrom | undefined,
    onChange: (value: string, select: [number, number]) => void,
) {
    const [active, setActive] = useState(0);
    // The text Esc was pressed on; the list stays away until the text changes.
    const [dismissed, setDismissed] = useState<string | null>(null);
    const listing = from !== undefined && caret !== null && dismissed !== value;
    const typed = listing ? mentionQuery(value, caret) : null;
    const sessions = useReachable(from, typed !== null) ?? [];
    const query = typed?.query ?? "";
    const inTask = from !== undefined && "session" in from;
    const matches = typed ? mentionMatches(sessions, query, inTask) : [];
    const hint = typed !== null && moreToFind(sessions, matches, query);
    const current = Math.min(active, Math.max(matches.length - 1, 0));
    const choose = (session: Reachable) => {
        if (!typed) {
            return;
        }
        setActive(0);
        const end = typed.start + 1 + typed.query.length;
        const placed = withMention(value, typed.start, end, session);
        onChange(placed.value, [placed.caret, placed.caret]);
    };

    /** Handles the key if the list is open and it is one of the list's; says whether it did. */
    const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
        if (event.nativeEvent.isComposing || event.shiftKey || !(matches.length > 0 || hint)) {
            return false;
        }
        const chosen = matches[current];
        const actions = new Map<string, () => void>([["Escape", () => setDismissed(value)]]);
        if (chosen) {
            actions.set("ArrowDown", () => setActive(stepActive(current, 1, matches.length)));
            actions.set("ArrowUp", () => setActive(stepActive(current, -1, matches.length)));
            actions.set("Enter", () => choose(chosen));
            actions.set("Tab", () => choose(chosen));
        }
        const action = actions.get(event.key);
        if (!action) {
            return false;
        }
        event.preventDefault();
        action();
        return true;
    };
    return { matches, active: current, hint, choose, onKeyDown, all: sessions };
}

interface MenuProps {
    menu: ReturnType<typeof useMentionMenu>;
}

/** The sessions offered for an `@` being typed, above the box like the command list. */
export function MentionMenu({ menu }: MenuProps) {
    if (menu.matches.length === 0 && !menu.hint) {
        return null;
    }
    const many = fromManyRepositories(menu.all);
    return (
        <ul className="slash-menu mention-menu" aria-label="Sessions to mention">
            {menu.matches.map((session, index) => (
                <li key={session.id}>
                    <button
                        type="button"
                        className="slash-option"
                        aria-current={index === menu.active ? "true" : undefined}
                        // Keeps the text box focused, so typing carries on after the click.
                        onMouseDown={(event) => event.preventDefault()}
                        onClick={() => menu.choose(session)}
                    >
                        <span className="mention-at" aria-hidden="true">
                            @
                        </span>
                        {session.title || `Untitled session ${session.id}`}
                        <span className="slash-kind">{placeOf(session, many)}</span>
                    </button>
                </li>
            ))}
            {menu.hint && (
                <li className="mention-hint">
                    {menu.matches.length === 0
                        ? "Type to find a session"
                        : "Type to find another session"}
                </li>
            )}
        </ul>
    );
}
