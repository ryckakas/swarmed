import { memo, useEffect, useMemo, useState } from "react";
import { sendOrQueue } from "../actions";
import { api } from "../api";
import type { FileChange, FileDiff, Project, Task } from "../api/types";
import { type DiffLine, parseDiff, splitPath } from "../diff";
import { commentKey, type ReviewComment, reviewMessage } from "../review";
import { useStore } from "../store";
import { useWords } from "../useWords";

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
    const words = useWords();

    return (
        <section className="changes" aria-label={words.changes}>
            <div className="changes-head">
                {Array.isArray(changes) && <Totals changes={changes} />}
                {files.length > 0 && (
                    <>
                        <button
                            type="button"
                            className="changes-refresh"
                            onClick={() => setOpen(new Set(files.map((file) => file.path)))}
                        >
                            {words.expandAll}
                        </button>
                        <button
                            type="button"
                            className="changes-refresh"
                            onClick={() => setOpen(new Set())}
                        >
                            {words.collapseAll}
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
            <ReviewBar taskId={task.id} />
            <div className="changes-scroll">
                {changes === null && <p className="changes-note">{words.readingChanges}</p>}
                {typeof changes === "string" && (
                    <p className="changes-note changes-error">{changes}</p>
                )}
                {Array.isArray(changes) && changes.length === 0 && (
                    <p className="changes-note">{words.noChanges(task.base_ref)}</p>
                )}
                {Array.isArray(changes) &&
                    changes.map((change) => (
                        <FileRow
                            key={change.path}
                            change={change}
                            open={open.has(change.path)}
                            onToggle={() => toggle(change.path)}
                            target={{
                                project: project.name,
                                task: task.name,
                                taskId: task.id,
                                version,
                            }}
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
    taskId: number;
    version: number;
}

/** The comments made so far, with the way to send them to the session on screen. */
function ReviewBar({ taskId }: { taskId: number }) {
    const words = useWords();
    const comments = useStore((state) => state.reviews[taskId]);
    const clearReview = useStore((state) => state.clearReview);
    const sessionId = useStore((state) => state.selectedSessionId);
    const report = useStore((state) => state.report);
    if (!comments || comments.length === 0) {
        return null;
    }
    const send = () => {
        if (sessionId === null) {
            return;
        }
        sendOrQueue(sessionId, reviewMessage(comments))
            .then(() => clearReview(taskId))
            .catch(report);
    };
    return (
        <div className="review-bar">
            <span>
                {comments.length} {comments.length === 1 ? "comment" : "comments"}
            </span>
            <button type="button" className="changes-refresh" onClick={() => clearReview(taskId)}>
                Discard
            </button>
            <button type="button" className="button" title={words.sendToSessionHint} onClick={send}>
                {words.sendToSession}
            </button>
        </div>
    );
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

/** A reference that stays the same, so a file without comments is not drawn again. */
const NO_COMMENTS: ReviewComment[] = [];

function Diff({ path, target }: { path: string; target: Target }) {
    const { project, task, taskId, version } = target;
    const all = useStore((state) => state.reviews[taskId]);
    const comments = useMemo(
        () => all?.filter((comment) => comment.path === path) ?? NO_COMMENTS,
        [all, path],
    );
    const comment = useStore((state) => state.comment);
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
            <DiffLines
                text={diff.text}
                path={path}
                comments={comments.length > 0 ? comments : NO_COMMENTS}
                onComment={(made) => comment(taskId, made)}
            />
            {diff.truncated && (
                <p className="changes-note">The diff is longer; the rest is not shown.</p>
            )}
        </>
    );
}

const SIGN = { added: "+", removed: "−", context: " " } as const;

interface DiffLinesProps {
    text: string;
    path: string;
    /** The comments on this file's lines. */
    comments: ReviewComment[];
    onComment(comment: ReviewComment): void;
}

/** Where a line of a diff is, for a comment on it; nothing for lines that are not code. */
function placeOf(line: DiffLine): Pick<ReviewComment, "line" | "side"> | null {
    if (line.kind === "removed") {
        return { line: line.oldLine, side: "old" };
    }
    return line.kind === "added" || line.kind === "context"
        ? { line: line.newLine, side: "new" }
        : null;
}

const DiffLines = memo(function DiffLines({ text, path, comments, onComment }: DiffLinesProps) {
    const lines = useMemo(() => parseDiff(text), [text]);
    // The line whose comment is being written, by its key.
    const [writing, setWriting] = useState<string | null>(null);
    const made = useMemo(
        () => new Map(comments.map((comment) => [commentKey(comment), comment])),
        [comments],
    );
    return (
        <div className="diff">
            <div className="diff-lines">
                {lines.map((line, index) => {
                    const place = placeOf(line);
                    const key = place && commentKey({ path, ...place });
                    return (
                        <DiffRow
                            // biome-ignore lint/suspicious/noArrayIndexKey: the lines of one diff never reorder
                            key={index}
                            line={line}
                            comment={key ? made.get(key) : undefined}
                            writing={key !== null && key === writing}
                            onWrite={() => setWriting(key === writing ? null : key)}
                            onSave={(said) => {
                                setWriting(null);
                                if (place) {
                                    onComment({ path, ...place, code: line.text, text: said });
                                }
                            }}
                        />
                    );
                })}
            </div>
        </div>
    );
});

interface DiffRowProps {
    line: DiffLine;
    comment: ReviewComment | undefined;
    writing: boolean;
    onWrite(): void;
    onSave(text: string): void;
}

function DiffRow({ line, comment, writing, onWrite, onSave }: DiffRowProps) {
    if (line.kind === "hunk" || line.kind === "note") {
        return <div className={`diff-line diff-${line.kind}`}>{line.text || "⋯"}</div>;
    }
    const number = line.kind === "removed" ? line.oldLine : line.newLine;
    return (
        <>
            <div className={`diff-line diff-${line.kind}`} data-commented={comment !== undefined}>
                <button
                    type="button"
                    className="diff-number"
                    title="Comment on this line"
                    aria-label={`Comment on line ${number}`}
                    onClick={onWrite}
                >
                    {number}
                </button>
                <span className="diff-sign">{SIGN[line.kind]}</span>
                <span className="diff-text">{line.text}</span>
            </div>
            {writing && <CommentBox said={comment?.text ?? ""} onSave={onSave} />}
            {!writing && comment && (
                <button type="button" className="diff-comment" onClick={onWrite}>
                    {comment.text}
                </button>
            )}
        </>
    );
}

/** Where a comment is written. Saving it empty takes the comment away. */
function CommentBox({ said, onSave }: { said: string; onSave(text: string): void }) {
    const [text, setText] = useState(said);
    return (
        <form
            className="diff-comment-box"
            onSubmit={(event) => {
                event.preventDefault();
                onSave(text);
            }}
        >
            <textarea
                // biome-ignore lint/a11y/noAutofocus: opened by a click on the line it belongs to
                autoFocus
                aria-label="Comment"
                placeholder="What should change here? ⌘↩ saves."
                rows={2}
                value={text}
                onChange={(event) => setText(event.target.value)}
                onKeyDown={(event) => {
                    if (event.key === "Enter" && event.metaKey) {
                        event.preventDefault();
                        onSave(text);
                    }
                }}
            />
            <button type="submit" className="button">
                {said !== "" && text.trim() === "" ? "Remove" : "Save"}
            </button>
        </form>
    );
}
