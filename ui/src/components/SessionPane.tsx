import type { AgentEvent, Session, Task } from "../api/types";
import { agentName, iconForAgent } from "../providerIcons";
import { useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
import { Composer } from "./Composer";
import { EndIcon, IconButton, PanelIcon } from "./IconButton";
import { NewTask } from "./NewTask";
import { ProviderMark } from "./ProviderMark";
import { SessionTitle } from "./SessionTitle";
import { StatusLine } from "./StatusLine";
import { StatusMark, statusLabel } from "./StatusMark";
import { Transcript } from "./Transcript";

const SUPERVISION = {
    ask: "Edits allowed, commands ask",
    auto: "Auto mode decides",
    bypass: "Nothing asks",
    plan: "Plans first, acts once approved",
};

/** Stands in for a transcript that has not been read, without a new array on every render. */
const NO_EVENTS: AgentEvent[] = [];

/** The centre of the window: the selected session's conversation. */
export function SessionPane() {
    const projects = useStore((state) => state.projects);
    const selectedSessionId = useStore((state) => state.selectedSessionId);
    const events = useStore((state) =>
        selectedSessionId === null ? undefined : state.events[selectedSessionId],
    );
    const askToClose = useStore((state) => state.askToClose);
    const panelOpen = useStore((state) => state.panelOpen);
    const togglePanel = useStore((state) => state.togglePanel);
    const session = findSession(projects, selectedSessionId);
    const home = session && findSessionHome(projects, session);

    if (!(session && home)) {
        return <NewTask projects={projects} />;
    }
    return (
        <article className="session">
            <header className="session-head">
                <div className="session-actions">
                    <IconButton
                        label="Close session: stops the agent and removes it from the sidebar"
                        danger
                        onClick={() => askToClose(session.id)}
                    >
                        <EndIcon />
                    </IconButton>
                    {!panelOpen && (
                        <IconButton label="Show the task panel" onClick={togglePanel}>
                            <PanelIcon />
                        </IconButton>
                    )}
                </div>
                <p className="crumbs">
                    {home.project.name} / {home.task.name}
                </p>
                <SessionTitle key={session.id} session={session} />
                <div className="session-meta">
                    <p className={`session-status status-${session.status}`}>
                        <StatusMark status={session.status} unread={session.unread} />
                        {statusLabel(session.status, session.unread)}
                    </p>
                    <p className="session-agent">
                        <ProviderMark icon={iconForAgent(session.agent)} />
                        {agentName(session.agent)}
                    </p>
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
            <StatusLine
                project={home.project.name}
                task={home.task}
                session={session}
                events={events ?? NO_EVENTS}
            />
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
