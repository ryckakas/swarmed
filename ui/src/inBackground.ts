// What the sidebar's marks say of work a session's agent left running in the background
// (#420). Pure, so it can be tested. What runs is drawn moving, since what is still runs
// nothing: a grub per sub-agent at work, or a fly per command left running.

import type { BackgroundCount, SessionStatus } from "./api/types";

/**
 * How much a mark says of what runs in the background: sub-agents at work, which the session is
 * not done with, or only commands left running, often on purpose and so said more quietly.
 */
export type BackgroundLevel = "agents" | "commands";

/** Nothing running in the background. */
export const NOTHING_IN_BACKGROUND: BackgroundCount = { agents: 0, commands: 0 };

/** What several sessions have running in the background, added up. */
export function addedUp(counts: BackgroundCount[]): BackgroundCount {
    return counts.reduce(
        (sum, count) => ({
            agents: sum.agents + count.agents,
            commands: sum.commands + count.commands,
        }),
        NOTHING_IN_BACKGROUND,
    );
}

/** Whether anything runs in the background. */
export function anyInBackground(count: BackgroundCount): boolean {
    return count.agents > 0 || count.commands > 0;
}

/**
 * What a status mark shows of the background: only beside idle, since working, waiting and an
 * error already say more, and never nothing while something runs there. Sub-agents rank above
 * commands.
 */
export function backgroundShown(
    status: SessionStatus,
    count: BackgroundCount,
): BackgroundLevel | null {
    if (status !== "idle") {
        return null;
    }
    if (count.agents > 0) {
        return "agents";
    }
    return count.commands > 0 ? "commands" : null;
}

/** The most grubs that circle a mark, however many sub-agents are at work. */
export const BROOD_MOST = 3;

/**
 * How many grubs circle a mark: one for each sub-agent at work in the background, up to
 * `BROOD_MOST`, so the eye can count a few. None while no sub-agent is at work.
 */
export function broodSize(count: BackgroundCount): number {
    return Math.min(Math.max(count.agents, 0), BROOD_MOST);
}

/**
 * One bead of a grub circling a mark. The orbit is two motions a quarter of a turn apart, one
 * across and one up and down, so it keeps to an oval and the bead stays round; each starts
 * that many seconds in, which is negative, so that the bead is on its way from the first frame.
 */
export interface Bead {
    /** When the motion across starts, in seconds. */
    across: number;
    /** When the motion up and down starts, in seconds. */
    upDown: number;
    /** Its radius, in the mark's own units. */
    r: number;
    opacity: number;
}

/** The beads of one grub, head first: each a little behind, smaller and fainter. */
const GRUB: readonly { ahead: number; r: number; opacity: number }[] = [
    { ahead: 0.2, r: 1.25, opacity: 1 },
    { ahead: 0.1, r: 0.95, opacity: 0.8 },
    { ahead: 0, r: 0.7, opacity: 0.55 },
];

/** Seconds, to the millisecond: what a style is given. */
function seconds(value: number): number {
    return Math.round(value * 1000) / 1000 || 0;
}

/**
 * The beads of `size` grubs on an orbit taking `period` seconds, spread evenly round it, each
 * grub's head the furthest along.
 */
export function broodBeads(size: number, period: number): Bead[] {
    const beads: Bead[] = [];
    for (let grub = 0; grub < size; grub += 1) {
        const spread = (grub * period) / size;
        for (const bead of GRUB) {
            const along = -(spread + bead.ahead);
            beads.push({
                across: seconds(along),
                upDown: seconds(along - period / 4),
                r: bead.r,
                opacity: bead.opacity,
            });
        }
    }
    return beads;
}

/** The most flies that buzz round a mark, however many commands are left running. */
export const FLIES_MOST = 3;

/**
 * How many flies buzz round a mark: one for each command left running, up to `FLIES_MOST`, so
 * the eye can count a few. None while no command runs.
 */
export function fliesSize(count: BackgroundCount): number {
    return Math.min(Math.max(count.commands, 0), FLIES_MOST);
}

/**
 * One fly buzzing round a mark. It turns round it as a grub does, by two swings a quarter of a
 * turn apart, and weaves a little across and up and down on top of that, out of step, so it
 * never keeps to the oval. Each start is negative, so that it is on its way from the first frame.
 */
export interface Flight {
    /** Seconds for half a turn round the mark. */
    halfTurn: number;
    /** When the swing across starts, in seconds. */
    across: number;
    /** When the swing up and down starts, in seconds: a quarter of a turn either way. */
    upDown: number;
    /** Seconds for each weave across, and up and down. */
    weaveAcross: number;
    weaveUpDown: number;
}

/**
 * Each fly's own pace: how long a turn takes, which way it goes and how quick it weaves. No two
 * alike, so they never fall into step; the weave is unhurried, so they drift rather than jitter.
 */
const PACES: readonly {
    turn: number;
    backwards: boolean;
    weaveAcross: number;
    weaveUpDown: number;
}[] = [
    { turn: 3.4, backwards: false, weaveAcross: 0.62, weaveUpDown: 0.47 },
    { turn: 4.2, backwards: true, weaveAcross: 0.78, weaveUpDown: 0.66 },
    { turn: 5, backwards: false, weaveAcross: 0.55, weaveUpDown: 0.71 },
];

/** The flights of `size` flies, spread round the mark, up to `FLIES_MOST`. */
export function flights(size: number): Flight[] {
    const shown = PACES.slice(0, Math.max(size, 0));
    return shown.map((pace, at) => {
        const along = -(at * pace.turn) / shown.length;
        const behind = pace.backwards ? (pace.turn * 3) / 4 : pace.turn / 4;
        return {
            halfTurn: seconds(pace.turn / 2),
            across: seconds(along),
            upDown: seconds(along - behind),
            weaveAcross: pace.weaveAcross,
            weaveUpDown: pace.weaveUpDown,
        };
    });
}
