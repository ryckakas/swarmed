// Tasks handed to cloud agents, as the sidebar shows them. Pure, so it can be tested.

import type { CloudProvider, CloudTask } from "./api/types";

/** The statuses a provider gives a task that is not finished yet. */
const UNFINISHED = new Set(["pending", "in_progress", "running", "queued"]);

/** Whether the task may still change, so its status is worth asking for again. */
export function isUnfinished(task: CloudTask): boolean {
    return UNFINISHED.has(task.status);
}

/** What a task is called in a list: the first line of what it was asked to do. */
export function cloudTitle(task: CloudTask): string {
    return task.prompt.split("\n")[0]?.trim() || "Cloud task";
}

/** What each provider's cloud agent is called. */
export const CLOUD_PROVIDERS: [provider: CloudProvider, name: string, agent: string][] = [
    ["codex", "Codex", "codex"],
    ["claude", "Claude Code", "claude"],
];

/** What the provider of a task is called, and the agent whose mark stands for it. */
export function cloudProvider(provider: string): { name: string; agent: string } {
    const known = CLOUD_PROVIDERS.find(([id]) => id === provider);
    return known ? { name: known[1], agent: known[2] } : { name: provider, agent: provider };
}

/** What a task's row says when pointed at. */
export function cloudTooltip(task: CloudTask): string {
    const { name } = cloudProvider(task.provider);
    const from = task.branch ? `, from ${task.branch}` : "";
    const state =
        task.status === "sent"
            ? `sent, and ${name} does not say how it is getting on`
            : task.status;
    return `${cloudTitle(task)}\nIn ${name}'s cloud${from}: ${state}. Opens at ${name}.`;
}

/**
 * How the task's state is drawn: still going, done, gone wrong, or only known to have been
 * sent.
 */
export function cloudTone(task: CloudTask): "going" | "done" | "failed" | "sent" {
    if (isUnfinished(task)) {
        return "going";
    }
    if (task.status === "sent") {
        return "sent";
    }
    return task.status === "error" || task.status === "failed" ? "failed" : "done";
}
