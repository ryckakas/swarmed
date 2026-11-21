// A task's files as the panel shows them: folders that unfold, one line each. Pure, so it can be
// tested; reading the folders is the component's part.

import type { FileListing } from "./api/types";

/** One line of the list: a file, or a folder that may be unfolded. */
export interface FileRow {
    /** Where it is, from the top of the worktree. */
    path: string;
    name: string;
    /** How many folders deep it lies; 0 at the top. */
    depth: number;
    dir: boolean;
    /** For a folder, whether what it holds is shown under it. */
    open: boolean;
}

/** The path of `name` in the folder `dir`; the top folder is the empty path. */
export function pathIn(dir: string, name: string): string {
    return dir === "" ? name : `${dir}/${name}`;
}

/**
 * The lines to show: what the top folder holds, and under every unfolded folder that has been
 * read, what that holds, and so on down. `listings` are the folders read so far, by path.
 */
export function fileRows(
    listings: Record<string, FileListing | undefined>,
    unfolded: ReadonlySet<string>,
    dir = "",
    depth = 0,
): FileRow[] {
    const entries = listings[dir]?.entries ?? [];
    return entries.flatMap((entry) => {
        const path = pathIn(dir, entry.name);
        const open = entry.dir && unfolded.has(path);
        const row = { path, name: entry.name, depth, dir: entry.dir, open };
        return open ? [row, ...fileRows(listings, unfolded, path, depth + 1)] : [row];
    });
}

/** The set with `path` taken out if it was in, and put in if it was not. */
export function toggled(unfolded: ReadonlySet<string>, path: string): Set<string> {
    const next = new Set(unfolded);
    if (!next.delete(path)) {
        next.add(path);
    }
    return next;
}

/** What stands under a folder's lines when it holds more than are listed. */
export function leftOut(listing: FileListing | undefined): string | null {
    const more = listing?.more ?? 0;
    return more > 0 ? `${more} more not shown. Open the folder in your editor to see them.` : null;
}
