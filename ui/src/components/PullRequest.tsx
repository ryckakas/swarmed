import { useState } from "react";
import { api } from "../api";
import type { Check, Pull, Task } from "../api/types";
import { mergeable } from "../merging";
import {
    CHECKS_SEARCHED_FROM,
    checkCounts,
    checksSummary,
    failedChecks,
    listedChecks,
    pullState,
    pullTone,
    pullTooltip,
} from "../pulls";
import { useStore } from "../store";
import { usePull, useReadPullOnce } from "../usePull";
import { useWords } from "../useWords";
import { capitalized } from "../wording";
import { MergeDialog } from "./MergeDialog";

/**
 * A task's pull request in the sidebar: its number, coloured by how its checks stand. What it
 * shows is what the core last read; the core follows it.
 */
export function PullBadge({ project, task }: { project: string; task: Task }) {
    const words = useWords();
    const pull = usePull(task);
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
            title={pullTooltip(pull, words)}
            aria-label={pullTooltip(pull, words)}
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
 * put the failed ones into the message box of the session on screen. Opening the panel asks
 * for it once.
 */
export function PullSection({ project, task }: { project: string; task: Task }) {
    const words = useWords();
    useReadPullOnce(project, task);
    const pull = usePull(task);
    const report = useStore((state) => state.report);
    const sessionId = useStore((state) => state.selectedSessionId);
    const [merging, setMerging] = useState<Pull | null>(null);
    if (!pull) {
        return null;
    }
    const open = (check: string | null) =>
        api.taskPullOpen(project, task.name, check).catch(report);
    const review = pull.review ? `, ${pull.review.replaceAll("_", " ")}` : "";
    const conflicts = pull.conflicts ? ", conflicts with its base" : "";
    const auto =
        pull.auto_merge && pull.state === "open"
            ? ", merges itself once its requirements are met"
            : "";
    const summary = `${checksSummary(pull)}${review}${conflicts}${auto}`;
    const sendFailure = failedChecks(pull).length > 0 && sessionId !== null;
    return (
        <section className="pull" aria-label={capitalized(words.terms.pullRequest)}>
            <button type="button" className="pull-title" onClick={() => void open(null)}>
                <span className="pull-number" data-state={pull.state}>
                    #{pull.number} {pullState(pull)}
                </span>
                {pull.title}
            </button>
            <Checks pull={pull} summary={summary} onOpen={(name) => void open(name)} />
            {(mergeable(pull) || sendFailure) && (
                <div className="pull-actions">
                    {mergeable(pull) && (
                        <button
                            type="button"
                            className="button"
                            title={`Merges the ${words.terms.pullRequest} on GitHub, or has GitHub merge it once its requirements are met`}
                            onClick={() => setMerging(pull)}
                        >
                            Merge…
                        </button>
                    )}
                    {sendFailure && (
                        <SendFailure project={project} task={task.name} sessionId={sessionId} />
                    )}
                </div>
            )}
            {merging && (
                <MergeDialog
                    project={project}
                    task={task}
                    opened={merging}
                    onClose={() => setMerging(null)}
                />
            )}
        </section>
    );
}

/**
 * Puts the failed checks of a task's pull request, with the last lines of their logs, into a
 * session's message box, for the user to read and send. Nothing reaches the agent unasked.
 */
export function SendFailure(props: { project: string; task: string; sessionId: number }) {
    const { project, task, sessionId } = props;
    const words = useWords();
    const report = useStore((state) => state.report);
    const draftFailure = useStore((state) => state.draftFailure);
    const [state, setState] = useState<"ready" | "reading" | "done">("ready");
    const put = () => {
        setState("reading");
        draftFailure(api, project, task, sessionId)
            .then(() => setState("done"))
            .catch((error: unknown) => {
                report(error);
                setState("ready");
            });
    };
    return (
        <button
            type="button"
            className="button"
            disabled={state === "reading"}
            title={`Puts the failed checks and the last lines of their logs into the message box, to read before sending to the ${words.terms.agent}`}
            onClick={put}
        >
            {state === "reading"
                ? "Reading the logs…"
                : state === "done"
                  ? "In the message box"
                  : `Send the failure to the ${words.terms.agent}`}
        </button>
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
    // Folded until the user says: the line that sums them up already tells what failed or runs.
    const [shown, setShown] = useState(false);
    const tone = pullTone(pull);
    if (pull.checks.length === 0) {
        return (
            <p className="pull-summary" data-tone={tone}>
                {summary}
            </p>
        );
    }
    return (
        <>
            <button
                type="button"
                className="pull-summary pull-summary-fold"
                data-tone={tone}
                aria-expanded={shown}
                title={shown ? "Folds the checks away" : "Shows each check"}
                onClick={() => setShown(!shown)}
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
