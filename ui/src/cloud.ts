// Tasks handed to cloud agents, as the sidebar shows them. Pure, so it can be tested.

import type { CloudProvider, CloudTask, NoteTarget, Unpublished } from "./api/types";

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

/** Where work is handed to the cloud from: a repository, a task, or a session of a task. */
export interface CloudTarget extends NoteTarget {
    /** The session whose conversation can go along, when it is handed over from one. */
    session?: number;
}

/**
 * What the cloud agent is told: only what the user writes, a brief the session's own agent
 * writes for it, or the session's conversation as it was said.
 */
export type CloudTelling = "written" | "brief" | "conversation";

/** The ways of telling it, as the form offers them. The last two need a session. */
export const CLOUD_TELLINGS: [telling: CloudTelling, label: string, note: string][] = [
    [
        "brief",
        "A brief this session's agent writes",
        "The agent here is asked to write up what the cloud agent needs and to hand it over with its own tool. It knows everything it read and did, not only what was said. It asks you first unless the session lets it act unasked.",
    ],
    [
        "conversation",
        "The whole conversation",
        "What you and the agent said, and the plans it proposed, go along as text: the latest 60,000 characters of it. Not what the agent read or ran, and no files.",
    ],
    ["written", "Only what I write here", ""],
];

/**
 * What the session's agent is asked when it is to write the brief and hand the work over
 * itself, with whatever the user added under it.
 */
export function briefRequest(provider: CloudProvider, added: string): string {
    const { name } = cloudProvider(provider);
    const asked = `Hand this work to a ${name} cloud agent, with the \`delegate_to_cloud\` tool and \`provider\` set to \`${provider}\`. It starts with no knowledge of this conversation and has the repository only as it is on GitHub. Write it a prompt that says everything it needs: the goal, the plan we arrived at in full, the files and decisions that matter, and how it can tell it is done. If it needs commits or files that are only in this worktree, do not delegate yet: tell me what is missing.`;
    return added.trim() === "" ? asked : `${asked}\n\n${added.trim()}`;
}

/** What of a task's work a cloud agent will not see, in a sentence, or `null` if it sees all. */
export function unseenByCloud(unpublished: Unpublished): string | null {
    const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
    const parts = [
        unpublished.changed > 0 &&
            `${count(unpublished.changed, "file is", "files are")} not committed`,
        unpublished.unpushed > 0 &&
            `${count(unpublished.unpushed, "commit is", "commits are")} not pushed`,
    ].filter((part) => part !== false);
    if (parts.length === 0) {
        return null;
    }
    return `Only on this Mac, where the cloud agent does not see it: ${parts.join(", ")}.`;
}
