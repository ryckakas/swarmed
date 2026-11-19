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
