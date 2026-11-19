import type { KeyboardEvent } from "react";
import { addProject, newSession } from "../actions";
import { api } from "../api";
import type { Project, Session, Task } from "../api/types";
import { useStore } from "../store";
import { countWaiting, projectStatus } from "../tree";
import { StatusMark } from "./StatusMark";

export function Sidebar() {
    const projects = useStore((state) => state.projects);
    const load = useStore((state) => state.load);
    const startNew = useStore((state) => state.startNew);
    const showSettings = useStore((state) => state.showSettings);
    const waiting = countWaiting(projects);

    return (
        <nav className="sidebar" aria-label="Projects, tasks and sessions" onKeyDown={moveFocus}>
            <div className="sidebar-top" data-tauri-drag-region>
                {waiting > 0 && (
                    <span className="needs-you">
                        {waiting === 1 ? "1 session needs you" : `${waiting} sessions need you`}
                    </span>
                )}
            </div>
            {projects.length > 0 && (
                <button type="button" className="new-task-button" onClick={() => startNew()}>
                    New task
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
                        Add repository
                    </button>
                    <button
                        type="button"
                        className="quiet-button quiet-button-centred"
                        onClick={() => showSettings(true)}
                    >
                        Settings
                    </button>
                </div>
            )}
        </nav>
    );
}

/** Up and down move between session rows, as in a list, without leaving Tab order behind. */
function moveFocus(event: KeyboardEvent<HTMLElement>) {
    const step = { ArrowDown: 1, ArrowUp: -1 }[event.key];
    if (step === undefined) {
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
    return (
        <div className="sidebar-empty">
            <p>No repositories yet.</p>
            <p>Add one to start a task in it. Swarmed never writes into the repository itself.</p>
            <button
                type="button"
                className="button button-primary"
                onClick={() => void addProject()}
            >
                Add repository
            </button>
        </div>
    );
}

function ProjectGroup({ project }: { project: Project }) {
    const startNew = useStore((state) => state.startNew);
    const folded = useStore((state) => state.folded.includes(project.id));
    const toggleFolded = useStore((state) => state.toggleFolded);
    const tasks = project.tasks.filter((task) => !task.archived);
    // A folded repository still shows whether one of its sessions needs something.
    const status = folded ? projectStatus(project) : null;
    return (
        <section className="project">
            <div className="row-head">
                <h2 className="project-name">
                    <button
                        type="button"
                        className="project-toggle"
                        aria-expanded={!folded}
                        title={folded ? "Show this repository's tasks" : "Hide its tasks"}
                        onClick={() => toggleFolded(project.id)}
                    >
                        <svg className="fold-chevron" viewBox="0 0 10 10" aria-hidden="true">
                            <path d="M3.5 1.5 7 5 3.5 8.5" />
                        </svg>
                        <span className="project-title">{project.name}</span>
                        {status && <StatusMark status={status} />}
                        {folded && <span className="project-count">{tasks.length}</span>}
                    </button>
                </h2>
                <button
                    type="button"
                    className="row-add"
                    aria-label={`New task in ${project.name}`}
                    title="New task"
                    onClick={() => startNew(project.name)}
                >
                    +
                </button>
            </div>
            {!folded && tasks.length === 0 && <p className="hint">No tasks</p>}
            {!folded &&
                tasks.map((task) => <TaskGroup key={task.id} project={project.name} task={task} />)}
        </section>
    );
}

function TaskGroup({ project, task }: { project: string; task: Task }) {
    return (
        <div className="task">
            <div className="row-head">
                <h3 className="task-name">
                    {task.name}
                    {task.setup === "failed" && <span className="task-flag">setup failed</span>}
                </h3>
                <button
                    type="button"
                    className="row-add"
                    aria-label={`New session in ${task.name}`}
                    title="New session"
                    onClick={() => void newSession(project, task.name)}
                >
                    +
                </button>
            </div>
            {task.sessions.length === 0 && <p className="hint">No sessions</p>}
            <ul className="sessions">
                {task.sessions.map((session) => (
                    <li key={session.id}>
                        <SessionRow session={session} />
                    </li>
                ))}
            </ul>
        </div>
    );
}

function SessionRow({ session }: { session: Session }) {
    const selected = useStore((state) => state.selectedSessionId === session.id);
    const open = useStore((state) => state.open);
    const askToClose = useStore((state) => state.askToClose);
    const title = session.title || "New session";
    return (
        <div className={`session-line status-${session.status}`} data-current={selected}>
            <button
                type="button"
                className="session-row"
                // The row cuts a long title short; this shows all of it.
                title={title}
                aria-current={selected ? "true" : undefined}
                onClick={() => void open(api, session.id)}
            >
                <StatusMark status={session.status} />
                <span className="session-title">{title}</span>
            </button>
            <button
                type="button"
                className="row-close"
                aria-label={`Close ${title}`}
                title="Close session"
                onClick={() => askToClose(session.id)}
            >
                ×
            </button>
        </div>
    );
}
