import { forkSession } from "../actions";
import type { AgentEvent, Session, Task } from "../api/types";
import { isSwarm } from "../appearance";
import { agentName, iconForAgent } from "../providerIcons";
import { useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
import { useWords } from "../useWords";
import { Composer } from "./Composer";
import { EndIcon, ForkIcon, IconButton, PanelIcon } from "./IconButton";
import { NewTask } from "./NewTask";
import { useAgents } from "./options";
import { Picker } from "./Picker";
import { ProviderMark } from "./ProviderMark";
import { SessionTitle } from "./SessionTitle";
import { StatusLine } from "./StatusLine";
import { Egg, StatusMark, useStatusLabel } from "./StatusMark";
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
    const words = useWords();
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
                        label={`${words.closeSession}: ${words.archiveSession.toLowerCase()} it, or ${words.deleteSession.toLowerCase()} it for good`}
                        danger
                        onClick={() => askToClose(session.id)}
                    >
                        <EndIcon />
                    </IconButton>
                    <IconButton
                        label={`${words.forkAll}: a new session that starts from all of it, on the same agent`}
                        onClick={() => void forkSession(session.id, null)}
                    >
                        <ForkIcon />
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
                    <StatusWords session={session} />
                    <p className="session-agent">
                        <ProviderMark icon={iconForAgent(session.agent)} />
                        {agentName(session.agent)}
                    </p>
                    <CloneTo session={session} />
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

/**
 * A menu of the other agents: choosing one starts a session on it that begins with all that was
 * said here. This session stays as it is, on its own agent.
 */
function CloneTo({ session }: { session: Session }) {
    const words = useWords();
    const others = useAgents().filter((agent) => agent.id !== session.agent);
    if (others.length === 0) {
        return null;
    }
    return (
        <Picker
            label={`${words.cloneTo} another agent: a new session there that starts from this conversation`}
            value=""
            onChange={(agent) => agent && void forkSession(session.id, null, agent)}
            options={[
                ["", words.cloneTo],
                ...others.map((agent): [string, string] => [agent.id, agent.name]),
            ]}
        />
    );
}

/** What a session that has not said anything yet will work with. */
function Facts({ session, task }: { session: Session; task: Task }) {
    const swarm = useStore((state) => isSwarm(state.settings?.style));
    return (
        <div className="facts-wrap">
            {swarm && (
                <p className="facts-egg">
                    <Egg label="Dormant" />
                </p>
            )}
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

/** The session's status in a mark and in words, which always say the same. */
function StatusWords({ session }: { session: Session }) {
    const status = useStatusLabel(session.status, session.unread);
    return (
        <p className={`session-status status-${session.status}`}>
            <StatusMark status={session.status} unread={session.unread} label={status} />
            {status}
        </p>
    );
}
