import { useState } from "react";
import { api } from "../api";
import type { Task } from "../api/types";
import { doneSaid, offersPutAway, putAwayDoes } from "../putAway";
import { useStore } from "../store";
import { usePull } from "../usePull";
import { useWords } from "../useWords";

interface Target {
    project: string;
    task: Task;
}

/** Asks one thing at a time, and says a refusal where the user is. */
function useOnce() {
    const report = useStore((state) => state.report);
    const [busy, setBusy] = useState(false);
    const run = async (asked: () => Promise<void>, refused: string) => {
        setBusy(true);
        try {
            await asked();
        } catch (error) {
            report(`${refused}: ${String(error).replace(/^Error: /, "")}`);
        } finally {
            setBusy(false);
        }
    };
    return { busy, run };
}

/**
 * Puts a task away: its agents and its stack stopped, and listed with the done ones. Nothing is
 * deleted, so it asks nothing first. `onDone` is told once it went.
 */
export function PutAwayButton(
    props: Target & { label?: string; primary?: boolean; onDone?(): void },
) {
    const { project, task, label, primary, onDone } = props;
    const settings = useStore((state) => state.settings);
    const putAway = useStore((state) => state.putAway);
    const { busy, run } = useOnce();
    const press = () =>
        run(async () => {
            await putAway(api, project, task.name);
            onDone?.();
        }, "Not put away");
    return (
        <button
            type="button"
            className={primary ? "button button-primary" : "button"}
            disabled={busy}
            title={putAwayDoes(settings)}
            onClick={() => void press()}
        >
            {busy ? "Putting away…" : (label ?? "Put away")}
        </button>
    );
}

/** Brings a task that was put away back to the others. Nothing is started. */
export function BringBackButton({ project, task }: Target) {
    const bringBack = useStore((state) => state.bringBack);
    const { busy, run } = useOnce();
    return (
        <button
            type="button"
            className="button"
            disabled={busy}
            title="Not done: lists it with the others again. Nothing is started."
            onClick={() => void run(() => bringBack(api, project, task.name), "Not brought back")}
        >
            Bring back
        </button>
    );
}

/**
 * In the task panel: when the task was put away and why, with the way back; the offer to put it
 * away once its pull request merged; or else a quiet way to put it away.
 */
export function PutAwayPart({ project, task }: Target) {
    const words = useWords();
    const pull = usePull(task);
    const said = doneSaid(task, Date.now() / 1000);
    if (said !== null) {
        return (
            <div className="put-away" data-done="">
                <p className="put-away-said">{said}.</p>
                <BringBackButton project={project} task={task} />
            </div>
        );
    }
    if (offersPutAway(task, pull)) {
        return (
            <div className="put-away" data-offer="">
                <p className="put-away-said">
                    Its {words.terms.pullRequest} merged. Put this {words.terms.task} away?
                </p>
                <PutAwayButton project={project} task={task} primary={true} />
            </div>
        );
    }
    return (
        <div className="panel-actions">
            <PutAwayButton project={project} task={task} />
        </div>
    );
}
