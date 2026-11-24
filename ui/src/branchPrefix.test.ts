import { describe, expect, it } from "vitest";
import { prefixToSave, problemOf } from "./branchPrefix";

describe("prefixToSave", () => {
    it("keeps what was typed once the core has nothing against it", () => {
        expect(prefixToSave("jd", "", { prefix: "jd", branch: "jd/fix-login" })).toBe("jd");
        // Not checked yet: the core checks it as it keeps it.
        expect(prefixToSave("jd", "", null)).toBe("jd");
    });

    it("keeps nothing when nothing changed", () => {
        expect(prefixToSave(null, "jd", null)).toBeNull();
        expect(prefixToSave("jd", "jd", { prefix: "jd", branch: "jd/fix-login" })).toBeNull();
    });

    it("keeps an emptied prefix, which stands for the login", () => {
        expect(prefixToSave("", "jd", { prefix: "", branch: "you/fix-login" })).toBe("");
    });

    it("keeps nothing the core refused, and is not held back by a refusal of earlier text", () => {
        const refused = { prefix: "a b", problem: "not a valid branch name" };
        expect(prefixToSave("a b", "jd", refused)).toBeNull();
        expect(prefixToSave("ab", "jd", refused)).toBe("ab");
    });
});

describe("problemOf", () => {
    it("says what the core said, or what was thrown", () => {
        expect(problemOf("the branch prefix `a b` cannot be used")).toBe(
            "the branch prefix `a b` cannot be used",
        );
        expect(problemOf(new Error("refused"))).toBe("refused");
    });
});
