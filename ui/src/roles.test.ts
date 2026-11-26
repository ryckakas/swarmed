import { describe, expect, it } from "vitest";
import { createMockApi, sampleProjects } from "./api/mock";
import { MOCK_ROLES } from "./api/mockRoles";
import { roleAbout, roleChangedSaid, roleChosen, roleOptions } from "./roles";
import { applySessionEvent, findSession } from "./tree";
import { wordsFor } from "./wording";

const plain = wordsFor("plain");
const swarm = wordsFor("swarm");

describe("the role menu", () => {
    it("offers none, then every role, and keeps one it does not know", () => {
        expect(roleOptions(MOCK_ROLES, null, plain)).toEqual([
            ["", "No role"],
            ["architect", "Architect"],
            ["planner", "Planner"],
            ["implementer", "Implementer"],
            ["reviewer", "Reviewer"],
        ]);
        const kept = roleOptions(MOCK_ROLES, "tester", plain);
        expect(kept[1]).toEqual(["tester", "Tester"]);
        expect(kept).toHaveLength(6);
        expect(roleOptions(MOCK_ROLES, "planner", swarm)[2]).toEqual(["planner", "Broodkeeper"]);
    });

    it("reads its choice back, none included", () => {
        expect(roleChosen("")).toBeNull();
        expect(roleChosen("reviewer")).toBe("reviewer");
    });

    it("says what a role is for, and a role it does not know by its name", () => {
        expect(roleAbout("planner", MOCK_ROLES, plain)).toMatch(/^Planner: plans one part/);
        expect(roleAbout("tester", MOCK_ROLES, plain)).toBe("Tester");
    });
});

describe("a change of role", () => {
    it("says the role from here on and the one before", () => {
        const change = { role: "implementer", previous: "reviewer" };
        expect(roleChangedSaid(change, plain)).toBe(
            "Implementer from here on, no longer Reviewer.",
        );
        expect(roleChangedSaid({ role: null, previous: "planner" }, plain)).toBe(
            "No role from here on, no longer Planner.",
        );
        expect(roleChangedSaid({ role: "architect", previous: null }, swarm)).toBe(
            "Queen from here on.",
        );
    });

    it("reaches the session in the tree", () => {
        const changed = applySessionEvent(sampleProjects(), {
            session_id: 1,
            status: null,
            event: { type: "role_changed", role: "planner", previous: null },
        });
        expect(findSession(changed, 1)?.role).toBe("planner");
        expect(findSession(changed, 2)?.role).toBeNull();
    });

    it("words a role's short form for a row in every style", () => {
        expect(plain.roleShort("implementer")).toBe("impl");
        expect(swarm.roleShort("reviewer")).toBe("guard");
        expect(plain.roleShort("tester")).toBe("tester");
    });
});

describe("the sample's roles", () => {
    it("are set as the core sets them, recorded only once something was said", async () => {
        const api = createMockApi();
        const tree = await api.overview();
        const planner = findSession(tree, 13);
        expect(planner?.role).toBe("planner");
        expect(findSession(tree, 9)?.role).toBe("architect");
        await expect(api.sessionSetRole(13, "boss")).rejects.toThrow("no role called `boss`");
        const reviewer = await api.sessionSetRole(13, "reviewer");
        expect(reviewer.role).toBe("reviewer");
        expect(await api.roles()).toEqual(MOCK_ROLES);
    });
});
