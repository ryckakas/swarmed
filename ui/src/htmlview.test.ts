import { describe, expect, it } from "vitest";
import { requestedHeight, viewDocument } from "./htmlview";

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
