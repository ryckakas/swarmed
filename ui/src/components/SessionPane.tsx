import { forkSession } from "../actions";
import type { AgentEvent, Project, Session, Task } from "../api/types";
import { isSwarm } from "../appearance";
import { placeName } from "../chats";
import { agentName, iconForAgent } from "../providerIcons";
import { scrollKey } from "../scrollSpot";
import { useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
import { useWords } from "../useWords";
import { capitalized } from "../wording";
import { CarryOnButton } from "./CarryOnDialog";
import { Composer } from "./Composer";
import { CloudIcon, EndIcon, ForkIcon, GlobeIcon, IconButton, PanelIcon } from "./IconButton";
import { NewTask } from "./NewTask";
import { CLAUDE, useAgents, useSendsToBackground } from "./options";
import { Picker } from "./Picker";
import { ProviderMark } from "./ProviderMark";
import { RemoteButton } from "./RemoteButton";
import { SessionRole } from "./Role";
import { SessionTitle } from "./SessionTitle";
import { SetupPanel } from "./SetupPanel";
import { StatusLine } from "./StatusLine";
import { Egg, StatusMark, useStatusLabel } from "./StatusMark";
import { SubAgentPane } from "./SubAgentPane";
import { Transcript } from "./Transcript";
import { UsageButton } from "./UsageButton";
import { WorkingWith, WorksForPicker } from "./WorksFor";

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
    const subView = useStore((state) => state.subView);
    const sendsToBackground = useSendsToBackground(session?.agent ?? "");

    if (!(session && home)) {
        return <NewTask projects={projects} />;
    }
    // The chats are no repository: what they are listed under stands in for its name.
    const where = placeName(home.project.name, words);
    if (subView?.sessionId === session.id) {
        return (
            <SubAgentPane
                session={session}
                crumbs={`${where} / ${home.task.name}`}
                id={subView.id}
                events={events ?? NO_EVENTS}
            />
        );
    }
    return (
        <article className="session">
            <header className="session-head">
                <div className="session-actions">
                    {/* Only Claude Code can be reached from claude.ai. */}
                    {session.agent === CLAUDE && (
                        <RemoteButton
                            key={`remote-${session.id}`}
                            session={session}
                            events={events ?? NO_EVENTS}
                        />
                    )}
                    {/* Only Claude Code reports the plan's usage. */}
                    {session.agent === CLAUDE && (
                        <UsageButton
                            key={`usage-${session.id}`}
                            session={session}
                            events={events ?? NO_EVENTS}
                        />
                    )}
                    <IconButton
                        label={`${words.forkAll}: a new ${words.session} that starts from all of it, on the same ${words.terms.agent}`}
                        onClick={() => void forkSession(session.id, null)}
                    >
                        <ForkIcon />
                    </IconButton>
                    <HandOn session={session} project={home.project} task={home.task} />
                    <BrowserButton task={home.task.id} running={home.task.browser} />
                    {!panelOpen && (
                        <IconButton
                            label={`Show the ${words.terms.task} panel`}
                            onClick={togglePanel}
                        >
                            <PanelIcon />
                        </IconButton>
                    )}
                    {/* Last, at the far right: it ends the session, apart from what carries it on. */}
                    <IconButton
                        label={`${words.closeSession}: ${words.archiveSession.toLowerCase()} it, or ${words.deleteSession.toLowerCase()} it for good`}
                        danger
                        onClick={() => askToClose(session.id)}
                    >
                        <EndIcon />
                    </IconButton>
                </div>
                <p className="crumbs">
                    {where} / {home.task.name}
                </p>
                <SessionTitle key={session.id} session={session} />
                <div className="session-meta">
                    <StatusWords session={session} />
                    <p className="session-agent">
                        <ProviderMark icon={iconForAgent(session.agent)} />
                        {agentName(session.agent)}
                    </p>
                    <SessionRole session={session} />
                    <WorksForPicker session={session} />
                    <CloneTo session={session} />
                </div>
                <WorkingWith session={session} />
            </header>
            <SetupPanel project={home.project.name} task={home.task} />
            {events === undefined && <p className="aside loading">{words.loadingConversation}</p>}
            {events?.length === 0 && <Facts session={session} task={home.task} />}
            {events !== undefined && events.length > 0 && (
                <Transcript
                    key={`transcript-${session.id}`}
                    sessionId={session.id}
                    place={scrollKey(session.id)}
                    events={events}
                    working={session.status === "working"}
                    sendsToBackground={sendsToBackground}
                    quotable
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

/** Shows or hides the task's own browser beside the conversation; marked while it runs. */
function BrowserButton({ task, running }: { task: number; running: boolean }) {
    const words = useWords();
    const open = useStore((state) => state.browserOpen.includes(task));
    const toggleBrowser = useStore((state) => state.toggleBrowser);
    const what = `the ${words.terms.task}'s own browser`;
    const label = open ? `Hide ${what}` : `Show ${what}${running ? ", which runs" : ""}`;
    return (
        <span className="browser-button" data-running={running} data-open={open}>
            <IconButton label={label} onClick={() => toggleBrowser(task)}>
                <GlobeIcon />
            </IconButton>
        </span>
    );
}

/**
 * Where what the session arrived at can go on: to a cloud agent, which works on a repository,
 * or from a chat, which has none, into a task of a repository.
 */
function HandOn({ session, project, task }: { session: Session; project: Project; task: Task }) {
    const words = useWords();
    const askToDelegate = useStore((state) => state.askToDelegate);
    if (project.chats) {
        return <CarryOnButton session={session} chat={task} />;
    }
    return (
        <IconButton
            label={`${words.delegate}: what this ${words.session} arrived at, as a brief or as the whole conversation`}
            onClick={() =>
                askToDelegate({ project: project.name, task: task.name, session: session.id })
            }
        >
            <CloudIcon />
        </IconButton>
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
            label={`${words.cloneTo} another ${words.terms.agent}: a new ${words.session} there that starts from this conversation`}
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
    const words = useWords();
    const swarm = useStore((state) => isSwarm(state.settings?.style));
    return (
        <div className="facts-wrap">
            {swarm && (
                <p className="facts-egg">
                    <Egg label={words.status.idle[0] ?? ""} />
                </p>
            )}
            <p>{words.emptySession}</p>
            <dl className="facts">
                <dt>{capitalized(words.terms.worktree)}</dt>
                <dd className="mono">{task.worktree_path}</dd>
                <dt>{capitalized(words.terms.branch)}</dt>
                <dd className="mono">{task.branch}</dd>
                <dt>Supervision</dt>
                <dd>{SUPERVISION[session.supervision]}</dd>
                <dt>{capitalized(words.terms.model)}</dt>
                <dd>{session.model ?? "Claude Code's default"}</dd>
            </dl>
        </div>
    );
}

/** The session's status in a mark and in words, which always say the same. */
function StatusWords({ session }: { session: Session }) {
    const status = useStatusLabel(session.status, session.unread, session.background);
    return (
        <p className={`session-status status-${session.status}`}>
            <StatusMark
                status={session.status}
                unread={session.unread}
                background={session.background}
                label={status}
            />
            {status}
        </p>
    );
}
