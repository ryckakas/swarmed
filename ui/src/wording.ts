// What things are called, by the style the user chose: the plain words, or a swarm's. Apart from
// the palette, so either goes with any colours. Pure, so it can be tested.

import type { SessionStatus, Style } from "./api/types";
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
};

/** The swarm's words, and its names for the tree as well. */
const COLONY: Words = {
    ...SWARM,
    startTask: "New brood",
    addRepository: "Found a colony",
    closed: (count) => `${count} burrowed`,
    reopen: "Unburrow",
    delegate: "Send spores to the cloud",
};

const WORDS: Record<Style, Words> = { plain: PLAIN, swarm: SWARM, colony: COLONY };

/** The words that go with a style. */
export function wordsFor(style: Style | undefined): Words {
    return WORDS[style ?? DEFAULT_STYLE];
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
