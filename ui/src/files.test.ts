import { describe, expect, it } from "vitest";
import type { FileListing } from "./api/types";
import { fileRows, leftOut, openable, opensInEditor, pathIn, toggled } from "./files";

const listing = (...names: string[]): FileListing => ({
    entries: names.map((name) => ({ name: name.replace(/\/$/, ""), dir: name.endsWith("/") })),
    more: 0,
});

describe("fileRows", () => {
    const listings = {
        "": listing("src/", "docs/", "README.md"),
        src: listing("auth/", "main.ts"),
        "src/auth": listing("login.ts"),
    };

    it("shows the top folder alone until a folder is unfolded", () => {
        const rows = fileRows(listings, new Set());
        expect(rows.map((row) => row.path)).toEqual(["src", "docs", "README.md"]);
        expect(rows.every((row) => row.depth === 0 && !row.open)).toBe(true);
    });

    it("puts what an unfolded folder holds under it, one step in", () => {
        const rows = fileRows(listings, new Set(["src", "src/auth"]));
        expect(rows.map((row) => `${row.depth} ${row.path}`)).toEqual([
            "0 src",
            "1 src/auth",
            "2 src/auth/login.ts",
            "1 src/main.ts",
            "0 docs",
            "0 README.md",
        ]);
    });

    it("shows nothing under a folder that is unfolded and not read yet, or under a hidden one", () => {
        expect(fileRows(listings, new Set(["docs"])).map((row) => row.path)).toEqual([
            "src",
            "docs",
            "README.md",
        ]);
        // Unfolded once, then its parent folded: it stays out of sight.
        expect(fileRows(listings, new Set(["src/auth"]))).toHaveLength(3);
    });
});

describe("the helpers", () => {
    it("join a path, fold and unfold, and say what a long folder leaves out", () => {
        expect(pathIn("", "src")).toBe("src");
        expect(pathIn("src", "main.ts")).toBe("src/main.ts");
        expect([...toggled(new Set(["a"]), "b")]).toEqual(["a", "b"]);
        expect([...toggled(new Set(["a", "b"]), "a")]).toEqual(["b"]);
        expect(leftOut(listing("a"))).toBeNull();
        expect(leftOut({ entries: [], more: 12 })).toContain("12 more");
    });
});

describe("opening a file", () => {
    it("names the file it opens", () => {
        expect(opensInEditor("src/auth/login.ts")).toBe("Opens src/auth/login.ts in your editor");
    });

    it("offers an added or changed file, never a deleted one", () => {
        expect(openable({ kind: "added" })).toBe(true);
        expect(openable({ kind: "modified" })).toBe(true);
        expect(openable({ kind: "deleted" })).toBe(false);
    });
});
