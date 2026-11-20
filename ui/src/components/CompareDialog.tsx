import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { FileChange, FileDiff } from "../api/types";
import { type ComparedFile, compared, totals } from "../broadcast";
import { parseDiff } from "../diff";
import { useStore } from "../store";

/** Two tasks of one repository, to set side by side. */
export interface Comparison {
    project: string;
    left: string;
    right: string;
}

/** A task's changed files: `null` while they are read, a message if that failed. */
function useTaskChanges(project: string, task: string): FileChange[] | string | null {
    const [changes, setChanges] = useState<FileChange[] | string | null>(null);
    useEffect(() => {
        let current = true;
        setChanges(null);
        api.taskChanges(project, task).then(
            (read) => current && setChanges(read),
            (error: unknown) => current && setChanges(String(error)),
        );
        return () => {
            current = false;
        };
    }, [project, task]);
    return changes;
}

/**
 * What two tasks changed, side by side: every file either of them touched, with each one's
 * diff. For telling apart what several agents made of the same prompt, and keeping one.
 */
export function CompareDialog() {
    const comparing = useStore((state) => state.comparing);
    const compare = useStore((state) => state.compare);
    const dialog = useRef<HTMLDialogElement>(null);

    // A modal <dialog> keeps the keyboard inside it and closes on Esc by itself.
    useEffect(() => {
        if (comparing && !dialog.current?.open) {
            dialog.current?.showModal();
        }
    }, [comparing]);

    if (!comparing) {
        return null;
    }
    return (
        <dialog
            ref={dialog}
            className="confirm compare"
            aria-label={`Compare ${comparing.left} with ${comparing.right}`}
            onClose={() => compare(null)}
        >
            <Compared comparing={comparing} />
            <div className="confirm-actions">
                <button type="button" className="button" onClick={() => compare(null)}>
                    Done
                </button>
            </div>
        </dialog>
    );
}

function Compared({ comparing }: { comparing: Comparison }) {
    const { project, left, right } = comparing;
    const leftChanges = useTaskChanges(project, left);
    const rightChanges = useTaskChanges(project, right);
    const [open, setOpen] = useState<string | null>(null);
    const rows = useMemo(
        () =>
            Array.isArray(leftChanges) && Array.isArray(rightChanges)
                ? compared(leftChanges, rightChanges)
                : [],
        [leftChanges, rightChanges],
    );
    return (
        <div className="compare-grid">
            <CompareHead project={project} task={left} changes={leftChanges} />
            <CompareHead project={project} task={right} changes={rightChanges} />
            {rows.map((row) => (
                <CompareRow
                    key={row.path}
                    row={row}
                    comparing={comparing}
                    open={open === row.path}
                    onToggle={() => setOpen(open === row.path ? null : row.path)}
                />
            ))}
        </div>
    );
}

interface HeadProps {
    project: string;
    task: string;
    changes: FileChange[] | string | null;
}

/** One side's task: its name, what it changed in all, and the way to drop it. */
function CompareHead({ project, task, changes }: HeadProps) {
    const askToRemove = useStore((state) => state.askToRemove);
    const compare = useStore((state) => state.compare);
    const sum = Array.isArray(changes) ? totals(changes) : null;
    return (
        <div className="compare-head">
            <h3 className="mono">{task}</h3>
            <p className="settings-hint">
                {changes === null && "Reading the changes…"}
                {typeof changes === "string" && changes}
                {sum &&
                    `${sum.files} ${sum.files === 1 ? "file" : "files"}, +${sum.added} −${sum.removed}`}
            </p>
            <button
                type="button"
                className="button environment-remove"
                title="Asks first. The other task stays."
                onClick={() => {
                    compare(null);
                    askToRemove({ project, task });
                }}
            >
                Remove this one…
            </button>
        </div>
    );
}

interface RowProps {
    row: ComparedFile;
    comparing: Comparison;
    open: boolean;
    onToggle(): void;
}

function CompareRow({ row, comparing, open, onToggle }: RowProps) {
    return (
        <>
            <button type="button" className="compare-file" aria-expanded={open} onClick={onToggle}>
                <span className="compare-cell">
                    <span className="compare-path mono">{row.path}</span>
                    <Side change={row.left} />
                </span>
                <Side change={row.right} />
            </button>
            {open && (
                <>
                    <SideDiff
                        project={comparing.project}
                        task={comparing.left}
                        file={row}
                        side="left"
                    />
                    <SideDiff
                        project={comparing.project}
                        task={comparing.right}
                        file={row}
                        side="right"
                    />
                </>
            )}
        </>
    );
}

/** What one task did to a file, in a few characters; a dash if it left the file alone. */
function Side({ change }: { change: FileChange | null }) {
    if (!change) {
        return <span className="compare-side compare-untouched">not changed</span>;
    }
    const binary = change.added === null || change.removed === null;
    return (
        <span className="compare-side">
            {change.kind}
            {binary ? ", binary" : `, +${change.added} −${change.removed}`}
        </span>
    );
}

interface SideDiffProps {
    project: string;
    task: string;
    file: ComparedFile;
    side: "left" | "right";
}

/** One task's diff of a file, read when the row is opened. */
function SideDiff({ project, task, file, side }: SideDiffProps) {
    const [diff, setDiff] = useState<FileDiff | string | null>(null);
    const touched = file[side] !== null;
    useEffect(() => {
        if (!touched) {
            return;
        }
        let current = true;
        api.taskDiff(project, task, file.path).then(
            (read) => current && setDiff(read),
            (error: unknown) => current && setDiff(String(error)),
        );
        return () => {
            current = false;
        };
    }, [project, task, file.path, touched]);
    if (!touched) {
        return <p className="compare-diff changes-note">Not changed in this task.</p>;
    }
    if (diff === null || typeof diff === "string") {
        return <p className="compare-diff changes-note">{diff ?? "Reading the diff…"}</p>;
    }
    return (
        <div className="compare-diff diff">
            <div className="diff-lines">
                {parseDiff(diff.text).map((line, index) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: the lines of one diff never reorder
                    <div key={index} className={`diff-line diff-${line.kind}`}>
                        {line.kind === "hunk" || line.kind === "note"
                            ? line.text || "⋯"
                            : line.text}
                    </div>
                ))}
            </div>
        </div>
    );
}
