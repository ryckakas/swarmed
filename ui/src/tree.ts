import type { Project, Session, SessionEvent, SessionStatus, Task } from "./api/types";

// Pure functions over the project tree, kept apart from the store so they can be tested.

/** The tree with one session's status replaced. Untouched branches keep their identity. */
export function withSessionStatus(
    projects: Project[],
    sessionId: number,
    status: SessionStatus,
): Project[] {
    return mapSession(projects, sessionId, (session) => ({ ...session, status }));
}

/** The tree with one session replaced by a newer copy of itself. */
export function withSession(projects: Project[], session: Session): Project[] {
    return mapSession(projects, session.id, () => session);
}

/** The tree without one session. */
export function withoutSession(projects: Project[], sessionId: number): Project[] {
    const inTask = (task: Task) => ({
        ...task,
        sessions: task.sessions.filter((session) => session.id !== sessionId),
    });
    return projects.map((project) => ({ ...project, tasks: project.tasks.map(inTask) }));
}

/** How long a title taken from a message may be; the same limit the core uses. */
const TITLE_CHARS = 60;

/** The title the core gives a session from its first message: the first line, shortened. */
export function titleFrom(message: string): string {
    const line = (message.split("\n")[0] ?? "").trim();
    return line.length <= TITLE_CHARS ? line : `${line.slice(0, TITLE_CHARS - 1)}…`;
}

function mapSession(
    projects: Project[],
    sessionId: number,
    change: (session: Session) => Session,
): Project[] {
    const inSession = (session: Session) => (session.id === sessionId ? change(session) : session);
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

/**
 * Applies what a published event means for the sidebar: the session's new status, and, for
 * the first message of an untitled session, the title the core has just given it.
 */
export function applySessionEvent(projects: Project[], published: SessionEvent): Project[] {
    const { session_id: id, status, event } = published;
    if (event.type === "renamed") {
        return mapSession(projects, id, (session) => ({ ...session, title: event.title }));
    }
    if (event.type === "supervision_changed") {
        const { supervision } = event;
        return mapSession(projects, id, (session) => ({ ...session, supervision }));
    }
    const firstMessage = event.type === "user_message" ? event.text : null;
    if (status === null && firstMessage === null) {
        return projects;
    }
    return mapSession(projects, id, (session) => ({
        ...session,
        status: status ?? session.status,
        title:
            session.title === "" && firstMessage !== null ? titleFrom(firstMessage) : session.title,
    }));
}

export function findSession(projects: Project[], sessionId: number | null): Session | undefined {
    return projects
        .flatMap((project) => project.tasks)
        .flatMap((task) => task.sessions)
        .find((session) => session.id === sessionId);
}

/** The id of the task a session belongs to. */
export function taskIdOf(projects: Project[], sessionId: number): number | undefined {
    return findSession(projects, sessionId)?.task_id;
}

/** Events after which a task's files may differ: a tool finished, or the turn did. */
export function mayHaveEdited(event: SessionEvent["event"]): boolean {
    return event.type === "tool_result" || event.type === "turn_done";
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

/** The one status that best describes a repository: the most urgent among its open tasks. */
export function projectStatus(project: Project): SessionStatus | null {
    const sessions = project.tasks.filter((task) => !task.archived).flatMap((t) => t.sessions);
    const urgency: SessionStatus[] = ["waiting", "errored", "working", "idle"];
    return urgency.find((status) => sessions.some((s) => s.status === status)) ?? null;
}

/** `ids` with `id` added if it was missing, removed if it was there. */
export function toggledId(ids: number[], id: number): number[] {
    return ids.includes(id) ? ids.filter((existing) => existing !== id) : [...ids, id];
}

/**
 * Turns what someone types into a task name the core accepts: lowercase letters, digits,
 * `-`, `_` and `.`, starting with a letter or digit. Anything else becomes a dash.
 */
export function taskNameFrom(typed: string): string {
    return typed
        .toLowerCase()
        .replace(/[^a-z0-9._-]+/g, "-")
        .replace(/\.{2,}/g, ".")
        .replace(/^[-_.]+|[-_.]+$/g, "")
        .slice(0, 64);
}

/** How long a task name taken from a prompt may be. */
const PROMPT_NAME_CHARS = 40;

/** A task name for a prompt: its first line as a name, cut at a word where possible. */
export function taskNameForPrompt(prompt: string): string {
    const name = taskNameFrom(prompt.split("\n")[0] ?? "");
    if (name.length <= PROMPT_NAME_CHARS) {
        return name || "task";
    }
    const cut = name.slice(0, PROMPT_NAME_CHARS);
    const lastDash = cut.lastIndexOf("-");
    return (lastDash > PROMPT_NAME_CHARS / 2 ? cut.slice(0, lastDash) : cut).replace(/[-_.]+$/, "");
}

/** `name`, or `name-2`, `name-3` and so on if tasks with that name already exist. */
export function uniqueTaskName(name: string, taken: string[]): string {
    if (!taken.includes(name)) {
        return name;
    }
    for (let suffix = 2; ; suffix += 1) {
        const candidate = `${name}-${suffix}`;
        if (!taken.includes(candidate)) {
            return candidate;
        }
    }
}
