import { type KeyboardEvent, useEffect, useState } from "react";
import { addProject, moveRow, newSession } from "../actions";
import { api } from "../api";
import type { CloudTask, Mark, Project, Session, Task } from "../api/types";
import { cloudProvider, cloudTitle, cloudTone, cloudTooltip, isUnfinished } from "../cloud";
import { taskTooltip } from "../environments";
import { orderAttributes, orderGroup, starterHere } from "../ordering";
import { agentName, iconForAgent } from "../providerIcons";
import { useStore } from "../store";
import type { SubAgent } from "../subagents";
import { listedUnder } from "../subagents";
import { countUnread, countWaiting, nestedSessions, projectStatus, projectUnread } from "../tree";
import { newVersion, updateLine } from "../updates";
import { useWords } from "../useWords";
import { withArticle, wordsFor } from "../wording";
import { ForgetCloudDialog } from "./ForgetCloudDialog";
import { ProviderMark } from "./ProviderMark";
import { PullBadge } from "./PullRequest";
import { StatusMark } from "./StatusMark";
import { useRowDrag } from "./useRowDrag";

export function Sidebar() {
    const projects = useStore((state) => state.projects);
    const load = useStore((state) => state.load);
    const startNew = useStore((state) => state.startNew);
    const showSettings = useStore((state) => state.showSettings);
    const showInbox = useStore((state) => state.showInbox);
    const showBanner = useStore((state) => state.showBanner);
    const version = useStore((state) => state.version);
    const update = useStore((state) => state.update);
    // A newer version that is out or ready marks the version, which opens the About view.
    const fresh = newVersion(update);
    const waiting = countWaiting(projects);
    const unread = countUnread(projects);
    const words = useStore((state) => wordsFor(state.settings?.style));
    const startDrag = useRowDrag(moveRow);

    return (
        <nav
            className="sidebar"
            aria-label={`Projects, ${words.terms.tasks} and sessions`}
            onKeyDown={moveFocus}
            onPointerDown={startDrag}
        >
            <div className="sidebar-top" data-tauri-drag-region>
                {version && (
                    <button
                        type="button"
                        className="sidebar-version"
                        data-new={fresh === null ? undefined : ""}
                        title={fresh === null ? words.about : updateLine(update, words)}
                        aria-label={`${words.about}, ${words.versionName(version)}${fresh === null ? "" : `, ${updateLine(update, words)}`}`}
                        onClick={() => showBanner("about")}
                    >
                        {words.versionTag(version)}
                    </button>
                )}
                {words.overview && <span className="sidebar-name">{words.overview}</span>}
                {waiting + unread > 0 && (
                    <button
                        type="button"
                        className="inbox-open"
                        title="List everything that waits for you"
                        onClick={() => showInbox(true)}
                    >
                        {waiting > 0 && <span className="needs-you">{words.needYou(waiting)}</span>}
                        {unread > 0 && <span className="unread-count">{words.unread(unread)}</span>}
                    </button>
                )}
            </div>
            {projects.length > 0 && (
                <button type="button" className="new-task-button" onClick={() => startNew()}>
                    {words.startTask}
                </button>
            )}
            <div className="sidebar-scroll">
                {load === "ready" && projects.length === 0 && <NoProjects />}
                {projects.map((project) => (
                    <ProjectGroup key={project.id} project={project} />
                ))}
            </div>
            {projects.length > 0 && (
                <div className="sidebar-bottom">
                    <button
                        type="button"
                        className="quiet-button quiet-button-centred"
                        onClick={() => void addProject()}
                    >
                        {words.addRepository}
                    </button>
                    <button
                        type="button"
                        className="quiet-button quiet-button-centred"
                        onClick={() => showSettings(true)}
                    >
                        {words.settings}
                    </button>
                </div>
            )}
        </nav>
    );
}

/** Up and down move between session rows, as in a list, without leaving Tab order behind. */
function moveFocus(event: KeyboardEvent<HTMLElement>) {
    const step = { ArrowDown: 1, ArrowUp: -1 }[event.key];
    // With a modifier the arrows are a shortcut's, such as the one that moves a row.
    if (step === undefined || event.metaKey || event.altKey || event.ctrlKey) {
        return;
    }
    const rows = [...event.currentTarget.querySelectorAll<HTMLButtonElement>(".session-row")];
    const current = rows.indexOf(document.activeElement as HTMLButtonElement);
    const next = rows[current === -1 ? 0 : current + step];
    if (next) {
        event.preventDefault();
        next.focus();
    }
}

function NoProjects() {
    const words = useWords();
    return (
        <div className="sidebar-empty">
            <p>{words.noRepositories}</p>
            <p>
                Add one to start {withArticle(words.terms.task)} in it. Swarmed never writes into
                the {words.terms.repository} itself.
            </p>
            <button
                type="button"
                className="button button-primary"
                onClick={() => void addProject()}
            >
                {words.addRepository}
            </button>
        </div>
    );
}

function ProjectGroup({ project }: { project: Project }) {
    const words = useStore((state) => wordsFor(state.settings?.style));
    const startNew = useStore((state) => state.startNew);
    const folded = useStore((state) => state.folded.includes(project.id));
    const toggleFolded = useStore((state) => state.toggleFolded);
    const tasks = project.tasks.filter((task) => !task.archived);
    // A folded repository still shows whether one of its sessions needs something.
    const status = folded ? projectStatus(project) : null;
    const order = orderAttributes(
        { kind: "project", project: project.name },
        orderGroup.projects(),
    );
    return (
        <section className="project" {...order}>
            <div className="row-head">
                <h2 className="project-name">
                    <button
                        type="button"
                        className="project-toggle"
                        data-drag-handle
                        aria-expanded={!folded}
                        title={folded ? words.showTasks : words.hideTasks}
                        onClick={() => toggleFolded(project.id)}
                    >
                        <svg className="fold-chevron" viewBox="0 0 10 10" aria-hidden="true">
                            <path d="M3.5 1.5 7 5 3.5 8.5" />
                        </svg>
                        <RepositoryIcon />
                        <span className="project-title">{project.name}</span>
                        {status && <StatusMark status={status} unread={projectUnread(project)} />}
                        {folded && <span className="project-count">{tasks.length}</span>}
                    </button>
                </h2>
                <CloudButton project={project.name} task={null} />
                <EnvironmentsButton project={project.name} />
                <NoteButton project={project.name} task={null} has={project.has_note} />
                <button
                    type="button"
                    className="row-add"
                    aria-label={`${words.startTask} in ${project.name}`}
                    title={words.startTask}
                    onClick={() => startNew(project.name)}
                >
                    +
                </button>
                <RemoveButton project={project.name} task={null} />
            </div>
            {!folded && tasks.length === 0 && <p className="hint">{words.noTasks}</p>}
            {!folded &&
                tasks.map((task) => <TaskGroup key={task.id} project={project.name} task={task} />)}
            {!folded && <CloudTasks project={project.name} tasks={project.cloud} />}
        </section>
    );
}

function TaskGroup({ project, task }: { project: string; task: Task }) {
    const words = useStore((state) => wordsFor(state.settings?.style));
    const order = orderAttributes(
        { kind: "task", project, task: task.name },
        orderGroup.tasks(project),
    );
    // A session is moved among those its starter started, or among the task's others.
    const sessionOrder = (session: Session) =>
        orderAttributes(
            { kind: "session", id: session.id },
            orderGroup.sessions(task.id, starterHere(task.sessions, session)),
        );
    return (
        <div className="task" {...order}>
            <div className="row-head">
                <h3 className="task-name" title={taskTooltip(task, words)} data-drag-handle>
                    <WorktreeIcon />
                    <span className="task-title">{task.name}</span>
                    {task.setup === "failed" && <span className="task-flag">setup failed</span>}
                </h3>
                <PullBadge project={project} task={task} />
                <CloudButton project={project} task={task.name} />
                <NoteButton project={project} task={task.name} has={task.has_note} />
                <button
                    type="button"
                    className="row-add"
                    aria-label={`${words.newSession} in ${task.name}`}
                    title={words.newSession}
                    onClick={() => void newSession(project, task.name)}
                >
                    +
                </button>
                <RemoveButton project={project} task={task.name} />
            </div>
            {task.sessions.length === 0 && <p className="hint">{words.noSessions}</p>}
            <ul className="sessions">
                {nestedSessions(task.sessions).map(({ session, nested }) => (
                    <li
                        key={session.id}
                        {...sessionOrder(session)}
                        data-nested={nested}
                        // One that started others is drawn larger, as the head of its brood.
                        data-brood={task.sessions.some((other) => other.spawned_by === session.id)}
                    >
                        <SessionRow session={session} />
                        <SubAgentRows session={session} />
                    </li>
                ))}
            </ul>
            {task.closed > 0 && (
                <ClosedSessions project={project} task={task.name} count={task.closed} />
            )}
        </div>
    );
}

/** Opens the note on a repository or a task. It stays in view once there is something noted. */
/** A cloud, drawn as one line. */
function CloudIcon() {
    return (
        <svg className="row-icon" viewBox="0 0 16 16" aria-hidden="true">
            <path d="M4.5 12.5a3 3 0 0 1-.4-5.97 4 4 0 0 1 7.7-.9A3.4 3.4 0 0 1 11.5 12.5z" />
        </svg>
    );
}

/** Opens the form that hands work to a cloud agent, from a repository or one of its tasks. */
function CloudButton({ project, task }: { project: string; task: string | null }) {
    const askToDelegate = useStore((state) => state.askToDelegate);
    const words = useStore((state) => wordsFor(state.settings?.style));
    const label = `${words.delegate} from ${task ?? project}`;
    return (
        <button
            type="button"
            className="row-note"
            aria-label={label}
            title={label}
            onClick={() => askToDelegate({ project, task })}
        >
            <CloudIcon />
        </button>
    );
}

/** How often the statuses of unfinished cloud tasks are asked for again. */
const CLOUD_POLL_MS = 45_000;

/**
 * The tasks handed to cloud agents from a repository: each opens at its provider. While any is
 * unfinished, their statuses are asked for again from time to time.
 */
function CloudTasks({ project, tasks }: { project: string; tasks: CloudTask[] }) {
    const words = useWords();
    const showCloudDiff = useStore((state) => state.showCloudDiff);
    const refresh = useStore((state) => state.refresh);
    const report = useStore((state) => state.report);
    const waiting = tasks.some(isUnfinished);
    // biome-ignore lint/correctness/useExhaustiveDependencies: restarted when the waiting starts or ends
    useEffect(() => {
        if (!waiting) {
            return;
        }
        const ask = () =>
            api
                .cloudTasks(project, true)
                .then(() => refresh(api))
                // Not hearing back is no reason to interrupt anyone.
                .catch(() => undefined);
        const timer = setInterval(ask, CLOUD_POLL_MS);
        return () => clearInterval(timer);
    }, [waiting, project]);
    // Asked first: the task goes on at its provider, but Swarmed's link to it is gone.
    const [forgetting, setForgetting] = useState<CloudTask | null>(null);
    const forget = (id: number) => {
        setForgetting(null);
        api.cloudTaskForget(id)
            .then(() => refresh(api))
            .catch(report);
    };
    return (
        <>
            {forgetting && (
                <ForgetCloudDialog
                    task={forgetting}
                    onForget={() => forget(forgetting.id)}
                    onKeep={() => setForgetting(null)}
                />
            )}
            <ul className="cloud-tasks">
                {tasks.map((task) => (
                    <li
                        key={task.id}
                        className="cloud-line"
                        data-tone={cloudTone(task)}
                        {...orderAttributes(
                            { kind: "cloud", id: task.id },
                            orderGroup.cloud(project),
                        )}
                    >
                        <button
                            type="button"
                            className="cloud-row"
                            data-drag-handle
                            title={cloudTooltip(task)}
                            onClick={() => void api.cloudTaskOpen(task.id).catch(report)}
                        >
                            <CloudIcon />
                            <span className="session-title">{cloudTitle(task)}</span>
                            <span className="cloud-status">{task.status}</span>
                            <ProviderMark
                                icon={iconForAgent(cloudProvider(task.provider).agent)}
                                label={cloudProvider(task.provider).name}
                            />
                        </button>
                        {task.provider === "codex" && !isUnfinished(task) && (
                            <button
                                type="button"
                                className="row-add"
                                aria-label={`Show what ${cloudTitle(task)} changed`}
                                title={`Show what it changed, and apply that to a local ${words.terms.task}`}
                                onClick={() => showCloudDiff(task.id)}
                            >
                                ±
                            </button>
                        )}
                        <button
                            type="button"
                            className="row-close"
                            aria-label={`${words.forgetCloud}: ${cloudTitle(task)}`}
                            title={`${words.forgetCloud}. The task itself is not stopped.`}
                            onClick={() => setForgetting(task)}
                        >
                            ×
                        </button>
                    </li>
                ))}
            </ul>
        </>
    );
}

/** Opens the repository's environments: the ways its tasks can be set up. */
function EnvironmentsButton({ project }: { project: string }) {
    const askForEnvironments = useStore((state) => state.askForEnvironments);
    const words = useStore((state) => wordsFor(state.settings?.style));
    const label = `${words.environments} of ${project}`;
    return (
        <button
            type="button"
            className="row-note"
            aria-label={label}
            title={label}
            onClick={() => askForEnvironments(project)}
        >
            <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M8 2 2 5l6 3 6-3z" />
                <path d="M2 8l6 3 6-3M2 11l6 3 6-3" />
            </svg>
        </button>
    );
}

function NoteButton({
    project,
    task,
    has,
}: {
    project: string;
    task: string | null;
    has: boolean;
}) {
    const askForNote = useStore((state) => state.askForNote);
    const label = `${has ? "Note" : "Add a note"} on ${task ?? project}`;
    return (
        <button
            type="button"
            className="row-note"
            data-has={has}
            aria-label={label}
            title={label}
            onClick={() => askForNote({ project, task })}
        >
            <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M4 2h6l3 3v9H4z" />
                <path d="M6.5 7.5h4M6.5 10.5h4" />
            </svg>
        </button>
    );
}

/** A repository: a bound book with a marker, as code hosts draw one. */
function RepositoryIcon() {
    return (
        <svg className="row-icon" viewBox="0 0 16 16" aria-hidden="true">
            <path d="M4.5 1.8h8v9.4h-8a1.6 1.6 0 0 0 0 3.2h8v-3.2" />
            <path d="M4.5 1.8a1.6 1.6 0 0 0-1.6 1.6v9.4M6.5 4.6h3.5" />
        </svg>
    );
}

/** A worktree: a branch leaving the trunk. */
function WorktreeIcon() {
    return (
        <svg className="row-icon" viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="4.5" cy="3.2" r="1.6" />
            <circle cx="4.5" cy="12.8" r="1.6" />
            <circle cx="11.5" cy="5.2" r="1.6" />
            <path d="M4.5 4.8v6.4M11.5 6.8c0 2.6-3.2 2.4-7 3.6" />
        </svg>
    );
}

interface ClosedProps {
    project: string;
    task: string;
    count: number;
}

/**
 * A task's closed sessions, folded away under its open ones. Closing a session, whether the
 * user did it or the agent that started it, only puts it here: it can be looked up and reopened.
 */
function ClosedSessions({ project, task, count }: ClosedProps) {
    const words = useStore((state) => wordsFor(state.settings?.style));
    const refresh = useStore((state) => state.refresh);
    const report = useStore((state) => state.report);
    const [sessions, setSessions] = useState<Session[] | null>(null);
    // Read when unfolded, and again whenever the number of closed sessions changes.
    // biome-ignore lint/correctness/useExhaustiveDependencies: count exists to trigger a re-read
    useEffect(() => {
        if (sessions !== null) {
            api.sessionsClosed(project, task).then(setSessions).catch(report);
        }
    }, [count]);
    const toggle = () => {
        if (sessions !== null) {
            return setSessions(null);
        }
        api.sessionsClosed(project, task).then(setSessions).catch(report);
    };
    const askToDelete = useStore((state) => state.askToDelete);
    const reopen = (id: number) =>
        api
            .sessionReopen(id)
            .then(() => refresh(api))
            .catch(report);
    return (
        <div className="closed-sessions">
            <button
                type="button"
                className="closed-toggle"
                aria-expanded={sessions !== null}
                onClick={toggle}
            >
                {words.closed(count)}
            </button>
            {sessions?.map((session) => (
                <div key={session.id} className="closed-row">
                    <span className="session-title" title={session.title}>
                        {session.title || words.untitled}
                    </span>
                    <button
                        type="button"
                        className="closed-reopen"
                        onClick={() => void reopen(session.id)}
                    >
                        {words.reopen}
                    </button>
                    <button
                        type="button"
                        className="row-close"
                        aria-label={`${words.deleteSession} ${session.title || words.untitled} for good`}
                        title={`${words.deleteSession} for good`}
                        onClick={() => askToDelete({ id: session.id, title: session.title })}
                    >
                        ×
                    </button>
                </div>
            ))}
        </div>
    );
}

/** Asks to remove a repository or a task. Nothing goes before the question is answered. */
function RemoveButton({ project, task }: { project: string; task: string | null }) {
    const words = useWords();
    const askToRemove = useStore((state) => state.askToRemove);
    const label =
        task === null ? `Remove ${project} from Swarmed` : `Remove the ${words.terms.task} ${task}`;
    return (
        <button
            type="button"
            className="row-close"
            aria-label={label}
            title={label}
            onClick={() => askToRemove({ project, task })}
        >
            ×
        </button>
    );
}

function SessionRow({ session }: { session: Session }) {
    const words = useWords();
    const selected = useStore((state) => state.selectedSessionId === session.id);
    const open = useStore((state) => state.open);
    const askToClose = useStore((state) => state.askToClose);
    const title = session.title || words.untitled;
    return (
        <div
            className={`session-line status-${session.status}`}
            data-current={selected}
            data-unread={session.unread}
        >
            <button
                type="button"
                className="session-row"
                data-drag-handle
                // The row cuts a long title short; this shows all of it.
                title={title}
                aria-current={selected ? "true" : undefined}
                onClick={() => void open(api, session.id)}
            >
                <StatusMark status={session.status} unread={session.unread} />
                {session.spawned_by !== null && (
                    <span
                        className="session-spawned"
                        title={words.startedBy(session.spawned_by)}
                        aria-label={words.startedBy(session.spawned_by)}
                        role="img"
                    >
                        <svg viewBox="0 0 12 12" aria-hidden="true">
                            <path d="M2 0.5C2 6.5 4 9 9 9" />
                            <circle cx="9.6" cy="9" r="1.4" />
                        </svg>
                    </span>
                )}
                <span className="session-title">{title}</span>
                <ProviderMark icon={iconForAgent(session.agent)} label={agentName(session.agent)} />
            </button>
            {session.mark && <SessionMark id={session.id} mark={session.mark} />}
            <button
                type="button"
                className="row-close"
                aria-label={`${words.archiveSession} or ${words.deleteSession.toLowerCase()} ${title}`}
                title={`${words.archiveSession} or ${words.deleteSession.toLowerCase()}`}
                onClick={() => askToClose(session.id)}
            >
                ×
            </button>
        </div>
    );
}

const NO_SUB_AGENTS: SubAgent[] = [];

/**
 * The sub-agents a session's agent has at work, under the session. They are the agent's own
 * doing: each is there while it works and gone when it has handed its answer back, and
 * pressing one shows what it says and does. Nothing can be said to one.
 */
function SubAgentRows({ session }: { session: Session }) {
    const words = useWords();
    const followed = useStore((state) => state.subAgents[session.id] ?? NO_SUB_AGENTS);
    const viewed = useStore((state) =>
        state.subView?.sessionId === session.id ? state.subView.id : null,
    );
    const openSub = useStore((state) => state.openSub);
    // While the session is idle, only those in the background are still at work.
    const atWork = listedUnder(followed, session.status);
    if (atWork.length === 0) {
        return null;
    }
    return (
        <ul className="sub-agents" aria-label={`${words.subAgent}s of ${session.title}`}>
            {atWork.map((one) => (
                <li key={one.id}>
                    <button
                        type="button"
                        className="sub-agent-row"
                        title={`${words.subAgent}: ${one.name || words.subAgent}${one.background ? ", in the background" : ""}`}
                        aria-current={viewed === one.id ? "true" : undefined}
                        onClick={() => void openSub(api, session.id, one.id)}
                    >
                        <span className="sub-agent-pulse" aria-hidden="true" />
                        <span className="session-title">{one.name || words.subAgent}</span>
                        {one.kind && <span className="sub-agent-kind">{one.kind}</span>}
                    </button>
                </li>
            ))}
        </ul>
    );
}

/**
 * The mark on a session: its colour, and its words if it has any. Pressing it takes it off,
 * for a mark that has said what it had to say.
 */
function SessionMark({ id, mark }: { id: number; mark: Mark }) {
    const report = useStore((state) => state.report);
    const said = mark.label === "" ? `Marked ${mark.colour}` : mark.label;
    return (
        <button
            type="button"
            className={`session-mark mark-${mark.colour}`}
            title={`${said}. Press to take the mark off.`}
            aria-label={`${said}. Take the mark off.`}
            onClick={() => api.sessionMark(id, "none", "").catch(report)}
        >
            <span className="session-mark-dot" aria-hidden="true" />
            {mark.label !== "" && <span className="session-mark-label">{mark.label}</span>}
        </button>
    );
}
