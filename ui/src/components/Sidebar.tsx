import { type FormEvent, type KeyboardEvent, useState } from "react";
import { api } from "../api";
import type { Project, Session, Task } from "../api/types";
import { useStore } from "../store";
import { countWaiting, taskNameFrom } from "../tree";
import { StatusMark } from "./StatusMark";

export function Sidebar() {
    const projects = useStore((state) => state.projects);
    const load = useStore((state) => state.load);
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
                        className="quiet-button"
                        onClick={() => void addProject()}
                    >
                        Add repository
                    </button>
                </div>
            )}
        </nav>
    );
}

/** Up and down move between session rows, as in a list, without leaving Tab order behind. */
function moveFocus(event: KeyboardEvent<HTMLElement>) {
    const step = { ArrowDown: 1, ArrowUp: -1 }[event.key];
    if (step === undefined || event.target instanceof HTMLInputElement) {
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

/** Asks for a folder and registers the repository it is in. */
async function addProject() {
    const { refresh, report } = useStore.getState();
    try {
        const path = await api.pickFolder();
        if (path !== null) {
            await api.projectAdd(path);
            await refresh(api);
        }
    } catch (error) {
        report(error);
    }
}

/** Creates a task and its first session, then opens that session. */
async function createTask(project: string, name: string) {
    const { refresh, open } = useStore.getState();
    await api.taskNew(project, name);
    const session = await api.sessionNew(project, name);
    await refresh(api);
    await open(api, session.id);
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
    const [adding, setAdding] = useState(false);
    const tasks = project.tasks.filter((task) => !task.archived);
    return (
        <section className="project">
            <div className="row-head">
                <h2 className="project-name">{project.name}</h2>
                <button
                    type="button"
                    className="row-add"
                    aria-label={`New task in ${project.name}`}
                    title="New task"
                    onClick={() => setAdding(true)}
                >
                    +
                </button>
            </div>
            {adding && <NewTask project={project.name} onClose={() => setAdding(false)} />}
            {tasks.length === 0 && !adding && <p className="hint">No tasks</p>}
            {tasks.map((task) => (
                <TaskGroup key={task.id} project={project.name} task={task} />
            ))}
        </section>
    );
}

/** A one-line form for a new task's name. Shows the hook's progress while the task is set up. */
function NewTask({ project, onClose }: { project: string; onClose: () => void }) {
    const [typed, setTyped] = useState("");
    const [busy, setBusy] = useState(false);
    const hookLine = useStore((state) => state.hookLine);
    const name = taskNameFrom(typed);

    const submit = async (event: FormEvent) => {
        event.preventDefault();
        if (name === "" || busy) {
            return;
        }
        setBusy(true);
        useStore.setState({ hookLine: null });
        try {
            await createTask(project, name);
            onClose();
        } catch (error) {
            // A task whose setup failed still exists; show it, so it can be inspected.
            const { refresh, report } = useStore.getState();
            report(error);
            await refresh(api);
            setBusy(false);
        }
    };

    if (busy) {
        return (
            <p className="hint new-task-progress" aria-live="polite">
                Setting up {name}… {hookLine}
            </p>
        );
    }
    return (
        <form className="new-task" onSubmit={(event) => void submit(event)}>
            <input
                // biome-ignore lint/a11y/noAutofocus: the field appears because the user asked for it
                autoFocus
                aria-label={`Name of the new task in ${project}`}
                placeholder="Task name, e.g. fix-login"
                value={typed}
                onChange={(event) => setTyped(event.target.value)}
                onKeyDown={(event) => event.key === "Escape" && onClose()}
                onBlur={() => typed === "" && onClose()}
            />
            {name !== "" && name !== typed && <p className="hint">Will be created as {name}</p>}
        </form>
    );
}

function TaskGroup({ project, task }: { project: string; task: Task }) {
    const newSession = async () => {
        const { refresh, open, report } = useStore.getState();
        try {
            const session = await api.sessionNew(project, task.name);
            await refresh(api);
            await open(api, session.id);
        } catch (error) {
            report(error);
        }
    };
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
                    onClick={() => void newSession()}
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
    return (
        <button
            type="button"
            className={`session-row status-${session.status}`}
            aria-current={selected ? "true" : undefined}
            onClick={() => void open(api, session.id)}
        >
            <StatusMark status={session.status} />
            <span className="session-title">{session.title || "New session"}</span>
        </button>
    );
}
