// What a session is for, as the window says it: the menu that chooses a session's role, the
// chip on its row, the line a change leaves in its conversation, and what the role keeps it to
// (plan mode, and a plan handed on once approved). Pure, so it can be tested.

import type { RoleInfo, Supervision } from "./api/types";
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

/**
 * The supervision role `name` holds a session in, as the core starts one with it: a role that
 * changes no files starts in plan mode, which an agent without one takes as its read-only mode.
 * `null` for no role, a role that holds none, or one the window does not know.
 */
export function roleHolds(name: string | null, roles: RoleInfo[]): Supervision | null {
    return roles.find((role) => role.name === name)?.supervision ?? null;
}

/**
 * Whether approving a plan of a session with role `name` keeps it planning: a role that changes
 * no files hands its plan on instead of carrying it out.
 */
export function keepsPlanning(name: string | null, roles: RoleInfo[]): boolean {
    return roles.some((role) => role.name === name && !role.changes_files);
}

/**
 * Why the supervision menu of a new session is fixed while its role `name` holds it, as the
 * menu says when pointed at; `null` while it is the user's to choose.
 */
export function heldSaid(name: string | null, roles: RoleInfo[], words: Words): string | null {
    return name !== null && roleHolds(name, roles) !== null
        ? words.roleHolds(words.role(name))
        : null;
}

/**
 * What the prompt to approve a plan of a session with role `name` says the approval does, for
 * a role that keeps planning; `null` for the usual.
 */
export function planKeptSaid(name: string | null, roles: RoleInfo[], words: Words): string | null {
    return name !== null && keepsPlanning(name, roles) ? words.planKept(words.role(name)) : null;
}
