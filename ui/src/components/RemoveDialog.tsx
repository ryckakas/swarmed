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
    const dialog = useRef<HTMLDialogElement>(null);
    const [busy, setBusy] = useState(false);
    // What stopped the last attempt. After one, the button removes regardless.
    const [problem, setProblem] = useState<string | null>(null);
    const { title, what, action } = wording(target, project);

    useEffect(() => {
        dialog.current?.showModal();
    }, []);

    const remove = async () => {
        setBusy(true);
        const force = problem !== null;
        try {
            await (target.task === null
                ? api.projectRemove(target.project, force)
                : api.taskRemove(target.project, target.task, force));
            askToRemove(null);
            await refresh(api);
        } catch (error) {
            setProblem(String(error).replace(/^Error: /, ""));
            setBusy(false);
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
            {problem && (
                <p className="note-problem">
                    Not removed: {problem}. Removing anyway discards that work.
                </p>
            )}
            {/* Opening the dialog focuses its first button, so a stray Enter removes nothing. */}
            <div className="confirm-actions">
                <button type="button" className="button" onClick={() => askToRemove(null)}>
                    Keep it
                </button>
                <button
                    type="button"
                    className="button button-danger"
                    disabled={busy}
                    onClick={() => void remove()}
                >
                    {busy ? "Removing…" : problem ? "Remove anyway" : action}
                </button>
            </div>
        </dialog>
    );
}
