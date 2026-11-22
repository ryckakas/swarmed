import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { cloudTitle } from "../cloud";
import { parseDiff } from "../diff";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { Picker } from "./Picker";

/**
 * What a cloud task changed, as its provider reports it, and the way to bring those changes
 * into one of the repository's local tasks: applied to its worktree, not committed.
 */
export function CloudDiffDialog() {
    const cloudId = useStore((state) => state.cloudDiffOf);
    const showCloudDiff = useStore((state) => state.showCloudDiff);
    const projects = useStore((state) => state.projects);
    const refresh = useStore((state) => state.refresh);
    const report = useStore((state) => state.report);
    const dialog = useRef<HTMLDialogElement>(null);
    const diff = useCloudDiff(cloudId);
    const [applied, setApplied] = useState<string | null>(null);

    // A modal <dialog> keeps the keyboard inside it and closes on Esc by itself.
    useEffect(() => {
        if (cloudId !== null && !dialog.current?.open) {
            dialog.current?.showModal();
        }
    }, [cloudId]);
    // Another task's changes were not applied anywhere yet.
    // biome-ignore lint/correctness/useExhaustiveDependencies: reset when another task is shown
    useEffect(() => setApplied(null), [cloudId]);

    const project = projects.find((one) => one.cloud.some((task) => task.id === cloudId));
    const cloud = project?.cloud.find((task) => task.id === cloudId);
    if (cloudId === null || !(project && cloud)) {
        return null;
    }
    const tasks = project.tasks.filter((task) => !task.archived);
    const apply = (task: string) =>
        api
            .cloudTaskApply(cloudId, project.name, task)
            .then(() => {
                setApplied(task);
                return refresh(api);
            })
            .catch(report);
    return (
        <dialog
            ref={dialog}
            className="confirm compare"
            aria-labelledby="cloud-diff-title"
            onClose={() => showCloudDiff(null)}
        >
            <h2 id="cloud-diff-title">{cloudTitle(cloud)}</h2>
            <Changes diff={diff} />
            {applied && (
                <p>Applied to {applied}, uncommitted. Its changes are in that task's panel.</p>
            )}
            <div className="confirm-actions">
                {typeof diff === "string" && tasks.length > 0 && (
                    <Picker
                        label="Apply these changes to a local task"
                        value=""
                        onChange={(task) => task && void apply(task)}
                        options={[
                            ["", "Apply to a local task…"],
                            ...tasks.map((task): [string, string] => [task.name, task.name]),
                        ]}
                    />
                )}
                <button type="button" className="button" onClick={() => showCloudDiff(null)}>
                    Done
                </button>
            </div>
        </dialog>
    );
}

/** What a cloud task changed: `null` while it is asked for, or why there is nothing. */
type CloudDiff = string | { error: string } | null;

function useCloudDiff(cloudId: number | null): CloudDiff {
    const [diff, setDiff] = useState<CloudDiff>(null);
    useEffect(() => {
        setDiff(null);
        if (cloudId === null) {
            return;
        }
        let current = true;
        api.cloudTaskDiff(cloudId).then(
            (read) => current && setDiff(read),
            (error: unknown) => current && setDiff({ error: String(error) }),
        );
        return () => {
            current = false;
        };
    }, [cloudId]);
    return diff;
}

function Changes({ diff }: { diff: CloudDiff }) {
    const words = useWords();
    if (diff === null) {
        return <p>{words.askingChanges}</p>;
    }
    return typeof diff === "string" ? <DiffText text={diff} /> : <p>{diff.error}</p>;
}

function DiffText({ text }: { text: string }) {
    return (
        <div className="compare-diff diff">
            <div className="diff-lines">
                {parseDiff(text).map((line, index) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: the lines of one diff never reorder
                    <div key={index} className={`diff-line diff-${line.kind}`}>
                        {line.text || (line.kind === "hunk" ? "⋯" : "")}
                    </div>
                ))}
            </div>
        </div>
    );
}
