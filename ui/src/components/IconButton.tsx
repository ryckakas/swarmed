import type { ReactNode } from "react";

interface Props {
    /** What the button does. Read by screen readers and shown when the pointer rests on it. */
    label: string;
    onClick(): void;
    /** A button whose action cannot be undone is drawn in the colour of an error. */
    danger?: boolean;
    children: ReactNode;
}

/** A button that is only an icon. Its label is its tooltip. */
export function IconButton({ label, onClick, danger = false, children }: Props) {
    return (
        <button
            type="button"
            className={danger ? "icon-button icon-button-danger" : "icon-button"}
            aria-label={label}
            title={label}
            onClick={onClick}
        >
            <svg viewBox="0 0 16 16" aria-hidden="true">
                {children}
            </svg>
        </button>
    );
}

/** A window with its right-hand panel marked. */
export function PanelIcon() {
    return (
        <>
            <rect x="2" y="3" width="12" height="10" rx="2" />
            <path d="M10 3v10" />
        </>
    );
}

/** A cross. */
export function CloseIcon() {
    return <path d="M4 4l8 8M12 4l-8 8" />;
}

/** A cross in a circle: ending something, not just hiding it. */
export function EndIcon() {
    return (
        <>
            <circle cx="8" cy="8" r="6" />
            <path d="M6 6l4 4M10 6l-4 4" />
        </>
    );
}

/** One line that becomes two: a conversation carried on in a second session. */
export function ForkIcon() {
    return (
        <>
            <circle cx="4.5" cy="3.5" r="1.5" />
            <circle cx="4.5" cy="12.5" r="1.5" />
            <circle cx="11.5" cy="6" r="1.5" />
            <path d="M4.5 5v6M4.5 9c0-1.5 1-3 5.5-3" />
        </>
    );
}

/** A window with a prompt in it: a terminal. */
export function TerminalIcon() {
    return (
        <>
            <rect x="2" y="3" width="12" height="10" rx="2" />
            <path d="M5 6.5l2 1.5-2 1.5M8.5 10H11" />
        </>
    );
}

/** Angle brackets around a slash: code, in an editor. */
export function EditorIcon() {
    return <path d="M5.5 4.5L2.5 8l3 3.5M10.5 4.5l3 3.5-3 3.5M9 3.5l-2 9" />;
}

/** Two coins, one over the other: what is used, and what it costs. */
export function CoinsIcon() {
    return (
        <>
            <circle cx="6" cy="6.2" r="4.2" />
            <path d="M10.18 5.8A4.2 4.2 0 1 1 5.82 10.4" />
            <path d="M5.3 4.7h.9v3M11.4 9.6l.5.5-1.9 1.9" />
        </>
    );
}

/** A cloud: work handed to an agent that runs in one. */
export function CloudIcon() {
    return <path d="M4.5 12.5a3 3 0 0 1-.4-5.97 4 4 0 0 1 7.7-.9A3.4 3.4 0 0 1 11.5 12.5z" />;
}
