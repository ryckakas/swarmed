import type { CSSProperties } from "react";
import type { AgentEvent } from "../api/types";
import { type ExtraUsage, money, onExtraUsageNow, spentShare } from "../extraUsage";
import {
    accountWindows,
    asOf,
    type LimitLevel,
    type LimitWindow,
    limitLevel,
    newestHeard,
    type WindowNotes,
    windowNote,
    windowNotes,
} from "../rateLimit";
import { type SpendBasis, sessionSpend, spendBasis } from "../spend";
import { useAccountUsage } from "../useAccountUsage";
import { useWords } from "../useWords";
import type { Words } from "../wording";
import { CLAUDE } from "./options";

/**
 * How usage is drawn: in the context card, the figures with a thin bar under each; in the
 * usage overview, larger bars that carry their own figures.
 */
export type UsageLook = "card" | "overview";

/**
 * The plan's usage limits and the extra usage, those there are, and what the session spent.
 * The first two are the account's, as any Claude Code session last heard them, and shown only
 * for a session on `agent` Claude Code: no other agent reports them, and its account is
 * another.
 */
export function UsageSections({
    agent,
    events,
    look,
}: {
    agent: string;
    events: AgentEvent[];
    look: UsageLook;
}) {
    const account = useAccountUsage();
    const usage = agent === CLAUDE ? account.usage : null;
    return (
        <>
            <Limits windows={accountWindows(usage, account.now)} look={look} />
            <Extra
                usage={usage?.extra ?? null}
                inUse={onExtraUsageNow(usage, account.now)}
                look={look}
            />
            <Spend spent={sessionSpend(events)} basis={spendBasis(events)} />
        </>
    );
}

/**
 * Claude Code's usage windows: how much of each is used, when it resets, and how old the
 * figures are. One heard before the newest says its own age.
 */
function Limits({ windows, look }: { windows: LimitWindow[]; look: UsageLook }) {
    const words = useWords();
    const newest = newestHeard(windows);
    if (newest === null) {
        return null;
    }
    const now = new Date();
    const Window = look === "card" ? CardWindow : OverviewWindow;
    return (
        <section className="context-section" aria-label={words.usageLimits}>
            <p className="context-title">
                {words.usageLimits}
                <span>{capitalized(asOf(newest, now))}</span>
            </p>
            <dl className="context-limits">
                {windows.map((window) => (
                    <Window
                        key={window.name}
                        window={window}
                        notes={windowNotes(window, newest, now)}
                    />
                ))}
            </dl>
        </section>
    );
}

/** A window in the context card: its name and share, a thin bar, and when it resets. */
function CardWindow({ window, notes }: { window: LimitWindow; notes: WindowNotes }) {
    const percent = Math.round(window.used * 100);
    const note = windowNote(notes);
    return (
        <div className="context-limit" data-level={limitLevel(window)}>
            <dt>{capitalized(window.name)}</dt>
            <dd>{percent}%</dd>
            <dd className="context-limit-bar" aria-hidden="true">
                <div
                    className="context-limit-fill"
                    style={{ width: `${Math.min(percent, 100)}%` }}
                />
            </dd>
            {note && <dd className="context-reset">{note}</dd>}
        </div>
    );
}

/**
 * A window in the overview: its name and when it resets, over a bar with its share inside, and
 * under it how old its figure is, if older than the rest.
 */
function OverviewWindow({ window, notes }: { window: LimitWindow; notes: WindowNotes }) {
    const percent = Math.round(window.used * 100);
    return (
        <div className="usage-window">
            <dt>{capitalized(window.name)}</dt>
            {notes.resets && <dd className="usage-reset">{notes.resets}</dd>}
            <dd className="usage-window-meter">
                <Meter share={window.used} level={limitLevel(window)} label={`${percent}%`} />
            </dd>
            {notes.age && <dd className="usage-age">{notes.age}</dd>}
        </div>
    );
}

/** A share this wide holds its figure inside its bar; a narrower one has it just past its end. */
const LABEL_FITS = 0.22;

/**
 * A bar filled as far as `share`, from 0 to 1, with `label` on it: yellow past the warning, red
 * used up. Extra usage is plain grey, so that yellow always means a warning.
 */
function Meter({
    share,
    level,
    label,
    tone = "limit",
}: {
    share: number;
    level: LimitLevel;
    label: string;
    tone?: "limit" | "extra";
}) {
    const filled = Math.min(Math.max(share, 0), 1);
    const style = { "--fill": `${filled * 100}%` } as CSSProperties;
    return (
        <div
            className="usage-meter"
            data-level={level}
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
 * The account's extra usage this billing period: what is spent against the monthly cap, how
 * old that figure is, and whether turns run on it now, billed. Only Claude Code reports it.
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
                {usage && <span>{capitalized(asOf(usage.heard_at, new Date()))}</span>}
            </p>
            {share != null && look === "card" && (
                <div className="context-bar context-bar-extra">
                    <div style={{ width: `${share}%` }} />
                </div>
            )}
            {share != null && look === "overview" && (
                <Meter
                    share={share / 100}
                    level={usage?.limit_reached ? "reached" : "fine"}
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
        return { text: "Turns run on it now: they are billed.", tone: "warning" };
    }
    if (usage && !usage.enabled) {
        return { text: "Off: requests stop at the plan's limits.", tone: "plain" };
    }
    return null;
}

/** What each kind of spend is, said after the figure. */
const SPEND_BASIS: Record<SpendBasis, (words: Words) => string> = {
    subscription: () => ", at API prices: your subscription covers it",
    api: () => ", billed to the API key it runs on",
    agent: (words) => `, as the ${words.terms.agent} reports it`,
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
                    {basis && SPEND_BASIS[basis](words)}
                </span>
            </p>
        </section>
    );
}

function capitalized(text: string): string {
    return text.charAt(0).toUpperCase() + text.slice(1);
}
