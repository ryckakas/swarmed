import { api } from "../api";
import type { Session, Task } from "../api/types";
import { useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
import { Composer } from "./Composer";
import { NewTask } from "./NewTask";
import { StatusMark, statusLabel } from "./StatusMark";
import { Transcript } from "./Transcript";

const SUPERVISION = {
    ask: "Edits allowed, commands ask",
    auto: "Auto mode decides",
    bypass: "Nothing asks",
};

/** The centre of the window: the selected session's conversation. */
export function SessionPane() {
    const projects = useStore((state) => state.projects);
    const selectedSessionId = useStore((state) => state.selectedSessionId);
    const events = useStore((state) =>
        selectedSessionId === null ? undefined : state.events[selectedSessionId],
    );
    const close = useStore((state) => state.close);
    const session = findSession(projects, selectedSessionId);
    const home = session && findSessionHome(projects, session);

    if (!(session && home)) {
        return <NewTask projects={projects} />;
    }
    return (
        <article className="session">
            <header className="session-head">
                <p className="crumbs">
                    {home.project.name} / {home.task.name}
                </p>
                <h1>{session.title || "New session"}</h1>
                <div className="session-meta">
                    <p className={`session-status status-${session.status}`}>
                        <StatusMark status={session.status} />
                        {statusLabel(session.status)}
                    </p>
                    <button
                        type="button"
                        className="quiet-button"
                        title="Stops the agent and removes the session from the sidebar"
                        onClick={() => void close(api, session.id)}
                    >
                        Close session
                    </button>
                </div>
            </header>
            {events === undefined && <p className="aside loading">Loading the conversation…</p>}
            {events?.length === 0 && <Facts session={session} task={home.task} />}
            {events !== undefined && events.length > 0 && (
                <Transcript
                    key={`transcript-${session.id}`}
                    sessionId={session.id}
                    events={events}
                    working={session.status === "working"}
                />
            )}
            <Composer key={`composer-${session.id}`} session={session} />
        </article>
    );
}

/** What a session that has not said anything yet will work with. */
function Facts({ session, task }: { session: Session; task: Task }) {
    return (
        <div className="facts-wrap">
            <p>Nothing has been said yet. The agent starts with your first message.</p>
            <dl className="facts">
                <dt>Worktree</dt>
                <dd className="mono">{task.worktree_path}</dd>
                <dt>Branch</dt>
                <dd className="mono">{task.branch}</dd>
                <dt>Supervision</dt>
                <dd>{SUPERVISION[session.supervision]}</dd>
                <dt>Model</dt>
                <dd>{session.model ?? "Claude Code's default"}</dd>
            </dl>
        </div>
    );
}
