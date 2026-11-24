import { describe, expect, it } from "vitest";
import {
    envFromText,
    environmentChoice,
    environmentOptions,
    environmentShown,
    envToText,
    OWN_SETUP,
    portsFromText,
    portsToText,
    setupsFromText,
    stacksFromText,
    stacksToText,
    taskTooltip,
} from "./environments";

describe("an environment's variables", () => {
    it("are typed as lines, the way a .env file has them", () => {
        const text = [
            "# the database",
            "DATABASE_URL=postgres://localhost/app?a=b",
            "",
            'export GREETING="hello there"',
            "EMPTY=",
            " SPACED = 'x' ",
        ].join("\n");
        expect(envFromText(text)).toEqual({
            DATABASE_URL: "postgres://localhost/app?a=b",
            GREETING: "hello there",
            EMPTY: "",
            SPACED: "x",
        });
        expect(envFromText("")).toEqual({});
    });

    it("say which line is not a variable", () => {
        expect(() => envFromText("A=1\njust words")).toThrow('"just words" is not a variable');
        expect(() => envFromText("=nothing")).toThrow("is not a variable");
    });

    it("are shown again as they are kept", () => {
        expect(envToText({ A: "1", B: "two words" })).toBe("A=1\nB=two words");
        expect(envFromText(envToText({ A: "1", B: "x=y" }))).toEqual({ A: "1", B: "x=y" });
    });
});

describe("an environment's limit on setups at once", () => {
    it("is a whole number from 0, which is no limit, or nothing for the repository's own", () => {
        expect(setupsFromText(" 2 ")).toBe(2);
        expect(setupsFromText("0")).toBe(0);
        expect(setupsFromText("")).toBeUndefined();
        expect(() => setupsFromText("-1")).toThrow('"-1" is not a number of setups');
        expect(() => setupsFromText("1.5")).toThrow("is not a number of setups");
    });
});

describe("an environment's stack limit", () => {
    it("is a whole number typed, or nothing for the repository's own", () => {
        expect(stacksFromText(" 3 ")).toBe(3);
        expect(stacksFromText("")).toBeUndefined();
        expect(stacksFromText("   ")).toBeUndefined();
    });

    it("lets at least one stack run", () => {
        expect(stacksFromText("1")).toBe(1);
        expect(() => stacksFromText("0")).toThrow('"0" is not a number of stacks');
        expect(() => stacksFromText("-2")).toThrow("is not a number of stacks");
    });

    it("is refused when it is no whole number the core can keep", () => {
        expect(() => stacksFromText("2.5")).toThrow("is not a number of stacks");
        expect(() => stacksFromText("many")).toThrow("is not a number of stacks");
        expect(stacksFromText("4294967295")).toBe(4294967295);
        expect(() => stacksFromText("4294967296")).toThrow("is not a number of stacks");
    });

    it("is shown again as it is kept", () => {
        expect(stacksToText(5)).toBe("5");
        expect(stacksToText(undefined)).toBe("");
        expect(stacksFromText(stacksToText(7))).toBe(7);
    });
});

describe("a new task's environment", () => {
    const names = ["docker", "staging"];

    it("is the repository's default until another is picked", () => {
        expect(environmentShown(names, "docker", undefined)).toBe("docker");
        expect(environmentChoice(names, undefined)).toBe("default");
        expect(environmentShown(names, "docker", "staging")).toBe("staging");
        expect(environmentChoice(names, "staging")).toEqual({ named: "staging" });
    });

    it("can be the repository's own setup, whatever its default", () => {
        expect(environmentShown(names, "docker", OWN_SETUP)).toBe(OWN_SETUP);
        expect(environmentChoice(names, OWN_SETUP)).toBe("own");
        expect(environmentShown(names, null, undefined)).toBe(OWN_SETUP);
    });

    it("goes back to the default when the one picked or the default is gone", () => {
        expect(environmentShown(names, "docker", "removed")).toBe("docker");
        expect(environmentChoice(names, "removed")).toBe("default");
        expect(environmentShown(names, "removed", undefined)).toBe(OWN_SETUP);
    });

    it("is listed with the default one marked", () => {
        expect(environmentOptions(names, "docker")).toEqual([
            ["docker", "docker (default)"],
            ["staging", "staging"],
        ]);
        expect(environmentOptions(names, null)).toEqual([
            ["docker", "docker"],
            ["staging", "staging"],
        ]);
    });
});

describe("taskTooltip", () => {
    it("names the environment a task was set up in, if any", () => {
        const task = { name: "one", branch: "me/one", environment: null };
        expect(taskTooltip(task)).toBe("Worktree one, on me/one");
        expect(taskTooltip({ ...task, environment: "staging" })).toBe(
            "Worktree one, on me/one, set up in staging",
        );
    });
});

describe("an environment's ports", () => {
    it("are typed as lines of a name and a number", () => {
        expect(portsFromText("web=3000\n# the database\ndb = 5432")).toEqual({
            web: 3000,
            db: 5432,
        });
        expect(portsToText({ web: 3000, db: 5432 })).toBe("web=3000\ndb=5432");
        expect(portsFromText("")).toEqual({});
    });

    it("say which line holds no port", () => {
        expect(() => portsFromText("web=three")).toThrow('"web=three" is not a port');
        expect(() => portsFromText("web=70000")).toThrow("is not a port");
    });
});
