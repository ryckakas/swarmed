import { type CSSProperties, useEffect, useState } from "react";
import { api } from "../api";
import type { AgentEvent, FileChange, Session, Task } from "../api/types";
import {
    type ContextUsage,
    contextBreakdown,
    contextPercent,
    contextUsage,
    formatTokens,
    STATUS_ITEM_LABELS,
    type StatusPart,
    statusParts,
} from "../statusline";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { useEffortDefault } from "./options";

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

    const words = useWords();
    const running = useRunning(session.id, items.includes("duration"));
    const usualEffort = useEffortDefault(session.agent, session.model);
    const parts = statusParts(items, { session, task, events, changes, running, usualEffort });
    const usage = contextUsage(events);
    // Where the context badge starts, while its overview is shown: the overview stands over it.
    const [detail, setDetail] = useState<number | null>(null);
    const show = (badge: HTMLElement) => setDetail(badge.offsetLeft);
    const hide = () => setDetail(null);
    return (
        <>
            <p className="status-text">
                {parts.map((part) =>
                    part.item === "context" ? (
                        // A button, so the keyboard reaches the overview too: it shows on
                        // hover and on focus, and a click keeps it open until the next one.
                        <button
                            key={part.item}
                            type="button"
                            className="status-part status-context"
                            aria-expanded={detail !== null}
                            aria-label={`Context window: ${part.text}`}
                            onMouseEnter={(event) => show(event.currentTarget)}
                            onMouseLeave={hide}
                            onFocus={(event) => show(event.currentTarget)}
                            onBlur={hide}
                        >
                            <Runs part={part} />
                        </button>
                    ) : (
                        <span
                            key={part.item}
                            className={`status-part status-${part.item}`}
                            title={
                                part.item === "duration"
                                    ? words.timeWorked
                                    : STATUS_ITEM_LABELS[part.item]
                            }
                        >
                            <Runs part={part} />
                        </span>
                    ),
                )}
            </p>
            {detail !== null && usage && <ContextCard usage={usage} at={detail} />}
        </>
    );
}

/**
 * How long the session's running turn has run, counted up once a second while that is shown.
 * `null` between turns, and for a turn that began before this window was looking.
 */
function useRunning(sessionId: number, wanted: boolean): number | null {
    const began = useStore((state) => state.turnBegan[sessionId]);
    const [now, setNow] = useState(Date.now);
    const ticking = wanted && began !== undefined;
    useEffect(() => {
        if (!ticking) {
            return;
        }
        setNow(Date.now());
        const timer = setInterval(() => setNow(Date.now()), 1000);
        return () => clearInterval(timer);
    }, [ticking]);
    return ticking ? Math.max(0, now - began) : null;
}

function Runs({ part }: { part: StatusPart }) {
    return part.runs.map(([text, tone]) => (
        <span key={`${tone}-${text}`} className={`status-run-${tone}`}>
            {text}
        </span>
    ));
}

/** What fills the context window, part by part, shown over the status line. */
function ContextCard({ usage, at }: { usage: ContextUsage; at: number }) {
    const percent = contextPercent(usage);
    const parts = contextBreakdown(usage);
    // The stylesheet keeps it inside the line when the badge is near the right edge.
    const place = { "--badge-left": `${at}px` } as CSSProperties;
    return (
        <div className="context-card" role="tooltip" style={place}>
            <p className="context-title">
                Context window
                <span>
                    {formatTokens(usage.used)} of {formatTokens(usage.max)} tokens, {percent}%
                </span>
            </p>
            <div className="context-bar">
                <div style={{ width: `${Math.min(percent, 100)}%` }} />
            </div>
            {parts.length === 0 && <p className="context-none">The agent gave no breakdown.</p>}
            <dl className="context-parts">
                {parts.map((part) => (
                    <div key={part.name} className={`context-part context-${part.kind}`}>
                        <dt>{part.name}</dt>
                        <dd>{formatTokens(part.tokens)}</dd>
                    </div>
                ))}
            </dl>
        </div>
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
