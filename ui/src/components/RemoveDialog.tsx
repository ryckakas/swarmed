import { useState } from "react";
import { api } from "../api";
import type { Project, RemoveTarget } from "../api/types";
import { anywayHelps, refusalSaid } from "../removal";
import { useStore } from "../store";
import { useWords } from "../useWords";
import type { Words } from "../wording";
import { Dialog } from "./Dialog";

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

/** What the dialog says about a repository, or about one of its tasks; its button in the style's words. */
function wording(target: RemoveTarget, project: Project, words: Words) {
    const tasks = project.tasks.filter((task) => !task.archived);
    if (target.task === null) {
        const worktrees = tasks.length === 1 ? "1 task" : `${tasks.length} tasks`;
        return {
            title: `Remove ${project.name} from Swarmed?`,
            what: `Its ${worktrees}, their worktrees and sessions, and your notes on it are deleted. The repository itself and its branches stay as they are.`,
            action: `Remove ${words.terms.repository}`,
        };
    }
    const branch = tasks.find((task) => task.name === target.task)?.branch;
    return {
        title: `Remove the task ${target.task}?`,
        what: `Its worktree, its sessions and your note on it are deleted, and so is ${branch ? `the branch ${branch}` : "its branch"}, unless commits on it are on neither the base nor a remote. A branch on a remote stays.`,
        action: `Remove ${words.terms.task}`,
    };
}

function Question({ target, project }: { target: RemoveTarget; project: Project }) {
    const words = useWords();
    const askToRemove = useStore((state) => state.askToRemove);
    const refresh = useStore((state) => state.refresh);
    const report = useStore((state) => state.report);
    const removalBegun = useStore((state) => state.removalBegun);
    const removalEnded = useStore((state) => state.removalEnded);
    const leaveForRemoval = useStore((state) => state.leaveForRemoval);
    // Kept in the store: a dialog closed and asked for again must not start a second removal.
    const key = removalKey(target);
    const busy = useStore((state) => state.removals.includes(key));
    // What stopped the last attempt. After one over unsaved work, the button removes regardless.
    const [problem, setProblem] = useState<string | null>(null);
    const anyway = problem !== null && anywayHelps(problem);
    const { title, what, action } = wording(target, project, words);

    // Closing the dialog does not stop a removal: what comes of it is then said elsewhere.
    const stillAsked = () => useStore.getState().removing === target;
    // Asked again meanwhile, the dialog on screen is another one about the same thing.
    const askedAgain = () => {
        const asked = useStore.getState().removing;
        return asked !== null && removalKey(asked) === key;
    };
    const remove = async () => {
        removalBegun(key);
        // Off its sessions before its agents stop and its worktree goes, not once it is gone.
        leaveForRemoval(target);
        try {
            const passedOver = await removeTarget(target, anyway);
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

    // No main button, so the keyboard starts on Close and a stray Enter removes nothing.
    return (
        <Dialog
            title={title}
            size="small"
            closeOnBackdrop={true}
            onClose={() => askToRemove(null)}
            problem={problem && refusalSaid(problem)}
            busy={busy}
            actions={
                <button
                    type="button"
                    className="button button-danger"
                    onClick={() => void remove()}
                >
                    {buttonLabel(busy, anyway, action)}
                </button>
            }
        >
            <p>{what}</p>
            {busy && (
                <p>
                    Agents and stacks are stopped first, which can take a while. Closing this does
                    not stop the removal.
                </p>
            )}
        </Dialog>
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

function buttonLabel(busy: boolean, anyway: boolean, action: string): string {
    if (busy) {
        return "Removing…";
    }
    return anyway ? "Remove anyway" : action;
}
