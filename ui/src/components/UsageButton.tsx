import { type RefObject, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { AgentEvent, Session } from "../api/types";
import { accountWindows, limitLevel, limitWarning, warningLine } from "../rateLimit";
import { type Asked, askedLine, NO_FIGURES, usageFigures } from "../usageAsk";
import { useAccountUsage } from "../useAccountUsage";
import { useWords } from "../useWords";
import { CoinsIcon } from "./IconButton";
import { UsageSections } from "./UsageSections";

/**
 * The coins in a Claude Code session's header: they open an overview of the plan's usage and
 * ask the session's Claude Code for it as they do. Until then, and between askings, the
 * figures are the account's as any Claude Code session last heard them, the same in every
 * session.
 */
export function UsageButton({ session, events }: { session: Session; events: AgentEvent[] }) {
    const words = useWords();
    const [open, setOpen] = useState(false);
    const { asked, ask } = useAsking(session.id, events);
    const wrap = useRef<HTMLDivElement>(null);
    useDismiss(wrap, open, setOpen);

    const toggle = () => {
        if (!open && asked?.state !== "asking") {
            ask();
        }
        setOpen(!open);
    };
    // Past the warning, the coins say so before they are opened, until the window resets.
    const { usage, now } = useAccountUsage();
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
                <div className="usage-overview" role="dialog" aria-label={words.usage}>
                    <p className="usage-overview-title">
                        {words.usage}
                        <span>{askedLine(asked ?? { state: "asking" })}</span>
                    </p>
                    <UsageSections agent={session.agent} events={events} look="overview" />
                    <div className="usage-overview-foot">
                        <button
                            type="button"
                            className="button"
                            disabled={asked?.state === "asking"}
                            onClick={ask}
                        >
                            {words.askAgain}
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
}

/** How long an answer may take: Claude Code may have to start first. */
const ANSWER_WAIT_MS = 30_000;

/**
 * Asking the session's Claude Code for the usage, and where that stands. The call returns once
 * the question is sent; the answer is the next usage figure among the session's events.
 */
function useAsking(sessionId: number, events: AgentEvent[]) {
    const [asked, setAsked] = useState<Asked | null>(null);
    // How many figures there were when the question went out, while it waits for an answer.
    const [waitingFrom, setWaitingFrom] = useState<number | null>(null);
    const figures = usageFigures(events);
    useEffect(() => {
        if (waitingFrom !== null && figures > waitingFrom) {
            setWaitingFrom(null);
            setAsked({ state: "asked", at: new Date() });
        }
    }, [figures, waitingFrom]);
    useEffect(() => {
        if (waitingFrom === null) {
            return;
        }
        const timer = setTimeout(() => {
            setWaitingFrom(null);
            setAsked({ state: "failed", error: NO_FIGURES });
        }, ANSWER_WAIT_MS);
        return () => clearTimeout(timer);
    }, [waitingFrom]);
    const ask = () => {
        setAsked({ state: "asking" });
        setWaitingFrom(figures);
        api.sessionUsageRefresh(sessionId).catch((error: unknown) => {
            setWaitingFrom(null);
            setAsked({ state: "failed", error: String(error) });
        });
    };
    return { asked, ask };
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
