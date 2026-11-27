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
