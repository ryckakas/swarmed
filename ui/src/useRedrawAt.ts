import { useEffect, useState } from "react";

/**
 * Draws the component again once the clock reaches `at`, in seconds since 1970, so that what
 * it offers until then goes in time. `null`, or a time that has passed, sets nothing. It only
 * redraws: nothing is asked of anyone.
 */
export function useRedrawAt(at: number | null): void {
    const [, redraw] = useState(0);
    useEffect(() => {
        if (at === null) {
            return;
        }
        const wait = at * 1000 - Date.now();
        if (wait <= 0) {
            return;
        }
        const timer = setTimeout(() => redraw((count) => count + 1), wait);
        return () => clearTimeout(timer);
    }, [at]);
}
