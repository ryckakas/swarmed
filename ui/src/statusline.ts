import type { AgentEvent, FileChange, Session, StatusItem, Task } from "./api/types";
import { formatDuration, timeWorked } from "./turns";

// The built-in pieces of the status line under the message box. Pure, so it can be tested.

export const STATUS_ITEM_LABELS: Record<StatusItem, string> = {
    branch: "Branch",
    changes: "Changes",
    model: "Model",
    context: "Context window",
    cost: "Cost",
    agent: "Agent",
    worktree: "Worktree",
    duration: "Time worked",
};

/** Every built-in piece, in the order they are offered. */
export const STATUS_ITEMS = Object.keys(STATUS_ITEM_LABELS) as StatusItem[];

/** What the status line is drawn from. `changes` is `null` until they have been read. */
export interface StatusSource {
    session: Session;
    task: Task;
    events: AgentEvent[];
    changes: FileChange[] | null;
    /** How long the turn that is running has run so far, in milliseconds, if that is known. */
    running?: number | null;
}

/** How a run of text in the status line is coloured: by what it says, not by its piece. */
export type StatusTone = "plain" | "added" | "removed" | "quiet";

export interface StatusPart {
    item: StatusItem;
    /** The piece as one string. */
    text: string;
    /** The same text in runs, where parts of it have a colour of their own. */
    runs: [text: string, tone: StatusTone][];
}

/** What the agent last said it actually runs with, if it has said. */
function applied(events: AgentEvent[]) {
    const latest = events.findLast((event) => event.type === "applied");
    return latest?.type === "applied" ? latest : null;
}

/** The effort the agent last said it runs with, if it has said. */
export function appliedEffort(events: AgentEvent[] | undefined): string | null {
    return applied(events ?? [])?.effort ?? null;
}

/** The model the agent last said it runs on, else the one the session was set to. */
function model({ session, events }: StatusSource): string | null {
    const init = events.findLast((event) => event.type === "init");
    const reported = applied(events)?.model ?? (init?.type === "init" ? init.model : null);
    return reported || session.model;
}

/**
 * The model, then the effort it thinks with: `opus | high`. The effort is the one chosen for
 * the session, else the one the agent reports its default comes to. An agent that says nothing
 * of effort shows its model alone.
 */
function modelRuns(source: StatusSource): StatusPart["runs"] | null {
    const name = model(source);
    const effort = source.session.effort ?? applied(source.events)?.effort;
    if (!(name && effort)) {
        return plain(name);
    }
    return [
        [name, "plain"],
        [" | ", "quiet"],
        [effort, "plain"],
    ];
}

export type ContextUsage = Extract<AgentEvent, { type: "context_usage" }>;

/** How full the context window was when the agent last said. */
export function contextUsage(events: AgentEvent[]): ContextUsage | null {
    const latest = events.findLast((event) => event.type === "context_usage");
    return latest?.type === "context_usage" ? latest : null;
}

/** A token count as people say it: 937, 29.6k, 1M. */
export function formatTokens(tokens: number): string {
    const short = (value: number, unit: string) =>
        `${value >= 100 ? Math.round(value) : Number(value.toFixed(1))}${unit}`;
    if (tokens >= 1_000_000) {
        return short(tokens / 1_000_000, "M");
    }
    return tokens >= 1000 ? short(tokens / 1000, "k") : String(tokens);
}

/** How much of the window is in use, as a whole percentage. */
export function contextPercent(usage: ContextUsage): number {
    return usage.max > 0 ? Math.round((usage.used / usage.max) * 100) : 0;
}

/** From this share on the window is close to full and the badge says so by its colour. */
export const CONTEXT_NEARLY_FULL = 80;

function context({ events }: StatusSource): StatusPart["runs"] | null {
    const usage = contextUsage(events);
    if (usage === null) {
        return null;
    }
    const percent = contextPercent(usage);
    const tone: StatusTone = percent >= CONTEXT_NEARLY_FULL ? "removed" : "plain";
    return [
        [`${percent}%`, tone],
        [` of ${formatTokens(usage.max)}`, "plain"],
    ];
}

/**
 * The parts of the window worth a line each: what is in use, largest first, then what is set
 * aside, then what is free. Parts that are listed but not loaded come last.
 */
export function contextBreakdown(usage: ContextUsage): ContextUsage["categories"] {
    const order = ["used", "buffer", "free", "deferred"];
    const rank = (kind: string) => (order.includes(kind) ? order.indexOf(kind) : 0);
    return usage.categories
        .filter((category) => category.tokens > 0)
        .toSorted((a, b) => rank(a.kind) - rank(b.kind) || b.tokens - a.tokens);
}

/** What the session has cost since its agent last started, as the agent last reported it. */
function cost({ events }: StatusSource): string | null {
    const done = events.findLast((event) => event.type === "turn_done" && event.cost_usd !== null);
    const amount = done?.type === "turn_done" ? done.cost_usd : null;
    return amount === null ? null : `$${amount.toFixed(2)}`;
}

/** The task's changes in runs: the file count, then what was added and what was removed. */
function changeRuns({ changes }: StatusSource): StatusPart["runs"] | null {
    if (changes === null) {
        return null;
    }
    const sum = (pick: (change: FileChange) => number | null) =>
        changes.reduce((total, change) => total + (pick(change) ?? 0), 0);
    const files = changes.length === 1 ? "1 file" : `${changes.length} files`;
    return [
        [`${files} `, "plain"],
        [`+${sum((c) => c.added)}`, "added"],
        [" ", "plain"],
        [`−${sum((c) => c.removed)}`, "removed"],
    ];
}

/** How long the agent has worked in this session: its turns added up, the running one too. */
function worked({ events, running }: StatusSource): string | null {
    const total = timeWorked(events) + (running ?? 0);
    return total > 0 ? formatDuration(total) : null;
}

const plain = (text: string | null): StatusPart["runs"] | null =>
    text === null || text === "" ? null : [[text, "plain"]];

const RUNS: Record<StatusItem, (source: StatusSource) => StatusPart["runs"] | null> = {
    branch: ({ task }) => plain(task.branch),
    changes: changeRuns,
    model: modelRuns,
    context,
    cost: (source) => plain(cost(source)),
    agent: ({ session }) => plain(session.agent),
    worktree: ({ task }) => plain(task.worktree_path),
    duration: (source) => plain(worked(source)),
};

/** The chosen pieces that have something to show, in the order chosen. */
export function statusParts(items: StatusItem[], source: StatusSource): StatusPart[] {
    return items.flatMap((item) => {
        const runs = RUNS[item]?.(source) ?? null;
        const text = runs?.map(([run]) => run).join("") ?? "";
        return runs === null ? [] : [{ item, text, runs }];
    });
}

/** `items` with `item` moved one place earlier (`by` -1) or later (`by` 1), if there is room. */
export function movedItem(items: StatusItem[], item: StatusItem, by: -1 | 1): StatusItem[] {
    const from = items.indexOf(item);
    const to = from + by;
    if (from === -1 || to < 0 || to >= items.length) {
        return items;
    }
    const moved = [...items];
    [moved[from], moved[to]] = [moved[to] as StatusItem, moved[from] as StatusItem];
    return moved;
}

/** `items` with `item` switched on, at the end, or off. */
export function withItem(items: StatusItem[], item: StatusItem, on: boolean): StatusItem[] {
    const rest = items.filter((existing) => existing !== item);
    return on ? [...rest, item] : rest;
}
