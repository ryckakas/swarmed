import { useState } from "react";
import { api } from "../api";
import type { Descendant, Task } from "../api/types";
import { alsoCloseElsewhereSaid, workerRows } from "../closing";
import { doneSaid, offersPutAway, putAwayDoes } from "../putAway";
import { useStore } from "../store";
import { usePull } from "../usePull";
import { useWords } from "../useWords";
import { WorkersToClose } from "./CloseWorkers";
import { Dialog } from "./Dialog";

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
 * deleted, so it asks nothing first, unless sessions of other tasks work for its sessions: then
 * it asks whether to close those too. `onDone` is told once it went.
 */
export function PutAwayButton(
    props: Target & { label?: string; primary?: boolean; onDone?(): void },
) {
    const { project, task, label, primary, onDone } = props;
    const settings = useStore((state) => state.settings);
    const putAway = useStore((state) => state.putAway);
    const { busy, run } = useOnce();
    const [asking, setAsking] = useState<Descendant[] | null>(null);
    const put = (closeWorkers: boolean) =>
        run(async () => {
            await putAway(api, project, task.name, closeWorkers);
            setAsking(null);
            onDone?.();
        }, "Not put away");
    const press = () =>
        run(async () => {
            const elsewhere = await api.taskDescendantsElsewhere(project, task.name);
            if (elsewhere.length > 0) {
                setAsking(elsewhere);
                return;
            }
            await putAway(api, project, task.name, false);
            onDone?.();
        }, "Not put away");
    return (
        <>
            <button
                type="button"
                className={primary ? "button button-primary" : "button"}
                disabled={busy}
                title={putAwayDoes(settings)}
                onClick={() => void press()}
            >
                {busy ? "Putting away…" : (label ?? "Put away")}
            </button>
            {asking && (
                <PutAwayAsk
                    task={task}
                    under={asking}
                    busy={busy}
                    onPut={(closeWorkers) => void put(closeWorkers)}
                    onClose={() => setAsking(null)}
                />
            )}
        </>
    );
}

interface AskProps {
    task: Task;
    /** The open sessions of other tasks that work for its sessions, as the core listed them. */
    under: Descendant[];
    busy: boolean;
    onPut(closeWorkers: boolean): void;
    onClose(): void;
}

/**
 * Asks before a task is put away while sessions of other tasks work for its sessions: they are
 * listed, and a tick box, unticked to begin with, closes them too. Their tasks stay.
 */
function PutAwayAsk({ task, under, busy, onPut, onClose }: AskProps) {
    const words = useWords();
    const settings = useStore((state) => state.settings);
    const projects = useStore((state) => state.projects);
    const [ticked, setTicked] = useState(false);
    const rows = workerRows(under, projects, task.id);
    const { task: taskWord, tasks } = words.terms;
    return (
        <Dialog
            title={`Put this ${taskWord} away?`}
            size="medium"
            closeOnBackdrop={!ticked}
            busy={busy}
            onClose={onClose}
            dismiss="Keep it"
            actions={
                <button
                    type="button"
                    className="button button-primary"
                    onClick={() => onPut(ticked && rows.length > 0)}
                >
                    {busy ? "Putting away…" : "Put away"}
                </button>
            }
        >
            <p className="dialog-subject">{task.name}</p>
            <p>{putAwayDoes(settings)}</p>
            {rows.length > 0 && (
                <>
                    <WorkersToClose
                        heading={`In other ${tasks}, working for its ${words.session}s:`}
                        rows={rows}
                        label={alsoCloseElsewhereSaid(rows.length, words)}
                        ticked={ticked}
                        disabled={busy}
                        onTick={setTicked}
                    />
                    <p>
                        Their {tasks} are not put away either way: ticked, only these{" "}
                        {words.session}s are closed.
                    </p>
                </>
            )}
        </Dialog>
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
