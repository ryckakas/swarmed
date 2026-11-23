// A project's environments as the form shows and takes them. Pure, so it can be tested.

import type { Environment, EnvironmentChoice } from "./api/types";
import { capitalized, type Words, wordsFor } from "./wording";

/** An environment with nothing set: all as the project has it. */
export const EMPTY_ENVIRONMENT: Environment = {
    setup: "",
    start: "",
    stop: "",
    teardown: "",
    open_url: "",
    ports: {},
    env: {},
};

/** Variables as lines of `NAME=value`, the way they are typed. */
export function envToText(env: Record<string, string>): string {
    return Object.entries(env)
        .map(([name, value]) => `${name}=${value}`)
        .join("\n");
}

/**
 * The variables in lines of `NAME=value`. Blank lines and lines starting with `#` are passed
 * over, `export ` in front of a name is dropped, and quotes around a whole value are taken off,
 * so a line pasted from a `.env` file works. A line without `=` is an error naming the line.
 */
export function envFromText(text: string): Record<string, string> {
    const env: Record<string, string> = {};
    for (const raw of text.split("\n")) {
        const line = raw.trim().replace(/^export\s+/, "");
        if (line === "" || line.startsWith("#")) {
            continue;
        }
        const at = line.indexOf("=");
        if (at < 1) {
            throw new Error(`"${line}" is not a variable: write NAME=value`);
        }
        env[line.slice(0, at).trim()] = unquoted(line.slice(at + 1).trim());
    }
    return env;
}

/** Ports as lines of `name=3000`, the way they are typed. */
export function portsToText(ports: Record<string, number>): string {
    return envToText(
        Object.fromEntries(Object.entries(ports).map(([name, base]) => [name, String(base)])),
    );
}

/** The ports in lines of `name=3000`. A line whose number is none is an error naming the line. */
export function portsFromText(text: string): Record<string, number> {
    const ports: Record<string, number> = {};
    for (const [name, value] of Object.entries(envFromText(text))) {
        const base = Number(value);
        if (!Number.isInteger(base) || base < 1 || base > 65535) {
            throw new Error(`"${name}=${value}" is not a port: write name=3000`);
        }
        ports[name] = base;
    }
    return ports;
}

/** The largest stack limit the core can keep. */
const MOST_STACKS = 0xffff_ffff;

/** A stack limit the way it is typed: empty for the project's own. */
export function stacksToText(limit: number | undefined): string {
    return limit === undefined ? "" : String(limit);
}

/**
 * The stack limit typed, or `undefined` when nothing is, which leaves the project's own. Anything
 * but a whole number from 1 is an error.
 */
export function stacksFromText(text: string): number | undefined {
    const typed = text.trim();
    if (typed === "") {
        return undefined;
    }
    const limit = Number(typed);
    if (!Number.isInteger(limit) || limit < 1 || limit > MOST_STACKS) {
        throw new Error(`"${typed}" is not a number of stacks: write a whole number from 1`);
    }
    return limit;
}

function unquoted(value: string): string {
    const quote = value[0];
    const quoted = value.length >= 2 && (quote === '"' || quote === "'") && value.endsWith(quote);
    return quoted ? value.slice(1, -1) : value;
}

/** Stands, in the new-task menu, for the repository's own setup: no environment at all. */
export const OWN_SETUP = "";

/**
 * What the new-task menu of environments stands at: what was picked for this repository, as
 * long as it still has it, else its default; `OWN_SETUP` when there is neither.
 */
export function environmentShown(
    names: string[],
    usual: string | null,
    picked: string | undefined,
): string {
    if (picked === OWN_SETUP || (picked !== undefined && names.includes(picked))) {
        return picked;
    }
    return usual !== null && names.includes(usual) ? usual : OWN_SETUP;
}

/**
 * What a new task is created with for what was picked. Nothing picked, or a name that is gone,
 * is the repository's default, which the core settles as the task is made: a task sent before
 * the environments were read is still set up in it.
 */
export function environmentChoice(names: string[], picked: string | undefined): EnvironmentChoice {
    if (picked === OWN_SETUP) {
        return "own";
    }
    return picked !== undefined && names.includes(picked) ? { named: picked } : "default";
}

/** The new-task menu's entries for the environments, the default one said to be so. */
export function environmentOptions(names: string[], usual: string | null): [string, string][] {
    return names.map((name) => [name, name === usual ? `${name} (default)` : name]);
}

/** What a task's worktree tooltip says of where and how it was set up, in the style's words. */
export function taskTooltip(
    task: { name: string; branch: string; environment: string | null },
    words: Words = wordsFor("plain"),
) {
    const environment = task.environment ? `, set up in ${task.environment}` : "";
    return `${capitalized(words.terms.worktree)} ${task.name}, on ${task.branch}${environment}`;
}
