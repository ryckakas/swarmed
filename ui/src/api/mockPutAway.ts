// Putting the sample tasks away and bringing them back, as the core does (`put_away.rs`): their
// agents and stack stopped, their sessions closed only if the settings say so, nothing deleted.

import { deepestFirst, descendantsElsewhere, type KeptSession } from "../closing";
import type { Api } from "./client";
import type { Project, Settings, Task } from "./types";

/** What putting a sample task away needs of the rest of the mock. */
export interface PutAwayHost {
    projects(): Project[];
    settings(): Settings;
    /** Stops a session's agent: a turn under way ends, as Stop ends it. */
    stop(id: number): void;
    /** Closes a session, keeping its conversation. */
    close(id: number): void;
    /** Every session of a task not removed, open or closed. */
    kept(): KeptSession[];
}

/** Seconds since 1970, as the core counts when a task was put away. */
function secondsNow(): number {
    return Math.floor(Date.now() / 1000);
}

/** Brings a put away task back to the others, as anything that works on it again does. */
export function bringBack(task: Task) {
    task.done_at = null;
    task.done_why = null;
}

/**
 * Closes the open sessions of other tasks that work for task `taskId`'s, the deepest first,
 * when asked to.
 */
function closeElsewhere(host: PutAwayHost, taskId: number, asked: boolean) {
    if (!asked) {
        return;
    }
    for (const id of deepestFirst(descendantsElsewhere(host.kept(), taskId))) {
        host.close(id);
    }
}

export function putAwayApi(
    host: PutAwayHost,
): Pick<Api, "taskDone" | "taskDescendantsElsewhere" | "taskUndone"> {
    const find = (project: string, name: string) => {
        const tasks = host.projects().find((one) => one.name === project)?.tasks ?? [];
        const task = tasks.find((one) => one.name === name && !one.archived);
        if (!task) {
            throw new Error(`task \`${name}\` not found in project \`${project}\``);
        }
        return task;
    };
    return {
        taskDone: async (project, name, closeWorkers) => {
            const task = find(project, name);
            if (task.done_at !== null) {
                return [];
            }
            closeElsewhere(host, task.id, closeWorkers);
            const close = host.settings().put_away_closes_sessions;
            for (const session of [...task.sessions]) {
                host.stop(session.id);
                // A stopped agent asks nothing and works on nothing; a failure still shows.
                if (session.status !== "errored") {
                    session.status = "idle";
                }
                if (close) {
                    host.close(session.id);
                }
            }
            task.running = [];
            if (task.stack !== null) {
                task.stack = "stopped";
            }
            task.done_at = secondsNow();
            task.done_why = "hand";
            return [];
        },
        taskDescendantsElsewhere: async (project, name) =>
            descendantsElsewhere(host.kept(), find(project, name).id),
        taskUndone: async (project, name) => bringBack(find(project, name)),
    };
}
