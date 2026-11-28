import { describe, expect, it } from "vitest";
import type { Snapshot, Task } from "./api/types";
import { snapshotHint, snapshotRows } from "./snapshots";

const task = (name: string, extra: Partial<Task>): Task =>
    ({ name, archived: false, setup: "ok", environment: null, ...extra }) as Task;

describe("snapshotRows", () => {
    it("has a row for the tasks without an environment and one for each environment", () => {
        const kept: Snapshot = {
            environment: "docker",
            task: "one",
            taken_at: 1,
            entries: 2,
            behind: false,
        };
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

describe("snapshotHint", () => {
    const kept: Snapshot = {
        environment: null,
        task: "one",
        taken_at: 1,
        entries: 2,
        behind: false,
    };

    it("says whom a snapshot is from", () => {
        expect(snapshotHint(kept, "task")).toMatch(/^From one, /);
        expect(snapshotHint(kept, "task")).not.toContain("Behind");
    });

    it("says when the base has other dependencies, and what comes of it", () => {
        const said = snapshotHint({ ...kept, behind: true }, "worktree");
        expect(said).toMatch(/^From one, .*\. Behind the base branch: /);
        expect(said).toContain("the next worktree started from it is kept in its place");
    });
});
