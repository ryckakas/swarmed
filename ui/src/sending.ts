import type { Api } from "./api/client";
import type { SessionStatus } from "./api/types";

// How a message reaches a session, and what becomes of one that did not. Pure, so it can be
// tested: the app's own `Api` is handed in.

/**
 * Whether a message for a session in this state waits its turn. Sent straight to an agent
 * that is at work or waiting for an answer, it would interrupt the turn and take the open
 * request away with it.
 */
export function waitsItsTurn(status: SessionStatus | undefined): boolean {
    return status === "working" || status === "waiting";
}

/**
 * Sends a message to a session, or puts it in the session's queue while its agent is busy.
 * With `atOnce` it is handed over whatever the agent is doing: one of the agent's own
 * commands that Swarmed does itself, which starts no turn and so interrupts none. With `now`
 * a busy agent's turn is stopped and the message goes ahead of what waits.
 */
export async function delivered(
    api: Pick<Api, "sessionSend" | "sessionEnqueue" | "sessionMessageNow">,
    status: SessionStatus | undefined,
    id: number,
    text: string,
    files: string[] = [],
    atOnce = false,
    now = false,
): Promise<void> {
    if (!waitsItsTurn(status) || atOnce) {
        await api.sessionSend(id, text, files);
    } else if (now) {
        // Asked for now: the turn under way is stopped, and it goes ahead of what waits.
        await api.sessionMessageNow(id, text, files);
    } else {
        await api.sessionEnqueue(id, text, files);
    }
}

/** What stands in the message box: the text and the files attached to it. */
export interface Draft {
    text: string;
    files: string[];
}

/**
 * What the message box holds after a message could not be sent: the message again, unless the
 * user has written something else in the meantime, which is theirs to keep.
 */
export function putBack(now: Draft, unsent: Draft): Draft {
    return now.text === "" && now.files.length === 0 ? unsent : now;
}
