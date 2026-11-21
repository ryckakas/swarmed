import { describe, expect, it } from "vitest";
import { createMockApi, suggestedTitle } from "./mock";
import type { SessionEvent } from "./types";

// The sample `Api` the UI runs on in a plain browser. These tests are about the mock alone:
// what the store makes of an `Api` is tested with the store.

const sessionOf = async (api: ReturnType<typeof createMockApi>, id: number) =>
    (await api.overview())
        .flatMap((project) => project.tasks)
        .flatMap((task) => task.sessions)
        .find((session) => session.id === id);

describe("the mock's notes", () => {
    it("are kept per repository and per task, and marked in the tree", async () => {
        const api = createMockApi();
        await api.setNote("bonsai-lint", "rust-support", "token: abc");
        expect(await api.note("bonsai-lint", "rust-support")).toBe("token: abc");
        expect(await api.note("bonsai-lint", null)).toBe("");
        const marked = (await api.overview()).find((project) => project.name === "bonsai-lint");
        expect(marked?.has_note).toBe(false);
        expect(marked?.tasks[0]?.has_note).toBe(true);

        await api.setNote("bonsai-lint", "rust-support", "  ");
        const cleared = (await api.overview()).find((project) => project.name === "bonsai-lint");
        expect(cleared?.tasks[0]?.has_note).toBe(false);
    });
});

describe("the mock's removing", () => {
    it("refuses a task with unsaved work until told to remove it anyway", async () => {
        const api = createMockApi();
        await expect(api.taskRemove("hero", "invoice-export", false)).rejects.toThrow(
            "uncommitted changes",
        );
        await api.taskRemove("hero", "invoice-export", true);
        const hero = (await api.overview()).find((project) => project.name === "hero");
        expect(hero?.tasks.map((task) => task.name)).not.toContain("invoice-export");
    });
});

describe("the mock's queue of waiting messages", () => {
    it("can be rewritten and emptied while it waits", async () => {
        const api = createMockApi();
        const [first] = await api.sessionEnqueue(3, "1st");
        await api.sessionEnqueue(3, "second");
        const edited = await api.sessionQueueEdit(3, first?.id ?? 0, "first");
        expect(edited.map((message) => message.text)).toEqual(["first", "second"]);
        const left = await api.sessionQueueEdit(3, first?.id ?? 0, " ");
        expect(left.map((message) => message.text)).toEqual(["second"]);
        await api.sessionSendQueued(3);
        expect(await api.sessionQueue(3)).toEqual([]);
    });
});

describe("the mock's agent setup", () => {
    it("installs an agent, then lets its models be chosen", async () => {
        const api = createMockApi();
        const setups = await api.agentSetups();
        expect(setups.map((setup) => setup.id)).toEqual(["claude", "codex", "opencode"]);
        expect(setups[2]?.installed).toBeNull();
        const installed = await api.agentInstall("opencode");
        expect(installed.configured).toBe(true);
        const chosen = await api.agentConfigure("opencode", ["openrouter/x-ai/grok-4.7"]);
        expect(chosen.models).toEqual(["openrouter/x-ai/grok-4.7"]);
        // Asked again without naming models, the choice stands.
        expect((await api.agentConfigure("opencode", null)).models).toEqual(chosen.models);
    });
});

describe("the mock's MCP servers", () => {
    it("set up here come first, are named by their secrets only, and can be removed", async () => {
        const api = createMockApi();
        const before = await api.mcpServers();
        expect(before.every((server) => server.origin !== "swarmed")).toBe(true);
        const listed = await api.mcpSave({
            name: "tracker",
            command: "npx -y tracker",
            url: "",
            secrets: [["TRACKER_TOKEN", "s3cret"]],
            enabled: true,
        });
        expect(listed[0]).toMatchObject({ name: "tracker", origin: "swarmed", kind: "stdio" });
        expect(JSON.stringify(listed)).not.toContain("s3cret");
        expect(await api.mcpRemove("tracker")).toEqual(before);
    });
});

describe("the mock's tree", () => {
    it("says what the events said: a working session is still working when read again", async () => {
        const api = createMockApi();
        expect((await sessionOf(api, 3))?.status).toBe("idle");
        await api.sessionSend(3, "slow");
        expect((await sessionOf(api, 3))?.status).toBe("working");
        await api.sessionInterrupt(3);
        expect(await sessionOf(api, 3)).toMatchObject({ status: "idle", unread: true });
    });

    it("keeps a new session's title, from its first message and from a renaming", async () => {
        const api = createMockApi();
        const created = await api.sessionNew("hero", "invoice-export", {
            agent: null,
            model: null,
            supervision: "auto",
            effort: null,
        });
        await api.sessionSend(created.id, "slow");
        expect((await sessionOf(api, created.id))?.title).toBe("slow");
        await api.sessionRename(created.id, "Slow work");
        expect((await sessionOf(api, created.id))?.title).toBe("Slow work");
    });
});

describe("the mock's agent", () => {
    it("is given a better title while it writes its first reply, and only then", async () => {
        const api = createMockApi();
        const heard: SessionEvent["event"][] = [];
        await api.onSessionEvent((published) => heard.push(published.event));
        const created = await api.sessionNew("hero", "invoice-export", {
            agent: null,
            model: null,
            supervision: "auto",
            effort: null,
        });
        const turn = async (message: string) => {
            heard.length = 0;
            await api.sessionSend(created.id, message);
            await expect.poll(() => heard.some((event) => event.type === "turn_done")).toBe(true);
            return heard.map((event) => event.type);
        };

        const first = await turn("fix the login redirect");
        const renamed = first.indexOf("renamed");
        // Between two pieces of the reply: what the store must not lose the text over.
        expect(first.slice(0, renamed)).toContain("text_delta");
        expect(first.slice(renamed, first.indexOf("text"))).toContain("text_delta");
        expect((await sessionOf(api, created.id))?.title).toBe(
            suggestedTitle("fix the login redirect"),
        );

        expect(await turn("and the tests")).not.toContain("renamed");
    });
});

describe("the mock's terminal", () => {
    it("says where its replay ends and where each piece of output starts", async () => {
        const api = createMockApi();
        const offsets: number[] = [];
        let printed = "";
        await api.onTerminalOutput((output) => {
            offsets.push(output.offset);
            printed += output.data;
        });
        expect(await api.terminalOpen(1, 80, 24)).toEqual({ replay: "", end: 0 });
        await expect.poll(() => printed).not.toBe("");
        await api.terminalWrite(1, "ls");
        expect(offsets).toEqual([0, printed.length - 2]);
        expect(await api.terminalOpen(1, 80, 24)).toEqual({ replay: printed, end: printed.length });
    });
});
