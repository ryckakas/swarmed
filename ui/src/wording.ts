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
};

const SWARM: Words = {
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
};

/** The words that go with a style. */
export function wordsFor(style: Style | undefined): Words {
    return (style ?? DEFAULT_STYLE) === "swarm" ? SWARM : PLAIN;
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
