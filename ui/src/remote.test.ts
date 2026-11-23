import { describe, expect, it } from "vitest";
import type { AgentEvent, RemoteState } from "./api/types";
import { remoteOf, remoteSummary, remoteUrl, remoteWanted } from "./remote";

const URL = "https://claude.ai/code/cse_1";

function report(state: RemoteState, url: string | null = URL, detail: string | null = null) {
    return { type: "remote_control", state, url, remote_id: "cse_1", detail } as const;
}

describe("remoteOf", () => {
    it("reads how it stands from the latest report, and nothing from a session never asked", () => {
        const events: AgentEvent[] = [
            report("starting", null),
            report("on"),
            { type: "text", text: "hi" },
            report("reconnecting", URL, "network"),
        ];
        expect(remoteOf(events)).toEqual({ state: "reconnecting", url: URL, detail: "network" });
        expect(remoteOf([{ type: "text", text: "hi" }])).toBeNull();
    });
});

describe("remoteWanted", () => {
    it("is wanted until it is turned off or fails", () => {
        const wanted = (state: RemoteState) => remoteWanted(remoteOf([report(state)]));
        const kept: RemoteState[] = ["starting", "on", "reconnecting", "paused"];
        expect(kept.every(wanted)).toBe(true);
        expect(wanted("off")).toBe(false);
        expect(wanted("failed")).toBe(false);
        expect(remoteWanted(null)).toBe(false);
    });
});

describe("remoteUrl", () => {
    it("gives the address only while the session can be reached there", () => {
        expect(remoteUrl(remoteOf([report("on")]))).toBe(URL);
        expect(remoteUrl(remoteOf([report("reconnecting")]))).toBe(URL);
        // Kept with the report, and not reachable for all that.
        expect(remoteUrl(remoteOf([report("paused")]))).toBeNull();
        expect(remoteUrl(remoteOf([report("off")]))).toBeNull();
        expect(remoteUrl(null)).toBeNull();
    });
});

describe("remoteSummary", () => {
    it("says how it stands, with Claude Code's words for what went wrong", () => {
        expect(remoteSummary(null)).toBe("Off");
        expect(remoteSummary(remoteOf([report("on")]))).toBe("On");
        expect(remoteSummary(remoteOf([report("on", null)]))).toBe("Connecting to claude.ai…");
        expect(remoteSummary(remoteOf([report("failed", null, "sign in again")]))).toBe(
            "Not on: sign in again",
        );
        expect(remoteSummary(remoteOf([report("paused")]))).toBe(
            "Waits for the agent to run again",
        );
    });
});
