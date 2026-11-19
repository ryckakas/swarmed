import type { Project, Session, SessionEvent, SessionStatus, Task } from "./api/types";

// Pure functions over the project tree, kept apart from the store so they can be tested.

/** The tree with one session's status replaced. Untouched branches keep their identity. */
export function withSessionStatus(
    projects: Project[],
    sessionId: number,
    status: SessionStatus,
): Project[] {
    const inSession = (session: Session) =>
        session.id === sessionId ? { ...session, status } : session;
    const inTask = (task: Task) =>
        task.sessions.some((session) => session.id === sessionId)
            ? { ...task, sessions: task.sessions.map(inSession) }
            : task;
    const inProject = (project: Project) =>
        project.tasks.some((task) => task.sessions.some((session) => session.id === sessionId))
            ? { ...project, tasks: project.tasks.map(inTask) }
            : project;
    return projects.map(inProject);
}

/** Applies what a published event means for the sidebar. */
export function applySessionEvent(projects: Project[], published: SessionEvent): Project[] {
    return published.status === null
        ? projects
        : withSessionStatus(projects, published.session_id, published.status);
}

export function findSession(projects: Project[], sessionId: number | null): Session | undefined {
    return projects
        .flatMap((project) => project.tasks)
        .flatMap((task) => task.sessions)
        .find((session) => session.id === sessionId);
}

/** The task a session belongs to, with its project. */
export function findSessionHome(
    projects: Project[],
    session: Session,
): { project: Project; task: Task } | undefined {
    for (const project of projects) {
        const task = project.tasks.find((candidate) => candidate.id === session.task_id);
        if (task) {
            return { project, task };
        }
    }
    return undefined;
}

/** How many sessions are waiting for a decision. */
export function countWaiting(projects: Project[]): number {
    return projects
        .flatMap((project) => project.tasks)
        .flatMap((task) => task.sessions)
        .filter((session) => session.status === "waiting").length;
}

/** The one status that best describes a task: the most urgent among its sessions. */
export function taskStatus(task: Task): SessionStatus | null {
    const urgency: SessionStatus[] = ["waiting", "errored", "working", "idle"];
    return urgency.find((status) => task.sessions.some((s) => s.status === status)) ?? null;
}
