import {
    type KeyboardEvent,
    type MouseEvent,
    type PointerEvent,
    type ReactNode,
    type SyntheticEvent,
    useEffect,
    useId,
    useRef,
} from "react";
import { CloseIcon, IconButton } from "./IconButton";

/**
 * How wide a dialog is, by what it holds: a question, a form or a list, the settings, or two
 * things side by side. Its height follows what it holds, up to a limit, past which its body
 * scrolls.
 */
export type DialogSize = "small" | "medium" | "large" | "wide";

/**
 * Where the keyboard starts when a dialog opens: the first element its content marks with
 * `data-dialog-start`, else the body's first field, else the main button, else the footer's
 * first button. A question whose action cannot be undone has no main button, so a stray Enter
 * takes its way out.
 */
const STARTS = [
    "[data-dialog-start]",
    ".dialog-body :is(input:not([type='hidden']), textarea, select):not(:disabled)",
    ".dialog-foot .button-primary:not(:disabled)",
    ".dialog-foot button:not(:disabled)",
];

function startOf(dialog: HTMLDialogElement): HTMLElement | null {
    for (const selector of STARTS) {
        const found = dialog.querySelector<HTMLElement>(selector);
        if (found) {
            return found;
        }
    }
    return null;
}

interface Props {
    /** The heading, which also names the dialog to a screen reader. */
    title: string;
    size: DialogSize;
    /** Called by Esc, the ×, the footer's way out, and a click beside it where that closes it. */
    onClose(): void;
    /**
     * What the footer's way out says, before its other buttons: Close unless given. `null`
     * leaves it out, where the footer's buttons are for something inside the dialog.
     */
    dismiss?: string | null;
    /** The footer's other buttons, the main one last. Without any, the way out is the main one. */
    actions?: ReactNode;
    /** What stands at the footer's start, apart from its buttons: a Remove, or a menu. */
    aside?: ReactNode;
    /** What went wrong, said just above the footer, where scrolling cannot hide it. */
    problem?: string | null;
    /**
     * While an action runs: the footer's buttons other than the way out are disabled, and a
     * click beside the dialog does not close it. Closing it stops nothing that runs.
     */
    busy?: boolean;
    /**
     * Whether a click beside the dialog closes it. Only where nothing would be lost: never
     * with typed or chosen input, and never while an action runs.
     */
    closeOnBackdrop?: boolean;
    /** One height whatever it holds, so that switching what it shows does not make it jump. */
    fixedHeight?: boolean;
    /**
     * The body is filled by panes that scroll on their own, as the settings' tabs and panel
     * are, and has no padding or scrolling of its own.
     */
    panes?: boolean;
    /** A class for what the content draws, never for the dialog's own frame. */
    className?: string;
    onKeyDown?(event: KeyboardEvent<HTMLDialogElement>): void;
    children: ReactNode;
}

/**
 * Every dialog in the app is this one: a head with its title and a ×, a body that alone
 * scrolls, and a footer with its buttons that stays put. It is modal, so the keyboard stays
 * inside, and Esc, the × and the footer's way out all close it. A dialog opened from another
 * is closed by Esc on its own, and the one under it stays.
 */
export function Dialog(props: Props) {
    const { title, size, onClose, busy = false, closeOnBackdrop = false } = props;
    const dialog = useRef<HTMLDialogElement>(null);
    const titleId = useId();
    // A click that began inside, such as text selected past the edge, is no click beside it.
    const pressedBeside = useRef(false);

    useEffect(() => {
        const element = dialog.current;
        if (element && !element.open) {
            element.showModal();
            startOf(element)?.focus();
        }
    }, []);

    const onDialogClose = (event: SyntheticEvent<HTMLDialogElement>) => {
        // A dialog opened from this one says it closed through this one too.
        if (event.target === event.currentTarget) {
            event.stopPropagation();
            onClose();
        }
    };
    const onPointerDown = (event: PointerEvent<HTMLDialogElement>) => {
        pressedBeside.current = event.target === event.currentTarget;
    };
    const onClick = (event: MouseEvent<HTMLDialogElement>) => {
        const beside = pressedBeside.current && event.target === event.currentTarget;
        if (beside && closeOnBackdrop && !busy) {
            onClose();
        }
    };
    return (
        <dialog
            ref={dialog}
            className={props.className ? `dialog ${props.className}` : "dialog"}
            data-size={size}
            data-fixed={props.fixedHeight || undefined}
            aria-labelledby={titleId}
            aria-busy={busy}
            onClose={onDialogClose}
            onKeyDown={props.onKeyDown}
            onPointerDown={onPointerDown}
            onClick={onClick}
        >
            <header className="dialog-head">
                <h2 id={titleId} className="dialog-title">
                    {title}
                </h2>
                <IconButton label="Close" onClick={onClose}>
                    <CloseIcon />
                </IconButton>
            </header>
            <div className="dialog-body" data-panes={props.panes || undefined}>
                {props.children}
            </div>
            {props.problem && (
                <p className="dialog-problem" role="alert">
                    {props.problem}
                </p>
            )}
            <Foot
                dismiss={props.dismiss === undefined ? "Close" : props.dismiss}
                actions={props.actions}
                aside={props.aside}
                busy={busy}
                onClose={onClose}
            />
        </dialog>
    );
}

interface FootProps {
    dismiss: string | null;
    actions: ReactNode;
    aside: ReactNode;
    busy: boolean;
    onClose(): void;
}

/** The footer: what stands apart at its start, then the way out, then the actions. */
function Foot({ dismiss, actions, aside, busy, onClose }: FootProps) {
    return (
        <footer className="dialog-foot">
            {/* A disabled fieldset disables every control inside it, whoever wrote them. */}
            {aside && (
                <fieldset className="dialog-aside" disabled={busy}>
                    {aside}
                </fieldset>
            )}
            {dismiss !== null && (
                <button
                    type="button"
                    className={actions ? "button" : "button button-primary"}
                    onClick={onClose}
                >
                    {dismiss}
                </button>
            )}
            {actions && (
                <fieldset className="dialog-actions" disabled={busy}>
                    {actions}
                </fieldset>
            )}
        </footer>
    );
}
