import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { FileListing, Project, Task } from "../api/types";
import { type FileRow, fileRows, leftOut, opensInEditor, toggled } from "../files";
import { useStore } from "../store";
import { useWords } from "../useWords";

/**
 * The files of a task's worktree: folders unfold in place, and a file opens in the user's
 * editor. A folder is read when it is first unfolded, and all that are shown again on Refresh.
 */
export function Files({ project, task }: { project: Project; task: Task }) {
    const words = useWords();
    const report = useStore((state) => state.report);
    const [listings, setListings] = useState<Record<string, FileListing | undefined>>({});
    const [unfolded, setUnfolded] = useState<ReadonlySet<string>>(new Set());
    const read = useCallback(
        (dir: string) =>
            api
                .taskFiles(project.name, task.name, dir)
                .then((listing) => setListings((held) => ({ ...held, [dir]: listing })))
                .catch(report),
        [project.name, task.name, report],
    );
    useEffect(() => void read(""), [read]);

    const press = (row: FileRow) => {
        if (!row.dir) {
            return void api.taskOpenFile(project.name, task.name, row.path).catch(report);
        }
        setUnfolded(toggled(unfolded, row.path));
        if (!row.open) {
            void read(row.path);
        }
    };
    const refresh = () => {
        void read("");
        for (const dir of unfolded) {
            void read(dir);
        }
    };
    const rows = fileRows(listings, unfolded);
    return (
        <section className="changes" aria-label={words.files}>
            <div className="changes-head">
                <span className="changes-totals file-root" title={task.worktree_path}>
                    {task.worktree_path}
                </span>
                <button type="button" className="changes-refresh" onClick={refresh}>
                    Refresh
                </button>
            </div>
            <div className="changes-scroll">
                {rows.map((row) => (
                    <FileLine
                        key={row.path}
                        row={row}
                        onPress={() => press(row)}
                        more={leftOut(listings[row.path])}
                    />
                ))}
                {listings[""] && rows.length === 0 && (
                    <p className="changes-note">{words.noFiles}</p>
                )}
                {leftOut(listings[""]) && <p className="changes-note">{leftOut(listings[""])}</p>}
            </div>
        </section>
    );
}

interface LineProps {
    row: FileRow;
    onPress(): void;
    /** What an unfolded folder leaves out, if it holds more than are listed. */
    more: string | null;
}

function FileLine({ row, onPress, more }: LineProps) {
    const indent = { paddingLeft: `calc(var(--space-3) + ${row.depth} * var(--space-4))` };
    return (
        <>
            <button
                type="button"
                className="file-row"
                style={indent}
                aria-expanded={row.dir ? row.open : undefined}
                title={row.dir ? row.path : opensInEditor(row.path)}
                onClick={onPress}
            >
                {row.dir ? (
                    <svg
                        className="file-chevron"
                        viewBox="0 0 10 10"
                        aria-hidden="true"
                        data-open={row.open}
                    >
                        <path d="M3.5 1.5 7 5 3.5 8.5" />
                    </svg>
                ) : (
                    <span className="file-row-gap" />
                )}
                <span className={row.dir ? "file-row-name file-row-dir" : "file-row-name"}>
                    {row.name}
                </span>
            </button>
            {row.open && more && (
                <p className="changes-note" style={indent}>
                    {more}
                </p>
            )}
        </>
    );
}
