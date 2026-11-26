import { describe, expect, it } from "vitest";
import { createMockApi, sampleProjects } from "./api/mock";
import { MOCK_ROLES } from "./api/mockRoles";
import {
    heldSaid,
    keepsPlanning,
    planKeptSaid,
    roleAbout,
    roleChangedSaid,
    roleChosen,
    roleHolds,
    roleOptions,
} from "./roles";
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

describe("what a role keeps its session to", () => {
    it("holds a role that changes no files in plan mode, and no other", () => {
        expect(roleHolds("planner", MOCK_ROLES)).toBe("plan");
        expect(roleHolds("reviewer", MOCK_ROLES)).toBe("plan");
        expect(roleHolds("implementer", MOCK_ROLES)).toBeNull();
        expect(roleHolds(null, MOCK_ROLES)).toBeNull();
        expect(roleHolds("tester", MOCK_ROLES)).toBeNull();
    });

    it("keeps such a role planning once its plan is approved", () => {
        expect(keepsPlanning("architect", MOCK_ROLES)).toBe(true);
        expect(keepsPlanning("implementer", MOCK_ROLES)).toBe(false);
        expect(keepsPlanning(null, MOCK_ROLES)).toBe(false);
        expect(plain.planKept("Planner")).toMatch(/^Planner: once approved, it stays in plan mode/);
        expect(swarm.roleHolds("Broodkeeper")).toMatch(/once the spawn hatches\.$/);
    });

    it("says so in the new-task view's menu and the plan's prompt", () => {
        expect(heldSaid("planner", MOCK_ROLES, plain)).toMatch(/^Planner: starts in plan mode/);
        expect(heldSaid("implementer", MOCK_ROLES, plain)).toBeNull();
        expect(heldSaid(null, MOCK_ROLES, plain)).toBeNull();
        expect(planKeptSaid("reviewer", MOCK_ROLES, swarm)).toMatch(/^Sentinel: once approved/);
        expect(planKeptSaid("implementer", MOCK_ROLES, plain)).toBeNull();
    });
});

describe("the sample's roles", () => {
    it("hold a session in plan mode and put back what it had, unless the user chose", async () => {
        const api = createMockApi();
        const tree = await api.overview();
        expect(findSession(tree, 13)?.supervision).toBe("plan");
        expect(findSession(tree, 10)?.supervision).toBe("ask");
        expect((await api.sessionSetRole(10, "reviewer")).supervision).toBe("plan");
        expect((await api.sessionSetRole(10, "implementer")).supervision).toBe("ask");
        await api.sessionSetRole(10, "planner");
        const settings = { model: null, supervision: "auto" as const, effort: null, fast: false };
        await api.sessionConfigure(10, settings);
        expect((await api.sessionSetRole(10, null)).supervision).toBe("auto");
    });

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
