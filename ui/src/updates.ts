import type { UpdateState } from "./api/types";
import type { Words } from "./wording";

// What the About view and the version in the corner say of updating Swarmed. Pure, so it can
// be tested.

/** What the About view says of updating. */
export function updateLine(update: UpdateState, words: Words): string {
    switch (update.state) {
        case "unknown":
        case "checking":
            return words.update.checking;
        case "up_to_date":
            return words.update.upToDate;
        case "available":
            return update.failed === undefined
                ? words.update.available(update.version)
                : `Swarmed ${update.version} could not be installed: ${update.failed}`;
        case "downloading":
            return words.update.downloading(
                update.version,
                progress(update.received, update.total),
            );
        case "ready":
            return words.update.ready(update.version);
        case "failed":
            return `Updating failed: ${update.reason}`;
    }
}

/** How far a download got: a percentage when its size is known, else the megabytes in. */
export function progress(received: number, total: number | null): string {
    if (total !== null && total > 0) {
        return `${Math.floor((received * 100) / total)}%`;
    }
    return `${(received / 1_048_576).toFixed(1)} MB`;
}

/** The version a newer Swarmed is out as, or `null` while none is known. */
export function newVersion(update: UpdateState): string | null {
    switch (update.state) {
        case "available":
        case "downloading":
        case "ready":
            return update.version;
        default:
            return null;
    }
}

/**
 * What is said before restarting while `busy` sessions are in a turn. It asks something of the
 * user, so it and its two answers stay plain in every style, like a failure.
 */
export function busyWarning(busy: number): string {
    return busy === 1
        ? "1 session is in the middle of a turn: restarting stops it."
        : `${busy} sessions are in the middle of a turn: restarting stops them.`;
}
