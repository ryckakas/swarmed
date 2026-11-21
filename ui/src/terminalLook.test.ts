import { describe, expect, it } from "vitest";
import {
    PALETTE_COLOURS,
    schemeOf,
    TERMINAL_COLOURS,
    terminalExtras,
    terminalOptions,
    terminalTheme,
} from "./terminalLook";

const base = { fontFamily: "Mono, monospace", fontSize: 13 };

describe("terminalTheme", () => {
    const resolved = (css: string) => `resolved(${css})`;

    it("gives all sixteen colours and the four around them from the palette", () => {
        const theme = terminalTheme(null, "dark", resolved);
        expect(Object.keys(theme)).toHaveLength(TERMINAL_COLOURS.length);
        expect(theme.red).toBe("resolved(var(--clay))");
        expect(theme.brightBlack).toBe("resolved(var(--faint))");
        expect(theme.brightBlue).toBe(`resolved(${PALETTE_COLOURS.bright_blue})`);
        expect(theme.selectionBackground).toContain("var(--focus)");
    });

    it("takes the user's colours for the form the app is in, and no other", () => {
        const config = {
            dark: { blue: "#7aa2f7", bright_red: "#ff0000" },
            light: { blue: "#0000aa" },
        };
        const dark = terminalTheme(config, "dark", resolved);
        expect([dark.blue, dark.brightRed]).toEqual(["#7aa2f7", "#ff0000"]);
        expect(terminalTheme(config, "light", resolved).blue).toBe("#0000aa");
        expect(terminalTheme(config, "light", resolved).brightRed).toContain("var(--clay)");
    });

    it("passes over a colour that is none", () => {
        const theme = terminalTheme({ dark: { red: "url(x)" } }, "dark", resolved);
        expect(theme.red).toBe("resolved(var(--clay))");
    });
});

describe("schemeOf", () => {
    it("follows the settings, and the system where they leave it open", () => {
        expect(schemeOf("light", true)).toBe("light");
        expect(schemeOf("dark", false)).toBe("dark");
        expect(schemeOf("system", true)).toBe("dark");
        expect(schemeOf(undefined, false)).toBe("light");
    });
});

describe("terminalOptions", () => {
    it("starts from the app's own text and sensible habits", () => {
        expect(terminalOptions(null, base)).toEqual({
            fontFamily: "Mono, monospace",
            fontSize: 13,
            lineHeight: 1,
            cursorStyle: "block",
            cursorBlink: true,
            scrollback: 10_000,
            drawBoldTextInBrightColors: true,
            macOptionIsMeta: false,
            minimumContrastRatio: 4.5,
        });
        expect(terminalExtras(null)).toEqual({ copyOnSelect: false, clipboard: true, gpu: false });
    });

    it("takes what the configuration sets, with the app's font behind the user's", () => {
        const options = terminalOptions(
            {
                font: "JetBrains Mono",
                font_size: 15,
                line_height: 120,
                cursor: "bar",
                cursor_blink: false,
            },
            base,
        );
        expect(options.fontFamily).toBe('"JetBrains Mono", Mono, monospace');
        expect([
            options.fontSize,
            options.lineHeight,
            options.cursorStyle,
            options.cursorBlink,
        ]).toEqual([15, 1.2, "bar", false]);
        expect(terminalOptions({ font: 'x", serif; }' }, base).fontFamily).toBe("Mono, monospace");
        expect(terminalExtras({ copy_on_select: true, gpu: true }).gpu).toBe(true);
    });
});
