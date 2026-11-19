import { describe, expect, it } from "vitest";
import { sampleProjects } from "./api/mock";
import type { Project, SessionEvent } from "./api/types";
import {
    applySessionEvent,
    countWaiting,
    findSession,
    findSessionHome,
    taskNameForPrompt,
    taskNameFrom,
    taskStatus,
    titleFrom,
    uniqueTaskName,
    withoutSession,
    withSession,
    withSessionStatus,
} from "./tree";

function withTitle(projects: Project[], sessionId: number, title: string): Project[] {
    const session = findSession(projects, sessionId);
    if (session) {
        session.title = title;
    }
    return projects;
}

describe("withSession and withoutSession", () => {
    it("replaces a session with its newer copy", () => {
        const before = sampleProjects();
        const current = findSession(before, 2);
        expect(current).toBeDefined();
        const after = withSession(before, { ...current, id: 2, model: "opus" } as typeof current & {
            id: number;
        });
        expect(findSession(after, 2)?.model).toBe("opus");
        expect(findSession(after, 1)).toBe(findSession(before, 1));
    });

    it("removes a session and nothing else", () => {
        const before = sampleProjects();
        const after = withoutSession(before, 2);
        expect(findSession(after, 2)).toBeUndefined();
        expect(findSession(after, 1)).toEqual(findSession(before, 1));
    });
});

describe("withSessionStatus", () => {
    it("replaces one session's status and leaves other branches untouched", () => {
        const before = sampleProjects();
        const after = withSessionStatus(before, 2, "idle");

        expect(findSession(after, 2)?.status).toBe("idle");
        expect(findSession(before, 2)?.status).toBe("working");
        expect(after[1]).toBe(before[1]);
        expect(after[0]?.tasks[1]).toBe(before[0]?.tasks[1]);
    });

    it("changes nothing for an unknown session", () => {
        const before = sampleProjects();
        expect(withSessionStatus(before, 999, "idle")).toEqual(before);
    });
});

describe("applySessionEvent", () => {
    const published = (status: SessionEvent["status"]): SessionEvent => ({
        session_id: 1,
        status,
        event: { type: "text", text: "hello" },
    });

    it("follows the status an event leads to", () => {
        const after = applySessionEvent(sampleProjects(), published("working"));
        expect(findSession(after, 1)?.status).toBe("working");
    });

    it("titles an untitled session from its first message, and only the first", () => {
        const untitled = withTitle(sampleProjects(), 1, "");
        const message = (text: string): SessionEvent => ({
            session_id: 1,
            status: "working",
            event: { type: "user_message", text },
        });
        const once = applySessionEvent(untitled, message("  Fix the login\nwith details"));
        expect(findSession(once, 1)?.title).toBe("Fix the login");
        const twice = applySessionEvent(once, message("And another thing"));
        expect(findSession(twice, 1)?.title).toBe("Fix the login");
    });

    it("keeps the tree as it is when the event changes no status", () => {
        const before = sampleProjects();
        expect(applySessionEvent(before, published(null))).toBe(before);
    });
});

describe("titleFrom", () => {
    it("shortens a long first line to the core's limit", () => {
        const title = titleFrom("x".repeat(200));
        expect(title).toHaveLength(60);
        expect(title.endsWith("…")).toBe(true);
    });
});

describe("lookups", () => {
    it("finds a session and where it lives", () => {
        const projects = sampleProjects();
        const session = findSession(projects, 3);
        expect(session?.title).toContain("CSV export");
        expect(session?.id).toBe(3);
        const home = session && findSessionHome(projects, session);
        expect(home?.project.name).toBe("hero");
        expect(home?.task.name).toBe("invoice-export");
        expect(findSession(projects, null)).toBeUndefined();
    });

    it("counts the sessions waiting for a decision", () => {
        expect(countWaiting(sampleProjects())).toBe(1);
    });

    it("describes a task by its most urgent session", () => {
        const [hero, bonsai] = sampleProjects();
        expect(hero?.tasks.map(taskStatus)).toEqual(["waiting", "idle", null]);
        expect(bonsai?.tasks.map(taskStatus)).toEqual(["errored"]);
    });
});

describe("taskNameFrom", () => {
    it("turns typed text into a name the core accepts", () => {
        expect(taskNameFrom("Fix the Login redirect!")).toBe("fix-the-login-redirect");
        expect(taskNameFrom("PM-16037")).toBe("pm-16037");
        expect(taskNameFrom("  ../up  ")).toBe("up");
        expect(taskNameFrom("v1..2")).toBe("v1.2");
        expect(taskNameFrom("???")).toBe("");
        expect(taskNameFrom("x".repeat(100))).toHaveLength(64);
    });
});

describe("taskNameForPrompt", () => {
    it("names a task after the first line of its prompt, cut at a word", () => {
        expect(taskNameForPrompt("Fix the login redirect\nUsers get stuck")).toBe(
            "fix-the-login-redirect",
        );
        const long = taskNameForPrompt(
            "Investigate why the nightly export job drops the VAT column",
        );
        expect(long).toBe("investigate-why-the-nightly-export-job");
        expect(taskNameForPrompt("x".repeat(100))).toHaveLength(40);
        expect(taskNameForPrompt("???")).toBe("task");
    });
});

describe("uniqueTaskName", () => {
    it("adds a number only when the name is taken", () => {
        expect(uniqueTaskName("fix", [])).toBe("fix");
        expect(uniqueTaskName("fix", ["fix"])).toBe("fix-2");
        expect(uniqueTaskName("fix", ["fix", "fix-2"])).toBe("fix-3");
    });
});
