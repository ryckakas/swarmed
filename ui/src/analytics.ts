import type { TurnStats } from "./api/types";
import { formatDuration } from "./turns";
import { formatUsd } from "./usage";

// What the sessions did, shaped for drawing. Pure, so it can be tested.

/** What a chart measures. */
export type Metric = "usd" | "duration_ms" | "turns";

export const METRICS: [Metric, string][] = [
    ["usd", "Cost"],
    ["duration_ms", "Time"],
    ["turns", "Turns"],
];

/** How many models get a colour of their own; the rest are drawn together. */
export const MAX_SERIES = 5;

/** What the models beyond the first few are drawn and listed as. */
export const OTHER = "Other";

/** A value of a metric as people read it. */
export function formatMetric(metric: Metric, value: number): string {
    if (metric === "usd") {
        return formatUsd(value);
    }
    return metric === "turns" ? value.toLocaleString("en-US") : formatDuration(value);
}

/** A day as `YYYY-MM-DD` in local time, which is how the core names days. */
export function dayOf(date: Date): string {
    const part = (value: number) => String(value).padStart(2, "0");
    return `${date.getFullYear()}-${part(date.getMonth() + 1)}-${part(date.getDate())}`;
}

/** The last `count` days up to and including `today`, the oldest first. */
export function lastDays(today: Date, count: number): string[] {
    return Array.from({ length: count }, (_, index) => {
        const day = new Date(today.getFullYear(), today.getMonth(), today.getDate());
        day.setDate(day.getDate() - (count - 1 - index));
        return dayOf(day);
    });
}

/**
 * The models that get a colour, in the order they first appear in everything that was ever
 * counted. A model keeps its place, and so its colour, whatever range or metric is shown.
 */
export function seriesOf(stats: TurnStats[]): string[] {
    const seen = [...new Set(stats.map((entry) => entry.model))];
    return seen.length > MAX_SERIES ? [...seen.slice(0, MAX_SERIES - 1), OTHER] : seen;
}

/** The series a model is drawn in. */
function seriesIndex(series: string[], model: string): number {
    const own = series.indexOf(model);
    return own === -1 ? series.indexOf(OTHER) : own;
}

export interface DayColumn {
    day: string;
    /** One value for each series, in their order. */
    values: number[];
    total: number;
}

/** One column for each of `days`, with what each series adds to it. */
export function columns(
    stats: TurnStats[],
    days: string[],
    series: string[],
    metric: Metric,
): DayColumn[] {
    const byDay = new Map(days.map((day) => [day, series.map(() => 0)]));
    for (const entry of stats) {
        const values = byDay.get(entry.day);
        const at = seriesIndex(series, entry.model);
        if (values && at !== -1) {
            values[at] = (values[at] ?? 0) + entry[metric];
        }
    }
    return days.map((day) => {
        const values = byDay.get(day) ?? [];
        return { day, values, total: values.reduce((sum, value) => sum + value, 0) };
    });
}

export interface ModelRow {
    model: string;
    agent: string;
    turns: number;
    failed: number;
    usd: number;
    duration_ms: number;
}

/** The days' figures added up for each model of each agent, the most expensive first. */
export function byModel(stats: TurnStats[], days: string[]): ModelRow[] {
    const within = new Set(days);
    const rows = new Map<string, ModelRow>();
    for (const entry of stats.filter((one) => within.has(one.day))) {
        const key = `${entry.agent}\n${entry.model}`;
        const row = rows.get(key) ?? {
            model: entry.model,
            agent: entry.agent,
            turns: 0,
            failed: 0,
            usd: 0,
            duration_ms: 0,
        };
        row.turns += entry.turns;
        row.failed += entry.failed;
        row.usd += entry.usd;
        row.duration_ms += entry.duration_ms;
        rows.set(key, row);
    }
    return [...rows.values()].sort((a, b) => b.usd - a.usd || b.turns - a.turns);
}

/** A round number at or above `max` for the top of the axis, so its ticks read easily. */
export function axisTop(max: number): number {
    if (max <= 0) {
        return 1;
    }
    const magnitude = 10 ** Math.floor(Math.log10(max));
    const step = [1, 2, 2.5, 5, 10].find((candidate) => candidate * magnitude >= max) ?? 10;
    return step * magnitude;
}

/** A day as an axis label: `20 Nov`. */
export function dayLabel(day: string): string {
    const [, month, date] = day.split("-").map(Number);
    const names = [
        "Jan",
        "Feb",
        "Mar",
        "Apr",
        "May",
        "Jun",
        "Jul",
        "Aug",
        "Sep",
        "Oct",
        "Nov",
        "Dec",
    ];
    return `${date} ${names[(month ?? 1) - 1] ?? ""}`;
}
