import { useEffect, useState } from "react";
import { api } from "../api";
import type { PullDraft, Task, Unpublished } from "../api/types";
import { nextStep, unpublishedSummary } from "../publish";
import { useStore } from "../store";
import { useWords } from "../useWords";

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
    // The session on screen is one of this task's: its agent is the one to write for it.
    const sessionId = useStore((state) => state.selectedSessionId);
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
                    write={() => api.taskCommitMessage(project, task.name, sessionId)}
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
                    write={() => api.taskPullMessage(project, task.name, sessionId)}
                    onOpen={(draft, asDraft) =>
                        run(api.taskPullCreate(project, task.name, draft, asDraft))
                    }
                />
            )}
        </section>
    );
}

interface CommitFormProps {
    busy: boolean;
    /** Has a message written for what changed. */
    write(): Promise<string>;
    onCommit(message: string): void;
}

/** How many lines the commit box shows: as many as the message has, within reason. */
function messageRows(message: string): number {
    return Math.min(8, Math.max(1, message.split("\n").length));
}

function CommitForm({ busy, write, onCommit }: CommitFormProps) {
    const report = useStore((state) => state.report);
    const words = useWords();
    const [message, setMessage] = useState("");
    const [writing, setWriting] = useState(false);
    const commit = () => {
        if (busy || message.trim() === "") {
            return;
        }
        onCommit(message);
        setMessage("");
    };
    const generate = () => {
        setWriting(true);
        write()
            .then(setMessage, report)
            .finally(() => setWriting(false));
    };
    return (
        <form
            className="publish-form publish-commit"
            onSubmit={(event) => {
                event.preventDefault();
                commit();
            }}
        >
            <div className="publish-message">
                <textarea
                    className="publish-input publish-message-input"
                    aria-label="Commit message"
                    placeholder={writing ? "Reading the changes…" : "Commit message"}
                    rows={messageRows(message)}
                    disabled={writing}
                    value={message}
                    onChange={(event) => setMessage(event.target.value)}
                    onKeyDown={(event) => {
                        // Enter commits, as it did in a one-line box; Shift Enter is a new line.
                        if (event.key === "Enter" && !event.shiftKey) {
                            event.preventDefault();
                            commit();
                        }
                    }}
                />
                <WriteButton
                    label={words.writeMessage}
                    writing={writing}
                    disabled={busy}
                    onClick={generate}
                />
            </div>
            <button type="submit" className="button" disabled={busy || message.trim() === ""}>
                {busy ? "Committing…" : "Commit all"}
            </button>
        </form>
    );
}

interface WriteButtonProps {
    /** What it has written, for its tooltip. */
    label: string;
    writing: boolean;
    disabled: boolean;
    onClick(): void;
}

/** The small button inside a text box that has its text written from the task's work. */
function WriteButton({ label, writing, disabled, onClick }: WriteButtonProps) {
    return (
        <button
            type="button"
            className="publish-write"
            aria-label={label}
            title={label}
            aria-busy={writing}
            disabled={writing || disabled}
            onClick={onClick}
        >
            <svg
                className="publish-write-mark"
                viewBox="0 0 16 16"
                aria-hidden="true"
                data-writing={writing}
            >
                <path d="M7 1.5 8.4 5.6 12.5 7 8.4 8.4 7 12.5 5.6 8.4 1.5 7 5.6 5.6Z" />
                <path d="M12.5 10.5l.6 1.4 1.4.6-1.4.6-.6 1.4-.6-1.4-1.4-.6 1.4-.6Z" />
            </svg>
        </button>
    );
}

interface PullFormProps extends Props {
    busy: boolean;
    /** Has a title and a description written for the branch. */
    write(): Promise<PullDraft>;
    onOpen(draft: PullDraft, asDraft: boolean): void;
}

/** The pull request's title and description, started from the task's commits, to edit first. */
function PullForm({ project, task, busy, write, onOpen }: PullFormProps) {
    const report = useStore((state) => state.report);
    const words = useWords();
    const [draft, setDraft] = useState<PullDraft | null>(null);
    const [asDraft, setAsDraft] = useState(false);
    const [writing, setWriting] = useState(false);
    const generate = () => {
        setWriting(true);
        write()
            .then(setDraft, report)
            .finally(() => setWriting(false));
    };
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
            <div className="publish-message">
                <input
                    className="publish-input publish-title-input"
                    aria-label="Pull request title"
                    placeholder={writing ? "Reading the branch…" : "Title"}
                    disabled={writing}
                    value={draft.title}
                    onChange={(event) => setDraft({ ...draft, title: event.target.value })}
                />
                {/* One press writes both the title and the description under it. */}
                <WriteButton
                    label={words.writePull}
                    writing={writing}
                    disabled={busy}
                    onClick={generate}
                />
            </div>
            <textarea
                className="publish-input"
                aria-label="Pull request description"
                placeholder={writing ? "Reading the branch…" : "Description"}
                rows={6}
                disabled={writing}
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
                    disabled={busy || writing || draft.title.trim() === ""}
                >
                    {busy ? "Opening…" : `Open into ${task.base_ref.replace(/^origin\//, "")}`}
                </button>
            </div>
        </form>
    );
}
