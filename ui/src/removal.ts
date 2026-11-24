/**
 * What the core says when another Swarmed on the same data, such as the command line, runs the
 * agent of a session it was asked about (`Error::RunsElsewhere`).
 */
const RUNS_ELSEWHERE = "runs in another Swarmed";

/**
 * Whether removing anyway can get past what stopped a removal. It can for unsaved work, which
 * it discards; not for an agent another Swarmed runs in the worktree, which only that one can
 * stop.
 */
export function anywayHelps(problem: string): boolean {
    return !problem.includes(RUNS_ELSEWHERE);
}

/** What the removal dialog says of what stopped the last attempt. */
export function refusalSaid(problem: string): string {
    return anywayHelps(problem)
        ? `Not removed: ${problem}. Removing anyway discards that work.`
        : `Not removed: ${problem}.`;
}
