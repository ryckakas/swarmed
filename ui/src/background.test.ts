import { describe, expect, it } from "vitest";
import type { BackgroundTask } from "./api/types";
import { stopOffered } from "./background";

function task(change: Partial<BackgroundTask>): BackgroundTask {
    return {
        session_id: 1,
        session_title: "A session",
        task_id: "b1",
        description: "Run the dev server",
        kind: "local_bash",
        state: "running",
        summary: null,
        tool_use_id: null,
        output: false,
        stoppable: true,
        ...change,
    };
}

describe("what a row offers to stop it", () => {
    it("is a button while it runs and its agent can stop it", () => {
        expect(stopOffered(task({}))).toBe("button");
    });

    it("says it cannot be stopped while it runs and its agent has no way to", () => {
        expect(stopOffered(task({ stoppable: false }))).toBe("cannot");
    });

    it("is nothing once it has ended, however it could have been stopped", () => {
        expect(stopOffered(task({ state: "completed" }))).toBeNull();
        expect(stopOffered(task({ state: "stopped", stoppable: false }))).toBeNull();
    });
});
