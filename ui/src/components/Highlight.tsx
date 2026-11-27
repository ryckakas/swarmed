import { type KeyboardEvent, useCallback, useEffect, useRef } from "react";
import { highlightRow } from "../actions";
import type { MarkColour } from "../api/types";
import {
    choiceName,
    HIGHLIGHT_CHOICES,
    type HighlightTarget,
    highlightLabel,
    highlightSaid,
    highlightTitle,
    paletteStart,
    paletteStep,
    sameRow,
} from "../highlights";
import { useStore } from "../store";
import { useDismiss } from "./UsageButton";

/**
 * The highlight on a task or a session: shown, in the row's colour if it has one, only while the
 * row is pointed at or has the keyboard, or while the palette is open (#475). Pressing it opens
 * the palette, as the shortcut does for the focused row or the open session.
 */
export function HighlightButton(props: {
    target: HighlightTarget;
    colour: MarkColour | null;
    name: string;
}) {
    const { target, colour, name } = props;
    const open = useStore((state) => sameRow(state.highlighting, target));
    const askToHighlight = useStore((state) => state.askToHighlight);
    const wrap = useRef<HTMLSpanElement>(null);
    const button = useRef<HTMLButtonElement>(null);
    // Only ever asked to close: a press outside, or Escape.
    const dismiss = useCallback(
        (shown: boolean) => {
            if (!shown) {
                askToHighlight(null);
            }
        },
        [askToHighlight],
    );
    useDismiss(wrap, open, dismiss);
    const close = (refocus: boolean) => {
        askToHighlight(null);
        if (refocus) {
            button.current?.focus();
        }
    };
    return (
        <span className="highlight-wrap" ref={wrap}>
            <button
                ref={button}
                type="button"
                className="row-highlight"
                data-highlight={colour ?? undefined}
                aria-haspopup="menu"
                aria-expanded={open}
                aria-label={highlightLabel(colour, name)}
                title={highlightTitle(colour)}
                onClick={() => askToHighlight(open ? null : target)}
            >
                {/* A drop of colour, apart from the round dot of an agent's mark. */}
                <svg viewBox="0 0 16 16" aria-hidden="true">
                    <path
                        className="highlight-drop"
                        d="M8 2.4C6.4 4.6 4.3 7 4.3 9.4a3.7 3.7 0 0 0 7.4 0C11.7 7 9.6 4.6 8 2.4z"
                    />
                </svg>
            </button>
            {open && <HighlightPalette {...props} close={close} />}
        </span>
    );
}

/**
 * A swatch for each colour and one for none. A click chooses and closes it; the arrows, Home
 * and End move between the swatches, Enter or the space bar chooses, Escape and Tab leave it.
 */
function HighlightPalette({
    target,
    colour,
    name,
    close,
}: {
    target: HighlightTarget;
    colour: MarkColour | null;
    name: string;
    close(refocus: boolean): void;
}) {
    const menu = useRef<HTMLDivElement>(null);
    const swatch = (at: number) => swatchOf(menu.current, at);
    // Opened, by a press or by the shortcut, the keyboard goes to the row's colour, and the row
    // is scrolled to where it can be seen.
    useEffect(() => {
        menu.current?.scrollIntoView({ block: "nearest" });
        swatchOf(menu.current, paletteStart(colour))?.focus();
    }, [colour]);
    const choose = (choice: MarkColour | null) => {
        close(true);
        if (choice !== colour) {
            void highlightRow(target, choice);
        }
    };
    const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        if (event.key === "Escape") {
            event.stopPropagation();
            close(true);
            return;
        }
        const at = HIGHLIGHT_CHOICES.findIndex(
            (_, index) => swatch(index) === document.activeElement,
        );
        const next = paletteStep(Math.max(at, 0), event.key);
        if (next !== null) {
            // Kept from the sidebar, whose arrows move between its rows.
            event.preventDefault();
            event.stopPropagation();
            swatch(next)?.focus();
        }
    };
    return (
        <div
            ref={menu}
            className="highlight-palette"
            role="menu"
            aria-label={`Highlight ${name}`}
            aria-orientation="horizontal"
            onKeyDown={onKeyDown}
            // A press keeps the keyboard where it is: WebKit gives a pressed button none, and
            // the swatch that had it would go blank and close the palette before the click.
            onMouseDown={(event) => event.preventDefault()}
            onBlur={(event) => {
                // Tab leaves it, for anywhere but its own button, which closes it itself.
                const wrap = event.currentTarget.parentElement;
                if (!wrap?.contains(event.relatedTarget)) {
                    close(false);
                }
            }}
        >
            {HIGHLIGHT_CHOICES.map((choice) => (
                <button
                    key={choice ?? "none"}
                    type="button"
                    role="menuitemradio"
                    aria-checked={choice === colour}
                    className="highlight-swatch"
                    data-highlight={choice ?? "none"}
                    aria-label={choiceName(choice)}
                    title={choice === null ? "No highlight" : choiceName(choice)}
                    tabIndex={-1}
                    onClick={() => choose(choice)}
                />
            ))}
        </div>
    );
}

/** The palette's swatch `at`, if the palette is there. */
function swatchOf(menu: HTMLElement | null, at: number): HTMLButtonElement | undefined {
    return menu?.querySelectorAll<HTMLButtonElement>("[role=menuitemradio]")[at];
}

/**
 * The colour a session is highlighted in, as a small dot where the session is named outside
 * the sidebar: its header and the inbox. Nothing for one that is not.
 */
export function HighlightDot({ colour }: { colour: MarkColour | null }) {
    if (colour === null) {
        return null;
    }
    const said = highlightSaid(colour);
    return (
        <span
            className="highlight-dot"
            data-highlight={colour}
            role="img"
            aria-label={said}
            title={said.charAt(0).toUpperCase() + said.slice(1)}
        />
    );
}

/** The words a highlighted row adds to its name for those who cannot see its colour. */
export function HighlightWords({ colour }: { colour: MarkColour | null }) {
    return colour === null ? null : <span className="unseen">, {highlightSaid(colour)}</span>;
}
