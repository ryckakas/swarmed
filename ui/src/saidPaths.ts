// The paths in what the user wrote, so that they are drawn as code and one in the task's
// worktree opens in the editor. Pure, so it can be tested.
//
// A path is absolute (`/` and at least two parts, one of them with a letter), from home
// (`~/…`), or relative: `./` or `../` and anything after, or a slash and a file name at its end
// (`ui/src/setup.ts`, `.github/workflows/ci.yml`, `config/.env`), so that `and/or`, `1/2` and a
// date stay words. Any of them may have a line and a column after it (`files.rs:85`,
// `setup.ts:12:5`). Punctuation that ends a sentence is left out of it, and so is a closing
// bracket with no opening one inside it.

/** A path as written, the file it names, and the line and column written after it. */
export interface SaidPath {
    path: string;
    shown: string;
    line?: number;
    column?: number;
}

/**
 * A path as it is found, before its end is trimmed: at the start of a word, or after an opening
 * bracket, a quote, `*` or `=`, a run of what a path holds, then a line and a column.
 */
const PATH =
    /(?<![^\s([{<"'`*=])([\p{L}\p{N}_.~/@][\p{L}\p{N}_.~/@+%[\]-]*)(?::(\d+)(?::(\d+))?(?![\p{L}\p{N}]))?/gu;

/** The last part of a relative path: a name with an extension, or a dot file. */
const FILE_NAME = /^(?:[^.].*)?\.\p{L}[\p{L}\p{N}]*$/u;

/** `text`, which holds no token, code or address, taken apart into words and paths. */
export function pathsIn(text: string): (string | SaidPath)[] {
    const pieces: (string | SaidPath)[] = [];
    let from = 0;
    for (const match of text.matchAll(PATH)) {
        const found = foundPath(match);
        if (found === null) {
            continue;
        }
        if (match.index > from) {
            pieces.push(text.slice(from, match.index));
        }
        pieces.push(found);
        from = match.index + found.shown.length;
    }
    if (from < text.length) {
        pieces.push(text.slice(from));
    }
    return pieces;
}

/**
 * Where `path` lies in a worktree at `worktree`, from its top, if it lies in it: a relative
 * path is the worktree's, an absolute one is if it starts with the worktree's own. `null` for a
 * path from home or one that leads out. The core checks it again, links followed.
 */
export function inWorktree(path: string, worktree: string): string | null {
    if (path.startsWith("~")) {
        return null;
    }
    if (!path.startsWith("/")) {
        return normalised(path);
    }
    const root = worktree.replace(/\/+$/, "");
    if (root === "" || !path.startsWith(`${root}/`)) {
        return null;
    }
    return normalised(path.slice(root.length + 1));
}

/** The address of the file `file`, from the top of a worktree at `worktree`. */
export function fileAddress(worktree: string, file: string): string {
    return encodeURI(`file://${worktree.replace(/\/+$/, "")}/${file}`);
}

/** The path a match stands for, or `null` if what it found is no path. */
function foundPath(match: RegExpExecArray): SaidPath | null {
    const [whole, written = "", line, column] = match;
    const path = line === undefined ? trimmed(written) : written;
    if (!namesAFile(path)) {
        return null;
    }
    return {
        path,
        shown: line === undefined ? path : whole,
        ...(line !== undefined && { line: Number(line) }),
        ...(column !== undefined && { column: Number(column) }),
    };
}

/**
 * `found` without the dots that end a sentence after it, or a square bracket it does not open.
 * Its brackets are counted once and a closing one taken off as it is trimmed, so a long run of
 * them is not counted again for each.
 */
function trimmed(found: string): string {
    const opened = count(found, "[");
    let closed = count(found, "]");
    let end = found.length;
    while (found[end - 1] === "." || (found[end - 1] === "]" && closed > opened)) {
        closed -= found[end - 1] === "]" ? 1 : 0;
        end -= 1;
    }
    return found.slice(0, end);
}

/** How many times `character` stands in `text`. */
function count(text: string, character: string): number {
    return text.split(character).length - 1;
}

/** Whether `path` is one by the rules above, rather than words with a slash in them. */
function namesAFile(path: string): boolean {
    if (path.startsWith("~/")) {
        return path.length > 2;
    }
    if (path.startsWith("/")) {
        const parts = path.split("/").filter((part) => part !== "");
        return parts.length >= 2 && parts.some((part) => /\p{L}/u.test(part));
    }
    if (/^\.\.?\//.test(path)) {
        return path.replace(/^(?:\.\.?\/)+/, "") !== "";
    }
    const parts = path.split("/");
    return (
        parts.length >= 2 &&
        parts.every((part) => part !== "") &&
        FILE_NAME.test(parts.at(-1) ?? "")
    );
}

/** `path` with its `.` parts gone and each `..` taking the part before it; `null` past the top. */
function normalised(path: string): string | null {
    const parts: string[] = [];
    for (const part of path.split("/")) {
        if (part === "..") {
            if (parts.pop() === undefined) {
                return null;
            }
        } else if (part !== "" && part !== ".") {
            parts.push(part);
        }
    }
    return parts.length === 0 ? null : parts.join("/");
}
