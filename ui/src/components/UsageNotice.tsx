import { onExtraUsageNow } from "../extraUsage";
import { accountWindows, dismissalOf, usageNotice, usageNoticeText } from "../rateLimit";
import { useStore } from "../store";
import { findSession } from "../tree";
import { useAccountUsage } from "../useAccountUsage";
import { CLAUDE } from "./options";

/**
 * The account's one usage notice, at the top of the window while a Claude Code session is on
 * screen: the highest level any usage window has reached, from the account's newest figures,
 * so the same in every session. Dismissed, it stays away in all of them until a window reaches
 * a higher level, one that reset climbs back to a level, or one first reported since reaches
 * one (`heldBack`).
 */
export function UsageNotice() {
    const session = useStore((state) => findSession(state.projects, state.selectedSessionId));
    const mode = useStore((state) => state.settings?.limit_warnings ?? "frequent");
    const dismissed = useStore((state) => state.usageDismissed);
    const dismiss = useStore((state) => state.dismissUsage);
    const { usage, now } = useAccountUsage();
    if (session?.agent !== CLAUDE) {
        return null;
    }
    const windows = accountWindows(usage, now);
    const paying = onExtraUsageNow(usage, now);
    const notice = usageNotice(windows, paying, mode, dismissed, now);
    if (notice === null) {
        return null;
    }
    return (
        <div className="usage-notice" role="status">
            <span>{usageNoticeText(notice, new Date(now * 1000))}</span>
            <button
                type="button"
                className="button"
                onClick={() => dismiss(dismissalOf(windows, paying))}
            >
                Dismiss
            </button>
        </div>
    );
}
