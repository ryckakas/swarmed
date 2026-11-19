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

/** The suggestion `step` places after `active`, wrapping round at both ends. */
export function stepActive(active: number, step: number, count: number): number {
    return count === 0 ? 0 : (active + step + count) % count;
}
