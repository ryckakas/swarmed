import { describe, expect, it } from "vitest";
import type { CloudTask } from "./api/types";
import { cloudTitle, cloudTone, isUnfinished } from "./cloud";

const task = (status: string, prompt = "Tidy the docs\nand the rest"): CloudTask => ({
    id: 1,
    project_id: 1,
    provider: "codex",
    remote_id: "task_1",
    url: "https://chatgpt.com/codex/tasks/task_1",
    prompt,
    branch: "main",
    status,
    session_id: null,
});

describe("a cloud task in the sidebar", () => {
    it("is called by the first line of its prompt", () => {
        expect(cloudTitle(task("pending"))).toBe("Tidy the docs");
        expect(cloudTitle(task("pending", "  \n"))).toBe("Cloud task");
    });

    it("is going, done or failed by what its provider says", () => {
        expect(cloudTone(task("pending"))).toBe("going");
        expect(cloudTone(task("ready"))).toBe("done");
        expect(cloudTone(task("applied"))).toBe("done");
        expect(cloudTone(task("error"))).toBe("failed");
        expect(isUnfinished(task("pending"))).toBe(true);
        expect(isUnfinished(task("ready"))).toBe(false);
    });
});
