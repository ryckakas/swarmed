import { useEffect, useState } from "react";
import { api } from "../api";
import type { AgentEvent, FileChange, Session, Task } from "../api/types";
import { STATUS_ITEM_LABELS, statusParts } from "../statusline";
import { useStore } from "../store";

interface Props {
    project: string;
    task: Task;
    session: Session;
    events: AgentEvent[];
}

/** The line under the message box: the user's script if they set one, else the built-in pieces. */
export function StatusLine(props: Props) {
    const open = useStore((state) => state.statusOpen);
    const toggle = useStore((state) => state.toggleStatus);
    const settings = useStore((state) => state.settings);
    if (!settings) {
        return null;
    }
    const scripted = settings.status_command.trim() !== "";
    return (
        <div className="status-line" data-open={open}>
            <button
                type="button"
                className="status-toggle"
                aria-expanded={open}
                aria-label={open ? "Hide the status line" : "Show the status line"}
                title={open ? "Hide the status line" : "Show the status line"}
                onClick={toggle}
            >
                <svg viewBox="0 0 10 10" aria-hidden="true">
                    <path d="M1.5 3.5 5 7l3.5-3.5" />
                </svg>
            </button>
            {open && (scripted ? <Scripted {...props} /> : <BuiltIn {...props} />)}
        </div>
    );
}

/** What changes while a session runs, as one value to re-read on: its status and its edits. */
function useTick(task: Task, session: Session): string {
    const edits = useStore((state) => state.edits[task.id] ?? 0);
    return `${session.status}-${edits}`;
}

function BuiltIn({ project, task, session, events }: Props) {
    const items = useStore((state) => state.settings?.status_items ?? []);
    const tick = useTick(task, session);
    const wanted = items.includes("changes");
    const [changes, setChanges] = useState<FileChange[] | null>(null);
    // biome-ignore lint/correctness/useExhaustiveDependencies: a new tick means read again
    useEffect(() => {
        if (!wanted) {
            return;
        }
        let current = true;
        api.taskChanges(project, task.name)
            .then((read) => current && setChanges(read))
            // The line just goes without the piece it could not read.
            .catch(() => current && setChanges(null));
        return () => {
            current = false;
        };
    }, [project, task.name, wanted, tick]);

    const parts = statusParts(items, { session, task, events, changes });
    return (
        <p className="status-text">
            {parts.map((part) => (
                <span
                    key={part.item}
                    className={`status-part status-${part.item}`}
                    title={STATUS_ITEM_LABELS[part.item]}
                >
                    {part.runs.map(([text, tone]) => (
                        <span key={`${tone}-${text}`} className={`status-run-${tone}`}>
                            {text}
                        </span>
                    ))}
                </span>
            ))}
        </p>
    );
}

function Scripted({ task, session }: Props) {
    const tick = useTick(task, session);
    const command = useStore((state) => state.settings?.status_command);
    const [shown, setShown] = useState<{ text: string; failed: boolean }>({
        text: "",
        failed: false,
    });
    // biome-ignore lint/correctness/useExhaustiveDependencies: a new tick or script means run again
    useEffect(() => {
        let current = true;
        api.statusLine(session.id)
            .then((text) => current && setShown({ text: text ?? "", failed: false }))
            .catch((error: unknown) => current && setShown({ text: String(error), failed: true }));
        return () => {
            current = false;
        };
    }, [session.id, tick, command]);
    if (shown.failed) {
        return <p className="status-text status-failed">{shown.text}</p>;
    }
    // A script that separates its values with bars gets a badge for each, like the built-in line.
    const values = shown.text
        .split("|")
        .map((value) => value.trim())
        .filter((value) => value !== "");
    return (
        <p className="status-text">
            {values.map((value, index) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: the values of one line never reorder
                <span key={index} className={`status-part status-nth-${index % 5}`}>
                    {value}
                </span>
            ))}
        </p>
    );
}
