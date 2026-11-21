import { beforeEach, describe, expect, it } from "vitest";
import type { Api } from "./api/client";
import { createMockApi } from "./api/mock";
import type { AgentEvent, SessionEvent } from "./api/types";
import {
    appendEvent,
    connect,
    heardWhileReading,
    menuChosen,
    type Reading,
    useStore,
    withEvent,
    withStreamed,
} from "./store";

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

    it("does not read again for the pieces of a reply, and shows them once it has read", async () => {
        let release: () => void = () => undefined;
        const gate = new Promise<void>((resolve) => {
            release = resolve;
        });
        let reads = 0;
        const api: Api = {
            ...createMockApi(),
            sessionEvents: async () => {
                reads += 1;
                await gate;
                return [text("first")];
            },
        };
        const delta = (value: string): AgentEvent => ({ type: "text_delta", text: value });

        const opening = useStore.getState().open(api, 7);
        // The agent is in the middle of a reply: its pieces are published and never stored.
        useStore.getState().apply(published(7, delta("Rain ")));
        useStore.getState().apply(published(7, delta("falls")));
        useStore.getState().apply(published(7, { type: "config_changed", kind: "rule" }));
        release();
        await opening;

        expect(reads).toBe(1);
        expect(useStore.getState().events[7]).toEqual([text("first"), delta("Rain falls")]);
        // The rest of the reply joins what was heard while reading.
        useStore.getState().apply(published(7, delta(".")));
        expect(useStore.getState().events[7]).toEqual([text("first"), delta("Rain falls.")]);
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

    it("drops unfinished pieces when the turn ends without the whole reply", () => {
        const done: AgentEvent = { type: "turn_done", is_error: true, result: null, cost_usd: 0 };
        expect(withEvent([delta("Rai")], done)).toEqual([done]);
        const exited: AgentEvent = { type: "exited", code: 1, stderr: "" };
        expect(withEvent([delta("Rai")], exited)).toEqual([exited]);
    });

    it("keeps the reply so far when something that is not part of it arrives meanwhile", () => {
        const renamed: AgentEvent = { type: "renamed", title: "Rain" };
        let events = withEvent([text("earlier"), delta("Rain ")], renamed);
        expect(events).toEqual([text("earlier"), renamed, delta("Rain ")]);
        // The reply goes on growing, and the whole of it still takes the pieces' place.
        events = withEvent(events, delta("falls."));
        expect(events).toEqual([text("earlier"), renamed, delta("Rain falls.")]);
        events = withEvent(events, text("Rain falls."));
        expect(events).toEqual([text("earlier"), renamed, text("Rain falls.")]);
    });

    it("keeps it for a mark, a sub-agent's word and a usage limit as well", () => {
        const others: AgentEvent[] = [
            { type: "marked", colour: "green", label: "done" },
            { type: "sub", parent: "t1", event: text("Looking.") },
            { type: "rate_limit", status: "allowed_warning", resets_at: null, raw: null },
        ];
        for (const other of others) {
            expect(withEvent([delta("Rain ")], other)).toEqual([other, delta("Rain ")]);
        }
    });
});

describe("heardWhileReading", () => {
    const delta = (value: string): AgentEvent => ({ type: "text_delta", text: value });
    const fresh = (): Reading => ({ missedEvent: false, streamed: "" });

    it("reads again only for events the core stores", () => {
        const unstored: AgentEvent[] = [
            delta("Rain"),
            { type: "queue", messages: [] },
            { type: "config_changed", kind: "rule" },
            { type: "deleted" },
        ];
        for (const event of unstored) {
            const reading = fresh();
            heardWhileReading(reading, event);
            expect(reading.missedEvent).toBe(false);
        }
        const reading = fresh();
        heardWhileReading(reading, { type: "renamed", title: "Rain" });
        expect(reading.missedEvent).toBe(true);
    });

    it("keeps the pieces of a reply until the whole reply is there to be read", () => {
        const reading = fresh();
        heardWhileReading(reading, delta("Rain "));
        heardWhileReading(reading, { type: "renamed", title: "Rain" });
        heardWhileReading(reading, delta("falls"));
        expect(reading.streamed).toBe("Rain falls");
        expect(withStreamed([text("first")], reading.streamed)).toEqual([
            text("first"),
            delta("Rain falls"),
        ]);

        heardWhileReading(reading, text("Rain falls."));
        expect(reading).toEqual({ missedEvent: true, streamed: "" });
        const history = [text("first"), text("Rain falls.")];
        expect(withStreamed(history, reading.streamed)).toBe(history);
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
});

describe("the queue of waiting messages", () => {
    it("is kept beside the transcript and follows the core's events", () => {
        useStore.setState({ events: { 1: [text("a")] }, queues: {} });
        const waiting = [{ id: 7, text: "next" }];
        useStore.getState().apply(published(1, { type: "queue", messages: waiting }));
        expect(useStore.getState().queues[1]).toEqual(waiting);
        expect(useStore.getState().events[1]).toEqual([text("a")]);
    });

    it("follows the mock through a rewrite and an emptying, as it would the core", async () => {
        const api = createMockApi();
        const disconnect = connect(api);
        const queued = () => useStore.getState().queues[3]?.map((message) => message.text);
        const [first] = await api.sessionEnqueue(3, "1st");
        await api.sessionEnqueue(3, "second");
        expect(queued()).toEqual(["1st", "second"]);
        await api.sessionQueueEdit(3, first?.id ?? 0, "first");
        expect(queued()).toEqual(["first", "second"]);
        await api.sessionQueueEdit(3, first?.id ?? 0, " ");
        expect(queued()).toEqual(["second"]);
        disconnect();
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
        const closed = await api.sessionsClosed("storefront", "invoice-export");
        expect(closed.map((session) => session.id)).toEqual([3]);

        await api.sessionReopen(3);
        await useStore.getState().refresh(api);
        expect(task()?.closed).toBe(0);
        expect(task()?.sessions.map((session) => session.id)).toEqual([3]);
    });
});

describe("runInTerminal", () => {
    it("shows the terminal and hands the command over once, to the session it is for", () => {
        useStore.setState({ terminalOpen: false, terminalCommand: null });
        useStore.getState().runInTerminal(7, "make test");
        expect(useStore.getState().terminalOpen).toBe(true);
        // Another session's terminal leaves it waiting.
        expect(useStore.getState().takeTerminalCommand(8)).toBeNull();
        expect(useStore.getState().takeTerminalCommand(7)).toBe("make test");
        // Asked again, as an effect that runs twice does: nothing left to type.
        expect(useStore.getState().takeTerminalCommand(7)).toBeNull();
    });

    it("shows the shell in place of a Claude Code shown there, so the command runs at once", () => {
        useStore.setState({ terminalCommand: null, claudeTerminals: { 3: "vim", 7: "vim" } });
        useStore.getState().runInTerminal(7, "make deploy");
        // Left waiting behind Claude Code, it would run whenever the shell was next shown.
        expect(useStore.getState().claudeTerminals).toEqual({ 3: "vim" });
        expect(useStore.getState().takeTerminalCommand(7)).toBe("make deploy");
    });
});

describe("Claude Code opened in a session's terminal", () => {
    it("shows the terminal with it, for that session only, until the shell is shown again", () => {
        useStore.setState({ terminalOpen: false, claudeTerminals: { 3: "vim" } });
        useStore.getState().openClaudeTerminal(7, "privacy-settings");
        expect(useStore.getState().terminalOpen).toBe(true);
        expect(useStore.getState().claudeTerminals).toEqual({ 3: "vim", 7: "privacy-settings" });
        useStore.getState().leaveClaudeTerminal(7);
        expect(useStore.getState().claudeTerminals).toEqual({ 3: "vim" });
    });
});

describe("the menu bar", () => {
    beforeEach(() => {
        useStore.setState({ banner: null, settingsOpen: false });
    });

    it("opens the About view, the shortcuts and the settings for its own items", () => {
        menuChosen("about");
        expect(useStore.getState().banner).toBe("about");
        menuChosen("shortcuts");
        expect(useStore.getState().banner).toBe("shortcuts");
        menuChosen("settings");
        expect(useStore.getState().settingsOpen).toBe(true);
    });

    it("does nothing for an item it does not know", () => {
        menuChosen("quit");
        expect(useStore.getState()).toMatchObject({ banner: null, settingsOpen: false });
    });

    it("reads the installed version when it connects", async () => {
        useStore.setState({ version: null });
        const stop = connect(createMockApi());
        await expect.poll(() => useStore.getState().version).toBe("0.1.0");
        stop();
    });
});
