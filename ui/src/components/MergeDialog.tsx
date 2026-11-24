import { useEffect, useState } from "react";
import { api } from "../api";
import type { MergeMethod, MergeOutcome, MergeRules, Pull, Task } from "../api/types";
import {
    type AutoMergeChoice,
    autoMergeChoice,
    heldBack,
    METHOD_NAMES,
    type MergeWay,
    mergeButton,
    mergeProblem,
    mergeSaid,
    mergeWays,
    outcomeSaid,
    requirementsSaid,
    shortHead,
    WAY_NAMES,
} from "../merging";
import { checksSummary, pullTone } from "../pulls";
import { useStore } from "../store";
import { usePull } from "../usePull";
import { useWords } from "../useWords";
import { Dialog } from "./Dialog";
import { Picker } from "./Picker";

interface Props {
    project: string;
    task: Task;
    /** The pull request as it was when the dialog was asked for. */
    opened: Pull;
    onClose(): void;
}

/** What came of the merge the dialog made, with the pull request it was of. */
interface Done {
    outcome: MergeOutcome;
    pull: Pull;
    method: MergeMethod;
}

/** What went wrong, as a dialog says it. */
function said(error: unknown): string {
    return mergeProblem(String(error).replace(/^Error: /, ""));
}

/**
 * The rules of the repository, asked for when the dialog opens, and the method chosen from
 * them. The pull request is read once more too: it is merged at the head the user sees.
 */
function useMergeRules(project: string, task: Task, onProblem: (problem: string) => void) {
    const loadPull = useStore((state) => state.loadPull);
    const [rules, setRules] = useState<MergeRules | null>(null);
    const [method, setMethod] = useState<MergeMethod | null>(null);
    useEffect(() => {
        let current = true;
        void loadPull(api, project, task.name, task.id);
        api.projectMergeRules(project)
            .then((found) => {
                if (current) {
                    setRules(found);
                    setMethod((chosen) => chosen ?? found.method);
                }
            })
            .catch((error: unknown) => current && onProblem(said(error)));
        return () => {
            current = false;
        };
    }, [project, task.name, task.id, loadPull, onProblem]);
    return { rules, method, setMethod };
}

/** Asks one thing at a time: busy meanwhile, and what went wrong kept to be said. */
function useAsking() {
    const [busy, setBusy] = useState(false);
    const [problem, setProblem] = useState<string | null>(null);
    const run = async (asked: () => Promise<void>) => {
        setBusy(true);
        setProblem(null);
        try {
            await asked();
        } catch (error) {
            setProblem(said(error));
        } finally {
            setBusy(false);
        }
    };
    return { busy, problem, setProblem, run };
}

/**
 * Merges a task's pull request: the method, where it goes and from which commit, how its
 * checks and review stand, and a split button whose menu chooses how to merge now. Beside
 * it, GitHub's own auto-merge, on or off at once. A merge cannot be taken back, so a click
 * beside the dialog never closes it, and the main button says what it does.
 */
export function MergeDialog({ project, task, opened, onClose }: Props) {
    const words = useWords();
    const pull = usePull(task) ?? opened;
    const { busy, problem, setProblem, run } = useAsking();
    const { rules, method, setMethod } = useMergeRules(project, task, setProblem);
    const [way, setWay] = useState<MergeWay>("now");
    const [done, setDone] = useState<Done | null>(null);

    const merge = (chosen: MergeMethod) =>
        run(async () => {
            const report = await api.taskPullMerge(project, task.name, chosen, way, pull.head);
            setDone({ outcome: report.outcome, pull: report.pull ?? pull, method: chosen });
        });
    const setAuto = (on: boolean, chosen: MergeMethod) =>
        run(async () => {
            await (on
                ? api.taskPullMerge(project, task.name, chosen, "auto", pull.head)
                : api.taskPullAutoMergeOff(project, task.name));
        });

    const title = `Merge ${words.terms.pullRequest} #${pull.number}`;
    if (done) {
        return (
            <Dialog title={title} size="small" onClose={onClose}>
                <p className="merge-done" data-outcome={done.outcome}>
                    {outcomeSaid(done.outcome, done.pull, done.method)}
                </p>
            </Dialog>
        );
    }
    const held = heldBack(pull, way);
    return (
        <Dialog
            title={title}
            size="small"
            onClose={onClose}
            dismiss="Cancel"
            busy={busy}
            problem={problem}
            actions={
                <SplitButton
                    way={way}
                    ways={rules ? mergeWays(rules) : ["now"]}
                    disabled={method === null || held !== null}
                    onWay={setWay}
                    onPress={() => method && void merge(method)}
                />
            }
        >
            <Facts pull={pull} />
            {rules !== null && (
                <MergeChoices
                    pull={pull}
                    rules={rules}
                    method={method}
                    busy={busy}
                    onMethod={setMethod}
                    onAuto={(on) => method && void setAuto(on, method)}
                />
            )}
            {rules === null && problem === null && (
                <p className="merge-note">Reading what the repository allows…</p>
            )}
            {method && (
                <p className="merge-said">
                    {held ?? (busy ? "Merging…" : mergeSaid(pull, way, method))}
                </p>
            )}
        </Dialog>
    );
}

/** Where the pull request goes and from which commit, and how its checks and review stand. */
function Facts({ pull }: { pull: Pull }) {
    const requirements = requirementsSaid(pull);
    return (
        <>
            <p className="merge-title">{pull.title}</p>
            <dl className="merge-facts">
                <dt>Into</dt>
                <dd className="mono">{pull.base || "its base"}</dd>
                <dt>At</dt>
                <dd className="mono" title={pull.head}>
                    {shortHead(pull.head)}
                </dd>
                <dt>Checks</dt>
                <dd className="pull-summary" data-tone={pullTone(pull)}>
                    {checksSummary(pull)}
                </dd>
                <dt>Review</dt>
                <dd>{pull.review ? pull.review.replaceAll("_", " ") : "none asked for"}</dd>
            </dl>
            {requirements && <p className="merge-note">{requirements}</p>}
        </>
    );
}

interface ChoicesProps {
    pull: Pull;
    rules: MergeRules;
    method: MergeMethod | null;
    busy: boolean;
    onMethod(method: MergeMethod): void;
    onAuto(on: boolean): void;
}

/** The method, from those the repository allows, and GitHub's auto-merge. */
function MergeChoices({ pull, rules, method, busy, onMethod, onAuto }: ChoicesProps) {
    if (method === null) {
        return <p className="merge-note">This repository allows no way to merge.</p>;
    }
    const choice = autoMergeChoice(pull, rules);
    return (
        <div className="merge-choices">
            <Picker
                label="How the commits go in"
                value={method}
                onChange={(chosen) => onMethod(chosen as MergeMethod)}
                options={rules.methods.map((known) => [known, METHOD_NAMES[known]])}
            />
            <AutoMerge choice={choice} busy={busy} onAuto={onAuto} />
        </div>
    );
}

/** GitHub's auto-merge: a box that turns it on or off at once, or why it cannot be. */
function AutoMerge(props: { choice: AutoMergeChoice; busy: boolean; onAuto(on: boolean): void }) {
    const { choice, busy, onAuto } = props;
    if (choice.kind === "unavailable") {
        return <p className="merge-note">{choice.why}</p>;
    }
    const on = choice.kind === "on";
    return (
        <>
            <label
                className="merge-auto"
                title="GitHub's own auto-merge: GitHub merges it once its requirements are met"
            >
                <input
                    type="checkbox"
                    checked={on}
                    disabled={busy}
                    onChange={(event) => onAuto(event.target.checked)}
                />
                Enable auto-merge
            </label>
            {on && (
                <p className="merge-note">
                    GitHub merges it with {METHOD_NAMES[choice.method].toLowerCase()} once its
                    requirements are met. Untick to stop that.
                </p>
            )}
        </>
    );
}

interface SplitProps {
    way: MergeWay;
    ways: MergeWay[];
    disabled: boolean;
    onWay(way: MergeWay): void;
    onPress(): void;
}

/**
 * The main button, which merges the way chosen, and beside it a menu that only chooses: a
 * native one lying unseen over its chevron, so it behaves as every other menu does.
 */
function SplitButton({ way, ways, disabled, onWay, onPress }: SplitProps) {
    return (
        <div className="split-button">
            <button
                type="button"
                className="button button-primary"
                disabled={disabled}
                onClick={onPress}
            >
                {mergeButton(way)}
            </button>
            <span className="button split-button-menu" title="Chooses how to merge">
                <svg viewBox="0 0 10 6" aria-hidden="true">
                    <path d="M1 1l4 4 4-4" />
                </svg>
                <select
                    aria-label="How to merge"
                    value={way}
                    onChange={(event) => onWay(event.target.value as MergeWay)}
                >
                    {ways.map((known) => (
                        <option key={known} value={known}>
                            {WAY_NAMES[known]}
                        </option>
                    ))}
                </select>
            </span>
        </div>
    );
}
