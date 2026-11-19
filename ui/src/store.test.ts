import { beforeEach, describe, expect, it } from "vitest";
import type { Api } from "./api/client";
import { createMockApi } from "./api/mock";
import type { AgentEvent, SessionEvent } from "./api/types";
import { appendEvent, connect, useStore, withEvent } from "./store";

const text = (value: string): AgentEvent => ({ type: "text", text: value });
const published = (sessionId: number, event: AgentEvent): SessionEvent => ({
    session_id: sessionId,
    status: null,
    event,
});

beforeEach(() => {
    useStore.setState({ projects: [], events: {}, selectedSessionId: null, problem: null });
});

describe("appendEvent", () => {
    it("adds to an open transcript and ignores sessions that are not open", () => {
        const events = { 1: [text("a")] };
        expect(appendEvent(events, published(1, text("b")))).toEqual({ 1: [text("a"), text("b")] });
        expect(appendEvent(events, published(2, text("b")))).toBe(events);
    });
});

describe("open", () => {
    it("selects the session and loads its history once", async () => {
        let reads = 0;
        const api: Api = {
            ...createMockApi(),
            sessionEvents: async () => {
                reads += 1;
                return [text("history")];
            },
        };
        await useStore.getState().open(api, 7);
        await useStore.getState().open(api, 7);

        expect(useStore.getState().selectedSessionId).toBe(7);
        expect(useStore.getState().events[7]).toEqual([text("history")]);
        expect(reads).toBe(1);
    });

    it("reads again when an event arrives while the history is loading", async () => {
        const stored = [text("first")];
        let release: () => void = () => undefined;
        const gate = new Promise<void>((resolve) => {
            release = resolve;
        });
        let reads = 0;
        const api: Api = {
            ...createMockApi(),
            sessionEvents: async () => {
                reads += 1;
                const snapshot = [...stored];
                if (reads === 1) {
                    await gate;
                }
                return snapshot;
            },
        };

        const opening = useStore.getState().open(api, 7);
        // The agent says something after the first read started but before it returned.
        stored.push(text("second"));
        useStore.getState().apply(published(7, text("second")));
        release();
        await opening;

        expect(reads).toBe(2);
        expect(useStore.getState().events[7]).toEqual([text("first"), text("second")]);
    });

    it("reports a history that cannot be read", async () => {
        const api: Api = {
            ...createMockApi(),
            sessionEvents: async () => {
                throw new Error("session 7 not found");
            },
        };
        await useStore.getState().open(api, 7);
        expect(useStore.getState().problem).toBe("Error: session 7 not found");
    });
});

describe("withEvent", () => {
    const delta = (value: string): AgentEvent => ({ type: "text_delta", text: value });

    it("grows a reply piece by piece and swaps in the whole reply", () => {
        let events = [text("earlier")];
        events = withEvent(events, delta("Rain "));
        events = withEvent(events, delta("falls."));
        expect(events).toEqual([text("earlier"), delta("Rain falls.")]);

        events = withEvent(events, text("Rain falls."));
        expect(events).toEqual([text("earlier"), text("Rain falls.")]);
    });

    it("drops unfinished pieces when something other than the reply follows", () => {
        const done: AgentEvent = { type: "turn_done", is_error: true, result: null, cost_usd: 0 };
        expect(withEvent([delta("Rai")], done)).toEqual([done]);
    });
});

describe("connect", () => {
    it("keeps delivering events after an earlier connection is cleaned up", async () => {
        const api = createMockApi();
        const disconnectFirst = connect(api);
        connect(api);
        disconnectFirst();
        await useStore.getState().open(api, 1);
        // Let the first connection's cleanup, which waits on a promise, run.
        await new Promise((resolve) => setTimeout(resolve, 0));

        const before = useStore.getState().events[1]?.length ?? 0;
        await api.sessionAllow(1, "sample-request");

        // The answer and the tool's result, each delivered once.
        expect(useStore.getState().events[1]).toHaveLength(before + 2);
    });
});

describe("notes", () => {
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

describe("removing", () => {
    it("takes a repository out of the tree, and the open session with it", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        await useStore.getState().open(api, 4);
        await api.projectRemove("bonsai-lint", false);
        await useStore.getState().refresh(api);
        const { projects, selectedSessionId } = useStore.getState();
        expect(projects.map((project) => project.name)).not.toContain("bonsai-lint");
        expect(selectedSessionId).toBeNull();
    });

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

describe("the queue of waiting messages", () => {
    it("is kept beside the transcript and follows the core's events", () => {
        useStore.setState({ events: { 1: [text("a")] }, queues: {} });
        const waiting = [{ id: 7, text: "next" }];
        useStore.getState().apply(published(1, { type: "queue", messages: waiting }));
        expect(useStore.getState().queues[1]).toEqual(waiting);
        expect(useStore.getState().events[1]).toEqual([text("a")]);
    });

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

describe("opening a session", () => {
    it("marks its unread answer as read, here and in the app", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        const unread = () =>
            useStore
                .getState()
                .projects.flatMap((project) => project.tasks)
                .flatMap((task) => task.sessions)
                .find((session) => session.id === 5)?.unread;
        expect(unread()).toBe(true);
        await useStore.getState().open(api, 5);
        expect(unread()).toBe(false);
        const stored = (await api.overview())
            .flatMap((project) => project.tasks)
            .flatMap((task) => task.sessions)
            .find((session) => session.id === 5);
        expect(stored?.unread).toBe(false);
    });
});

describe("setting up an agent", () => {
    it("installs it, then lets its models be chosen", async () => {
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

    it("tells the menus to read the agents again", () => {
        const before = useStore.getState().agentsVersion;
        useStore.getState().agentsChanged();
        expect(useStore.getState().agentsVersion).toBe(before + 1);
    });
});

describe("the settings", () => {
    it("open on the part asked for, and otherwise where they were last", () => {
        useStore.getState().showSettings(true, "shortcuts");
        expect(useStore.getState().settingsTab).toBe("shortcuts");
        useStore.getState().showSettings(false);
        useStore.getState().showSettings(true);
        expect(useStore.getState()).toMatchObject({ settingsOpen: true, settingsTab: "shortcuts" });
        useStore.getState().showSettings(false, "appearance");
    });
});

describe("MCP servers", () => {
    it("set up here come first, are named by their secrets only, and can be removed", async () => {
        const api = createMockApi();
        const before = await api.mcpServers();
        expect(before.every((server) => server.origin === "claude")).toBe(true);
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

describe("closed sessions", () => {
    it("are counted on their task, listed, and come back when reopened", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        await useStore.getState().close(api, 3);
        const task = () =>
            useStore
                .getState()
                .projects.flatMap((project) => project.tasks)
                .find((candidate) => candidate.name === "invoice-export");
        expect(task()?.closed).toBe(1);
        expect(task()?.sessions).toEqual([]);
        const closed = await api.sessionsClosed("hero", "invoice-export");
        expect(closed.map((session) => session.id)).toEqual([3]);

        await api.sessionReopen(3);
        await useStore.getState().refresh(api);
        expect(task()?.closed).toBe(0);
        expect(task()?.sessions.map((session) => session.id)).toEqual([3]);
    });
});
