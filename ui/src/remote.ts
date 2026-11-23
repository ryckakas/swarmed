import type { AgentEvent, RemoteState } from "./api/types";

// How Remote Control of a session stands, read from its events. Pure, so it can be tested.

/** How Remote Control stands, by the session's latest report of it. */
export interface Remote {
    state: RemoteState;
    /** Where the session is on claude.ai, once Claude Code has said. */
    url: string | null;
    /** Claude Code's words for a failure or a reconnection. */
    detail: string | null;
}

/** Remote Control as the session's events last said; `null` if it was never asked for. */
export function remoteOf(events: AgentEvent[]): Remote | null {
    const latest = events.findLast((event) => event.type === "remote_control");
    if (latest?.type !== "remote_control") {
        return null;
    }
    return { state: latest.state, url: latest.url, detail: latest.detail };
}

/**
 * Whether the user wants it on as it stands, so that it is turned on again whenever the
 * session's agent starts. Turned off or failed, it waits to be turned on again.
 */
export function remoteWanted(remote: Remote | null): boolean {
    return remote !== null && remote.state !== "off" && remote.state !== "failed";
}

/** Where the session can be opened on claude.ai now, if it can be. */
export function remoteUrl(remote: Remote | null): string | null {
    const reachable = remote?.state === "on" || remote?.state === "reconnecting";
    return reachable ? (remote?.url ?? null) : null;
}

/** What is said of how it stands, in a few words. */
export function remoteSummary(remote: Remote | null): string {
    const because = remote?.detail ? `: ${remote.detail}` : "";
    switch (remote?.state) {
        case "starting":
            return "Turning on…";
        case "on":
            return remote.url ? "On" : "Connecting to claude.ai…";
        case "reconnecting":
            return `Reconnecting to claude.ai${because}`;
        case "paused":
            return "Waits for the agent to run again";
        case "failed":
            return `Not on${because}`;
        default:
            return "Off";
    }
}
