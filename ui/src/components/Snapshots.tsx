import { useEffect, useState } from "react";
import { api } from "../api";
import type { Snapshot } from "../api/types";
import { type SnapshotRow, snapshotRows, takenAt } from "../snapshots";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { withArticle } from "../wording";
import { Picker } from "./Picker";

interface Props {
    project: string;
    /** The project's environments, by name. */
    environments: string[];
}

/**
 * What new tasks start from: for the tasks without an environment and for each environment,
 * the snapshot there is, a way to take one from a task that was set up well, and a way to
 * forget it.
 */
export function Snapshots({ project, environments }: Props) {
    const words = useWords();
    const tasks = useStore((state) => state.projects.find((one) => one.name === project)?.tasks);
    const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
    const [problem, setProblem] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);
    const failed = (error: unknown) => setProblem(String(error));
    const read = () => api.snapshots(project).then(setSnapshots, failed);
    // biome-ignore lint/correctness/useExhaustiveDependencies: read once, when the dialog opens
    useEffect(() => {
        void read();
    }, []);
    const run = (change: Promise<unknown>) => {
        setBusy(true);
        setProblem(null);
        change.then(read, failed).finally(() => setBusy(false));
    };
    return (
        <section className="snapshots" aria-label={words.snapshots}>
            <h3>{words.snapshots}</h3>
            <p className="settings-hint">
                {`Setting a task up again and again repeats the same installing and building. Take ${withArticle(words.snapshot)} from a task that was set up well, and new tasks start with what git ignores in it already in place: dependencies, build output, local configuration. Their setup still runs, with less to do. On this Mac's disk the copies share their space until they change.`}
            </p>
            <ul className="snapshot-list">
                {snapshotRows(environments, snapshots, tasks ?? []).map((row) => (
                    <Row
                        key={row.environment ?? ""}
                        row={row}
                        busy={busy}
                        onTake={(task) => run(api.snapshotSave(project, task))}
                        onForget={() => run(api.snapshotRemove(project, row.environment))}
                    />
                ))}
            </ul>
            {busy && <p className="settings-hint">Copying…</p>}
            {problem && <p className="note-problem">{problem}</p>}
        </section>
    );
}

interface RowProps {
    row: SnapshotRow;
    busy: boolean;
    onTake(task: string): void;
    onForget(): void;
}

function Row({ row, busy, onTake, onForget }: RowProps) {
    const words = useWords();
    const { snapshot, sources } = row;
    return (
        <li className="snapshot-row">
            <div>
                <p className="settings-label">{row.environment ?? words.ownSetup}</p>
                <p className="settings-hint">
                    {snapshot
                        ? `From ${snapshot.task}, ${takenAt(snapshot)}`
                        : `No ${words.snapshot.toLowerCase()}: ${words.terms.tasks} are set up from nothing.`}
                </p>
            </div>
            {sources.length > 0 && (
                <Picker
                    label={`Take ${withArticle(words.snapshot)} from ${withArticle(words.terms.task)}`}
                    value=""
                    onChange={(task) => {
                        if (task && !busy) {
                            onTake(task);
                        }
                    }}
                    options={[
                        ["", snapshot ? "Replace from…" : "Take from…"],
                        ...sources.map((name): [string, string] => [name, name]),
                    ]}
                />
            )}
            {snapshot && (
                <button type="button" className="button" disabled={busy} onClick={onForget}>
                    Forget
                </button>
            )}
        </li>
    );
}
