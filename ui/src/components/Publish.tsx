import { useEffect, useState } from "react";
import { api } from "../api";
import type { PullDraft, Task, Unpublished } from "../api/types";
import { nextStep, unpublishedSummary } from "../publish";
import { useStore } from "../store";

interface Props {
    project: string;
    task: Task;
}

/** What has not left the task's worktree, read again whenever its files may have changed. */
function useUnpublished(project: string, task: Task, version: number): Unpublished | null {
    const [state, setState] = useState<Unpublished | null>(null);
    // biome-ignore lint/correctness/useExhaustiveDependencies: `version` is what asks again
    useEffect(() => {
        let current = true;
        api.taskUnpublished(project, task.name).then(
            (read) => current && setState(read),
            // A worktree that cannot be read has nothing to publish from here.
            () => current && setState(null),
        );
        return () => {
            current = false;
        };
    }, [project, task.name, version]);
    return state;
}

/**
 * The way out for a task's work, one step at a time: commit what changed, push the commits,
 * open a pull request. Each is done only when its button is pressed.
 */
export function Publish({ project, task }: Props) {
    const edits = useStore((state) => state.edits[task.id] ?? 0);
    const hasPull = useStore((state) => Boolean(state.pulls[task.id]));
    const loadPull = useStore((state) => state.loadPull);
    const report = useStore((state) => state.report);
    const [done, setDone] = useState(0);
    const [busy, setBusy] = useState(false);
    const state = useUnpublished(project, task, edits + done);
    if (!state) {
        return null;
    }
    const step = nextStep(state, hasPull);
    /** Runs one step, then reads again what is left. */
    const run = (action: Promise<unknown>) => {
        setBusy(true);
        action
            .then(() => loadPull(api, project, task.name, task.id))
            .catch(report)
            .finally(() => {
                setBusy(false);
                setDone((count) => count + 1);
            });
    };
    return (
        <section className="publish" aria-label="Publish">
            <p className="publish-summary">{unpublishedSummary(state)}</p>
            {step === "commit" && (
                <CommitForm
                    busy={busy}
                    onCommit={(message) => run(api.taskCommit(project, task.name, message))}
                />
            )}
            {step === "push" && (
                <button
                    type="button"
                    className="button"
                    disabled={busy}
                    title={`Pushes ${task.branch} to origin`}
                    onClick={() => run(api.taskPush(project, task.name))}
                >
                    {busy ? "Pushing…" : "Push"}
                </button>
            )}
            {step === "pull" && (
                <PullForm
                    project={project}
                    task={task}
                    busy={busy}
                    onOpen={(draft, asDraft) =>
                        run(api.taskPullCreate(project, task.name, draft, asDraft))
                    }
                />
            )}
        </section>
    );
}

function CommitForm({ busy, onCommit }: { busy: boolean; onCommit(message: string): void }) {
    const [message, setMessage] = useState("");
    return (
        <form
            className="publish-form"
            onSubmit={(event) => {
                event.preventDefault();
                onCommit(message);
                setMessage("");
            }}
        >
            <input
                className="publish-input"
                aria-label="Commit message"
                placeholder="Commit message"
                value={message}
                onChange={(event) => setMessage(event.target.value)}
            />
            <button type="submit" className="button" disabled={busy || message.trim() === ""}>
                {busy ? "Committing…" : "Commit all"}
            </button>
        </form>
    );
}

interface PullFormProps extends Props {
    busy: boolean;
    onOpen(draft: PullDraft, asDraft: boolean): void;
}

/** The pull request's title and description, started from the task's commits, to edit first. */
function PullForm({ project, task, busy, onOpen }: PullFormProps) {
    const report = useStore((state) => state.report);
    const [draft, setDraft] = useState<PullDraft | null>(null);
    const [asDraft, setAsDraft] = useState(false);
    if (draft === null) {
        return (
            <button
                type="button"
                className="button"
                onClick={() => api.taskPullDraft(project, task.name).then(setDraft, report)}
            >
                Open a pull request…
            </button>
        );
    }
    return (
        <form
            className="publish-form publish-pull"
            onSubmit={(event) => {
                event.preventDefault();
                onOpen(draft, asDraft);
            }}
        >
            <input
                className="publish-input"
                aria-label="Pull request title"
                placeholder="Title"
                value={draft.title}
                onChange={(event) => setDraft({ ...draft, title: event.target.value })}
            />
            <textarea
                className="publish-input"
                aria-label="Pull request description"
                placeholder="Description"
                rows={6}
                value={draft.body}
                onChange={(event) => setDraft({ ...draft, body: event.target.value })}
            />
            <label className="publish-draft">
                <input
                    type="checkbox"
                    checked={asDraft}
                    onChange={(event) => setAsDraft(event.target.checked)}
                />
                Open as a draft
            </label>
            <div className="publish-actions">
                <button type="button" className="button" onClick={() => setDraft(null)}>
                    Cancel
                </button>
                <button
                    type="submit"
                    className="button button-primary"
                    disabled={busy || draft.title.trim() === ""}
                >
                    {busy ? "Opening…" : `Open into ${task.base_ref.replace(/^origin\//, "")}`}
                </button>
            </div>
        </form>
    );
}
