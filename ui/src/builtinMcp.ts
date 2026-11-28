// A built-in MCP server's row in the settings, such as repolore's, put into words: what its
// state is called, what it offers, and what is left before it is given to sessions. Pure, so
// it can be tested; `BuiltinMcpRow.tsx` draws it.

import type {
    BuiltinMcpState,
    BuiltinMcpView,
    BuiltinRegistration,
    DoctorCheck,
    ExistingSetup,
    HeldSession,
    OldServer,
} from "./api/types";

/** What each state is called beside the server's name. */
const LABELS: Record<BuiltinMcpState, string> = {
    missing: "not installed",
    outdated: "out of date",
    off: "off",
    waiting: "waiting",
    on: "on",
};

/** What the state is called beside the server's name. */
export function stateLabel(state: BuiltinMcpState): string {
    return LABELS[state];
}

/** The colour of that label: on is good, what waits for something is pollen, the rest plain. */
export function stateTone(state: BuiltinMcpState): "good" | "off" | "plain" {
    if (state === "on") {
        return "good";
    }
    return state === "waiting" || state === "outdated" ? "off" : "plain";
}

/** Whether turning on offers a copy of the user's own setup beside a fresh start. */
export function offersCopy(view: BuiltinMcpView): boolean {
    return view.existing !== null && !view.home_ready;
}

/** Whether turning on, without a copy, starts an empty setup the user fills in its own page. */
export function startsFresh(view: BuiltinMcpView, copy: boolean): boolean {
    return !(copy || view.home_ready);
}

/** The line under the server's name: where it stands, and what that means for sessions. */
export function summaryLine(view: BuiltinMcpView): string {
    const { name } = view;
    switch (view.state) {
        case "missing":
            return `Swarmed installs ${name}, keeps its settings and index in a folder of its own, and gives it to every session once it is on.`;
        case "outdated":
            return `${outdatedLine(view)} ${KEPT_THROUGH_UPDATE}`;
        case "off":
            return offLine(view);
        case "waiting":
            return leftLine(view);
        case "on":
            return "Given to every session started from now on.";
    }
}

/**
 * What an update leaves: an older repolore cannot copy, so the offer to copy a setup of the
 * user's comes once it is updated.
 */
const KEPT_THROUGH_UPDATE =
    "Updating leaves a setup you have as it is: once updated, you can copy it or start fresh.";

function outdatedLine(view: BuiltinMcpView): string {
    const { name, minimum } = view;
    return view.version === null
        ? `The ${name} installed is too old: Swarmed needs ${minimum} or later.`
        : `${name} ${view.version} is installed, and Swarmed needs ${minimum} or later.`;
}

function offLine(view: BuiltinMcpView): string {
    if (offersCopy(view)) {
        return `You have ${view.name} set up already: copy that setup to keep its repositories and index, or start fresh. Yours is not changed either way.`;
    }
    if (view.home_ready) {
        return "Its setup in Swarmed is kept, ready to turn on again.";
    }
    return `Turning it on starts fresh, and opens ${view.name}'s page to pick the repositories.`;
}

/** What holds a built-in that is turned on back from sessions, so that only one runs. */
export function leftLine(view: BuiltinMcpView): string {
    const count = view.registrations.length;
    const parts: string[] = [];
    if (count > 0) {
        const registrations = count === 1 ? "1 registration" : `${count} registrations`;
        parts.push(`${registrations} outside Swarmed ${count === 1 ? "remains" : "remain"}`);
    }
    if (view.old_server !== null) {
        parts.push("the old shared server is still in use");
    }
    if (parts.length === 0) {
        return "On, but not given to sessions yet. Check again to see whether anything is left.";
    }
    return `On, but not given to sessions while ${parts.join(" and ")}, so that only one ${view.name} runs at a time.`;
}

/** What the row's buttons do. */
export type BuiltinAction =
    | "install"
    | "update"
    | "copy"
    | "fresh"
    | "turn-on"
    | "check-again"
    | "check"
    | "open"
    | "turn-off";

/**
 * The buttons a state offers, the main one last. Moving the old setup to the Trash is offered
 * apart from these, under the old setup's line (`trashOffered`).
 */
export function offered(view: BuiltinMcpView): BuiltinAction[] {
    switch (view.state) {
        case "missing":
            return ["install"];
        case "outdated":
            return ["update"];
        case "off":
            return offersCopy(view) ? ["fresh", "copy"] : ["turn-on"];
        case "waiting":
            // A fresh start has nothing chosen to read until the user picks it in the page.
            return view.home_ready
                ? ["turn-off", "check-again"]
                : ["turn-off", "check-again", "open"];
        case "on":
            return ["turn-off", "check", "open"];
    }
}

/** What a button says, and what it says while what it started runs. */
export function actionLabel(action: BuiltinAction, name: string): { idle: string; busy: string } {
    switch (action) {
        case "install":
            return { idle: "Install", busy: "Installing…" };
        case "update":
            return { idle: "Update", busy: "Updating…" };
        case "copy":
            return { idle: "Copy your existing setup", busy: "Copying your setup…" };
        case "fresh":
            return { idle: "Start fresh", busy: "Starting fresh…" };
        case "turn-on":
            return { idle: "Turn on", busy: "Turning on…" };
        case "check-again":
            return { idle: "Check again", busy: "Checking…" };
        case "check":
            return { idle: "Check", busy: "Checking…" };
        case "open":
            return { idle: `Open ${name}`, busy: `Opening ${name}…` };
        case "turn-off":
            return { idle: "Turn off", busy: "Turning off…" };
    }
}

/** What the row may be doing: a button's action, or one of the steps under it. */
export type BuiltinStep = BuiltinAction | "remove" | "restart" | "trash";

/** Whether a step prints as it goes: its last line is then what shows while it runs. */
export function printsOutput(step: BuiltinStep): boolean {
    return step === "install" || step === "update" || step === "remove";
}

/** What shows while a step that prints nothing runs. */
export function busyLine(step: BuiltinStep, name: string): string {
    switch (step) {
        case "remove":
            return "Removing…";
        case "restart":
            return "Restarting the idle agents…";
        case "trash":
            return "Moving to the Trash…";
        default:
            return actionLabel(step, name).busy;
    }
}

/** Why a step failed, and what is left to do: nothing is lost when a copy or a removal fails. */
export function failedLine(step: BuiltinStep, reason: string): string {
    const why = reason.trim().replace(/[.\s]+$/, "");
    if (step === "copy") {
        return `The copy failed: ${why}. Nothing was changed: copy again, or start fresh.`;
    }
    if (step === "remove") {
        return `Removing it failed: ${why}. Run the command shown yourself, then check again.`;
    }
    return `${why}.`;
}

/** Whether a button is the row's main one, drawn as such. */
export function isMain(action: BuiltinAction): boolean {
    return action === "install" || action === "update" || action === "copy" || action === "open";
}

/** Whether a button only reads again, so the figures shown stay while it runs. */
export function onlyReads(action: BuiltinAction): boolean {
    return action === "check" || action === "check-again";
}

/**
 * How a registration outside Swarmed is taken out: by a command Swarmed runs, shown before it
 * does, or by a step for the user, with the command to run by hand where there is one.
 */
export type RegistrationStep =
    | { kind: "run"; command: string }
    | { kind: "user"; text: string; command: string | null };

export function registrationStep(registration: BuiltinRegistration): RegistrationStep {
    const { command, step } = registration;
    if (registration.runs_here && command !== null) {
        return { kind: "run", command };
    }
    if (step !== null) {
        return { kind: "user", text: step, command };
    }
    if (command !== null) {
        return { kind: "user", text: "Swarmed cannot run this one. Run it yourself:", command };
    }
    return {
        kind: "user",
        text: `Take it out of ${registration.client} yourself.`,
        command: null,
    };
}

/** Where a registration is: its client, and its scope where it has one. */
export function registrationPlace(registration: BuiltinRegistration): string {
    const scope = registration.scope.trim();
    return scope === "" ? registration.client : `${registration.client}, ${scope} scope`;
}

/**
 * The old shared server: whether it runs, starts or is stopped, and how many sessions are
 * connected to it. Stopped, it is still in use while any is, since each would start it again.
 * Starting, it runs with no process said yet: never shown as stopped.
 */
export function oldServerLine(server: OldServer): string {
    const sessions = server.relays === 1 ? "1 session" : `${server.relays} sessions`;
    if (!server.running) {
        return server.relays === 0
            ? "The old shared server is stopped."
            : `The old shared server is stopped, but ${sessions} ${server.relays === 1 ? "is" : "are"} still connected to it and would start it again.`;
    }
    if (server.pid === null) {
        const connected = server.relays === 0 ? "no session" : sessions;
        return `The old shared server is starting, with ${connected} connected.`;
    }
    if (server.relays === 0) {
        return `The old shared server (process ${server.pid}) runs, with no session connected. It stops by itself within 10 minutes.`;
    }
    return `The old shared server (process ${server.pid}) runs, with ${sessions} connected. It stops by itself 10 minutes after the last one ends.`;
}

/**
 * Swarmed's sessions that still use the old server, by title, and whether restarting the idle
 * ones lets go of it. `session` is what a session is called, `untitled` one without a title.
 */
export function heldLine(sessions: HeldSession[], session: string, untitled: string): string {
    const titles = sessions.map((held) => `“${held.title.trim() || untitled}”`);
    const idle = sessions.filter((held) => held.idle).length;
    const who =
        sessions.length === 1
            ? `${titles[0]} still uses the old server.`
            : `${sessions.length} ${session}s still use the old server: ${joined(titles)}.`;
    return `${who} ${restartLine(idle, sessions.length)}`;
}

/** What restarting does for the idle ones of `held` sessions, `idle` of them. */
function restartLine(idle: number, held: number): string {
    const after =
        "restarting lets go of the old server, and the next message starts the agent again.";
    if (idle === 0) {
        return held === 1
            ? "It is at work now; once it is idle, restarting its agent lets go of the old server."
            : "None is idle now; once they are, restarting their agents lets go of the old server.";
    }
    if (held === 1) {
        return `It is idle: ${after}`;
    }
    if (idle === held) {
        return `All of them are idle: ${after}`;
    }
    return `${idle} of them ${idle === 1 ? "is" : "are"} idle: ${after}`;
}

/** Whether there is an idle session to restart. */
export function restartOffered(sessions: HeldSession[]): boolean {
    return sessions.some((held) => held.idle);
}

/** Whether to point out a `session-start` hook: only once the built-in is turned on. */
export function hookNoted(view: BuiltinMcpView): boolean {
    return view.hook && (view.state === "waiting" || view.state === "on");
}

/** Whether a check passes, passes with a warning, or fails. */
export function checkTone(check: DoctorCheck): "good" | "warning" | "failing" {
    if (check.warning) {
        return "warning";
    }
    return check.ok ? "good" : "failing";
}

/** The fix shown under a check: only one that does not simply pass has it. */
export function shownFix(check: DoctorCheck): string | null {
    return checkTone(check) === "good" ? null : check.fix;
}

/** How the checks came out, and which doctor ran them. */
export function checksLine(view: BuiltinMcpView): string | null {
    if (view.checked === null) {
        return null;
    }
    const total = view.checks.length;
    const failing = view.checks.filter((check) => checkTone(check) === "failing").length;
    const warning = view.checks.filter((check) => checkTone(check) === "warning").length;
    const kind =
        view.checked === "full"
            ? "the full check, with GitHub and the connection"
            : "the quick check, without the network";
    const counted = total === 1 ? "1 check" : `${total} checks`;
    if (failing > 0) {
        return `${failing} of ${counted} ${failing === 1 ? "fails" : "fail"}, in ${kind}.`;
    }
    const warned = warning === 0 ? "" : `, ${warning} with a warning`;
    return `${total === 1 ? "The check passes" : `All ${counted} pass`}${warned}, in ${kind}.`;
}

/**
 * Whether moving the old setup to the Trash is offered: once the built-in is on and every
 * check of the last doctor passed, while there is an old setup and its server has stopped.
 */
export function trashOffered(view: BuiltinMcpView): boolean {
    return (
        view.state === "on" &&
        view.checked !== null &&
        view.checks.every((check) => check.ok) &&
        view.existing !== null &&
        view.existing.folders.length > 0 &&
        view.old_server === null &&
        view.problem === null
    );
}

/**
 * What is left to set up in a fresh start, once turned on: the repositories, picked in the
 * server's own page. `null` once its folder holds a setup.
 */
export function unpickedLine(view: BuiltinMcpView): string | null {
    if (view.home_ready || (view.state !== "waiting" && view.state !== "on")) {
        return null;
    }
    return `Nothing is chosen for ${view.name} to read yet: pick the repositories in its page, with Open ${view.name}.`;
}

/**
 * What Claude Code last said of the server, and its colour: nothing until a session it was
 * given to has started.
 */
export function statusMark(status: string | null): {
    said: string;
    tone: "good" | "off" | "plain";
} {
    if (status === null) {
        return { said: "not seen yet", tone: "plain" };
    }
    const tone = status === "connected" || status === "passed" ? "good" : "off";
    return { said: status, tone };
}

/** The user's setup from before: where it is, and whether it has an index. */
export function existingLine(existing: ExistingSetup): string {
    const where = existing.folders.length === 1 ? "Your setup's folder" : "Your setup's folders";
    return existing.indexed ? `${where}, with its index:` : `${where}, not indexed yet:`;
}

/** "a", "a and b", "a, b and c". */
function joined(items: string[]): string {
    if (items.length < 2) {
        return items.join("");
    }
    return `${items.slice(0, -1).join(", ")} and ${items.at(-1)}`;
}
