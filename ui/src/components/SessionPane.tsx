import { useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
import { StatusMark, statusLabel } from "./StatusMark";

const SUPERVISION = {
    ask: "Edits allowed, commands ask",
    auto: "Auto mode decides",
    bypass: "Nothing asks",
};

/** The centre of the window: the selected session. The transcript itself comes later. */
export function SessionPane() {
    const projects = useStore((state) => state.projects);
    const selectedSessionId = useStore((state) => state.selectedSessionId);
    const session = findSession(projects, selectedSessionId);
    const home = session && findSessionHome(projects, session);

    if (!(session && home)) {
        return (
            <div className="notice">
                <h1>No session selected</h1>
                <p>Pick a session on the left, or use the arrow keys to move between them.</p>
            </div>
        );
    }
    return (
        <article className="session">
            <header className="session-head">
                <p className="crumbs">
                    {home.project.name} / {home.task.name}
                </p>
                <h1>{session.title || "New session"}</h1>
                <p className={`session-status status-${session.status}`}>
                    <StatusMark status={session.status} />
                    {statusLabel(session.status)}
                </p>
            </header>
            <dl className="facts">
                <dt>Worktree</dt>
                <dd className="mono">{home.task.worktree_path}</dd>
                <dt>Branch</dt>
                <dd className="mono">{home.task.branch}</dd>
                <dt>Supervision</dt>
                <dd>{SUPERVISION[session.supervision]}</dd>
                <dt>Model</dt>
                <dd>{session.model ?? "Claude Code's default"}</dd>
            </dl>
        </article>
    );
}
