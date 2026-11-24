import { describe, expect, it } from "vitest";
import { closesBeside, DIALOG_STARTS, dialogStart } from "./dialogs";

// The UI's source, as text: every module but the tests.
const sources = import.meta.glob<string>(["./**/*.{ts,tsx}", "!./**/*.test.ts"], {
    query: "?raw",
    import: "default",
    eager: true,
});

/** A dialog element, written in JSX or made by a script. */
const DIALOG_ELEMENT = /<dialog[\s>]|createElement\(\s*["'`]dialog["'`]/;

const DIALOG = "./components/Dialog.tsx";

/** The component taken from Dialog.tsx: a file that takes it and drew nothing with it would
 *  not pass the lint, and a mention of it in a comment is not this. */
const TAKES_DIALOG = /import \{[^}]*\bDialog\b[^}]*\} from "\.\/Dialog";/;

describe("dialogs", () => {
    it("are a dialog element in one file only, Dialog.tsx", () => {
        const holding = Object.entries(sources)
            .filter(([, text]) => DIALOG_ELEMENT.test(text))
            .map(([path]) => path);
        expect(holding).toEqual([DIALOG]);
    });

    it("are each built with Dialog", () => {
        const named = Object.keys(sources).filter(
            (path) => path.endsWith("Dialog.tsx") && path !== DIALOG,
        );
        expect(named.length).toBeGreaterThan(10);
        const without = named.filter((path) => !TAKES_DIALOG.test(sources[path] ?? ""));
        expect(without).toEqual([]);
    });

    it("start the keyboard on what the content marks, then a field, then the main button", () => {
        const [marked, field, main, any] = DIALOG_STARTS;
        const holding = (...there: (string | undefined)[]) =>
            dialogStart((selector) => (there.includes(selector) ? selector : null));
        expect(holding(marked, field, main, any)).toBe(marked);
        expect(holding(field, main, any)).toBe(field);
        expect(holding(main, any)).toBe(main);
        // A question whose action cannot be undone has no main button: its way out is first.
        expect(holding(any)).toBe(any);
        expect(holding()).toBeNull();
    });

    it("close on a click beside them only where they say so, and never while busy", () => {
        const beside = {
            pressedBeside: true,
            endedBeside: true,
            closeOnBackdrop: true,
            busy: false,
        };
        expect(closesBeside(beside)).toBe(true);
        expect(closesBeside({ ...beside, closeOnBackdrop: false })).toBe(false);
        expect(closesBeside({ ...beside, busy: true })).toBe(false);
        // Text selected from inside and let go beside it, and the other way round.
        expect(closesBeside({ ...beside, pressedBeside: false })).toBe(false);
        expect(closesBeside({ ...beside, endedBeside: false })).toBe(false);
    });
});
