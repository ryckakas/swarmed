import { api } from "../api";
import { type GitHubMentioned, githubSaid, mentionedPieces } from "../githubMentions";
import type { Mentioned } from "../mentions";
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

/**
 * A pull request or an issue a message mentions, coloured by its state when the message went:
 * pressing it opens it on GitHub.
 */
export function GitHubChip({ item, shown }: { item: GitHubMentioned; shown?: string }) {
    const report = useStore((state) => state.report);
    const title = shown ?? item.title;
    return (
        <button
            type="button"
            className="mention-chip github-chip"
            data-state={item.state}
            title={`${githubSaid(item)}: ${item.title}. Open it on GitHub`}
            onClick={() => void api.openLink(item.url).catch(report)}
        >
            <span className="github-number" data-state={item.state}>
                #{item.number}
            </span>
            {title && ` ${title}`}
        </button>
    );
}

interface MentionedProps {
    text: string;
    sessions: Mentioned[];
    github: GitHubMentioned[];
}

/**
 * What the user wrote, with the sessions, pull requests and issues listed under the message
 * drawn where their tokens stand.
 */
export function WithMentions({ text, sessions, github }: MentionedProps) {
    if (sessions.length === 0 && github.length === 0) {
        return <span>{text}</span>;
    }
    return mentionedPieces(text, sessions, github).map((piece, index) => {
        if (typeof piece === "string") {
            // biome-ignore lint/suspicious/noArrayIndexKey: the pieces of a sent message never change
            return <span key={index}>{piece}</span>;
        }
        if ("session" in piece) {
            // biome-ignore lint/suspicious/noArrayIndexKey: a session can be named twice
            return <MentionChip key={index} session={piece.session} />;
        }
        // biome-ignore lint/suspicious/noArrayIndexKey: a pull request can be named twice
        return <GitHubChip key={index} item={piece.github} shown={piece.github.shown} />;
    });
}

/**
 * Sessions, pull requests and issues a message mentions that its words name nowhere, as on the
 * command line: under it.
 */
export function MentionList({ sessions, github }: Omit<MentionedProps, "text">) {
    if (sessions.length === 0 && github.length === 0) {
        return null;
    }
    return (
        <ul className="mention-chips" aria-label="Mentioned">
            {sessions.map((session) => (
                <li key={`@${session.id}`}>
                    <MentionChip session={session} />
                </li>
            ))}
            {github.map((item) => (
                <li key={`#${item.number}`}>
                    <GitHubChip item={item} />
                </li>
            ))}
        </ul>
    );
}
