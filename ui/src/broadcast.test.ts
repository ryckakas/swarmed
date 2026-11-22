import { describe, expect, it } from "vitest";
import type { AgentInfo, FileChange } from "./api/types";
import {
    compared,
    totals,
    variantChoices,
    variantFrom,
    variantKey,
    variantLabel,
    variantNames,
} from "./broadcast";

const agents: AgentInfo[] = [
    {
        id: "claude",
        name: "Claude Code",
        models: ["opus", "sonnet"],
        default_model: null,
        hears_mid_turn: true,
    },
    {
        id: "codex",
        name: "Codex",
        models: ["gpt-6.2-sol"],
        default_model: null,
        hears_mid_turn: false,
    },
];
const change = (
    path: string,
    added: number | null = 1,
    removed: number | null = 0,
): FileChange => ({
    path,
    kind: "modified",
    added,
    removed,
});

describe("variants", () => {
    it("offers every agent with its default and each model, without what is taken", () => {
        const taken = [
            { agent: "claude", model: "" },
            { agent: "codex", model: "gpt-6.2-sol" },
        ];
        expect(variantChoices(agents, taken)).toEqual([
            { agent: "claude", model: "opus" },
            { agent: "claude", model: "sonnet" },
            { agent: "codex", model: "" },
        ]);
    });

    it("survives being a menu value", () => {
        const variant = { agent: "codex", model: "gpt-6.2-sol" };
        expect(variantFrom(variantKey(variant))).toEqual(variant);
        expect(variantFrom(variantKey({ agent: "codex", model: "" }))).toEqual({
            agent: "codex",
            model: "",
        });
    });

    it("is shown by the agent's name and the model", () => {
        expect(variantLabel({ agent: "codex", model: "gpt-6.2-sol" }, agents)).toBe(
            "Codex, gpt-6.2-sol",
        );
        expect(variantLabel({ agent: "codex", model: "" }, agents)).toBe("Codex");
        expect(variantLabel({ agent: "other", model: "" }, agents)).toBe("other");
    });

    it("names the variants' tasks after the first one, without clashes", () => {
        const variants = [
            { agent: "codex", model: "" },
            { agent: "codex", model: "gpt-6.2-sol" },
            { agent: "claude", model: "sonnet" },
        ];
        expect(variantNames("fix-login", variants, ["fix-login-claude"])).toEqual([
            "fix-login-codex",
            "fix-login-codex-2",
            "fix-login-claude-2",
        ]);
    });
});

describe("compared", () => {
    it("lists every file either task changed, with what each did", () => {
        const rows = compared(
            [change("b.ts"), change("a.ts")],
            [change("b.ts", 5, 2), change("c.ts")],
        );
        expect(rows.map((row) => row.path)).toEqual(["a.ts", "b.ts", "c.ts"]);
        expect(rows[0]).toMatchObject({ right: null });
        expect(rows[1]?.left?.added).toBe(1);
        expect(rows[1]?.right?.added).toBe(5);
        expect(rows[2]).toMatchObject({ left: null });
    });

    it("sums a task's changes, counting a binary file as a file only", () => {
        expect(totals([change("a", 3, 1), change("b", null, null), change("c", 2, 2)])).toEqual({
            files: 3,
            added: 5,
            removed: 3,
        });
    });
});
