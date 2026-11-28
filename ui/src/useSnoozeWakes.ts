import { useEffect } from "react";
import { nextWake, untilWake } from "./snooze";
import { useStore } from "./store";

/**
 * Wakes snoozed sessions on time, for as long as the app is on screen: a timer set for the next
 * one's time moves the store's `snoozeNow` on, which draws the sidebar, the inbox and the Dock's
 * count again, and unfolds the task of a session that woke. Nothing is asked of the core, and
 * nothing stored changes. A timer is never set for longer than a browser allows; one that ran
 * that long only looks again. A sleeping Mac holds timers back, so the snoozes are looked at
 * again too when the window gets the focus or comes into view.
 */
export function useSnoozeWakes(): void {
    const wake = useStore((state) => nextWake(state.projects, state.snoozeNow));
    const now = useStore((state) => state.snoozeNow);
    const wakeDue = useStore((state) => state.wakeDue);
    // Set again whenever the time looked at moves on, so that a timer cut short by the longest
    // a browser allows is followed by the next.
    // biome-ignore lint/correctness/useExhaustiveDependencies: restarted when the time looked at moves on
    useEffect(() => {
        if (wake === null) {
            return;
        }
        const timer = setTimeout(wakeDue, untilWake(wake, Date.now()));
        return () => clearTimeout(timer);
    }, [wake, now, wakeDue]);
    useEffect(() => {
        const look = () => {
            if (document.visibilityState === "visible") {
                wakeDue();
            }
        };
        window.addEventListener("focus", look);
        document.addEventListener("visibilitychange", look);
        return () => {
            window.removeEventListener("focus", look);
            document.removeEventListener("visibilitychange", look);
        };
    }, [wakeDue]);
}
