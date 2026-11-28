import type { ReactNode } from "react";

interface Props {
    /** What the button does. Read by screen readers and shown when the pointer rests on it. */
    label: string;
    onClick(): void;
    /** A button whose action cannot be undone is drawn in the colour of an error. */
    danger?: boolean;
    /** A button with nothing to do now, such as Back on the first page. */
    disabled?: boolean;
    /** For a button that turns something on and off: whether it is on. */
    pressed?: boolean;
    children: ReactNode;
}

/** A button that is only an icon. Its label is its tooltip. */
export function IconButton({ label, onClick, danger = false, disabled, pressed, children }: Props) {
    return (
        <button
            type="button"
            className={danger ? "icon-button icon-button-danger" : "icon-button"}
            aria-label={label}
            aria-pressed={pressed}
            title={label}
            disabled={disabled}
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

/** A plus: something new, added. */
export function PlusIcon() {
    return <path d="M8 3v10M3 8h10" />;
}

/** A plus over a minus: what was added and what was taken away. */
export function PlusMinusIcon() {
    return <path d="M8 2.5v7M4.5 6h7M4.5 13.5h7" />;
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

/** A phone with a signal beside it: a session reached from elsewhere, through Remote Control. */
export function RemoteIcon() {
    return (
        <>
            <rect x="3" y="2" width="6.5" height="12" rx="1.4" />
            <path d="M5.4 11.6h1.7M11.6 5.6a3 3 0 0 1 0 4.8M13.4 4a5.4 5.4 0 0 1 0 8" />
        </>
    );
}

/** A cloud: work handed to an agent that runs in one. */
export function CloudIcon() {
    return <path d="M4.5 12.5a3 3 0 0 1-.4-5.97 4 4 0 0 1 7.7-.9A3.4 3.4 0 0 1 11.5 12.5z" />;
}

/** A globe: a web page, the task's own browser. */
export function GlobeIcon() {
    return (
        <>
            <circle cx="8" cy="8" r="6" />
            <path d="M2 8h12M8 2c-2 2-2 10 0 12M8 2c2 2 2 10 0 12" />
        </>
    );
}

/**
 * The globe of a browser that runs: two meridians a quarter of a turn apart, each narrowing to a
 * line and widening again, so that it turns, slowly.
 */
export function TurningGlobeIcon() {
    return (
        <>
            <circle cx="8" cy="8" r="6" />
            <path d="M2 8h12" />
            <ellipse className="globe-meridian" cx="8" cy="8" rx="6" ry="6" />
            <ellipse className="globe-meridian" cx="8" cy="8" rx="6" ry="6" />
        </>
    );
}

/** An arrow to the left: back. */
export function BackIcon() {
    return <path d="M13 8H3M7 4L3 8l4 4" />;
}

/** An arrow to the right: forward. */
export function ForwardIcon() {
    return <path d="M3 8h10M9 4l4 4-4 4" />;
}

/** A circling arrow: load again. */
export function ReloadIcon() {
    return <path d="M13 8a5 5 0 1 1-1.5-3.5M13 2.5v3h-3" />;
}

/** An arrow out of a box: opened elsewhere. */
export function OpenOutIcon() {
    return <path d="M9 3h4v4M13 3L7 9M11 10v3H3V5h3" />;
}

/** An arrow pointing into a frame: picking something on a page, as developer tools do. */
export function PickIcon() {
    return (
        <>
            <path d="M6.5 13.5h-3a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1h9a1 1 0 0 1 1 1v3" />
            <path d="M8 8l6 2.2-2.6 1-1.1 2.8z" />
        </>
    );
}

/** A power switch: stopping what runs. */
export function PowerIcon() {
    return <path d="M8 2v6M5 4.2a5 5 0 1 0 6 0" />;
}

/** Two sheets, one in front of the other: a copy. */
export function CopyIcon() {
    return (
        <>
            <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
            <path d="M10.5 3.5V3A1.5 1.5 0 0 0 9 1.5H4A1.5 1.5 0 0 0 2.5 3v5A1.5 1.5 0 0 0 4 9.5h.5" />
        </>
    );
}

/** An alarm clock: snoozing until a time. */
export function SnoozeIcon() {
    return (
        <>
            <circle cx="8" cy="8.8" r="5.2" />
            <path d="M8 6v2.8l1.8 1.2M2.6 4.2 4.6 2.4M13.4 4.2l-2-1.8" />
        </>
    );
}
