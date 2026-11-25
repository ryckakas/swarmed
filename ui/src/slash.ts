import type { SlashCommand } from "./api/types";

// Completing the `/` commands and skills an agent offers. Pure, so it can be tested.

/** How many suggestions are shown at once. */
export const SUGGESTION_LIMIT = 8;

/**
 * What has been typed of a command name, if the message is a command being named: a slash at
 * the very start with no space after it yet. `null` once the name is finished or for any other text.
 */
export function slashQuery(text: string): string | null {
    const match = /^\/(\S*)$/.exec(text);
    return match ? (match[1] ?? "") : null;
}

/** The commands a query suggests: those starting with it first, then those containing it. */
export function slashMatches(query: string, commands: string[]): string[] {
    const wanted = query.toLowerCase();
    const names = [...new Set(commands)].sort((a, b) => a.localeCompare(b));
    const starting = names.filter((name) => name.toLowerCase().startsWith(wanted));
    const containing = names.filter(
        (name) => !starting.includes(name) && name.toLowerCase().includes(wanted),
    );
    return [...starting, ...containing].slice(0, SUGGESTION_LIMIT);
}

/** The message after choosing a command: its name, ready for arguments. */
export function withCommand(name: string): string {
    return `/${name} `;
}

/**
 * Where the first place to fill in stands in a kept prompt: the start and end of its first
 * `{{something}}`, or `null` if it has none.
 */
export function firstPlaceholder(text: string): [number, number] | null {
    const match = /\{\{[^{}\n]*\}\}/.exec(text);
    return match ? [match.index, match.index + match[0].length] : null;
}

/** The suggestion `step` places after `active`, wrapping round at both ends. */
export function stepActive(active: number, step: number, count: number): number {
    return count === 0 ? 0 : (active + step + count) % count;
}

/**
 * What the list says beside a command that is not simply sent to the agent: Swarmed shows a
 * screen for it, asks about it where the conversation stands, or cannot do it at all.
 */
export function kindLabel(command: SlashCommand | undefined): string | null {
    switch (command?.kind) {
        case "screen":
            return "shown here";
        case "setting":
            return "asks here";
        case "own":
            return "done here";
        case "choice":
            return "opens here";
        case "terminal":
            return command.note ? `not sent. ${command.note}` : "not sent";
        default:
            return null;
    }
}

/** The command a message calls with its first word, if it is one of `commands`. */
export function calledCommand(text: string, commands: SlashCommand[]): SlashCommand | null {
    const name = /^\s*\/(\S+)/.exec(text)?.[1];
    return commands.find((command) => command.name === name) ?? null;
}

/**
 * Whether Swarmed itself does what the message asks, so that it is handed over at once and
 * never waits in the queue: a screen or a question interrupts no turn, and anything of the
 * kind that does have to reach the agent is made to wait by the core.
 */
export function takenBySwarmed(text: string, commands: SlashCommand[]): boolean {
    const command = calledCommand(text, commands);
    return command !== null && command.kind !== "prompt";
}

/**
 * Whether a message is one of the agent's own commands for which the window opens a choice of
 * Swarmed's own, such as `/rewind`, typed alone: it is never sent.
 */
export function opensChoice(text: string, commands: SlashCommand[]): boolean {
    const alone = /^\s*\/\S+\s*$/.test(text);
    return alone && calledCommand(text, commands)?.kind === "choice";
}

/** Whether what the message box holds opens a choice of Swarmed's own: such a command alone. */
export function boxOpensChoice(
    box: { text: string; files: unknown[]; images: unknown[] },
    commands: SlashCommand[],
): boolean {
    const nothingElse = box.files.length === 0 && box.images.length === 0;
    return nothingElse && opensChoice(box.text, commands);
}

/**
 * Whether a message is handed over at once, never queued: one of the agent's own commands
 * Swarmed does itself (`takenBySwarmed`), with no files, images, sessions, pull requests or
 * issues to go with it, which only a message to the agent can take.
 */
export function goesAtOnce(
    message: string,
    going: {
        files: unknown[];
        images: unknown[];
        sessions: unknown[];
        github: unknown[];
        elements?: unknown[];
    },
    commands: SlashCommand[],
): boolean {
    const lists = [going.files, going.images, going.sessions, going.github, going.elements ?? []];
    const alone = lists.every((list) => list.length === 0);
    return alone && takenBySwarmed(message, commands);
}

/** The commands that are messages to the agent: all a first message can start with. */
export function sentOn(commands: SlashCommand[]): SlashCommand[] {
    return commands.filter((command) => command.kind === "prompt");
}
