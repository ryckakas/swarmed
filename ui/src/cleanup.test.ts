import { describe, expect, it } from "vitest";
import type { CleanupItem } from "./api/types";
import {
    age,
    deleteLabel,
    initialTicks,
    itemDetail,
    itemTitle,
    keptNote,
    kindSummary,
    kindWording,
    shownAtRisk,
    tickedKeys,
    toggledTick,
} from "./cleanup";
import { wordsFor } from "./wording";

const NOW = 1_800_000_000;
const DAY = 86_400;

const item = (key: string, extra: Partial<CleanupItem> = {}): CleanupItem => ({
    key,
    project: "shop",
    name: key,
    task: null,
    reason: "no task owns this folder",
    bytes: null,
    since: null,
    at_risk: false,
    ...extra,
});

describe("kindSummary", () => {
    it("counts the items and adds up the sizes of those on disk", () => {
        expect(kindSummary([])).toBe("None");
        expect(kindSummary([item("a"), item("b")])).toBe("2");
        const sized = [item("a", { bytes: 1_200_000 }), item("b"), item("c", { bytes: 300_000 })];
        expect(kindSummary(sized)).toBe("3, 1.5 MB");
    });
});

describe("ticks", () => {
    const safe = item("safe");
    const risky = item("risky", { at_risk: true });
    const other = item("other");

    it("starts with everything ticked that loses no work, and nothing at risk", () => {
        expect(initialTicks([safe, risky, other])).toEqual(["safe", "other"]);
    });

    it("ticks and unticks one at a time", () => {
        const ticked = toggledTick(["safe"], "risky");
        expect(ticked).toEqual(["safe", "risky"]);
        expect(toggledTick(ticked, "safe")).toEqual(["risky"]);
    });

    it("asks to delete only what is still listed, in the list's order", () => {
        // A tick left over from an item a clean-up took away is not asked for again.
        expect(tickedKeys([safe, risky, other], ["other", "gone", "safe"])).toEqual([
            "safe",
            "other",
        ]);
    });

    it("tells the core which of the keys asked for were shown as losing work", () => {
        expect(shownAtRisk([safe, risky, other], ["safe", "risky"])).toEqual(["risky"]);
        // One shown as losing work and left unticked is not among them.
        expect(shownAtRisk([safe, risky, other], ["safe"])).toEqual([]);
    });

    it("says how many the button deletes", () => {
        expect(deleteLabel(0, false)).toBe("Delete");
        expect(deleteLabel(3, false)).toBe("Delete 3");
        expect(deleteLabel(3, true)).toBe("Deleting…");
    });
});

describe("keptNote", () => {
    it("names what was not deleted and why, and says nothing when all went", () => {
        expect(keptNote({ deleted: ["a"], kept: [] })).toBeNull();
        const kept = [{ key: "shop:x", name: "x", reason: "git would not delete it" }];
        expect(keptNote({ deleted: [], kept })).toBe("Not deleted: x (git would not delete it).");
    });
});

describe("items", () => {
    it("names a removed worktree by its task, and a snapshot by its repository", () => {
        const removed = item("/w/shop/.removed/a-1-2", { task: "fix-login" });
        expect(itemTitle("removed", removed)).toBe("shop / fix-login");
        expect(itemTitle("removed", item("/w/shop/.removed/odd"))).toBe(
            "shop / /w/shop/.removed/odd",
        );
        expect(itemTitle("snapshots", item("shop:", { name: "staging" }))).toBe("shop / staging");
        expect(itemTitle("branches", item("shop:ada/x", { name: "ada/x" }))).toBe("ada/x");
    });

    it("says whose it was, why it is there, since when and its size", () => {
        const branch = item("shop:ada/x", {
            task: "x",
            reason: "1 commit on it is on neither main nor a remote",
            since: NOW - 3 * DAY,
        });
        expect(itemDetail("branches", branch, NOW)).toBe(
            "of x · 1 commit on it is on neither main nor a remote · removed 3 days ago",
        );
        const snapshot = item("shop:", {
            task: "one",
            reason: "new tasks start from it",
            since: NOW - 20 * DAY,
            bytes: 640_000_000,
        });
        expect(itemDetail("snapshots", snapshot, NOW)).toBe(
            "from one · new tasks start from it · taken 2 weeks ago · 640 MB",
        );
    });
});

describe("age", () => {
    it("says how long ago, in the largest unit that fits", () => {
        expect(age(NOW - 60, NOW)).toBe("today");
        expect(age(NOW + 60, NOW)).toBe("today");
        expect(age(NOW - DAY, NOW)).toBe("yesterday");
        expect(age(NOW - 13 * DAY, NOW)).toBe("13 days ago");
        expect(age(NOW - 14 * DAY, NOW)).toBe("2 weeks ago");
        expect(age(NOW - 90 * DAY, NOW)).toBe("3 months ago");
        expect(age(NOW - 800 * DAY, NOW)).toBe("2 years ago");
    });
});

describe("kindWording", () => {
    it("speaks in the style's words", () => {
        expect(kindWording("branches", wordsFor("plain")).label).toBe("Branches of removed tasks");
        expect(kindWording("removed", wordsFor("colony")).label).toBe("Removed cells");
        expect(kindWording("snapshots", wordsFor("colony")).title).toBe("Forget cocoons?");
    });
});
