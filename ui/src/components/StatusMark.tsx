import { useEffect, useState } from "react";
import type { SessionStatus } from "../api/types";
import { DEFAULT_STYLE } from "../appearance";
import { useStore } from "../store";
import { pickWord, type StatusKind, statusKind, wordsFor } from "../wording";

/** How often a status with several words shows another one, in milliseconds. */
const NEW_WORD_EVERY = 5000;

/**
 * What a session's status is called, in the words of the style in use. A status with several
 * words gets one at random, and another one now and then.
 */
export function useStatusLabel(status: SessionStatus, unread = false): string {
    const kind = statusKind(status, unread);
    const words = useStore((state) => wordsFor(state.settings?.style).status[kind]);
    const [roll, setRoll] = useState(Math.random);
    useEffect(() => {
        if (words.length < 2) {
            return;
        }
        const timer = setInterval(() => setRoll(Math.random()), NEW_WORD_EVERY);
        return () => clearInterval(timer);
    }, [words]);
    return pickWord(words, roll);
}

interface Props {
    status: SessionStatus;
    /** Whether the session holds an answer nobody has looked at. */
    unread?: boolean;
    /** What to call the status, where the words beside the mark already chose; else its own. */
    label?: string;
}

/**
 * A small mark for a session's status, which says what it means when pointed at. Each status
 * has its own shape, not only its own colour. An idle session that has answered and not been
 * looked at is told apart from one that is merely idle; a session that is busy, waiting or
 * failed already says more than "unread" would.
 */
export function StatusMark({ status, unread = false, label }: Props) {
    const own = useStatusLabel(status, unread);
    const style = useStore((state) => state.settings?.style ?? DEFAULT_STYLE);
    const kind = statusKind(status, unread);
    const name = label ?? own;
    if (style === "swarm" && kind === "idle") {
        return <Egg label={name} />;
    }
    if (style === "swarm" && (kind === "working" || kind === "unread")) {
        return <Larva kind={kind} label={name} />;
    }
    return <span className={`mark mark-${kind}`} role="img" aria-label={name} title={name} />;
}

/** Dormant: a sac standing on three roots, waiting. */
function Egg({ label }: { label: string }) {
    return (
        <svg className="egg" viewBox="0 0 12 14" role="img" aria-label={label}>
            <title>{label}</title>
            <path className="egg-sac" d="M6 .8C3.4.8 2 4.2 2 7a4 4 0 0 0 8 0C10 4.2 8.6.8 6 .8z" />
            <path d="M.6 12.6c1.6-.2 2.6-.9 3.2-2M11.4 12.6c-1.6-.2-2.6-.9-3.2-2M6 11v1.9" />
        </svg>
    );
}

/**
 * A tapering grub with pincers at its head: crawling while its agent works, glowing once it has
 * hatched an answer.
 */
function Larva({ kind, label }: { kind: StatusKind; label: string }) {
    return (
        <svg className={`larva larva-${kind}`} viewBox="0 0 16 10" role="img" aria-label={label}>
            <title>{label}</title>
            <circle cx="2.3" cy="5.6" r="1.2" />
            <circle cx="4.5" cy="5.3" r="1.8" />
            <circle cx="7.2" cy="5" r="2.3" />
            <circle cx="10.3" cy="5" r="2.7" />
            <path d="M12.6 3.4c1.4-.9 2.3-.5 2.6.5M12.6 6.6c1.4.9 2.3.5 2.6-.5" />
        </svg>
    );
}
