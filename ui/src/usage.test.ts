import { describe, expect, it } from "vitest";
import type { Usage } from "./api/types";
import { formatBytes, formatCpu, formatUsd, totalBytes } from "./usage";

describe("formatBytes", () => {
    it("says a size in the unit that fits it", () => {
        expect(formatBytes(0)).toBe("0 B");
        expect(formatBytes(512)).toBe("512 B");
        expect(formatBytes(4_100)).toBe("4.1 kB");
        expect(formatBytes(365_600_000)).toBe("366 MB");
        expect(formatBytes(1_230_000_000)).toBe("1.2 GB");
    });
});

describe("totalBytes", () => {
    it("adds up the database, the notes and the worktrees that were measured", () => {
        const worktree = (bytes: number | null) => ({ project: "p", task: "t", path: "/w", bytes });
        const usage: Usage = {
            database_bytes: 1000,
            events: 3,
            notes_bytes: 10,
            worktrees: [worktree(500), worktree(null)],
            running: { processes: 1, memory_bytes: 0, cpu_percent: 0 },
            sessions: [],
        };
        expect(totalBytes(usage)).toBe(1510);
    });
});

describe("formatUsd", () => {
    it("shows cents for what is small and whole dollars for what is large", () => {
        expect(formatUsd(0)).toBe("$0.00");
        expect(formatUsd(0.004)).toBe("under $0.01");
        expect(formatUsd(3.416)).toBe("$3.42");
        expect(formatUsd(1234.5)).toBe("$1,235");
    });
});

describe("formatCpu", () => {
    it("keeps a decimal only where it says something", () => {
        expect(formatCpu(0.42)).toBe("0.4%");
        expect(formatCpu(12.6)).toBe("13%");
        expect(formatCpu(180)).toBe("180%");
    });
});
