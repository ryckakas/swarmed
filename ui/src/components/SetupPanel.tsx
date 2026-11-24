import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Task } from "../api/types";
import {
    elapsed,
    headline,
    isSettingUp,
    lastLines,
    ranSince,
    type SetupState,
    setupState,
    whyNotDone,
} from "../setup";
import { useStore } from "../store";
import { useNow } from "../useNow";
import { Incubating } from "./Incubating";

/** How many of the last lines a setup printed are shown. */
const SHOWN_LINES = 400;

/** How often the log of a setup another Swarmed runs is read again, in milliseconds. */
const ELSEWHERE_EVERY = 2000;

/**
 * How the setup of the task on screen goes, or why it did not finish, with what can be done
 * about it: stopped while it runs; run again, started anyway, its log shown, or the task
 * removed once it failed. Nothing for a task that is set up.
 */
export function SetupPanel({ project, task }: { project: string; task: Task }) {
    const state = setupState(task);
    const busy = isSettingUp(task);
    const began = task.setting_up?.state === "here" ? task.setting_up.began : null;
    const seen = useStore((store) => store.setups[task.id]);
    const readSetupLog = useStore((store) => store.readSetupLog);
    // The log is open while the setup runs, and once asked for after.
    const [asked, setAsked] = useState(false);
    const shown = busy || asked;
    const now = useNow(state === "running");
    // What this window did not hear: read once for each run, and again while another
    // Swarmed runs it, whose reports never reach this one.
    useEffect(() => {
        if (state === "done") {
            return;
        }
        const target = { project, task: task.name, taskId: task.id };
        const read = () => void readSetupLog(api, target, began);
        read();
        if (state !== "elsewhere") {
            return;
        }
        const timer = setInterval(read, ELSEWHERE_EVERY);
        return () => clearInterval(timer);
    }, [project, task.name, task.id, began, state, readSetupLog]);
    if (state === "done") {
        return null;
    }
    return (
        <section className="setup-panel" data-state={state} aria-live="polite">
            <header className="setup-head">
                {busy && <Incubating />}
                <strong>{headline(task, seen)}</strong>
                {began !== null && <span className="setup-time">{elapsed(began, now)}</span>}
            </header>
            {!busy && <p className="setup-reason">{whyNotDone(task)}</p>}
            {shown && <SetupOutput text={seen?.text ?? ""} />}
            <SetupActions
                project={project}
                task={task}
                state={state}
                shown={shown}
                onShow={() => setAsked(!asked)}
            />
        </section>
    );
}

/** What the setup printed, the end of it, following what comes while it is at the bottom. */
function SetupOutput({ text }: { text: string }) {
    const box = useRef<HTMLPreElement>(null);
    const following = useRef(true);
    const said = lastLines(text, SHOWN_LINES);
    // biome-ignore lint/correctness/useExhaustiveDependencies: `said` is what moves the bottom
    useLayoutEffect(() => {
        const pre = box.current;
        if (pre && following.current) {
            pre.scrollTop = pre.scrollHeight;
        }
    }, [said]);
    return (
        <pre
            ref={box}
            className="setup-output mono"
            onScroll={(event) => {
                const pre = event.currentTarget;
                following.current = pre.scrollHeight - pre.scrollTop - pre.clientHeight < 8;
            }}
        >
            {said || "Nothing printed yet."}
        </pre>
    );
}

interface ActionsProps {
    project: string;
    task: Task;
    state: SetupState;
    shown: boolean;
    onShow(): void;
}

/**
 * Runs the task's setup again. How it goes is heard as it goes, and how it ended shows in the
 * panel, not as an error; only a refusal before any setup ran, which nothing else says, is one.
 */
function runAgain(project: string, task: Task): void {
    const { report, refresh } = useStore.getState();
    const before = useStore.getState().setups[task.id];
    void api.taskSetup(project, task.name).catch((error: unknown) => {
        if (!ranSince(before, useStore.getState().setups[task.id])) {
            report(error);
        }
        return refresh(api);
    });
}

/** What can be done about a setup: stop one under way; or act on one that did not finish. */
function SetupActions({ project, task, state, shown, onShow }: ActionsProps) {
    const { report, refresh, askToRemove } = useStore.getState();
    const waits = task.sessions.some((session) => session.waits_for_setup);
    if (state === "running") {
        return (
            <div className="setup-actions">
                <button
                    type="button"
                    className="button"
                    title="Ends setup with everything it started; the task stays, to be set up again"
                    onClick={() => api.taskSetupStop(project, task.name).catch(report)}
                >
                    Stop setup
                </button>
            </div>
        );
    }
    if (state === "elsewhere") {
        return null;
    }
    return (
        <div className="setup-actions">
            <button
                type="button"
                className="button button-primary"
                title={
                    waits ? "Runs setup again, then sends what waits for it" : "Runs setup again"
                }
                onClick={() => runAgain(project, task)}
            >
                Run setup again
            </button>
            {waits && (
                <button
                    type="button"
                    className="button"
                    title="Sends what waits for the setup now, in the worktree as it is"
                    onClick={() =>
                        api
                            .taskStartAnyway(project, task.name)
                            .then(() => refresh(api))
                            .catch(report)
                    }
                >
                    Start anyway
                </button>
            )}
            <button type="button" className="button" aria-expanded={shown} onClick={onShow}>
                {shown ? "Hide log" : "Show log"}
            </button>
            <button
                type="button"
                className="button button-danger"
                onClick={() => askToRemove({ project, task: task.name })}
            >
                Remove task
            </button>
        </div>
    );
}
