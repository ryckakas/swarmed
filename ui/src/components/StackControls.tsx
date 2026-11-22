import { useEffect, useState } from "react";
import { api } from "../api";
import type { StackState, Task } from "../api/types";
import { useStore } from "../store";
import { useWords } from "../useWords";

/** How often a running stack is looked at again: whether it still runs, and what it printed. */
const POLL_MS = 4000;

/** What the longer hint under the pointer says of each state. */
const HINTS: Record<StackState, string> = {
    stopped: "Its start command is not running.",
    starting:
        "Its start command runs, but nothing answers at its address yet. It may still be installing or building.",
    running: "Something answers at its address.",
};

/**
 * What a task keeps running, such as its dev server: whether it is up, a button that starts or
 * stops it, and the end of what it has printed. Nothing is shown for a task whose project has
 * no start command.
 */
export function StackControls({ project, task }: { project: string; task: Task }) {
    const report = useStore((state) => state.report);
    const words = useWords();
    const refresh = useStore((state) => state.refresh);
    const [busy, setBusy] = useState(false);
    const [log, setLog] = useState<string | null>(null);
    const running = task.stack !== null && task.stack !== "stopped";

    // While it runs, or its log is open, look again from time to time: a stack can end by itself.
    // biome-ignore lint/correctness/useExhaustiveDependencies: restarted when what is watched changes
    useEffect(() => {
        if (!(running || log !== null)) {
            return;
        }
        const look = () => {
            void refresh(api);
            if (log !== null) {
                api.stackLog(project, task.name).then(setLog, () => undefined);
            }
        };
        const timer = setInterval(look, POLL_MS);
        return () => clearInterval(timer);
    }, [running, log !== null, project, task.name]);

    if (task.stack === null) {
        return null;
    }
    const toggle = () => {
        setBusy(true);
        const done = running
            ? api.stackStop(project, task.name)
            : api.stackStart(project, task.name).then(() => undefined);
        done.then(() => refresh(api))
            .catch(report)
            .finally(() => setBusy(false));
    };
    const toggleLog = () => {
        if (log !== null) {
            return setLog(null);
        }
        api.stackLog(project, task.name).then(setLog, report);
    };

    return (
        <div className="stack">
            <p className="stack-state" data-state={task.stack} title={HINTS[task.stack]}>
                {words.stack[task.stack]}
            </p>
            <button type="button" className="button" disabled={busy} onClick={toggle}>
                {stackAction(running, busy, words.stackStarting)}
            </button>
            <button
                type="button"
                className="button"
                aria-expanded={log !== null}
                onClick={toggleLog}
            >
                {log === null ? "Show log" : "Hide log"}
            </button>
            {log !== null && <pre className="stack-log mono">{log || "Nothing printed yet."}</pre>}
        </div>
    );
}

/** What the button that starts or stops a stack says. */
export function stackAction(running: boolean, busy: boolean, starting = "Starting…"): string {
    if (busy) {
        return running ? "Stopping…" : starting;
    }
    return running ? "Stop" : "Start";
}
