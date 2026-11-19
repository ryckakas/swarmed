import { memo, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { FileChange, FileDiff, Project, Task } from "../api/types";
import { type DiffLine, parseDiff, splitPath } from "../diff";
import { useStore } from "../store";

interface Props {
    project: Project;
    task: Task;
}

/** The files a task has changed, each opening into its diff. Read-only. */
export function Changes({ project, task }: Props) {
    // Goes up when one of the task's agents finishes a tool call, which is when files change.
    const edits = useStore((state) => state.edits[task.id] ?? 0);
    const [manual, setManual] = useState(0);
    const version = edits + manual;
    const changes = useChanges(project.name, task.name, version);
    const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
    const files = Array.isArray(changes) ? changes : [];
    const toggle = (path: string) => setOpen((current) => toggled(current, path));

    return (
        <section className="changes" aria-label="Changes">
            <div className="changes-head">
                <h3>Changes</h3>
                {Array.isArray(changes) && <Totals changes={changes} />}
                {files.length > 0 && (
                    <>
                        <button
                            type="button"
                            className="changes-refresh"
                            onClick={() => setOpen(new Set(files.map((file) => file.path)))}
                        >
                            Expand all
                        </button>
                        <button
                            type="button"
                            className="changes-refresh"
                            onClick={() => setOpen(new Set())}
                        >
                            Collapse all
                        </button>
                    </>
                )}
                <button
                    type="button"
                    className="changes-refresh"
                    onClick={() => setManual((count) => count + 1)}
                >
                    Refresh
                </button>
            </div>
            <div className="changes-scroll">
                {changes === null && <p className="changes-note">Reading the changes…</p>}
                {typeof changes === "string" && (
                    <p className="changes-note changes-error">{changes}</p>
                )}
                {Array.isArray(changes) && changes.length === 0 && (
                    <p className="changes-note">
                        Nothing has changed since this branch left {task.base_ref}.
                    </p>
                )}
                {Array.isArray(changes) &&
                    changes.map((change) => (
                        <FileRow
                            key={change.path}
                            change={change}
                            open={open.has(change.path)}
                            onToggle={() => toggle(change.path)}
                            target={{ project: project.name, task: task.name, version }}
                        />
                    ))}
            </div>
        </section>
    );
}

/** `open` with `path` added if it was missing, removed if it was there. */
function toggled(open: ReadonlySet<string>, path: string): ReadonlySet<string> {
    const next = new Set(open);
    if (!next.delete(path)) {
        next.add(path);
    }
    return next;
}

/** The task's changed files: `null` while first reading, a message if that failed. */
function useChanges(project: string, task: string, version: number) {
    const [changes, setChanges] = useState<FileChange[] | string | null>(null);
    // biome-ignore lint/correctness/useExhaustiveDependencies: a new version means read again
    useEffect(() => {
        let current = true;
        api.taskChanges(project, task)
            .then((read) => current && setChanges(read))
            .catch((error: unknown) => current && setChanges(String(error)));
        return () => {
            current = false;
        };
    }, [project, task, version]);
    return changes;
}

function Totals({ changes }: { changes: FileChange[] }) {
    const sum = (pick: (change: FileChange) => number | null) =>
        changes.reduce((total, change) => total + (pick(change) ?? 0), 0);
    return (
        <p className="changes-totals">
            {changes.length} {changes.length === 1 ? "file" : "files"}
            <Counts added={sum((c) => c.added)} removed={sum((c) => c.removed)} />
        </p>
    );
}

function Counts({ added, removed }: { added: number | null; removed: number | null }) {
    if (added === null || removed === null) {
        return <span className="counts">binary</span>;
    }
    return (
        <span className="counts">
            <span className="count-added">+{added}</span>
            <span className="count-removed">−{removed}</span>
        </span>
    );
}

interface Target {
    project: string;
    task: string;
    version: number;
}

interface FileRowProps {
    change: FileChange;
    open: boolean;
    onToggle(): void;
    target: Target;
}

const KIND_MARK = { added: "A", modified: "M", deleted: "D" } as const;

function FileRow({ change, open, onToggle, target }: FileRowProps) {
    const { folder, name } = splitPath(change.path);
    return (
        <div className="change">
            <button type="button" className="change-row" aria-expanded={open} onClick={onToggle}>
                <svg className="change-chevron" viewBox="0 0 10 10" aria-hidden="true">
                    <path d="M3.5 1.5 7 5 3.5 8.5" />
                </svg>
                <span className={`change-kind change-${change.kind}`} title={change.kind}>
                    {KIND_MARK[change.kind]}
                </span>
                <span className="change-path" title={change.path}>
                    {/* Kept left to right inside the right-to-left box that cuts the path's start. */}
                    <bdi>
                        <span className="change-folder">{folder}</span>
                        {name}
                    </bdi>
                </span>
                <Counts added={change.added} removed={change.removed} />
            </button>
            {open && <Diff path={change.path} target={target} />}
        </div>
    );
}

function Diff({ path, target }: { path: string; target: Target }) {
    const { project, task, version } = target;
    const [diff, setDiff] = useState<FileDiff | string | null>(null);
    // biome-ignore lint/correctness/useExhaustiveDependencies: a new version means read again
    useEffect(() => {
        let current = true;
        api.taskDiff(project, task, path)
            .then((read) => current && setDiff(read))
            .catch((error: unknown) => current && setDiff(String(error)));
        return () => {
            current = false;
        };
    }, [project, task, path, version]);

    if (diff === null) {
        return <p className="changes-note">Reading the diff…</p>;
    }
    if (typeof diff === "string") {
        return <p className="changes-note changes-error">{diff}</p>;
    }
    return (
        <>
            <DiffLines text={diff.text} />
            {diff.truncated && (
                <p className="changes-note">The diff is longer; the rest is not shown.</p>
            )}
        </>
    );
}

const SIGN = { added: "+", removed: "−", context: " " } as const;

const DiffLines = memo(function DiffLines({ text }: { text: string }) {
    const lines = useMemo(() => parseDiff(text), [text]);
    return (
        <div className="diff">
            <div className="diff-lines">
                {lines.map((line, index) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: the lines of one diff never reorder
                    <DiffRow key={index} line={line} />
                ))}
            </div>
        </div>
    );
});

function DiffRow({ line }: { line: DiffLine }) {
    if (line.kind === "hunk" || line.kind === "note") {
        return <div className={`diff-line diff-${line.kind}`}>{line.text || "⋯"}</div>;
    }
    const number = line.kind === "removed" ? line.oldLine : line.newLine;
    return (
        <div className={`diff-line diff-${line.kind}`}>
            <span className="diff-number">{number}</span>
            <span className="diff-sign">{SIGN[line.kind]}</span>
            <span className="diff-text">{line.text}</span>
        </div>
    );
}
