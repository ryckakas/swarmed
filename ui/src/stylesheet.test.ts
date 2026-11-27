import { describe, expect, it } from "vitest";
import type { Stylesheet } from "./api/types";
import {
    changedLines,
    changeSaid,
    drawUserStylesheet,
    NOTHING_PUT_ASIDE,
    stylesheetNotice,
} from "./stylesheet";

const sheet = (over: Partial<Stylesheet> = {}): Stylesheet => ({
    on: true,
    left_out: false,
    applied: "a {}",
    accepted: "a {}",
    file: "a {}",
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
