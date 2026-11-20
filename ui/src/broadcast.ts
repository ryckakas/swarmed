// Sending one prompt to several agents at once, each in a task of its own, and comparing what
// two tasks changed. Pure, so it can be tested.

import type { AgentInfo, FileChange } from "./api/types";
import { uniqueTaskName } from "./tree";

/** One more agent to run the same prompt on, and the model to run it with. */
export interface Variant {
    agent: string;
    /** A model name, or the empty string for the agent's default. */
    model: string;
}

/** What tells one variant from another, and what a menu holds it as. */
export function variantKey(variant: Variant): string {
    return `${variant.agent}\n${variant.model}`;
}

/** The variant a menu value stands for. */
export function variantFrom(key: string): Variant {
    const [agent = "", model = ""] = key.split("\n");
    return { agent, model };
}

/** A variant as it is shown: "Codex, gpt-6.2-sol", or the agent's name alone for its default. */
export function variantLabel(variant: Variant, agents: AgentInfo[]): string {
    const name = agents.find((agent) => agent.id === variant.agent)?.name ?? variant.agent;
    return variant.model ? `${name}, ${variant.model}` : name;
}

/**
 * Every agent with each of its models, and with its default, to choose more variants from:
 * without the ones chosen already and the one the task runs on anyway.
 */
export function variantChoices(agents: AgentInfo[], taken: Variant[]): Variant[] {
    const used = new Set(taken.map(variantKey));
    return agents
        .flatMap((agent) => ["", ...agent.models].map((model) => ({ agent: agent.id, model })))
        .filter((variant) => !used.has(variantKey(variant)));
}

/**
 * The names of the tasks the variants run in: the first task's name with the agent's id after
 * it, numbered where that is taken, by another task or by an earlier variant.
 */
export function variantNames(base: string, variants: Variant[], taken: string[]): string[] {
    const used = [...taken, base];
    return variants.map((variant) => {
        const name = uniqueTaskName(`${base}-${variant.agent}`, used);
        used.push(name);
        return name;
    });
}

/** One file in a comparison of two tasks: what each of them did to it, if anything. */
export interface ComparedFile {
    path: string;
    left: FileChange | null;
    right: FileChange | null;
}

/** Every file either task changed, by path, with what each did to it. */
export function compared(left: FileChange[], right: FileChange[]): ComparedFile[] {
    const paths = [...new Set([...left, ...right].map((change) => change.path))].sort((a, b) =>
        a.localeCompare(b),
    );
    const find = (changes: FileChange[], path: string) =>
        changes.find((change) => change.path === path) ?? null;
    return paths.map((path) => ({ path, left: find(left, path), right: find(right, path) }));
}

/** How many lines a task added and removed in all, and in how many files. */
export function totals(changes: FileChange[]): { files: number; added: number; removed: number } {
    return {
        files: changes.length,
        added: changes.reduce((sum, change) => sum + (change.added ?? 0), 0),
        removed: changes.reduce((sum, change) => sum + (change.removed ?? 0), 0),
    };
}
