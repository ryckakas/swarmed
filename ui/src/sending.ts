import type { Api } from "./api/client";
import type { SessionStatus } from "./api/types";
import type { PastedImage } from "./images";

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
 * What a message points the agent at besides its words: files, sessions to read, pull requests
 * and issues of the session's repository, and elements picked in its task's browser, by number.
 */
export interface Attached {
    files: string[];
    sessions: number[];
    github: number[];
    elements: number[];
}

/** A message that points at nothing. */
export const NOTHING_ATTACHED: Attached = { files: [], sessions: [], github: [], elements: [] };

/**
 * Has the core read the pull requests and issues `github` of `project`, unless it did a moment
 * ago, so that the message naming them waits on nothing: the core lists them from what it read.
 * Nothing is asked for none.
 */
export async function githubRead(
    api: Pick<Api, "projectGithubRead">,
    project: string | undefined,
    github: number[],
): Promise<void> {
    if (github.length > 0 && project !== undefined) {
        await api.projectGithubRead(project, github);
    }
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
    attached: Attached = NOTHING_ATTACHED,
    atOnce = false,
    now = false,
): Promise<void> {
    const { files, sessions, github, elements } = attached;
    if (!waitsItsTurn(status) || atOnce) {
        await api.sessionSend(id, text, files, sessions, github, elements);
    } else if (now) {
        // Asked for now: the turn under way is stopped, and it goes ahead of what waits.
        await api.sessionMessageNow(id, text, files, sessions, github, elements);
    } else {
        await api.sessionEnqueue(id, text, files, sessions, github, elements);
    }
}

/**
 * The files a message goes with: those attached, then the images pasted into it once the core
 * has kept them as files of the session's own. Nothing is kept for a message with none.
 */
export async function filesWith(
    api: Pick<Api, "sessionImagesKeep">,
    id: number,
    draft: Pick<Draft, "files" | "images">,
): Promise<string[]> {
    if (draft.images.length === 0) {
        return draft.files;
    }
    const kept = await api.sessionImagesKeep(
        id,
        draft.images.map((image) => image.data),
    );
    return [...draft.files, ...kept];
}

/**
 * What stands in the message box: the text, the files attached to it, and the images pasted
 * into it, which are only in the window's memory until the message goes.
 */
export interface Draft {
    text: string;
    files: string[];
    images: PastedImage[];
}

/** An empty message box; one object, so that reading it twice gives the same thing. */
export const NO_DRAFT: Draft = { text: "", files: [], images: [] };

/** Whether a draft holds nothing at all. */
export function isEmpty(draft: Draft): boolean {
    return draft.text === "" && draft.files.length === 0 && draft.images.length === 0;
}

/** `drafts` with the one under `key` changed by `change`; one emptied is dropped. */
export function withDraft<K extends string | number>(
    drafts: Record<K, Draft>,
    key: K,
    change: Partial<Draft>,
): Record<K, Draft> {
    const draft: Draft = { ...(drafts[key] ?? NO_DRAFT), ...change };
    const kept = { ...drafts };
    if (isEmpty(draft)) {
        delete kept[key];
    } else {
        kept[key] = draft;
    }
    return kept;
}

/**
 * What the message box holds after a message could not be sent: the message again, unless the
 * user has written something else in the meantime, which is theirs to keep.
 */
export function putBack(now: Draft, unsent: Draft): Draft {
    return isEmpty(now) ? unsent : now;
}
