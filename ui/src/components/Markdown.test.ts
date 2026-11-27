import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { MathShown } from "../api/types";
import { AgentMarkdown, UntrustedMarkdown } from "./Markdown";

const TABLE = [
    "| Task | What it does |",
    "| --- | --- |",
    "| `own-links` | Lets a rule name the rules it builds on. |",
].join("\n");

/** Where a table starts: in the box that scrolls it sideways, as a table still. */
const BOXED = '<div class="prose-table"><table>';

describe("a table", () => {
    it("is in a box of its own in a reply, whatever the settings", () => {
        const settings: [MathShown, boolean][] = [
            ["off", false],
            ["off", true],
            ["standard", false],
            ["standard", true],
        ];
        for (const [math, views] of settings) {
            const drawn = renderToStaticMarkup(
                createElement(AgentMarkdown, { text: TABLE, math, views }),
            );
            expect(drawn).toContain(BOXED);
            expect(drawn).toContain("<th>Task</th>");
        }
    });

    it("is in a box of its own in an issue", () => {
        const drawn = renderToStaticMarkup(createElement(UntrustedMarkdown, { text: TABLE }));
        expect(drawn).toContain(BOXED);
    });

    it("leaves the rest of the text as it was", () => {
        const drawn = renderToStaticMarkup(
            createElement(AgentMarkdown, { text: "No table here.", math: "off", views: false }),
        );
        expect(drawn).toBe("<p>No table here.</p>");
    });
});

/** The copy button, as each code block has it. */
const COPY = 'aria-label="Copy"';

/** How many times `part` stands in `drawn`. */
function count(drawn: string, part: string): number {
    return drawn.split(part).length - 1;
}

describe("a code block", () => {
    const CODE = "Run:\n\n```sh\ncargo test\n```\n\nThen:\n\n    cargo fmt\n";

    it("has a copy button in a reply, whatever the settings", () => {
        const settings: [MathShown, boolean][] = [
            ["off", false],
            ["off", true],
            ["standard", false],
            ["standard", true],
        ];
        for (const [math, views] of settings) {
            const drawn = renderToStaticMarkup(
                createElement(AgentMarkdown, { text: CODE, math, views }),
            );
            expect(count(drawn, COPY)).toBe(2);
            expect(drawn).toContain('<div class="copyable"><pre>');
        }
    });

    it("has a copy button in an issue", () => {
        const drawn = renderToStaticMarkup(createElement(UntrustedMarkdown, { text: CODE }));
        expect(count(drawn, COPY)).toBe(2);
    });

    it("has a copy button in a <pre> written as HTML in an issue", () => {
        const drawn = renderToStaticMarkup(
            createElement(UntrustedMarkdown, { text: "<pre>make all</pre>" }),
        );
        expect(count(drawn, COPY)).toBe(1);
    });

    it("has none where it is shown as a view", () => {
        const view = "```html-view\n<p>Hello</p>\n```";
        const drawn = renderToStaticMarkup(
            createElement(AgentMarkdown, { text: view, math: "off", views: true }),
        );
        expect(drawn).not.toContain(COPY);
        expect(drawn).not.toContain("<pre");
    });

    it("has one when a view's block stays code", () => {
        const view = "```html-view\n<p>Hello</p>\n```";
        const off = renderToStaticMarkup(
            createElement(AgentMarkdown, { text: view, math: "off", views: false }),
        );
        const issue = renderToStaticMarkup(createElement(UntrustedMarkdown, { text: view }));
        expect(count(off, COPY)).toBe(1);
        expect(count(issue, COPY)).toBe(1);
    });

    it("of HTML has both the copy button and the offer to show it as a view", () => {
        const html = "```html\n<p>Hello</p>\n```";
        const drawn = renderToStaticMarkup(
            createElement(AgentMarkdown, { text: html, math: "off", views: true }),
        );
        expect(drawn).toContain("Show as a view");
        expect(count(drawn, COPY)).toBe(1);
    });

    it("leaves code in a line alone", () => {
        const drawn = renderToStaticMarkup(
            createElement(AgentMarkdown, { text: "Run `ls` now.", math: "off", views: false }),
        );
        expect(drawn).not.toContain(COPY);
    });
});
