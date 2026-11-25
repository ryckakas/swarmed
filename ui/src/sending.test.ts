import { describe, expect, it } from "vitest";
import type { SessionStatus } from "./api/types";
import {
    type Draft,
    delivered,
    filesWith,
    NO_DRAFT,
    NOTHING_ATTACHED,
    putBack,
    waitsItsTurn,
    withDraft,
} from "./sending";

/** What a message points at, as a note: its files, then `@` and each session, `#` and each number. */
function pointed(
    files: string[] = [],
    sessions: number[] = [],
    github: number[] = [],
    elements: number[] = [],
): string {
    const named = [...sessions.map((id) => `@${id}`), ...github.map((n) => `#${n}`)];
    return [...files, ...named, ...elements.map((n) => `element ${n}`)].join(",");
}

/** An `Api` that only notes how each message was handed over. */
function noting() {
    const calls: string[] = [];
    const api = {
        sessionSend: async (
            id: number,
            text: string,
            files?: string[],
            sessions?: number[],
            github?: number[],
            elements?: number[],
        ) => {
            calls.push(`send ${id} ${text} ${pointed(files, sessions, github, elements)}`);
        },
        sessionEnqueue: async (
            id: number,
            text: string,
            files?: string[],
            sessions?: number[],
            github?: number[],
            elements?: number[],
        ) => {
            calls.push(`queue ${id} ${text} ${pointed(files, sessions, github, elements)}`);
            return [];
        },
        sessionMessageNow: async (
            id: number,
            text: string,
            files?: string[],
            sessions?: number[],
            github?: number[],
            elements?: number[],
        ) => {
            calls.push(`now ${id} ${text} ${pointed(files, sessions, github, elements)}`);
        },
    };
    return { api, calls };
}

describe("delivered", () => {
    it("queues for an agent that is working or waiting for an answer", async () => {
        const { api, calls } = noting();
        await delivered(api, "working", 1, "fix it");
        const attached = { files: ["/a.txt"], sessions: [4], github: [142], elements: [3] };
        await delivered(api, "waiting", 1, "and this", attached);
        expect(calls).toEqual(["queue 1 fix it ", "queue 1 and this /a.txt,@4,#142,element 3"]);
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
        await delivered(api, "working", 1, "/usage", NOTHING_ATTACHED, true);
        await delivered(api, "waiting", 1, "/model", NOTHING_ATTACHED, true);
        await delivered(api, "idle", 1, "/status", NOTHING_ATTACHED, true);
        expect(calls).toEqual(["send 1 /usage ", "send 1 /model ", "send 1 /status "]);
    });
});

describe("delivered now", () => {
    it("stops a busy agent's turn and sends ahead of what waits, and sends as Enter does to an idle one", async () => {
        const { api, calls } = noting();
        const attached = { files: ["/a.txt"], sessions: [], github: [], elements: [] };
        await delivered(api, "working", 1, "this instead", attached, false, true);
        await delivered(api, "idle", 1, "hello", NOTHING_ATTACHED, false, true);
        // A command Swarmed does itself goes at once in any case.
        await delivered(api, "working", 1, "/usage", NOTHING_ATTACHED, true, true);
        expect(calls).toEqual(["now 1 this instead /a.txt", "send 1 hello ", "send 1 /usage "]);
    });
});

describe("putBack", () => {
    const unsent = { text: "the message", files: ["/a.txt"], images: [] };

    it("returns the message to a box that is still empty", () => {
        expect(putBack({ text: "", files: [], images: [] }, unsent)).toBe(unsent);
    });

    it("leaves alone what was written or attached since", () => {
        const typed = { text: "something new", files: [], images: [] };
        expect(putBack(typed, unsent)).toBe(typed);
        const attached = { text: "", files: ["/b.txt"], images: [] };
        expect(putBack(attached, unsent)).toBe(attached);
    });
});

describe("withDraft", () => {
    it("changes one draft and leaves the others alone", () => {
        const drafts: Record<number, Draft> = {
            1: { text: "one", files: [], images: [] },
            2: { text: "two", files: ["/a"], images: [] },
        };
        expect(withDraft(drafts, 1, { files: ["/b"], images: [] })).toEqual({
            1: { text: "one", files: ["/b"], images: [] },
            2: { text: "two", files: ["/a"], images: [] },
        });
        expect(withDraft(drafts, 3, { text: "three" })[3]).toEqual({
            text: "three",
            files: [],
            images: [],
        });
        expect(drafts[1]).toEqual({ text: "one", files: [], images: [] });
    });

    it("drops a draft that is emptied", () => {
        const drafts = { storefront: { text: "fix it", files: [], images: [] } };
        expect(withDraft(drafts, "storefront", { text: "" })).toEqual({});
        expect(withDraft(drafts, "storefront", NO_DRAFT)).toEqual({});
    });
});

describe("filesWith", () => {
    const image = { key: 1, format: "png" as const, data: "iVBORw0KGgo=" };

    it("keeps the pasted images as the session's files and attaches them after the others", async () => {
        const asked: string[] = [];
        const api = {
            sessionImagesKeep: async (id: number, images: string[]) => {
                asked.push(`${id} ${images.join(",")}`);
                return images.map((_, index) => `/data/attachments/${id}/pasted-${index + 1}.png`);
            },
        };
        const files = await filesWith(api, 4, { files: ["/a.txt"], images: [image] });
        expect(files).toEqual(["/a.txt", "/data/attachments/4/pasted-1.png"]);
        expect(asked).toEqual(["4 iVBORw0KGgo="]);
        // With none pasted, nothing is kept.
        expect(await filesWith(api, 4, { files: ["/a.txt"], images: [] })).toEqual(["/a.txt"]);
        expect(asked).toHaveLength(1);
    });

    it("lets a draft with only an image count as one, and puts it back", () => {
        const pasted = { ...NO_DRAFT, images: [image] };
        const none: Record<number, Draft> = {};
        expect(withDraft(none, 1, pasted)).toEqual({ 1: pasted });
        expect(putBack(NO_DRAFT, pasted)).toBe(pasted);
        expect(putBack(pasted, { ...NO_DRAFT, text: "older" })).toBe(pasted);
    });
});
