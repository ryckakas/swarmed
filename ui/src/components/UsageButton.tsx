import { type RefObject, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { AccountUsage, AgentEvent, Session } from "../api/types";
import { accountWindows, limitLevel, limitWarning, warningLine } from "../rateLimit";
import {
    type Asked,
    answerHeard,
    answerWait,
    askedDetail,
    askedLine,
    figuresKey,
    NO_FIGURES,
    underWay,
    usageFigures,
    waitedShare,
} from "../usageAsk";
import { useAccountUsage } from "../useAccountUsage";
import { useWords } from "../useWords";
import { CoinsIcon } from "./IconButton";
import { UsageSections } from "./UsageSections";

/**
 * The coins in a Claude Code session's header: they open an overview of the plan's usage and
 * ask the session's Claude Code for it as they do. Until then, and between askings, the
 * figures are the account's as any Claude Code session last heard them, the same in every
 * session. While a question is under way the coins pulse, so that it shows with the overview
 * closed too.
 */
export function UsageButton({ session, events }: { session: Session; events: AgentEvent[] }) {
    const words = useWords();
    const [open, setOpen] = useState(false);
    // Past the warning, the coins say so before they are opened, until the window resets.
    const { usage, now } = useAccountUsage();
    const { asked, ask, before } = useAsking(session.id, events, usage);
    const wrap = useRef<HTMLDivElement>(null);
    useDismiss(wrap, open, setOpen);

    const toggle = () => {
        if (!(open || underWay(asked))) {
            ask();
        }
        setOpen(!open);
    };
    const warning = limitWarning(accountWindows(usage, now));
    const about = `${words.usage}: your plan's limits and ${words.extraUsage.toLowerCase()}, asked of Claude Code as it opens.`;
    const label = warning ? `${about} ${warningLine(warning)}` : about;
    return (
        <div className="usage-wrap" ref={wrap}>
            <button
                type="button"
                className="icon-button usage-coins"
                aria-label={label}
                title={label}
                aria-expanded={open}
                aria-haspopup="dialog"
                data-asking={underWay(asked)}
                onClick={toggle}
            >
                <svg viewBox="0 0 16 16" aria-hidden="true">
                    <CoinsIcon />
                </svg>
                {warning && (
                    <svg
                        className="usage-mark"
                        data-level={limitLevel(warning)}
                        viewBox="0 0 10 10"
                        aria-hidden="true"
                    >
                        <path d="M5 1.2 9.2 8.8H.8z" />
                    </svg>
                )}
            </button>
            {open && (
                <UsageOverview
                    session={session}
                    events={events}
                    asked={asked ?? { state: "asking" }}
                    same={before !== null && figuresKey(usage) === before}
                    onAsk={ask}
                />
            )}
        </div>
    );
}

/**
 * The overview the coins open: where asking stands, the figures, and a button to ask again.
 * Asking shows at every step: while the question is held back or goes, a pulsing dot, a bar
 * and a line that say it is refreshing, in how many seconds while held back, and that the
 * figures shown until then are the latest heard; once answered, the time to the second,
 * whether the figures are new or the same, and a moment's light on them. The figures are
 * drawn as they are throughout, never dimmed.
 */
function UsageOverview({
    session,
    events,
    asked,
    same,
    onAsk,
}: {
    session: Session;
    events: AgentEvent[];
    asked: Asked;
    same: boolean;
    onAsk: () => void;
}) {
    const words = useWords();
    const now = useClock(asked.state === "waiting");
    const busy = underWay(asked);
    const answered = asked.state === "asked" ? asked.at.getTime() : null;
    const detail = askedDetail(asked, now);
    return (
        <div className="usage-overview" role="dialog" aria-label={words.usage}>
            <p className="usage-overview-title">{words.usage}</p>
            <div className="usage-asked" data-busy={busy}>
                <p className="usage-asked-line">{askedLine(asked, now, same)}</p>
                {busy && <RefreshBar share={waitedShare(asked, now)} />}
                {detail && <p className="usage-asked-detail">{detail}</p>}
            </div>
            <div
                // A new answer draws the figures anew, which lights them up once.
                key={answered ?? "figures"}
                className="usage-overview-figures"
                data-fresh={answered !== null}
            >
                <UsageSections agent={session.agent} events={events} look="overview" />
            </div>
            <div className="usage-overview-foot">
                <button type="button" className="button" disabled={busy} onClick={onAsk}>
                    {busy ? words.asking : words.askAgain}
                </button>
            </div>
        </div>
    );
}

/**
 * A thin bar under the line that says the overview is refreshing: filled as far as the wait
 * for Claude Code's copy to run out has come (`share`), or moving to and fro while the question
 * is asked and there is no telling how long the answer takes (`null`).
 */
function RefreshBar({ share }: { share: number | null }) {
    const measured = share !== null;
    return (
        <div
            className="usage-refresh"
            role="progressbar"
            aria-label="Refreshing"
            aria-valuemin={measured ? 0 : undefined}
            aria-valuemax={measured ? 100 : undefined}
            aria-valuenow={measured ? Math.round(share * 100) : undefined}
            data-measured={measured}
        >
            <span
                className="usage-refresh-fill"
                style={measured ? { width: `${share * 100}%` } : undefined}
            />
        </div>
    );
}

/** The time, drawn again every second while `ticking`, as a countdown needs. */
function useClock(ticking: boolean): Date {
    const [now, setNow] = useState(() => new Date());
    useEffect(() => {
        if (!ticking) {
            return;
        }
        setNow(new Date());
        const timer = setInterval(() => setNow(new Date()), 1000);
        return () => clearInterval(timer);
    }, [ticking]);
    return ticking ? now : new Date();
}

/** How long an answer may take once the question goes: Claude Code may have to start first. */
const ANSWER_WAIT_MS = 30_000;

/**
 * Asking the session's Claude Code for the usage, and where that stands. The call returns once
 * the question is sent, or set to go once Claude Code's own copy of the figures is old enough
 * for it to fetch them again; the answer is the next usage figure among the session's events
 * from then on.
 */
function useAsking(sessionId: number, events: AgentEvent[], usage: AccountUsage | null) {
    const [asked, setAsked] = useState<Asked | null>(null);
    // The figures as they were when the question was asked, to say whether the answer changed them.
    const [before, setBefore] = useState<string | null>(null);
    // How many figures there were when the question went out, while it waits for an answer.
    const [waitingFrom, setWaitingFrom] = useState<number | null>(null);
    const figures = usageFigures(events);
    useEffect(() => {
        if (waitingFrom === null) {
            return;
        }
        const heard = answerHeard(asked, waitingFrom, figures, Date.now());
        if (heard === "early") {
            // Figures heard before the question went answer something else: its answer is next.
            setWaitingFrom(figures);
        } else if (heard === "answer") {
            setWaitingFrom(null);
            setAsked({ state: "asked", at: new Date() });
        }
    }, [asked, figures, waitingFrom]);
    useEffect(() => {
        if (waitingFrom === null) {
            return;
        }
        const timer = setTimeout(
            () => {
                setWaitingFrom(null);
                setAsked({ state: "failed", error: NO_FIGURES });
            },
            answerWait(asked, Date.now(), ANSWER_WAIT_MS),
        );
        return () => clearTimeout(timer);
    }, [asked, waitingFrom]);
    const ask = () => {
        setAsked({ state: "asking" });
        setBefore(figuresKey(usage));
        setWaitingFrom(figures);
        api.sessionUsageRefresh(sessionId).then(
            ({ waits_until, copy_fetched_at }) => {
                if (waits_until !== null && copy_fetched_at !== null) {
                    const goesAt = new Date(waits_until);
                    setAsked({ state: "waiting", goesAt, copyFrom: new Date(copy_fetched_at) });
                }
            },
            (error: unknown) => {
                setWaitingFrom(null);
                setAsked({ state: "failed", error: String(error) });
            },
        );
    };
    return { asked, ask, before };
}

/** Closes what is open when the user presses Escape or clicks anywhere outside `inside`. */
export function useDismiss(
    inside: RefObject<HTMLElement | null>,
    open: boolean,
    setOpen: (open: boolean) => void,
) {
    useEffect(() => {
        if (!open) {
            return;
        }
        const onPointer = (event: PointerEvent) => {
            if (!inside.current?.contains(event.target as Node)) {
                setOpen(false);
            }
        };
        const onKey = (event: KeyboardEvent) => {
            if (event.key === "Escape") {
                setOpen(false);
            }
        };
        document.addEventListener("pointerdown", onPointer);
        document.addEventListener("keydown", onKey);
        return () => {
            document.removeEventListener("pointerdown", onPointer);
            document.removeEventListener("keydown", onKey);
        };
    }, [inside, open, setOpen]);
}
