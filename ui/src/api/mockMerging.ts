import type { Api } from "./client";
import type { MergeMethod, MergeRules, MergeWhen, Pull, PullChange } from "./types";

// Merging the sample pull requests, to see the merge dialog in a browser. storefront lets the
// user merge every way, as its admin, with auto-merge; bonsai-lint only squashes, and allows no
// auto-merge. Its required check holds `fix-login-redirect` back, so a plain or forced merge
// is refused as GitHub refuses it, auto-merge can be turned on, and bypassing merges it.

/** What each sample repository allows, as GitHub would say. */
const SAMPLE_RULES: Record<string, Omit<MergeRules, "method"> & { default: MergeMethod }> = {
    storefront: {
        methods: ["merge", "squash", "rebase"],
        default: "squash",
        auto_merge: true,
        admin: true,
    },
    "bonsai-lint": { methods: ["squash"], default: "squash", auto_merge: false, admin: false },
};

/** What the core's window needs of the sample tasks' pull requests to merge them. */
export interface MergeHost {
    /** A task's pull request as the core last read it. */
    pullOf(name: string): Pull | null;
    /** Keeps a task's pull request as read now, tells every window, and records what changed. */
    pullTakenIn(name: string, pull: Pull, change: PullChange | null): void;
}

/** What GitHub says of a plain or forced merge that a requirement of the repository holds back. */
function blocked(pull: Pull): string {
    const repository = pull.url.replace("https://github.com/", "").replace(/\/pull\/\d+$/, "");
    return [
        `#${pull.number} was not merged: Pull request ${repository}#${pull.number} is not mergeable: the base branch policy prohibits the merge.`,
        "To have the pull request merged after all the requirements have been met, add the `--auto` flag.",
        "To use administrator privileges to immediately merge the pull request, add the `--admin` flag.",
    ].join("\n");
}

/** Why the core refuses to merge `pull` `when` asked at `head`, or GitHub does, as they word it. */
function refusal(pull: Pull, when: MergeWhen, head: string): string | null {
    const number = pull.number;
    if (pull.state !== "open") {
        return pull.state === "merged" ? `#${number} is merged already` : `#${number} is closed`;
    }
    if (pull.draft) {
        return `#${number} is a draft: mark it ready for review first`;
    }
    if (pull.head !== head) {
        return `#${number} moved on since it was read: its head is ${pull.head.slice(0, 7)} now. Look at it again before merging.`;
    }
    const failed = pull.checks.filter((check) => check.outcome === "failed");
    if (when === "now" && failed.length > 0) {
        const names = failed.map((check) => check.name).join(", ");
        return `checks failed on #${number}: ${names}. A forced merge merges it anyway, where no rule of the repository requires them.`;
    }
    const now = ["clean", "unstable", "has_hooks"].includes(pull.merge_state ?? "");
    if (when === "auto" && now) {
        return `#${number} can be merged now: its requirements are met, so auto-merge would merge it at once. Merge it instead.`;
    }
    const held = when === "now" || when === "force";
    return held && pull.merge_state === "blocked" ? blocked(pull) : null;
}

/** The merge part of the mock's `Api`, over the sample tasks' pull requests. */
export function mergeApi(
    host: MergeHost,
): Pick<Api, "projectMergeRules" | "taskPullMerge" | "taskPullAutoMergeOff"> {
    // The method last merged with, by repository, as the core keeps it.
    const used = new Map<string, MergeMethod>();
    const pullOf = (name: string) => {
        const pull = host.pullOf(name);
        if (!pull) {
            throw new Error(`no pull requests found for branch "${name}"`);
        }
        return pull;
    };
    return {
        projectMergeRules: async (project) => {
            const rules = SAMPLE_RULES[project] ?? SAMPLE_RULES.storefront;
            if (!rules) {
                throw new Error("the sample repositories are gone");
            }
            const { default: first, ...allowed } = rules;
            return { ...allowed, method: used.get(project) ?? first };
        },
        taskPullMerge: async (project, name, method, when, head) => {
            const pull = pullOf(name);
            const refused = refusal(pull, when, head);
            if (refused) {
                throw new Error(refused);
            }
            used.set(project, method);
            if (when === "auto") {
                const auto = { ...pull, auto_merge: method };
                host.pullTakenIn(name, auto, null);
                return { outcome: "auto_merge", pull: auto };
            }
            const merged: Pull = { ...pull, state: "merged", auto_merge: null, merge_state: null };
            host.pullTakenIn(name, merged, { kind: "merged" });
            return { outcome: "merged", pull: merged };
        },
        taskPullAutoMergeOff: async (_project, name) => {
            const off = { ...pullOf(name), auto_merge: null };
            host.pullTakenIn(name, off, null);
            return { outcome: "auto_merge_off", pull: off };
        },
    };
}
