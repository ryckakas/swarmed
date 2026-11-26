import { useEffect, useRef, useState } from "react";
import type { BackgroundCount, SessionStatus } from "../api/types";
import { isSwarm } from "../appearance";
import {
    anyInBackground,
    type BackgroundLevel,
    backgroundShown,
    NOTHING_IN_BACKGROUND,
} from "../inBackground";
import { useStore } from "../store";
import { useRoll } from "../useRoll";
import { pickWord, type StatusKind, statusKind, wordsFor } from "../wording";

/**
 * What a session's status is called, in the words of the style in use. A status with several
 * words gets one at random, and another one now and then. With work left in the background,
 * that is said too: alone beside idle, which it stands in for, else after the status.
 */
export function useStatusLabel(
    status: SessionStatus,
    unread = false,
    background: BackgroundCount = NOTHING_IN_BACKGROUND,
): string {
    const kind = statusKind(status, unread);
    const style = useStore((state) => wordsFor(state.settings?.style));
    const words = style.status[kind];
    const word = pickWord(words, useRoll(words.length > 1));
    if (!anyInBackground(background)) {
        return word;
    }
    const running = style.inBackground(background);
    return kind === "idle" ? running : `${word}, ${running}`;
}

/** Whether the mark just went from working to holding an unseen answer: the moment it hatched. */
function useHatching(kind: StatusKind): boolean {
    const before = useRef(kind);
    const [hatching, setHatching] = useState(false);
    useEffect(() => {
        setHatching(before.current === "working" && kind === "unread");
        before.current = kind;
    }, [kind]);
    return hatching;
}

interface Props {
    status: SessionStatus;
    /** Whether the session holds an answer nobody has looked at. */
    unread?: boolean;
    /** What the session, or the sessions it stands for, left running in the background. */
    background?: BackgroundCount;
    /** What to call the status, where the words beside the mark already chose; else its own. */
    label?: string;
}

/**
 * A small mark for a session's status, which says what it means when pointed at. Each status
 * has its own shape, not only its own colour. An idle session that has answered and not been
 * looked at is told apart from one that is merely idle; a session that is busy, waiting or
 * failed already says more than "unread" would. An idle one with work left in the background
 * is never drawn as merely idle: a ring around it says so, clear for sub-agents at work and
 * quiet for commands left running.
 */
export function StatusMark({
    status,
    unread = false,
    background = NOTHING_IN_BACKGROUND,
    label,
}: Props) {
    const own = useStatusLabel(status, unread, background);
    const swarm = useStore((state) => isSwarm(state.settings?.style));
    const kind = statusKind(status, unread);
    const hatching = useHatching(kind);
    const ring = backgroundShown(status, background) ?? undefined;
    const name = label ?? own;
    if (!swarm) {
        return (
            <span
                className={`mark mark-${kind}`}
                data-background={ring}
                role="img"
                aria-label={name}
                title={name}
            />
        );
    }
    const Shape = SHAPES[kind];
    return <Shape label={name} hatching={hatching} background={ring} />;
}

interface ShapeProps {
    label: string;
    /** Whether to flash once, as what just hatched. */
    hatching: boolean;
    /** What runs in the background, which a ring around the shape says. */
    background?: BackgroundLevel | undefined;
}

/** Dormant: a sac standing on three roots, waiting. */
export function Egg({
    label,
    background,
}: {
    label: string;
    background?: BackgroundLevel | undefined;
}) {
    return (
        <svg
            className="egg"
            data-background={background}
            viewBox="0 0 12 14"
            role="img"
            aria-label={label}
        >
            <title>{label}</title>
            <path
                className="swarm-body"
                d="M6 .8C3.4.8 2 4.2 2 7a4 4 0 0 0 8 0C10 4.2 8.6.8 6 .8z"
            />
            <path d="M.6 12.6c1.6-.2 2.6-.9 3.2-2M11.4 12.6c-1.6-.2-2.6-.9-3.2-2M6 11v1.9" />
        </svg>
    );
}

/** A tapering grub with pincers at its head, crawling while its agent works. */
function Crawling({ label }: ShapeProps) {
    return (
        <svg className="larva larva-working" viewBox="0 0 16 10" role="img" aria-label={label}>
            <title>{label}</title>
            <Grub />
        </svg>
    );
}

/** The same grub, glowing: it has hatched an answer nobody has looked at. */
function Hatched({ label, hatching, background }: ShapeProps) {
    return (
        <svg
            className="larva larva-unread"
            data-hatching={hatching}
            data-background={background}
            viewBox="0 0 16 10"
            role="img"
            aria-label={label}
        >
            <title>{label}</title>
            <Grub />
        </svg>
    );
}

function Grub() {
    return (
        <>
            <circle cx="2.3" cy="5.6" r="1.2" />
            <circle cx="4.5" cy="5.3" r="1.8" />
            <circle cx="7.2" cy="5" r="2.3" />
            <circle cx="10.3" cy="5" r="2.7" />
            <path d="M12.6 3.4c1.4-.9 2.3-.5 2.6.5M12.6 6.6c1.4.9 2.3.5 2.6-.5" />
        </>
    );
}

/** Needs you: the grub reared up, pincers open. */
function Reared({ label }: ShapeProps) {
    return (
        <svg className="larva larva-waiting" viewBox="0 0 16 12" role="img" aria-label={label}>
            <title>{label}</title>
            <circle cx="2.6" cy="10" r="1.3" />
            <circle cx="4.9" cy="9.3" r="1.8" />
            <circle cx="7.6" cy="7.8" r="2.2" />
            <circle cx="9.8" cy="5.2" r="2.4" />
            <path d="M10.4 2.9c0-1.6.9-2.4 2-2.3M12.2 5.6c1.5.6 2.6.2 3-.8" />
        </svg>
    );
}

/** Stopped with an error: a sac burst open, its top torn off. */
function Burst({ label }: ShapeProps) {
    return (
        <svg className="egg egg-burst" viewBox="0 0 12 14" role="img" aria-label={label}>
            <title>{label}</title>
            <path
                className="swarm-body"
                d="M2 7.4 3.4 5l1.3 2 1.3-2.6L7.3 7l1.3-2L10 7.4a4 4 0 0 1-8 0z"
            />
            <path d="M.6 12.6c1.6-.2 2.6-.9 3.2-2M11.4 12.6c-1.6-.2-2.6-.9-3.2-2M6 11v1.9" />
            <path d="M3.4 1.6l.6 1.2M8.6 1.6 8 2.8M6 .6v1.3" />
        </svg>
    );
}

const SHAPES: Record<StatusKind, (props: ShapeProps) => React.ReactNode> = {
    idle: Egg,
    working: Crawling,
    unread: Hatched,
    waiting: Reared,
    errored: Burst,
};
