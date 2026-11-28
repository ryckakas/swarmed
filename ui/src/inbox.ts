// Everything that waits for the user, across all repositories, in the order to deal with it.
// Pure, so it can be tested.

import type { Project, Session } from "./api/types";
import { snoozed } from "./snooze";

/** Why a session is in the inbox. */
export type InboxKind = "waiting" | "errored" | "unread";

/** The kinds, most urgent first: a question blocks an agent, a failure ended one. */
export const INBOX_KINDS: InboxKind[] = ["waiting", "errored", "unread"];

export interface InboxItem {
    session: Session;
    /** The repository and task the session belongs to, to tell same-named sessions apart. */
    project: string;
    task: string;
    kind: InboxKind;
}

/** Why a session waits for the user, if it does. */
function kindOf(session: Session): InboxKind | null {
    if (session.status === "waiting" || session.status === "errored") {
        return session.status;
    }
    return session.status === "idle" && session.unread ? "unread" : null;
}

/**
 * The sessions that wait for the user: those that ask for a decision, then those that failed,
 * then those with an answer nobody has looked at. Within a kind, in the sidebar's order. One
 * snoozed at `now`, in milliseconds since 1970, is left out until it wakes.
 */
export function inbox(projects: Project[], now: number): InboxItem[] {
    const items = projects.flatMap((project) =>
        project.tasks
            .filter((task) => !task.archived)
            .flatMap((task) =>
                task.sessions.flatMap((session): InboxItem[] => {
                    const kind = snoozed(session, now) ? null : kindOf(session);
                    return kind ? [{ session, project: project.name, task: task.name, kind }] : [];
                }),
            ),
    );
    return INBOX_KINDS.flatMap((kind) => items.filter((item) => item.kind === kind));
}
