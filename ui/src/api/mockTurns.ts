import type { Api } from "./client";
import type { FileChange, FileDiff, SkippedFile, Turn } from "./types";

// The sample sessions' turns, to see the Changes tab's turn picker in a browser. The first
// session of `fix-login-redirect` has three turns: two ended, one with the second session at
// work beside it, and the latest still under way. The second session's only turn is under
// way. The session of `invoice-export` left out a CSV too large to keep.

const MINUTE = 60_000;
const NOW = Date.now();

interface SampleTurn extends Turn {
    files: FileChange[];
    skipped: SkippedFile[];
}

const SAMPLE_TURNS: Record<number, SampleTurn[]> = {
    1: [
        {
            message: 0,
            words: "Fix the redirect loop after login",
            at_ms: NOW - 42 * MINUTE,
            ended: true,
            beside: [],
            files: [
                { path: "src/auth/login.ts", kind: "modified", added: 9, removed: 2 },
                { path: "src/auth/safe-next.ts", kind: "added", added: 6, removed: 0 },
            ],
            skipped: [],
        },
        {
            message: 1,
            words: "Keep the next page when it is safe, and swap the logo",
            at_ms: NOW - 25 * MINUTE,
            ended: true,
            beside: ["Write a regression test for the loop"],
            files: [
                { path: "public/logo.png", kind: "modified", added: null, removed: null },
                { path: "src/auth/login.ts", kind: "modified", added: 3, removed: 1 },
                { path: "test/auth/login.test.ts", kind: "added", added: 24, removed: 0 },
            ],
            skipped: [],
        },
        {
            message: 2,
            words: "Remove the old redirect test",
            at_ms: NOW - 3 * MINUTE,
            ended: false,
            beside: ["Write a regression test for the loop"],
            files: [
                {
                    path: "test/auth/old-redirect.test.ts",
                    kind: "deleted",
                    added: 0,
                    removed: 41,
                },
            ],
            skipped: [],
        },
    ],
    2: [
        {
            message: 0,
            words: "Write a regression test for the loop",
            at_ms: NOW - 27 * MINUTE,
            ended: false,
            beside: ["Fix the redirect loop after login"],
            files: [{ path: "test/auth/login.test.ts", kind: "added", added: 24, removed: 0 }],
            skipped: [],
        },
    ],
    3: [
        {
            message: 0,
            words: "Why does the CSV export drop the VAT column?",
            at_ms: NOW - 3 * 60 * MINUTE,
            ended: true,
            beside: [],
            files: [{ path: "src/export/csv.ts", kind: "modified", added: 4, removed: 1 }],
            skipped: [{ path: "exports/all-invoices-2025.csv", bytes: 48_300_000 }],
        },
    ],
};

/** The diff the sample shows for a file of a turn: built from its counts, as git would. */
function sampleDiff(file: FileChange): string {
    const header = `diff --git a/${file.path} b/${file.path}\n`;
    if (file.added === null || file.removed === null) {
        return `${header}Binary files a/${file.path} and b/${file.path} differ\n`;
    }
    const removed = Array.from({ length: file.removed }, (_, at) => `-old line ${at + 1}`);
    const added = Array.from({ length: file.added }, (_, at) => `+new line ${at + 1}`);
    const from = file.kind === "added" ? "0,0" : `1,${file.removed}`;
    const to = file.kind === "deleted" ? "0,0" : `1,${file.added}`;
    return `${header}--- a/${file.path}\n+++ b/${file.path}\n@@ -${from} +${to} @@\n${[...removed, ...added].join("\n")}\n`;
}

/** What a sample session has in place of files that went back, for any other session. */
const SOME_FILES: FileChange[] = [
    { path: "src/app.ts", kind: "modified", added: 5, removed: 2 },
    { path: "src/notes.md", kind: "deleted", added: 0, removed: 12 },
];

/**
 * What taking a sample session's files back to before its message `message` would change: what
 * its turns from that one on changed, each file once, the other way round.
 */
export function sampleTurnFiles(id: number, message: number): FileChange[] {
    const turns = SAMPLE_TURNS[id]?.filter((turn) => turn.message >= message) ?? [];
    // A turn sent in the browser has no sample of its own.
    if (turns.length === 0) {
        return SOME_FILES;
    }
    const back = (file: FileChange): FileChange => {
        const kind =
            file.kind === "added" ? "deleted" : file.kind === "deleted" ? "added" : file.kind;
        return { ...file, kind, added: file.removed, removed: file.added };
    };
    const byPath = new Map(
        turns.flatMap((turn) => turn.files).map((file) => [file.path, back(file)]),
    );
    return [...byPath.values()].sort((a, b) => a.path.localeCompare(b.path));
}

function turnOf(id: number, message: number): SampleTurn {
    const turn = SAMPLE_TURNS[id]?.find((one) => one.message === message);
    if (!turn) {
        throw new Error(`session ${id} keeps no checkpoints of the turn of its message ${message}`);
    }
    return turn;
}

/** The sample sessions' turns, as the core lists and reads them. */
export function turnsApi(): Pick<Api, "sessionTurns" | "sessionTurnChanges" | "sessionTurnDiff"> {
    return {
        sessionTurns: async (id) =>
            (SAMPLE_TURNS[id] ?? []).map(({ files: _files, skipped: _skipped, ...turn }) => turn),
        sessionTurnChanges: async (id, message) => {
            const turn = turnOf(id, message);
            return { files: turn.files, skipped: turn.skipped, ended: turn.ended };
        },
        sessionTurnDiff: async (id, message, path): Promise<FileDiff> => {
            const file = turnOf(id, message).files.find((one) => one.path === path);
            return { text: file ? sampleDiff(file) : "", truncated: false };
        },
    };
}
