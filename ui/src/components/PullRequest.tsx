import { useState } from "react";
import { sendOrQueue } from "../actions";
import { api } from "../api";
import type { Check, Pull, Task } from "../api/types";
import {
    CHECKS_SEARCHED_FROM,
    checkCounts,
    checksShownByDefault,
    checksSummary,
    failedChecks,
    fixPrompt,
    listedChecks,
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
                    onClick={() => sendOrQueue(sessionId, fixPrompt(pull)).catch(report)}
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

/** What each outcome is called on the buttons that narrow the list to it. */
const OUTCOME_NAMES: Record<Check["outcome"], string> = {
    failed: "Failed",
    running: "Running",
    passed: "Passed",
    skipped: "Skipped",
};

/**
 * The checks, what failed first. Over the list, a button per outcome narrows it to that one,
 * and a long list has a line to find a check by name.
 */
function CheckList({ pull, onOpen }: { pull: Pull; onOpen(name: string): void }) {
    const [outcome, setOutcome] = useState<Check["outcome"] | null>(null);
    const [typed, setTyped] = useState("");
    const counts = checkCounts(pull.checks);
    // An outcome no check has any more, after a re-run, narrows to nothing: it is let go.
    const narrowed = counts.some(([known]) => known === outcome) ? outcome : null;
    const listed = listedChecks(pull.checks, narrowed, typed);
    return (
        <>
            <div className="pull-check-filters">
                {counts.length > 1 &&
                    counts.map(([known, count]) => (
                        <button
                            key={known}
                            type="button"
                            className="pull-check-filter"
                            data-outcome={known}
                            aria-pressed={narrowed === known}
                            title={
                                narrowed === known
                                    ? "Shows every check again"
                                    : `Shows only what ${OUTCOME_NAMES[known].toLowerCase()}`
                            }
                            onClick={() => setOutcome(narrowed === known ? null : known)}
                        >
                            {OUTCOME_NAMES[known]} {count}
                        </button>
                    ))}
                {pull.checks.length >= CHECKS_SEARCHED_FROM && (
                    <input
                        className="key-input pull-check-search"
                        type="search"
                        aria-label="Find a check by name"
                        placeholder="Find a check"
                        spellCheck={false}
                        value={typed}
                        onChange={(event) => setTyped(event.target.value)}
                    />
                )}
            </div>
            <ul className="pull-checks">
                {listed.map((check) => (
                    // Two workflows may each have a job of the same name; their details differ.
                    <li key={`${check.workflow ?? ""}/${check.name}/${check.url ?? ""}`}>
                        <button
                            type="button"
                            className="pull-check"
                            data-outcome={check.outcome}
                            title={`Opens the details of ${check.name}`}
                            onClick={() => onOpen(check.name)}
                        >
                            <span className="pull-check-name">
                                {check.workflow && (
                                    <span className="pull-check-workflow">{check.workflow} / </span>
                                )}
                                {check.name}
                            </span>
                            <span className="pull-check-outcome">{OUTCOMES[check.outcome]}</span>
                        </button>
                    </li>
                ))}
                {listed.length === 0 && <li className="pull-check-none">No check matches.</li>}
            </ul>
        </>
    );
}
