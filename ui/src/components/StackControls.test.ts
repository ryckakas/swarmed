import { describe, expect, it } from "vitest";
import { stackAction } from "./StackControls";

describe("stackAction", () => {
    it("offers to start a stopped stack and to stop a running one", () => {
        expect(stackAction(false, undefined)).toBe("Start");
        expect(stackAction(true, undefined)).toBe("Stop");
    });

    it("says what is being done until it is, in the style's word for starting", () => {
        expect(stackAction(true, "stopping")).toBe("Stopping…");
        expect(stackAction(false, "starting", "Waking…")).toBe("Waking…");
        // Started, the stack runs before the start is done: it is still starting.
        expect(stackAction(true, "starting")).toBe("Starting…");
    });
});
