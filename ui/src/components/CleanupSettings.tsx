import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Cleanup, CleanupItem, CleanupKind } from "../api/types";
import {
    CLEANUP_KINDS,
    deleteLabel,
    initialTicks,
    itemDetail,
    itemTitle,
    keptNote,
    kindSummary,
    kindWording,
    tickedKeys,
    toggledTick,
} from "../cleanup";
import { useStore } from "../store";
import { useWords } from "../useWords";

/**
 * What removed tasks left behind, and the snapshots: each kind with how many and their size,
 * and a button that lists exactly what it deletes and asks first. `onCleaned` is told after
 * every clean-up, so the figures above can be measured again.
 */
export function CleanupSettings({ onCleaned }: { onCleaned(): void }) {
    const words = useWords();
    const report = useStore((state) => state.report);
    const [cleanup, setCleanup] = useState<Cleanup | null>(null);
    const [asked, setAsked] = useState<CleanupKind | null>(null);
    const look = useCallback(() => {
        api.cleanup().then(setCleanup).catch(report);
    }, [report]);
    useEffect(look, [look]);
    const cleaned = () => {
        look();
        onCleaned();
    };

    return (
        <section className="settings-group" aria-label="Clean up">
            <h3>Clean up</h3>
            <p className="settings-hint">
                What removed {words.terms.tasks} left behind, and {words.snapshots.toLowerCase()}.
                Each button lists what it deletes and asks first. Nothing of a current{" "}
                {words.terms.task}, no {words.terms.branch} on a remote and no{" "}
                {words.terms.repository}'s own checkout is ever listed.
            </p>
            {cleanup === null ? (
                <p className="settings-hint">Looking…</p>
            ) : (
                <dl className="usage">
                    {CLEANUP_KINDS.map((kind) => (
                        <KindRow
                            key={kind}
                            kind={kind}
                            items={cleanup[kind]}
                            onAsk={() => setAsked(kind)}
                        />
                    ))}
                </dl>
            )}
            {asked && cleanup && (
                <CleanupDialog
                    kind={asked}
                    items={cleanup[asked]}
                    onClose={() => setAsked(null)}
                    onCleaned={cleaned}
                />
            )}
        </section>
    );
}

interface KindRowProps {
    kind: CleanupKind;
    items: CleanupItem[];
    onAsk(): void;
}

/** One kind: its name, how many and their size, and the button that asks to delete them. */
function KindRow({ kind, items, onAsk }: KindRowProps) {
    const words = useWords();
    const { label, detail } = kindWording(kind, words);
    return (
        <div className="usage-row cleanup-row">
            <dt>
                <span className="usage-name">{label}</span>
                <span className="usage-detail">{detail}</span>
            </dt>
            <dd>
                {kindSummary(items)}
                <button
                    type="button"
                    className="button"
                    disabled={items.length === 0}
                    aria-label={`Clean up: ${label}`}
                    onClick={onAsk}
                >
                    Clean up…
                </button>
            </dd>
        </div>
    );
}

interface DialogProps {
    kind: CleanupKind;
    /** The kind's items as last listed: after a clean-up, what is left of them. */
    items: CleanupItem[];
    onClose(): void;
    onCleaned(): void;
}

/**
 * Lists what a kind's clean-up deletes and asks. Everything that loses no work is ticked;
 * what would lose work found nowhere else is listed apart, unticked, and goes only if each is
 * ticked on its own. What could not be deleted is said here, and the dialog stays.
 */
function CleanupDialog({ kind, items, onClose, onCleaned }: DialogProps) {
    const words = useWords();
    const dialog = useRef<HTMLDialogElement>(null);
    const [ticks, setTicks] = useState(() => initialTicks(items));
    const [busy, setBusy] = useState(false);
    const [problem, setProblem] = useState<string | null>(null);
    const { title, what } = kindWording(kind, words);
    const chosen = tickedKeys(items, ticks);
    const safe = items.filter((item) => !item.at_risk);
    const atRisk = items.filter((item) => item.at_risk);
    const toggle = (key: string) => setTicks((now) => toggledTick(now, key));

    useEffect(() => {
        dialog.current?.showModal();
    }, []);

    const clean = async () => {
        setBusy(true);
        setProblem(null);
        try {
            const done = await api.cleanupClean(kind, chosen);
            onCleaned();
            const note = keptNote(done);
            if (note === null) {
                onClose();
            } else {
                setProblem(note);
            }
        } catch (error) {
            setProblem(String(error).replace(/^Error: /, ""));
        } finally {
            setBusy(false);
        }
    };

    return (
        <dialog
            ref={dialog}
            className="confirm cleanup"
            aria-labelledby="cleanup-title"
            aria-busy={busy}
            onClose={(event) => {
                // The settings around it are a dialog too, which must stay open.
                event.stopPropagation();
                onClose();
            }}
        >
            <h2 id="cleanup-title">{title}</h2>
            <p>{what}</p>
            <div className="cleanup-body">
                <Items kind={kind} items={safe} ticks={ticks} onToggle={toggle} />
                {atRisk.length > 0 && (
                    <div className="cleanup-risk">
                        <h3>Would lose work found nowhere else</h3>
                        <p>Each is deleted only if you tick it.</p>
                        <Items kind={kind} items={atRisk} ticks={ticks} onToggle={toggle} />
                    </div>
                )}
            </div>
            {problem && <p className="note-problem">{problem}</p>}
            {/* Opening the dialog focuses its first button, so a stray Enter deletes nothing. */}
            <div className="confirm-actions">
                <button type="button" className="button" onClick={onClose}>
                    Close
                </button>
                <button
                    type="button"
                    className="button button-danger"
                    disabled={busy || chosen.length === 0}
                    onClick={() => void clean()}
                >
                    {deleteLabel(chosen.length, busy)}
                </button>
            </div>
        </dialog>
    );
}

interface ItemsProps {
    kind: CleanupKind;
    items: CleanupItem[];
    ticks: string[];
    onToggle(key: string): void;
}

/** Items to tick, each with what it is, why it is there, since when and its size. */
function Items({ kind, items, ticks, onToggle }: ItemsProps) {
    const now = Date.now() / 1000;
    if (items.length === 0) {
        return null;
    }
    return (
        <ul className="cleanup-items">
            {items.map((item) => (
                <li key={item.key}>
                    <label className="cleanup-item">
                        <input
                            type="checkbox"
                            checked={ticks.includes(item.key)}
                            onChange={() => onToggle(item.key)}
                        />
                        <span className="cleanup-what">
                            <span className="cleanup-name" title={item.name}>
                                {itemTitle(kind, item)}
                            </span>
                            <span className="cleanup-detail">{itemDetail(kind, item, now)}</span>
                        </span>
                    </label>
                </li>
            ))}
        </ul>
    );
}
