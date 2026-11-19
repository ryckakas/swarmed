import type { AgentEvent, FileChange, Session, StatusItem, Task } from "./api/types";

// The built-in pieces of the status line under the message box. Pure, so it can be tested.

export const STATUS_ITEM_LABELS: Record<StatusItem, string> = {
    branch: "Branch",
    changes: "Changes",
    model: "Model",
    cost: "Cost",
    agent: "Agent",
    worktree: "Worktree",
};

/** Every built-in piece, in the order they are offered. */
export const STATUS_ITEMS = Object.keys(STATUS_ITEM_LABELS) as StatusItem[];

/** What the status line is drawn from. `changes` is `null` until they have been read. */
export interface StatusSource {
    session: Session;
    task: Task;
    events: AgentEvent[];
    changes: FileChange[] | null;
}

export interface StatusPart {
    item: StatusItem;
    text: string;
}

/** The model the agent last said it runs on, else the one the session was set to. */
function model({ session, events }: StatusSource): string | null {
    const init = events.findLast((event) => event.type === "init");
    return (init?.type === "init" ? init.model : null) || session.model;
}

/** What the session has cost since its agent last started, as the agent last reported it. */
function cost({ events }: StatusSource): string | null {
    const done = events.findLast((event) => event.type === "turn_done" && event.cost_usd !== null);
    const amount = done?.type === "turn_done" ? done.cost_usd : null;
    return amount === null ? null : `$${amount.toFixed(2)}`;
}

function changes({ changes }: StatusSource): string | null {
    if (changes === null) {
        return null;
    }
    const sum = (pick: (change: FileChange) => number | null) =>
        changes.reduce((total, change) => total + (pick(change) ?? 0), 0);
    const files = changes.length === 1 ? "1 file" : `${changes.length} files`;
    return `${files} +${sum((c) => c.added)} −${sum((c) => c.removed)}`;
}

const TEXT: Record<StatusItem, (source: StatusSource) => string | null> = {
    branch: ({ task }) => task.branch,
    changes,
    model,
    cost,
    agent: ({ session }) => session.agent,
    worktree: ({ task }) => task.worktree_path,
};

/** The chosen pieces that have something to show, in the order chosen. */
export function statusParts(items: StatusItem[], source: StatusSource): StatusPart[] {
    return items.flatMap((item) => {
        const text = TEXT[item]?.(source) ?? null;
        return text === null || text === "" ? [] : [{ item, text }];
    });
}

/** `items` with `item` switched on, at the end, or off. */
export function withItem(items: StatusItem[], item: StatusItem, on: boolean): StatusItem[] {
    const rest = items.filter((existing) => existing !== item);
    return on ? [...rest, item] : rest;
}
