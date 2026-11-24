import type { AccountUsage, AgentEvent, UsageLimit, UsageWindow } from "./types";

// The account's usage for the sample agent, taken in from its reports as the core takes in
// Claude Code's (`account_usage.rs`): each window keeps the newest figure any report gave it.

const seconds = () => Math.round(Date.now() / 1000);

/** What the core calls a window, by its kind. */
const NAMES: Record<string, string> = {
    five_hour: "5-hour",
    seven_day: "weekly",
    seven_day_overage_included: "weekly, extra usage included",
};

function windowName(kind: string): string {
    return NAMES[kind] ?? kind.replace(/^seven_day_/, "weekly ").replaceAll("_", " ");
}

/** What the sample account stood at when the window opened: heard after a turn a while ago. */
export function sampleAccountUsage(): AccountUsage {
    const now = seconds();
    const heard = now - 25 * 60;
    const window = (kind: string, used: number, resetsAt: number): UsageWindow => ({
        kind,
        name: windowName(kind),
        used,
        resets_at: resetsAt,
        status: "allowed",
        heard_at: heard,
        asked: false,
    });
    return {
        windows: [
            window("five_hour", 0.12, now + 2 * 3600),
            window("seven_day", 0.27, now + 6 * 86_400),
            window("seven_day_overage_included", 0.4, now + 6 * 86_400),
        ],
        extra: null,
        billed: { on: false, resets_at: now + 2 * 3600, heard_at: heard },
    };
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null;
}

/** The windows a report after a turn lists. */
function reported(raw: unknown, at: number): UsageWindow[] {
    const windows = isRecord(raw) ? raw.unifiedWindows : null;
    if (!isRecord(windows)) {
        return [];
    }
    return Object.entries(windows).flatMap(([kind, window]): UsageWindow[] =>
        isRecord(window) && typeof window.utilization === "number"
            ? [
                  {
                      kind,
                      name: windowName(kind),
                      used: window.utilization,
                      resets_at: typeof window.resetsAt === "number" ? window.resetsAt : null,
                      status: typeof window.status === "string" ? window.status : "allowed",
                      heard_at: at,
                      asked: false,
                  },
              ]
            : [],
    );
}

/** A limit from an answer to asking, as a window of the kind a report after a turn names. */
function asked(limit: UsageLimit, at: number): UsageWindow {
    const kinds: Record<string, string> = { session: "five_hour", weekly_all: "seven_day" };
    const scoped = limit.kind === "weekly_scoped" && limit.model !== null;
    const kind = scoped
        ? `seven_day_${(limit.model ?? "").toLowerCase().replace(/[^a-z0-9]/g, "_")}`
        : (kinds[limit.kind] ?? limit.kind);
    const resets = limit.resets_at === null ? Number.NaN : Date.parse(limit.resets_at) / 1000;
    return {
        kind,
        name: scoped ? `weekly ${limit.model}` : windowName(kind),
        used: limit.percent / 100,
        resets_at: Number.isNaN(resets) ? null : Math.round(resets),
        status: limit.severity === "normal" ? "allowed" : limit.severity,
        heard_at: at,
        asked: true,
    };
}

/** Puts `windows` in place of those of their kind; an answer drops what an earlier one gave. */
function take(usage: AccountUsage, windows: UsageWindow[], fromAnswer: boolean): AccountUsage {
    const listed = new Set(windows.map((window) => window.kind));
    const kept = usage.windows.filter(
        (window) => !(listed.has(window.kind) || (fromAnswer && window.asked)),
    );
    const all = [...kept, ...windows].sort((a, b) => (a.kind < b.kind ? -1 : 1));
    return { ...usage, windows: all };
}

/** The usage after a report of the sample agent's; `null` for an event that is none. */
export function heardUsage(usage: AccountUsage, event: AgentEvent): AccountUsage | null {
    const at = seconds();
    if (event.type === "rate_limit") {
        const raw = isRecord(event.raw) ? event.raw : {};
        const on = raw.isUsingOverage === true || raw.overageInUse === true;
        const billed = { on, resets_at: event.resets_at, heard_at: at };
        return take({ ...usage, billed }, reported(event.raw, at), false);
    }
    if (event.type === "usage_limits") {
        const windows = event.limits.map((limit) => asked(limit, at));
        return take(usage, windows, true);
    }
    if (event.type === "extra_usage") {
        const { type: _, ...figures } = event;
        return { ...usage, extra: { ...figures, heard_at: at } };
    }
    return null;
}
