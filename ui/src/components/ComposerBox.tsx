import {
    type ClipboardEvent,
    type KeyboardEvent,
    type ReactNode,
    type Ref,
    type RefObject,
    useEffect,
    useLayoutEffect,
    useRef,
    useState,
} from "react";
import { api } from "../api";
import type { SlashCommand, Template } from "../api/types";
import { type Edit, editFor } from "../editing";
import { matchHotkey } from "../hotkeys";
import type { PastedImage } from "../images";
import { isLargePaste, tokenFor, withToken } from "../pastes";
import { caretAfter, type Direction, recallDirection } from "../recall";
import type { Draft } from "../sending";
import {
    firstPlaceholder,
    kindLabel,
    slashMatches,
    slashQuery,
    stepActive,
    withCommand,
} from "../slash";
import { useStore } from "../store";
import { takesSuggestion } from "../suggestion";
import { usePastedImages } from "../usePastedImages";
import { useWords } from "../useWords";
import { FileChips } from "./FileChips";

interface Props {
    value: string;
    onChange(value: string): void;
    onSend(): void;
    /** What the shortcut that sends now does: stops the agent and sends; Enter's send if absent. */
    onSendNow?: () => void;
    /** Shown instead of Send while the agent works; also what Esc does. */
    onStop?: (() => void) | undefined;
    canSend: boolean;
    placeholder: string;
    disabled?: boolean;
    autoFocus?: boolean;
    inputRef?: Ref<HTMLTextAreaElement>;
    /** The commands and skills suggested while a message starts with `/`. */
    commands?: SlashCommand[];
    /** The files the message points the agent at. */
    files: string[];
    onFiles(files: string[]): void;
    /** Images pasted into the message, kept in the draft until it goes. */
    images: PastedImage[];
    onImages(images: PastedImage[]): void;
    /** The session the message goes to, whose pasted images show as images; none for a new task. */
    sessionId?: number;
    /** The message the agent suggests next: shown in the empty box, and taken with Tab. */
    suggestion?: string | null;
    /** What sits under the text, left of the button: pickers or labels. */
    children?: ReactNode;
    /**
     * Brings back a message sent earlier, or goes on to a newer one, for ↑ at the very start
     * or ↓ at the very end: what the box holds then, or `null` to leave the key to move the caret.
     */
    onRecall?: (direction: Direction) => Draft | null;
}

/** The message box: text that grows with what is typed, options below it, and one round button. */
export function ComposerBox(props: Props) {
    const { value, onChange, onSend, onStop, canSend, placeholder, children } = props;
    const { files, onFiles, images, onImages } = props;
    const offered = value === "" && !props.disabled ? props.suggestion : null;
    const own = useRef<HTMLTextAreaElement>(null);
    // What to select once a kept prompt is in the box: its first place to fill in.
    const toSelect = useRef<[number, number] | null>(null);
    const templates = useStore((state) => state.templates);
    const menu = useSlashMenu(value, props.commands ?? [], templates, (text, select) => {
        toSelect.current = select ?? null;
        onChange(text);
    });
    const attach = useAttaching(files, onFiles);
    const takeImages = usePastedImages(images, onImages);

    // Grow with the text, up to the limit the stylesheet sets, then scroll.
    // biome-ignore lint/correctness/useExhaustiveDependencies: runs when the text changes
    useLayoutEffect(() => {
        const element = own.current;
        if (element) {
            element.style.height = "auto";
            element.style.height = `${element.scrollHeight}px`;
        }
        if (element && toSelect.current) {
            element.setSelectionRange(...toSelect.current);
            toSelect.current = null;
        }
    }, [value]);

    const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
        // The command list goes first: while it is open, ↑ and ↓ move in it.
        const recalling = { value, onRecall: props.onRecall, dismiss: menu.dismiss, toSelect };
        if (!(menu.onKeyDown(event) || recalled(event, recalling))) {
            typed(event, {
                value,
                offered,
                onChange,
                onSend,
                onSendNow: props.onSendNow ?? onSend,
                onStop,
            });
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
                                {menu.kind(name) && (
                                    <span className="slash-kind">{menu.kind(name)}</span>
                                )}
                            </button>
                        </li>
                    ))}
                </ul>
            )}
            <FileChips
                files={files}
                onRemove={(file) => onFiles(files.filter((other) => other !== file))}
                sessionId={props.sessionId}
                images={images}
                onRemoveImage={(image) => onImages(images.filter((other) => other !== image))}
            />
            <textarea
                ref={(element) => {
                    own.current = element;
                    assign(props.inputRef, element);
                }}
                aria-label="Message"
                rows={1}
                value={value}
                placeholder={offered || placeholder}
                disabled={props.disabled}
                // biome-ignore lint/a11y/noAutofocus: the screen exists to type into this box
                autoFocus={props.autoFocus}
                onChange={(event) => onChange(event.target.value)}
                onPaste={(event) => onPaste(event, onChange, takeImages)}
                onKeyDown={onKeyDown}
            />
            <SuggestionHint shown={Boolean(offered)} />
            <div className="composer-row">
                <div className="composer-options">
                    <button
                        type="button"
                        className="icon-picker"
                        aria-label="Attach files"
                        title="Attach files: point the agent at files on this Mac. You can also drop them on the window."
                        onClick={attach}
                    >
                        <svg viewBox="0 0 16 16" aria-hidden="true">
                            <path d="M8 3v10M3 8h10" />
                        </svg>
                    </button>
                    {children}
                </div>
                <ActionButton onStop={onStop} canSend={canSend} />
            </div>
        </form>
    );
}

/** What the box knows that bringing back a sent message needs. */
interface Recalling {
    value: string;
    onRecall: ((direction: Direction) => Draft | null) | undefined;
    /** Keeps the command list away from a text until it changes. */
    dismiss(text: string): void;
    /** Where to put the caret once the box shows the text it is given. */
    toSelect: RefObject<[number, number] | null>;
}

/** Takes ↑ or ↓ if it brings a sent message back; says whether it did. */
function recalled(event: KeyboardEvent<HTMLTextAreaElement>, recalling: Recalling): boolean {
    const { value, onRecall } = recalling;
    const box = event.currentTarget;
    const direction = recallDirection(event, value, box.selectionStart, box.selectionEnd);
    const shown = direction && onRecall && !event.nativeEvent.isComposing && onRecall(direction);
    if (!(direction && shown)) {
        return false;
    }
    event.preventDefault();
    // A message that starts with a command is not the start of a new one to complete.
    recalling.dismiss(shown.text);
    const caret = caretAfter(direction, shown.text);
    if (shown.text === value) {
        box.setSelectionRange(caret, caret);
    } else {
        recalling.toSelect.current = [caret, caret];
    }
    return true;
}

/** What the box and its owner make of a key the command list did not take. */
interface Typing {
    value: string;
    offered: string | null | undefined;
    onChange(value: string): void;
    onSend(): void;
    onSendNow(): void;
    onStop: (() => void) | undefined;
}

function typed(event: KeyboardEvent<HTMLTextAreaElement>, typing: Typing) {
    const { value, offered, onChange } = typing;
    if (takesSuggestion(event, value, offered)) {
        event.preventDefault();
        return onChange(offered);
    }
    const box = event.currentTarget;
    const edit = editFor(event, value, box.selectionStart, box.selectionEnd);
    if (edit) {
        event.preventDefault();
        return applyEdit(edit, box, onChange);
    }
    // The shortcut that sends now is in the table with the rest; it acts here, where the text is.
    if (matchHotkey(event) === "send-now" && !event.nativeEvent.isComposing) {
        event.preventDefault();
        return typing.onSendNow();
    }
    // Enter sends; Shift+Enter is a new line. Enter that confirms an IME candidate is neither.
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
        event.preventDefault();
        typing.onSend();
    } else if (event.key === "Escape") {
        typing.onStop?.();
    }
}

/** How to take the suggestion that stands in the empty box. */
function SuggestionHint({ shown }: { shown: boolean }) {
    return shown ? (
        <p className="composer-suggestion">
            <kbd>Tab</kbd> to use the suggestion
        </p>
    ) : null;
}

/** The round button: Stop while the agent works, Send otherwise. */
function ActionButton(props: { onStop: (() => void) | undefined; canSend: boolean }) {
    if (props.onStop) {
        return (
            <button type="button" className="round-button" aria-label="Stop" onClick={props.onStop}>
                <svg viewBox="0 0 16 16" aria-hidden="true">
                    <rect x="4.5" y="4.5" width="7" height="7" rx="1" />
                </svg>
            </button>
        );
    }
    return (
        <button
            type="submit"
            className="round-button round-button-send"
            aria-label="Send"
            disabled={!props.canSend}
        >
            <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M8 12.5v-9M4 7.5l4-4 4 4" />
            </svg>
        </button>
    );
}

/** `files` with `added` at the end, each file once. */
export function withAdded(files: string[], added: string[]): string[] {
    return [...files, ...added.filter((file) => !files.includes(file))];
}

/**
 * Adding files to the message: returns what the "+" button does, which is to open the system's
 * file picker, and takes files dropped on the window while this box is on screen. Nothing is
 * read or copied: a file is attached by its path.
 */
function useAttaching(files: string[], onFiles: (files: string[]) => void) {
    // The latest list and setter, for the listener that outlives the render that made it.
    const latest = useRef({ files, onFiles });
    latest.current = { files, onFiles };
    const add = (added: string[]) => {
        const now = latest.current;
        now.onFiles(withAdded(now.files, added));
    };
    // biome-ignore lint/correctness/useExhaustiveDependencies: subscribed once, for as long as the box is shown
    useEffect(() => {
        const subscription = api.onFilesDropped(add);
        return () => void subscription.then((unsubscribe) => unsubscribe());
    }, []);
    return () => void api.pickFiles().then(add).catch(useStore.getState().report);
}

function applyEdit(edit: Edit, box: HTMLTextAreaElement, onChange: (value: string) => void) {
    if (edit.kind === "clear") {
        onChange("");
    } else if (edit.kind === "remove") {
        replaceIn(box, edit.start, edit.end, "", onChange);
    } else {
        box.setSelectionRange(edit.start, edit.end, edit.direction);
    }
}

/**
 * Puts `text` in place of what stands between `start` and `end` in the box, and the caret
 * after it once the box has been drawn again with its new text.
 */
function replaceIn(
    box: HTMLTextAreaElement,
    start: number,
    end: number,
    text: string,
    onChange: (value: string) => void,
) {
    const changed = withToken(box.value, start, end, text);
    onChange(changed.value);
    requestAnimationFrame(() => box.setSelectionRange(changed.caret, changed.caret));
}

/**
 * Images in a paste go into the draft as images, and the text beside them, if any, is pasted
 * as text. A large paste goes into the box as a short token that stands for it; the text itself
 * is sent with the message. Anything smaller is pasted as it always is.
 */
function onPaste(
    event: ClipboardEvent<HTMLTextAreaElement>,
    onChange: (value: string) => void,
    takeImages: (data: DataTransfer) => boolean,
) {
    const pasted = event.clipboardData.getData("text/plain");
    const images = takeImages(event.clipboardData);
    if (images && pasted === "") {
        event.preventDefault();
        return;
    }
    if (!isLargePaste(pasted)) {
        return;
    }
    event.preventDefault();
    const box = event.currentTarget;
    replaceIn(box, box.selectionStart, box.selectionEnd, tokenFor(pasted), onChange);
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
function useSlashMenu(
    value: string,
    commands: SlashCommand[],
    templates: Template[],
    onChange: (value: string, select?: [number, number] | null) => void,
) {
    const words = useWords();
    const [active, setActive] = useState(0);
    // The text Esc was pressed on; the list stays away until the text changes.
    const [dismissed, setDismissed] = useState<string | null>(null);
    const query = dismissed === value ? null : slashQuery(value);
    const kept = new Map(templates.map((template) => [template.name, template.text]));
    // The user's own prompts stand beside the agent's commands; one of the same name wins.
    const names = commands.map((command) => command.name);
    const matches = query === null ? [] : slashMatches(query, [...kept.keys(), ...names]);
    const current = Math.min(active, Math.max(matches.length - 1, 0));
    const choose = (name: string) => {
        setActive(0);
        const text = kept.get(name);
        if (text === undefined) {
            onChange(withCommand(name));
        } else {
            onChange(text, firstPlaceholder(text));
        }
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
    return {
        matches,
        active: current,
        choose,
        onKeyDown,
        /** Keeps the list away from `text` until the text changes, as Esc does. */
        dismiss: setDismissed,
        /** What the list says beside a name: that it is the user's own, or what Swarmed does. */
        kind: (name: string) =>
            kept.has(name)
                ? `your ${words.terms.prompt}`
                : kindLabel(commands.find((command) => command.name === name)),
    };
}
