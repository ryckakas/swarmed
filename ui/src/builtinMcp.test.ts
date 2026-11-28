import { describe, expect, it } from "vitest";
import type { BuiltinMcpView, BuiltinRegistration, DoctorCheck } from "./api/types";
import {
    actionLabel,
    busyLine,
    checksLine,
    checkTone,
    existingLine,
    failedLine,
    heldLine,
    hookNoted,
    leftLine,
    offered,
    oldServerLine,
    printsOutput,
    registrationPlace,
    registrationStep,
    restartOffered,
    shownFix,
    startsFresh,
    stateLabel,
    stateTone,
    statusMark,
    summaryLine,
    trashOffered,
    unpickedLine,
} from "./builtinMcp";

function view(change: Partial<BuiltinMcpView> = {}): BuiltinMcpView {
    return {
        name: "repolore",
        state: "on",
        version: "0.5.1",
        minimum: "0.5.1",
        install_command: "uv tool install repolore-mcp",
        home: "/data/repolore",
        home_ready: true,
        existing: null,
        registrations: [],
        old_server: null,
        sessions: [],
        hook: false,
        checks: [],
        checked: null,
        status: null,
        page_open: false,
        problem: null,
        ...change,
    };
}

const passing: DoctorCheck = { name: "index", ok: true, warning: false, detail: "", fix: null };
const warned: DoctorCheck = {
    name: "github",
    ok: true,
    warning: true,
    detail: "gh is signed out",
    fix: "gh auth login",
};
const failing: DoctorCheck = {
    name: "model",
    ok: false,
    warning: false,
    detail: "not downloaded",
    fix: "repolore refresh",
};
const existing = { folders: ["/Users/you/.config/repolore"], indexed: true };

function registration(change: Partial<BuiltinRegistration> = {}): BuiltinRegistration {
    return {
        key: "claude:user",
        client: "Claude Code",
        scope: "user",
        command: "claude mcp remove repolore -s user",
        runs_here: true,
        step: null,
        ...change,
    };
}

describe("a built-in's state", () => {
    it("is named, and coloured by whether it wants something", () => {
        expect(stateLabel("missing")).toBe("not installed");
        expect(stateLabel("outdated")).toBe("out of date");
        expect(stateTone("on")).toBe("good");
        expect(stateTone("waiting")).toBe("off");
        expect(stateTone("outdated")).toBe("off");
        expect(stateTone("off")).toBe("plain");
    });

    it("says which version is too old, and which is needed", () => {
        const old = view({ state: "outdated", version: "0.5.0" });
        expect(summaryLine(old)).toBe(
            "repolore 0.5.0 is installed, and Swarmed needs 0.5.1 or later.",
        );
        expect(summaryLine({ ...old, version: null })).toContain("needs 0.5.1 or later");
    });

    it("offers a copy while off only for a setup of the user's not copied yet", () => {
        const fresh = view({ state: "off", home_ready: false });
        expect(offered(fresh)).toEqual(["turn-on"]);
        expect(summaryLine(fresh)).toContain("starts fresh");
        const found = view({ state: "off", home_ready: false, existing });
        expect(offered(found)).toEqual(["fresh", "copy"]);
        expect(summaryLine(found)).toContain("Yours is not changed");
        const kept = view({ state: "off", home_ready: true, existing });
        expect(offered(kept)).toEqual(["turn-on"]);
        expect(summaryLine(kept)).toContain("ready to turn on again");
    });

    it("introduces the user's own setup's folders, saying whether it is indexed", () => {
        expect(existingLine(existing)).toBe("Your setup's folder, with its index:");
        expect(existingLine({ folders: ["/a", "/b"], indexed: false })).toBe(
            "Your setup's folders, not indexed yet:",
        );
    });

    it("offers what each state can do next, the main one last", () => {
        expect(offered(view({ state: "missing" }))).toEqual(["install"]);
        expect(offered(view({ state: "outdated" }))).toEqual(["update"]);
        expect(offered(view({ state: "waiting" }))).toEqual(["turn-off", "check-again"]);
        // A fresh start waits with nothing chosen: its page stays in reach to pick that.
        expect(offered(view({ state: "waiting", home_ready: false }))).toEqual([
            "turn-off",
            "check-again",
            "open",
        ]);
        expect(offered(view({ state: "on" }))).toEqual(["turn-off", "check", "open"]);
        expect(actionLabel("open", "repolore")).toEqual({
            idle: "Open repolore",
            busy: "Opening repolore…",
        });
        expect(actionLabel("check-again", "repolore").busy).toBe("Checking…");
    });

    it("shows a step's own output while it prints, and a line saying what runs otherwise", () => {
        expect(printsOutput("install")).toBe(true);
        expect(printsOutput("remove")).toBe(true);
        expect(printsOutput("check")).toBe(false);
        expect(busyLine("check", "repolore")).toBe("Checking…");
        expect(busyLine("copy", "repolore")).toBe("Copying your setup…");
        expect(busyLine("restart", "repolore")).toBe("Restarting the idle agents…");
        expect(busyLine("trash", "repolore")).toBe("Moving to the Trash…");
    });

    it("says what is left after a step failed", () => {
        expect(failedLine("copy", "a refresh is running; try again when it is done")).toBe(
            "The copy failed: a refresh is running; try again when it is done. Nothing was changed: copy again, or start fresh.",
        );
        expect(failedLine("remove", "codex exited with 1.\n")).toBe(
            "Removing it failed: codex exited with 1. Run the command shown yourself, then check again.",
        );
        expect(failedLine("check", "the doctor timed out")).toBe("the doctor timed out.");
    });

    it("opens the server's page only after a fresh start", () => {
        expect(startsFresh(view({ home_ready: false }), false)).toBe(true);
        expect(startsFresh(view({ home_ready: false }), true)).toBe(false);
        expect(startsFresh(view({ home_ready: true }), false)).toBe(false);
    });
});

describe("what a built-in waits for", () => {
    it("names the registrations and the old server that hold it back", () => {
        const waiting = view({ state: "waiting" });
        expect(leftLine({ ...waiting, registrations: [registration()] })).toBe(
            "On, but not given to sessions while 1 registration outside Swarmed remains, so that only one repolore runs at a time.",
        );
        expect(
            leftLine({
                ...waiting,
                registrations: [registration(), registration({ key: "codex" })],
                old_server: { pid: 41, relays: 1 },
            }),
        ).toContain(
            "2 registrations outside Swarmed remain and the old shared server is still in use",
        );
        expect(leftLine(waiting)).toContain("Check again");
    });

    it("runs a command shown first, or leaves a step to the user", () => {
        expect(registrationStep(registration())).toEqual({
            kind: "run",
            command: "claude mcp remove repolore -s user",
        });
        const opencode = registration({
            client: "OpenCode",
            command: null,
            runs_here: false,
            step: "Take repolore out of opencode.json",
        });
        expect(registrationStep(opencode)).toEqual({
            kind: "user",
            text: "Take repolore out of opencode.json",
            command: null,
        });
        expect(registrationStep({ ...opencode, step: null })).toEqual({
            kind: "user",
            text: "Take it out of OpenCode yourself.",
            command: null,
        });
        // A command Swarmed may not run is shown for the user to run by hand.
        const cursor = registration({ client: "Cursor", runs_here: false, command: "cursor x" });
        expect(registrationStep(cursor)).toMatchObject({ kind: "user", command: "cursor x" });
        // One Swarmed would run but that has no command is the user's too.
        expect(registrationStep(registration({ command: null })).kind).toBe("user");
        expect(registrationPlace(registration())).toBe("Claude Code, user scope");
        expect(registrationPlace(registration({ scope: "" }))).toBe("Claude Code");
    });

    it("says whether the old server runs, and how many sessions are connected to it", () => {
        expect(oldServerLine({ pid: 4242, relays: 2 })).toBe(
            "The old shared server (process 4242) runs, with 2 sessions connected. It stops by itself 10 minutes after the last one ends.",
        );
        expect(oldServerLine({ pid: 4242, relays: 1 })).toContain("with 1 session connected");
        expect(oldServerLine({ pid: 4242, relays: 0 })).toContain("with no session connected");
        // Stopped, it is still in use: each session connected would start it again.
        expect(oldServerLine({ pid: null, relays: 2 })).toBe(
            "The old shared server is stopped, but 2 sessions are still connected to it and would start it again.",
        );
        expect(oldServerLine({ pid: null, relays: 1 })).toContain(
            "but 1 session is still connected",
        );
    });

    it("names the sessions on the old server, and whether restarting them helps now", () => {
        const one = [{ id: 4, title: "Fix the login", idle: true }];
        expect(heldLine(one, "session", "New session")).toBe(
            "“Fix the login” still uses the old server. It is idle: restarting lets go of the old server, and the next message starts the agent again.",
        );
        expect(heldLine([{ id: 4, title: "", idle: false }], "session", "New session")).toBe(
            "“New session” still uses the old server. It is at work now; once it is idle, restarting its agent lets go of the old server.",
        );
        const three = [
            { id: 1, title: "A", idle: true },
            { id: 2, title: "B", idle: false },
            { id: 3, title: "C", idle: false },
        ];
        expect(heldLine(three, "spawn", "New spawn")).toBe(
            "3 spawns still use the old server: “A”, “B” and “C”. 1 of them is idle: restarting lets go of the old server, and the next message starts the agent again.",
        );
        expect(restartOffered(three)).toBe(true);
        const busy = three.map((held) => ({ ...held, idle: false }));
        expect(heldLine(busy, "session", "")).toContain("None is idle now");
        expect(restartOffered(busy)).toBe(false);
    });

    it("points out a refresh hook only once the built-in is turned on", () => {
        expect(hookNoted(view({ state: "off", hook: true }))).toBe(false);
        expect(hookNoted(view({ state: "waiting", hook: true }))).toBe(true);
        expect(hookNoted(view({ state: "on", hook: true }))).toBe(true);
        expect(hookNoted(view({ state: "on", hook: false }))).toBe(false);
    });
});

describe("a built-in that is on", () => {
    it("tells passing, warning and failing checks apart, and shows a fix for the last two", () => {
        expect(checkTone(passing)).toBe("good");
        expect(checkTone(warned)).toBe("warning");
        expect(checkTone(failing)).toBe("failing");
        expect(shownFix(passing)).toBeNull();
        expect(shownFix(warned)).toBe("gh auth login");
        expect(shownFix(failing)).toBe("repolore refresh");
    });

    it("sums the checks up, saying which doctor ran them", () => {
        expect(checksLine(view())).toBeNull();
        expect(checksLine(view({ checked: "offline", checks: [passing, warned] }))).toBe(
            "All 2 checks pass, 1 with a warning, in the quick check, without the network.",
        );
        expect(checksLine(view({ checked: "full", checks: [passing, failing] }))).toBe(
            "1 of 2 checks fails, in the full check, with GitHub and the connection.",
        );
        expect(checksLine(view({ checked: "full", checks: [passing] }))).toContain(
            "The check passes",
        );
    });

    it("offers the old setup for the Trash only once every check passed and its server stopped", () => {
        const well = view({ checked: "offline", checks: [passing, warned], existing });
        expect(trashOffered(well)).toBe(true);
        expect(trashOffered({ ...well, existing: null })).toBe(false);
        expect(trashOffered({ ...well, checks: [passing, failing] })).toBe(false);
        expect(trashOffered({ ...well, checked: null })).toBe(false);
        expect(trashOffered({ ...well, old_server: { pid: 1, relays: 0 } })).toBe(false);
        expect(trashOffered({ ...well, state: "waiting" })).toBe(false);
        // The core refuses while the last reading or step failed.
        expect(trashOffered({ ...well, problem: "the doctor timed out" })).toBe(false);
    });

    it("says Claude Code has not seen it until a session it was given to started", () => {
        expect(statusMark(null)).toEqual({ said: "not seen yet", tone: "plain" });
        expect(statusMark("connected")).toEqual({ said: "connected", tone: "good" });
        expect(statusMark("failed")).toEqual({ said: "failed", tone: "off" });
    });

    it("asks for the repositories while a fresh start has none chosen", () => {
        expect(unpickedLine(view({ state: "waiting", home_ready: false }))).toContain(
            "pick the repositories in its page, with Open repolore",
        );
        expect(unpickedLine(view({ state: "on", home_ready: false }))).not.toBeNull();
        expect(unpickedLine(view({ state: "on", home_ready: true }))).toBeNull();
        expect(unpickedLine(view({ state: "off", home_ready: false }))).toBeNull();
    });
});
