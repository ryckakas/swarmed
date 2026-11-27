import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { MathShown } from "./api/types";
import { AgentMarkdown } from "./components/Markdown";
import { fenced, marked, mathText } from "./math";

/** An agent's text as the transcript draws it, with formulas counted as `math` says. */
const drawn = (text: string, math: MathShown = "standard") =>
    renderToStaticMarkup(createElement(AgentMarkdown, { text, math, views: true }));

/** How many formulas KaTeX drew, inline and display. */
const formulas = (html: string) => html.match(/<span class="katex">/g)?.length ?? 0;
const displays = (html: string) => html.match(/<span class="katex-display">/g)?.length ?? 0;

describe("fenced", () => {
    it("fences a formula longer than any dollars in it, oddly in the line, evenly as display", () => {
        expect(fenced("x", false)).toBe("$$$ x $$$");
        expect(fenced("x", true)).toBe("$$ x $$");
        expect(fenced("a \\$ b", true)).toBe("$$ a \\$ b $$");
        expect(fenced("a $$ b", true)).toBe("$$$$ a $$ b $$$$");
        expect(fenced("a $$$ b", false)).toBe("$$$$$ a $$$ b $$$$$");
    });

    it("puts the formula on one line, without the marks of the containers it ran through", () => {
        expect(fenced("a +\n>  b\n   + c", false)).toBe("$$$ a + b + c $$$");
    });
});

describe("marked", () => {
    it("leaves text with no formula as it is", () => {
        const text = "Plain words, `code`, and a [link](https://example.com).";
        expect(marked(text, false)).toBe(text);
        expect(marked(text, true)).toBe(text);
    });

    it("writes \\(…\\) as a formula in the line and \\[…\\] as one that may stand alone", () => {
        expect(marked("Sorting is \\(O(n \\log n)\\) here.", false)).toBe(
            "Sorting is $$$ O(n \\log n) $$$ here.",
        );
        expect(marked("\\[x^2\\]", false)).toBe("$$ x^2 $$");
        expect(marked("where \\(a*b*c\\) holds", false)).toBe("where $$$ a*b*c $$$ holds");
    });

    it("keeps two formulas that touch apart, so their fences do not join", () => {
        expect(marked("\\(a\\)\\(b\\)", false)).toBe("$$$ a $$$ $$$ b $$$");
        expect(marked("\\[a\\]\\[b\\]", false)).toBe("$$ a $$ $$ b $$");
        expect(marked("\\(a\\)\\[b\\]", false)).toBe("$$$ a $$$ $$ b $$");
    });

    it("makes \\[ and \\] on lines of their own a block of math, in quotes and lists too", () => {
        expect(marked("Energy:\n\\[\nE = mc^2\n\\]\nand on.", false)).toBe(
            "Energy:\n$$\nE = mc^2\n$$\nand on.",
        );
        // A line of the formula that Markdown would read as a list item stays in it.
        expect(marked("\\[\na\n- b\n+ c\n\\]", false)).toBe("$$\na\n- b\n+ c\n$$");
        expect(marked("> \\[\n> x\n> \\]", false)).toBe("> $$\n> x\n> $$");
        expect(marked("- item\n  \\[\n  x\n  \\]", false)).toBe("- item\n  $$\n  x\n  $$");
    });

    it("counts an escaped \\\\[ in a block's lines, such as row spacing, as part of it", () => {
        const aligned = "\\[\n\\begin{aligned}\na &= b \\\\[2pt]\n- c &= d\n\\end{aligned}\n\\]";
        expect(marked(aligned, false)).toBe(
            "$$\n\\begin{aligned}\na &= b \\\\[2pt]\n- c &= d\n\\end{aligned}\n$$",
        );
        // A \[ after an escaped backslash is a delimiter all the same.
        expect(marked("\\[\nx \\\\\\[ y\n\\]", false)).not.toMatch(/^\$\$\n/);
    });

    it("guesses no block past a blank line, code, or a quote it did not start in", () => {
        for (const text of [
            "\\[\nx\n\n\\]",
            "\\[\nx\n```\n\\]\n```",
            "> \\[\n> x\n\\]",
            "\\[\nx \\]\ny\n\\]",
        ]) {
            expect(marked(text, false)).not.toMatch(/^\$\$\n/);
        }
    });

    it("leaves code alone, in spans, fenced blocks and indented blocks", () => {
        for (const text of [
            "Write `\\(x\\)` or `$x$`.",
            "```\n\\(x\\) and \\[y\\] and $z$\n```",
            "~~~tex\n\\[\nx\n\\]\n~~~",
            "    \\(x\\) indented",
        ]) {
            expect(marked(text, true)).toBe(text);
        }
    });

    it("leaves links' addresses and what is already math alone", () => {
        const link = "[Set](https://en.wikipedia.org/wiki/Set_\\(mathematics\\))";
        expect(marked(link, false)).toBe(link);
        const math = "$$f\\(x\\)$$ and\n\n$$\n\\[y\\]\n$$\n\n```math\n\\(z\\)\n```";
        expect(marked(math, false)).toBe(math);
    });

    it("leaves what is escaped, unclosed, empty or against a dollar as written", () => {
        for (const text of [
            "\\\\(x\\\\)",
            "\\(x",
            "an \\( open one",
            "\\( \\)",
            "\\(x\\)$5",
            "$\\(x\\)",
            "\\(a\n\nb\\)",
        ]) {
            expect(marked(text, false)).toBe(text);
        }
    });

    it("counts a single $ only with single dollars on, and only where it reads as math", () => {
        expect(marked("The rate $r$ applies.", false)).toBe("The rate $r$ applies.");
        expect(marked("The rate $r$ applies.", true)).toBe("The rate $$$ r $$$ applies.");
        expect(marked("$a+b$ and $c$", true)).toBe("$$$ a+b $$$ and $$$ c $$$");
    });

    it("keeps prices, shell variables and an escaped $ as text, single dollars on or off", () => {
        for (const text of [
            "Costs $5 and $10.",
            "It costs $20, or $25 with VAT.",
            "Between $20,000 and $30,000.",
            "Set $HOME first.",
            "Set $HOME and $PATH.",
            "It writes to $EXPORT_DIR, or to $HOME/exports.",
            "Not \\$x\\$ either.",
            "Not $ x$ nor $x $.",
            "A $$ alone, and $ alone.",
        ]) {
            expect(marked(text, false)).toBe(text);
            expect(marked(text, true)).toBe(text);
        }
    });
});

describe("mathText", () => {
    it("rewrites nothing with math off", () => {
        expect(mathText("\\(x\\) and $y$", "off")).toBe("\\(x\\) and $y$");
        expect(mathText("\\(x\\) and $y$", "standard")).toBe("$$$ x $$$ and $y$");
        expect(mathText("\\(x\\) and $y$", "with_single_dollars")).toBe("$$$ x $$$ and $$$ y $$$");
    });
});

describe("a reply with formulas, as the transcript draws it", () => {
    it("draws \\(…\\) and $$…$$ in the line, and display math where it stands alone", () => {
        const inline = drawn("Sorting is \\(O(n \\log n)\\), so $$n^2$$ is out.");
        expect([formulas(inline), displays(inline)]).toEqual([2, 0]);
        for (const text of ["\\[x^2\\]", "$$x^2$$", "$$\nx^2\n$$", "```math\nx^2\n```"]) {
            const html = drawn(text);
            expect([formulas(html), displays(html)], text).toEqual([1, 1]);
        }
        // \( … \) alone in its paragraph, or a list item, is still in the line.
        const alone = drawn("\\(x\\)\n\n- \\(y\\)");
        expect([formulas(alone), displays(alone)]).toEqual([2, 0]);
    });

    it("draws two formulas that touch as two", () => {
        for (const text of ["\\(a\\)\\(b\\)", "\\[a\\]\\[b\\]", "\\(a\\)\\[b\\]"]) {
            const html = drawn(text);
            expect(formulas(html), text).toBe(2);
            expect(html, text).not.toContain("$");
        }
    });

    it("draws an aligned block with row spacing as one display, its lines no list", () => {
        const html = drawn(
            "\\[\n\\begin{aligned}\na &= b \\\\[2pt]\n- c &= d\n\\end{aligned}\n\\]",
        );
        expect([formulas(html), displays(html)]).toEqual([1, 1]);
        expect(html).not.toContain("<li>");
    });

    it("keeps prices, shell variables and code as text by default", () => {
        const html = drawn("Costs $5 and $10 in $HOME, and `$$x$$` is code.");
        expect(formulas(html)).toBe(0);
        expect(html).toContain("Costs $5 and $10 in $HOME");
        expect(html).toContain("<code>$$x$$</code>");
    });

    it("draws a single $ with single dollars on, and nothing with math off", () => {
        expect(formulas(drawn("The rate $r$ applies."))).toBe(0);
        expect(formulas(drawn("The rate $r$ applies.", "with_single_dollars"))).toBe(1);
        const off = drawn("$$x$$ and \\(y\\)", "off");
        expect(formulas(off)).toBe(0);
        expect(off).toContain("$$x$$");
    });

    it("shows a formula KaTeX cannot read as written, and the rest of the reply with it", () => {
        const html = drawn("Before \\(\\frac{1}{\\) after, and \\(x\\).");
        expect(html).toMatch(/<span class="katex-error"[^>]*>\\frac\{1\}\{<\/span>/);
        expect(html).toContain("Before ");
        expect(html).toContain(" after, and ");
        expect(formulas(html)).toBe(1);
    });

    it("lets nothing in a formula link, load or run", () => {
        const html = drawn(
            [
                "$$\\href{javascript:alert(1)}{x}$$",
                "$$\\url{https://example.com}$$",
                "$$\\includegraphics{https://example.com/a.png}$$",
                "$$\\htmlClass{x}{y}\\htmlStyle{color:red}{z}\\htmlData{a=b}{c}\\htmlId{i}{j}$$",
                "$$\\text{<img src=x onerror=alert(1)>}<script>alert(1)</script>$$",
            ].join("\n\n"),
        );
        expect(html).not.toMatch(/<(a|img|script|iframe|object)\b/);
        // In a tag: the same words inside a formula's text are escaped text, which is fine.
        expect(html).not.toMatch(/<[^>]*\s(href|src|onerror|data-a|id)=/);
        expect(html).not.toMatch(/class="[^"]*\bx\b/);
    });

    it("keeps each formula's TeX, which copying gives", () => {
        expect(drawn("\\(x^2\\)")).toContain(
            '<annotation encoding="application/x-tex">x^2</annotation>',
        );
    });
});
