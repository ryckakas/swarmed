// What things are called, by the style the user chose: the plain words, or a swarm's. Apart from
// the palette, so either goes with any colours. Pure, so it can be tested.

import type { BackgroundState, SessionStatus, StackState, Style } from "./api/types";
import { DEFAULT_STYLE } from "./appearance";

/** What a session is doing, as the marks tell it: its status, or idle with an unseen answer. */
export type StatusKind = SessionStatus | "unread";

/** What a handful of things are called. */
export interface Words {
    /** The small label over the tree of repositories; nothing in the plain style. */
    overview: string;
    /** The question on the new-task screen. */
    newTask: string;
    /** What the button that starts a session in a task is called. */
    newSession: string;
    /** What stands under a task that has no sessions. */
    noSessions: string;
    /**
     * What each status is called. Every one is a list: with more than one word, the app shows
     * one of them at random and changes it now and then. Add words here to add variety.
     */
    status: Record<StatusKind, string[]>;
    /**
     * Other words for what the agent is doing right now, by the plain word: lists, like the
     * statuses. What is not named here, such as which tool runs, is shown as it is.
     */
    activity: Record<string, string[]>;
    /** How many sessions wait for the user, in the strip over the tree. */
    needYou: (count: number) => string;
    /** How many sessions hold an answer nobody has looked at. */
    unread: (count: number) => string;
    /** What stands under a repository that has no tasks. */
    noTasks: string;
    /** The button that starts a task. */
    startTask: string;
    /** The button that adds a repository. */
    addRepository: string;
    /** The line that unfolds a task's closed sessions. */
    closed: (count: number) => string;
    /** The button that puts a closed session back. */
    reopen: string;
    /** What handing work to a cloud agent is called, before "from <where>". */
    delegate: string;
    /** Taking a cloud task off the list, which leaves it running at its provider. */
    forgetCloud: string;
    /** The context card's parts on the agent's usage windows and on what the session spent. */
    usageLimits: string;
    spentHere: string;
    /** The context card's part on the account's extra usage, billed beyond the plan. */
    extraUsage: string;
    /** The overview of the plan's usage that the coins button in a session's header opens. */
    usage: string;
    /** Its button that asks Claude Code for the figures again. */
    askAgain: string;
    /** The button on a command Claude Code has a screen for only in its own terminal. */
    openInClaude: string;
    /** The button that ends that Claude Code and shows the session's shell again. */
    backToShell: string;
    /** What ending a session and taking it out of the sidebar is called. */
    closeSession: string;
    /** Starting a session from all of this one, on the same agent and on another. */
    forkAll: string;
    cloneTo: string;
    /** Closing a session and keeping its conversation, and deleting it with the conversation. */
    archiveSession: string;
    deleteSession: string;
    /** The question asked before a session is closed. */
    closeAsk: string;
    /** One way of setting a task up, and several of them. */
    environment: string;
    environments: string;
    /** How a turn's end reads: one that finished, and one that was stopped or failed. */
    turnDone: string;
    turnStopped: string;
    /** What each state of a task's stack is called. */
    stack: Record<StackState, string>;
    /** The status line's piece that adds up how long the agent has worked. */
    timeWorked: string;
    /** The task panel's two tabs: what the task changed, and what its agents left running. */
    files: string;
    /** An agent a session's agent started for part of its work, and going back from one. */
    subAgent: string;
    subAgentWorking: string;
    subAgentDone: string;
    backToSession: string;
    changes: string;
    background: string;
    /** What each state of something left running is called. */
    backgroundState: Record<BackgroundState, string>;
    /** The button that stops something left running. */
    stopBackground: string;
    /** What the background tab says when nothing was left running. */
    noBackground: string;
    /** The heading over the charts of what the sessions did. */
    activityTitle: string;
    /** The switch that asks an agent for its fast mode. */
    fast: string;
    /** What a task's setup produced, kept for new tasks to start from; and several. */
    snapshot: string;
    snapshots: string;
    /** The buttons that unfold and fold every changed file at once. */
    expandAll: string;
    collapseAll: string;
    /** The button in the commit box that has a message written for the changes. */
    writeMessage: string;
    /** The button in the pull request form that has its title and description written. */
    writePull: string;
    /** The row of the user's own buttons on a task, and the menu holding those that do not fit. */
    actions: string;
    moreActions: string;
    /** What the version in the corner opens, and what the About view is called. */
    about: string;
    /** What Swarmed is, in a sentence, in the About view. */
    aboutLine: string;
}

const PLAIN: Words = {
    overview: "",
    newTask: "What should the agent work on?",
    newSession: "New session",
    noSessions: "No sessions",
    status: {
        idle: ["Idle"],
        unread: ["Unread reply"],
        working: ["Working"],
        waiting: ["Needs you"],
        errored: ["Stopped with an error"],
    },
    activity: {},
    needYou: (count) => (count === 1 ? "1 session needs you" : `${count} sessions need you`),
    unread: (count) => `${count} unread`,
    noTasks: "No tasks",
    startTask: "New task",
    addRepository: "Add repository",
    closed: (count) => `${count} closed`,
    reopen: "Reopen",
    delegate: "Delegate to the cloud",
    forgetCloud: "Take off the list",
    usageLimits: "Usage limits",
    spentHere: "Spent in this session",
    extraUsage: "Extra usage",
    usage: "Usage",
    askAgain: "Ask again",
    openInClaude: "Open in Claude Code",
    backToShell: "Back to the shell",
    closeSession: "Close session",
    forkAll: "Fork this conversation",
    cloneTo: "Clone to…",
    archiveSession: "Archive",
    deleteSession: "Delete",
    closeAsk: "Close this session?",
    environment: "Environment",
    environments: "Environments",
    turnDone: "Turn finished",
    turnStopped: "Turn stopped",
    stack: { stopped: "Stack stopped", starting: "Stack starting", running: "Stack running" },
    timeWorked: "Time worked",
    files: "Files",
    subAgent: "Sub-agent",
    subAgentWorking: "At work",
    subAgentDone: "Done",
    backToSession: "Back to the session",
    changes: "Changes",
    background: "Background",
    backgroundState: {
        running: "Running",
        completed: "Finished",
        failed: "Failed",
        stopped: "Stopped",
    },
    stopBackground: "Stop",
    noBackground: "The agents of this task have left nothing running.",
    activityTitle: "Activity",
    fast: "Fast",
    snapshot: "Snapshot",
    snapshots: "Snapshots",
    expandAll: "Expand all",
    collapseAll: "Collapse all",
    writeMessage: "Generate a commit message",
    writePull: "Generate the title and description",
    actions: "Actions",
    moreActions: "More actions…",
    about: "About Swarmed",
    aboutLine: "Coding agents side by side, each on a branch and in a worktree of its own.",
};

const SWARM: Words = {
    ...PLAIN,
    overview: "Hive",
    newTask: "What should the swarm work on?",
    newSession: "Spawn a session",
    noSessions: "Nothing spawned yet",
    status: {
        idle: ["Dormant"],
        unread: ["Hatched"],
        working: ["Mutating", "Incubating"],
        // These two ask something of the user, so they stay plain in every style.
        waiting: ["Needs you"],
        errored: ["Stopped with an error"],
    },
    activity: {
        Starting: ["Stirring", "Waking"],
        Thinking: ["Gestating", "Digesting", "Brooding"],
        Writing: ["Secreting", "Spinning"],
    },
    needYou: (count) => (count === 1 ? "1 awaits you" : `${count} await you`),
    unread: (count) => `${count} hatched`,
    noTasks: "The nest is empty",
    closeSession: "Terminate spawn",
    forkAll: "Split this spawn",
    cloneTo: "Clone onto…",
    archiveSession: "Burrow",
    deleteSession: "Cull",
    closeAsk: "Terminate this spawn?",
    environment: "Spawning pool",
    environments: "Spawning pools",
    turnDone: "Molt complete",
    turnStopped: "Molt cut short",
    stack: { stopped: "Pool dormant", starting: "Pool gestating", running: "Pool thriving" },
    timeWorked: "Time mutating",
    files: "Tunnels",
    subAgent: "Hatchling",
    subAgentWorking: "Foraging",
    subAgentDone: "Returned",
    backToSession: "Back to the spawn",
    changes: "Mutations",
    background: "Drones",
    backgroundState: {
        running: "Foraging",
        completed: "Returned",
        // Something went wrong, which the user should not have to decode.
        failed: "Failed",
        stopped: "Culled",
    },
    stopBackground: "Cull",
    noBackground: "No drones are out for this task.",
    activityTitle: "Hive activity",
    fast: "Frenzy",
    snapshot: "Cocoon",
    snapshots: "Cocoons",
    expandAll: "Unburrow all",
    collapseAll: "Burrow all",
    writeMessage: "Spin a commit message",
    writePull: "Spin the title and description",
    actions: "Instincts",
    moreActions: "More instincts…",
    about: "About the hive",
    aboutLine:
        "A swarm of coding agents at work side by side, each on a branch and in a worktree of its own.",
};

/** The swarm's words, and its names for the tree as well. */
const COLONY: Words = {
    ...SWARM,
    startTask: "New brood",
    addRepository: "Found a colony",
    closed: (count) => `${count} burrowed`,
    reopen: "Unburrow",
    delegate: "Send spores to the cloud",
    forgetCloud: "Let it drift",
    usageLimits: "Hunger limits",
    spentHere: "Consumed by this spawn",
    extraUsage: "Borrowed biomass",
    usage: "Hunger",
    askAgain: "Sniff again",
    openInClaude: "Open in Claude Code's lair",
    backToShell: "Back to the burrow",
};

const WORDS: Record<Style, Words> = { plain: PLAIN, swarm: SWARM, colony: COLONY };

/** The words that go with a style. */
export function wordsFor(style: Style | undefined): Words {
    return WORDS[style ?? DEFAULT_STYLE];
}

/** A name as it stands in the middle of a sentence: "an environment", "a spawning pool". */
export function withArticle(name: string): string {
    const lower = name.toLowerCase();
    return `${/^[aeiou]/.test(lower) ? "an" : "a"} ${lower}`;
}

/** The words on offer for what the agent is doing: the swarm's, or the plain word alone. */
export function activityWords(words: Words, plain: string): string[] {
    return words.activity[plain] ?? [plain];
}

/** Which mark a session gets: unread only counts while it is idle, as the rest says more. */
export function statusKind(status: SessionStatus, unread: boolean): StatusKind {
    return unread && status === "idle" ? "unread" : status;
}

/** One of the words, chosen by a roll from 0 up to, not including, 1. */
export function pickWord(words: string[], roll: number): string {
    const index = Math.min(words.length - 1, Math.max(0, Math.floor(roll * words.length)));
    return words[index] ?? "";
}
