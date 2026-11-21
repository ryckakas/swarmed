import { describe, expect, it } from "vitest";
import { busyWarning, newVersion, progress, updateLine } from "./updates";
import { wordsFor } from "./wording";

const plain = wordsFor("plain");

describe("updateLine", () => {
    it("says where updating stands, in the style's words", () => {
        expect(updateLine({ state: "checking" }, plain)).toBe("Checking for updates…");
        expect(updateLine({ state: "up_to_date" }, plain)).toBe("Swarmed is up to date.");
        expect(updateLine({ state: "available", version: "0.3.0" }, plain)).toBe(
            "Swarmed 0.3.0 is out.",
        );
        expect(updateLine({ state: "ready", version: "0.3.0" }, wordsFor("colony"))).toBe(
            "Evolution 0.3.0 has gestated, and takes hold when you quit.",
        );
    });

    it("keeps a failure plain in every style", () => {
        const failed = { state: "failed", reason: "offline" } as const;
        expect(updateLine(failed, wordsFor("colony"))).toBe("Updating failed: offline");
    });

    it("tells how far a download got", () => {
        const downloading = {
            state: "downloading",
            version: "0.3.0",
            received: 50,
            total: 200,
        } as const;
        expect(updateLine(downloading, plain)).toBe("Downloading Swarmed 0.3.0… 25%");
    });
});

describe("progress", () => {
    it("is a percentage when the size is known, else the megabytes in", () => {
        expect(progress(999, 1000)).toBe("99%");
        expect(progress(3 * 1_048_576, null)).toBe("3.0 MB");
        expect(progress(10, 0)).toBe("0.0 MB");
    });
});

describe("newVersion", () => {
    it("is the version out, coming in or ready, and nothing otherwise", () => {
        expect(newVersion({ state: "available", version: "0.3.0" })).toBe("0.3.0");
        expect(newVersion({ state: "ready", version: "0.3.0" })).toBe("0.3.0");
        expect(newVersion({ state: "up_to_date" })).toBeNull();
        expect(newVersion({ state: "failed", reason: "offline" })).toBeNull();
    });
});

describe("busyWarning", () => {
    it("counts the sessions a restart would stop", () => {
        expect(busyWarning(1)).toBe("1 session is in the middle of a turn: restarting stops it.");
        expect(busyWarning(3)).toBe(
            "3 sessions are in the middle of a turn: restarting stops them.",
        );
    });
});
