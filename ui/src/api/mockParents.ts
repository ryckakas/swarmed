// Who works for whom in the sample, as the core keeps it (`parents.rs`): a tree of sessions in
// bonsai-lint's two rule tasks, three levels deep and across both tasks.

import { parentChoices } from "../tree";
import type { AgentEvent, Project, Session } from "./types";

/**
 * Whom each sample session works for, by id: the rule engine's architect (9) over the parser
 * (10) and the port of the built-in rules (12), the parser over its tests (11), and the
 * architect over the documentation, in a task of its own (13).
 */
export const SAMPLE_PARENTS: Partial<Record<number, number>> = { 10: 9, 11: 10, 12: 9, 13: 9 };

/** The sample sessions another session's agent started in its own worktree. */
export const SAMPLE_SPAWNED = new Set([10, 11, 12]);

/**
 * Has sample session `id` work for `parent`, or with `null` for nobody, as the core checks it:
 * another open session of its repository that does not work for it. What every session it
 * concerns records, which the core records in each of them.
 */
export function sampleParentSet(
    projects: Project[],
    id: number,
    parent: number | null,
): { session: Session; event: AgentEvent; concerned: number[] } {
    const sessions = projects.flatMap((project) => project.tasks).flatMap((task) => task.sessions);
    const session = sessions.find((one) => one.id === id);
    if (!session) {
        throw new Error(`session ${id} not found`);
    }
    if (parent !== null && !parentChoices(projects, session).some((one) => one.id === parent)) {
        throw new Error(
            `session ${id} cannot work for session ${parent}: it is not another open session of the same repository that does not work for this one`,
        );
    }
    const previous = session.parent;
    session.parent = parent;
    const event: AgentEvent = { type: "parent_changed", child: id, parent, previous };
    const concerned = [id, previous, parent].filter(
        (one, at, all): one is number => one !== null && all.indexOf(one) === at,
    );
    return { session, event, concerned };
}
