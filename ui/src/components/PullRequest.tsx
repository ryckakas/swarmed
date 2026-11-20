import { api } from "../api";
import type { Check, Pull, Task } from "../api/types";
import { checksSummary, failedChecks, fixPrompt, pullState, pullTone, pullTooltip } from "../pulls";
import { useStore } from "../store";
import { usePull } from "../usePull";

/** A task's pull request in the sidebar: its number, coloured by how its checks stand. */
export function PullBadge({ project, task }: { project: string; task: Task }) {
    const pull = usePull(project, task);
    const report = useStore((state) => state.report);
    if (!pull) {
        return null;
    }
    return (
        <button
            type="button"
            className="pull-badge"
            data-tone={pullTone(pull)}
            data-state={pull.state}
            title={pullTooltip(pull)}
            aria-label={pullTooltip(pull)}
            onClick={() => api.taskPullOpen(project, task.name, null).catch(report)}
        >
            #{pull.number}
        </button>
    );
}

const OUTCOMES: Record<Check["outcome"], string> = {
    passed: "passed",
    failed: "failed",
    running: "running",
    skipped: "skipped",
};

/**
 * A task's pull request in the task panel: where it stands, each of its checks, and a way to
 * hand the failed ones to the session on screen.
 */
export function PullSection({ project, task }: { project: string; task: Task }) {
    const pull = usePull(project, task);
    const report = useStore((state) => state.report);
    const sessionId = useStore((state) => state.selectedSessionId);
    if (!pull) {
        return null;
    }
    const open = (check: string | null) =>
        api.taskPullOpen(project, task.name, check).catch(report);
    return (
        <section className="pull" aria-label="Pull request">
            <button type="button" className="pull-title" onClick={() => void open(null)}>
                <span className="pull-number" data-state={pull.state}>
                    #{pull.number} {pullState(pull)}
                </span>
                {pull.title}
            </button>
            <p className="pull-summary" data-tone={pullTone(pull)}>
                {checksSummary(pull)}
                {pull.review && `, ${pull.review.replaceAll("_", " ")}`}
            </p>
            <Checks pull={pull} onOpen={(name) => void open(name)} />
            {failedChecks(pull).length > 0 && sessionId !== null && (
                <button
                    type="button"
                    className="button"
                    title="Sends the failed checks to the session on screen, as a message"
                    onClick={() => api.sessionSend(sessionId, fixPrompt(pull)).catch(report)}
                >
                    Ask this session to fix them
                </button>
            )}
        </section>
    );
}

function Checks({ pull, onOpen }: { pull: Pull; onOpen(name: string): void }) {
    if (pull.checks.length === 0) {
        return null;
    }
    return (
        <ul className="pull-checks">
            {pull.checks.map((check) => (
                <li key={check.name}>
                    <button
                        type="button"
                        className="pull-check"
                        data-outcome={check.outcome}
                        title={`Opens the details of ${check.name}`}
                        onClick={() => onOpen(check.name)}
                    >
                        <span className="pull-check-name">{check.name}</span>
                        <span className="pull-check-outcome">{OUTCOMES[check.outcome]}</span>
                    </button>
                </li>
            ))}
        </ul>
    );
}
