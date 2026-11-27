import { type CSSProperties, useEffect, useState } from "react";
import type { Descendant } from "../api/types";
import { atWorkSaid, type WorkerRow } from "../closing";
import { useWords } from "../useWords";
import { StatusMark, useStatusLabel } from "./StatusMark";

/** How far down the list draws a session nested; those deeper are drawn as deep as that. */
const DRAWN_DEPTH = 4;

/**
 * What the core lists of session `id` or a task, by `ask`, once it has answered: those the
 * dialog offers to close. Nothing while it is asked, and nothing if asking failed, so that the
 * dialog still closes the one asked about alone.
 */
export function useAsked(key: string, ask: () => Promise<Descendant[]>): Descendant[] {
    const [found, setFound] = useState<{ key: string; under: Descendant[] } | null>(null);
    // biome-ignore lint/correctness/useExhaustiveDependencies: asked again for another key only
    useEffect(() => {
        let current = true;
        const keep = (under: Descendant[]) => {
            if (current) {
                setFound({ key, under });
            }
        };
        ask().then(keep, () => keep([]));
        return () => {
            current = false;
        };
    }, [key]);
    return found?.key === key ? found.under : [];
}

interface Props {
    /** What stands over the list. */
    heading: string;
    rows: WorkerRow[];
    /** What the tick box says. */
    label: string;
    ticked: boolean;
    disabled: boolean;
    onTick(ticked: boolean): void;
}

/**
 * The sessions that closing would take along, each with its status and its task where that is
 * another, nested as they work for one another, and the tick box that takes them along,
 * unticked until the user ticks it. Those at work are named: closing them stops their agents in
 * the middle of a turn.
 */
export function WorkersToClose({ heading, rows, label, ticked, disabled, onTick }: Props) {
    const words = useWords();
    const atWork = atWorkSaid(rows, words);
    return (
        <div className="close-workers">
            <p className="close-workers-heading">{heading}</p>
            <ul className="close-workers-list">
                {rows.map((row) => (
                    <Worker key={row.session.id} row={row} />
                ))}
            </ul>
            <label className="close-workers-tick">
                <input
                    type="checkbox"
                    checked={ticked}
                    disabled={disabled}
                    onChange={(event) => onTick(event.target.checked)}
                />
                {label}
            </label>
            {atWork && <p className="close-workers-note">{atWork}</p>}
        </div>
    );
}

/** One session of the list: its status, its title, its task if another, and whether at work. */
function Worker({ row }: { row: WorkerRow }) {
    const words = useWords();
    const { session, depth, task, atWork } = row;
    const status = useStatusLabel(session.status, session.unread, session.background);
    const nested = Math.min(depth, DRAWN_DEPTH) - 1;
    return (
        <li className="close-workers-item" style={{ "--depth": nested } as CSSProperties}>
            <StatusMark
                status={session.status}
                unread={session.unread}
                background={session.background}
                label={status}
            />
            <span className="close-workers-title">{session.title || words.untitled}</span>
            {task !== null && <span className="close-workers-task">{task}</span>}
            {atWork && <span className="close-workers-status">{status}</span>}
        </li>
    );
}
