// What sessions are for in the sample, as the core keeps it (`roles.rs`): its built-in roles,
// and the roles of bonsai-lint's sample tree.

import type { AgentEvent, Project, RoleInfo, Session } from "./types";

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
 * The role of each sample session that has one, by id: the rule engine's architect (9) over
 * two implementers (10 and 12) and a planner in a task of its own (13), and a reviewer of the
 * parser (11).
 */
export const SAMPLE_ROLES: Partial<Record<number, string>> = {
    9: "architect",
    10: "implementer",
    11: "reviewer",
    12: "implementer",
    13: "planner",
};

/**
 * Gives sample session `id` the role `role`, or with `null` none, as the core does: one there
 * is not is refused, and the change is recorded only once something was said in the session.
 */
export function sampleRoleSet(
    projects: Project[],
    id: number,
    role: string | null,
    said: boolean,
): { session: Session; event: AgentEvent | null } {
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
        return { session, event: null };
    }
    session.role = role;
    const event: AgentEvent | null = said ? { type: "role_changed", role, previous } : null;
    return { session, event };
}
