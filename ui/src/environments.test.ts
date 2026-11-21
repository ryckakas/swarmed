import { describe, expect, it } from "vitest";
import { envFromText, envToText, portsFromText, portsToText, taskTooltip } from "./environments";

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
