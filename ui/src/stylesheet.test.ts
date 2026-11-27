import { describe, expect, it } from "vitest";
import type { Stylesheet } from "./api/types";
import {
    answersInOrder,
    changedAt,
    changedLines,
    changeSaid,
    drawUserStylesheet,
    mayKeep,
    NOTHING_PUT_ASIDE,
    shownLine,
    stylesheetBox,
    stylesheetNotice,
} from "./stylesheet";

const sheet = (over: Partial<Stylesheet> = {}): Stylesheet => ({
    on: true,
    left_out: false,
    applied: "a {}",
    accepted: "a {}",
    file: "a {}",
    too_large: false,
    changed_at: 1_700_000_000,
    waiting: false,
    ...over,
});

interface FakeElement {
    id: string;
    textContent: string;
    remove(): void;
}

/** A head with its elements in order, and what of a document drawing a stylesheet reaches. */
function fakePage() {
    const elements: FakeElement[] = [];
    const takeOut = (element: FakeElement) => {
        const at = elements.indexOf(element);
        if (at >= 0) {
            elements.splice(at, 1);
        }
    };
    const create = (): FakeElement => {
        const element: FakeElement = { id: "", textContent: "", remove: () => takeOut(element) };
        return element;
    };
    const head = {
        get lastElementChild() {
            return elements.at(-1) ?? null;
        },
        appendChild(element: FakeElement) {
            takeOut(element);
            elements.push(element);
            return element;
        },
    };
    const page = {
        head,
        createElement: create,
        getElementById: (id: string) => elements.find((element) => element.id === id) ?? null,
    };
    return { page: page as unknown as Document, head, elements, create };
}

describe("drawUserStylesheet", () => {
    it("keeps one element, last in the head, after the app's own stylesheets", () => {
        const { page, head, elements, create } = fakePage();
        const own = head.appendChild(create());
        drawUserStylesheet(page, "a { color: red; }");
        // A stylesheet added later, as the custom layout's on first drawing, comes before it.
        head.appendChild(create());
        drawUserStylesheet(page, "a { color: blue; }");
        const drawn = elements.filter((element) => element.id === "user-stylesheet");
        expect(drawn).toHaveLength(1);
        expect(elements.at(-1)).toBe(drawn[0]);
        expect(drawn[0]?.textContent).toBe("a { color: blue; }");
        expect(elements[0]).toBe(own);
        expect(elements).toHaveLength(3);
    });

    it("takes the element away when there is nothing to apply", () => {
        const { page, elements } = fakePage();
        drawUserStylesheet(page, "a {}");
        drawUserStylesheet(page, "");
        expect(elements).toHaveLength(0);
    });
});

describe("stylesheetNotice", () => {
    it("asks about a change on disk only while the stylesheet is used", () => {
        const changed = sheet({ file: ".deny { display: none; }", waiting: true });
        expect(stylesheetNotice(changed, NOTHING_PUT_ASIDE)).toEqual({
            kind: "changed",
            file: ".deny { display: none; }",
            accepted: "a {}",
        });
        expect(stylesheetNotice({ ...changed, on: false }, NOTHING_PUT_ASIDE)).toBeNull();
        expect(stylesheetNotice(sheet(), NOTHING_PUT_ASIDE)).toBeNull();
        expect(stylesheetNotice(null, NOTHING_PUT_ASIDE)).toBeNull();
    });

    it("stays away for the change put aside, and comes back for the next one", () => {
        const changed = sheet({ file: "b {}", waiting: true });
        const aside = { file: "b {}", leftOut: false };
        expect(stylesheetNotice(changed, aside)).toBeNull();
        expect(stylesheetNotice({ ...changed, file: "c {}" }, aside)?.kind).toBe("changed");
    });

    it("says a run leaves it out, before anything about the file", () => {
        const out = sheet({ left_out: true, applied: "", file: "b {}", waiting: true });
        expect(stylesheetNotice(out, NOTHING_PUT_ASIDE)).toEqual({ kind: "left_out" });
        expect(stylesheetNotice(out, { file: null, leftOut: true })).toBeNull();
        expect(stylesheetNotice({ ...out, on: false }, NOTHING_PUT_ASIDE)).toBeNull();
    });
});

describe("changedLines", () => {
    it("shows only what changed, taken out before put in, with gaps between", () => {
        const before = "a {}\nb {}\nc {}\nd {}\ne {}\n";
        const after = "a {}\nB {}\nc {}\nd {}\ne {}\nf {}\n";
        expect(changedLines(before, after)).toEqual([
            { kind: "removed", text: "b {}" },
            { kind: "added", text: "B {}" },
            { kind: "gap", text: "" },
            { kind: "added", text: "f {}" },
        ]);
    });

    it("shows a new file as all put in, and says how much", () => {
        const lines = changedLines("", ".deny {\n  display: none;\n}");
        expect(lines.map((line) => line.kind)).toEqual(["added", "added", "added"]);
        expect(changeSaid(lines)).toBe("3 lines added");
        expect(changeSaid(changedLines("a\nb", "a"))).toBe("1 line removed");
        expect(changeSaid(changedLines("a\n", "a"))).toBe("only line breaks changed");
    });
});

describe("changedLines at its edges", () => {
    it("keeps a carriage return in its line, so a line ending changed shows", () => {
        expect(changedLines("a {}\n", "a {}\r\n")).toEqual([
            { kind: "removed", text: "a {}" },
            { kind: "added", text: "a {}\r" },
        ]);
    });

    it("shows everything of both past what it compares line by line", () => {
        const many = (last: string) =>
            [...Array.from({ length: 2_100 }, (_, n) => `.r${n} {}`), last].join("\n");
        const lines = changedLines(many("a {}"), many("b {}"));
        expect(lines.filter((line) => line.kind === "removed")).toHaveLength(2_101);
        expect(lines.filter((line) => line.kind === "added")).toHaveLength(2_101);
    });
});

describe("shownLine", () => {
    it("draws what would not show as its code", () => {
        expect(shownLine("a\u200B{}\u202Eb")).toBe("a⟨U+200B⟩{}⟨U+202E⟩b");
        expect(shownLine("--sky: #4fc8b4;")).toBe("--sky: #4fc8b4;");
    });
});

describe("stylesheetBox", () => {
    it("starts from what is applied, never from a change not accepted", () => {
        const waiting = sheet({ file: ".deny { display: none; }", waiting: true });
        expect(stylesheetBox(waiting)).toEqual({ kind: "box", text: "a {}" });
        expect(stylesheetBox(sheet())).toEqual({ kind: "box", text: "a {}" });
        expect(stylesheetBox(sheet({ file: null }))).toEqual({ kind: "box", text: "" });
    });

    it("offers no box for a file too large to read, which saving would replace unseen", () => {
        const large = sheet({ file: null, too_large: true, applied: "" });
        expect(stylesheetBox(large)).toEqual({ kind: "too_large" });
    });

    it("saves only an edit of the user's own", () => {
        expect(mayKeep("a {}", null)).toBe(false);
        expect(mayKeep("a {}", "a {}")).toBe(false);
        expect(mayKeep("a {}", "b {}")).toBe(true);
    });
});

describe("answersInOrder", () => {
    it("takes no answer asked before one taken", () => {
        const answers = answersInOrder();
        const read = answers.ask();
        const accept = answers.ask();
        expect(answers.takes(accept)).toBe(true);
        expect(answers.takes(read)).toBe(false);
        expect(answers.takes(answers.ask())).toBe(true);
    });
});

describe("changedAt", () => {
    it("says when the file changed, and nothing when that is not known", () => {
        expect(changedAt(sheet({ changed_at: null }))).toBe("");
        expect(changedAt(sheet())).toMatch(/2023/);
    });
});

// The UI's source, as text: every module but the tests.
const sources = import.meta.glob<string>(["./**/*.{ts,tsx}", "!./**/*.test.ts"], {
    query: "?raw",
    import: "default",
    eager: true,
});

describe("the app's own stylesheets", () => {
    it("are all imported in main.tsx, so none can come after the user's", () => {
        // A stylesheet imported by a module that loads later is put in the head as it loads,
        // after the user's, and would win over it.
        const importing = Object.entries(sources)
            .filter(([, text]) => /^import\s+["'][^"']+\.css["'];?$/m.test(text))
            .map(([path]) => path);
        expect(importing).toEqual(["./main.tsx"]);
    });
});
