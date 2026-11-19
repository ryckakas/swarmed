import { describe, expect, it } from "vitest";
import { parseDiff, splitPath } from "./diff";

const DIFF = `diff --git a/src/auth/login.ts b/src/auth/login.ts
index 1a2b3c4..5d6e7f8 100644
--- a/src/auth/login.ts
+++ b/src/auth/login.ts
@@ -10,4 +10,5 @@ export function login() {
 const a = 1;
-const b = 2;
+const b = 3;
+const c = 4;
 return a;
\\ No newline at end of file
`;

describe("parseDiff", () => {
    it("numbers the lines of both sides and drops the file header", () => {
        expect(parseDiff(DIFF)).toEqual([
            { kind: "hunk", text: "export function login() {" },
            { kind: "context", text: "const a = 1;", oldLine: 10, newLine: 10 },
            { kind: "removed", text: "const b = 2;", oldLine: 11 },
            { kind: "added", text: "const b = 3;", newLine: 11 },
            { kind: "added", text: "const c = 4;", newLine: 12 },
            { kind: "context", text: "return a;", oldLine: 12, newLine: 13 },
            { kind: "note", text: "No newline at end of file" },
        ]);
    });

    it("keeps what git says instead of lines", () => {
        const binary =
            "diff --git a/logo.png b/logo.png\nBinary files a/logo.png and b/logo.png differ\n";
        expect(parseDiff(binary)).toEqual([
            { kind: "note", text: "Binary files a/logo.png and b/logo.png differ" },
        ]);
    });

    it("reads a new file's diff, which starts at line one", () => {
        const added =
            "diff --git a/n b/n\nnew file\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+one\n+two\n";
        expect(parseDiff(added).slice(1)).toEqual([
            { kind: "added", text: "one", newLine: 1 },
            { kind: "added", text: "two", newLine: 2 },
        ]);
    });
});

describe("splitPath", () => {
    it("separates the folder from the file name", () => {
        expect(splitPath("src/auth/login.ts")).toEqual({ folder: "src/auth/", name: "login.ts" });
        expect(splitPath("README.md")).toEqual({ folder: "", name: "README.md" });
    });
});
