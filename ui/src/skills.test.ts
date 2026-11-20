import { describe, expect, it } from "vitest";
import type { Skill } from "./api/types";
import { skillCount, skillsOf } from "./skills";

const skill = (origin: string, name: string, shared = true): Skill => ({
    origin,
    name,
    description: `Use ${name}.`,
    folder: `/skills/${name}`,
    shared,
});

describe("skills in the settings", () => {
    const all = [
        skill("swarmed", "house-style"),
        skill("claude", "review"),
        skill("codex", "deploy", false),
    ];

    it("are grouped by whose they are", () => {
        expect(skillsOf(all, "claude").map((one) => one.name)).toEqual(["review"]);
        expect(skillsOf(all, "opencode")).toEqual([]);
    });

    it("are counted, with those kept back from other agents", () => {
        expect(skillCount(skillsOf(all, "claude"))).toBe("1");
        expect(skillCount(skillsOf(all, "codex"))).toBe("1, 1 kept back");
        expect(skillCount([])).toBe("0");
    });
});
