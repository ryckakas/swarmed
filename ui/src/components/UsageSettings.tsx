import { useEffect, useState } from "react";
import { api } from "../api";
import type { Costs, Usage } from "../api/types";
import { placeName } from "../chats";
import { useStore } from "../store";
import { formatBytes, formatCpu, formatUsd, totalBytes } from "../usage";
import { useWords } from "../useWords";
import { ActivityCharts } from "./ActivityCharts";
import { CleanupSettings } from "./CleanupSettings";

/** How many worktrees are listed before the rest are summed up in one line. */
const LISTED = 8;

/** What Swarmed takes up on disk, and what it and its agents are using right now. */
export function UsageSettings() {
    const words = useWords();
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
        <>
            <CostFigures />
            <ActivityCharts />
            <section className="settings-group" aria-label="Storage and activity">
                <h3>Storage and activity</h3>
                <div className="settings-row">
                    <p className="settings-hint">
                        {usage
                            ? `${formatBytes(totalBytes(usage))} on disk in all. Worktrees are your tasks' checkouts, with whatever was built in them.`
                            : `Measuring the ${words.terms.worktrees}…`}
                    </p>
                    <button type="button" className="button" disabled={measuring} onClick={measure}>
                        {measuring ? "Measuring…" : "Measure again"}
                    </button>
                </div>
                {usage && <Figures usage={usage} />}
            </section>
            <CleanupSettings onCleaned={measure} />
        </>
    );
}

/** What the agents say their work has cost: today, this week, in all, and by task. */
function CostFigures() {
    const words = useWords();
    const [costs, setCosts] = useState<Costs | null>(null);
    useEffect(() => {
        // Not being able to read them is no reason to interrupt the settings.
        api.costs().then(setCosts, () => undefined);
    }, []);
    if (!costs) {
        return null;
    }
    return (
        <section className="settings-group" aria-label="Cost">
            <h3>Cost</h3>
            <p className="settings-hint">
                As the agents report it, at API prices. On a subscription this is what the work
                would have cost, not what you paid. Agents that report nothing count as nothing.
            </p>
            <dl className="usage">
                <Figure name="Today">{formatUsd(costs.today)}</Figure>
                <Figure name="Last 7 days">{formatUsd(costs.week)}</Figure>
                <Figure name="In all">{formatUsd(costs.total)}</Figure>
                {costs.tasks.map((task) => (
                    <Figure
                        key={`${task.project}/${task.task}`}
                        name={`${placeName(task.project, words)} / ${task.task}`}
                        detail={words.terms.task}
                    >
                        {formatUsd(task.usd)}
                    </Figure>
                ))}
            </dl>
        </section>
    );
}

function Figures({ usage }: { usage: Usage }) {
    const words = useWords();
    const listed = usage.worktrees.slice(0, LISTED);
    const rest = usage.worktrees.slice(LISTED);
    const restBytes = rest.reduce((sum, worktree) => sum + (worktree.bytes ?? 0), 0);
    return (
        <dl className="usage">
            <Figure name="Conversations" detail={`${usage.events.toLocaleString()} events`}>
                {formatBytes(usage.database_bytes)}
            </Figure>
            <Figure name="Notes">{formatBytes(usage.notes_bytes)}</Figure>
            <Figure name="Images" detail="pasted, and returned by tools">
                {formatBytes(usage.images_bytes)}
            </Figure>
            <Figure name="Checkpoints" detail="what each turn changed">
                {formatBytes(usage.checkpoints_bytes)}
            </Figure>
            <Figure name="Browsers" detail={`each ${words.terms.task}'s own Chrome`}>
                {formatBytes(usage.browsers_bytes)}
            </Figure>
            {usage.repolore_bytes > 0 && (
                <Figure name="repolore" detail="the built-in's settings and index">
                    {formatBytes(usage.repolore_bytes)}
                </Figure>
            )}
            {listed.map((worktree) => (
                <Figure
                    key={worktree.path}
                    name={`${placeName(worktree.project, words)} / ${worktree.task}`}
                    detail={words.terms.worktree}
                >
                    {worktree.bytes === null ? "not measured" : formatBytes(worktree.bytes)}
                </Figure>
            ))}
            {rest.length > 0 && (
                <Figure name={`${rest.length} more ${words.terms.worktrees}`}>
                    {formatBytes(restBytes)}
                </Figure>
            )}
            {usage.removed_bytes > 0 && (
                <Figure
                    name={`Removed ${words.terms.tasks}`}
                    detail={`${words.terms.worktrees} not deleted yet, or that could not be`}
                >
                    {formatBytes(usage.removed_bytes)}
                </Figure>
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
                    name={session.title || words.untitled}
                    detail={words.terms.agent}
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
