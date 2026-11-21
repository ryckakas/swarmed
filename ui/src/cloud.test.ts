import { describe, expect, it } from "vitest";
import type { CloudTask } from "./api/types";
import {
    briefRequest,
    cloudProvider,
    cloudTitle,
    cloudTone,
    cloudTooltip,
    isUnfinished,
    unseenByCloud,
} from "./cloud";

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

    it("says of a Claude Code session only that it was sent, and is not asked about again", () => {
        const sent: CloudTask = { ...task("sent"), provider: "claude", branch: "" };
        expect(cloudTone(sent)).toBe("sent");
        expect(isUnfinished(sent)).toBe(false);
        expect(cloudProvider("claude")).toEqual({ name: "Claude Code", agent: "claude" });
        expect(cloudTooltip(sent)).toBe(
            "Tidy the docs\nIn Claude Code's cloud: sent, and Claude Code does not say how it is getting on. Opens at Claude Code.",
        );
        expect(cloudTooltip(task("ready"))).toBe(
            "Tidy the docs\nIn Codex's cloud, from main: ready. Opens at Codex.",
        );
    });
});

describe("handing a session's work to the cloud", () => {
    it("asks the session's agent for a brief, naming the provider and what the user added", () => {
        const asked = briefRequest("claude", "  only step 2 ");
        expect(asked).toContain("a Claude Code cloud agent");
        expect(asked).toContain("`provider` set to `claude`");
        expect(asked.endsWith("\n\nonly step 2")).toBe(true);
        expect(briefRequest("codex", " ").endsWith("tell me what is missing.")).toBe(true);
    });

    it("says what the cloud agent will not see, and nothing when it sees all", () => {
        const pushed = { changed: 0, unpushed: 0, pushed_before: true };
        expect(unseenByCloud(pushed)).toBeNull();
        expect(unseenByCloud({ ...pushed, changed: 1 })).toBe(
            "Only on this Mac, where the cloud agent does not see it: 1 file is not committed.",
        );
        expect(unseenByCloud({ ...pushed, changed: 3, unpushed: 2 })).toBe(
            "Only on this Mac, where the cloud agent does not see it: 3 files are not committed, 2 commits are not pushed.",
        );
    });
});
