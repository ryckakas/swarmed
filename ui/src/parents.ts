// Who works for whom, as the window says it: the menu that chooses whom a session works for,
// and the line its change leaves in each conversation it concerns. Pure, so it can be tested.

import type { Project, Session } from "./api/types";
import { findSession, parentChoices } from "./tree";
import type { Words } from "./wording";

/** A change of whom session `child` works for, as the transcript keeps it. */
export interface Regrouped {
    child: number;
    parent: number | null;
    previous: number | null;
}

/**
 * What the change says in session `own`'s conversation: that one's own new place, or which
 * session works for it from here on, or no longer does. It applies from the next message.
 */
export function regroupedSaid({ child, parent, previous }: Regrouped, own: number, words: Words) {
    const numbered = words.sessionNumbered;
    if (own !== child) {
        const what = parent === own ? "works for this" : "no longer works for this";
        const since = parent === own ? " from here on" : "";
        return `${numbered(child)} ${what} ${words.session}${since}.`;
    }
    const now = parent === null ? words.worksForNobody : words.worksFor(parent);
    const before = previous === null ? "" : `, no longer for ${words.session} ${previous}`;
    return `${now} from here on${before}.`;
}

/** A session in the menu: its number and its title. */
function choiceLabel(session: Session, words: Words): string {
    return `${words.worksFor(session.id)}: ${session.title || words.untitled}`;
}

/**
 * The choices of the menu that chooses whom `session` works for, as `[value, label]`: nobody,
 * then every session it may work for, the one it works for now included even where it could
 * not be chosen again, such as a closed one.
 */
export function parentOptions(
    projects: Project[],
    session: Session,
    words: Words,
): [value: string, label: string][] {
    const choices = parentChoices(projects, session);
    const now = session.parent;
    const kept = now !== null && !choices.some((one) => one.id === now);
    const current: [string, string][] = kept
        ? [[String(now), currentLabel(projects, now, words)]]
        : [];
    return [
        ["", words.worksForNobody],
        ...current,
        ...choices.map((one): [string, string] => [String(one.id), choiceLabel(one, words)]),
    ];
}

/** The menu's choice for the session one works for now, which is not in the tree if closed. */
function currentLabel(projects: Project[], id: number, words: Words): string {
    const found = findSession(projects, id);
    return found ? choiceLabel(found, words) : words.worksFor(id);
}

/** The session a menu's value names; `null` for nobody. */
export function parentChosen(value: string): number | null {
    const id = Number.parseInt(value, 10);
    return Number.isNaN(id) ? null : id;
}
