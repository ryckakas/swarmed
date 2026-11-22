import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { BackgroundTask } from "../api/types";
import { useStore } from "../store";
import { useWords } from "../useWords";

/** How often the list is read again: something left running can end at any moment. */
const POLL_MS = 4000;

/** What kind of thing was left running, by the name the agent gives its kind. */
const KINDS: Record<string, string> = {
    local_bash: "Command",
};

/**
 * What the task's agents left running, read again from time to time and whenever one of them
 * reports a start or an end. `null` until it has been read, a string if it could not be.
 */
export function useBackground(project: string, task: string) {
    const [tasks, setTasks] = useState<BackgroundTask[] | string | null>(null);
    const heard = useStore((state) => state.backgroundHeard);
    const read = useCallback(
        () => api.taskBackground(project, task).then(setTasks, (error) => setTasks(String(error))),
        [project, task],
    );
    // biome-ignore lint/correctness/useExhaustiveDependencies: a new report means read again
    useEffect(() => {
        void read();
        const timer = setInterval(read, POLL_MS);
        return () => clearInterval(timer);
    }, [read, heard]);
    return { tasks, read };
}

/** How many of them still run. */
export function runningCount(tasks: BackgroundTask[] | string | null): number {
    return Array.isArray(tasks) ? tasks.filter((task) => task.state === "running").length : 0;
}

interface Props {
    tasks: BackgroundTask[] | string | null;
    /** Reads the list again, after one of them was asked to stop. */
    onChanged(): void;
}

/** The list of what was left running, each with how it stands and a way to stop it. */
export function Background({ tasks, onChanged }: Props) {
    const words = useWords();
    if (tasks === null) {
        return <p className="changes-note">Reading…</p>;
    }
    if (typeof tasks === "string") {
        return <p className="changes-note changes-error">{tasks}</p>;
    }
    if (tasks.length === 0) {
        return <p className="changes-note">{words.noBackground}</p>;
    }
    return (
        <ul className="background-list">
            {tasks.map((task) => (
                <Row key={`${task.session_id}-${task.task_id}`} task={task} onChanged={onChanged} />
            ))}
        </ul>
    );
}

function Row({ task, onChanged }: { task: BackgroundTask; onChanged(): void }) {
    const words = useWords();
    const report = useStore((state) => state.report);
    const open = useStore((state) => state.open);
    const openSub = useStore((state) => state.openSub);
    // A sub-agent tied to the call that started it can be followed in its own conversation.
    const call = task.kind === "local_agent" ? task.tool_use_id : null;
    const [stopping, setStopping] = useState(false);
    const stop = () => {
        setStopping(true);
        api.sessionStopBackground(task.session_id, task.task_id)
            .then(onChanged, report)
            .finally(() => setStopping(false));
    };
    return (
        <li className="background-row" data-state={task.state}>
            {call === null ? (
                <p className="background-what">{task.description || task.task_id}</p>
            ) : (
                <button
                    type="button"
                    className="background-what background-open"
                    title={`Show what this ${words.subAgent} says and does`}
                    onClick={() => void openSub(api, task.session_id, call)}
                >
                    {task.description || task.task_id}
                </button>
            )}
            <p className="background-facts">
                <span className="background-state">{words.backgroundState[task.state]}</span>
                <span>
                    {task.kind === "local_agent"
                        ? words.backgroundAgent
                        : (KINDS[task.kind] ?? task.kind)}
                </span>
                <button
                    type="button"
                    className="background-session"
                    title={words.backgroundOwner}
                    onClick={() => void open(api, task.session_id)}
                >
                    {task.session_title || words.sessionNumbered(task.session_id)}
                </button>
            </p>
            {task.summary && task.state !== "running" && (
                <p className="background-summary">{task.summary}</p>
            )}
            {task.state === "running" && (
                <button
                    type="button"
                    className="button background-stop"
                    disabled={stopping}
                    onClick={stop}
                >
                    {words.stopBackground}
                </button>
            )}
        </li>
    );
}
