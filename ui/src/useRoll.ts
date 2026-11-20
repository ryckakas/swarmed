import { useEffect, useState } from "react";

/** How often something with several words shows another one, in milliseconds. */
const NEW_WORD_EVERY = 5000;

/**
 * A number from 0 up to, not including, 1, to choose one of several words with. While `varied`
 * it is rolled again now and then, so the words change; otherwise it stays as it is.
 */
export function useRoll(varied: boolean): number {
    const [roll, setRoll] = useState(Math.random);
    useEffect(() => {
        if (!varied) {
            return;
        }
        const timer = setInterval(() => setRoll(Math.random()), NEW_WORD_EVERY);
        return () => clearInterval(timer);
    }, [varied]);
    return roll;
}
