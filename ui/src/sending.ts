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

/** Sends a message to a session, or puts it in the session's queue while its agent is busy. */
export async function delivered(
    api: Pick<Api, "sessionSend" | "sessionEnqueue">,
    status: SessionStatus | undefined,
    id: number,
    text: string,
    files: string[] = [],
): Promise<void> {
    if (waitsItsTurn(status)) {
        await api.sessionEnqueue(id, text, files);
    } else {
        await api.sessionSend(id, text, files);
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
