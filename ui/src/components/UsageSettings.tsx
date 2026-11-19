import { useEffect, useState } from "react";
import { api } from "../api";
import type { Usage } from "../api/types";
import { useStore } from "../store";
import { formatBytes, formatCpu, totalBytes } from "../usage";

/** How many worktrees are listed before the rest are summed up in one line. */
const LISTED = 8;

/** What Swarmed takes up on disk, and what it and its agents are using right now. */
export function UsageSettings() {
    const report = useStore((state) => state.report);
    const [usage, setUsage] = useState<Usage | null>(null);
    const [measuring, setMeasuring] = useState(false);
    const measure = () => {
        setMeasuring(true);
        api.usage()
            .then(setUsage)
            .catch(report)
            .finally(() => setMeasuring(false));
    };
    // biome-ignore lint/correctness/useExhaustiveDependencies: measured once, when the settings open
    useEffect(measure, []);

    return (
        <section className="settings-group" aria-label="Storage and activity">
            <h3>Storage and activity</h3>
            <div className="settings-row">
                <p className="settings-hint">
                    {usage
                        ? `${formatBytes(totalBytes(usage))} on disk in all. Worktrees are your tasks' checkouts, with whatever was built in them.`
                        : "Measuring the worktrees…"}
                </p>
                <button type="button" className="button" disabled={measuring} onClick={measure}>
                    {measuring ? "Measuring…" : "Measure again"}
                </button>
            </div>
            {usage && <Figures usage={usage} />}
        </section>
    );
}

function Figures({ usage }: { usage: Usage }) {
    const listed = usage.worktrees.slice(0, LISTED);
    const rest = usage.worktrees.slice(LISTED);
    const restBytes = rest.reduce((sum, worktree) => sum + (worktree.bytes ?? 0), 0);
    return (
        <dl className="usage">
            <Figure name="Conversations" detail={`${usage.events.toLocaleString()} events`}>
                {formatBytes(usage.database_bytes)}
            </Figure>
            <Figure name="Notes">{formatBytes(usage.notes_bytes)}</Figure>
            {listed.map((worktree) => (
                <Figure
                    key={worktree.path}
                    name={`${worktree.project} / ${worktree.task}`}
                    detail="worktree"
                >
                    {worktree.bytes === null ? "not measured" : formatBytes(worktree.bytes)}
                </Figure>
            ))}
            {rest.length > 0 && (
                <Figure name={`${rest.length} more worktrees`}>{formatBytes(restBytes)}</Figure>
            )}
            <Figure
                name="Running now"
                detail={`Swarmed and what it started, ${usage.running.processes} processes`}
            >
                {formatBytes(usage.running.memory_bytes)}, {formatCpu(usage.running.cpu_percent)} of
                a core
            </Figure>
            {usage.sessions.map((session) => (
                <Figure
                    key={session.session_id}
                    name={session.title || "New session"}
                    detail="agent"
                >
                    {formatBytes(session.memory_bytes)}, {formatCpu(session.cpu_percent)}
                </Figure>
            ))}
        </dl>
    );
}

interface FigureProps {
    name: string;
    detail?: string;
    children: React.ReactNode;
}

function Figure({ name, detail, children }: FigureProps) {
    return (
        <div className="usage-row">
            <dt>
                <span className="usage-name">{name}</span>
                {detail && <span className="usage-detail">{detail}</span>}
            </dt>
            <dd>{children}</dd>
        </div>
    );
}
