// A project's environments as the form shows and takes them. Pure, so it can be tested.

import type { Environment } from "./api/types";

/** An environment with nothing set: all as the project has it. */
export const EMPTY_ENVIRONMENT: Environment = {
    setup: "",
    start: "",
    stop: "",
    teardown: "",
    open_url: "",
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

function unquoted(value: string): string {
    const quote = value[0];
    const quoted = value.length >= 2 && (quote === '"' || quote === "'") && value.endsWith(quote);
    return quoted ? value.slice(1, -1) : value;
}

/** What a task's worktree tooltip says of where and how it was set up. */
export function taskTooltip(task: { name: string; branch: string; environment: string | null }) {
    const environment = task.environment ? `, set up in ${task.environment}` : "";
    return `Worktree ${task.name}, on ${task.branch}${environment}`;
}
