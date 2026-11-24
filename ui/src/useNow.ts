import { useEffect, useState } from "react";

/**
 * The time now, in milliseconds since 1970, read again every `every` milliseconds while
 * `ticking`, so that what says how long something has run draws again. Not ticking, it stays
 * as it was and nothing runs.
 */
export function useNow(ticking: boolean, every = 1000): number {
    const [now, setNow] = useState(() => Date.now());
    useEffect(() => {
        if (!ticking) {
            return;
        }
        setNow(Date.now());
        const timer = setInterval(() => setNow(Date.now()), every);
        return () => clearInterval(timer);
    }, [ticking, every]);
    return now;
}
