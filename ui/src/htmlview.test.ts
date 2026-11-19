import { describe, expect, it } from "vitest";
import { needsNetwork, requestedHeight, viewDocument } from "./htmlview";

const theme = {
    text: "#eee",
    muted: "#aaa",
    background: "#111",
    line: "#333",
    accent: "#8fb4ff",
    fontFamily: "system-ui",
    fontSize: "14px",
    dark: true,
};

describe("viewDocument", () => {
    it("puts the fragment, untouched, between the theme and the height reporter", () => {
        const fragment = '<table id="t"><tr><td>1</td></tr></table><script>sort()</script>';
        const document = viewDocument(fragment, theme);
        expect(document.startsWith("<!doctype html>")).toBe(true);
        expect(document).toContain(fragment);
        expect(document).toContain("color-scheme: dark");
        expect(document.indexOf("--text: #eee")).toBeLessThan(document.indexOf(fragment));
        expect(document.indexOf("swarmedViewHeight")).toBeGreaterThan(document.indexOf(fragment));
    });
});

describe("requestedHeight", () => {
    it("reads a usable height and nothing else", () => {
        expect(requestedHeight({ swarmedViewHeight: 240 })).toBe(240);
        expect(requestedHeight({ swarmedViewHeight: -1 })).toBeNull();
        expect(requestedHeight({ swarmedViewHeight: "240" })).toBeNull();
        expect(requestedHeight("anything")).toBeNull();
        expect(requestedHeight(null)).toBeNull();
    });
});

describe("needsNetwork", () => {
    it("spots scripts, styles, images and requests by address", () => {
        expect(needsNetwork('<script src="https://cdn.example/chart.js"></script>')).toBe(true);
        expect(needsNetwork("<link href=//fonts.example/a.css rel=stylesheet>")).toBe(true);
        expect(needsNetwork("<style>@import 'x.css';</style>")).toBe(true);
        expect(needsNetwork("<script>fetch('/api')</script>")).toBe(true);
    });

    it("leaves a self-contained view alone", () => {
        expect(needsNetwork('<img src="data:image/png;base64,AAAA"><a href="#top">Top</a>')).toBe(
            false,
        );
        expect(needsNetwork("<svg><path d='M0 0'/></svg><script>let x = 1;</script>")).toBe(false);
    });
});

describe("a whole document as a view", () => {
    it("is kept as written, with the theme first in its head and the height script after it", () => {
        const page =
            '<!doctype html>\n<html lang="en"><head><title>t</title></head><body><p>hi</p></body></html>';
        const shown = viewDocument(page, theme);
        expect(shown.startsWith('<!doctype html>\n<html lang="en"><head>\n<style>')).toBe(true);
        expect(shown).toContain("<title>t</title></head><body><p>hi</p></body></html>");
        expect(shown.indexOf("swarmedViewHeight")).toBeGreaterThan(shown.indexOf("</html>"));
        // Not wrapped a second time.
        expect(shown.match(/<html/g)).toHaveLength(1);
    });
});
