// Turns a unified diff into lines the changes view can draw. Pure, so it can be tested.

export type DiffLine =
    /** The start of a stretch of changes; `text` is what git prints after the line numbers. */
    | { kind: "hunk"; text: string }
    | { kind: "context"; text: string; oldLine: number; newLine: number }
    | { kind: "added"; text: string; newLine: number }
    | { kind: "removed"; text: string; oldLine: number }
    /** Something git says instead of showing lines, such as "Binary files differ". */
    | { kind: "note"; text: string };

const HUNK = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@ ?(.*)$/;
/** The lines before the first hunk that only repeat the file's name and mode. */
const HEADER =
    /^(diff --git |index |--- |\+\+\+ |new file|deleted file|old mode|new mode|similarity )/;

/** The lines of one file's unified diff, numbered as in the old and the new file. */
export function parseDiff(text: string): DiffLine[] {
    const lines: DiffLine[] = [];
    let oldLine = 0;
    let newLine = 0;
    let inHunk = false;
    for (const raw of text.split("\n")) {
        const hunk = HUNK.exec(raw);
        if (hunk) {
            oldLine = Number(hunk[1]);
            newLine = Number(hunk[2]);
            inHunk = true;
            lines.push({ kind: "hunk", text: hunk[3] ?? "" });
        } else if (!inHunk) {
            pushNote(lines, raw);
        } else if (raw.startsWith("+")) {
            lines.push({ kind: "added", text: raw.slice(1), newLine: newLine++ });
        } else if (raw.startsWith("-")) {
            lines.push({ kind: "removed", text: raw.slice(1), oldLine: oldLine++ });
        } else if (raw.startsWith(" ")) {
            lines.push({
                kind: "context",
                text: raw.slice(1),
                oldLine: oldLine++,
                newLine: newLine++,
            });
        } else {
            // "\ No newline at end of file", and the empty string after the last newline.
            pushNote(lines, raw.replace(/^\\ /, ""));
        }
    }
    return lines;
}

function pushNote(lines: DiffLine[], text: string) {
    if (text !== "" && !HEADER.test(text)) {
        lines.push({ kind: "note", text });
    }
}

/** A path split into its folder, shown quietly, and the file's own name. */
export function splitPath(path: string): { folder: string; name: string } {
    const cut = path.lastIndexOf("/") + 1;
    return { folder: path.slice(0, cut), name: path.slice(cut) };
}
