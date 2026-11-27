import { useEffect, useMemo, useState } from "react";
import { setParent } from "../actions";
import { api } from "../api";
import type { Descendant, Session, SessionStatus } from "../api/types";
import { parentChosen, parentOptions } from "../parents";
import { useStore } from "../store";
import { findSessionHome, toggledId } from "../tree";
import { useWords } from "../useWords";
import type { StatusKind } from "../wording";
import {
    foldsDown,
    parentLine,
    shownWorkers,
    underSaid,
    type WorkerLine,
    type Workers,
    workersKey,
    workersOf,
    workersSummary,
} from "../workers";
import { nestedBy, WorkerParts } from "./CloseWorkers";
import { Picker } from "./Picker";
import { PullBadge } from "./PullRequest";
import { ParentLink } from "./Sidebar";
import { StatusMark } from "./StatusMark";

/**
 * The menu that chooses whom the session works for, of every other open session of its
 * repository: the sidebar lists it under that one, and both agents hear of it with their next
 * message.
 */
export function WorksForPicker({ session }: { session: Session }) {
    const words = useWords();
    const projects = useStore((state) => state.projects);
    return (
        <Picker
            label={`Whom this ${words.session} works for: it reports to that one, and is listed under it`}
            value={session.parent === null ? "" : String(session.parent)}
            onChange={(value) => void setParent(session.id, parentChosen(value))}
            options={parentOptions(projects, session, words)}
        />
    );
}

/**
 * Under the session's title, when anyone works with it: the session it works for, and one line
 * that counts those working for it, however far down and in whichever task, by how they are
 * doing. That line unfolds into their tree, kept per session in the store: nested as they work
 * for one another, each with how it is doing, its task where that is another and that task's pull
 * request, the closed ones folded below to reopen. Nothing for a session that works with nobody.
 */
export function WorkingWith({ session }: { session: Session }) {
    const words = useWords();
    const projects = useStore((state) => state.projects);
    const shown = useStore((state) => state.workersShown.includes(session.id));
    const toggleWorkersShown = useStore((state) => state.toggleWorkersShown);
    const parent = parentLine(projects, session);
    const workers = useWorkers(session);
    const anyone = workers.open.length > 0 || workers.closed.length > 0;
    if (!(parent || anyone)) {
        return null;
    }
    return (
        <div className="working-with">
            <div className="working-with-line">
                {parent && (
                    <span className="working-with-parent">
                        <span className="working-with-label">{words.worksForHeading}</span>
                        <OpenButton line={parent} />
                        {parent.elsewhere && (
                            <PullBadge
                                project={parent.elsewhere.project}
                                task={parent.elsewhere.task}
                            />
                        )}
                    </span>
                )}
                {anyone && (
                    <WorkersLine
                        workers={workers}
                        shown={shown}
                        onToggle={() => toggleWorkersShown(session.id)}
                    />
                )}
            </div>
            {anyone && shown && <WorkersTree key={session.id} workers={workers} />}
        </div>
    );
}

/** The kind a count is of, as the status mark takes it. */
function markOf(kind: StatusKind): { status: SessionStatus; unread: boolean } {
    return kind === "unread" ? { status: "idle", unread: true } : { status: kind, unread: false };
}

interface LineProps {
    workers: Workers;
    /** Whether their tree is unfolded under the line. */
    shown: boolean;
    onToggle: () => void;
}

/**
 * Those working for the session on one line, which unfolds into their tree: how many, counted by
 * how they are doing with the most urgent first, each count beside the sidebar's mark of it, and
 * how many are closed.
 */
function WorkersLine({ workers, shown, onToggle }: LineProps) {
    const words = useWords();
    const { lead, counts, closed, said } = workersSummary(workers, words);
    return (
        <button
            type="button"
            className="workers-summary"
            aria-expanded={shown}
            aria-label={said}
            title={shown ? "Fold them into this line" : "Show them as a tree, however far down"}
            onClick={onToggle}
        >
            <span className="workers-count">
                <svg className="fold-chevron" viewBox="0 0 10 10" aria-hidden="true">
                    <path d="M3.5 1.5 7 5 3.5 8.5" />
                </svg>
                {lead}:
            </span>
            {counts.map((one) => (
                <span key={one.kind} className="workers-count">
                    <StatusMark
                        {...markOf(one.kind)}
                        background={one.background}
                        label={one.said}
                    />
                    {one.said}
                </span>
            ))}
            {closed !== null && (
                <span className="workers-count">{counts.length > 0 ? `· ${closed}` : closed}</span>
            )}
        </button>
    );
}

const NONE: Descendant[] = [];

/**
 * Those that work for `session`, as the core lists them (`session_descendants`), the closed ones
 * included, drawn with how the tree has the open ones doing now. Read again whenever who works
 * for whom in its repository changes; what was read stays drawn meanwhile.
 */
function useWorkers(session: Session): Workers {
    const projects = useStore((state) => state.projects);
    const key = useStore((state) => {
        const home = findSessionHome(state.projects, session);
        return home ? workersKey(home.project) : "";
    });
    const [read, setRead] = useState<{ id: number; listed: Descendant[] } | null>(null);
    const id = session.id;
    // biome-ignore lint/correctness/useExhaustiveDependencies: key exists to have it read again
    useEffect(() => {
        let current = true;
        const keep = (listed: Descendant[]) => {
            if (current) {
                setRead({ id, listed });
            }
        };
        // A failed read keeps what is drawn: the session may be going, and its header with it.
        api.sessionDescendants(id, true).then(keep, () => undefined);
        return () => {
            current = false;
        };
    }, [id, key]);
    const listed = read?.id === id ? read.listed : NONE;
    return useMemo(() => workersOf(listed, projects, session), [listed, projects, session]);
}

/**
 * The tree of those working for the session, and its closed ones folded below it. A long tree
 * is folded to those working for it directly, each unfolded on its own.
 */
function WorkersTree({ workers }: { workers: Workers }) {
    const [unfolded, setUnfolded] = useState<number[]>([]);
    const folding = foldsDown(workers.open);
    const toggle = (id: number) => setUnfolded((ids) => toggledId(ids, id));
    return (
        <div className="workers-tree">
            {workers.open.length > 0 && (
                <ul className="workers">
                    {shownWorkers(workers.open, unfolded).map((line) => (
                        <OpenWorker
                            key={line.session.id}
                            line={line}
                            {...(folding && line.under > 0
                                ? {
                                      folded: !unfolded.includes(line.session.id),
                                      onFold: () => toggle(line.session.id),
                                  }
                                : {})}
                        />
                    ))}
                </ul>
            )}
            {workers.closed.length > 0 && <ClosedWorkers lines={workers.closed} />}
        </div>
    );
}

interface OpenProps {
    line: WorkerLine;
    /** Whether those under it are hidden, where the tree folds; unset where it does not. */
    folded?: boolean;
    onFold?: () => void;
}

/**
 * An open session that works with this one: pressing it opens it; its task's pull request, its
 * closed parent and the fold of those under it are buttons of their own.
 */
function OpenWorker({ line, folded, onFold }: OpenProps) {
    const { elsewhere } = line;
    return (
        <li className="worker-row" style={nestedBy(line.depth)}>
            <OpenButton line={line} />
            {elsewhere && <PullBadge project={elsewhere.project} task={elsewhere.task} />}
            {line.parent !== null && <ParentLink id={line.parent} />}
            {folded !== undefined && onFold && (
                <FoldUnder count={line.under} folded={folded} onFold={onFold} />
            )}
        </li>
    );
}

/** An open session by how it is doing, its title and its task where that is another: opens it. */
function OpenButton({ line }: { line: WorkerLine }) {
    const words = useWords();
    const open = useStore((state) => state.open);
    const { session, elsewhere } = line;
    const where = elsewhere ? `, in ${elsewhere.task.name}` : "";
    return (
        <button
            type="button"
            className="worker-open"
            title={`Open ${words.sessionNumbered(session.id).toLowerCase()}${where}: ${session.title || words.untitled}`}
            onClick={() => void open(api, session.id)}
        >
            <WorkerParts session={session} task={elsewhere?.task.name ?? null} />
        </button>
    );
}

/** Shows or hides the `count` sessions under a line of a long tree. */
function FoldUnder(props: { count: number; folded: boolean; onFold: () => void }) {
    const { count, folded, onFold } = props;
    const words = useWords();
    const what = underSaid(count, words);
    return (
        <button
            type="button"
            className="workers-toggle"
            aria-expanded={!folded}
            title={folded ? `Show ${what}` : `Hide ${what}`}
            onClick={onFold}
        >
            <svg className="fold-chevron" viewBox="0 0 10 10" aria-hidden="true">
                <path d="M3.5 1.5 7 5 3.5 8.5" />
            </svg>
            {count}
        </button>
    );
}

/**
 * The closed sessions that worked for this one, folded under one quiet line, each with its task
 * and whom it worked for where that was another, and a way to reopen it, which opens it.
 */
function ClosedWorkers({ lines }: { lines: WorkerLine[] }) {
    const words = useWords();
    const [shown, setShown] = useState(false);
    return (
        <>
            <button
                type="button"
                className="workers-toggle"
                aria-expanded={shown}
                onClick={() => setShown(!shown)}
            >
                <svg className="fold-chevron" viewBox="0 0 10 10" aria-hidden="true">
                    <path d="M3.5 1.5 7 5 3.5 8.5" />
                </svg>
                {words.closed(lines.length)}
            </button>
            {shown && (
                <ul className="workers workers-closed">
                    {lines.map((line) => (
                        <ClosedWorker key={line.session.id} line={line} />
                    ))}
                </ul>
            )}
        </>
    );
}

/** A closed session that worked for this one, which can be reopened. */
function ClosedWorker({ line }: { line: WorkerLine }) {
    const words = useWords();
    const open = useStore((state) => state.open);
    const refresh = useStore((state) => state.refresh);
    const report = useStore((state) => state.report);
    const { session, elsewhere } = line;
    const reopen = () =>
        api
            .sessionReopen(session.id)
            .then(() => refresh(api))
            .then(() => open(api, session.id))
            .catch(report);
    return (
        <li className="worker-row">
            <span className="worker-closed" title={session.title}>
                <WorkerParts session={session} task={elsewhere?.task.name ?? null} closed />
            </span>
            {elsewhere && <PullBadge project={elsewhere.project} task={elsewhere.task} />}
            {line.parent !== null && <ParentLink id={line.parent} />}
            <button type="button" className="closed-reopen" onClick={() => void reopen()}>
                {words.reopen}
            </button>
        </li>
    );
}
