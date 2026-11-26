// What sessions are for in the sample, as the core keeps it (`roles.rs`): its built-in roles,
// and the roles of bonsai-lint's sample tree.

import type { AgentEvent, Project, RoleInfo, Session, Supervision } from "./types";

/** The roles the core has built in, in the order it offers them. */
export const MOCK_ROLES: RoleInfo[] = [
    {
        name: "architect",
        description:
            "shapes a whole feature across tasks: splits it into parts, each one task, one branch and one pull request, and starts a planner for each",
        supervision: "plan",
        changes_files: false,
        starts: ["planner", "reviewer"],
        child: "planner",
    },
    {
        name: "planner",
        description:
            "plans one part inside one task, hands every change to implementers in its worktree, and checks what they report against the plan",
        supervision: "plan",
        changes_files: false,
        starts: ["implementer", "reviewer"],
        child: "implementer",
    },
    {
        name: "implementer",
        description:
            "builds the part it was given and nothing beside it, and reports back when done or stuck",
        supervision: null,
        changes_files: true,
        starts: [],
        child: null,
    },
    {
        name: "reviewer",
        description:
            "reads and reviews what it is pointed at, changes nothing, and reports its findings",
        supervision: "plan",
        changes_files: false,
        starts: [],
        child: null,
    },
];

/**
 * The role of each sample session that has one, by id, each one its starter's role may start
 * (`roles.rs`): the rule engine's architect (9) over two planners beside it (10 and 12) and one
 * in a task of its own (13), and the parser's planner over its implementer (11).
 */
export const SAMPLE_ROLES: Partial<Record<number, string>> = {
    9: "architect",
    10: "planner",
    11: "implementer",
    12: "planner",
    13: "planner",
};

/** The supervision a role holds its sessions in, if it holds one: a role that changes no files. */
function holds(role: string | null): Supervision | null {
    return MOCK_ROLES.find((known) => known.name === role)?.supervision ?? null;
}

/**
 * What each sample session's role took the place of, by id, which taking the role off puts back:
 * as the core keeps it in `supervision_before_role`. The user's own choice forgets it.
 */
const heldFrom = new Map<number, Supervision>();

/** The supervision sample session `id` starts in: its role's, if it holds one, else asking. */
export function sampleSupervision(id: number): Supervision {
    const held = holds(SAMPLE_ROLES[id] ?? null);
    if (held === null) {
        return "ask";
    }
    heldFrom.set(id, "ask");
    return held;
}

/**
 * The supervision a new sample session with `role` starts in when it was asked to start as
 * `asked`, as the core starts one: the role's, if it holds one, remembering what to put back.
 */
export function sampleStartsIn(id: number, role: string | null, asked: Supervision): Supervision {
    const held = holds(role);
    if (held === null || held === asked) {
        return asked;
    }
    heldFrom.set(id, asked);
    return held;
}

/** The user chose sample session `id`'s supervision by hand: their choice wins over its role. */
export function sampleSupervisionChosen(id: number) {
    heldFrom.delete(id);
}

/**
 * Puts `session` under what its role `role` holds, or back under what an earlier role took the
 * place of, as the core does when a role is set (`held` in `roles.rs`). The supervision it has
 * now if that changed it; else `null`.
 */
function heldBy(session: Session, role: string | null): Supervision | null {
    const now = session.supervision;
    const held = holds(role);
    const before = heldFrom.get(session.id);
    if (held !== null && held !== now) {
        heldFrom.set(session.id, before ?? now);
    } else if (held === null) {
        heldFrom.delete(session.id);
    }
    const next = held ?? before ?? now;
    session.supervision = next;
    return next === now ? null : next;
}

/** Whether a sample session with `role` keeps planning once its plan is approved. */
export function sampleKeepsPlanning(role: string | null): boolean {
    return MOCK_ROLES.some((known) => known.name === role && !known.changes_files);
}

/**
 * Gives sample session `id` the role `role`, or with `null` none, as the core does: one there
 * is not is refused, and the change is recorded only once something was said in the session.
 * A role that changes no files holds it in plan mode, and taking it off puts back what it had.
 */
export function sampleRoleSet(
    projects: Project[],
    id: number,
    role: string | null,
    said: boolean,
): { session: Session; event: AgentEvent | null; supervision: Supervision | null } {
    const sessions = projects.flatMap((project) => project.tasks).flatMap((task) => task.sessions);
    const session = sessions.find((one) => one.id === id);
    if (!session) {
        throw new Error(`session ${id} not found`);
    }
    if (role !== null && !MOCK_ROLES.some((known) => known.name === role)) {
        const names = MOCK_ROLES.map((known) => known.name).join(", ");
        throw new Error(`there is no role called \`${role}\`; the roles are ${names}`);
    }
    const previous = session.role;
    if (previous === role) {
        return { session, event: null, supervision: null };
    }
    session.role = role;
    const event: AgentEvent | null = said ? { type: "role_changed", role, previous } : null;
    return { session, event, supervision: heldBy(session, role) };
}
