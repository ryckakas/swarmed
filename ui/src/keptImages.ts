// What the window has read of the images kept for sessions, so that one shown again is not
// read again. A whole image is held as a `data:` address, which can be tens of megabytes, so
// a session's images go when the session does, as its draft and its place in the transcript do.

/** What has been read, by session and then by path; a kept image never changes. */
const read = new Map<number, Map<string, Promise<string>>>();

/**
 * The image at `file` of session `sessionId`, as a `data:` address: read with `ask` the first
 * time, and from then on what that gave. One that could not be read is asked for again next
 * time, since it may be there by then.
 */
export function keptImage(
    sessionId: number,
    file: string,
    ask: (sessionId: number, file: string) => Promise<string>,
): Promise<string> {
    const ofSession = read.get(sessionId) ?? new Map<string, Promise<string>>();
    read.set(sessionId, ofSession);
    const known = ofSession.get(file);
    if (known !== undefined) {
        return known;
    }
    const image = ask(sessionId, file);
    ofSession.set(file, image);
    image.catch(() => {
        // Only if it is still this reading: the session may have gone and come back meanwhile.
        if (read.get(sessionId)?.get(file) === image) {
            ofSession.delete(file);
        }
    });
    return image;
}

/** Forgets what was read of the images of sessions `sessionIds`, which are closed or gone. */
export function forgetImages(sessionIds: number[]): void {
    for (const id of sessionIds) {
        read.delete(id);
    }
}
