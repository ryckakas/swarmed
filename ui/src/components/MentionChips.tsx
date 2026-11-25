import { api } from "../api";
import { type Mentioned, mentionPieces } from "../mentions";
import { useStore } from "../store";
import { findSession } from "../tree";

/**
 * A session a message mentions, by its title as it is now: pressing it opens that session.
 * One that is closed or gone keeps the title it had when the message went, and opens nothing.
 */
export function MentionChip({ session }: { session: Mentioned }) {
    const now = useStore((state) => findSession(state.projects, session.id));
    const report = useStore((state) => state.report);
    const title = (now ? now.title : session.title) || `Session ${session.id}`;
    const hint = now
        ? `${title}: open session ${session.id}`
        : `${title}: session ${session.id} is closed or gone`;
    return (
        <button
            type="button"
            className="mention-chip"
            title={hint}
            disabled={!now}
            onClick={() => void useStore.getState().open(api, session.id).catch(report)}
        >
            <span className="mention-at" aria-hidden="true">
                @
            </span>
            {title}
        </button>
    );
}

/** What the user wrote, with the sessions listed under the message drawn where their tokens stand. */
export function WithMentions({ text, sessions }: { text: string; sessions: Mentioned[] }) {
    if (sessions.length === 0) {
        return <span>{text}</span>;
    }
    return mentionPieces(text, sessions).map((piece, index) =>
        typeof piece === "string" ? (
            // biome-ignore lint/suspicious/noArrayIndexKey: the pieces of a sent message never change
            <span key={index}>{piece}</span>
        ) : (
            // biome-ignore lint/suspicious/noArrayIndexKey: a session can be named twice
            <MentionChip key={index} session={piece} />
        ),
    );
}

/** Sessions a message mentions that its words name nowhere, as on the command line: under it. */
export function MentionList({ sessions }: { sessions: Mentioned[] }) {
    if (sessions.length === 0) {
        return null;
    }
    return (
        <ul className="mention-chips" aria-label="Sessions mentioned">
            {sessions.map((session) => (
                <li key={session.id}>
                    <MentionChip session={session} />
                </li>
            ))}
        </ul>
    );
}
