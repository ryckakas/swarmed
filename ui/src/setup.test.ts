import { describe, expect, it } from "vitest";
import type { SetupEnd, SetupProgress, Task } from "./api/types";
import {
    elapsed,
    headline,
    heard,
    KEPT_BYTES,
    lastLines,
    ranSince,
    readFrom,
    SETUP_GRACE,
    setupShown,
    setupState,
    sidebarSays,
    standing,
    whyNotDone,
} from "./setup";

function report(change: Partial<SetupProgress>): SetupProgress {
    const output = change.output ?? "";
    const from = change.from ?? 0;
    return {
        task_id: 1,
        step: "hook",
        began: 1000,
        output,
        from,
        to: from + new TextEncoder().encode(output).length,
        ended: null,
        ...change,
    };
}

type SetupOf = Pick<Task, "setup" | "setting_up" | "setup_reason">;

const running: SetupOf = {
    setup: "pending",
    setup_reason: null,
    setting_up: { state: "here", step: "hook", began: 1000, latest: "npm ci" },
};

describe("what is kept of a setup", () => {
    it("adds what each report printed where it goes on", () => {
        let seen = heard(undefined, report({ output: "one\n" }));
        seen = heard(seen, report({ output: "two\n", from: 4 }));
        expect(seen.text).toBe("one\ntwo\n");
        expect([seen.start, seen.end]).toEqual([0, 8]);
        // A report heard twice changes nothing.
        expect(heard(seen, report({ output: "two\n", from: 4 }))).toEqual(seen);
    });

    it("starts afresh for a new run, and keeps how it ended", () => {
        const old = heard(undefined, report({ output: "old\n" }));
        const again = heard(old, report({ began: 2000, output: "new\n" }));
        expect(again.text).toBe("new\n");
        const done = heard(again, report({ began: 2000, from: 4, ended: { outcome: "done" } }));
        expect(done.ended).toEqual({ outcome: "done" });
        expect(done.step).toBe("hook");
    });

    it("counts places in bytes, as the core does", () => {
        const seen = heard(undefined, report({ output: "café\n" }));
        expect(seen.end).toBe(6);
        // A report that overlaps what was kept adds only what is new.
        const more = heard(seen, report({ output: "é\nnext\n", from: 3 }));
        expect(more.text).toBe("café\nnext\n");
    });

    it("puts the log read and what was heard meanwhile together, without twice", () => {
        // Heard while the log was being read: the second line, and the third.
        const meanwhile = heard(undefined, report({ output: "two\nthree\n", from: 4 }));
        const read = readFrom(meanwhile, { text: "one\ntwo\n", end: 8 }, 1000);
        expect(read.text).toBe("one\ntwo\nthree\n");
        expect(read.end).toBe(14);
        // A log of another run, or of one that is over, takes the place of what was heard.
        const over = readFrom(meanwhile, { text: "all of it\n", end: 10 }, null);
        expect(over.text).toBe("all of it\n");
    });

    it("keeps only the end of a great deal, from the start of a line", () => {
        const line = `${"x".repeat(99)}\n`;
        const much = line.repeat(Math.ceil(KEPT_BYTES / 100) + 10);
        const seen = heard(undefined, report({ output: much }));
        expect(seen.text.length).toBeLessThanOrEqual(KEPT_BYTES);
        expect(seen.text.startsWith("x")).toBe(true);
        expect(seen.start + seen.text.length).toBe(seen.end);
    });
});

describe("how a setup is told", () => {
    it("says how long it has run", () => {
        expect(elapsed(0, 45_900)).toBe("45s");
        expect(elapsed(0, 125_000)).toBe("2m 05s");
        expect(lastLines("a\nb\nc\n", 2)).toBe("b\nc");
    });

    it("tells running, elsewhere, failed, interrupted and done apart", () => {
        expect(setupState(running)).toBe("running");
        expect(setupState({ setup: "failed", setting_up: { state: "elsewhere" } })).toBe(
            "elsewhere",
        );
        expect(setupState({ setup: "failed", setting_up: null })).toBe("failed");
        expect(setupState({ setup: "pending", setting_up: { state: "interrupted" } })).toBe(
            "interrupted",
        );
        expect(setupState({ setup: "ok", setting_up: null })).toBe("done");
    });

    it("says in the sidebar what a setup does, in place of nothing spawned", () => {
        const says = sidebarSays(running, undefined, 61_000);
        expect(says).toEqual({ busy: true, text: "Setting up… 1m 00s · npm ci" });
        const seen = heard(undefined, report({ output: "Linking\n" }));
        expect(sidebarSays(running, seen, 3000)?.text).toBe("Setting up… 2s · Linking");
        const waiting: SetupOf = {
            ...running,
            setting_up: { state: "here", step: "waiting", began: 1000, latest: null },
        };
        expect(sidebarSays(waiting, undefined, 1000)?.text).toContain("waiting for a place");
        const failed: SetupOf = { setup: "failed", setting_up: null, setup_reason: "killed" };
        expect(sidebarSays(failed, undefined, 0)).toEqual({ busy: false, text: "setup failed" });
        const byYou = { ...failed, setup_reason: "Stopped by you." };
        expect(sidebarSays(byYou, undefined, 0)?.text).toBe("setup stopped");
        const done: SetupOf = { setup: "ok", setting_up: null, setup_reason: null };
        expect(sidebarSays(done, undefined, 0)).toBeNull();
    });

    it("says why a setup is not done, and what stopped it", () => {
        const killed = { setup: "failed" as const, setting_up: null, setup_reason: "killed" };
        expect([headline(killed), whyNotDone(killed)]).toEqual(["Setup failed", "killed"]);
        const stopped = { ...killed, setup_reason: "Stopped by you." };
        expect(headline(stopped)).toBe("Setup was stopped");
        const cut = {
            setup: "pending" as const,
            setting_up: { state: "interrupted" as const },
            setup_reason: null,
        };
        expect(headline(cut)).toBe("Setup was interrupted");
        expect(whyNotDone(cut)).toContain("quit or crashed");
        expect(headline(running)).toBe("Running setup");
        // The step a report of the same run said goes before the tree's, read when it began.
        const later = heard(undefined, report({ step: "stack" }));
        expect(headline(running, later)).toBe("Starting the stack");
        const other = heard(undefined, report({ step: "stack", began: 5 }));
        expect(headline(running, other)).toBe("Running setup");
    });

    it("has what is typed into a session of a task being set up wait its turn", () => {
        expect(standing("idle", running)).toBe("working");
        expect(standing("idle", { setup: "ok", setting_up: null })).toBe("idle");
        expect(standing("waiting", undefined)).toBe("waiting");
    });
});

describe("whether a setup is shown", () => {
    const ended = (end: SetupEnd) => heard(undefined, report({ ended: end }));

    it("waits out the grace of one under way here, counted from when it began", () => {
        expect(setupShown(running, undefined, 1000)).toBe(false);
        expect(setupShown(running, undefined, 1000 + SETUP_GRACE - 1)).toBe(false);
        expect(setupShown(running, undefined, 1000 + SETUP_GRACE)).toBe(true);
        // A window that opens on one long under way shows it at once.
        expect(setupShown(running, undefined, 61_000)).toBe(true);
        // What was heard of it as it goes changes nothing.
        expect(setupShown(running, heard(undefined, report({ output: "npm ci\n" })), 1100)).toBe(
            false,
        );
    });

    it("never shows one that ended well within the grace, even before the tree says so", () => {
        const done = ended({ outcome: "done" });
        expect(setupShown(running, done, 1100)).toBe(false);
        expect(setupShown(running, done, 1000 + SETUP_GRACE * 10)).toBe(false);
        expect(setupShown({ setup: "ok", setting_up: null }, done, 1100)).toBe(false);
    });

    it("shows one that failed or was stopped at once", () => {
        const failed = ended({ outcome: "failed", reason: "killed" });
        expect(setupShown(running, failed, 1100)).toBe(true);
        const stopped = ended({ outcome: "stopped", why: "user", reason: "Stopped by you." });
        expect(setupShown(running, stopped, 1100)).toBe(true);
        // A stop for the task's removal is said nowhere, and the task goes.
        const removed = ended({ outcome: "stopped", why: "removed", reason: "Stopped because…" });
        expect(setupShown(running, removed, 1100)).toBe(false);
        expect(setupShown({ setup: "failed", setting_up: null }, failed, 1100)).toBe(true);
        expect(
            setupShown({ setup: "pending", setting_up: { state: "interrupted" } }, undefined, 0),
        ).toBe(true);
    });

    it("goes by the end of the run under way, not of an earlier one", () => {
        const earlier = heard(
            undefined,
            report({ began: 5, ended: { outcome: "failed", reason: "x" } }),
        );
        expect(setupShown(running, earlier, 1100)).toBe(false);
        expect(setupShown(running, earlier, 1000 + SETUP_GRACE)).toBe(true);
    });

    it("shows one another Swarmed runs, which says nothing of when it began", () => {
        expect(
            setupShown({ setup: "pending", setting_up: { state: "elsewhere" } }, undefined, 0),
        ).toBe(true);
    });
});

describe("whether a setup ran since it was asked for", () => {
    it("is a run that began after what was kept before", () => {
        const before = heard(undefined, report({ began: 1000 }));
        expect(ranSince(before, heard(before, report({ began: 2000 })))).toBe(true);
        expect(ranSince(undefined, heard(undefined, report({})))).toBe(true);
    });

    it("is not the run kept before, one only read from its log, or none", () => {
        const before = heard(undefined, report({ began: 1000 }));
        expect(ranSince(before, before)).toBe(false);
        expect(ranSince(undefined, readFrom(undefined, { text: "npm ci\n", end: 7 }, null))).toBe(
            false,
        );
        expect(ranSince(undefined, undefined)).toBe(false);
    });
});
