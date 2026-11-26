// What a session is for, as the window says it: the menu that chooses a session's role, the
// chip on its row, and the line a change leaves in its conversation. Pure, so it can be tested.

import type { RoleInfo } from "./api/types";
import type { Words } from "./wording";

/** A change of a session's role, as the transcript keeps it. */
export interface RoleChange {
    role: string | null;
    previous: string | null;
}

/**
 * The choices of the menu that chooses a role, as `[value, label]`: none, then every role, and
 * the one chosen now first where it is not among them, such as one a newer Swarmed knows.
 */
export function roleOptions(
    roles: RoleInfo[],
    current: string | null,
    words: Words,
): [value: string, label: string][] {
    const unknown = current !== null && !roles.some((role) => role.name === current);
    const kept: [string, string][] = unknown ? [[current, words.role(current)]] : [];
    return [
        ["", words.noRole],
        ...kept,
        ...roles.map((role): [string, string] => [role.name, words.role(role.name)]),
    ];
}

/** The role a menu's value names; `null` for none. */
export function roleChosen(value: string): string | null {
    return value === "" ? null : value;
}

/** What the change says in its conversation: the role from here on, and the one before. */
export function roleChangedSaid({ role, previous }: RoleChange, words: Words): string {
    const now = role === null ? words.noRole : words.role(role);
    const before = previous === null ? "" : `, no longer ${words.role(previous)}`;
    return `${now} from here on${before}.`;
}

/** What role `name` is for, as a row's chip or the menu says when pointed at. */
export function roleAbout(name: string, roles: RoleInfo[], words: Words): string {
    const role = roles.find((one) => one.name === name);
    return role ? `${words.role(name)}: ${role.description}` : words.role(name);
}
