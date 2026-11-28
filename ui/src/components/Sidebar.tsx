import { type CSSProperties, type KeyboardEvent, type ReactNode, useEffect, useState } from "react";
import { addProject, moveRow, newSession, pinRow, snoozeSession } from "../actions";
import { api } from "../api";
import type { CloudTask, Mark, Project, Session, Task } from "../api/types";
import { sessionForBrowser } from "../browser";
import { CHATS, chatsOf, repositoriesOf } from "../chats";
import { cloudProvider, cloudTitle, cloudTone, cloudTooltip, isUnfinished } from "../cloud";
import { taskTooltip } from "../environments";
import { writtenHotkey } from "../hotkeys";
import {
    gripTitle,
    orderAttributes,
    orderGroup,
    type PinTarget,
    pinLabel,
    pinTitle,
} from "../ordering";
import { agentName, iconForAgent } from "../providerIcons";
import { doneSaid, doneTasks, openTasks } from "../putAway";
import { clockTime } from "../rateLimit";
import { isSettingUp, setupShown, sidebarSays } from "../setup";
import { awakeSessions, snoozedAtWork, snoozedSessions, statusSeen, wakeTime } from "../snooze";
import { useStore } from "../store";
import type { SubAgent } from "../subagents";
import { listedUnder } from "../subagents";
import {
    countUnread,
    countWaiting,
    findSession,
    foldedSummary,
    nestedSessions,
    parentElsewhere,
    parentHere,
    projectBackground,
    projectStatus,
    projectUnread,
    saysNoSessions,
} from "../tree";
import { newVersion, updateLine } from "../updates";
import { useNow } from "../useNow";
import { useWords } from "../useWords";
import { type Words, withArticle, wordsFor } from "../wording";
import { ForgetCloudDialog } from "./ForgetCloudDialog";
import { HighlightButton, HighlightWords } from "./Highlight";
import {
    CloseIcon,
    CloudIcon,
    PlusIcon,
    PlusMinusIcon,
    SnoozeIcon,
    TurningGlobeIcon,
} from "./IconButton";
import { Incubating } from "./Incubating";
import { ProviderMark } from "./ProviderMark";
import { PullBadge } from "./PullRequest";
import { RoleChip } from "./Role";
import { StatusMark, SubAgentMark } from "./StatusMark";
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
    // A snoozed session does not count until it wakes.
    const now = useStore((state) => state.snoozeNow);
    const waiting = countWaiting(projects, now);
    const unread = countUnread(projects, now);
    const words = useStore((state) => wordsFor(state.settings?.style));
    const startDrag = useRowDrag(moveRow);
    const chats = chatsOf(projects);
    const repositories = repositoriesOf(projects);

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
            {/* A chat needs no repository, so a task can always be started. */}
            <button type="button" className="new-task-button" onClick={() => startNew()}>
                {words.startTask}
            </button>
            <div className="sidebar-scroll">
                {/* The room the rows have, without a scroll bar: what they draw short by. */}
                <div className="sidebar-rows">
                    {chats && <ChatsGroup home={chats} />}
                    {load === "ready" && repositories.length === 0 && <NoProjects />}
                    {repositories.map((project) => (
                        <ProjectGroup key={project.id} project={project} />
                    ))}
                </div>
            </div>
            {repositories.length > 0 && (
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
    const now = useStore((state) => state.snoozeNow);
    const tasks = openTasks(project);
    const done = doneTasks(project);
    // A folded repository still shows whether one of its sessions needs something.
    const status = folded ? projectStatus(project, now) : null;
    const target = { kind: "project", project: project.name } as const;
    const order = orderAttributes(target, orderGroup.projects(project.pinned));
    return (
        <section className="project" {...order}>
            <div className="row-head">
                <RowParts
                    offered={
                        <>
                            <CloudButton project={project.name} task={null} />
                            <IssuesButton project={project.name} />
                            <EnvironmentsButton project={project.name} />
                            {!project.has_note && (
                                <NoteButton project={project.name} task={null} has={false} />
                            )}
                        </>
                    }
                    pin={<PinButton target={target} pinned={project.pinned} name={project.name} />}
                    shown={
                        project.has_note && <NoteButton project={project.name} task={null} has />
                    }
                >
                    <h2 className="project-name">
                        <button
                            type="button"
                            className="project-toggle"
                            data-drag-handle
                            aria-expanded={!folded}
                            title={folded ? words.showTasks : words.hideTasks}
                            onClick={() => toggleFolded(project.id)}
                        >
                            <FoldChevron />
                            <RepositoryIcon />
                            <span className="project-title">{project.name}</span>
                            {status && (
                                <StatusMark
                                    status={status}
                                    unread={projectUnread(project, now)}
                                    background={projectBackground(project)}
                                />
                            )}
                            {folded && <span className="project-count">{tasks.length}</span>}
                        </button>
                    </h2>
                </RowParts>
                <button
                    type="button"
                    className="row-add"
                    aria-label={`${words.startTask} in ${project.name}`}
                    title={words.startTask}
                    onClick={() => startNew(project.name)}
                >
                    <ButtonIcon>
                        <PlusIcon />
                    </ButtonIcon>
                </button>
                <RemoveButton project={project.name} task={null} />
            </div>
            {!folded && tasks.length === 0 && done.length === 0 && (
                <p className="hint">{words.noTasks}</p>
            )}
            {!folded &&
                tasks.map((task) => <TaskGroup key={task.id} project={project.name} task={task} />)}
            {!folded && <CloudTasks project={project.name} tasks={project.cloud} />}
            {!folded && done.length > 0 && (
                <DoneGroup project={project.name} projectId={project.id} tasks={done} />
            )}
        </section>
    );
}

/**
 * The chats, sessions with no repository, above the repositories and apart from them: each
 * chat is drawn as a task is, with what only a repository has left out. They fold like a
 * repository's tasks, and are not moved among the repositories.
 */
function ChatsGroup({ home }: { home: Project }) {
    const words = useStore((state) => wordsFor(state.settings?.style));
    const startNew = useStore((state) => state.startNew);
    const folded = useStore((state) => state.folded.includes(home.id));
    const toggleFolded = useStore((state) => state.toggleFolded);
    const now = useStore((state) => state.snoozeNow);
    const chats = home.tasks.filter((task) => !task.archived);
    if (chats.length === 0) {
        return null;
    }
    const status = folded ? projectStatus(home, now) : null;
    return (
        <section className="project project-chats">
            <div className="row-head">
                <h2 className="project-name">
                    <button
                        type="button"
                        className="project-toggle"
                        aria-expanded={!folded}
                        title={folded ? `Show the ${words.chat}s` : `Hide the ${words.chat}s`}
                        onClick={() => toggleFolded(home.id)}
                    >
                        <FoldChevron />
                        <ChatIcon />
                        <span className="project-title">{words.chats}</span>
                        {status && (
                            <StatusMark
                                status={status}
                                unread={projectUnread(home, now)}
                                background={projectBackground(home)}
                            />
                        )}
                        {folded && <span className="project-count">{chats.length}</span>}
                    </button>
                </h2>
                <button
                    type="button"
                    className="row-add"
                    aria-label={`New ${words.chat}`}
                    title={`New ${words.chat}`}
                    onClick={() => startNew(CHATS)}
                >
                    <ButtonIcon>
                        <PlusIcon />
                    </ButtonIcon>
                </button>
            </div>
            {!folded &&
                chats.map((task) => (
                    <TaskGroup key={task.id} project={home.name} task={task} chat />
                ))}
        </section>
    );
}

/**
 * The tasks put away, folded under the repository's others until the user unfolds them. Each
 * keeps its sessions, its pull request and its note, and offers the way back and removal.
 */
function DoneGroup(props: { project: string; projectId: number; tasks: Task[] }) {
    const { project, projectId, tasks } = props;
    const words = useWords();
    const shown = useStore((state) => state.doneShown.includes(projectId));
    const toggleDoneShown = useStore((state) => state.toggleDoneShown);
    return (
        <div className="done-group">
            <button
                type="button"
                className="closed-toggle done-toggle"
                aria-expanded={shown}
                title={
                    shown
                        ? `Hide the ${words.terms.tasks} put away`
                        : `Show the ${words.terms.tasks} put away`
                }
                onClick={() => toggleDoneShown(projectId)}
            >
                {words.done(tasks.length)}
            </button>
            {shown &&
                tasks.map((task) => <DoneTaskGroup key={task.id} project={project} task={task} />)}
        </div>
    );
}

/** A task put away: no order of its own, and the way back in place of a new session. */
function DoneTaskGroup({ project, task }: { project: string; task: Task }) {
    const folded = useStore((state) => state.foldedTasks.includes(task.id));
    return (
        <div className="task task-done">
            <div className="row-head" data-highlight={task.highlight ?? undefined}>
                <RowParts
                    offered={
                        !task.has_note && (
                            <NoteButton project={project} task={task.name} has={false} />
                        )
                    }
                    shown={
                        <>
                            {task.has_note && <NoteButton project={project} task={task.name} has />}
                            <IssueBadge project={project} task={task} />
                            <PullBadge project={project} task={task} />
                        </>
                    }
                >
                    <TaskHeading task={task} folded={folded} chat={false} />
                </RowParts>
                <BringBackRow project={project} task={task.name} />
                <RemoveButton project={project} task={task.name} />
            </div>
            {!folded && <TaskSessions project={project} task={task} />}
        </div>
    );
}

/**
 * Brings a task put away back to the others, in the place where a task listed with them has its
 * button for a new session. Nothing is started.
 */
function BringBackRow({ project, task }: { project: string; task: string }) {
    const bringBack = useStore((state) => state.bringBack);
    const report = useStore((state) => state.report);
    return (
        <button
            type="button"
            className="row-add row-back"
            aria-label={`Bring ${task} back`}
            title="Not done: lists it with the others again. Nothing is started."
            onClick={() => void bringBack(api, project, task).catch(report)}
        >
            <ButtonIcon>
                <path d="M6 3.5 2.5 7 6 10.5" />
                <path d="M3 7h6.5a3.5 3.5 0 0 1 0 7H8" />
            </ButtonIcon>
        </button>
    );
}

function TaskGroup({ project, task, chat }: { project: string; task: Task; chat?: boolean }) {
    const words = useStore((state) => wordsFor(state.settings?.style));
    const folded = useStore((state) => state.foldedTasks.includes(task.id));
    const target = { kind: "task", project, task: task.name } as const;
    const order = orderAttributes(target, orderGroup.tasks(project, task.pinned));
    return (
        <div className="task" {...order}>
            <div className="row-head" data-highlight={task.highlight ?? undefined}>
                <Grip />
                <RowParts
                    offered={
                        <>
                            <HighlightButton
                                target={target}
                                colour={task.highlight}
                                name={task.name}
                            />
                            {!chat && <CloudButton project={project} task={task.name} />}
                            {!task.has_note && (
                                <NoteButton project={project} task={task.name} has={false} />
                            )}
                        </>
                    }
                    pin={<PinButton target={target} pinned={task.pinned} name={task.name} />}
                    shown={<TaskShown project={project} task={task} chat={chat === true} />}
                >
                    <TaskHeading task={task} folded={folded} chat={chat === true} />
                </RowParts>
                <button
                    type="button"
                    className="row-add"
                    aria-label={`${words.newSession} in ${task.name}`}
                    title={words.newSession}
                    onClick={() => void newSession(project, task.name)}
                >
                    <ButtonIcon>
                        <PlusIcon />
                    </ButtonIcon>
                </button>
                <RemoveButton project={project} task={task.name} chat={chat === true} />
            </div>
            {!folded && <TaskSessions project={project} task={task} />}
        </div>
    );
}

/**
 * What a task's row shows by itself, the least lasting first: a note that holds something, which
 * a narrow sidebar leaves to the pointer beside the badges (#500), so that the badges stay put
 * when it comes; the browser's badge while it runs; then the issue and the pull request.
 */
function TaskShown({ project, task, chat }: { project: string; task: Task; chat: boolean }) {
    return (
        <>
            {task.has_note && <NoteButton project={project} task={task.name} has />}
            <BrowserBadge task={task} />
            {!chat && <IssueBadge project={project} task={task} />}
            {!chat && <PullBadge project={project} task={task} />}
        </>
    );
}

/**
 * A task's name, which folds its sessions away and brings them back, as a repository's name
 * does its tasks. Folded, it still says how many sessions are open, what the most urgent of
 * them needs and whether one holds an unread answer, as a folded repository does, a snoozed one
 * counting as idle and read but for its work; and how many of them are snoozed, in its title.
 */
function TaskHeading({ task, folded, chat }: { task: Task; folded: boolean; chat: boolean }) {
    const words = useStore((state) => wordsFor(state.settings?.style));
    const toggleTaskFolded = useStore((state) => state.toggleTaskFolded);
    const now = useStore((state) => state.snoozeNow);
    const summary = folded ? foldedSummary(task, now) : null;
    const fold = folded ? words.showSessions : words.hideSessions;
    const done = doneSaid(task, Date.now() / 1000);
    const where = chat ? `A ${words.chat}, in ${task.worktree_path}` : taskTooltip(task, words);
    const asleep = summary && summary.snoozed > 0 ? `. ${words.snoozed(summary.snoozed)}` : "";
    const what = `${done === null ? where : `${where}. ${done}`}${asleep}`;
    return (
        <h3 className="task-name">
            <button
                type="button"
                className="task-toggle"
                aria-expanded={!folded}
                title={`${what}. ${fold}`}
                onClick={() => toggleTaskFolded(task.id)}
            >
                <FoldChevron />
                {chat ? <ChatIcon /> : <WorktreeIcon />}
                <span className="task-title">{task.name}</span>
                <HighlightWords colour={task.highlight} />
                <SetupFlag task={task} />
                {summary?.status && (
                    <StatusMark
                        status={summary.status}
                        unread={summary.unread}
                        background={summary.background}
                    />
                )}
                {summary && <span className="task-count">{summary.count}</span>}
            </button>
        </h3>
    );
}

/** A flag on a task whose setup failed or was cut short. */
function SetupFlag({ task }: { task: Task }) {
    const says = sidebarSays(task, undefined, 0);
    if (says === null || says.busy) {
        return null;
    }
    return <span className="task-flag">{says.text}</span>;
}

/**
 * Under a task being set up: how long it has run and what it printed last, or that it waits
 * for a place, in place of "nothing spawned yet". Nothing in a setup's first moments
 * (`setupShown`), which one with nothing to run never outlasts.
 */
function SetupLine({ task }: { task: Task }) {
    const busy = isSettingUp(task);
    const seen = useStore((state) => state.setups[task.id]);
    const now = useNow(busy);
    const says = busy && setupShown(task, seen, now) ? sidebarSays(task, seen, now) : null;
    if (says === null) {
        return null;
    }
    return (
        <p className="setup-line" title={says.text}>
            <Incubating />
            {says.text}
        </p>
    );
}

/** How many steps in a session is drawn at most: deeper ones line up with the deepest. */
const DRAWN_DEPTH = 4;

/**
 * A task's open sessions that are awake, each under the one it works for, the sub-agents at
 * work under them, then the line to its snoozed ones and the line to its closed ones. A session
 * that works for a snoozed one is listed with the others, as the core counts it among them when
 * a row is moved.
 */
function TaskSessions({ project, task }: { project: string; task: Task }) {
    const words = useWords();
    const now = useStore((state) => state.snoozeNow);
    const listed = awakeSessions(task.sessions, now);
    const asleep = snoozedSessions(task.sessions, now);
    // A session is moved among the others under its parent, or among the task's others.
    const sessionOrder = (session: Session) =>
        orderAttributes(
            { kind: "session", id: session.id },
            orderGroup.sessions(task.id, parentHere(listed, session), session.pinned),
        );
    return (
        <>
            <SetupLine task={task} />
            {saysNoSessions(task) && <p className="hint">{words.noSessions}</p>}
            <ul className="sessions">
                {nestedSessions(listed).map(({ session, depth }) => (
                    <li
                        key={session.id}
                        {...sessionOrder(session)}
                        data-nested={depth > 0}
                        style={{ "--depth": Math.min(depth, DRAWN_DEPTH) } as CSSProperties}
                        // One that others work for is drawn larger, as the head of its brood.
                        data-brood={listed.some(
                            (other) => parentHere(listed, other) === session.id,
                        )}
                    >
                        <SessionRow
                            session={session}
                            nested={depth > 0}
                            elsewhere={parentElsewhere(listed, session)}
                        />
                        <SubAgentRows session={session} />
                    </li>
                ))}
            </ul>
            {asleep.length > 0 && <SnoozedSessions task={task.id} sessions={asleep} />}
            {task.closed > 0 && (
                <ClosedSessions project={project} task={task.name} count={task.closed} />
            )}
        </>
    );
}

/** A cloud, drawn as one line: what kind of row a cloud task is. */
function CloudRowIcon() {
    return (
        <svg className="row-icon" viewBox="0 0 16 16" aria-hidden="true">
            <CloudIcon />
        </svg>
    );
}

/**
 * Opens the form that hands work to a cloud agent, from a repository or one of its tasks. Its
 * cloud is a button's icon, not a row's: a layout enlarges the button, and a row's icon inside
 * it would be enlarged a second time, larger than the icons beside it.
 */
function CloudButton({ project, task }: { project: string; task: string | null }) {
    const askToDelegate = useStore((state) => state.askToDelegate);
    const words = useStore((state) => wordsFor(state.settings?.style));
    const label = `${words.delegate} from ${task ?? project}`;
    return (
        <button
            type="button"
            className="row-note row-cloud"
            aria-label={label}
            title={label}
            onClick={() => askToDelegate({ project, task })}
        >
            <ButtonIcon>
                <CloudIcon />
            </ButtonIcon>
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
                        <Grip />
                        <button
                            type="button"
                            className="cloud-row"
                            title={cloudTooltip(task)}
                            onClick={() => void api.cloudTaskOpen(task.id).catch(report)}
                        >
                            <CloudRowIcon />
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
                                <ButtonIcon>
                                    <PlusMinusIcon />
                                </ButtonIcon>
                            </button>
                        )}
                        <button
                            type="button"
                            className="row-close"
                            aria-label={`${words.forgetCloud}: ${cloudTitle(task)}`}
                            title={`${words.forgetCloud}. The task itself is not stopped.`}
                            onClick={() => setForgetting(task)}
                        >
                            <ButtonIcon>
                                <CloseIcon />
                            </ButtonIcon>
                        </button>
                    </li>
                ))}
            </ul>
        </>
    );
}

/** Opens the repository's GitHub issues, to read them and start a task on one. */
function IssuesButton({ project }: { project: string }) {
    const showIssues = useStore((state) => state.showIssues);
    const label = `Issues of ${project}`;
    return (
        <button
            type="button"
            className="row-note"
            aria-label={label}
            title={`${label}: read them and start a task on one`}
            onClick={() => showIssues({ project, issue: null })}
        >
            <IssueIcon />
        </button>
    );
}

/** An issue as GitHub draws one: a ring with a dot in it. */
function IssueIcon() {
    return (
        <svg viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="8" cy="8" r="5.5" />
            <circle cx="8" cy="8" r="1.2" />
        </svg>
    );
}

/**
 * The issue a task was started for, beside its pull request: pressing it shows the issue in
 * the repository's Issues view.
 */
function IssueBadge({ project, task }: { project: string; task: Task }) {
    const showIssues = useStore((state) => state.showIssues);
    if (task.issue === null) {
        return null;
    }
    const label = `For issue #${task.issue}: show it`;
    return (
        <button
            type="button"
            className="issue-badge"
            title={label}
            aria-label={label}
            onClick={() => showIssues({ project, issue: task.issue })}
        >
            <IssueIcon />
            {/* Left out in a narrow sidebar, where the ring alone stands for it (#500). */}
            <span className="issue-number">{task.issue}</span>
        </button>
    );
}

/**
 * A task whose own browser runs is marked so, as anything left running is: pressing the mark
 * shows the browser beside one of the task's sessions.
 */
function BrowserBadge({ task }: { task: Task }) {
    const words = useWords();
    const selected = useStore((state) => state.selectedSessionId);
    const open = useStore((state) => state.open);
    const toggleBrowser = useStore((state) => state.toggleBrowser);
    if (!task.browser) {
        return null;
    }
    const session = sessionForBrowser(task, selected);
    const label = `Its own browser runs, until it sits unused for a while or the ${words.terms.task} is put away: show it`;
    const show = () => {
        if (session !== null && session !== selected) {
            void open(api, session);
        }
        toggleBrowser(task.id, true);
    };
    return (
        <button
            type="button"
            className="issue-badge browser-badge"
            title={label}
            aria-label={label}
            disabled={session === null}
            onClick={show}
        >
            <svg viewBox="0 0 16 16" aria-hidden="true">
                <TurningGlobeIcon />
            </svg>
        </button>
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

/**
 * The pin on a repository, a task or a session: out of the way until the row is pointed at,
 * and in view once it is pinned. Pressing it pins the row above its siblings that are not
 * pinned, or unpins it. It stands between what the row offers and what it shows (`RowParts`), so
 * that it moves neither when the pointer comes nor when it is pressed.
 */
function PinButton({ target, pinned, name }: { target: PinTarget; pinned: boolean; name: string }) {
    return (
        <button
            type="button"
            className="row-pin"
            data-pinned={pinned}
            aria-pressed={pinned}
            aria-label={pinLabel(pinned, name)}
            title={pinTitle(pinned)}
            onClick={() => void pinRow(target, !pinned)}
        >
            <ButtonIcon>
                <PinIcon />
            </ButtonIcon>
        </button>
    );
}

/** A pin, pushed in at a slant: its head, the bar it is held by, and its point. */
function PinIcon() {
    return (
        <>
            <path
                className="pin-head"
                d="M9.2 2.2 13.8 6.8 11.6 7.6 8.9 10.3 8.4 13 3 7.6 5.7 7.1 8.4 4.4z"
            />
            <path d="M5.7 10.3 2.4 13.6" />
        </>
    );
}

/** Opens the note on a repository or a task. It stays in view once there is something noted. */
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

/**
 * A row's name and what follows it, in the one order that keeps everything the row shows without
 * the pointer in place when the pointer comes to press it. The name (`children`) comes first; then
 * what the row offers only while it is pointed at or focused (`offered`), hidden until then by
 * one rule for the lot (`.row-offered`); then its pin, which is either and is pressed where it
 * stands, so it sits at the seam and pinning or unpinning moves it nowhere. Those three share one
 * part (`.row-main`), whose size the pointer does not change: what shows under it takes its room
 * from the name alone. Then what the row shows by itself (`shown`): its badges and marks, a note
 * that holds something, and whatever some work puts there while it runs. The row's + and × come
 * after, and always keep their room. A button that is either by its state goes into the part its
 * state says, as the note does.
 */
function RowParts(props: {
    children: ReactNode;
    offered?: ReactNode;
    pin?: ReactNode;
    shown?: ReactNode;
}) {
    return (
        <>
            <span className="row-main">
                {props.children}
                <span className="row-offered">{props.offered}</span>
                {props.pin}
            </span>
            <span className="row-shown">{props.shown}</span>
        </>
    );
}

/**
 * The drawing on one of a row's buttons, in the same box and strokes as the others beside it.
 * The add and remove buttons are drawn too: a text glyph sits where its font puts it, off their
 * centre line, and changes size and weight with every font and layout.
 */
function ButtonIcon({ children }: { children: ReactNode }) {
    return (
        <svg viewBox="0 0 16 16" aria-hidden="true">
            {children}
        </svg>
    );
}

/**
 * What a task, a session or a cloud task is dragged by: six dots in the indent before its row,
 * shown while the row is pointed at or has the keyboard. The row itself is not a handle, so
 * pressing it opens it and a click that slips moves nothing. It cannot take the keyboard; the
 * shortcuts its title names move a row from there.
 */
function Grip() {
    return (
        <span className="row-grip" data-drag-handle title={gripTitle()} aria-hidden="true">
            <svg className="grip-dots" viewBox="0 0 8 12" aria-hidden="true">
                <circle cx="2" cy="2" r="1.2" />
                <circle cx="6" cy="2" r="1.2" />
                <circle cx="2" cy="6" r="1.2" />
                <circle cx="6" cy="6" r="1.2" />
                <circle cx="2" cy="10" r="1.2" />
                <circle cx="6" cy="10" r="1.2" />
            </svg>
        </span>
    );
}

/** What a repository's or a task's name shows of whether it is folded: it turns down when open. */
function FoldChevron() {
    return (
        <svg className="fold-chevron" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M3.5 1.5 7 5 3.5 8.5" />
        </svg>
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

/** A chat: a speech bubble. */
function ChatIcon() {
    return (
        <svg className="row-icon" viewBox="0 0 16 16" aria-hidden="true">
            <path d="M2.5 3.5h11v7.2H7l-3.2 2.6v-2.6H2.5z" />
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
                        <ButtonIcon>
                            <CloseIcon />
                        </ButtonIcon>
                    </button>
                </div>
            ))}
        </div>
    );
}

/**
 * A task's snoozed sessions, folded away under its open ones as its closed ones are, the one to
 * wake first first. Each opens from its title, says when it wakes, and wakes now from its
 * button. While one of them works, or has something running in the background, the line says
 * so with the busiest one's mark: work under the fold is never hidden.
 */
function SnoozedSessions({ task, sessions }: { task: number; sessions: Session[] }) {
    const words = useWords();
    const shown = useStore((state) => state.snoozedShown.includes(task));
    const toggleSnoozedShown = useStore((state) => state.toggleSnoozedShown);
    const atWork = snoozedAtWork(sessions);
    return (
        <div className="closed-sessions snoozed-sessions">
            <button
                type="button"
                className="closed-toggle snoozed-toggle"
                aria-expanded={shown}
                title={shown ? "Hide the snoozed sessions" : "Show the snoozed sessions"}
                onClick={() => toggleSnoozedShown(task)}
            >
                {atWork && <StatusMark status={atWork.status} background={atWork.background} />}
                {words.snoozed(sessions.length)}
            </button>
            {shown && sessions.map((session) => <SnoozedRow key={session.id} session={session} />)}
        </div>
    );
}

/** A snoozed session under its task's Snoozed line: its mark and title, when it wakes, and Wake. */
function SnoozedRow({ session }: { session: Session }) {
    const words = useWords();
    const now = useStore((state) => state.snoozeNow);
    const selected = useStore((state) => state.selectedSessionId === session.id);
    const open = useStore((state) => state.open);
    const title = session.title || words.untitled;
    const until = session.snoozed_until ?? 0;
    const when = wakeTime(until, new Date(now));
    return (
        <div className="closed-row snoozed-row" data-current={selected}>
            <button
                type="button"
                className="snoozed-open"
                title={`${title}. Snoozed until ${when}`}
                aria-current={selected ? "true" : undefined}
                onClick={() => void open(api, session.id)}
            >
                {/* Counted as idle and read while snoozed, but its work still shows. */}
                <StatusMark status={statusSeen(session, now)} background={session.background} />
                <span className="session-title">{title}</span>
            </button>
            <span className="snoozed-until">{when}</span>
            <button
                type="button"
                className="closed-reopen"
                aria-label={`${words.wake} ${title} now`}
                title={`${words.wake} now: listed with the others again`}
                onClick={() => void snoozeSession(session.id, null)}
            >
                {words.wake}
            </button>
        </div>
    );
}

/**
 * Snoozes a session until a time, among what its row offers under the pointer or with the
 * keyboard: the dialog of choices opens.
 */
function SnoozeButton({ id, name }: { id: number; name: string }) {
    const askToSnooze = useStore((state) => state.askToSnooze);
    return (
        <button
            type="button"
            className="row-snooze"
            aria-label={`Snooze ${name}`}
            title={`Snooze: out of the list and the inbox until a time. ${writtenHotkey("snooze")} on the focused row, or the open session.`}
            onClick={() => askToSnooze(id)}
        >
            <ButtonIcon>
                <SnoozeIcon />
            </ButtonIcon>
        </button>
    );
}

/** Asks to remove a repository, a task or a chat. Nothing goes before the question is answered. */
function RemoveButton(props: { project: string; task: string | null; chat?: boolean }) {
    const { project, task } = props;
    const words = useWords();
    const askToRemove = useStore((state) => state.askToRemove);
    const kind = props.chat ? words.chat : words.terms.task;
    const label = task === null ? `Remove ${project} from Swarmed` : `Remove the ${kind} ${task}`;
    return (
        <button
            type="button"
            className="row-close"
            aria-label={label}
            title={label}
            onClick={() => askToRemove({ project, task })}
        >
            <ButtonIcon>
                <CloseIcon />
            </ButtonIcon>
        </button>
    );
}

/**
 * A session's row. One `nested` under the session it works for hangs off it by a tendril; one
 * that works for a session the task does not list, of another task or closed, says which
 * (`elsewhere`), and pressing that opens it.
 */
function SessionRow(props: { session: Session; nested: boolean; elsewhere: number | null }) {
    const { session, nested, elsewhere } = props;
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
            data-highlight={session.highlight ?? undefined}
        >
            <Grip />
            <RowParts
                offered={
                    <>
                        <HighlightButton
                            target={{ kind: "session", id: session.id }}
                            colour={session.highlight}
                            name={title}
                        />
                        <SnoozeButton id={session.id} name={title} />
                    </>
                }
                pin={
                    <PinButton
                        target={{ kind: "session", id: session.id }}
                        pinned={session.pinned}
                        name={title}
                    />
                }
                shown={<SessionShown session={session} elsewhere={elsewhere} />}
            >
                <button
                    type="button"
                    className="session-row"
                    // The row cuts a long title short; this shows all of it.
                    title={title}
                    aria-current={selected ? "true" : undefined}
                    onClick={() => void open(api, session.id)}
                >
                    <StatusMark
                        status={session.status}
                        unread={session.unread}
                        background={session.background}
                    />
                    {nested && session.parent !== null && (
                        <span
                            className="session-spawned"
                            title={hangsOff(words, session, session.parent)}
                            aria-label={hangsOff(words, session, session.parent)}
                            role="img"
                        >
                            <svg viewBox="0 0 12 12" aria-hidden="true">
                                <path d="M2 0.5C2 6.5 4 9 9 9" />
                                <circle cx="9.6" cy="9" r="1.4" />
                            </svg>
                        </span>
                    )}
                    <span className="session-title">{title}</span>
                    <RoleChip role={session.role} />
                    <HighlightWords colour={session.highlight} />
                    {/* The icon stands outside the button, so the button says the agent's name. */}
                    <span className="unseen">, {agentName(session.agent)}</span>
                </button>
            </RowParts>
            <button
                type="button"
                className="row-close"
                aria-label={`${words.archiveSession} or ${words.deleteSession.toLowerCase()} ${title}`}
                title={`${words.archiveSession} or ${words.deleteSession.toLowerCase()}`}
                onClick={() => askToClose(session.id)}
            >
                <ButtonIcon>
                    <CloseIcon />
                </ButtonIcon>
            </button>
        </div>
    );
}

/** What a session's line shows by itself: whom it works for, its mark, its continuation, its agent. */
function SessionShown({ session, elsewhere }: { session: Session; elsewhere: number | null }) {
    const open = useStore((state) => state.open);
    return (
        <>
            {elsewhere !== null && <ParentLink id={elsewhere} />}
            {session.mark && <SessionMark id={session.id} mark={session.mark} />}
            {session.continue_at !== null && (
                <SessionContinues id={session.id} at={session.continue_at} />
            )}
            <RowAgent agent={session.agent} onPress={() => void open(api, session.id)} />
        </>
    );
}

/**
 * A session's agent, by its icon at the end of its row: after whom the session works for, its
 * mark and its continuation, and before only its ×, so that the icons of a task's sessions stand
 * in one column, pointed at or not. Those are buttons of their own, so the icon cannot be in the
 * row's button; a press on it opens the session all the same. Hidden from screen readers, to
 * which the row's button says the agent's name, and never focused: it is a second place to
 * press the row, for the pointer alone.
 */
function RowAgent({ agent, onPress }: { agent: string; onPress: () => void }) {
    return (
        <span className="row-agent" title={agentName(agent)} aria-hidden="true" onClick={onPress}>
            <ProviderMark icon={iconForAgent(agent)} />
        </span>
    );
}

/** What the tendril of a session listed under the one it works for, `parent`, says. */
function hangsOff(words: Words, session: Session, parent: number): string {
    return session.spawned ? words.startedBy(parent) : words.worksFor(parent);
}

/**
 * The session a row works for when it is not listed under it, by its number: pressing it opens
 * that one, if it is open.
 */
export function ParentLink({ id }: { id: number }) {
    const words = useWords();
    const parent = useStore((state) => findSession(state.projects, id));
    const open = useStore((state) => state.open);
    const named = parent ? `${words.worksFor(id)}: ${parent.title || words.untitled}` : "";
    return (
        <button
            type="button"
            className="parent-link"
            disabled={!parent}
            title={parent ? named : `${words.worksFor(id)}, which is closed`}
            aria-label={parent ? `Open ${named}` : `${words.worksFor(id)}, which is closed`}
            onClick={() => void open(api, id)}
        >
            {/* Left out in a narrow sidebar, where the number alone stands for it (#500). */}
            <span className="parent-for">for </span>#{id}
        </button>
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
                        <SubAgentMark />
                        <span className="session-title">{one.name || words.subAgent}</span>
                        {one.kind && <span className="sub-agent-kind">{one.kind}</span>}
                    </button>
                </li>
            ))}
        </ul>
    );
}

/**
 * The clock beside a session a usage limit stopped, which goes on by itself when the limit
 * resets, with the time it does. Pressing it cancels that.
 */
function SessionContinues({ id, at }: { id: number; at: number }) {
    const report = useStore((state) => state.report);
    const time = clockTime(at, new Date());
    const said = `Goes on by itself at ${time}, when the usage limit resets`;
    return (
        <button
            type="button"
            className="session-continues"
            title={`${said}. Press to cancel.`}
            aria-label={`${said}. Cancel.`}
            onClick={() => api.sessionContinueCancel(id).catch(report)}
        >
            <svg className="session-continues-clock" viewBox="0 0 12 12" aria-hidden="true">
                <circle cx="6" cy="6" r="4.8" />
                <path d="M6 3.4V6l1.8 1.2" />
            </svg>
            <span>{time}</span>
        </button>
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
