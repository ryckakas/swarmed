import { useState } from "react";
import { api } from "../api";
import type { Session } from "../api/types";
import {
    DEFAULT_SNOOZE_HOURS,
    fieldTime,
    snoozeChoices,
    snoozed,
    timeOfField,
    wakeTime,
} from "../snooze";
import { useStore } from "../store";
import { findSession } from "../tree";
import { useWords } from "../useWords";
import { Dialog } from "./Dialog";

/**
 * Snoozes a session until a time: out of the sidebar's list and the inbox until then, back early
 * when it needs the user, its agent never stopped. Opened from the session's row, its header and
 * the shortcut.
 */
export function SnoozeDialog() {
    const session = useStore((state) => findSession(state.projects, state.snoozing));
    if (!session) {
        return null;
    }
    // Asked for another, it starts again: nothing typed for the last carries over.
    return <SnoozeChoices key={session.id} session={session} />;
}

/** Each fixed choice with its time, a time of the user's own, and a way to wake it now. */
function SnoozeChoices({ session }: { session: Session }) {
    const words = useWords();
    const askToSnooze = useStore((state) => state.askToSnooze);
    const refresh = useStore((state) => state.refresh);
    const settings = useStore((state) => state.settings);
    const asleep = useStore((state) => snoozed(session, state.snoozeNow));
    // The choices are those of the moment the dialog opened, as shown.
    const [now] = useState(() => new Date());
    const hours = {
        morning: settings?.snooze_morning_hour ?? DEFAULT_SNOOZE_HOURS.morning,
        evening: settings?.snooze_evening_hour ?? DEFAULT_SNOOZE_HOURS.evening,
    };
    const choices = snoozeChoices(now, hours);
    const [own, setOwn] = useState(() => fieldTime(choices.at(-1)?.at ?? now.getTime() / 1000));
    const [problem, setProblem] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);
    const ownAt = timeOfField(own);
    const ownPast = ownAt !== null && ownAt * 1000 <= Date.now();
    const close = () => askToSnooze(null);
    const snooze = async (until: number | null) => {
        setBusy(true);
        try {
            await api.sessionSnooze(session.id, until);
            close();
            await refresh(api);
        } catch (error) {
            setProblem(String(error));
            setBusy(false);
        }
    };
    return (
        <Dialog
            title="Snooze until"
            size="small"
            onClose={close}
            dismiss="Cancel"
            problem={problem}
            busy={busy}
            aside={
                asleep && (
                    <button
                        type="button"
                        className="button"
                        disabled={busy}
                        onClick={() => void snooze(null)}
                    >
                        {words.wake} now
                    </button>
                )
            }
        >
            <p className="dialog-subject">{session.title || words.untitled}</p>
            <p>
                Out of the sidebar's list and the inbox until then. It comes back early when you
                write to it, it asks for something or it fails. Its agent is not stopped.
            </p>
            <ul className="snooze-choices">
                {choices.map((choice, at) => (
                    <li key={choice.label}>
                        <button
                            type="button"
                            className="snooze-choice"
                            // The keyboard starts on the choices, not on the field below them.
                            data-dialog-start={at === 0 ? "" : undefined}
                            disabled={busy}
                            onClick={() => void snooze(choice.at)}
                        >
                            <span>{choice.label}</span>
                            <span className="snooze-when">{wakeTime(choice.at, now)}</span>
                        </button>
                    </li>
                ))}
            </ul>
            <div className="snooze-own">
                <label className="snooze-own-label">
                    A time of your own
                    <input
                        className="key-input"
                        type="datetime-local"
                        value={own}
                        min={fieldTime(now.getTime() / 1000)}
                        onChange={(event) => setOwn(event.target.value)}
                    />
                </label>
                <button
                    type="button"
                    className="button"
                    disabled={busy || ownAt === null || ownPast}
                    title={ownPast ? "That time has passed" : undefined}
                    onClick={() => ownAt !== null && void snooze(ownAt)}
                >
                    Snooze
                </button>
            </div>
            {ownPast && <p className="hint">That time has passed: choose one still to come.</p>}
        </Dialog>
    );
}
