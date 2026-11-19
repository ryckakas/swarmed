import {
    type KeyboardEvent,
    type ReactNode,
    type Ref,
    useLayoutEffect,
    useRef,
    useState,
} from "react";
import { type Edit, editFor } from "../editing";
import { slashMatches, slashQuery, stepActive, withCommand } from "../slash";

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
    /** The commands and skills suggested while a message starts with `/`. */
    commands?: string[];
    /** What sits under the text, left of the button: pickers or labels. */
    children?: ReactNode;
}

/** The message box: text that grows with what is typed, options below it, and one round button. */
export function ComposerBox(props: Props) {
    const { value, onChange, onSend, onStop, canSend, placeholder, children } = props;
    const own = useRef<HTMLTextAreaElement>(null);
    const menu = useSlashMenu(value, props.commands ?? [], onChange);

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
        if (menu.onKeyDown(event)) {
            return;
        }
        const box = event.currentTarget;
        const edit = editFor(event, value, box.selectionStart, box.selectionEnd);
        if (edit) {
            event.preventDefault();
            return applyEdit(edit, box, onChange);
        }
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
            {menu.matches.length > 0 && (
                <ul className="slash-menu" aria-label="Commands">
                    {menu.matches.map((name, index) => (
                        <li key={name}>
                            <button
                                type="button"
                                className="slash-option"
                                aria-current={index === menu.active ? "true" : undefined}
                                // Keeps the text box focused, so typing carries on after the click.
                                onMouseDown={(event) => event.preventDefault()}
                                onClick={() => menu.choose(name)}
                            >
                                /{name}
                            </button>
                        </li>
                    ))}
                </ul>
            )}
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

function applyEdit(edit: Edit, box: HTMLTextAreaElement, onChange: (value: string) => void) {
    if (edit.kind === "clear") {
        onChange("");
    } else {
        box.setSelectionRange(edit.start, edit.end, edit.direction);
    }
}

function assign<T>(ref: Ref<T> | undefined, value: T | null) {
    if (typeof ref === "function") {
        ref(value);
    } else if (ref) {
        ref.current = value;
    }
}

/**
 * The suggestions for a message that starts with `/`: which to show, which one the arrow keys
 * are on, and the keys the list takes for itself while it is open.
 */
function useSlashMenu(value: string, commands: string[], onChange: (value: string) => void) {
    const [active, setActive] = useState(0);
    // The text Esc was pressed on; the list stays away until the text changes.
    const [dismissed, setDismissed] = useState<string | null>(null);
    const query = dismissed === value ? null : slashQuery(value);
    const matches = query === null ? [] : slashMatches(query, commands);
    const current = Math.min(active, Math.max(matches.length - 1, 0));
    const choose = (name: string) => {
        setActive(0);
        onChange(withCommand(name));
    };

    /** Handles the key if the list is open and it is one of the list's; says whether it did. */
    const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
        const chosen = matches[current];
        if (chosen === undefined || event.nativeEvent.isComposing) {
            return false;
        }
        const actions = new Map<string, () => void>([
            ["ArrowDown", () => setActive(stepActive(current, 1, matches.length))],
            ["ArrowUp", () => setActive(stepActive(current, -1, matches.length))],
            ["Enter", () => choose(chosen)],
            ["Tab", () => choose(chosen)],
            ["Escape", () => setDismissed(value)],
        ]);
        const action = actions.get(event.key);
        if (!action || event.shiftKey) {
            return false;
        }
        event.preventDefault();
        action();
        return true;
    };
    return { matches, active: current, choose, onKeyDown };
}
