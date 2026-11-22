// A project's environments as the form shows and takes them. Pure, so it can be tested.

import type { Environment } from "./api/types";
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

function unquoted(value: string): string {
    const quote = value[0];
    const quoted = value.length >= 2 && (quote === '"' || quote === "'") && value.endsWith(quote);
    return quoted ? value.slice(1, -1) : value;
}

/** What a task's worktree tooltip says of where and how it was set up, in the style's words. */
export function taskTooltip(
    task: { name: string; branch: string; environment: string | null },
    words: Words = wordsFor("plain"),
) {
    const environment = task.environment ? `, set up in ${task.environment}` : "";
    return `${capitalized(words.terms.worktree)} ${task.name}, on ${task.branch}${environment}`;
}
