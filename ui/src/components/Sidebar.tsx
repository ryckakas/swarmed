import type { KeyboardEvent } from "react";
import type { Project, Session, Task } from "../api/types";
import { useStore } from "../store";
import { countWaiting } from "../tree";
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
            <p>No projects yet.</p>
            <p>Add a repository from a terminal:</p>
            <code>swarmed project add ~/code/my-repo</code>
        </div>
    );
}

function ProjectGroup({ project }: { project: Project }) {
    const tasks = project.tasks.filter((task) => !task.archived);
    return (
        <section className="project">
            <h2 className="project-name">{project.name}</h2>
            {tasks.length === 0 && <p className="hint">No tasks</p>}
            {tasks.map((task) => (
                <TaskGroup key={task.id} task={task} />
            ))}
        </section>
    );
}

function TaskGroup({ task }: { task: Task }) {
    return (
        <div className="task">
            <h3 className="task-name">
                {task.name}
                {task.setup === "failed" && <span className="task-flag">setup failed</span>}
            </h3>
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
    const select = useStore((state) => state.select);
    return (
        <button
            type="button"
            className={`session-row status-${session.status}`}
            aria-current={selected ? "true" : undefined}
            onClick={() => select(session.id)}
        >
            <StatusMark status={session.status} />
            <span className="session-title">{session.title || "New session"}</span>
        </button>
    );
}
