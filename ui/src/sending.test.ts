import { describe, expect, it } from "vitest";
import type { SessionStatus } from "./api/types";
import { delivered, putBack, waitsItsTurn } from "./sending";

/** An `Api` that only notes how each message was handed over. */
function noting() {
    const calls: string[] = [];
    const api = {
        sessionSend: async (id: number, text: string, files?: string[]) => {
            calls.push(`send ${id} ${text} ${files?.join(",")}`);
        },
        sessionEnqueue: async (id: number, text: string, files?: string[]) => {
            calls.push(`queue ${id} ${text} ${files?.join(",")}`);
            return [];
        },
        sessionMessageNow: async (id: number, text: string, files?: string[]) => {
            calls.push(`now ${id} ${text} ${files?.join(",")}`);
        },
    };
    return { api, calls };
}

describe("delivered", () => {
    it("queues for an agent that is working or waiting for an answer", async () => {
        const { api, calls } = noting();
        await delivered(api, "working", 1, "fix it");
        await delivered(api, "waiting", 1, "and this", ["/a.txt"]);
        expect(calls).toEqual(["queue 1 fix it ", "queue 1 and this /a.txt"]);
    });

    it("sends to one that is idle, stopped with an error, or not known", async () => {
        const { api, calls } = noting();
        const states: (SessionStatus | undefined)[] = ["idle", "errored", undefined];
        for (const status of states) {
            await delivered(api, status, 2, "go");
        }
        expect(calls).toEqual(["send 2 go ", "send 2 go ", "send 2 go "]);
        expect(states.map(waitsItsTurn)).toEqual([false, false, false]);
    });

    it("passes on a failure, so whoever sent can put the message back", async () => {
        const api = {
            sessionSend: async () => {
                throw new Error("the agent is gone");
            },
            sessionEnqueue: async () => [],
            sessionMessageNow: async () => undefined,
        };
        await expect(delivered(api, "idle", 1, "hello")).rejects.toThrow("the agent is gone");
    });
});

describe("delivered at once", () => {
    it("hands a command Swarmed does itself over whatever the agent is doing", async () => {
        const { api, calls } = noting();
        await delivered(api, "working", 1, "/usage", [], true);
        await delivered(api, "waiting", 1, "/model", [], true);
        await delivered(api, "idle", 1, "/status", [], true);
        expect(calls).toEqual(["send 1 /usage ", "send 1 /model ", "send 1 /status "]);
    });
});

describe("delivered now", () => {
    it("stops a busy agent's turn and sends ahead of what waits, and sends as Enter does to an idle one", async () => {
        const { api, calls } = noting();
        await delivered(api, "working", 1, "this instead", ["/a.txt"], false, true);
        await delivered(api, "idle", 1, "hello", [], false, true);
        // A command Swarmed does itself goes at once in any case.
        await delivered(api, "working", 1, "/usage", [], true, true);
        expect(calls).toEqual(["now 1 this instead /a.txt", "send 1 hello ", "send 1 /usage "]);
    });
});

describe("putBack", () => {
    const unsent = { text: "the message", files: ["/a.txt"] };

    it("returns the message to a box that is still empty", () => {
        expect(putBack({ text: "", files: [] }, unsent)).toBe(unsent);
    });

    it("leaves alone what was written or attached since", () => {
        const typed = { text: "something new", files: [] };
        expect(putBack(typed, unsent)).toBe(typed);
        const attached = { text: "", files: ["/b.txt"] };
        expect(putBack(attached, unsent)).toBe(attached);
    });
});
