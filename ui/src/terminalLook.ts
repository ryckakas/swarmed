// How a terminal is drawn: the app's own choices, with the user's configuration on top. Pure, so
// it can be tested: what turns a stylesheet's colour into one the terminal can use is handed in.

import type { TerminalConfig } from "./api/types";

/** The names a terminal's colours go by in the user's configuration. */
export const TERMINAL_COLOURS = [
    "background",
    "foreground",
    "cursor",
    "selection",
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
] as const;

export type TerminalColourName = (typeof TERMINAL_COLOURS)[number];

/** A colour in its bright form: nearer to what the palette stresses text with. */
function bright(colour: string): string {
    return `color-mix(in srgb, ${colour} 70%, var(--bright))`;
}

/**
 * Every colour of the terminal as the app's palette gives it, written as the stylesheet would:
 * the same meanings as elsewhere, so red is what failed and green what went well, in dark and
 * in light and in a custom layout alike.
 */
export const PALETTE_COLOURS: Record<TerminalColourName, string> = {
    background: "var(--panel)",
    foreground: "var(--text)",
    cursor: "var(--text)",
    selection: "color-mix(in srgb, var(--focus) 35%, var(--panel))",
    black: "var(--line)",
    red: "var(--clay)",
    green: "var(--moss)",
    yellow: "var(--pollen)",
    blue: "var(--sky)",
    magenta: "var(--lilac)",
    cyan: "var(--teal)",
    white: "var(--muted)",
    bright_black: "var(--faint)",
    bright_red: bright("var(--clay)"),
    bright_green: bright("var(--moss)"),
    bright_yellow: bright("var(--pollen)"),
    bright_blue: bright("var(--sky)"),
    bright_magenta: bright("var(--lilac)"),
    bright_cyan: bright("var(--teal)"),
    bright_white: "var(--bright)",
};

/** What the terminal emulator calls each colour. */
function themeKey(name: TerminalColourName): string {
    if (name === "selection") {
        return "selectionBackground";
    }
    return name.replace(/_([a-z])/, (_all, letter: string) => letter.toUpperCase());
}

const HEX = /^#[0-9a-f]{6}$/i;

/**
 * The terminal's colours: the user's own for the form the app is in, and the palette's for the
 * rest. `resolved` turns a colour as a stylesheet writes it into one a terminal takes.
 */
export function terminalTheme(
    config: TerminalConfig | null,
    scheme: "dark" | "light",
    resolved: (css: string) => string,
): Record<string, string> {
    const own = config?.[scheme] ?? {};
    const theme: Record<string, string> = {};
    for (const name of TERMINAL_COLOURS) {
        const given = own[name];
        theme[themeKey(name)] = given && HEX.test(given) ? given : resolved(PALETTE_COLOURS[name]);
    }
    return theme;
}

/** Whether the app is dark right now: as the settings say, or as the system is. */
export function schemeOf(theme: string | undefined, systemIsDark: boolean): "dark" | "light" {
    if (theme === "dark" || theme === "light") {
        return theme;
    }
    return systemIsDark ? "dark" : "light";
}

const FONT_NAME = /^[\p{L}\p{N} ._-]{1,60}$/u;

/** What a terminal starts from where the configuration says nothing: the app's own text. */
export interface TerminalBase {
    fontFamily: string;
    fontSize: number;
}

/** The emulator's options for a configuration. */
export function terminalOptions(config: TerminalConfig | null, base: TerminalBase) {
    const font = config?.font && FONT_NAME.test(config.font) ? `"${config.font}", ` : "";
    return {
        fontFamily: `${font}${base.fontFamily}`,
        fontSize: config?.font_size ?? base.fontSize,
        lineHeight: (config?.line_height ?? 100) / 100,
        cursorStyle: config?.cursor ?? "block",
        cursorBlink: config?.cursor_blink ?? true,
        // A build's whole output stays within reach of the wheel.
        scrollback: config?.scrollback ?? 10_000,
        drawBoldTextInBrightColors: config?.bold_is_bright ?? true,
        // Off unless asked for: Option types @, [ and | on many keyboards.
        macOptionIsMeta: config?.option_is_meta ?? false,
        // A colour a program asks for is kept readable on the background, whatever the two are.
        minimumContrastRatio: (config?.minimum_contrast ?? 45) / 10,
    } as const;
}

/** What else a configuration switches, apart from the emulator's own options. */
export function terminalExtras(config: TerminalConfig | null) {
    return {
        copyOnSelect: config?.copy_on_select ?? false,
        clipboard: config?.clipboard ?? true,
        // Off unless asked for: faster for much output, but not yet seen to draw right everywhere.
        gpu: config?.gpu ?? false,
    };
}
