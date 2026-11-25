// What the window has read of the images kept for sessions, so that one shown again is not
// read again. A whole image is held as a `data:` address, which can be tens of megabytes, so
// a session's images go when the session does, as its draft and its place in the transcript do.
// Images a tool returned are held only as the smaller copies drawn in the transcript.

/** What has been read, by session and then by path; a kept image never changes. */
type Read = Map<number, Map<string, Promise<string>>>;

/** Whole images, as pasted into messages. */
const read: Read = new Map();

/** Smaller copies, by session and then by size and path. */
const smaller: Read = new Map();

/**
 * What `make` gives for `file` of session `sessionId`, made the first time and remembered in
 * `cache` from then on. One that failed is made again next time, since it may be there by then.
 */
function remembered(
    cache: Read,
    sessionId: number,
    file: string,
    make: () => Promise<string>,
): Promise<string> {
    const ofSession = cache.get(sessionId) ?? new Map<string, Promise<string>>();
    cache.set(sessionId, ofSession);
    const known = ofSession.get(file);
    if (known !== undefined) {
        return known;
    }
    const image = make();
    ofSession.set(file, image);
    image.catch(() => {
        // Only if it is still this reading: the session may have gone and come back meanwhile.
        if (cache.get(sessionId)?.get(file) === image) {
            ofSession.delete(file);
        }
    });
    return image;
}

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
    return remembered(read, sessionId, file, () => ask(sessionId, file));
}

/**
 * The image at `file` of session `sessionId` as a smaller copy, whose longest side is at most
 * `most` pixels: read with `ask` and made smaller with `shrink` the first time. Only the copy
 * is held, never the whole image.
 */
export function keptThumbnail(
    sessionId: number,
    file: string,
    most: number,
    ask: (sessionId: number, file: string) => Promise<string>,
    shrink: (source: string, most: number) => Promise<string>,
): Promise<string> {
    const make = () => ask(sessionId, file).then((whole) => shrink(whole, most));
    return remembered(smaller, sessionId, `${most}:${file}`, make);
}

/** Forgets what was read of the images of sessions `sessionIds`, which are closed or gone. */
export function forgetImages(sessionIds: number[]): void {
    for (const id of sessionIds) {
        read.delete(id);
        smaller.delete(id);
    }
}
