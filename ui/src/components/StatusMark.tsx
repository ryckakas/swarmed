import type { SessionStatus } from "../api/types";

const LABELS: Record<SessionStatus, string> = {
    idle: "Idle",
    working: "Working",
    waiting: "Needs you",
    errored: "Stopped with an error",
};

/** What an idle session with an answer nobody has looked at is called. */
const UNREAD = "Unread reply";

export function statusLabel(status: SessionStatus, unread = false): string {
    return unread && status === "idle" ? UNREAD : LABELS[status];
}

interface Props {
    status: SessionStatus;
    /** Whether the session holds an answer nobody has looked at. */
    unread?: boolean;
}

/**
 * A small mark for a session's status. Each status has its own shape, not only its own colour.
 * An idle session that has answered and not been looked at is told apart from one that is
 * merely idle; a session that is busy, waiting or failed already says more than "unread" would.
 */
export function StatusMark({ status, unread = false }: Props) {
    const kind = unread && status === "idle" ? "unread" : status;
    return (
        <span className={`mark mark-${kind}`} role="img" aria-label={statusLabel(status, unread)} />
    );
}
