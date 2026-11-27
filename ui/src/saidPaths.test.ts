import { describe, expect, it } from "vitest";
import { fileAddress, inWorktree, pathsIn } from "./saidPaths";

/** A path in the pieces, written as `shown`. */
function path(written: string, line?: number, column?: number) {
    const shown = [written, line, column].filter((part) => part !== undefined).join(":");
    return {
        path: written,
        shown,
        ...(line !== undefined && { line }),
        ...(column !== undefined && { column }),
    };
}

describe("pathsIn", () => {
    it("finds an absolute path of two parts or more", () => {
        expect(pathsIn("Open /Users/me/swarmed/task/ui/src/setup.ts now")).toEqual([
            "Open ",
            path("/Users/me/swarmed/task/ui/src/setup.ts"),
            " now",
        ]);
        expect(pathsIn("/etc/hosts")).toEqual([path("/etc/hosts")]);
        expect(pathsIn("/Users/me/")).toEqual([path("/Users/me/")]);
    });

    it("finds a path from home", () => {
        expect(pathsIn("in ~/swarmed/swarmed/task")).toEqual([
            "in ",
            path("~/swarmed/swarmed/task"),
        ]);
        expect(pathsIn("~/.zshrc")).toEqual([path("~/.zshrc")]);
    });

    it("finds a relative path with a slash and a file name", () => {
        for (const written of [
            "ui/src/setup.ts",
            "crates/core/src/files.rs",
            ".github/workflows/ci.yml",
            "config/.env",
            "app/[id]/page.tsx",
            "ui/src/said.test.ts",
        ]) {
            expect(pathsIn(`see ${written} first`)).toEqual(["see ", path(written), " first"]);
        }
    });

    it("finds a relative path that starts with a dot, whatever its end", () => {
        expect(pathsIn("./scripts/a.sh")).toEqual([path("./scripts/a.sh")]);
        expect(pathsIn("../other/bin")).toEqual([path("../other/bin")]);
        expect(pathsIn("./bin")).toEqual([path("./bin")]);
    });

    it("takes a line, and a line and a column, after a path", () => {
        expect(pathsIn("crates/core/src/files.rs:85")).toEqual([
            path("crates/core/src/files.rs", 85),
        ]);
        expect(pathsIn("at /tmp/x/setup.ts:12:5.")).toEqual([
            "at ",
            path("/tmp/x/setup.ts", 12, 5),
            ".",
        ]);
        expect(pathsIn("ui/a.ts:12: fails")).toEqual([path("ui/a.ts", 12), ": fails"]);
        expect(pathsIn("ui/a.ts:x")).toEqual([path("ui/a.ts"), ":x"]);
    });

    it("leaves out the punctuation that ends a sentence", () => {
        for (const end of [".", ",", ";", ":", "!", "?", "...", ")", "]", "'", '"']) {
            expect(pathsIn(`Look at ui/src/setup.ts${end}`)).toEqual([
                "Look at ",
                path("ui/src/setup.ts"),
                end,
            ]);
        }
    });

    it("finds a path inside brackets, quotes and emphasis", () => {
        for (const [open, close] of [
            ["(", ")"],
            ["[", "]"],
            ['"', '"'],
            ["'", "'"],
            ["**", "**"],
            ["<", ">"],
        ]) {
            expect(pathsIn(`${open}ui/a.ts${close}`)).toEqual([open, path("ui/a.ts"), close]);
        }
        expect(pathsIn("--config=ui/a.json")).toEqual(["--config=", path("ui/a.json")]);
    });

    it("trims a long run of closing brackets in one pass, not a count per bracket", () => {
        const brackets = "]".repeat(20_000);
        const started = performance.now();
        expect(pathsIn(`ui/a.ts${brackets}`)).toEqual([path("ui/a.ts"), brackets]);
        // Counted again for each bracket, this took over a second.
        expect(performance.now() - started).toBeLessThan(200);
    });

    it("leaves words with a slash that name no file as words", () => {
        for (const text of [
            "and/or",
            "1/2",
            "11/26",
            "2026/11/26",
            "TCP/IP",
            "client/server.",
            "w/o",
            "v1.2/v1.3",
            "/login",
            "/1/2",
            "ui/src/",
            "ui//a.ts",
            "//a",
            "~/",
            "./",
            "Cargo.toml",
            "km /h",
        ]) {
            expect(pathsIn(text)).toEqual([text]);
        }
    });

    it("finds no path inside a word", () => {
        for (const text of ["x/etc/hosts", "a:/etc/hosts", "localhost:3000/login"]) {
            expect(pathsIn(text).some((piece) => typeof piece !== "string")).toBe(false);
        }
    });

    it("finds every path in a message", () => {
        expect(pathsIn("Compare ui/a.ts with /tmp/b/c.ts,\nthen ~/d")).toEqual([
            "Compare ",
            path("ui/a.ts"),
            " with ",
            path("/tmp/b/c.ts"),
            ",\nthen ",
            path("~/d"),
        ]);
    });
});

describe("fileAddress", () => {
    it("is the file's address, its characters encoded", () => {
        expect(fileAddress("/Users/me/task/", "app/[id]/100%.ts")).toBe(
            "file:///Users/me/task/app/%5Bid%5D/100%25.ts",
        );
    });
});

describe("inWorktree", () => {
    const worktree = "/Users/me/swarmed/swarmed/task";

    it("takes a relative path as the worktree's", () => {
        expect(inWorktree("ui/src/setup.ts", worktree)).toBe("ui/src/setup.ts");
        expect(inWorktree("./scripts/a.sh", worktree)).toBe("scripts/a.sh");
        expect(inWorktree("ui/../docs/a.md", worktree)).toBe("docs/a.md");
    });

    it("takes an absolute path under the worktree from its top", () => {
        expect(inWorktree(`${worktree}/ui/src/setup.ts`, worktree)).toBe("ui/src/setup.ts");
        expect(inWorktree(`${worktree}/ui/src/setup.ts`, `${worktree}/`)).toBe("ui/src/setup.ts");
    });

    it("refuses a path that leads out of the worktree", () => {
        expect(inWorktree("../other/a.ts", worktree)).toBeNull();
        expect(inWorktree("ui/../../a.ts", worktree)).toBeNull();
        expect(inWorktree("/Users/me/swarmed/swarmed/task-2/a.ts", worktree)).toBeNull();
        expect(inWorktree("/etc/hosts", worktree)).toBeNull();
        expect(inWorktree(`${worktree}/../a.ts`, worktree)).toBeNull();
    });

    it("refuses the worktree itself, a path from home, and any path with no worktree", () => {
        expect(inWorktree(worktree, worktree)).toBeNull();
        expect(inWorktree(`${worktree}/`, worktree)).toBeNull();
        expect(inWorktree("~/swarmed/swarmed/task/a.ts", worktree)).toBeNull();
        expect(inWorktree("/a/b.ts", "")).toBeNull();
    });
});
