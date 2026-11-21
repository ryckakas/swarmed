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
    const key = `${removing.project}/${removing.task ?? ""}`;
    return <Question key={key} target={removing} project={project} />;
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
        what: `Its worktree, its sessions and your note on it are deleted. ${branch ? `The branch ${branch} stays` : "Its branch stays"} in the repository.`,
        action: "Remove task",
    };
}

function Question({ target, project }: { target: RemoveTarget; project: Project }) {
    const askToRemove = useStore((state) => state.askToRemove);
    const refresh = useStore((state) => state.refresh);
    const report = useStore((state) => state.report);
    const dialog = useRef<HTMLDialogElement>(null);
    const [busy, setBusy] = useState(false);
    // What stopped the last attempt. After one, the button removes regardless.
    const [problem, setProblem] = useState<string | null>(null);
    const { title, what, action } = wording(target, project);

    useEffect(() => {
        dialog.current?.showModal();
    }, []);

    // Closing the dialog does not stop a removal: what comes of it is then said elsewhere.
    const stillAsked = () => useStore.getState().removing === target;
    const remove = async () => {
        setBusy(true);
        try {
            const passedOver = await removeTarget(target, problem !== null);
            if (stillAsked()) {
                askToRemove(null);
            }
            await refresh(api);
            if (passedOver.length > 0) {
                report(`Removed, though not everything went as set up: ${passedOver.join("; ")}`);
            }
        } catch (error) {
            const message = String(error).replace(/^Error: /, "");
            if (stillAsked()) {
                setProblem(message);
                setBusy(false);
            } else {
                report(`Not removed: ${message}`);
            }
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
