// The few places where the Swarm palette changes what things are called, not only how they
// look. Every other palette keeps the plain words. Pure, so it can be tested.

import type { Palette } from "./api/types";

/** What a handful of things are called. */
export interface Words {
    /** The small label over the tree of repositories; nothing in the plain palettes. */
    overview: string;
    /** The question on the new-task screen. */
    newTask: string;
    /** What the button that starts a session in a task is called. */
    newSession: string;
    /** What stands under a task that has no sessions. */
    noSessions: string;
}

const PLAIN: Words = {
    overview: "",
    newTask: "What should the agent work on?",
    newSession: "New session",
    noSessions: "No sessions",
};

const SWARM: Words = {
    overview: "Hive",
    newTask: "What should the swarm work on?",
    newSession: "Spawn a session",
    noSessions: "Nothing spawned yet",
};

/** The words that go with a palette. */
export function wordsFor(palette: Palette | undefined): Words {
    return palette === "swarm" ? SWARM : PLAIN;
}
