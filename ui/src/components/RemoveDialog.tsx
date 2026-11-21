import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Project, RemoveTarget } from "../api/types";
import { useStore } from "../store";

/** Asks before a repository or a task is removed from Swarmed, and says what goes with it. */
export function RemoveDialog() {
    const removing = useStore((state) => state.removing);
    const project = useStore((state) =>
        state.projects.find((candidate) => candidate.name === state.removing?.project),
    );
    if (removing === null || !project) {
        return null;
    }
    return <Question key={removalKey(removing)} target={removing} project={project} />;
}

/** What names a removal: a repository, or one of its tasks. */
function removalKey(target: RemoveTarget): string {
    return `${target.project}/${target.task ?? ""}`;
}

/** What the dialog says about a repository, or about one of its tasks. */
function wording(target: RemoveTarget, project: Project) {
    const tasks = project.tasks.filter((task) => !task.archived);
    if (target.task === null) {
        const worktrees = tasks.length === 1 ? "1 task" : `${tasks.length} tasks`;
        return {
            title: `Remove ${project.name} from Swarmed?`,
            what: `Its ${worktrees}, their worktrees and sessions, and your notes on it are deleted. The repository itself and its branches stay as they are.`,
            action: "Remove repository",
        };
    }
    const branch = tasks.find((task) => task.name === target.task)?.branch;
    return {
        title: `Remove the task ${target.task}?`,
        what: `Its worktree, its sessions and your note on it are deleted, and so is ${branch ? `the branch ${branch}` : "its branch"}, unless commits on it are on neither the base nor a remote. A branch on a remote stays.`,
        action: "Remove task",
    };
}

function Question({ target, project }: { target: RemoveTarget; project: Project }) {
    const askToRemove = useStore((state) => state.askToRemove);
    const refresh = useStore((state) => state.refresh);
    const report = useStore((state) => state.report);
    const removalBegun = useStore((state) => state.removalBegun);
    const removalEnded = useStore((state) => state.removalEnded);
    const dialog = useRef<HTMLDialogElement>(null);
    // Kept in the store: a dialog closed and asked for again must not start a second removal.
    const key = removalKey(target);
    const busy = useStore((state) => state.removals.includes(key));
    // What stopped the last attempt. After one, the button removes regardless.
    const [problem, setProblem] = useState<string | null>(null);
    const { title, what, action } = wording(target, project);

    useEffect(() => {
        dialog.current?.showModal();
    }, []);

    // Closing the dialog does not stop a removal: what comes of it is then said elsewhere.
    const stillAsked = () => useStore.getState().removing === target;
    // Asked again meanwhile, the dialog on screen is another one about the same thing.
    const askedAgain = () => {
        const asked = useStore.getState().removing;
        return asked !== null && removalKey(asked) === key;
    };
    const remove = async () => {
        removalBegun(key);
        try {
            const passedOver = await removeTarget(target, problem !== null);
            if (askedAgain()) {
                askToRemove(null);
            }
            useStore.getState().forgetTasks(removedTaskIds(target, project));
            await refresh(api);
            if (passedOver.length > 0) {
                report(`Removed, though not everything went as set up: ${passedOver.join("; ")}`);
            }
        } catch (error) {
            const message = String(error).replace(/^Error: /, "");
            if (stillAsked()) {
                setProblem(message);
            } else {
                report(`Not removed: ${message}`);
            }
        } finally {
            removalEnded(key);
        }
    };

    return (
        <dialog
            ref={dialog}
            className="confirm"
            aria-labelledby="remove-title"
            aria-busy={busy}
            onClose={() => askToRemove(null)}
        >
            <h2 id="remove-title">{title}</h2>
            <p>{what}</p>
            <Progress problem={problem} busy={busy} />
            {/* Opening the dialog focuses its first button, so a stray Enter removes nothing. */}
            <div className="confirm-actions">
                <button type="button" className="button" onClick={() => askToRemove(null)}>
                    Close
                </button>
                <button
                    type="button"
                    className="button button-danger"
                    disabled={busy}
                    onClick={() => void remove()}
                >
                    {buttonLabel(busy, problem, action)}
                </button>
            </div>
        </dialog>
    );
}

/** The ids of the tasks a removal of `target` takes away: all of a repository's, or one. */
function removedTaskIds(target: RemoveTarget, project: Project): number[] {
    const tasks = project.tasks.filter((task) => target.task === null || task.name === target.task);
    return tasks.map((task) => task.id);
}

/** Removes what the dialog asks about; resolves to what the removal passed over. */
function removeTarget(target: RemoveTarget, force: boolean): Promise<string[]> {
    return target.task === null
        ? api.projectRemove(target.project, force)
        : api.taskRemove(target.project, target.task, force);
}

/** What stopped the last attempt, and that one that runs can take a while. */
function Progress({ problem, busy }: { problem: string | null; busy: boolean }) {
    return (
        <>
            {problem && (
                <p className="note-problem">
                    Not removed: {problem}. Removing anyway discards that work.
                </p>
            )}
            {busy && (
                <p>
                    Agents and stacks are stopped first, which can take a while. Closing this does
                    not stop the removal.
                </p>
            )}
        </>
    );
}

function buttonLabel(busy: boolean, problem: string | null, action: string): string {
    if (busy) {
        return "Removing…";
    }
    return problem ? "Remove anyway" : action;
}
