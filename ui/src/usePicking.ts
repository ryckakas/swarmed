import { type KeyboardEvent, useState } from "react";
import { stepActive } from "./slash";

/**
 * What a list opened by a sign typed in a box (`@` for sessions, `#` for pull requests and
 * issues) keeps between keys: which entry the arrows are on, and whether Esc put it away for
 * the text the box holds (`away`), until that text changes.
 */
export function usePicking(value: string) {
    const [active, setActive] = useState(0);
    // The text Esc was pressed on; the list stays away until the text changes.
    const [dismissed, setDismissed] = useState<string | null>(null);

    /**
     * The keys the list takes while it is open with `matches`, or with none but something to
     * say (`open`): the arrows, Enter and Tab to choose, Esc to put it away. Handles the key if
     * it is one of those and says whether it did.
     */
    const keysFor =
        <T>(matches: T[], open: boolean, choose: (item: T) => void) =>
        (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
            if (event.nativeEvent.isComposing || event.shiftKey || !(matches.length > 0 || open)) {
                return false;
            }
            const current = Math.min(active, Math.max(matches.length - 1, 0));
            const chosen = matches[current];
            const actions = new Map<string, () => void>([["Escape", () => setDismissed(value)]]);
            if (chosen !== undefined) {
                const step = (by: number) => setActive(stepActive(current, by, matches.length));
                actions.set("ArrowDown", () => step(1));
                actions.set("ArrowUp", () => step(-1));
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

    /** Which of `count` entries the arrows are on. */
    const activeOf = (count: number) => Math.min(active, Math.max(count - 1, 0));

    return { away: dismissed === value, activeOf, restart: () => setActive(0), keysFor };
}
