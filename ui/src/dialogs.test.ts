import { describe, expect, it } from "vitest";

// The UI's source, as text: every module but the tests.
const sources = import.meta.glob<string>(["./**/*.{ts,tsx}", "!./**/*.test.ts"], {
    query: "?raw",
    import: "default",
    eager: true,
});

/** A dialog element, written in JSX or made by a script. */
const DIALOG_ELEMENT = /<dialog[\s>]|createElement\(\s*["'`]dialog["'`]/;

const DIALOG = "./components/Dialog.tsx";

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
        const without = named.filter((path) => !sources[path]?.includes("<Dialog"));
        expect(without).toEqual([]);
    });
});
