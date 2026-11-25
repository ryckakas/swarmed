import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { Issue, IssueDetail, IssueFilter, IssueList } from "../api/types";
import { problemOf } from "../branchPrefix";
import { issueLeavesOut } from "../issueHtml";
import {
    ageOf,
    FIRST_FILTER,
    type IssuesTarget,
    issueLine,
    issuesCloseBeside,
    STATE_CHOICES,
    shownIssues,
    WHO_CHOICES,
} from "../issues";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { Dialog } from "./Dialog";
import { UntrustedMarkdown } from "./Markdown";
import { Picker } from "./Picker";

/** How long typing in the search box has to pause before GitHub is asked, in milliseconds. */
const PAUSE_MS = 450;

/**
 * A repository's GitHub issues, opened from its row in the sidebar or the keyboard: the
 * filters and the list on one side, the issue picked on the other, and a way to start a task
 * on it. Anyone can write an issue: what it says is drawn as GitHub draws it, with only the
 * HTML GitHub allows, narrowed further, nothing run or loaded, and its source a press away.
 */
export function IssuesDialog() {
    const target = useStore((state) => state.issuesOf);
    if (target === null) {
        return null;
    }
    // A key per repository and issue asked for, so one's list never shows under another's name.
    return <Issues key={`${target.project}#${target.issue ?? ""}`} target={target} />;
}

function Issues({ target }: { target: IssuesTarget }) {
    const words = useWords();
    const { project } = target;
    const showIssues = useStore((state) => state.showIssues);
    const startFromIssue = useStore((state) => state.startFromIssue);
    const report = useStore((state) => state.report);
    const [filter, setFilter] = useState<IssueFilter>(FIRST_FILTER);
    const [typed, setTyped] = useState("");
    const [picked, setPicked] = useState<number | null>(target.issue);
    const { list, reading, read } = useIssueList(project, filter);
    const detail = useIssueDetail(project, picked);
    // GitHub is asked for the words once typing pauses; the list holds back meanwhile.
    useEffect(() => {
        // The same words keep the same filter: a new one would ask GitHub again.
        const settle = () =>
            setFilter((now) => (now.search === typed ? now : { ...now, search: typed }));
        const timer = setTimeout(settle, PAUSE_MS);
        return () => clearTimeout(timer);
    }, [typed]);
    const shown = list ? shownIssues(list.issues, typed) : [];
    const chosen = pickedIssue(picked, shown, detail);
    const start = () => chosen && startFromIssue(project, chosen);
    return (
        <Dialog
            title={`Issues of ${project}`}
            size="wide"
            fixedHeight={true}
            panes={true}
            closeOnBackdrop={issuesCloseBeside(filter, typed)}
            onClose={() => showIssues(null)}
            aside={
                chosen && (
                    <button
                        type="button"
                        className="button"
                        title={`Opens ${chosen.url}`}
                        onClick={() => api.openLink(chosen.url).catch(report)}
                    >
                        Open on GitHub
                    </button>
                )
            }
            actions={
                <button
                    type="button"
                    className="button button-primary"
                    disabled={chosen === null}
                    title={
                        chosen
                            ? `Opens ${words.newTask.toLowerCase()} with a prompt about #${chosen.number}, for you to read and send`
                            : "Pick an issue first"
                    }
                    onClick={start}
                >
                    Start a {words.terms.task}
                </button>
            }
        >
            <div className="issues-main">
                <section className="issues-side" aria-label="Issues">
                    <Filters
                        filter={filter}
                        onFilter={setFilter}
                        typed={typed}
                        onTyped={setTyped}
                        onRefresh={read}
                    />
                    <IssueRows
                        list={list}
                        shown={shown}
                        reading={reading}
                        picked={picked}
                        onPick={setPicked}
                    />
                </section>
                <section className="issues-detail" aria-live="polite">
                    <IssueShown picked={picked} detail={detail} />
                </section>
            </div>
        </Dialog>
    );
}

/** The issue the buttons act on: the one picked, as the list or its own reading has it. */
function pickedIssue(
    picked: number | null,
    shown: Issue[],
    detail: IssueRead,
): Pick<Issue, "number" | "title" | "url"> | null {
    if (picked === null) {
        return null;
    }
    const read = typeof detail === "object" && detail !== null ? detail : null;
    return read ?? shown.find((issue) => issue.number === picked) ?? null;
}

interface FiltersProps {
    filter: IssueFilter;
    onFilter(filter: IssueFilter): void;
    typed: string;
    onTyped(typed: string): void;
    onRefresh(): void;
}

/** Whose issues, open or closed ones, words to look for, and a way to ask again. */
function Filters({ filter, onFilter, typed, onTyped, onRefresh }: FiltersProps) {
    return (
        <div className="issues-filters">
            <Picker
                label="Whose issues"
                value={filter.who}
                onChange={(who) => onFilter({ ...filter, who: who as IssueFilter["who"] })}
                options={WHO_CHOICES}
            />
            <Picker
                label="Open or closed"
                value={filter.state}
                onChange={(state) => onFilter({ ...filter, state: state as IssueFilter["state"] })}
                options={STATE_CHOICES}
            />
            <input
                className="issues-search"
                type="search"
                aria-label="Search issues"
                placeholder="Search: label:bug login"
                value={typed}
                onChange={(event) => onTyped(event.target.value)}
            />
            <button type="button" className="changes-refresh" onClick={onRefresh}>
                Refresh
            </button>
        </div>
    );
}

interface RowsProps {
    list: IssueList | null;
    shown: Issue[];
    reading: boolean;
    picked: number | null;
    onPick(number: number): void;
}

/** The issues listed, each its title and what it is about on one line under it. */
function IssueRows({ list, shown, reading, picked, onPick }: RowsProps) {
    const now = Date.now() / 1000;
    if (list === null) {
        return <p className="issues-note">{reading ? "Asking GitHub…" : ""}</p>;
    }
    if (list.unavailable !== null) {
        return <p className="issues-note">No issues can be listed here: {list.unavailable}</p>;
    }
    return (
        <>
            {shown.length === 0 && (
                <p className="issues-note">{reading ? "Asking GitHub…" : "No issue matches."}</p>
            )}
            <ul className="issues-list" aria-busy={reading}>
                {shown.map((issue) => (
                    <li key={issue.number}>
                        <button
                            type="button"
                            className="issue-row"
                            aria-current={issue.number === picked}
                            onClick={() => onPick(issue.number)}
                        >
                            <span className="issue-row-title">{issue.title}</span>
                            <span className="issue-row-line">{issueLine(issue, now)}</span>
                        </button>
                    </li>
                ))}
            </ul>
        </>
    );
}

/** The issue picked, read whole, or why it is not shown yet. */
function IssueShown({ picked, detail }: { picked: number | null; detail: IssueRead }) {
    if (picked === null) {
        return <p className="issues-note">Pick an issue to read it here.</p>;
    }
    if (detail === null) {
        return <p className="issues-note">Reading #{picked}…</p>;
    }
    if (typeof detail === "string") {
        return (
            <p className="issues-note">
                #{picked} could not be read: {detail}
            </p>
        );
    }
    // A key per issue, so each one opens formatted, whatever the one before was shown as.
    return <IssueText key={detail.number} detail={detail} />;
}

/**
 * What an issue and its comments say: drawn as GitHub draws it, or as the text it is, which is
 * what an agent reads. When drawing it leaves anything out, it says so.
 */
function IssueText({ detail }: { detail: IssueDetail }) {
    const [source, setSource] = useState(false);
    const leftOut = useMemo(() => issueLeavesOut(detail), [detail]);
    const now = Date.now() / 1000;
    const assigned =
        detail.assignees.length > 0 ? `Assigned to ${detail.assignees.join(", ")}.` : null;
    return (
        <article className="issue-detail">
            <div className="issue-detail-head">
                <h3 className="issue-detail-title">{detail.title}</h3>
                <button
                    type="button"
                    className="changes-refresh"
                    title={
                        source
                            ? "Shows the issue formatted, as GitHub would"
                            : "Shows what the issue says as written, which is what an agent reads"
                    }
                    onClick={() => setSource((shown) => !shown)}
                >
                    {source ? "Show formatted" : "Show source"}
                </button>
            </div>
            <p className="issue-row-line">{issueLine(detail, now)}</p>
            {assigned && <p className="issue-row-line">{assigned}</p>}
            {leftOut && !source && (
                <p className="issue-left-out">
                    Parts of this issue are not shown here; Show source shows the whole text.
                </p>
            )}
            {detail.body.trim() === "" ? (
                <p className="issues-note">It says nothing more.</p>
            ) : (
                <Said text={detail.body} source={source} />
            )}
            {detail.comments.map((comment, index) => (
                <section
                    // Comments have no id of their own here; their order never changes.
                    // biome-ignore lint/suspicious/noArrayIndexKey: oldest first, only added to
                    key={index}
                    className="issue-comment"
                    aria-label={`Comment by ${comment.author || "a deleted account"}`}
                >
                    <p className="issue-comment-head">
                        {comment.author || "A deleted account"},{" "}
                        {ageOf(comment.created_at, now) ?? comment.created_at}
                    </p>
                    {comment.hidden === null ? (
                        <Said text={comment.body} source={source} />
                    ) : (
                        <p className="issues-note">Hidden on GitHub as {comment.hidden}.</p>
                    )}
                </section>
            ))}
        </article>
    );
}

/** What an issue or a comment says: formatted, or as the text it is. */
function Said({ text, source }: { text: string; source: boolean }) {
    return source ? <pre className="issue-source">{text}</pre> : <UntrustedMarkdown text={text} />;
}

/** The list for `filter`, while it is read again, and a way to read it again. */
function useIssueList(project: string, filter: IssueFilter) {
    const report = useStore((state) => state.report);
    const [list, setList] = useState<IssueList | null>(null);
    const [reading, setReading] = useState(true);
    const [asked, setAsked] = useState(0);
    // biome-ignore lint/correctness/useExhaustiveDependencies: asked exists to trigger a re-read
    useEffect(() => {
        let current = true;
        setReading(true);
        api.projectIssues(project, filter)
            .then((read) => current && setList(read))
            .catch((error: unknown) => current && report(error))
            .finally(() => current && setReading(false));
        return () => {
            current = false;
        };
    }, [project, filter, asked, report]);
    return { list, reading, read: () => setAsked((count) => count + 1) };
}

/** An issue read whole, why it could not be, or `null` while it is read. */
type IssueRead = IssueDetail | string | null;

/** Issue `number` read whole, read again when another is picked. */
function useIssueDetail(project: string, number: number | null): IssueRead {
    const [detail, setDetail] = useState<IssueRead>(null);
    useEffect(() => {
        setDetail(null);
        if (number === null) {
            return;
        }
        let current = true;
        api.projectIssue(project, number)
            .then((read) => current && setDetail(read))
            .catch((error: unknown) => current && setDetail(problemOf(error)));
        return () => {
            current = false;
        };
    }, [project, number]);
    return detail;
}
