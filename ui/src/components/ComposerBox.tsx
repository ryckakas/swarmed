import { type KeyboardEvent, type ReactNode, type Ref, useLayoutEffect, useRef } from "react";

interface Props {
    value: string;
    onChange(value: string): void;
    onSend(): void;
    /** Shown instead of Send while the agent works; also what Esc does. */
    onStop?: (() => void) | undefined;
    canSend: boolean;
    placeholder: string;
    disabled?: boolean;
    autoFocus?: boolean;
    inputRef?: Ref<HTMLTextAreaElement>;
    /** What sits under the text, left of the button: pickers or labels. */
    children?: ReactNode;
}

/** The message box: text that grows with what is typed, options below it, and one round button. */
export function ComposerBox(props: Props) {
    const { value, onChange, onSend, onStop, canSend, placeholder, children } = props;
    const own = useRef<HTMLTextAreaElement>(null);

    // Grow with the text, up to the limit the stylesheet sets, then scroll.
    // biome-ignore lint/correctness/useExhaustiveDependencies: runs when the text changes
    useLayoutEffect(() => {
        const element = own.current;
        if (element) {
            element.style.height = "auto";
            element.style.height = `${element.scrollHeight}px`;
        }
    }, [value]);

    const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
        // Enter sends; Shift+Enter is a new line. Enter that confirms an IME candidate is neither.
        if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault();
            onSend();
        } else if (event.key === "Escape") {
            onStop?.();
        }
    };

    return (
        <form
            className="composer-box"
            onSubmit={(event) => {
                event.preventDefault();
                onSend();
            }}
        >
            <textarea
                ref={(element) => {
                    own.current = element;
                    assign(props.inputRef, element);
                }}
                aria-label="Message"
                rows={1}
                value={value}
                placeholder={placeholder}
                disabled={props.disabled}
                // biome-ignore lint/a11y/noAutofocus: the screen exists to type into this box
                autoFocus={props.autoFocus}
                onChange={(event) => onChange(event.target.value)}
                onKeyDown={onKeyDown}
            />
            <div className="composer-row">
                <div className="composer-options">{children}</div>
                {onStop ? (
                    <button
                        type="button"
                        className="round-button"
                        aria-label="Stop"
                        onClick={onStop}
                    >
                        <svg viewBox="0 0 16 16" aria-hidden="true">
                            <rect x="4.5" y="4.5" width="7" height="7" rx="1" />
                        </svg>
                    </button>
                ) : (
                    <button
                        type="submit"
                        className="round-button round-button-send"
                        aria-label="Send"
                        disabled={!canSend}
                    >
                        <svg viewBox="0 0 16 16" aria-hidden="true">
                            <path d="M8 12.5v-9M4 7.5l4-4 4 4" />
                        </svg>
                    </button>
                )}
            </div>
        </form>
    );
}

function assign<T>(ref: Ref<T> | undefined, value: T | null) {
    if (typeof ref === "function") {
        ref(value);
    } else if (ref) {
        ref.current = value;
    }
}
