import type { SessionStatus } from "../api/types";

const LABELS: Record<SessionStatus, string> = {
    idle: "Idle",
    working: "Working",
    waiting: "Needs you",
    errored: "Stopped with an error",
};

export function statusLabel(status: SessionStatus): string {
    return LABELS[status];
}

/** A small mark for a session's status. Each status has its own shape, not only its own colour. */
export function StatusMark({ status }: { status: SessionStatus }) {
    return <span className={`mark mark-${status}`} role="img" aria-label={statusLabel(status)} />;
}
