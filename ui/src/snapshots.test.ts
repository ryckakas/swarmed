import { describe, expect, it } from "vitest";
import type { Snapshot, Task } from "./api/types";
import { snapshotRows } from "./snapshots";

const task = (name: string, extra: Partial<Task>): Task =>
    ({ name, archived: false, setup: "ok", environment: null, ...extra }) as Task;

describe("snapshotRows", () => {
    it("has a row for the tasks without an environment and one for each environment", () => {
        const kept: Snapshot = { environment: "docker", task: "one", taken_at: 1, entries: 2 };
        const tasks = [
            task("plain", {}),
            task("one", { environment: "docker" }),
            task("failed", { environment: "docker", setup: "failed" }),
            task("gone", { environment: "docker", archived: true }),
        ];
        expect(snapshotRows(["docker", "devenv"], [kept], tasks)).toEqual([
            { environment: null, snapshot: null, sources: ["plain"] },
            { environment: "docker", snapshot: kept, sources: ["one"] },
            { environment: "devenv", snapshot: null, sources: [] },
        ]);
    });
});
