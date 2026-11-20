import { useState } from "react";
import { api } from "../api";
import type { Check, Pull, Task } from "../api/types";
import {
    checksShownByDefault,
    checksSummary,
    failedChecks,
    fixPrompt,
    pullState,
    pullTone,
    pullTooltip,
} from "../pulls";
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
    const summary = `${checksSummary(pull)}${pull.review ? `, ${pull.review.replaceAll("_", " ")}` : ""}`;
    return (
        <section className="pull" aria-label="Pull request">
            <button type="button" className="pull-title" onClick={() => void open(null)}>
                <span className="pull-number" data-state={pull.state}>
                    #{pull.number} {pullState(pull)}
                </span>
                {pull.title}
            </button>
            <Checks pull={pull} summary={summary} onOpen={(name) => void open(name)} />
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

interface ChecksProps {
    pull: Pull;
    /** How the checks stand, in a line: what is left of them while they are folded away. */
    summary: string;
    onOpen(name: string): void;
}

/**
 * The checks of a pull request under the line that sums them up. Pressing that line folds the
 * list away or brings it back; a long list scrolls in a space of its own, so a repository
 * with dozens of checks does not push the rest of the panel out of sight.
 */
function Checks({ pull, summary, onOpen }: ChecksProps) {
    // Until the user says, shown while a check failed or runs and folded once all is well.
    const [chosen, setChosen] = useState<boolean | null>(null);
    const tone = pullTone(pull);
    if (pull.checks.length === 0) {
        return (
            <p className="pull-summary" data-tone={tone}>
                {summary}
            </p>
        );
    }
    const shown = chosen ?? checksShownByDefault(pull);
    return (
        <>
            <button
                type="button"
                className="pull-summary pull-summary-fold"
                data-tone={tone}
                aria-expanded={shown}
                title={shown ? "Folds the checks away" : "Shows each check"}
                onClick={() => setChosen(!shown)}
            >
                <svg
                    className="pull-fold-chevron"
                    viewBox="0 0 10 10"
                    aria-hidden="true"
                    data-open={shown}
                >
                    <path d="M3.5 1.5 7 5 3.5 8.5" />
                </svg>
                {summary}
            </button>
            {shown && <CheckList pull={pull} onOpen={onOpen} />}
        </>
    );
}

function CheckList({ pull, onOpen }: { pull: Pull; onOpen(name: string): void }) {
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
