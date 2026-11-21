// When the user last sent each session a command that Swarmed does itself. Kept apart from the
// store: nothing is drawn from it, it is only looked at as a question appears.

/** How long after a command was sent a question still counts as what that command asked. */
const JUST_SENT_MS = 3000;

const sent = new Map<number, number>();

/** Notes that the user has just sent the session a command of the kind Swarmed takes. */
export function noteCommandSent(sessionId: number, now = Date.now()): void {
    sent.set(sessionId, now);
}

/**
 * Whether a command's question that appears now is the one the user has just asked for. One
 * left unanswered and drawn again later, when its session is opened, is not: it must not take
 * the keys from a message box the user is about to type into.
 */
export function commandJustSent(sessionId: number, now = Date.now()): boolean {
    const at = sent.get(sessionId);
    return at !== undefined && now >= at && now - at <= JUST_SENT_MS;
}
