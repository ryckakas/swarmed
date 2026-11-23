import { useEffect, useState } from "react";
import type { AccountUsage } from "./api/types";
import { untilNextReset } from "./rateLimit";
import { useStore } from "./store";

/**
 * The account's usage as any Claude Code session last heard it, with the time to read it at,
 * in seconds since 1970. It is drawn again when the next window resets, so that a warning goes
 * when its window does. Nothing is asked of Claude Code for that: it only redraws.
 */
export function useAccountUsage(): { usage: AccountUsage | null; now: number } {
    const usage = useStore((state) => state.accountUsage);
    const [, redraw] = useState(0);
    const now = Date.now() / 1000;
    const wait = untilNextReset(usage, now);
    useEffect(() => {
        if (wait === null) {
            return;
        }
        const timer = setTimeout(() => redraw((count) => count + 1), wait);
        return () => clearTimeout(timer);
    }, [wait]);
    return { usage, now };
}
