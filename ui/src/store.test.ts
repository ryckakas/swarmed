import { beforeEach, describe, expect, it } from "vitest";
import type { Api } from "./api/client";
import { createMockApi } from "./api/mock";
import type { AgentEvent, SessionEvent } from "./api/types";
import { appendEvent, connect, useStore } from "./store";

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
