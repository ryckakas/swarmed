import { describe, expect, it } from "vitest";
import {
    axisTop,
    byModel,
    columns,
    dayLabel,
    formatMetric,
    lastDays,
    OTHER,
    seriesOf,
} from "./analytics";
import type { TurnStats } from "./api/types";

const entry = (day: string, model: string, extra: Partial<TurnStats> = {}): TurnStats => ({
    day,
    project: "demo",
    task: "one",
    agent: "claude",
    model,
    turns: 1,
    failed: 0,
    usd: 1,
    duration_ms: 1000,
    ...extra,
});

describe("lastDays", () => {
    it("ends today and crosses a month's end", () => {
        expect(lastDays(new Date(2026, 10, 2), 4)).toEqual([
            "2026-10-30",
            "2026-10-31",
            "2026-11-01",
            "2026-11-02",
        ]);
    });
});

describe("seriesOf", () => {
    it("keeps the models in the order they first appear", () => {
        const stats = [entry("d1", "opus"), entry("d1", "haiku"), entry("d2", "opus")];
        expect(seriesOf(stats)).toEqual(["opus", "haiku"]);
    });

    it("draws the models beyond the first few together", () => {
        const stats = ["a", "b", "c", "d", "e", "f"].map((model) => entry("d1", model));
        expect(seriesOf(stats)).toEqual(["a", "b", "c", "d", OTHER]);
    });
});

describe("columns", () => {
    it("has a column for every day, empty ones too, and folds the rest into Other", () => {
        const stats = [
            entry("2026-11-19", "opus", { usd: 2 }),
            entry("2026-11-19", "haiku", { usd: 0.5 }),
            entry("2026-11-19", "rare", { usd: 0.25 }),
            entry("2026-11-01", "opus", { usd: 9 }),
        ];
        const shown = columns(stats, ["2026-11-18", "2026-11-19"], ["opus", "haiku", OTHER], "usd");
        expect(shown).toEqual([
            { day: "2026-11-18", values: [0, 0, 0], total: 0 },
            { day: "2026-11-19", values: [2, 0.5, 0.25], total: 2.75 },
        ]);
    });
});

describe("byModel", () => {
    it("adds up the days shown for each model of each agent, the most expensive first", () => {
        const stats = [
            entry("d1", "haiku", { usd: 1, turns: 2 }),
            entry("d2", "haiku", { usd: 1, failed: 1 }),
            entry("d1", "opus", { usd: 5 }),
            entry("d0", "opus", { usd: 50 }),
        ];
        const rows = byModel(stats, ["d1", "d2"]);
        expect(rows.map((row) => [row.model, row.usd, row.turns, row.failed])).toEqual([
            ["opus", 5, 1, 0],
            ["haiku", 2, 3, 1],
        ]);
    });
});

describe("the axis and its labels", () => {
    it("tops the axis at a round number at or above the largest column", () => {
        expect(axisTop(0)).toBe(1);
        expect(axisTop(0.34)).toBe(0.5);
        expect(axisTop(7)).toBe(10);
        expect(axisTop(180)).toBe(200);
        expect(axisTop(250)).toBe(250);
    });

    it("names days and values as people read them", () => {
        expect(dayLabel("2026-11-05")).toBe("5 Nov");
        expect(formatMetric("usd", 1.5)).toBe("$1.50");
        expect(formatMetric("turns", 1200)).toBe("1,200");
        expect(formatMetric("duration_ms", 72_000)).toBe("1m 12s");
    });
});
