import type { CSSProperties } from "react";
import type { AgentEvent } from "../api/types";
import {
    type ExtraUsage,
    latestExtraUsage,
    money,
    onExtraUsageNow,
    spentShare,
} from "../extraUsage";
import { type LimitWindow, limitWindows, resetWhen } from "../rateLimit";
import { type SpendBasis, sessionSpend, spendBasis } from "../spend";
import { useWords } from "../useWords";

/**
 * How usage is drawn: in the context card, the figures with a thin bar under each; in the
 * usage overview, larger bars that carry their own figures.
 */
export type UsageLook = "card" | "overview";

/** The plan's usage limits, the extra usage and what the session spent, those there are. */
export function UsageSections({ events, look }: { events: AgentEvent[]; look: UsageLook }) {
    return (
        <>
            <Limits windows={limitWindows(events)} look={look} />
            <Extra usage={latestExtraUsage(events)} inUse={onExtraUsageNow(events)} look={look} />
            <Spend spent={sessionSpend(events)} basis={spendBasis(events)} />
        </>
    );
}

/** A window this full is drawn as nearly used up. */
const LIMIT_NEARLY_REACHED = 0.8;

function isNear(window: LimitWindow): boolean {
    return window.used >= LIMIT_NEARLY_REACHED || window.status !== "allowed";
}

/** Claude Code's usage windows: how much of each is used, and when it resets. */
function Limits({ windows, look }: { windows: LimitWindow[]; look: UsageLook }) {
    const words = useWords();
    if (windows.length === 0) {
        return null;
    }
    const now = new Date();
    const Window = look === "card" ? CardWindow : OverviewWindow;
    return (
        <section className="context-section" aria-label={words.usageLimits}>
            <p className="context-title">{words.usageLimits}</p>
            <dl className="context-limits">
                {windows.map((window) => (
                    <Window key={window.name} window={window} now={now} />
                ))}
            </dl>
        </section>
    );
}

/** A window in the context card: its name and share, a thin bar, and when it resets. */
function CardWindow({ window, now }: { window: LimitWindow; now: Date }) {
    const percent = Math.round(window.used * 100);
    return (
        <div className="context-limit" data-near={isNear(window)}>
            <dt>{capitalized(window.name)}</dt>
            <dd>{percent}%</dd>
            <dd className="context-limit-bar" aria-hidden="true">
                <div
                    className="context-limit-fill"
                    style={{ width: `${Math.min(percent, 100)}%` }}
                />
            </dd>
            {window.resetsAt !== null && (
                <dd className="context-reset">Resets {resetWhen(window.resetsAt, now)}</dd>
            )}
        </div>
    );
}

/** A window in the overview: its name and when it resets, over a bar with its share inside. */
function OverviewWindow({ window, now }: { window: LimitWindow; now: Date }) {
    const percent = Math.round(window.used * 100);
    return (
        <div className="usage-window">
            <dt>{capitalized(window.name)}</dt>
            {window.resetsAt !== null && (
                <dd className="usage-reset">Resets {resetWhen(window.resetsAt, now)}</dd>
            )}
            <dd className="usage-window-meter">
                <Meter share={window.used} near={isNear(window)} label={`${percent}%`} />
            </dd>
        </div>
    );
}

/** A share this wide holds its figure inside its bar; a narrower one has it just past its end. */
const LABEL_FITS = 0.22;

/** A bar filled as far as `share`, from 0 to 1, with `label` on it. */
function Meter({
    share,
    near,
    label,
    tone = "limit",
}: {
    share: number;
    near: boolean;
    label: string;
    tone?: "limit" | "extra";
}) {
    const filled = Math.min(Math.max(share, 0), 1);
    const style = { "--fill": `${filled * 100}%` } as CSSProperties;
    return (
        <div
            className="usage-meter"
            data-near={near}
            data-tone={tone}
            data-inside={filled >= LABEL_FITS}
            style={style}
        >
            <div className="usage-meter-fill" />
            <span className="usage-meter-label">{label}</span>
        </div>
    );
}

/**
 * The account's extra usage this billing period: what is spent against the monthly cap, and
 * whether this session's turns run on it, billed. Only Claude Code reports it.
 */
function Extra({
    usage,
    inUse,
    look,
}: {
    usage: ExtraUsage | null;
    inUse: boolean;
    look: UsageLook;
}) {
    const words = useWords();
    if (usage === null && !inUse) {
        return null;
    }
    const share = usage && spentShare(usage);
    const note = extraNote(usage, inUse);
    return (
        <section className="context-section" aria-label={words.extraUsage}>
            <p className="context-title">
                {words.extraUsage}
                {usage && <span>{spentThisMonth(usage)}</span>}
            </p>
            {share != null && look === "card" && (
                <div className="context-bar context-bar-extra">
                    <div style={{ width: `${share}%` }} />
                </div>
            )}
            {share != null && look === "overview" && (
                <Meter
                    share={share / 100}
                    near={usage?.limit_reached === true}
                    label={`${share}%`}
                    tone="extra"
                />
            )}
            {note && (
                <p className="context-extra-note" data-tone={note.tone}>
                    {note.text}
                </p>
            )}
        </section>
    );
}

function spentThisMonth(usage: ExtraUsage): string {
    const spent = money(usage.used, usage.currency, usage.decimals);
    if (usage.limit === null) {
        return `${spent} this month, with no cap`;
    }
    return `${spent} of ${money(usage.limit, usage.currency, usage.decimals)} this month`;
}

/** What to say under the figure: the most pressing of a cap reached, use now, and off. */
function extraNote(
    usage: ExtraUsage | null,
    inUse: boolean,
): { text: string; tone: "warning" | "plain" } | null {
    if (usage?.limit_reached) {
        return { text: "The cap is reached: extra usage pays for nothing more.", tone: "warning" };
    }
    if (inUse) {
        return { text: "This session runs on it now: its turns are billed.", tone: "warning" };
    }
    if (usage && !usage.enabled) {
        return { text: "Off: requests stop at the plan's limits.", tone: "plain" };
    }
    return null;
}

/** What each kind of spend is, said after the figure. */
const SPEND_BASIS: Record<SpendBasis, string> = {
    subscription: ", at API prices: your subscription covers it",
    api: ", billed to the API key it runs on",
    agent: ", as the agent reports it",
};

/** What the session has spent in all, across every run of its agent. */
function Spend({ spent, basis }: { spent: number | null; basis: SpendBasis | null }) {
    const words = useWords();
    if (spent === null) {
        return null;
    }
    return (
        <section className="context-section" aria-label={words.spentHere}>
            <p className="context-title">
                {words.spentHere}
                <span>
                    ${spent.toFixed(2)}
                    {basis && SPEND_BASIS[basis]}
                </span>
            </p>
        </section>
    );
}

function capitalized(text: string): string {
    return text.charAt(0).toUpperCase() + text.slice(1);
}
